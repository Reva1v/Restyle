//! Capture-поток: STA + message loop. Владеет `IUIAutomation`, окном-слушателем
//! клипборда и элементом последнего чтения. Запросы — через канал + wake-up
//! `PostThreadMessageW`; ответ — `tokio::oneshot`.
//!
//! Цепочка захвата: UIA (TextPattern/ValuePattern) → клипборд (Ctrl+A, Ctrl+C,
//! ожидание WM_CLIPBOARDUPDATE ≤ 300 мс) → «Нет текста». Клипборд
//! восстанавливается сразу после чтения (не после вставки): отмена по Esc,
//! ошибка сети или падение не оставят пользователю чужой буфер.

pub mod clipboard;
pub mod input;
pub mod process;
pub mod text;
pub mod uia;

use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use tokio::sync::oneshot;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW, TranslateMessage, MSG,
    PM_NOREMOVE, WM_APP, WM_USER,
};

use text::{detect_line_ending, normalize_newlines, LineEnding};

const WM_APP_REQUEST: u32 = WM_APP + 10;
/// Ожидание WM_CLIPBOARDUPDATE после Ctrl+C.
const CLIPBOARD_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSource {
    Uia,
    Clipboard,
}

#[derive(Clone, Debug)]
#[allow(dead_code)] // line_ending — для вставки (фаза 5)
pub struct Captured {
    /// Текст с нормализованными `\n`.
    pub text: String,
    pub line_ending: LineEnding,
    pub source: TextSource,
    /// Можно писать назад через `ValuePattern.SetValue`.
    pub uia_writable: bool,
    /// Взято выделение (режим «только выделение»), а не всё поле.
    pub selection_only: bool,
    pub elapsed: Duration,
}

#[derive(Clone, Debug)]
pub enum CaptureError {
    NoText,
    Password,
    Failed(String),
}

pub type CaptureResult = Result<Captured, CaptureError>;

#[derive(Debug, Clone, Copy)]
pub struct CaptureOptions {
    /// Только Ctrl+C / выделение UIA, без Ctrl+A.
    pub select_only: bool,
    /// Отладка: пропустить UIA (`RESTYLE_FORCE_CLIPBOARD=1`).
    pub force_clipboard: bool,
}

/// Как вставлять результат.
#[derive(Debug, Clone, Copy)]
pub struct PasteOptions {
    /// Сначала попробовать `ValuePattern.SetValue` в элемент последнего чтения.
    pub prefer_uia: bool,
    /// Только Ctrl+V (выделение в приложении ещё активно), без Ctrl+A.
    pub select_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteMethod {
    Uia,
    Clipboard,
}

pub type PasteResult = Result<PasteMethod, String>;

/// Пауза после Ctrl+V до восстановления клипборда: приложение должно успеть
/// забрать данные (WM_PASTE обрабатывается асинхронно относительно SendInput).
const PASTE_SETTLE: Duration = Duration::from_millis(150);

enum Request {
    Capture { opts: CaptureOptions, reply: oneshot::Sender<CaptureResult> },
    Paste { text: String, opts: PasteOptions, reply: oneshot::Sender<PasteResult> },
    /// Положить текст в клипборд без вставки (undo в режиме «только выделение»).
    SetClipboard { text: String, reply: oneshot::Sender<Result<(), String>> },
}

/// Хэндл capture-потока; живёт в `tauri::State`.
#[derive(Clone)]
pub struct CaptureHandle {
    tx: Sender<Request>,
    thread_id: u32,
}

impl CaptureHandle {
    fn post(&self, req: Request) {
        let _ = self.tx.send(req);
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_APP_REQUEST, WPARAM(0), LPARAM(0));
        }
    }

    pub fn capture(&self, opts: CaptureOptions) -> oneshot::Receiver<CaptureResult> {
        let (reply, rx) = oneshot::channel();
        self.post(Request::Capture { opts, reply });
        rx
    }

    pub fn paste(&self, text: String, opts: PasteOptions) -> oneshot::Receiver<PasteResult> {
        let (reply, rx) = oneshot::channel();
        self.post(Request::Paste { text, opts, reply });
        rx
    }

    pub fn set_clipboard(&self, text: String) -> oneshot::Receiver<Result<(), String>> {
        let (reply, rx) = oneshot::channel();
        self.post(Request::SetClipboard { text, reply });
        rx
    }
}

struct Worker {
    hwnd: HWND,
    uia: Option<uia::Client>,
}

