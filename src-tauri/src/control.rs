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
use crate::hotkey::{self, HotkeyEvent, COMBO_LAYOUT, COMBO_MAIN, COMBO_STYLE_PREFIX, COMBO_UNDO};
use crate::textfx::{self, Cyr, FORMAT_PREFIX, LAYOUT_ID};
use crate::ai::{self, AiError, RewriteRequest};
use crate::ipc::{
    RewriteChunk, RewriteDone, RewriteError, RewriteStart, RingPayload, ScreenshotPayload,
    ShowPayload, TextPayload,
};
use crate::screenshot::{self, Encoded, Mode};
use crate::secrets;
use crate::settings::{Settings, StyleKind};
use crate::translate;
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
/// Логическая высота тоста (макет: иконка 26 + два ряда текста).
const TOAST_H: f32 = 58.0;
/// Тост — карточка по размеру текста, а не полоса во всю панель: окно
/// подкрашено акрилом, и всё, что карточка не закрыла, видно серым фоном.
/// Стартовая ширина — оценка по длине строки, точную присылает фронтенд.
const TOAST_W_MIN: f32 = 110.0;
const TOAST_W_MAX: f32 = 460.0;
/// Кольцо удержания: маленький кружок у курсора (окно ровно по нему).
const RING_H: f32 = 46.0;
const RING_W: f32 = 46.0;
/// Своё меню трея: ширина как в макете, высота по содержимому.
const MENU_W: f32 = 272.0;
/// Список регистров — то же окно меню, но уже.
const FMT_MENU_W: f32 = 230.0;
const MENU_H: f32 = 300.0;
/// Тост после вставки живёт дольше: у него есть подсказка про undo.

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
    /// Генерация завершилась (успех или ошибка) — снять хэндл. `req` — номер
    /// запроса в сессии: результат уже отменённого запроса отбрасывается.
    Generated { gen: u64, req: u64, result: Result<String, AiError> },
    /// Enter в оверлее: вставить результат (или дождаться его и вставить).
    Paste,
    /// Хоткей/трей: вернуть предыдущий текст (повторно — снова результат).
    Undo,
    /// Вставка завершилась (capture-поток).
    Pasted { gen: u64, result: PasteResult, entry: Box<HistoryEntry> },
    /// Undo-вставка завершилась.
    Undone { result: PasteResult, restored_original: bool },
    /// Настройки истории изменились (диск, лимит записей).
    HistoryConfig { persist: bool, limit: u32 },
    /// Вернуть текст конкретной записи истории (окно истории).
    UndoEntry(usize),
    /// Фронтенд оверлея померил содержимое: подогнать высоту окна.
    Resize(f32),
    /// Окно настроек просит список последних переписываний.
    GetHistory(tokio::sync::oneshot::Sender<Vec<HistoryItem>>),
    /// Трей/настройки: положить результат записи в буфер (0 — самая свежая).
    CopyHistory(usize),
    /// Стереть историю (в памяти и на диске).
    ClearHistory,
    /// Клик по иконке в трее — показать своё меню у курсора.
    TrayMenu,
    /// Спрятать меню трея (клик мимо, Esc, выбран пункт).
    CloseTrayMenu,
    /// Меню померило свою высоту — подогнать окно.
    ResizeMenu(f32),
    /// Тост померил себя во фронтенде: {ширина, высота} в логических px.
    ResizeToast(f32, f32),
    /// Удержание дотянуло до кольца-индикатора у курсора.
    HoldRing { gen: u64 },
    /// Удержание дотянуло до открытия панели выбора.
    HoldOpen { gen: u64 },
    /// Трей собран — наполнить подменю тем, что уже прочитано с диска.
    RefreshTray,
    /// Кнопка «Регистр» в панели: границы кнопки в логических px окна оверлея.
    FormatMenu { left: f32, top: f32, bottom: f32, current: String },
}

/// Что сейчас показывает окно меню.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuMode {
    Tray,
    Format,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelState {
    /// Кольцо-индикатор удержания: окно видно, но клавиши не глотаем.
    Ring,
    Hidden,
    Visible,
    Toast,
}

