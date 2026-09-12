//! Restyle — переписывание набранного текста через AI в любом приложении.
//! Фаза 1: скелет — трей, оверлей-окно без кражи фокуса, hotkey-хук,
//! настройки, single instance, автозапуск.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod control;
mod ai;
mod history;
mod hotkey;
mod ipc;
mod screenshot;
mod secrets;
mod settings;
mod textfx;
mod translate;
mod tray;
mod window;

use tauri::{Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tokio::sync::mpsc::UnboundedSender;

use control::ControlMsg;
use history::HistoryItem;
use settings::Settings;

/// Канал в control-цикл; тела команд — только `send`, возврат сразу.
struct CtrlTx(UnboundedSender<ControlMsg>);

#[tauri::command]
fn panel_shown(tx: State<CtrlTx>) {
    let _ = tx.0.send(ControlMsg::FrontendShown);
}

#[tauri::command]
fn frontend_ready(tx: State<CtrlTx>) {
    let _ = tx.0.send(ControlMsg::FrontendReady);
}

#[tauri::command]
fn hide_overlay(tx: State<CtrlTx>) {
    let _ = tx.0.send(ControlMsg::Hide);
}

#[tauri::command]
fn select_style(tx: State<CtrlTx>, style_id: String) {
    let _ = tx.0.send(ControlMsg::SelectStyle(style_id));
}

#[tauri::command]
fn regenerate(tx: State<CtrlTx>) {
    let _ = tx.0.send(ControlMsg::Regenerate);
}

#[tauri::command]
fn paste_result(tx: State<CtrlTx>) {
    let _ = tx.0.send(ControlMsg::Paste);
}

#[tauri::command]
fn undo_last(tx: State<CtrlTx>) {
    let _ = tx.0.send(ControlMsg::Undo);
}

// --- API-ключ: только keyring, во фронтенд не возвращается ---

#[tauri::command]
async fn set_api_key(key: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || secrets::set_api_key(&key))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn has_api_key() -> bool {
    tokio::task::spawn_blocking(secrets::has_api_key).await.unwrap_or(false)
}

#[tauri::command]
async fn clear_api_key() -> Result<(), String> {
    tokio::task::spawn_blocking(secrets::clear_api_key).await.map_err(|e| e.to_string())?
}

/// Окно настроек захватывает новую комбинацию: хук не должен срабатывать.
#[tauri::command]
fn set_hotkey_capture(on: bool) {
    hotkey::set_suspended(on);
}

/// Открыть ссылку в системном браузере. `target=_blank` в WebView2 открыл бы
/// новое окно вебвью, а не браузер. Только https и только известные сайты —
/// команда доступна любому окну приложения.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    const ALLOWED: &[&str] = &["https://github.com/", "https://www.deepl.com/", "https://aistudio.google.com/"];
    if !ALLOWED.iter().any(|p| url.starts_with(p)) {
        return Err("ссылка не из списка разрешённых".into());
    }
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(url.as_str()), None, None, SW_SHOWNORMAL) };
    // ShellExecuteW: значения > 32 — успех.
    if r.0 as isize > 32 { Ok(()) } else { Err(format!("ShellExecute: {}", r.0 as isize)) }
}

/// Оверлей померил своё содержимое — окно подгоняется по высоте.
#[tauri::command]
fn resize_overlay(tx: State<CtrlTx>, height: f32) {
    let _ = tx.0.send(ControlMsg::Resize(height));
}

/// Тост померил карточку — окно ужимается ровно под неё (иначе вокруг
/// карточки виден акриловый фон окна).
#[tauri::command]
fn resize_toast(tx: State<CtrlTx>, width: f32, height: f32) {
    let _ = tx.0.send(ControlMsg::ResizeToast(width, height));
}

/// Вернуть текст конкретной записи истории (окно истории).
#[tauri::command]
fn undo_entry(tx: State<CtrlTx>, index: usize) {
    let _ = tx.0.send(ControlMsg::UndoEntry(index));
}

// --- окна и их заголовки (декораций нет, шапка своя — как в макете) ---

// Команды async специально: синхронная команда выполняется в главном потоке,
// а `WebviewWindowBuilder::build()` из главного потока встаёт намертво (окно
// создаётся событийным циклом, которого мы же и ждём) — вебвью остаётся белым.
#[tauri::command]
async fn open_settings_window(app: tauri::AppHandle) {
    open_settings(&app);
}

#[tauri::command]
async fn open_history_window(app: tauri::AppHandle) {
    open_history(&app);
}

