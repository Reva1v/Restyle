//! Окно с фокусом и имя его процесса (для списков «только выделение»
//! и исключений скриншота). Вызывается из control-цикла в момент хоткея —
//! оверлей никогда не активируется, поэтому foreground = целевое приложение.

use windows::core::PWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
};

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

/// Видимое приложение с окном: exe + заголовок окна.
#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/bindings/")]
pub struct RunningApp {
    /// Имя exe, как его видят списки настроек (`Telegram.exe`).
    pub exe: String,
    /// Заголовок окна: у UWP-приложений все окна принадлежат
    /// `ApplicationFrameHost.exe`, и различить их можно только по нему.
    pub title: String,
}

/// Приложения с видимым окном и заголовком. Свои окна и окна-инструменты
/// пропускаем; по одному экземпляру на exe (первый попавшийся заголовок).
pub fn running_apps() -> Vec<RunningApp> {
    let mut found: Vec<RunningApp> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut found as *mut _ as isize));
    }
    found.sort_by_key(|a| a.exe.to_lowercase());
    found
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let out = &mut *(lparam.0 as *mut Vec<RunningApp>);
    if !IsWindowVisible(hwnd).as_bool() {
        return true.into();
    }
    // Окна-инструменты (панельки, трей-хосты) приложением не считаются.
    if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
        return true.into();
    }
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return true.into();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = GetWindowTextW(hwnd, &mut buf);
    let title = wide_to_string(&buf[..n as usize]);
    if title.trim().is_empty() {
        return true.into();
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == std::process::id() || pid == 0 {
        return true.into();
    }
    let Some(exe) = process_exe(pid) else { return true.into() };
    if exe.is_empty() || out.iter().any(|a| a.exe.eq_ignore_ascii_case(&exe)) {
        return true.into();
    }
    out.push(RunningApp { exe, title });
    true.into()
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