/// Стартовая ширина тоста по длине текста: окно показывается сразу, а точный
/// размер приезжает из фронтенда кадром позже. Промах в меньшую сторону лучше
/// промаха в большую: лишняя ширина — это видимый кусок акрила.
fn estimate_toast_width(text: &str) -> f32 {
    let (title, rest) = text.split_once(" — ").unwrap_or((text, ""));
    // 13 px medium ≈ 7 px на знак, 11,5 px ≈ 6 px; плюс иконка, отступы и зазор.
    let title_w = title.chars().count() as f32 * 7.0;
    let rest_w = rest.chars().count() as f32 * 6.0;
    (28.0 + 26.0 + 11.0 + title_w.max(rest_w)).clamp(TOAST_W_MIN, TOAST_W_MAX)
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
    /// Номер текущего запроса генерации (см. `ControlMsg::Generated`).
    req: u64,
    /// Последний полный результат.
    result: Option<String>,
    /// Enter нажат до конца генерации — вставить, как только результат придёт.
    paste_on_done: bool,
    /// Быстрый стиль + настройка autoPaste: вставить без Enter.
    auto_paste: bool,
    /// Результат — только выделенный фрагмент: Ctrl+V без Ctrl+A.
    paste_selection: bool,
    /// Вернуть каретку/выделение после вставки (единицы UIA).
    caret: Option<(i32, i32)>,
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
    /// Растёт на каждый запрос генерации: смена стиля не меняет `gen`, а
    /// результат уже отменённого запроса мог успеть лечь в канал.
    req_seq: u64,
    /// Логическая высота панели (фронтенд присылает замер содержимого).
    panel_h: f32,
    /// Курсор на момент показа — от него пересчитывается позиция при росте.
    anchor: (i32, i32),
    /// Длительность следующего тоста, если она отличается от обычной.
    toast_hold: Option<Duration>,
    session: Option<Session>,
    history: History,
    /// Undo уже идёт — второй не запускаем.
    undo_in_flight: bool,
    /// Окно своего меню трея (создаётся при старте, живёт скрытым).
    menu_hwnd: isize,
    menu_visible: bool,
    /// Высота меню по содержимому и точка, у которой его открыли.
    menu_h: f32,
    /// Ширина тоста: у каждого сообщения своя, окно под неё подгоняется.
    toast_w: f32,
    menu_anchor: (i32, i32),
    /// Основная комбинация зажата, развилка «короткое/долгое» ещё не решена.
    hold: Option<Hold>,
    /// Какую запись истории откатываем (индекс как в `items()`).
    undo_index: usize,
    menu_mode: MenuMode,
    /// Кнопка «Регистр» на экране: (левый край, верх, низ) в пикселях.
    fmt_btn: (i32, i32, i32),
    /// Высота списка регистров по замеру.
    fmt_h: f32,
    /// Когда список закрылся: клик по кнопке сначала закрывает его хуком мыши,
    /// и следующий за ним `FormatMenu` не должен открыть список снова.
    fmt_closed_at: Option<Instant>,
}

/// Зажатая основная комбинация: ждём отпускания (быстрый стиль) или
/// истечения `long_press_open_ms` (панель выбора).
struct Hold {
    /// Поколение сессии, к которому относится удержание.
    gen: u64,
    /// Момент нажатия — от него считаются оба порога.
    at: Instant,
    /// Панель уже открыта: отпускание больше ничего не решает.
    opened: bool,
}

impl Control {
    /// Геометрия панели у точки `cursor` при логической высоте `panel_h`.
    fn place_at(&self, cursor: (i32, i32), panel_h: f32) -> (i32, i32, i32, i32, f32) {
        self.place_sized(cursor, self.state_width(), panel_h)
    }

    /// Ширина окна оверлея в текущем состоянии: у кольца оно размером с кольцо.
    fn state_width(&self) -> f32 {
        match self.state {
            PanelState::Ring => RING_W,
            PanelState::Toast => self.toast_w,
            _ => PANEL_W,
        }
    }

    fn place_sized(&self, cursor: (i32, i32), logical_w: f32, panel_h: f32) -> (i32, i32, i32, i32, f32) {
        let (work, dpi) = window::monitor_at(cursor);
        let scale = dpi as f32 / 96.0;
        let w = (logical_w * scale).round() as i32;
        let h = (panel_h * scale).round() as i32;
        let offset = (CURSOR_OFFSET * scale).round() as i32;
        let (x, y) = window::place_panel(cursor, (w, h), work, offset);
        (x, y, w, h, scale)
    }

    /// Высота содержимого приехала из фронтенда: подгоняем окно, не трогая
    /// фокус. Позиция пересчитывается от того же курсора — панель, отражённая
    /// вверх у нижнего края экрана, при росте не уползает за границу.
    fn resize_panel(&mut self, logical_h: f32) {
        let h = logical_h.clamp(window::PANEL_H_MIN, window::PANEL_H_MAX);
        // У тоста свой замер (`resize_toast`) и свой минимум высоты: панельный
        // PANEL_H_MIN оставил бы под карточкой полоску акрила.
        if matches!(self.state, PanelState::Hidden | PanelState::Toast)
            || (self.panel_h - h).abs() < 0.5
        {
            return;
        }
        self.panel_h = h;
        let (x, y, w, ph, _) = self.place_at(self.anchor, h);
        let hwnd = self.hwnd;
        let _ = self.app.run_on_main_thread(move || window::move_to(hwnd, x, y, w, ph));
    }

    /// Фронтенд померил карточку тоста: окно ужимается ровно под неё, иначе
    /// вокруг карточки остаётся акриловый фон окна.
    fn resize_toast(&mut self, logical_w: f32, logical_h: f32) {
        if self.state != PanelState::Toast {
            return;
        }
        let w = logical_w.clamp(TOAST_W_MIN, TOAST_W_MAX);
        let h = logical_h.clamp(40.0, 160.0);
        if (self.toast_w - w).abs() < 0.5 && (self.panel_h - h).abs() < 0.5 {
            return;
        }
        self.toast_w = w;
        self.panel_h = h;
        let (x, y, pw, ph, _) = self.place_sized(self.anchor, w, h);
        let hwnd = self.hwnd;
        let _ = self.app.run_on_main_thread(move || window::move_to(hwnd, x, y, pw, ph));
    }

