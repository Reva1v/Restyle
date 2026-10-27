//! Сессия Restyle (`restyle_core::session`) на фейковых `Host` и `Platform`:
//! без окон, хука, буфера обмена, сети и keyring. Время реальное, но все
//! ответы фейков мгновенные, а пороги удержания в настройках — десятки мс.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use restyle_core::ai::{AiError, BoxFuture, Provider, RewriteRequest};
use restyle_core::capture::process::Foreground;
use restyle_core::capture::text::LineEnding;
use restyle_core::capture::{
    CaptureError, CaptureOptions, CaptureResult, Captured, PasteMethod, PasteOptions, PasteResult, TextSource,
};
use restyle_core::hotkey::HotkeyEvent;
use restyle_core::screenshot::{Mode, RawShot};
use restyle_core::session::{self, ControlMsg, Host, Platform, PxRect, Surface};
use restyle_core::settings::{Settings, Style, StyleKind};
use restyle_core::window::Rect;
use serde_json::Value;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::sync::oneshot;

const VK_ESCAPE: u32 = 0x1B;
const TARGET: isize = 0x1234;

// --- фейки -------------------------------------------------------------------

#[derive(Default)]
struct FakeHost {
    events: Mutex<Vec<(String, Value)>>,
    calls: Mutex<Vec<String>>,
    settings: Mutex<Settings>,
}

impl FakeHost {
    fn events(&self, name: &str) -> Vec<Value> {
        self.events.lock().unwrap().iter().filter(|(n, _)| n == name).map(|(_, v)| v.clone()).collect()
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
    fn toasts(&self) -> Vec<String> {
        self.events("overlay:show")
            .iter()
            .filter_map(|p| p["toast"].as_str().map(str::to_string))
            .collect()
    }
}

impl Host for FakeHost {
    fn emit(&self, event: &str, payload: &serde_json::value::RawValue) {
        let v: Value = serde_json::from_str(payload.get()).unwrap();
        self.events.lock().unwrap().push((event.to_string(), v));
    }
    fn show_panel(&self, surface: Surface, _rect: PxRect) {
        self.calls.lock().unwrap().push(format!("show:{surface:?}"));
    }
    fn move_panel(&self, surface: Surface, _rect: PxRect) {
        self.calls.lock().unwrap().push(format!("move:{surface:?}"));
    }
    fn hide_panel(&self, surface: Surface) {
        self.calls.lock().unwrap().push(format!("hide:{surface:?}"));
    }
    fn panel_keys(&self, on: bool) {
        self.calls.lock().unwrap().push(format!("keys:{on}"));
    }
    fn menu_keys(&self, rect: Option<PxRect>) {
        self.calls.lock().unwrap().push(format!("menu_keys:{}", rect.is_some()));
    }
    fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }
    fn config_dir(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn spawn(&self, f: BoxFuture<'static, ()>) {
        tokio::spawn(f);
    }
}

struct FakeSys {
    foreground: Mutex<Foreground>,
    capture: Mutex<CaptureResult>,
    captures: Mutex<Vec<CaptureOptions>>,
    pastes: Mutex<Vec<(String, PasteOptions)>>,
    clipboard: Mutex<Vec<String>>,
    gemini_key: Option<String>,
    rewrites: Arc<AtomicUsize>,
    masks: AtomicUsize,
}

impl FakeSys {
    fn new(text: &str) -> Self {
        Self {
            foreground: Mutex::new(Foreground { hwnd: TARGET, pid: 1, exe: "notepad.exe".into() }),
            capture: Mutex::new(Ok(captured(text))),
            captures: Mutex::default(),
            pastes: Mutex::default(),
            clipboard: Mutex::default(),
            gemini_key: Some("test-key".into()),
            rewrites: Arc::default(),
            masks: AtomicUsize::new(0),
        }
    }
    fn pastes(&self) -> Vec<(String, PasteOptions)> {
        self.pastes.lock().unwrap().clone()
    }
}

fn captured(text: &str) -> Captured {
    Captured {
        text: text.into(),
        line_ending: LineEnding::Lf,
        source: TextSource::Uia,
        uia_writable: true,
        selection_only: false,
        selection: None,
        elapsed: Duration::from_millis(3),
    }
}

/// Модель: стримит текст двумя кусками и отвечает `<текст>!`.
struct FakeProvider(Arc<AtomicUsize>);

impl Provider for FakeProvider {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn rewrite<'a>(
        &'a self,
        _api_key: &'a str,
        req: &'a RewriteRequest,
        chunks: UnboundedSender<String>,
    ) -> BoxFuture<'a, Result<String, AiError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let out = format!("{}!", req.text);
            let _ = chunks.send(out[..1].to_string());
            let _ = chunks.send(out[1..].to_string());
            Ok(out)
        })
    }
}

