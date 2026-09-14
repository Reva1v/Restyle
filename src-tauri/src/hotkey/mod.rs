//! Hotkey-поток: SetWindowsHookExW(WH_KEYBOARD_LL) + собственный message loop
//! (схема из HoldMix).
//!
//! Колбэк хука обязан отработать за микросекунды: только чистый автомат
//! (`combo.rs`), пара атомиков и `UnboundedSender::send`. Любая работа — в
//! других потоках, иначе Windows молча снимет хук по таймауту
//! LowLevelHooksTimeout.
//!
//! Отличия от HoldMix:
//! - несколько именованных комбинаций (основная, быстрые стили, undo);
//! - сработавшая комбинация ГЛОТАЕТСЯ (`return 1`), чтобы не улететь в
//!   приложение под оверлеем;
//! - при видимом оверлее глотаются и его клавиши: Enter/Esc/Tab/R/стрелки/цифры;
//! - `SUSPENDED` — пауза распознавания на время захвата нового хоткея в настройках.

pub mod combo;

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use tokio::sync::mpsc::UnboundedSender;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
    MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::control::ControlMsg;
use combo::ComboSet;

/// Идентификаторы комбинаций в `ComboSet`. Стили — `style:<id>`.
pub const COMBO_MAIN: &str = "main";
pub const COMBO_UNDO: &str = "undo";
/// Исправить раскладку («ghbdtn» → «привет»).
pub const COMBO_LAYOUT: &str = "layout";
pub const COMBO_STYLE_PREFIX: &str = "style:";

/// События hotkey-потока для control-цикла.
#[derive(Debug)]
pub enum HotkeyEvent {
    /// Сработала комбинация `id` (см. константы выше); `at` — момент события
    /// в колбэке хука (точка отсчёта бюджета 100 мс до оверлея).
    Combo { id: String, at: Instant },
    /// Комбинацию `id` отпустили. Для основной комбинации это развилка
    /// «короткое нажатие» / «удержание»: см. `control.rs`.
    ComboReleased { id: String },
    /// Клавиша оверлея при видимой панели (проглочена хуком): Enter/Esc/Tab/R/стрелки/1-9.
    PanelKey(u32),
}

// --- сообщения message loop hotkey-потока ---
pub const WM_APP_REINSTALL: u32 = WM_APP + 1;
pub const WM_APP_RELOAD_COMBOS: u32 = WM_APP + 2;
/// Меню трея открылось/закрылось — поставить/снять хук мыши.
pub const WM_APP_MENU: u32 = WM_APP + 3;

// --- разделяемое состояние (пишет control-цикл/настройки, читает колбэк хука) ---
static TX: OnceLock<UnboundedSender<ControlMsg>> = OnceLock::new();
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static PANEL_VISIBLE: AtomicBool = AtomicBool::new(false);
/// Меню трея видно: ловим клик мимо и Esc.
static MENU_VISIBLE: AtomicBool = AtomicBool::new(false);
/// Прямоугольник меню в пикселях экрана (x, y, w, h) — клик внутри не наш.
static MENU_RECT: std::sync::Mutex<(i32, i32, i32, i32)> = std::sync::Mutex::new((0, 0, 0, 0));
/// Пауза распознавания (окно настроек захватывает новую комбинацию).
static SUSPENDED: AtomicBool = AtomicBool::new(false);
/// GetTickCount64 последнего события клавиатурного хука — для watchdog
static LAST_HOOK_TICK: AtomicU64 = AtomicU64::new(0);
/// Хук стоит (для мастера первого запуска и диагностики).
static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
/// Комбинации (id, нормализованные VK); меняются из настроек, применяются по
/// WM_APP_RELOAD_COMBOS.
static COMBOS: Mutex<Vec<(String, Vec<u32>)>> = Mutex::new(Vec::new());

thread_local! {
    // Всё локальное состояние хука живёт на hotkey-потоке: LL-хуки вызываются
    // на потоке, который их установил (внутри его message loop).
    static SET: RefCell<ComboSet> = RefCell::new(ComboSet::default());
    static KBD_HOOK: Cell<Option<HHOOK>> = const { Cell::new(None) };
}

pub fn set_panel_visible(visible: bool) {
    PANEL_VISIBLE.store(visible, Ordering::Release);
}

pub fn set_suspended(on: bool) {
    // Единственный способ увидеть это состояние снаружи: залипший `true`
    // означает мёртвые хоткеи, поэтому переходы печатаем.
    if SUSPENDED.swap(on, Ordering::AcqRel) != on {
        println!("[restyle] хук {}", if on { "приостановлен (захват хоткея)" } else { "снова слушает" });
    }
}

pub fn set_combos(combos: Vec<(String, Vec<u32>)>) {
    *COMBOS.lock().unwrap() = combos;
    post(WM_APP_RELOAD_COMBOS);
}

/// Отправить сообщение в message loop hotkey-потока.
pub fn post(msg: u32) {
    let id = HOOK_THREAD_ID.load(Ordering::Acquire);
    if id != 0 {
        unsafe {
            let _ = PostThreadMessageW(id, msg, WPARAM(0), LPARAM(0));
        }
    }
}

