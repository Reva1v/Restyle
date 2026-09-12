//! Control-цикл: state-машина оверлея (hidden / visible / toast), захват текста,
//! позиционирование, эмит событий во фронтенд, watchdog хука. Единственная
//! точка, которая решает «показать/спрятать». Tokio-таск на рантайме Tauri.
//!
//! Поток хоткея: Combo → маска Alt → foreground-процесс → запрос capture-потоку
//! → ждём до `CAPTURE_SOFT_DEADLINE` → показ оверлея (текст уже есть или придёт
//! `overlay:text`). Нет текста → тост «Нет текста», без оверлея.
//!
//! Все операции с HWND уходят на главный поток Tauri через
//! `run_on_main_thread` — окно принадлежит ему.

use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::capture::text::{apply_line_ending, matches_process};
use crate::capture::{self, process::Foreground, CaptureError, CaptureHandle, CaptureOptions, CaptureResult, Captured, PasteMethod, PasteOptions, PasteResult, TextSource};
use crate::history::{self, History, HistoryEntry, HistoryItem};
use crate::hotkey::{self, HotkeyEvent, COMBO_MAIN, COMBO_STYLE_PREFIX, COMBO_UNDO};
use crate::ai::{self, AiError, RewriteRequest};
use crate::ipc::{RewriteChunk, RewriteDone, RewriteError, RewriteStart, ScreenshotPayload, ShowPayload, TextPayload};
use crate::screenshot::{self, Encoded, Mode};
use crate::secrets;
use crate::settings::Settings;
use crate::window::{self, CURSOR_OFFSET, PANEL_H, PANEL_W};

/// Период watchdog хука.
const TICK: Duration = Duration::from_secs(2);
/// Сколько ждём захват текста до показа оверлея (UIA обычно < 30 мс,
/// клипборд — до 300 мс); дольше — показываем «Читаю текст…».
const CAPTURE_SOFT_DEADLINE: Duration = Duration::from_millis(250);
/// Сырой кадр обязан быть снят до показа оверлея (иначе попадёт в кадр);
/// дольше этого не ждём — показываем без скриншота.
const SCREENSHOT_DEADLINE: Duration = Duration::from_millis(150);
/// Пауза после скрытия видимого оверлея перед снимком (перерисовка DWM).
const HIDE_SETTLE: Duration = Duration::from_millis(40);
const TOAST_DURATION: Duration = Duration::from_millis(1400);

const VK_ESCAPE: u32 = 0x1B;