impl Platform for FakeSys {
    fn foreground(&self) -> Foreground {
        self.foreground.lock().unwrap().clone()
    }
    fn mask_menu_activation(&self) {
        self.masks.fetch_add(1, Ordering::SeqCst);
    }
    fn capture(&self, opts: CaptureOptions) -> oneshot::Receiver<CaptureResult> {
        self.captures.lock().unwrap().push(opts);
        let (tx, rx) = oneshot::channel();
        // Как настоящий capture-поток: «только выделение» помечает результат.
        let result = self.capture.lock().unwrap().clone().map(|mut c| {
            c.selection_only |= opts.select_only;
            c
        });
        let _ = tx.send(result);
        rx
    }
    fn paste(&self, text: String, opts: PasteOptions) -> oneshot::Receiver<PasteResult> {
        self.pastes.lock().unwrap().push((text, opts));
        let (tx, rx) = oneshot::channel();
        let _ = tx.send(Ok(PasteMethod::Clipboard));
        rx
    }
    fn set_clipboard(&self, text: String) -> oneshot::Receiver<Result<(), String>> {
        self.clipboard.lock().unwrap().push(text);
        let (tx, rx) = oneshot::channel();
        let _ = tx.send(Ok(()));
        rx
    }
    fn cursor_pos(&self) -> (i32, i32) {
        (500, 400)
    }
    fn monitor_at(&self, _pt: (i32, i32)) -> (Rect, u32) {
        (Rect { left: 0, top: 0, right: 1920, bottom: 1040 }, 96)
    }
    fn screenshot(&self, _mode: Mode, _hwnd: isize, _cursor: (i32, i32)) -> Result<RawShot, String> {
        Err("no screenshots in tests".into())
    }
    fn installed_cyrillic(&self) -> (bool, bool) {
        (true, false)
    }
    fn switch_layout(&self, _hwnd: isize, _primary_lang: u16) {}
    fn gemini_key(&self) -> Result<Option<String>, String> {
        Ok(self.gemini_key.clone())
    }
    fn deepl_key(&self) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn provider(&self, _model: &str) -> Box<dyn Provider> {
        Box::new(FakeProvider(self.rewrites.clone()))
    }
    fn translate(&self, _key: String, text: String, _lang: String) -> BoxFuture<'static, Result<String, AiError>> {
        Box::pin(async move { Ok(text) })
    }
}

// --- обвязка -----------------------------------------------------------------

struct Rig {
    host: Arc<FakeHost>,
    sys: Arc<FakeSys>,
    tx: UnboundedSender<ControlMsg>,
}

fn base_settings() -> Settings {
    let mut s = Settings { screenshot_enabled: false, ..Settings::default() };
    s.styles.push(Style {
        id: "polite".into(),
        name: "Polite".into(),
        instruction: "be polite".into(),
        screenshot: false,
        kind: StyleKind::Prompt,
        ..Style::default()
    });
    s
}

fn rig_with(settings: Settings, sys: FakeSys) -> Rig {
    let host = Arc::new(FakeHost { settings: Mutex::new(settings), ..FakeHost::default() });
    let sys = Arc::new(sys);
    let (tx, rx) = unbounded_channel();
    session::spawn(rx, tx.clone(), host.clone(), sys.clone());
    Rig { host, sys, tx }
}

fn rig(text: &str) -> Rig {
    rig_with(base_settings(), FakeSys::new(text))
}