#[tauri::command]
async fn win_minimize(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
async fn win_close(window: tauri::Window) {
    let _ = window.close();
}

/// Состояние для мастера первого запуска: стоит ли хук.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Diagnostics {
    hook_installed: bool,
}

#[tauri::command]
fn diagnostics() -> Diagnostics {
    Diagnostics { hook_installed: hotkey::is_installed() }
}

/// Проверка ключа тестовым запросом (мастер первого запуска). Ключ приходит
/// из поля ввода и в keyring не попадает — сохраняет его отдельная команда.
#[tauri::command]
async fn test_api_key(app: tauri::AppHandle, key: String) -> Result<(), String> {
    let settings = Settings::load(&app);
    let req = ai::RewriteRequest {
        system_prompt: "You are a test probe. Answer with a single word.".into(),
        style_name: "Ping".into(),
        style_instruction: "Reply with the single word OK.".into(),
        text: "ping".into(),
        screenshot_jpeg_base64: None,
        model: settings.model.clone(),
    };
    let provider = ai::provider_for(&req.model);
    let (chunks, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let out = provider.rewrite(&key.trim().to_string(), &req, chunks).await;
    let _ = drain.await;
    out.map(|_| ()).map_err(|e| e.user_message())
}

/// Каталог моделей ключа: ключи видят разный набор, поэтому список тянем у
/// API. Ключ берём из keyring — во фронтенд он по-прежнему не уходит.
#[tauri::command]
async fn list_models() -> Result<Vec<ai::ModelInfo>, String> {
    let key = tokio::task::spawn_blocking(secrets::get_api_key)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e)?
        .ok_or_else(|| ai::AiError::NoApiKey.user_message())?;
    let list = ai::gemini::Gemini::new()
        .list_models(&key)
        .await
        .map_err(|e| e.user_message())?;
    println!("[restyle] каталог моделей: {} шт.", list.len());
    Ok(list)
}

/// Запущенные приложения с окнами — для списков «не снимать» и «только
/// выделение»: вводить имена exe руками неудобно и легко ошибиться.
#[tauri::command]
async fn running_apps() -> Vec<capture::process::RunningApp> {
    tokio::task::spawn_blocking(capture::process::running_apps)
        .await
        .unwrap_or_default()
}

/// Ключ DeepL: как и ключ Gemini, живёт только в Credential Manager.
#[tauri::command]
async fn set_deepl_key(key: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || secrets::set_deepl_key(&key))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn has_deepl_key() -> bool {
    tokio::task::spawn_blocking(secrets::has_deepl_key).await.unwrap_or(false)
}

#[tauri::command]
async fn clear_deepl_key() -> Result<(), String> {
    tokio::task::spawn_blocking(secrets::clear_deepl_key)
        .await
        .map_err(|e| e.to_string())?
}

/// Проверка ключа DeepL + остаток квоты. Ключ приходит из поля ввода
/// (проверяем до сохранения) либо берётся из keyring, если поле пустое.
#[tauri::command]
async fn test_deepl_key(key: String) -> Result<translate::DeeplUsage, String> {
    let key = key.trim().to_string();
    let key = if key.is_empty() {
        tokio::task::spawn_blocking(secrets::get_deepl_key)
            .await
            .map_err(|e| e.to_string())??
            .ok_or_else(|| "Ключ DeepL не задан".to_string())?
    } else {
        key
    };
    translate::usage(&key).await.map_err(|e| e.user_message())
}

/// Выход из трея (своё меню).
#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

/// Пункт меню выбран/клик мимо — спрятать меню.
#[tauri::command]
fn close_tray_menu(app: tauri::AppHandle) {
    if let Some(tx) = app.try_state::<CtrlTx>() {
        let _ = tx.0.send(ControlMsg::CloseTrayMenu);
    }
}

/// Меню померило содержимое — подогнать высоту окна.
#[tauri::command]
fn open_format_menu(app: tauri::AppHandle, left: f32, top: f32, bottom: f32, current: String) {
    if let Some(tx) = app.try_state::<CtrlTx>() {
        let _ = tx.0.send(ControlMsg::FormatMenu { left, top, bottom, current });
    }
}

#[tauri::command]
fn resize_menu(app: tauri::AppHandle, height: f32) {
    if let Some(tx) = app.try_state::<CtrlTx>() {
        let _ = tx.0.send(ControlMsg::ResizeMenu(height));
    }
}

/// Мастер пройден (или пропущен) — больше не показываем.
#[tauri::command]
async fn finish_onboarding(app: tauri::AppHandle, window: tauri::Window) {
    let mut s = Settings::load(&app);
    s.onboarded = true;
    s.store(&app);
    let _ = app.emit("settings-changed", &s);
    let _ = window.close();
}