/// Watchdog (вызывается из control-цикла раз в ~2 с): если был пользовательский
/// ввод в последнюю секунду, а хук молчит дольше 5 с — Windows сняла его по
/// таймауту; переустанавливаем. LASTINPUTINFO включает и мышь, поэтому редкая
/// ложная переустановка возможна — она идемпотентна и безвредна.
pub fn watchdog_check() {
    let last_hook = LAST_HOOK_TICK.load(Ordering::Relaxed);
    if last_hook == 0 {
        return; // хук ещё не получал событий с момента установки
    }
    let mut lii = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    unsafe {
        if !GetLastInputInfo(&mut lii).as_bool() {
            return;
        }
        let now = GetTickCount64();
        if hook_is_silent(now, lii.dwTime, last_hook) {
            eprintln!(
                "[restyle] keyboard hook silent {} ms with recent input - reinstalling",
                now.saturating_sub(last_hook)
            );
            post(WM_APP_REINSTALL);
        }
    }
}

/// Чистая часть watchdog: ввод был меньше секунды назад, а хук молчит дольше
/// 5 с. `last_input32` — `LASTINPUTINFO.dwTime`, он 32-битный (GetTickCount),
/// поэтому возраст ввода считаем в 32 битах с переносом: после 49,7 дня
/// аптайма прямое вычитание из `GetTickCount64` давало бы ~2^32 мс, и
/// переустановка хука никогда бы не срабатывала.
fn hook_is_silent(now: u64, last_input32: u32, last_hook: u64) -> bool {
    if last_hook == 0 {
        return false; // хук ещё не получал событий с момента установки
    }
    let input_age = (now as u32).wrapping_sub(last_input32);
    let hook_age = now.saturating_sub(last_hook);
    input_age < 1_000 && hook_age > 5_000
}

/// Клавиши, которые оверлей забирает себе, пока виден.
fn is_panel_key(vk: u32) -> bool {
    matches!(
        vk,
        0x0D /* Enter */ | 0x1B /* Esc */ | 0x09 /* Tab */ | 0x52 /* R */
        | 0x25..=0x28 /* стрелки */ | 0x31..=0x39 /* 1-9 */
    )
}

unsafe extern "system" fn kbd_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        LAST_HOOK_TICK.store(GetTickCount64(), Ordering::Relaxed);

        let msg = wparam.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        let injected = kb.flags.contains(LLKHF_INJECTED);

        // Свои же SendInput (Ctrl+A/C/V в фазе 2) не должны попадать в автомат.
        if (down || up) && !injected && !SUSPENDED.load(Ordering::Acquire) {
            let vk = kb.vkCode;
            let (hit, mods_held) = SET.with_borrow_mut(|s| {
                let hit = s.on_key_at(vk, down, kb.time as u64).map(|(id, edge)| (id.to_owned(), edge));
                (hit, s.modifiers_held())
            });
            if let Some((id, edge)) = hit {
                match edge {
                    combo::Edge::Show => {
                        if let Some(tx) = TX.get() {
                            let _ = tx.send(ControlMsg::Hotkey(HotkeyEvent::Combo {
                                id,
                                at: Instant::now(),
                            }));
                        }
                        // Комбинация — наша: приложению под оверлеем её не отдаём.
                        return LRESULT(1);
                    }
                    // Двойное нажатие модификатора: событие, но клавишу не глотаем —
                    // её нажатие приложение уже получило.
                    combo::Edge::Double => {
                        if let Some(tx) = TX.get() {
                            let _ = tx.send(ControlMsg::Hotkey(HotkeyEvent::Combo {
                                id,
                                at: Instant::now(),
                            }));
                        }
                    }
                    combo::Edge::Hide => {
                        if let Some(tx) = TX.get() {
                            let _ = tx.send(ControlMsg::Hotkey(HotkeyEvent::ComboReleased { id }));
                        }
                        // Отпускание пропускаем дальше: приложение под оверлеем
                        // уже видело down этой клавиши (его мы не глотали).
                    }
                }
            }
            // Esc закрывает меню трея; остальные клавиши ему не нужны.
            if down && vk == 0x1B && MENU_VISIBLE.load(Ordering::Acquire) {
                if let Some(tx) = TX.get() {
                    let _ = tx.send(ControlMsg::CloseTrayMenu);
                }
                return LRESULT(1);
            }
            // Клавиши оверлея глотаем (и down, и up), пока он виден — но только
            // без модификаторов: Alt+Tab, Ctrl+Tab, Ctrl+R, Win+стрелки — не наши.
            if PANEL_VISIBLE.load(Ordering::Acquire) && is_panel_key(vk) && !mods_held {
                if down {
                    if let Some(tx) = TX.get() {
                        let _ = tx.send(ControlMsg::Hotkey(HotkeyEvent::PanelKey(vk)));
                    }
                }
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

thread_local! {
    static MOUSE_HOOK: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

fn install_mouse_hook() {
    MOUSE_HOOK.with(|h| {
        if h.get() != 0 {
            return;
        }
        match unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) } {
            Ok(hook) => h.set(hook.0 as isize),
            Err(e) => eprintln!("[restyle] хук мыши не встал: {e}"),
        }
    });
}

fn uninstall_mouse_hook() {
    MOUSE_HOOK.with(|h| {
        let raw = h.get();
        if raw != 0 {
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(raw as *mut _));
            }
            h.set(0);
        }
    });
}