/// Сообщения control-цикла: от hotkey-потока, tauri-команд и собственных тасков.
#[derive(Debug)]
pub enum ControlMsg {
    Hotkey(HotkeyEvent),
    /// Фронтенд просит спрятать оверлей (крестик/клик по «Отмена»).
    Hide,
    /// ack фронтенда после первого отрисованного кадра — замер задержки.
    FrontendShown,
    /// Фронтенд оверлея загрузился и подписался: повторить `overlay:show`, если видим.
    FrontendReady,
    /// Поздний результат захвата (после мягкого дедлайна); `gen` — поколение сессии.
    Captured { gen: u64, result: CaptureResult },
    /// Скрыть тост, если он всё ещё того же поколения.
    ToastExpired(u64),
    /// JPEG готов (или кодирование упало).
    ScreenshotReady { gen: u64, result: Result<Encoded, String> },
    /// Фронтенд выбрал стиль (кнопка/Tab/цифры) — запустить генерацию.
    SelectStyle(String),
    /// `R` — сгенерировать заново тем же стилем.
    Regenerate,
    /// Генерация завершилась (успех или ошибка) — снять хэндл.
    Generated { gen: u64, result: Result<String, AiError> },
    /// Enter в оверлее: вставить результат (или дождаться его и вставить).
    Paste,
    /// Хоткей/трей: вернуть предыдущий текст (повторно — снова результат).
    Undo,
    /// Вставка завершилась (capture-поток).
    Pasted { gen: u64, result: PasteResult, entry: Box<HistoryEntry> },
    /// Undo-вставка завершилась.
    Undone { result: PasteResult, restored_original: bool },
    /// Настройка «история на диск» изменилась.
    SetHistoryPersist(bool),
    /// Окно настроек просит список последних переписываний.
    GetHistory(tokio::sync::oneshot::Sender<Vec<HistoryItem>>),
    /// Трей/настройки: положить результат записи в буфер (0 — самая свежая).
    CopyHistory(usize),
    /// Стереть историю (в памяти и на диске).
    ClearHistory,
    /// Трей собран — наполнить подменю тем, что уже прочитано с диска.
    RefreshTray,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelState {
    Hidden,
    Visible,
    Toast,
}

/// Состояние скриншота в сессии.
#[derive(Clone, Debug)]
pub enum Shot {
    /// Выключен настройкой / процесс в исключениях / захват не удался.
    None(&'static str),
    /// Сырой кадр снят, идёт ресайз+JPEG.
    Encoding,
    Ready(Encoded),
}

impl Shot {
    fn label(&self) -> &'static str {
        match self {
            Shot::None(why) => why,
            Shot::Encoding => "pending",
            Shot::Ready(_) => "ready",
        }
    }
}

/// Текущий цикл переписывания.
#[allow(dead_code)] // select_only/target нужны вставке (фаза 5)
struct Session {
    target: Foreground,
    select_only: bool,
    style_id: Option<String>,
    captured: Option<Captured>,
    shot: Shot,
    started: Instant,
    /// Стиль, который надо сгенерировать (ждёт текст и скриншот).
    wanted_style: Option<String>,
    /// Текущая генерация: abort() = реальный обрыв HTTP-стрима.
    generation: Option<tokio::task::JoinHandle<()>>,
    /// Последний полный результат.
    result: Option<String>,
    /// Enter нажат до конца генерации — вставить, как только результат придёт.
    paste_on_done: bool,
    /// Быстрый стиль + настройка autoPaste: вставить без Enter.
    auto_paste: bool,
}

struct Control {
    app: AppHandle,
    tx: UnboundedSender<ControlMsg>,
    capture: CaptureHandle,
    hwnd: isize,
    state: PanelState,
    show_started: Option<Instant>,
    last_show: Option<ShowPayload>,
    /// Растёт на каждый показ; отсекает устаревшие Captured/ToastExpired.
    gen: u64,
    session: Option<Session>,
    history: History,
    /// Undo уже идёт — второй не запускаем.
    undo_in_flight: bool,
}

impl Control {
    fn place(&self) -> (i32, i32, i32, i32, f32) {
        let cursor = window::cursor_pos();
        let (work, dpi) = window::monitor_at(cursor);
        let scale = dpi as f32 / 96.0;
        let w = (PANEL_W * scale).round() as i32;
        let h = (PANEL_H * scale).round() as i32;
        let offset = (CURSOR_OFFSET * scale).round() as i32;
        let (x, y) = window::place_panel(cursor, (w, h), work, offset);
        (x, y, w, h, scale)
    }

    fn show_window(&mut self, at: Instant, payload: ShowPayload, x: i32, y: i32, w: i32, h: i32) {
        let hwnd = self.hwnd;
        let _ = self.app.run_on_main_thread(move || window::show_at(hwnd, x, y, w, h));
        hotkey::set_panel_visible(payload.toast.is_none());
        self.state = if payload.toast.is_some() { PanelState::Toast } else { PanelState::Visible };
        self.show_started = Some(at);
        let _ = self.app.emit("overlay:show", &payload);
        self.last_show = Some(payload);
    }

    fn show_overlay(&mut self, at: Instant, capturing: bool) {
        let (x, y, w, h, scale) = self.place();
        let s = self.session.as_ref();
        let payload = ShowPayload {
            gen: self.gen,
            x,
            y,
            dpi_scale: scale,
            style_id: s.and_then(|s| s.style_id.clone()),
            toast: None,
            capturing,
            target_exe: s.map(|s| s.target.exe.clone()).unwrap_or_default(),
            screenshot: s.map(|s| s.shot.label()).unwrap_or("off").to_string(),
        };
        self.show_window(at, payload, x, y, w, h);
    }