impl Rig {
    fn send(&self, m: ControlMsg) {
        self.tx.send(m).unwrap();
    }
    fn hotkey(&self, id: &str) {
        self.send(ControlMsg::Hotkey(HotkeyEvent::Combo { id: id.into(), at: Instant::now() }));
    }
    fn release(&self, id: &str) {
        self.send(ControlMsg::Hotkey(HotkeyEvent::ComboReleased { id: id.into() }));
    }
    fn count(&self, event: &str) -> usize {
        self.host.events(event).len()
    }
    /// Открыть панель основным хоткеем и дождаться текста поля.
    async fn open(&self) {
        self.hotkey("main");
        wait(|| self.count("overlay:text") == 1, "overlay:text").await;
    }
    async fn pick(&self, style: &str) {
        let done = self.count("rewrite:done");
        self.send(ControlMsg::SelectStyle(style.into()));
        wait(|| self.count("rewrite:done") > done, "rewrite:done").await;
    }
}

async fn wait(cond: impl Fn() -> bool, what: &str) {
    let t0 = Instant::now();
    while !cond() {
        assert!(t0.elapsed() < Duration::from_secs(3), "timeout waiting for {what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Дать циклу обработать всё, что уже в канале (для проверок «ничего не было»).
async fn settle() {
    tokio::time::sleep(Duration::from_millis(120)).await;
}

// --- тесты -------------------------------------------------------------------

#[tokio::test]
async fn hotkey_opens_panel_with_field_text() {
    let r = rig("hello world");
    r.open().await;
    let show = &r.host.events("overlay:show")[0];
    assert!(show["toast"].is_null());
    assert_eq!(r.host.events("overlay:text")[0]["text"], "hello world");
    assert!(r.host.calls().contains(&"show:Panel".to_string()));
    assert!(r.host.calls().contains(&"keys:true".to_string()), "panel keys are swallowed while the panel is up");
    assert!(r.sys.masks.load(Ordering::SeqCst) >= 1, "Alt menu activation is masked");
}

#[tokio::test]
async fn picking_a_case_style_gives_a_result_without_the_model() {
    let r = rig("hello world");
    r.open().await;
    r.pick("format:upper").await;
    assert_eq!(r.host.events("rewrite:done")[0]["text"], "HELLO WORLD");
    assert_eq!(r.sys.rewrites.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn picking_a_model_style_streams_chunks() {
    let r = rig("hi");
    r.open().await;
    r.pick("polite").await;
    let chunks: Vec<String> =
        r.host.events("rewrite:chunk").iter().map(|c| c["text"].as_str().unwrap().to_string()).collect();
    assert_eq!(chunks.concat(), "hi!");
    assert_eq!(r.host.events("rewrite:done")[0]["text"], "hi!");
    assert_eq!(r.host.events("rewrite:start")[0]["styleId"], "polite");
}

#[tokio::test]
async fn enter_pastes_the_result_into_the_same_window() {
    let r = rig("hello world");
    r.open().await;
    r.pick("format:upper").await;
    r.send(ControlMsg::Paste);
    wait(|| r.sys.pastes().len() == 1, "paste").await;
    let (text, opts) = &r.sys.pastes()[0];
    assert_eq!(text, "HELLO WORLD");
    assert!(opts.prefer_uia && !opts.select_only);
    wait(|| r.count("history-changed") == 1, "history-changed").await;
    assert!(r.count("overlay:hide") >= 1);
    assert!(r.host.calls().contains(&"hide:Panel".to_string()));
    assert!(r.host.calls().contains(&"keys:false".to_string()));
}

#[tokio::test]
async fn enter_before_the_result_pastes_when_it_arrives() {
    let r = rig("hi");
    r.open().await;
    r.send(ControlMsg::Paste);
    wait(|| r.count("rewrite:pending_paste") == 1, "pending paste").await;
    r.pick("polite").await;
    wait(|| r.sys.pastes().len() == 1, "paste").await;
    assert_eq!(r.sys.pastes()[0].0, "hi!");
}

#[tokio::test]
async fn r_regenerates_with_the_same_style() {
    let r = rig("hi");
    r.open().await;
    r.pick("polite").await;
    r.send(ControlMsg::Regenerate);
    wait(|| r.count("rewrite:done") == 2, "second rewrite:done").await;
    assert_eq!(r.sys.rewrites.load(Ordering::SeqCst), 2);
    let starts = r.host.events("rewrite:start");
    assert_eq!(starts[1]["styleId"], "polite");
    assert_eq!(starts[1]["req"], 2);
}

#[tokio::test]
async fn esc_hides_the_panel_and_drops_the_session() {
    let r = rig("hello world");
    r.open().await;
    r.send(ControlMsg::Hotkey(HotkeyEvent::PanelKey(VK_ESCAPE)));
    wait(|| r.count("overlay:hide") == 1, "overlay:hide").await;
    assert!(r.host.calls().contains(&"hide:Panel".to_string()));
    // Сессии нет: выбор стиля и Enter больше ничего не делают.
    r.send(ControlMsg::SelectStyle("format:upper".into()));
    r.send(ControlMsg::Paste);
    settle().await;
    assert_eq!(r.count("rewrite:start"), 0);
    assert!(r.sys.pastes().is_empty());
}

#[tokio::test]
async fn other_panel_keys_go_to_the_frontend() {
    let r = rig("hello world");
    r.open().await;
    r.send(ControlMsg::Hotkey(HotkeyEvent::PanelKey(0x0D)));
    wait(|| r.count("overlay:key") == 1, "overlay:key").await;
    assert_eq!(r.host.events("overlay:key")[0], 0x0D);
}

#[tokio::test]
async fn window_change_cancels_the_paste() {
    let r = rig("hello world");
    r.open().await;
    r.pick("format:upper").await;
    r.sys.foreground.lock().unwrap().hwnd = 0x9999;
    r.send(ControlMsg::Paste);
    wait(|| !r.host.toasts().is_empty(), "toast").await;
    assert_eq!(r.host.toasts(), ["Окно сменилось — вставка отменена"]);
    assert!(r.sys.pastes().is_empty());
}

#[tokio::test]
async fn undo_returns_the_original_and_again_the_result() {
    let r = rig("hello world");
    r.open().await;
    r.pick("format:upper").await;
    r.send(ControlMsg::Paste);
    wait(|| r.count("history-changed") == 1, "history").await;
    r.hotkey("undo");
    wait(|| r.sys.pastes().len() == 2, "undo paste").await;
    assert_eq!(r.sys.pastes()[1].0, "hello world");
    wait(|| r.count("history-changed") == 2, "undo recorded").await;
    r.send(ControlMsg::Undo);
    wait(|| r.sys.pastes().len() == 3, "redo paste").await;
    assert_eq!(r.sys.pastes()[2].0, "HELLO WORLD");
}

#[tokio::test]
async fn undo_with_empty_history_shows_a_toast() {
    let r = rig("hello world");
    r.send(ControlMsg::Undo);
    wait(|| !r.host.toasts().is_empty(), "toast").await;
    assert_eq!(r.host.toasts(), ["Нечего возвращать"]);
}

#[tokio::test]
async fn quick_style_hotkey_runs_without_the_panel_choice_and_auto_pastes() {
    let settings = Settings { auto_paste: true, ..base_settings() };
    let r = rig_with(settings, FakeSys::new("hello world"));
    r.hotkey("style:format:upper");
    wait(|| r.sys.pastes().len() == 1, "auto paste").await;
    assert_eq!(r.sys.pastes()[0].0, "HELLO WORLD");
    assert_eq!(r.host.events("rewrite:start")[0]["styleId"], "format:upper");
}

#[tokio::test]
async fn quick_style_without_auto_paste_waits_for_enter() {
    let r = rig("hello world");
    r.hotkey("style:format:upper");
    wait(|| r.count("rewrite:done") == 1, "rewrite:done").await;
    settle().await;
    assert!(r.sys.pastes().is_empty());
    r.send(ControlMsg::Paste);
    wait(|| r.sys.pastes().len() == 1, "paste").await;
}

fn long_press_settings() -> Settings {
    Settings {
        long_press: true,
        quick_style: "format:upper".into(),
        long_press_ring_ms: 40,
        long_press_open_ms: 120,
        ..base_settings()
    }
}

#[tokio::test]
async fn short_press_applies_the_quick_style() {
    let r = rig_with(long_press_settings(), FakeSys::new("hello world"));
    r.hotkey("main");
    r.release("main");
    wait(|| r.count("rewrite:done") == 1, "rewrite:done").await;
    assert_eq!(r.host.events("rewrite:done")[0]["text"], "HELLO WORLD");
    settle().await;
    assert_eq!(r.count("overlay:ring"), 0);
}

#[tokio::test]
async fn long_press_shows_the_ring_then_opens_the_panel() {
    let r = rig_with(long_press_settings(), FakeSys::new("hello world"));
    r.hotkey("main");
    wait(|| r.count("overlay:ring") == 1, "overlay:ring").await;
    assert_eq!(r.count("overlay:text"), 0, "the text waits for the panel");
    wait(|| r.count("overlay:text") == 1, "panel opened").await;
    r.release("main");
    settle().await;
    assert_eq!(r.count("rewrite:start"), 0, "release after opening picks nothing");
}

#[tokio::test]
async fn password_field_is_not_read_or_pasted() {
    let sys = FakeSys::new("");
    *sys.capture.lock().unwrap() = Err(CaptureError::Password);
    let r = rig_with(base_settings(), sys);
    r.hotkey("main");
    wait(|| !r.host.toasts().is_empty(), "toast").await;
    assert_eq!(r.host.toasts(), ["Поле пароля — не читаю"]);
    assert_eq!(r.count("overlay:text"), 0);
    r.send(ControlMsg::Paste);
    settle().await;
    assert!(r.sys.pastes().is_empty());
    assert!(r.sys.clipboard.lock().unwrap().is_empty());
}

#[tokio::test]
async fn empty_field_shows_no_text() {
    let sys = FakeSys::new("");
    *sys.capture.lock().unwrap() = Err(CaptureError::NoText);
    let r = rig_with(base_settings(), sys);
    r.hotkey("main");
    wait(|| !r.host.toasts().is_empty(), "toast").await;
    assert_eq!(r.host.toasts(), ["Нет текста"]);
}

#[tokio::test]
async fn select_only_apps_read_and_paste_only_the_selection() {
    let settings = Settings { select_only_processes: vec!["Code.exe".into()], ..base_settings() };
    let sys = FakeSys::new("hello world");
    sys.foreground.lock().unwrap().exe = "Code.exe".into();
    let r = rig_with(settings, sys);
    r.open().await;
    assert!(r.sys.captures.lock().unwrap()[0].select_only);
    r.pick("format:upper").await;
    r.send(ControlMsg::Paste);
    wait(|| r.sys.pastes().len() == 1, "paste").await;
    let (_, opts) = &r.sys.pastes()[0];
    assert!(opts.select_only && !opts.prefer_uia);
}

#[tokio::test]
async fn paste_follows_the_restore_clipboard_setting() {
    for restore in [true, false] {
        let settings = Settings { restore_clipboard: restore, ..base_settings() };
        let r = rig_with(settings, FakeSys::new("hello world"));
        r.open().await;
        r.pick("format:upper").await;
        r.send(ControlMsg::Paste);
        wait(|| r.sys.pastes().len() == 1, "paste").await;
        assert_eq!(r.sys.pastes()[0].1.restore_clipboard, restore);
    }
}

#[tokio::test]
async fn missing_key_reports_an_error() {
    let mut sys = FakeSys::new("hi");
    sys.gemini_key = None;
    let r = rig_with(base_settings(), sys);
    r.open().await;
    r.send(ControlMsg::SelectStyle("polite".into()));
    wait(|| r.count("rewrite:error") == 1, "rewrite:error").await;
    assert_eq!(r.host.events("rewrite:error")[0]["message"], AiError::NoApiKey.user_message());
    assert_eq!(r.sys.rewrites.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn layout_hotkey_fixes_and_pastes_without_the_panel() {
    let r = rig("ghbdtn");
    r.hotkey("layout");
    wait(|| r.sys.pastes().len() == 1, "paste").await;
    assert_eq!(r.sys.pastes()[0].0, "привет");
    assert_eq!(r.count("overlay:text"), 0);
}
