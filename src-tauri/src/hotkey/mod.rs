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
    WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::control::ControlMsg;
use combo::ComboSet;

/// Идентификаторы комбинаций в `ComboSet`. Стили — `style:<id>`.
pub const COMBO_MAIN: &str = "main";
pub const COMBO_UNDO: &str = "undo";
pub const COMBO_STYLE_PREFIX: &str = "style:";

/// События hotkey-потока для control-цикла.
#[derive(Debug)]
pub enum HotkeyEvent {
    /// Сработала комбинация `id` (см. константы выше); `at` — момент события
    /// в колбэке хука (точка отсчёта бюджета 100 мс до оверлея).
    Combo { id: String, at: Instant },
    /// Клавиша оверлея при видимой панели (проглочена хуком): Enter/Esc/Tab/R/стрелки/1-9.
    PanelKey(u32),
}

// --- сообщения message loop hotkey-потока ---
pub const WM_APP_REINSTALL: u32 = WM_APP + 1;
pub const WM_APP_RELOAD_COMBOS: u32 = WM_APP + 2;

// --- разделяемое состояние (пишет control-цикл/настройки, читает колбэк хука) ---
static TX: OnceLock<UnboundedSender<ControlMsg>> = OnceLock::new();
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static PANEL_VISIBLE: AtomicBool = AtomicBool::new(false);
/// Пауза распознавания (окно настроек захватывает новую комбинацию).
static SUSPENDED: AtomicBool = AtomicBool::new(false);
/// GetTickCount64 последнего события клавиатурного хука — для watchdog
static LAST_HOOK_TICK: AtomicU64 = AtomicU64::new(0);
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
    SUSPENDED.store(on, Ordering::Release);
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
        let input_age = now.saturating_sub(lii.dwTime as u64);
        let hook_age = now.saturating_sub(last_hook);
        if input_age < 1_000 && hook_age > 5_000 {
            eprintln!(
                "[restyle] keyboard hook silent {hook_age} ms with recent input - reinstalling"
            );
            post(WM_APP_REINSTALL);
        }
    }
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
            let fired = SET.with_borrow_mut(|s| s.on_key(vk, down).map(str::to_owned));
            if let Some(id) = fired {
                if let Some(tx) = TX.get() {
                    let _ = tx.send(ControlMsg::Hotkey(HotkeyEvent::Combo {
                        id,
                        at: Instant::now(),
                    }));
                }
                // Комбинация — наша: приложению под оверлеем её не отдаём.
                return LRESULT(1);
            }
            // Клавиши оверлея глотаем (и down, и up), пока он виден.
            if PANEL_VISIBLE.load(Ordering::Acquire) && is_panel_key(vk) {
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

fn install_kbd_hook() {
    unsafe {
        match SetWindowsHookExW(WH_KEYBOARD_LL, Some(kbd_proc), None, 0) {
            Ok(h) => KBD_HOOK.set(Some(h)),
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
                    _ => {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
        })
        .expect("не удалось запустить hotkey-поток");
}
