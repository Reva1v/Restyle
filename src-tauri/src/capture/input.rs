//! SendInput-хелперы: аккорды Ctrl+A/C/V, сброс физически зажатых модификаторов
//! хоткея, маскировка «одиночного Alt» (иначе отпускание Alt после проглоченной
//! комбинации активирует меню в Блокноте/Проводнике — приём из AutoHotkey).
//!
//! Все инжектированные события помечены LLKHF_INJECTED, LL-хук их игнорирует.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY,
};

pub const VK_CONTROL: u16 = 0x11;
pub const VK_A: u16 = 0x41;
pub const VK_C: u16 = 0x43;
#[allow(dead_code)] // вставка, фаза 5
pub const VK_V: u16 = 0x56;
pub const VK_RIGHT: u16 = 0x27;

const MODIFIERS: [u16; 8] = [
    0xA0, 0xA1, // L/R Shift
    0xA2, 0xA3, // L/R Ctrl
    0xA4, 0xA5, // L/R Alt
    0x5B, 0x5C, // L/R Win
];

fn is_extended(vk: u16) -> bool {
    matches!(vk, 0xA3 | 0xA5 | 0x5B | 0x5C | 0x21..=0x2E)
}

fn key(vk: u16, up: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if is_extended(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) {
    if inputs.is_empty() {
        return;
    }
    unsafe {
        let n = SendInput(inputs, size_of::<INPUT>() as i32);
        if n as usize != inputs.len() {
            eprintln!("[restyle] SendInput: отправлено {n} из {}", inputs.len());
        }
    }
}

fn is_down(vk: u16) -> bool {
    unsafe { GetAsyncKeyState(vk as i32) < 0 }
}

/// Сразу после срабатывания комбинации: если зажат Alt или Win, инжектируем
/// Ctrl down/up, чтобы последующее отпускание Alt/Win не открыло меню окна /
/// Пуск (система считает Alt «одиночным», если между его нажатием и
/// отпусканием не было других клавиш; проглоченную хуком клавишу приложение
/// не видело).
pub fn mask_menu_activation() {
    let alt_or_win = [0xA4u16, 0xA5, 0x5B, 0x5C].iter().any(|&vk| is_down(vk));
    if alt_or_win {
        send(&[key(VK_CONTROL, false), key(VK_CONTROL, true)]);
    }
}

/// Отпустить все физически зажатые модификаторы (иначе Ctrl+A превратится в
/// Ctrl+Alt+A). Реальное отпускание пользователем позже даст лишний key-up —
/// это безвредно; «залипания» нет, т.к. итоговое состояние — все отпущены.
pub fn release_modifiers() {
    let ups: Vec<INPUT> = MODIFIERS.iter().filter(|&&vk| is_down(vk)).map(|&vk| key(vk, true)).collect();
    send(&ups);
}

/// modifier+key: down, key down, key up, up.
pub fn chord(modifier: u16, vk: u16) {
    send(&[key(modifier, false), key(vk, false), key(vk, true), key(modifier, true)]);
}

pub fn tap(vk: u16) {
    send(&[key(vk, false), key(vk, true)]);
}

/// Ни один модификатор не должен остаться зажатым после операции
/// (проверка критерия приёмки; логирует нарушителей).
pub fn assert_modifiers_released() -> bool {
    let stuck: Vec<u16> = MODIFIERS.iter().copied().filter(|&vk| is_down(vk)).collect();
    if !stuck.is_empty() {
        // Физически зажатые пользователем клавиши сюда тоже попадут — это не
        // залипание, а честное состояние; сообщение информационное.
        eprintln!("[restyle] модификаторы всё ещё зажаты: {stuck:#04X?}");
    }
    stuck.is_empty()
}