    fn show_window(&mut self, at: Instant, payload: ShowPayload, x: i32, y: i32, w: i32, h: i32) {
        self.close_format_menu();
        let hwnd = self.hwnd;
        let _ = self.app.run_on_main_thread(move || window::show_at(hwnd, x, y, w, h));
        hotkey::set_panel_visible(payload.toast.is_none());
        self.state = if payload.toast.is_some() { PanelState::Toast } else { PanelState::Visible };
        self.show_started = Some(at);
        let _ = self.app.emit("overlay:show", &payload);
        self.last_show = Some(payload);
    }

    fn show_overlay(&mut self, at: Instant, capturing: bool) {
        self.anchor = window::cursor_pos();
        self.panel_h = PANEL_H;
        // Кольцо могло сузить окно — панель считаем по своей ширине.
        let (x, y, w, h, scale) = self.place_sized(self.anchor, PANEL_W, self.panel_h);
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

    /// Кольцо-индикатор удержания у курсора. Клавиши при нём не глотаем:
    /// пользователь ещё может отпустить комбинацию и получить быстрый стиль.
    fn show_ring(&mut self) {
        let settings = Settings::load(&self.app);
        self.anchor = window::cursor_pos();
        self.panel_h = RING_H;
        let (x, y, w, _h, scale) = self.place_sized(self.anchor, RING_W, self.panel_h);
        let h = (RING_H * scale).round() as i32;
        let hwnd = self.hwnd;
        let _ = self.app.run_on_main_thread(move || window::show_at(hwnd, x, y, w, h));
        self.state = PanelState::Ring;
        let _ = self.app.emit(
            "overlay:ring",
            RingPayload {
                gen: self.gen,
                x,
                y,
                dpi_scale: scale,
                // Сколько осталось до открытия панели: столько и заполняется кольцо.
                fill_ms: settings.long_press_open_ms.saturating_sub(settings.long_press_ring_ms),
            },
        );
    }

    /// Короткое нажатие: применяем стиль по умолчанию, как быстрый хоткей.
    fn apply_quick_style(&mut self, at: Instant, style_id: String) {
        if style_id.is_empty() || self.session.is_none() {
            return;
        }
        let auto_paste = Settings::load(&self.app).auto_paste;
        let captured = {
            let Some(s) = self.session.as_mut() else { return };
            s.style_id = Some(style_id.clone());
            s.wanted_style = Some(style_id);
            s.auto_paste = auto_paste;
            s.captured.clone()
        };
        // Панель показываем, только когда текст уже есть: иначе при пустом
        // поле она мелькает с «читаю текст…» и сразу сменяется тостом.
        // Нет текста — покажет `apply_capture`, когда захват ответит.
        if let Some(c) = captured {
            self.show_overlay(at, false);
            self.emit_text(&c);
        } else if self.state == PanelState::Ring {
            // Кольцо убираем, но сессию оставляем: захват ещё идёт, и
            // `Captured` покажет панель и запустит стиль, когда текст придёт.
            let hwnd = self.hwnd;
            let _ = self.app.run_on_main_thread(move || window::hide(hwnd));
            self.state = PanelState::Hidden;
            let _ = self.app.emit("overlay:hide", ());
        }
        self.maybe_start_generation();
    }

    /// Меню трея у курсора. Окно не активируется, поэтому клик мимо ловим
    /// LL-хуком мыши, а не событием потери фокуса.
    fn show_tray_menu(&mut self) {
        self.menu_mode = MenuMode::Tray;
        let cursor = window::cursor_pos();
        self.menu_anchor = cursor;
        let (x, y, w, h, scale) = self.place_menu(cursor, self.menu_h);
        let hwnd = self.menu_hwnd;
        let _ = self.app.run_on_main_thread(move || window::show_at(hwnd, x, y, w, h));
        self.menu_visible = true;
        hotkey::set_menu_visible(true, (x, y, w, h));
        let _ = self.app.emit("menu:show", scale);
    }

    fn close_tray_menu(&mut self) {
        if !self.menu_visible {
            return;
        }
        self.menu_visible = false;
        if self.menu_mode == MenuMode::Format {
            self.fmt_closed_at = Some(Instant::now());
        }
        hotkey::set_menu_visible(false, (0, 0, 0, 0));
        let hwnd = self.menu_hwnd;
        let _ = self.app.run_on_main_thread(move || window::hide(hwnd));
        let _ = self.app.emit("menu:hide", ());
    }

    /// Геометрия меню у курсора (та же обрезка по рабочей области, что у панели).
    fn place_menu(&self, cursor: (i32, i32), logical_h: f32) -> (i32, i32, i32, i32, f32) {
        let (work, dpi) = window::monitor_at(cursor);
        let scale = dpi as f32 / 96.0;
        let w = (MENU_W * scale).round() as i32;
        let h = (logical_h * scale).round() as i32;
        let (x, y) = window::place_panel(cursor, (w, h), work, 0);
        (x, y, w, h, scale)
    }

    fn show_toast(&mut self, text: &str) {
        self.gen += 1;
        self.session = None;
        self.anchor = window::cursor_pos();
        self.panel_h = TOAST_H;
        self.toast_w = estimate_toast_width(text);
        let (x, y, w, th, scale) = self.place_sized(self.anchor, self.toast_w, self.panel_h);
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
        let hold = self.toast_hold.take().unwrap_or(TOAST_DURATION);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(hold).await;
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

    /// Список регистров живёт только вместе с панелью.
    fn close_format_menu(&mut self) {
        if self.menu_visible && self.menu_mode == MenuMode::Format {
            self.close_tray_menu();
        }
    }

    /// Геометрия списка регистров: под кнопкой, а если снизу не хватает
    /// рабочей области — над ней. Панель при этом не двигается.
    fn place_format_menu(&self, logical_h: f32) -> (i32, i32, i32, i32) {
        let (bx, top, bottom) = self.fmt_btn;
        let (work, dpi) = window::monitor_at((bx, top));
        let scale = dpi as f32 / 96.0;
        let w = (FMT_MENU_W * scale).round() as i32;
        let h = (logical_h * scale).round() as i32;
        let gap = (4.0 * scale).round() as i32;
        let y = if bottom + gap + h <= work.bottom {
            bottom + gap
        } else if top - gap - h >= work.top {
            top - gap - h
        } else {
            (work.bottom - h).max(work.top)
        };
        let x = bx.min(work.right - w).max(work.left);
        (x, y, w, h)
    }

    fn show_format_menu(&mut self, left: f32, top: f32, bottom: f32, current: String) {
        if self.state != PanelState::Visible {
            return;
        }
        if self.menu_visible {
            self.close_tray_menu();
            return;
        }
        if self.fmt_closed_at.is_some_and(|t| t.elapsed() < Duration::from_millis(350)) {
            return; // этот клик по кнопке уже закрыл список
        }
        let (x, y, _, _, scale) = self.place_at(self.anchor, self.panel_h);
        let px = |v: f32| (v * scale).round() as i32;
        self.fmt_btn = (x + px(left), y + px(top), y + px(bottom));
        self.menu_mode = MenuMode::Format;
        // Сначала содержимое, потом окно: иначе первый кадр — старое меню трея.
        let _ = self.app.emit("menu:format", serde_json::json!({ "scale": scale, "current": current }));
        let (mx, my, mw, mh) = self.place_format_menu(self.fmt_h);
        let hwnd = self.menu_hwnd;
        let _ = self.app.run_on_main_thread(move || window::show_at(hwnd, mx, my, mw, mh));
        self.menu_visible = true;
        hotkey::set_menu_visible(true, (mx, my, mw, mh));
    }

    fn hide(&mut self) {
        self.close_format_menu();
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
    /// Меню трея — обычное окно и само перечитывает историю по событию
    /// `history-changed`, так что здесь ничего перестраивать не нужно.
    fn refresh_tray_history(&self) {}

    /// Запустить генерацию, если есть стиль, текст и решён скриншот.
    fn maybe_start_generation(&mut self) {
        let Some(s) = &self.session else { return };
        let Some(style_id) = s.wanted_style.clone() else { return };
        let Some(captured) = &s.captured else { return };
        // Регистр — не модель: считаем на месте и отдаём тем же путём, что и
        // результат генерации (Enter, история и undo работают без изменений).
        if let Some(kind) = style_id.strip_prefix(FORMAT_PREFIX) {
            // Есть выделение — меняем только его и вклеиваем в текст поля.
            let sel = captured.selection.as_ref().map(|x| (x.text.as_str(), x.start_chars));
            let Some(sp) = textfx::apply_to(&captured.text, sel, |t| textfx::apply_case(kind, t)) else { return };
            let caret = if sp.keep_caret {
                captured.selection.as_ref().map(|x| (x.uia_start, x.uia_len))
            } else {
                None
            };
            self.abort_generation();
            self.req_seq += 1;
            let req = self.req_seq;
            if let Some(s) = &mut self.session {
                s.result = None;
                s.req = req;
                s.paste_selection = sp.only_selection;
                s.caret = caret;
            }
            let gen = self.gen;
            let _ = self.app.emit("rewrite:start", RewriteStart { gen, req, style_id: style_id.clone(), with_screenshot: false });
            let _ = self.app.emit(
                "rewrite:done",
                RewriteDone { gen, req, text: sp.shown, elapsed_ms: 0.0, first_chunk_ms: 0.0 },
            );
            let _ = self.tx.send(ControlMsg::Generated { gen, req, result: Ok(sp.paste) });
            return;
        }
        let settings = Settings::load(&self.app);
        let Some(style) = settings.styles.iter().find(|st| st.id == style_id).cloned() else {
            let _ = self.app.emit(
                "rewrite:error",
                RewriteError { gen: self.gen, req: self.req_seq, message: format!("Стиль «{style_id}» не найден") },
            );
            return;
        };
        // У стиля свой тумблер: правке грамматики кадр не нужен.
        if style.screenshot && matches!(s.shot, Shot::Encoding) {
            return; // ScreenshotReady вызовет нас снова
        }
        let screenshot = match &s.shot {
            Shot::Ready(e) if style.screenshot => Some(e.jpeg_base64.clone()),
            _ => None,
        };
        // Перевод отдельным движком: модель получает уже переведённый текст —
        // и только если у стиля задано «причесать после перевода».
        let translate_to = match style.kind {
            StyleKind::Translate if settings.translator == "deepl" => Some(style.target_lang.clone()),
            _ => None,
        };
        let post = settings
            .styles
            .iter()
            .find(|st| !style.post_style.is_empty() && st.id == style.post_style)
            .map(|st| (st.name.clone(), st.instruction.clone()));

        let mut req = RewriteRequest {
            system_prompt: ai::system_prompt(self.app.path().app_config_dir().ok()),
            style_name: style.name.clone(),
            style_instruction: style.instruction.clone(),
            text: captured.text.clone(),
            screenshot_jpeg_base64: screenshot,
            model: settings.model.clone(),
        };
        if let Some((name, instruction)) = post.clone() {
            // После перевода модель работает инструкцией стиля-причёски.
            req.style_name = name;
            req.style_instruction = instruction;
        }

        self.abort_generation();
        self.req_seq += 1;
        let req_id = self.req_seq;
        let gen = self.gen;
        let with_screenshot = req.screenshot_jpeg_base64.is_some();
        let _ = self.app.emit("rewrite:start", RewriteStart { gen, req: req_id, style_id: style.id.clone(), with_screenshot });
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

            // --- перевод (DeepL) --------------------------------------------
            let mut req = req;
            if let Some(lang) = translate_to {
                let key = tokio::task::spawn_blocking(secrets::get_deepl_key)
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
                match key {
                    Ok(Some(k)) => match translate::translate(&k, &req.text, &lang).await {
                        Ok(translated) => {
                            println!(
                                "[restyle] DeepL → {lang}: {:.0} мс, {} символов",
                                t0.elapsed().as_secs_f32() * 1000.0,
                                translated.chars().count()
                            );
                            if post.is_none() {
                                // Причёсывать нечем — перевод и есть результат.
                                let elapsed_ms = t0.elapsed().as_secs_f32() * 1000.0;
                                let _ = app.emit("rewrite:chunk", RewriteChunk { gen, req: req_id, text: translated.clone() });
                                let _ = app.emit(
                                    "rewrite:done",
                                    RewriteDone {
                                        gen,
                                        req: req_id,
                                        text: translated.clone(),
                                        elapsed_ms,
                                        first_chunk_ms: elapsed_ms,
                                    },
                                );
                                let _ = tx.send(ControlMsg::Generated { gen, req: req_id, result: Ok(translated) });
                                return;
                            }
                            req.text = translated;
                        }
                        Err(e) => {
                            eprintln!("[restyle] DeepL: {e:?}");
                            let _ = app.emit("rewrite:error", RewriteError { gen, req: req_id, message: e.user_message() });
                            let _ = tx.send(ControlMsg::Generated { gen, req: req_id, result: Err(e) });
                            return;
                        }
                    },
                    // Ключа DeepL нет — переводит модель по инструкции стиля.
                    Ok(None) => println!("[restyle] ключа DeepL нет — перевод моделью"),
                    Err(e) => eprintln!("[restyle] keyring DeepL: {e}"),
                }
            }

            let key_result = match tokio::task::spawn_blocking(secrets::get_api_key).await {
                Ok(r) => r,
                Err(e) => Err(e.to_string()),
            };
            let key = match key_result {
                Ok(Some(k)) => k,
                Ok(None) => {
                    let _ = app.emit("rewrite:error", RewriteError { gen, req: req_id, message: AiError::NoApiKey.user_message() });
                    let _ = tx.send(ControlMsg::Generated { gen, req: req_id, result: Err(AiError::NoApiKey) });
                    return;
                }
                Err(e) => {
                    let err = AiError::Other(e);
                    let _ = app.emit("rewrite:error", RewriteError { gen, req: req_id, message: err.user_message() });
                    let _ = tx.send(ControlMsg::Generated { gen, req: req_id, result: Err(err) });
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
                    let _ = app2.emit("rewrite:chunk", RewriteChunk { gen, req: req_id, text });
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
                        RewriteDone { gen, req: req_id, text: text.clone(), elapsed_ms, first_chunk_ms: first },
                    );
                }
                Err(e) => {
                    eprintln!("[restyle] rewrite error: {e:?}");
                    let _ = app.emit("rewrite:error", RewriteError { gen, req: req_id, message: e.user_message() });
                }
            }
            let _ = tx.send(ControlMsg::Generated { gen, req: req_id, result });
        });
        if let Some(s) = &mut self.session {
            s.generation = Some(handle);
            s.req = req_id;
            s.result = None;
            // Модель переписывает всё поле: длина другая, каретку не вернуть.
            s.paste_selection = false;
            s.caret = None;
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
        let settings = Settings::load(&self.app);
        let select_only = captured.selection_only || s.paste_selection;
        let opts = PasteOptions {
            prefer_uia: captured.uia_writable && !select_only,
            select_only,
            restore_clipboard: settings.restore_clipboard,
            caret: s.caret,
        };
        let original = match (&captured.selection, s.paste_selection) {
            (Some(sel), true) => sel.text.clone(),
            _ => captured.text.clone(),
        };
        let entry = HistoryEntry {
            at: history::now_ms(),
            target_exe: target_exe.clone(),
            target_hwnd,
            style_id: s.wanted_style.clone().unwrap_or_default(),
            original,
            result: result.clone(),
            line_ending: history::line_ending_to_str(captured.line_ending).into(),
            select_only,
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
        self.close_format_menu();
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

    fn do_undo(&mut self) {
        self.do_undo_entry(0);
    }

    /// Undo записи истории (`index` как в `items()`, 0 — последняя): вернуть
    /// исходник (или результат, если исходник уже стоит). Окно должно быть тем
    /// же; select-only — только буфер.
    fn do_undo_entry(&mut self, index: usize) {
        if self.undo_in_flight {
            return;
        }
        let Some(e) = self.history.nth_newest(index).cloned() else {
            self.show_toast("Нечего возвращать");
            return;
        };
        self.undo_index = index;
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
        let opts = PasteOptions {
            prefer_uia: e.via_uia,
            select_only: false,
            restore_clipboard: Settings::load(&self.app).restore_clipboard,
            caret: None,
        };
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
                // Пока решается развилка «короткое/долгое», окна ещё нет:
                // текст уедет фронту в момент открытия панели.
                let holding = self.hold.as_ref().is_some_and(|h| !h.opened);
                if !already_visible && !holding {
                    self.show_overlay(at, false);
                }
                if !holding {
                    self.emit_text(&c);
                }
                if let Some(s) = &mut self.session {
                    s.captured = Some(c);
                }
                self.maybe_start_generation();
            }
            Err(e) => {
                // Причину отказа пишем всегда: пользователь видит только тост,
                // и без лога «почему пусто» не разобрать ни одной жалобы.
                eprintln!("[restyle] захват пуст: {e:?}");
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

    /// Хоткей раскладки: прочитать текст, перевести «ghbdtn» ↔ «привет» и сразу
    /// вставить — без панели. Направление определяется по буквам текста.
    async fn fix_layout(&mut self, at: Instant) {
        capture::input::mask_menu_activation();
        if self.state != PanelState::Hidden {
            self.hide();
            tokio::time::sleep(HIDE_SETTLE).await;
        }
        let settings = Settings::load(&self.app);
        let target = capture::process::foreground();
        let select_only = matches_process(&settings.select_only_processes, &target.exe);
        self.gen += 1;
        let rx = self.capture.capture(CaptureOptions { select_only, force_clipboard: false });
        let captured = match tokio::time::timeout(Duration::from_millis(1500), rx).await {
            Ok(Ok(Ok(c))) => c,
            Ok(Ok(Err(e))) => {
                eprintln!("[restyle] раскладка: захват пуст: {e:?}");
                self.show_toast(match e {
                    CaptureError::NoText => "Нет текста",
                    CaptureError::Password => "Поле пароля — не читаю",
                    CaptureError::Failed(_) => "Не удалось прочитать текст",
                });
                return;
            }
            _ => {
                self.show_toast("Не удалось прочитать текст");
                return;
            }
        };
        let (ru, uk) = window::installed_cyrillic();
        let cyr = if uk && (!ru || settings.language == "uk") { Cyr::Uk } else { Cyr::Ru };
        let sel = captured.selection.as_ref().map(|x| (x.text.as_str(), x.start_chars));
        let mut to = textfx::Layout::En;
        let Some(sp) = textfx::apply_to(&captured.text, sel, |t| {
            textfx::fix_layout(t, cyr).map(|(o, l)| {
                to = l;
                o
            })
        }) else {
            self.show_toast("Нет букв — нечего переключать");
            return;
        };
        // Каретка/выделение остаются, где были: длина текста не меняется.
        let caret = if sp.keep_caret {
            captured.selection.as_ref().map(|x| (x.uia_start, x.uia_len))
        } else {
            None
        };
        println!(
            "[restyle] раскладка → {to:?}: {} символов за {} мс (выделение: {}, каретка: {caret:?})",
            sp.shown.chars().count(),
            at.elapsed().as_millis(),
            captured.selection.as_ref().is_some_and(|x| !x.text.trim().is_empty())
        );
        let hwnd = target.hwnd;
        self.session = Some(Session {
            target,
            select_only,
            style_id: None,
            captured: Some(captured),
            shot: Shot::None("off"),
            started: at,
            wanted_style: Some(LAYOUT_ID.into()),
            generation: None,
            req: 0,
            result: Some(sp.paste),
            paste_on_done: false,
            auto_paste: false,
            paste_selection: sp.only_selection,
            caret,
        });
        window::switch_layout(hwnd, to.primary_lang());
        self.do_paste();
    }

    /// Хоткей: новая сессия. Захват стартует сразу; оверлей — после захвата
    /// либо по мягкому дедлайну.
    /// `hold_mode` — основная комбинация при включённом «долгом нажатии»:
    /// захват идёт сразу, а панель ждёт решения развилки.
    async fn start(&mut self, at: Instant, style_id: Option<String>, hold_mode: bool) {
        // Проглоченная клавиша + отпускание Alt = меню окна; маскируем сразу.
        capture::input::mask_menu_activation();

        let settings = Settings::load(&self.app);
        let target = capture::process::foreground();
        let select_only = matches_process(&settings.select_only_processes, &target.exe);
        println!(
            "[restyle] hotkey in {} (select_only={select_only}, hold={hold_mode})",
            target.exe
        );

        // Повторный хоткей при видимом оверлее: спрятать до снимка, иначе
        // оверлей попадёт в кадр.
        if self.state != PanelState::Hidden {
            self.hide();
            tokio::time::sleep(HIDE_SETTLE).await;
        }

        self.gen += 1;
        let gen = self.gen;
        let quick = style_id.is_some();
        self.hold = if hold_mode { Some(Hold { gen, at, opened: false }) } else { None };
        if hold_mode {
            // Кольцо и панель — два таймера от момента нажатия; оба проверяют
            // поколение, поэтому новый хоткей их обесценивает.
            for (delay, open) in [
                (settings.long_press_ring_ms, false),
                (settings.long_press_open_ms, true),
            ] {
                let tx = self.tx.clone();
                let left = Duration::from_millis(delay as u64).saturating_sub(at.elapsed());
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(left).await;
                    let _ = tx.send(if open {
                        ControlMsg::HoldOpen { gen }
                    } else {
                        ControlMsg::HoldRing { gen }
                    });
                });
            }
        }

        // Текст и экран — параллельно: захват текста уже идёт на capture-потоке,
        // пока здесь снимается кадр.
        let mut rx = self.capture.capture(CaptureOptions { select_only, force_clipboard: false });
        // Быстрый стиль знает заранее, нужен ли ему кадр: если нет — не тратим
        // 20-60 мс на захват и кодирование.
        let wants_shot = style_id
            .as_ref()
            .and_then(|id| settings.styles.iter().find(|s| &s.id == id))
            .map(|s| s.screenshot)
            .unwrap_or(true);
        let shot = if wants_shot {
            self.take_screenshot(&settings, &target, gen).await
        } else {
            Shot::None("off")
        };

        self.session = Some(Session {
            target,
            select_only,
            wanted_style: style_id.clone(), // быстрый стиль: генерация стартует сама
            style_id,
            captured: None,
            shot,
            started: at,
            generation: None,
            req: 0,
            result: None,
            paste_on_done: false,
            auto_paste: quick && settings.auto_paste,
            paste_selection: false,
            caret: None,
        });
        // `&mut rx`: по таймауту receiver остаётся у нас и уходит в фоновый таск.
        match tokio::time::timeout(CAPTURE_SOFT_DEADLINE, &mut rx).await {
            Ok(Ok(result)) => self.apply_capture(at, result, false),
            Ok(Err(_)) => self.show_toast("Захват недоступен"),
            Err(_elapsed) => {
                // Долгий захват (UIA в тяжёлом приложении, клипборд): показываем
                // оверлей с «Читаю текст…», результат догонит через Captured.
                // При удержании панель откроет таймер, а не захват.
                if self.hold.is_none() {
                    self.show_overlay(at, true);
                }
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
                    let s = Settings::load(&self.app);
                    let hold = s.long_press && !s.quick_style.is_empty();
                    self.start(at, None, hold).await;
                } else if let Some(style) = id.strip_prefix(COMBO_STYLE_PREFIX) {
                    self.start(at, Some(style.to_string()), false).await;
                } else if id == COMBO_UNDO {
                    capture::input::mask_menu_activation();
                    self.do_undo();
                } else if id == COMBO_LAYOUT {
                    self.fix_layout(at).await;
                }
            }
            ControlMsg::Hotkey(HotkeyEvent::ComboReleased { id }) => {
                if id != COMBO_MAIN {
                    return;
                }
                // Alt держали долго — маскируем ещё раз, иначе его отпускание
                // откроет меню окна под оверлеем.
                capture::input::mask_menu_activation();
                let Some(hold) = self.hold.take() else { return };
                if hold.opened || hold.gen != self.gen {
                    return;
                }
                let style = Settings::load(&self.app).quick_style;
                println!(
                    "[restyle] короткое нажатие ({} мс) — стиль {style}",
                    hold.at.elapsed().as_millis()
                );
                self.apply_quick_style(hold.at, style);
            }
            ControlMsg::TrayMenu => {
                if self.menu_visible {
                    self.close_tray_menu();
                } else {
                    self.show_tray_menu();
                }
            }
            ControlMsg::CloseTrayMenu => self.close_tray_menu(),
            ControlMsg::FormatMenu { left, top, bottom, current } => {
                self.show_format_menu(left, top, bottom, current);
            }
            ControlMsg::ResizeMenu(h) if self.menu_mode == MenuMode::Format => {
                let h = h.clamp(40.0, 400.0);
                if !self.menu_visible || (self.fmt_h - h).abs() < 0.5 {
                    self.fmt_h = h;
                    return;
                }
                self.fmt_h = h;
                let (x, y, w, ph) = self.place_format_menu(h);
                let hwnd = self.menu_hwnd;
                let _ = self.app.run_on_main_thread(move || window::move_to(hwnd, x, y, w, ph));
                hotkey::set_menu_visible(true, (x, y, w, ph));
            }
            ControlMsg::ResizeMenu(h) => {
                let h = h.clamp(80.0, 620.0);
                if !self.menu_visible || (self.menu_h - h).abs() < 0.5 {
                    self.menu_h = h;
                    return;
                }
                self.menu_h = h;
                let cursor = self.menu_anchor;
                let (x, y, w, ph, _) = self.place_menu(cursor, h);
                let hwnd = self.menu_hwnd;
                let _ = self.app.run_on_main_thread(move || window::move_to(hwnd, x, y, w, ph));
            }
            ControlMsg::HoldRing { gen } => {
                if self.hold.as_ref().is_some_and(|h| h.gen == gen && !h.opened)
                    && self.state == PanelState::Hidden
                {
                    println!("[restyle] удержание: кольцо у курсора");
                    self.show_ring();
                }
            }
            ControlMsg::HoldOpen { gen } => {
                let Some(hold) = self.hold.as_mut() else { return };
                if hold.gen != gen || hold.opened {
                    return;
                }
                hold.opened = true;
                let at = hold.at;
                println!("[restyle] удержание: панель после {} мс", at.elapsed().as_millis());
                let captured = self.session.as_ref().and_then(|s| s.captured.clone());
                self.show_overlay(at, captured.is_none());
                if let Some(c) = captured {
                    self.emit_text(&c);
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
                // Не только при видимой панели: при долгом нажатии она до развилки
                // не показывается (state Hidden/Ring), а захват дольше 250 мс
                // иначе терялся — панель зависала на «Читаю текст…».
                if gen == self.gen && self.session.is_some() {
                    let at = self.session.as_ref().map(|s| s.started).unwrap_or_else(Instant::now);
                    let visible = self.state == PanelState::Visible;
                    self.apply_capture(at, result, visible);
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
            ControlMsg::Generated { gen, req, result } => {
                if gen != self.gen {
                    return;
                }
                let mut paste_now = false;
                if let Some(s) = &mut self.session {
                    if s.req != req {
                        // Стиль сменили в момент, когда старый стрим уже кончился:
                        // его результат не должен стать результатом нового запроса.
                        println!("[restyle] generation #{req} устарела (текущая #{})", s.req);
                        return;
                    }
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
                        let _ = self.app.emit("history-changed", ());
                        // Подтверждения не показываем: переписанный текст уже
                        // стоит в поле, тост только перекрывал бы его.
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
                        let idx = self.undo_index;
                        let mut changed = false;
                        if let Some(e) = self.history.nth_newest_mut(idx) {
                            if !e.select_only {
                                e.showing_original = restored_original;
                                changed = true;
                                println!("[restyle] undo: {}", if restored_original { "исходник возвращён" } else { "результат возвращён" });
                            }
                        }
                        if changed {
                            self.history.save();
                            let _ = self.app.emit("history-changed", ());
                        }
                    }
                    Err(e) => {
                        eprintln!("[restyle] undo failed: {e}");
                        self.show_toast("Не удалось вернуть текст");
                    }
                }
            }
            ControlMsg::HistoryConfig { persist, limit } => {
                self.history.set_persist(persist);
                self.history.set_limit(limit as usize);
                self.refresh_tray_history();
            }
            ControlMsg::UndoEntry(index) => self.do_undo_entry(index),
            ControlMsg::Resize(h) => self.resize_panel(h),
            ControlMsg::ResizeToast(w, h) => self.resize_toast(w, h),
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
                // Отклик показывает то окно, откуда копировали.
            }
            ControlMsg::ClearHistory => {
                self.history.clear();
                self.refresh_tray_history();
                let _ = self.app.emit("history-changed", ());
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
    menu_hwnd: isize,
    capture: CaptureHandle,
) {
    tauri::async_runtime::spawn(async move {
        let settings = Settings::load(&app);
        let history_path = app.path().app_config_dir().ok().map(|d| d.join("history.json"));
        let mut ctl = Control {
            history: History::load(history_path, settings.history_to_disk, settings.history_limit as usize),
            app,
            tx,
            capture,
            hwnd,
            state: PanelState::Hidden,
            show_started: None,
            last_show: None,
            gen: 0,
            req_seq: 0,
            panel_h: PANEL_H,
            anchor: (0, 0),
            toast_hold: None,
            hold: None,
            menu_hwnd,
            menu_visible: false,
            menu_h: MENU_H,
            toast_w: TOAST_W_MIN,
            menu_anchor: (0, 0),
            session: None,
            undo_in_flight: false,
            undo_index: 0,
            menu_mode: MenuMode::Tray,
            fmt_btn: (0, 0, 0),
            fmt_h: 164.0,
            fmt_closed_at: None,
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