/// Клик мимо открытого меню закрывает его. Сам клик пропускаем дальше —
/// пользователь целился в то, что под меню, а не «в пустоту».
unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && MENU_VISIBLE.load(Ordering::Acquire) {
        let click = matches!(
            wparam.0 as u32,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN
        );
        if click {
            let pt = (*(lparam.0 as *const MSLLHOOKSTRUCT)).pt;
            let (x, y, w, h) = MENU_RECT.lock().map(|r| *r).unwrap_or((0, 0, 0, 0));
            let inside = pt.x >= x && pt.x < x + w && pt.y >= y && pt.y < y + h;
            if !inside {
                if let Some(tx) = TX.get() {
                    let _ = tx.send(ControlMsg::CloseTrayMenu);
                }
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

/// Меню трея открылось (`rect` — его геометрия) или закрылось.
pub fn set_menu_visible(on: bool, rect: (i32, i32, i32, i32)) {
    if let Ok(mut r) = MENU_RECT.lock() {
        *r = rect;
    }
    MENU_VISIBLE.store(on, Ordering::Release);
    let tid = HOOK_THREAD_ID.load(Ordering::Acquire);
    if tid != 0 {
        unsafe {
            let _ = PostThreadMessageW(tid, WM_APP_MENU, WPARAM(on as usize), LPARAM(0));
        }
    }
}

/// Установлен ли LL-хук клавиатуры (мастер первого запуска, диагностика).
pub fn is_installed() -> bool {
    HOOK_INSTALLED.load(Ordering::Acquire)
}

fn install_kbd_hook() {
    unsafe {
        match SetWindowsHookExW(WH_KEYBOARD_LL, Some(kbd_proc), None, 0) {
            Ok(h) => {
                HOOK_INSTALLED.store(true, Ordering::Release);
                KBD_HOOK.set(Some(h))
            }
            Err(e) => eprintln!("[restyle] SetWindowsHookExW(WH_KEYBOARD_LL) failed: {e}"),
        }
    }
}

fn uninstall_kbd_hook() {
    if let Some(h) = KBD_HOOK.take() {
        unsafe {
            let _ = UnhookWindowsHookEx(h);
        }
    }
}

fn reload_set() {
    let combos = COMBOS.lock().unwrap().clone();
    // Видно, какие быстрые хоткеи стилей реально попали в хук.
    println!(
        "[restyle] combos: {}",
        combos.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join(", ")
    );
    SET.with_borrow_mut(|s| *s = ComboSet::new(&combos));
}

/// Запуск hotkey-потока. `combos` — (id, VK-коды) всех комбинаций.
pub fn spawn(tx: UnboundedSender<ControlMsg>, combos: Vec<(String, Vec<u32>)>) {
    TX.set(tx).expect("hotkey::spawn вызывается один раз");
    *COMBOS.lock().unwrap() = combos;

    std::thread::Builder::new()
        .name("restyle-hotkey".into())
        .spawn(|| unsafe {
            HOOK_THREAD_ID.store(GetCurrentThreadId(), Ordering::Release);
            reload_set();
            install_kbd_hook();

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                match msg.message {
                    WM_APP_REINSTALL => {
                        uninstall_kbd_hook();
                        install_kbd_hook();
                        // после переустановки состояние клавиш неизвестно — сброс
                        reload_set();
                    }
                    WM_APP_RELOAD_COMBOS => reload_set(),
                    // Хук мыши живёт только пока открыто меню трея.
                    WM_APP_MENU => {
                        if msg.wParam.0 != 0 {
                            install_mouse_hook();
                        } else {
                            uninstall_mouse_hook();
                        }
                    }
                    _ => {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
        })
        .expect("не удалось запустить hotkey-поток");
}

#[cfg(test)]
mod watchdog_tests {
    use super::hook_is_silent;

    #[test]
    fn detects_silent_hook_after_32bit_tick_wrap() {
        // Аптайм > 49,7 дня: GetTickCount64 ушёл за 2^32, dwTime остался 32-битным.
        let now: u64 = (1u64 << 32) + 500;
        let last_input32: u32 = (now as u32).wrapping_sub(200); // ввод 200 мс назад
        let last_hook: u64 = now - 6_000; // хук молчит 6 с
        assert!(hook_is_silent(now, last_input32, last_hook));
    }

    #[test]
    fn quiet_user_or_live_hook_is_not_silent() {
        let now: u64 = 100_000;
        assert!(!hook_is_silent(now, (now as u32) - 5_000, now - 6_000)); // ввода давно нет
        assert!(!hook_is_silent(now, (now as u32) - 200, now - 300)); // хук живой
        assert!(!hook_is_silent(now, (now as u32) - 200, 0)); // событий ещё не было
    }
}
