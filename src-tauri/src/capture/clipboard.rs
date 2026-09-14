//! Клипборд: снапшот/восстановление всех HGLOBAL-форматов, чтение/запись текста,
//! ожидание `WM_CLIPBOARDUPDATE` через `AddClipboardFormatListener`.
//!
//! Все функции вызываются только с capture-потока — окна-слушателя (`HWND_MESSAGE`)
//! и его message loop. `wait_update` крутит вложенный цикл сообщений только для
//! этого окна (thread-сообщения с запросами не трогает).

use std::cell::Cell;
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, EnumClipboardFormats,
    GetClipboardData, GetClipboardSequenceNumber, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW,
    RegisterClassW, TranslateMessage, HWND_MESSAGE, MSG, PM_REMOVE, QS_ALLINPUT, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CLIPBOARDUPDATE, WNDCLASSW,
};

use super::text::{is_snapshot_format, MAX_FORMAT_BYTES};

const CF_UNICODETEXT: u32 = 13;

thread_local! {
    static CLIP_UPDATED: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_CLIPBOARDUPDATE {
        CLIP_UPDATED.set(true);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Скрытое message-only окно + подписка на изменения клипборда.
pub fn create_listener_window() -> Result<HWND, String> {
    unsafe {
        let hinst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = w!("RestyleClipboardListener");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        // 0 при повторной регистрации в том же процессе — не ошибка
        let _ = RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            hinst,
            None,
        )
        .map_err(|e| e.to_string())?;
        AddClipboardFormatListener(hwnd).map_err(|e| e.to_string())?;
        Ok(hwnd)
    }
}

/// Сбросить флаг перед действием, которое должно изменить клипборд. Сначала
/// выгребаем уже лежащие в очереди `WM_CLIPBOARDUPDATE`: после вставки
/// (`set_text` + `restore`) они ждут в очереди окна-слушателя, и если запрос
/// захвата встал в очередь во время паузы вставки, `wait_update` вернул бы
/// «обновился» до реального Ctrl+C и прочитал бы прежний буфер.
pub fn arm_update_flag(hwnd: HWND) {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, hwnd, WM_CLIPBOARDUPDATE, WM_CLIPBOARDUPDATE, PM_REMOVE).as_bool() {}
    }
    CLIP_UPDATED.set(false);
}

/// Ждать `WM_CLIPBOARDUPDATE` до дедлайна, прокачивая сообщения окна-слушателя.
pub fn wait_update(hwnd: HWND, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    unsafe {
        loop {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, hwnd, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if CLIP_UPDATED.get() {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            let _ = MsgWaitForMultipleObjects(None, false, left.as_millis() as u32, QS_ALLINPUT);
        }
    }
}

pub fn sequence_number() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

/// OpenClipboard с повторами: другой процесс может держать его пару мс.
fn open(hwnd: HWND) -> Result<Guard, String> {
    for _ in 0..25 {
        if unsafe { OpenClipboard(hwnd) }.is_ok() {
            return Ok(Guard);
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    Err("клипборд занят другим приложением".into())
}

struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// Снимок содержимого: все HGLOBAL-форматы (см. `is_snapshot_format`).
#[derive(Debug, Default)]
pub struct Snapshot {
    formats: Vec<(u32, Vec<u8>)>,
}

#[allow(dead_code)]
impl Snapshot {
    pub fn is_empty(&self) -> bool {
        self.formats.is_empty()
    }
    pub fn format_count(&self) -> usize {
        self.formats.len()
    }
}

pub fn snapshot(hwnd: HWND) -> Result<Snapshot, String> {
    let _g = open(hwnd)?;
    let mut snap = Snapshot::default();
    unsafe {
        let mut fmt = 0u32;
        loop {
            fmt = EnumClipboardFormats(fmt);
            if fmt == 0 {
                break;
            }
            if !is_snapshot_format(fmt) {
                continue;
            }
            let Ok(h) = GetClipboardData(fmt) else { continue };
            if h.0.is_null() {
                continue;
            }
            let hg = HGLOBAL(h.0);
            let size = GlobalSize(hg);
            if size == 0 || size > MAX_FORMAT_BYTES {
                continue;
            }
            let p = GlobalLock(hg);
            if p.is_null() {
                continue;
            }
            let bytes = std::slice::from_raw_parts(p as *const u8, size).to_vec();
            let _ = GlobalUnlock(hg);
            snap.formats.push((fmt, bytes));
        }
    }
    Ok(snap)
}

fn alloc_global(bytes: &[u8]) -> Result<HGLOBAL, String> {
    unsafe {
        let hg = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).map_err(|e| e.to_string())?;
        let p = GlobalLock(hg);
        if p.is_null() {
            let _ = GlobalFree(hg);
            return Err("GlobalLock".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
        let _ = GlobalUnlock(hg);
        Ok(hg)
    }
}

/// Вернуть клипборд в состояние снапшота (пустой снапшот → пустой клипборд).
pub fn restore(hwnd: HWND, snap: &Snapshot) -> Result<(), String> {
    let _g = open(hwnd)?;
    unsafe {
        EmptyClipboard().map_err(|e| e.to_string())?;
        for (fmt, bytes) in &snap.formats {
            let hg = alloc_global(bytes)?;
            if SetClipboardData(*fmt, HANDLE(hg.0)).is_err() {
                let _ = GlobalFree(hg);
            }
        }
    }
    Ok(())
}

/// CF_UNICODETEXT, если есть.
pub fn read_text(hwnd: HWND) -> Result<Option<String>, String> {
    let _g = open(hwnd)?;
    unsafe {
        let Ok(h) = GetClipboardData(CF_UNICODETEXT) else { return Ok(None) };
        if h.0.is_null() {
            return Ok(None);
        }
        let hg = HGLOBAL(h.0);
        let p = GlobalLock(hg) as *const u16;
        if p.is_null() {
            return Ok(None);
        }
        let n = GlobalSize(hg) / 2;
        let s = super::text::wide_to_string(std::slice::from_raw_parts(p, n));
        let _ = GlobalUnlock(hg);
        Ok(Some(s))
    }
}

/// Положить текст (для вставки результата, фаза 5).
#[allow(dead_code)]
pub fn set_text(hwnd: HWND, text: &str) -> Result<(), String> {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
    let _g = open(hwnd)?;
    unsafe {
        EmptyClipboard().map_err(|e| e.to_string())?;
        let hg = alloc_global(bytes)?;
        if let Err(e) = SetClipboardData(CF_UNICODETEXT, HANDLE(hg.0)) {
            let _ = GlobalFree(hg);
            return Err(e.to_string());
        }
    }
    Ok(())
}