    fn show_toast(&mut self, text: &str) {
        self.gen += 1;
        self.session = None;
        let (x, y, w, _h, scale) = self.place();
        // Тост — узкая полоска у курсора.
        let th = (56.0 * scale).round() as i32;
        let payload = ShowPayload {
            gen: self.gen,
            x,
            y,
            dpi_scale: scale,
            style_id: None,
            toast: Some(text.to_string()),
            capturing: false,
            target_exe: String::new(),
            screenshot: "off".into(),
        };
        self.show_window(Instant::now(), payload, x, y, w, th);
        let tx = self.tx.clone();
        let gen = self.gen;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(TOAST_DURATION).await;
            let _ = tx.send(ControlMsg::ToastExpired(gen));
        });
    }

    /// Оборвать текущую генерацию (drop future → reqwest закрывает стрим).
    fn abort_generation(&mut self) {
        if let Some(h) = self.session.as_mut().and_then(|s| s.generation.take()) {
            if !h.is_finished() {
                println!("[restyle] generation aborted");
            }
            h.abort();
        }
    }

    fn hide(&mut self) {
        self.abort_generation();
        let hwnd = self.hwnd;
        let _ = self.app.run_on_main_thread(move || window::hide(hwnd));
        hotkey::set_panel_visible(false);
        self.state = PanelState::Hidden;
        self.session = None;
        self.gen += 1;
        let _ = self.app.emit("overlay:hide", ());
    }

    /// Перестроить подменю трея «Последние переписывания». Пункты меню сами
    /// прыгают в главный поток и ждут его, поэтому уводим вызов в blocking-таск.
    fn refresh_tray_history(&self) {
        let app = self.app.clone();
        let items = self.history.items();
        tauri::async_runtime::spawn_blocking(move || crate::tray::refresh_history(&app, &items));
    }

    /// Запустить генерацию, если есть стиль, текст и решён скриншот.
    fn maybe_start_generation(&mut self) {
        let Some(s) = &self.session else { return };
        let Some(style_id) = s.wanted_style.clone() else { return };
        let Some(captured) = &s.captured else { return };
        if matches!(s.shot, Shot::Encoding) {
            return; // ScreenshotReady вызовет нас снова
        }
        let settings = Settings::load(&self.app);
        let Some(style) = settings.styles.iter().find(|st| st.id == style_id).cloned() else {
            let _ = self.app.emit(
                "rewrite:error",
                RewriteError { gen: self.gen, message: format!("Стиль «{style_id}» не найден") },
            );
            return;
        };
        let screenshot = match &s.shot {
            Shot::Ready(e) => Some(e.jpeg_base64.clone()),
            _ => None,
        };
        let req = RewriteRequest {
            system_prompt: ai::system_prompt(self.app.path().app_config_dir().ok()),
            style_name: style.name.clone(),
            style_instruction: style.instruction.clone(),
            text: captured.text.clone(),
            screenshot_jpeg_base64: screenshot,
            model: settings.model.clone(),
        };

        self.abort_generation();
        let gen = self.gen;
        let with_screenshot = req.screenshot_jpeg_base64.is_some();
        let _ = self.app.emit("rewrite:start", RewriteStart { gen, style_id: style.id.clone(), with_screenshot });
        println!(
            "[restyle] rewrite start: style={} model={} screenshot={with_screenshot} chars={}",
            style.id,
            req.model,
            req.text.chars().count()
        );

        let app = self.app.clone();
        let tx = self.tx.clone();
        let handle = tokio::spawn(async move {
            let t0 = Instant::now();
            let key_result = match tokio::task::spawn_blocking(secrets::get_api_key).await {
                Ok(r) => r,
                Err(e) => Err(e.to_string()),
            };
            let key = match key_result {
                Ok(Some(k)) => k,
                Ok(None) => {
                    let _ = app.emit("rewrite:error", RewriteError { gen, message: AiError::NoApiKey.user_message() });
                    let _ = tx.send(ControlMsg::Generated { gen, result: Err(AiError::NoApiKey) });
                    return;
                }
                Err(e) => {
                    let err = AiError::Other(e);
                    let _ = app.emit("rewrite:error", RewriteError { gen, message: err.user_message() });
                    let _ = tx.send(ControlMsg::Generated { gen, result: Err(err) });
                    return;
                }
            };
            let provider = ai::provider_for(&req.model);
            let (ctx, mut crx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let app2 = app.clone();
            let first_chunk = std::sync::Arc::new(std::sync::Mutex::new(None::<f32>));
            let fc = first_chunk.clone();
            let forwarder = tokio::spawn(async move {
                while let Some(text) = crx.recv().await {
                    let mut g = fc.lock().unwrap();
                    if g.is_none() {
                        let ms = t0.elapsed().as_secs_f32() * 1000.0;
                        *g = Some(ms);
                        println!("[restyle] first chunk in {ms:.0} ms");
                    }
                    drop(g);
                    let _ = app2.emit("rewrite:chunk", RewriteChunk { gen, text });
                }
            });
            let result = provider.rewrite(&key, &req, ctx).await;
            let _ = forwarder.await;
            let elapsed_ms = t0.elapsed().as_secs_f32() * 1000.0;
            match &result {
                Ok(text) => {
                    let first = first_chunk.lock().unwrap().unwrap_or(elapsed_ms);
                    println!("[restyle] rewrite done in {elapsed_ms:.0} ms, {} chars", text.chars().count());
                    let _ = app.emit(
                        "rewrite:done",
                        RewriteDone { gen, text: text.clone(), elapsed_ms, first_chunk_ms: first },
                    );
                }
                Err(e) => {
                    eprintln!("[restyle] rewrite error: {e:?}");
                    let _ = app.emit("rewrite:error", RewriteError { gen, message: e.user_message() });
                }
            }
            let _ = tx.send(ControlMsg::Generated { gen, result });
        });
        if let Some(s) = &mut self.session {
            s.generation = Some(handle);
            s.result = None;
        }
    }

    /// Вставить результат текущей сессии в целевое окно. Оверлей прячется
    /// сразу; сама вставка — на capture-потоке, итог придёт `Pasted`.
    fn do_paste(&mut self) {
        let Some(s) = &self.session else { return };
        let (Some(captured), Some(result)) = (&s.captured, &s.result) else {
            // результата ещё нет — вставим по готовности
            if let Some(s) = &mut self.session {
                s.paste_on_done = true;
            }
            let _ = self.app.emit("rewrite:pending_paste", self.gen);
            return;
        };
        // Всё нужное — во владение, чтобы отпустить заём сессии.
        let result_empty = result.trim().is_empty();
        let target_hwnd = s.target.hwnd;
        let target_exe = s.target.exe.clone();
        let text = apply_line_ending(result, captured.line_ending);
        let opts = PasteOptions {
            prefer_uia: captured.uia_writable && !captured.selection_only,
            select_only: captured.selection_only,
        };
        let entry = HistoryEntry {
            at: history::now_ms(),
            target_exe: target_exe.clone(),
            target_hwnd,
            style_id: s.wanted_style.clone().unwrap_or_default(),
            original: captured.text.clone(),
            result: result.clone(),
            line_ending: history::line_ending_to_str(captured.line_ending).into(),
            select_only: captured.selection_only,
            via_uia: false,
            showing_original: false,
        };
        let chars = result.chars().count();

        if result_empty {
            self.show_toast("Пустой результат — нечего вставлять");
            return;
        }
        let fg = capture::process::foreground();
        if fg.hwnd != target_hwnd {
            eprintln!("[restyle] paste: фокус ушёл из {target_exe} в {}", fg.exe);
            self.hide();
            self.show_toast("Окно сменилось — вставка отменена");
            return;
        }
        let gen = self.gen;
        println!("[restyle] paste: {chars} chars via {}", if opts.prefer_uia { "uia?" } else { "clipboard" });
        // Прячем оверлей до вставки: он без фокуса, но лишний кадр не нужен.
        let hwnd = self.hwnd;
        let _ = self.app.run_on_main_thread(move || window::hide(hwnd));
        hotkey::set_panel_visible(false);
        self.state = PanelState::Hidden;
        let _ = self.app.emit("overlay:hide", ());
        let rx = self.capture.paste(text, opts);
        let tx = self.tx.clone();
        tauri::async_runtime::spawn(async move {
            let result = rx.await.unwrap_or_else(|_| Err("capture-поток завершился".into()));
            let _ = tx.send(ControlMsg::Pasted { gen, result, entry: Box::new(entry) });
        });
    }

    /// Undo: последняя запись истории → вернуть исходник (или результат, если
    /// исходник уже стоит). Окно должно быть тем же; select-only — только буфер.
    fn do_undo(&mut self) {
        if self.undo_in_flight {
            return;
        }
        let Some(e) = self.history.last_mut().cloned() else {
            self.show_toast("Нечего возвращать");
            return;
        };
        let restore_original = !e.showing_original;
        let text = if restore_original { e.original.clone() } else { e.result.clone() };
        let le = history::line_ending_from_str(&e.line_ending);
        let text = apply_line_ending(&text, le);
        let fg = capture::process::foreground();
        if e.select_only {
            // Выделение уже не восстановить — отдаём текст в буфер.
            let rx = self.capture.set_clipboard(text);
            let tx = self.tx.clone();
            tauri::async_runtime::spawn(async move {
                let result = rx.await.unwrap_or_else(|_| Err("capture-поток завершился".into()));
                let _ = tx.send(ControlMsg::Undone { result: result.map(|_| PasteMethod::Clipboard), restored_original: false });
            });
            self.show_toast(if restore_original { "Исходный текст скопирован в буфер" } else { "Результат скопирован в буфер" });
            return;
        }
        if fg.hwnd != e.target_hwnd {
            self.show_toast(&format!("Окно {} не активно — undo отменён", e.target_exe));
            return;
        }
        let opts = PasteOptions { prefer_uia: e.via_uia, select_only: false };
        self.undo_in_flight = true;
        if self.state != PanelState::Hidden {
            self.hide();
        }
        let rx = self.capture.paste(text, opts);
        let tx = self.tx.clone();
        tauri::async_runtime::spawn(async move {
            let result = rx.await.unwrap_or_else(|_| Err("capture-поток завершился".into()));
            let _ = tx.send(ControlMsg::Undone { result, restored_original: restore_original });
        });
    }

    fn emit_text(&self, c: &Captured) {
        let _ = self.app.emit(
            "overlay:text",
            TextPayload {
                text: c.text.clone(),
                source: match c.source {
                    TextSource::Uia => "uia".into(),
                    TextSource::Clipboard => "clipboard".into(),
                },
                selection_only: c.selection_only,
                uia_writable: c.uia_writable,
                elapsed_ms: c.elapsed.as_secs_f32() * 1000.0,
            },
        );
    }

    fn emit_screenshot(&self, e: &Encoded) {
        let _ = self.app.emit(
            "overlay:screenshot",
            ScreenshotPayload {
                width: e.width,
                height: e.height,
                bytes: e.bytes as u32,
                capture_ms: e.capture_ms,
                encode_ms: e.encode_ms,
            },
        );
    }

    /// Снимок экрана для новой сессии: решает по настройкам, снимает сырой
    /// кадр (blocking-таск, ждём до SCREENSHOT_DEADLINE), кодирование уходит
    /// в фон и вернётся `ScreenshotReady`.
    async fn take_screenshot(&mut self, settings: &Settings, target: &Foreground, gen: u64) -> Shot {
        if !settings.screenshot_enabled {
            return Shot::None("off");
        }
        if matches_process(&settings.screenshot_excluded_processes, &target.exe) {
            println!("[restyle] screenshot: {} в исключениях", target.exe);
            return Shot::None("excluded");
        }
        let mode = Mode::from_setting(&settings.screenshot_mode);
        let hwnd = target.hwnd;
        let cursor = window::cursor_pos();
        let job = tokio::task::spawn_blocking(move || screenshot::capture(mode, hwnd, cursor));
        let raw = match tokio::time::timeout(SCREENSHOT_DEADLINE, job).await {
            Ok(Ok(Ok(raw))) => raw,
            Ok(Ok(Err(e))) => {
                eprintln!("[restyle] screenshot: {e} — без скриншота");
                return Shot::None("failed");
            }
            Ok(Err(e)) => {
                eprintln!("[restyle] screenshot task: {e}");
                return Shot::None("failed");
            }
            Err(_) => {
                eprintln!("[restyle] screenshot: дольше {SCREENSHOT_DEADLINE:?} — без скриншота");
                return Shot::None("failed");
            }
        };
        println!("[restyle] screenshot captured in {:.1} ms", raw.capture.as_secs_f64() * 1000.0);
        let tx = self.tx.clone();
        tauri::async_runtime::spawn(async move {
            let result = match tokio::task::spawn_blocking(move || screenshot::encode(raw)).await {
                Ok(r) => r,
                Err(e) => Err(format!("encode task: {e}")),
            };
            let _ = tx.send(ControlMsg::ScreenshotReady { gen, result });
        });
        Shot::Encoding
    }

    /// Результат захвата (ранний или поздний) в контексте текущей сессии.
    fn apply_capture(&mut self, at: Instant, result: CaptureResult, already_visible: bool) {
        match result {
            Ok(c) => {
                println!(
                    "[restyle] captured {} chars via {:?} in {:.1} ms (select_only={}, writable={})",
                    c.text.chars().count(),
                    c.source,
                    c.elapsed.as_secs_f64() * 1000.0,
                    c.selection_only,
                    c.uia_writable
                );
                if !already_visible {
                    self.show_overlay(at, false);
                }
                self.emit_text(&c);
                if let Some(s) = &mut self.session {
                    s.captured = Some(c);
                }
                self.maybe_start_generation();
            }
            Err(e) => {
                let msg = match e {
                    CaptureError::NoText => "Нет текста",
                    CaptureError::Password => "Поле пароля — не читаю",
                    CaptureError::Failed(ref m) => {
                        eprintln!("[restyle] захват не удался: {m}");
                        "Не удалось прочитать текст"
                    }
                };
                if already_visible {
                    self.hide();
                }
                self.show_toast(msg);
            }
        }
    }

    /// Хоткей: новая сессия. Захват стартует сразу; оверлей — после захвата
    /// либо по мягкому дедлайну.
    async fn start(&mut self, at: Instant, style_id: Option<String>) {
        // Проглоченная клавиша + отпускание Alt = меню окна; маскируем сразу.
        capture::input::mask_menu_activation();

        let settings = Settings::load(&self.app);
        let target = capture::process::foreground();
        let select_only = matches_process(&settings.select_only_processes, &target.exe);
        println!("[restyle] hotkey in {} (select_only={select_only})", target.exe);

        // Повторный хоткей при видимом оверлее: спрятать до снимка, иначе
        // оверлей попадёт в кадр.
        if self.state != PanelState::Hidden {
            self.hide();
            tokio::time::sleep(HIDE_SETTLE).await;
        }

        self.gen += 1;
        let gen = self.gen;
        let quick = style_id.is_some();

        // Текст и экран — параллельно: захват текста уже идёт на capture-потоке,
        // пока здесь снимается кадр.
        let mut rx = self.capture.capture(CaptureOptions { select_only, force_clipboard: false });
        let shot = self.take_screenshot(&settings, &target, gen).await;

        self.session = Some(Session {
            target,
            select_only,
            wanted_style: style_id.clone(), // быстрый стиль: генерация стартует сама
            style_id,
            captured: None,
            shot,
            started: at,
            generation: None,
            result: None,
            paste_on_done: false,
            auto_paste: quick && settings.auto_paste,
        });
        // `&mut rx`: по таймауту receiver остаётся у нас и уходит в фоновый таск.
        match tokio::time::timeout(CAPTURE_SOFT_DEADLINE, &mut rx).await {
            Ok(Ok(result)) => self.apply_capture(at, result, false),
            Ok(Err(_)) => self.show_toast("Захват недоступен"),
            Err(_elapsed) => {
                // Долгий захват (UIA в тяжёлом приложении, клипборд): показываем
                // оверлей с «Читаю текст…», результат догонит через Captured.
                self.show_overlay(at, true);
                let tx = self.tx.clone();
                tauri::async_runtime::spawn(async move {
                    let result = rx
                        .await
                        .unwrap_or_else(|_| Err(CaptureError::Failed("capture-поток завершился".into())));
                    let _ = tx.send(ControlMsg::Captured { gen, result });
                });
            }
        }
    }

    async fn handle(&mut self, msg: ControlMsg) {
        match msg {
            ControlMsg::Hotkey(HotkeyEvent::Combo { id, at }) => {
                if id == COMBO_MAIN {
                    self.start(at, None).await;
                } else if let Some(style) = id.strip_prefix(COMBO_STYLE_PREFIX) {
                    self.start(at, Some(style.to_string())).await;
                } else if id == COMBO_UNDO {
                    capture::input::mask_menu_activation();
                    self.do_undo();
                }
            }
            ControlMsg::Hotkey(HotkeyEvent::PanelKey(vk)) => {
                if self.state != PanelState::Visible {
                    return;
                }
                if vk == VK_ESCAPE {
                    self.hide();
                } else {
                    // Enter/Tab/R/стрелки/цифры: навигация оверлея на фронте
                    let _ = self.app.emit("overlay:key", vk);
                }
            }
            ControlMsg::Hide => {
                if self.state != PanelState::Hidden {
                    self.hide();
                }
            }
            ControlMsg::Captured { gen, result } => {
                if gen == self.gen && self.state == PanelState::Visible {
                    let at = self.session.as_ref().map(|s| s.started).unwrap_or_else(Instant::now);
                    self.apply_capture(at, result, true);
                }
            }
            ControlMsg::ToastExpired(gen) => {
                if gen == self.gen && self.state == PanelState::Toast {
                    self.hide();
                }
            }
            ControlMsg::ScreenshotReady { gen, result } => {
                if gen != self.gen {
                    return;
                }
                match result {
                    Ok(e) => {
                        println!(
                            "[restyle] screenshot {}x{} {} KB: capture {:.1} ms + encode {:.1} ms",
                            e.width,
                            e.height,
                            e.bytes / 1024,
                            e.capture_ms,
                            e.encode_ms
                        );
                        self.emit_screenshot(&e);
                        if let Some(s) = &mut self.session {
                            s.shot = Shot::Ready(e);
                        }
                    }
                    Err(err) => {
                        eprintln!("[restyle] screenshot encode: {err}");
                        if let Some(s) = &mut self.session {
                            s.shot = Shot::None("failed");
                        }
                    }
                }
                self.maybe_start_generation();
            }
            ControlMsg::SelectStyle(id) => {
                if self.state != PanelState::Visible {
                    return;
                }
                if let Some(s) = &mut self.session {
                    s.wanted_style = Some(id);
                }
                self.maybe_start_generation();
            }
            ControlMsg::Regenerate => {
                if self.state == PanelState::Visible {
                    self.maybe_start_generation();
                }
            }
            ControlMsg::Generated { gen, result } => {
                if gen != self.gen {
                    return;
                }
                let mut paste_now = false;
                if let Some(s) = &mut self.session {
                    s.generation = None;
                    match result {
                        Ok(text) => {
                            s.result = Some(text);
                            paste_now = s.paste_on_done || s.auto_paste;
                            s.paste_on_done = false;
                            s.auto_paste = false;
                        }
                        Err(_) => s.paste_on_done = false,
                    }
                }
                if paste_now && self.state == PanelState::Visible {
                    self.do_paste();
                }
            }
            ControlMsg::Paste => {
                if self.state == PanelState::Visible {
                    self.do_paste();
                }
            }
            ControlMsg::Undo => self.do_undo(),
            ControlMsg::Pasted { gen, result, mut entry } => {
                match result {
                    Ok(method) => {
                        println!("[restyle] pasted via {method:?}");
                        entry.via_uia = method == PasteMethod::Uia;
                        self.history.push(*entry);
                        self.refresh_tray_history();
                    }
                    Err(e) => {
                        eprintln!("[restyle] paste failed: {e}");
                        self.show_toast("Не удалось вставить");
                    }
                }
                if gen == self.gen {
                    self.session = None;
                    self.gen += 1;
                }
            }
            ControlMsg::Undone { result, restored_original } => {
                self.undo_in_flight = false;
                match result {
                    Ok(_) => {
                        if let Some(e) = self.history.last_mut() {
                            if !e.select_only {
                                e.showing_original = restored_original;
                                self.history.save();
                                println!("[restyle] undo: {}", if restored_original { "исходник возвращён" } else { "результат возвращён" });
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[restyle] undo failed: {e}");
                        self.show_toast("Не удалось вернуть текст");
                    }
                }
            }
            ControlMsg::SetHistoryPersist(on) => self.history.set_persist(on),
            ControlMsg::GetHistory(reply) => {
                let _ = reply.send(self.history.items());
            }
            ControlMsg::CopyHistory(index) => {
                let Some(e) = self.history.nth_newest(index) else { return };
                let le = history::line_ending_from_str(&e.line_ending);
                let text = apply_line_ending(&e.result, le);
                let rx = self.capture.set_clipboard(text);
                tauri::async_runtime::spawn(async move {
                    if let Ok(Err(e)) = rx.await {
                        eprintln!("[restyle] история → буфер: {e}");
                    }
                });
                self.show_toast("Результат скопирован в буфер");
            }
            ControlMsg::ClearHistory => {
                self.history.clear();
                self.refresh_tray_history();
            }
            ControlMsg::RefreshTray => self.refresh_tray_history(),
            ControlMsg::FrontendReady => {
                if self.state != PanelState::Hidden {
                    if let Some(p) = &self.last_show {
                        let _ = self.app.emit("overlay:show", p);
                    }
                    if let Some(c) = self.session.as_ref().and_then(|s| s.captured.as_ref()) {
                        self.emit_text(c);
                    }
                    if let Some(Shot::Ready(e)) = self.session.as_ref().map(|s| &s.shot) {
                        self.emit_screenshot(e);
                    }
                }
            }
            ControlMsg::FrontendShown => {
                if let Some(t) = self.show_started.take() {
                    // Критерий приёмки: < 100 мс от хоткея до отрисованного оверлея
                    // (включая захват текста; фаза 3 добавит скриншот).
                    println!(
                        "[restyle] show latency: {:.1} ms",
                        t.elapsed().as_secs_f64() * 1000.0
                    );
                }
            }
        }
    }
}

pub fn spawn(
    mut rx: UnboundedReceiver<ControlMsg>,
    tx: UnboundedSender<ControlMsg>,
    app: AppHandle,
    hwnd: isize,
    capture: CaptureHandle,
) {
    tauri::async_runtime::spawn(async move {
        let settings = Settings::load(&app);
        let history_path = app.path().app_config_dir().ok().map(|d| d.join("history.json"));
        let mut ctl = Control {
            history: History::load(history_path, settings.history_to_disk),
            app,
            tx,
            capture,
            hwnd,
            state: PanelState::Hidden,
            show_started: None,
            last_show: None,
            gen: 0,
            session: None,
            undo_in_flight: false,
        };
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(m) => ctl.handle(m).await,
                    None => break,
                },
                _ = tick.tick() => hotkey::watchdog_check(),
            }
        }
    });
}