/// Автозапуск: плагин пишет ключ в реестре. Правда — в настройках, реестр
/// лишь приводится к ней (пользователь мог снять галку в Диспетчере задач).
fn apply_autostart(app: &tauri::AppHandle, on: bool) {
    let manager = app.autolaunch();
    let r = if on { manager.enable() } else { manager.disable() };
    if let Err(e) = r {
        eprintln!("[restyle] автозапуск: {e}");
    }
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Settings {
    Settings::load(&app)
}

/// Сохранение из окна настроек. Всё, что пришло с фронта, сначала
/// нормализуется (id стилей, пустые строки, комбинации без модификатора),
/// потом применяется: store → хук → история → автозапуск → событие.
/// Возвращает нормализованные настройки, чтобы окно показало, что реально легло.
#[tauri::command]
fn save_settings(app: tauri::AppHandle, mut settings: Settings) -> Settings {
    settings.normalize();
    settings.store(&app);
    hotkey::set_combos(settings.combos());
    for (a, b) in settings.hotkey_conflicts() {
        eprintln!("[restyle] конфликт хоткеев: {a} и {b} — сработает только {a}");
    }
    if let Some(tx) = app.try_state::<CtrlTx>() {
        let _ = tx.0.send(ControlMsg::HistoryConfig {
            persist: settings.history_to_disk,
            limit: settings.history_limit,
        });
        // имена стилей видны в подменю трея
        let _ = tx.0.send(ControlMsg::RefreshTray);
    }
    apply_autostart(&app, settings.autostart);
    // Оба окна (оверлей и настройки) подхватывают изменения из этого события.
    let _ = app.emit("settings-changed", &settings);
    settings
}

/// Встроенные стили в исходном виде — кнопка «вернуть» в окне настроек.
#[tauri::command]
fn default_styles() -> Vec<settings::Style> {
    settings::builtin_styles()
}

/// Пары действий с одинаковыми комбинациями — окно настроек подсвечивает их.
#[tauri::command]
fn hotkey_conflicts(settings: Settings) -> Vec<(String, String)> {
    settings.hotkey_conflicts()
}

// --- история ---

#[tauri::command]
async fn get_history(tx: State<'_, CtrlTx>) -> Result<Vec<HistoryItem>, String> {
    let (reply, rx) = tokio::sync::oneshot::channel();
    tx.0.send(ControlMsg::GetHistory(reply)).map_err(|e| e.to_string())?;
    rx.await.map_err(|e| e.to_string())
}

#[tauri::command]
fn copy_history(tx: State<CtrlTx>, index: usize) {
    let _ = tx.0.send(ControlMsg::CopyHistory(index));
}

#[tauri::command]
fn clear_history(tx: State<CtrlTx>) {
    let _ = tx.0.send(ControlMsg::ClearHistory);
}

/// Акриловый тинт окна меню: тёмный/светлый (RGBA). Ошибка не фатальна — CSS-фолбэк.
/// Оверлею акрил не ставим: DWM рисует его на всё окно, и всё, что не закрыто
/// CSS (углы кольца удержания, поля вокруг тоста), видно серой подложкой.
/// Фон панели и тоста и так почти непрозрачный — разницы на глаз нет.
fn apply_overlay_tint(overlay: &tauri::WebviewWindow, dark: bool) {
    let tint = if dark { (18, 22, 27, 200) } else { (246, 248, 250, 220) };
    if let Err(e) = window_vibrancy::apply_acrylic(overlay, Some(tint)) {
        eprintln!("[restyle] acrylic unavailable, CSS fallback: {e}");
    }
}



/// Показать уже созданное окно (или сказать, что его нет).
fn focus_window(app: &tauri::AppHandle, label: &str) -> bool {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        bring_to_front(&w);
        return true;
    }
    false
}

/// Поверх всех приложений: окна открываются из меню трея, которое фокус не
/// берёт, и обычный `set_focus` Windows игнорирует (см. `window::force_foreground`).
fn bring_to_front(w: &tauri::WebviewWindow) {
    let Ok(h) = w.hwnd() else { return };
    let raw = h.0 as isize;
    let _ = w.run_on_main_thread(move || window::force_foreground(raw));
}