impl Worker {
    fn capture(&mut self, opts: CaptureOptions) -> CaptureResult {
        let t0 = Instant::now();
        let force_clipboard = opts.force_clipboard || std::env::var_os("RESTYLE_FORCE_CLIPBOARD").is_some();

        if !force_clipboard {
            if let Some(uia) = &self.uia {
                match uia.read(opts.select_only) {
                    Ok(Some(t)) => {
                        return Ok(Captured {
                            line_ending: detect_line_ending(&t.text),
                            text: normalize_newlines(&t.text),
                            source: TextSource::Uia,
                            uia_writable: t.writable,
                            selection_only: t.selection_only,
                            elapsed: t0.elapsed(),
                        });
                    }
                    Ok(None) => {}
                    Err(uia::UiaError::Password) => return Err(CaptureError::Password),
                    Err(uia::UiaError::Com(e)) => eprintln!("[restyle] UIA: {e} — клипборд"),
                }
            }
        }

        let raw = self.copy_via_clipboard(opts.select_only)?;
        if raw.trim().is_empty() {
            return Err(CaptureError::NoText);
        }
        Ok(Captured {
            line_ending: detect_line_ending(&raw),
            text: normalize_newlines(&raw),
            source: TextSource::Clipboard,
            uia_writable: false,
            selection_only: opts.select_only,
            elapsed: t0.elapsed(),
        })
    }

    /// Снапшот → Ctrl+A (если не select_only) → Ctrl+C → ждать обновление →
    /// прочитать → снять выделение → восстановить снапшот.
    fn copy_via_clipboard(&self, select_only: bool) -> Result<String, CaptureError> {
        let snap = clipboard::snapshot(self.hwnd).map_err(CaptureError::Failed)?;
        let seq0 = clipboard::sequence_number();
        clipboard::arm_update_flag();

        input::release_modifiers();
        if !select_only {
            input::chord(input::VK_CONTROL, input::VK_A);
        }
        input::chord(input::VK_CONTROL, input::VK_C);

        let updated = clipboard::wait_update(self.hwnd, CLIPBOARD_TIMEOUT)
            || clipboard::sequence_number() != seq0;
        let text = if updated {
            clipboard::read_text(self.hwnd).map_err(CaptureError::Failed)?.unwrap_or_default()
        } else {
            String::new()
        };
        if !select_only && !text.is_empty() {
            // Ctrl+A оставил всё выделенным: следующий символ пользователя
            // (после Esc) стёр бы текст. Right — свернуть выделение в конец.
            input::tap(input::VK_RIGHT);
        }
        if let Err(e) = clipboard::restore(self.hwnd, &snap) {
            eprintln!("[restyle] клипборд не восстановлен: {e}");
        }
        input::assert_modifiers_released();
        Ok(text)
    }

    /// Вставка: UIA `SetValue` (если просили и элемент даёт) → иначе клипборд:
    /// снапшот → текст в буфер → Ctrl+A (если не select_only) → Ctrl+V →
    /// 150 мс → восстановление снапшота.
    fn paste(&self, text: &str, opts: PasteOptions) -> PasteResult {
        if opts.prefer_uia {
            match uia::write(text) {
                Ok(true) => return Ok(PasteMethod::Uia),
                Ok(false) => eprintln!("[restyle] paste: UIA SetValue недоступен — клипборд"),
                Err(e) => eprintln!("[restyle] paste: UIA SetValue: {e:?} — клипборд"),
            }
        }
        let snap = clipboard::snapshot(self.hwnd)?;
        clipboard::set_text(self.hwnd, text)?;
        input::release_modifiers();
        if !opts.select_only {
            input::chord(input::VK_CONTROL, input::VK_A);
        }
        input::chord(input::VK_CONTROL, input::VK_V);
        std::thread::sleep(PASTE_SETTLE);
        let restored = clipboard::restore(self.hwnd, &snap);
        input::assert_modifiers_released();
        restored.map_err(|e| format!("клипборд не восстановлен: {e}"))?;
        Ok(PasteMethod::Clipboard)
    }
}

/// Запуск capture-потока. Блокирует до готовности message queue.
pub fn spawn() -> CaptureHandle {
    let (tx, rx) = mpsc::channel::<Request>();
    let (ready_tx, ready_rx) = mpsc::channel::<u32>();

    std::thread::Builder::new()
        .name("restyle-capture".into())
        .spawn(move || unsafe {
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if hr.is_err() {
                eprintln!("[restyle] CoInitializeEx: {hr}");
            }
            // Очередь сообщений потока создаётся первым Peek/GetMessage —
            // только после этого PostThreadMessageW не теряется.
            let mut msg = MSG::default();
            let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);

            let hwnd = match clipboard::create_listener_window() {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("[restyle] окно клипборда: {e}");
                    HWND::default()
                }
            };
            let uia = match uia::Client::new() {
                Ok(c) => Some(c),
                Err(e) => {
                    eprintln!("[restyle] UIA недоступен: {e}");
                    None
                }
            };
            let mut worker = Worker { hwnd, uia };
            let _ = ready_tx.send(GetCurrentThreadId());

            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_APP_REQUEST {
                    while let Ok(req) = rx.try_recv() {
                        match req {
                            Request::Capture { opts, reply } => {
                                let _ = reply.send(worker.capture(opts));
                            }
                            Request::Paste { text, opts, reply } => {
                                let _ = reply.send(worker.paste(&text, opts));
                            }
                            Request::SetClipboard { text, reply } => {
                                let _ = reply.send(clipboard::set_text(worker.hwnd, &text));
                            }
                        }
                    }
                } else {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        })
        .expect("не удалось запустить capture-поток");

    let thread_id = ready_rx.recv().expect("capture-поток не стартовал");
    CaptureHandle { tx, thread_id }
}
