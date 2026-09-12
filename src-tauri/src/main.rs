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
mod tray;
mod window;

use tauri::{Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tokio::sync::mpsc::UnboundedSender;

use control::ControlMsg;
use history::HistoryItem;
use settings::Settings;
use tray::TrayMenuItems;

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
        let _ = tx.0.send(ControlMsg::SetHistoryPersist(settings.history_to_disk));
        // имена стилей видны в подменю трея
        let _ = tx.0.send(ControlMsg::RefreshTray);
    }
    apply_autostart(&app, settings.autostart);
    sync_tray_autostart(&app, settings.autostart);
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

/// Акриловый тинт оверлея: тёмный/светлый (RGBA). Ошибка не фатальна — CSS-фолбэк.
fn apply_overlay_tint(overlay: &tauri::WebviewWindow, dark: bool) {
    let tint = if dark { (18, 22, 27, 200) } else { (246, 248, 250, 220) };
    if let Err(e) = window_vibrancy::apply_acrylic(overlay, Some(tint)) {
        eprintln!("[restyle] acrylic unavailable, CSS fallback: {e}");
    }
}

#[tauri::command]
fn set_overlay_tint(app: tauri::AppHandle, dark: bool) {
    if let Some(overlay) = app.get_webview_window("overlay") {
        apply_overlay_tint(&overlay, dark);
    }
}

/// Реестр HKCU\...\Run через плагин; ошибки не фатальны (пишем в stderr).
pub(crate) fn apply_autostart(app: &tauri::AppHandle, on: bool) {
    let mgr = app.autolaunch();
    let res = if on { mgr.enable() } else { mgr.disable() };
    match res {
        Ok(()) => {}
        // disable() падает, если записи и так нет — это не ошибка
        Err(_) if !on => {}
        Err(e) => eprintln!("[restyle] автозапуск: {e}"),
    }
}

pub(crate) fn sync_tray_autostart(app: &tauri::AppHandle, on: bool) {
    let handle = app.clone();
    // set_checked обязан выполняться на главном потоке
    let _ = app.run_on_main_thread(move || {
        if let Some(items) = handle.try_state::<TrayMenuItems>() {
            let _ = items.autostart.set_checked(on);
        }
    });
}

/// Окно настроек: создаём по требованию, повторный вызов — фокус.
pub(crate) fn open_settings(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    // Заглушка до дизайн-макета: системные декорации.
    let r = tauri::WebviewWindowBuilder::new(
        app,
        "settings",
        tauri::WebviewUrl::App("settings.html".into()),
    )
    .title("Restyle — настройки")
    .inner_size(620.0, 760.0)
    .min_inner_size(480.0, 480.0)
    .resizable(true)
    .maximizable(false)
    .center()
    .build();
    if let Err(e) = r {
        eprintln!("[restyle] окно настроек: {e}");
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
            // Тинт по сохранённой теме; для "system" стартуем тёмным,
            // фронтенд оверлея уточнит через set_overlay_tint.
            apply_overlay_tint(&overlay, settings.theme != "light");

            // --- потоки и каналы ---
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ControlMsg>();
            let capture = capture::spawn();
            hotkey::spawn(tx.clone(), settings.combos());
            control::spawn(rx, tx.clone(), app.handle().clone(), hwnd, capture);
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

            // --- автозапуск: реестр = настройки (правда в store) ---
            apply_autostart(app.handle(), settings.autostart);

            // --- трей ---
            tray::build(app, &settings)?;
            // история уже прочитана с диска — наполнить подменю
            let _ = tx.send(ControlMsg::RefreshTray);

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
            get_settings,
            save_settings,
            hotkey_conflicts,
            default_styles,
            get_history,
            copy_history,
            clear_history,
            set_overlay_tint,
        ])
        .run(tauri::generate_context!())
        .expect("ошибка запуска tauri");
}