/// Окно истории переписываний (макет, раздел 04): тёмное, своя шапка.
pub(crate) fn open_history(app: &tauri::AppHandle) {
    if focus_window(app, "history") {
        return;
    }
    let r = tauri::WebviewWindowBuilder::new(app, "history", tauri::WebviewUrl::App("history.html".into()))
        .title("Restyle — История")
        .inner_size(840.0, 560.0)
        .min_inner_size(620.0, 420.0)
        .decorations(false)
        .resizable(true)
        .maximizable(false)
        .center()
        .build();
    match r {
        Ok(w) => bring_to_front(&w),
        Err(e) => eprintln!("[restyle] окно истории: {e}"),
    }
}

/// Мастер первого запуска (макет, раздел 05): три шага, без максимизации.
pub(crate) fn open_welcome(app: &tauri::AppHandle) {
    if focus_window(app, "welcome") {
        return;
    }
    let r = tauri::WebviewWindowBuilder::new(app, "welcome", tauri::WebviewUrl::App("welcome.html".into()))
        .title("Restyle")
        .inner_size(560.0, 540.0)
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .center()
        .build();
    match r {
        // Мастер сам ставит поле хоткея в режим захвата — пауза не должна пережить окно.
        Ok(w) => w.on_window_event(|e| {
            if matches!(e, tauri::WindowEvent::Destroyed | tauri::WindowEvent::CloseRequested { .. }) {
                hotkey::set_suspended(false);
            }
        }),
        Err(e) => eprintln!("[restyle] окно мастера: {e}"),
    }
}

/// Окно настроек: создаём по требованию, повторный вызов — фокус.
pub(crate) fn open_settings(app: &tauri::AppHandle) {
    if focus_window(app, "settings") {
        return;
    }
    // Заглушка до дизайн-макета: системные декорации.
    let r = tauri::WebviewWindowBuilder::new(
        app,
        "settings",
        tauri::WebviewUrl::App("settings.html".into()),
    )
    .title("Restyle — Настройки")
    // Обработчик перетаскивания файлов Tauri съедает HTML5 drag&drop внутри
    // страницы: без этого порядок стилей мышью не потаскать.
    .disable_drag_drop_handler()
    .inner_size(840.0, 620.0)
    // Ниже 820 три колонки «Стили» начинают тесниться, ниже 540 обрезается
    // редактор: не даём окну складываться так, чтобы что-то липло к краю.
    .min_inner_size(820.0, 540.0)
    .decorations(false)
    .resizable(true)
    .maximizable(false)
    .center()
    .build();
    match r {
        // Закрытие крестиком (и перезагрузка вебвью) сносит фронтенд без
        // размонтирования React: если в этот момент поле хоткея было в режиме
        // захвата, хук так и остался бы приостановленным до перезапуска.
        Ok(w) => {
            bring_to_front(&w);
            w.on_window_event(|e| {
                if matches!(e, tauri::WindowEvent::Destroyed | tauri::WindowEvent::CloseRequested { .. }) {
                    hotkey::set_suspended(false);
                }
            })
        }
        Err(e) => eprintln!("[restyle] окно настроек: {e}"),
    }
}

