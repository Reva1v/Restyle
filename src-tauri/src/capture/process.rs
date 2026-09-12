//! Окно с фокусом и имя его процесса (для списков «только выделение»
//! и исключений скриншота). Вызывается из control-цикла в момент хоткея —
//! оверлей никогда не активируется, поэтому foreground = целевое приложение.

use windows::core::PWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::text::wide_to_string;

#[derive(Clone, Debug, Default)]
#[allow(dead_code)] // hwnd/pid — проверка окна перед вставкой (фаза 5)
pub struct Foreground {
    pub hwnd: isize,
    pub pid: u32,
    /// Имя exe (например `Telegram.exe`); пусто, если не удалось определить.
    pub exe: String,
}

pub fn foreground() -> Foreground {
    unsafe {
        let h = GetForegroundWindow();
        if h.0.is_null() {
            return Foreground::default();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        Foreground { hwnd: h.0 as isize, pid, exe: process_exe(pid).unwrap_or_default() }
    }
}

fn process_exe(pid: u32) -> Option<String> {
    unsafe {
        let hp = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let r = QueryFullProcessImageNameW(hp, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n);
        let _ = CloseHandle(hp);
        r.ok()?;
        let path = wide_to_string(&buf[..n as usize]);
        path.rsplit(['\\', '/']).next().map(str::to_string)
    }
}
