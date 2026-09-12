//! Работа с HWND оверлея: стили, позиционирование, DPI (взято из HoldMix).
//! `place_panel` — чистая функция, покрыта тестами.
//!
//! Показ/скрытие вызываются только с главного потока Tauri
//! (`AppHandle::run_on_main_thread`) — окно принадлежит ему.

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE,
    HWND_TOPMOST, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNA, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

/// Логический размер оверлея (умножается на DPI-масштаб монитора).
/// Должен совпадать с `app.windows[overlay]` в tauri.conf.json.
pub const PANEL_W: f32 = 480.0;
pub const PANEL_H: f32 = 400.0;
/// Логический сдвиг панели от курсора.
pub const CURSOR_OFFSET: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Позиция панели у курсора: предпочтительно снизу-справа со сдвигом; если
/// вылезает за рабочую область — зеркалим на другую сторону курсора, затем
/// жёстко обрезаем по границам. Чистая функция — юнит-тесты ниже.
pub fn place_panel(cursor: (i32, i32), size: (i32, i32), work: Rect, offset: i32) -> (i32, i32) {
    let (cx, cy) = cursor;
    let (w, h) = size;

    let mut x = cx + offset;
    if x + w > work.right {
        x = cx - offset - w;
    }
    let mut y = cy + offset;
    if y + h > work.bottom {
        y = cy - offset - h;
    }

    x = x.clamp(work.left, (work.right - w).max(work.left));
    y = y.clamp(work.top, (work.bottom - h).max(work.top));
    (x, y)
}

/// Рабочая область и DPI монитора под точкой.
pub fn monitor_at(pt: (i32, i32)) -> (Rect, u32) {
    unsafe {
        let hmon = MonitorFromPoint(POINT { x: pt.0, y: pt.1 }, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if GetMonitorInfoW(hmon, &mut mi).as_bool() {
            Rect {
                left: mi.rcWork.left,
                top: mi.rcWork.top,
                right: mi.rcWork.right,
                bottom: mi.rcWork.bottom,
            }
        } else {
            Rect { left: 0, top: 0, right: 1920, bottom: 1080 }
        };
        // Per-monitor DPI v2: DPI монитора под курсором, не системный.
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() {
            dx = 96;
        }
        let _ = dy;
        (work, dx)
    }
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut core::ffi::c_void)
}

/// Расширенные стили оверлея + скруглённые углы DWM.
/// WS_EX_NOACTIVATE — панель никогда не забирает фокус (критично: вставка идёт
/// в поле, где остался курсор); WS_EX_TOOLWINDOW — нет в Alt-Tab.
pub fn apply_overlay_styles(raw: isize) {
    unsafe {
        let h = hwnd(raw);
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        SetWindowLongPtrW(
            h,
            GWL_EXSTYLE,
            ex | (WS_EX_TOOLWINDOW.0 as isize) | (WS_EX_NOACTIVATE.0 as isize),
        );
    }
    apply_rounded_corners(raw);
}

/// Скруглённые углы DWM для окон без системного титлбара
/// (`decorations: false` сам по себе на Win11 их не даёт).
/// На Win10 атрибут не поддержан — игнорируем, углы даёт CSS.
pub fn apply_rounded_corners(raw: isize) {
    unsafe {
        let h = hwnd(raw);
        let pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            h,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const core::ffi::c_void,
            size_of_val(&pref) as u32,
        );
    }
}

/// Показ без активации: SetWindowPos(SWP_NOACTIVATE) + ShowWindow(SW_SHOWNA).
pub fn show_at(raw: isize, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        let hw = hwnd(raw);
        let _ = SetWindowPos(hw, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
        let _ = ShowWindow(hw, SW_SHOWNA);
    }
}

pub fn hide(raw: isize) {
    unsafe {
        let _ = ShowWindow(hwnd(raw), SW_HIDE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect { left: 0, top: 0, right: 2560, bottom: 1400 };

    #[test]
    fn center_of_screen_offsets_bottom_right() {
        assert_eq!(place_panel((1000, 500), (480, 400), WORK, 16), (1016, 516));
    }

    #[test]
    fn near_right_edge_flips_left() {
        let (x, _) = place_panel((2500, 500), (480, 400), WORK, 16);
        assert_eq!(x, 2500 - 16 - 480);
    }

    #[test]
    fn near_bottom_edge_flips_up() {
        let (_, y) = place_panel((1000, 1350), (480, 400), WORK, 16);
        assert_eq!(y, 1350 - 16 - 400);
    }

    #[test]
    fn corner_flips_both() {
        let (x, y) = place_panel((2550, 1390), (480, 400), WORK, 16);
        assert_eq!((x, y), (2550 - 496, 1390 - 416));
    }

    #[test]
    fn top_left_cursor_stays_inside() {
        let (x, y) = place_panel((0, 0), (480, 400), WORK, 16);
        assert!(x >= WORK.left && y >= WORK.top);
        assert!(x + 480 <= WORK.right && y + 400 <= WORK.bottom);
    }

    #[test]
    fn monitor_with_negative_origin() {
        let work = Rect { left: -1920, top: 0, right: 0, bottom: 1080 };
        let (x, y) = place_panel((-100, 900), (480, 400), work, 16);
        assert!(x >= work.left && x + 480 <= work.right);
        assert!(y >= work.top && y + 400 <= work.bottom);
    }

    #[test]
    fn panel_larger_than_work_area_clamps_to_origin() {
        let work = Rect { left: 0, top: 0, right: 300, bottom: 300 };
        let (x, y) = place_panel((150, 150), (480, 400), work, 16);
        assert_eq!((x, y), (0, 0));
    }
}