fn main() {
    tauri::Builder::default()
        // Вторая копия не стартует; вместо неё открываем настройки в живой
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            open_settings(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(|app| {
            let settings = Settings::load(app.handle());

            // --- оверлей: создан конфигом один раз, живёт скрытым ---
            let overlay = app
                .get_webview_window("overlay")
                .expect("окно overlay объявлено в tauri.conf.json");
            let hwnd = overlay.hwnd()?.0 as isize;
            window::apply_overlay_styles(hwnd);
            window::clear_dwm_frame(hwnd);

            // Меню трея — такое же окно без фокуса, создано конфигом.
            let menu_win = app
                .get_webview_window("menu")
                .expect("окно menu объявлено в tauri.conf.json");
            let menu_hwnd = menu_win.hwnd()?.0 as isize;
            window::apply_overlay_styles(menu_hwnd);
            apply_overlay_tint(&menu_win, settings.theme != "light");

            // --- потоки и каналы ---
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ControlMsg>();
            let capture = capture::spawn();
            hotkey::spawn(tx.clone(), settings.combos());
            control::spawn(rx, tx.clone(), app.handle().clone(), hwnd, menu_hwnd, capture);
            app.manage(CtrlTx(tx.clone()));

            // Демо-режим для проверки без клавиатуры:
            // RESTYLE_DEMO=<мс>[:<style_id>] — через задержку сработать, как будто
            // нажат основной (или быстрый) хоткей; RESTYLE_DEMO_HIDE=<мс> — затем
            // спрятать оверлей (эмуляция Esc, проверка отмены запроса).
            if let Ok(v) = std::env::var("RESTYLE_DEMO") {
                let (delay, style) = match v.split_once(':') {
                    Some((d, s)) => (d.parse::<u64>().unwrap_or(1500), Some(s.to_string())),
                    None => (v.parse::<u64>().unwrap_or(1500), None),
                };
                let hide_after = std::env::var("RESTYLE_DEMO_HIDE").ok().and_then(|h| h.parse::<u64>().ok());
                let demo_tx = tx.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    let id = match style {
                        Some(s) => format!("{}{s}", hotkey::COMBO_STYLE_PREFIX),
                        None => hotkey::COMBO_MAIN.into(),
                    };
                    let _ = demo_tx.send(ControlMsg::Hotkey(hotkey::HotkeyEvent::Combo {
                        id,
                        at: std::time::Instant::now(),
                    }));
                    if let Some(h) = hide_after {
                        tokio::time::sleep(std::time::Duration::from_millis(h)).await;
                        let _ = demo_tx.send(ControlMsg::Hide);
                    }
                    // RESTYLE_DEMO_SELECT=<мс>:<style_id> — выбрать стиль в открытой панели
                    // (инжектированные клавиши хук не видит, автотесту иначе не кликнуть).
                    if let Some(v) = std::env::var("RESTYLE_DEMO_SELECT").ok() {
                        if let Some((ms, id)) = v.split_once(':') {
                            if let Ok(ms) = ms.parse::<u64>() {
                                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                                let _ = demo_tx.send(ControlMsg::SelectStyle(id.to_string()));
                            }
                        }
                    }
                    // RESTYLE_DEMO_RELEASE=<мс> — отпустить основную комбинацию:
                    // развилка «короткое/долгое» иначе не проверяется, свои
                    // SendInput хук игнорирует (LLKHF_INJECTED).
                    if let Some(r) = std::env::var("RESTYLE_DEMO_RELEASE").ok().and_then(|v| v.parse::<u64>().ok()) {
                        tokio::time::sleep(std::time::Duration::from_millis(r)).await;
                        let _ = demo_tx.send(ControlMsg::Hotkey(hotkey::HotkeyEvent::ComboReleased {
                            id: hotkey::COMBO_MAIN.into(),
                        }));
                    }
                    // RESTYLE_DEMO_PASTE=<мс> — Enter; RESTYLE_DEMO_UNDO=<мс> — undo (после paste)
                    if let Some(p) = std::env::var("RESTYLE_DEMO_PASTE").ok().and_then(|v| v.parse::<u64>().ok()) {
                        tokio::time::sleep(std::time::Duration::from_millis(p)).await;
                        let _ = demo_tx.send(ControlMsg::Paste);
                    }
                    if let Some(u) = std::env::var("RESTYLE_DEMO_UNDO").ok().and_then(|v| v.parse::<u64>().ok()) {
                        tokio::time::sleep(std::time::Duration::from_millis(u)).await;
                        let _ = demo_tx.send(ControlMsg::Undo);
                    }
                });
            }

            // RESTYLE_DEMO_MENU=<мс> — открыть меню трея: кликнуть по иконке
            // в области уведомлений автотестом нельзя.
            if let Some(m) = std::env::var("RESTYLE_DEMO_MENU").ok().and_then(|v| v.parse::<u64>().ok()) {
                let menu_tx = tx.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(m)).await;
                    let _ = menu_tx.send(ControlMsg::TrayMenu);
                });
            }

            // --- автозапуск: реестр = настройки (правда в store) ---
            apply_autostart(app.handle(), settings.autostart);

            // Первый запуск (ключа нет и мастер не пройден) — открываем мастер,
            // иначе приложение молча уходит в трей и выглядит незапустившимся.
            if !settings.onboarded {
                open_welcome(app.handle());
            }

            // --- трей ---
            tray::build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            panel_shown,
            frontend_ready,
            hide_overlay,
            select_style,
            regenerate,
            paste_result,
            undo_last,
            set_api_key,
            has_api_key,
            clear_api_key,
            set_hotkey_capture,
            resize_overlay,
            resize_toast,
            open_url,
            undo_entry,
            open_settings_window,
            open_history_window,
            win_minimize,
            win_close,
            diagnostics,
            test_api_key,
            finish_onboarding,
            get_settings,
            save_settings,
            hotkey_conflicts,
            default_styles,
            get_history,
            copy_history,
            clear_history,
            list_models,
            running_apps,
            set_deepl_key,
            has_deepl_key,
            clear_deepl_key,
            test_deepl_key,
            exit_app,
            close_tray_menu,
            resize_menu,
            open_format_menu,
        ])
        .run(tauri::generate_context!())
        .expect("ошибка запуска tauri");
}
