//! Трей: меню, подменю «Последние переписывания», реакция на пункты.
//!
//! Меню в Tauri v2 статично после сборки, поэтому подменю истории живёт в
//! state (`TrayMenuItems`) и перестраивается целиком при изменении истории
//! (`refresh_history`). Операции над пунктами сами прыгают в главный поток,
//! причём при вызове ИЗ главного потока выполняются на месте — значит
//! обновлять можно и из control-цикла, и из обработчика клика.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::control::ControlMsg;
use crate::history::{self, HistoryItem, TRAY_MAX};
use crate::settings::Settings;
use crate::CtrlTx;

/// `hist:<номер>` — скопировать результат записи (0 = самая свежая).
const HISTORY_PREFIX: &str = "hist:";
const HISTORY_CLEAR: &str = "history-clear";
const HISTORY_EMPTY: &str = "history-empty";

/// Пункты, которые надо менять после сборки меню (живут в state приложения).
pub struct TrayMenuItems {
    pub autostart: CheckMenuItem<Wry>,
    undo: MenuItem<Wry>,
    history: Submenu<Wry>,
}

pub fn build(app: &tauri::App, settings: &Settings) -> tauri::Result<()> {
    let settings_item = MenuItem::with_id(app, "settings", "Настройки", true, None::<&str>)?;
    let history = Submenu::with_id(app, "history", "Последние переписывания", true)?;
    let undo_item = MenuItem::with_id(app, "undo", "Вернуть предыдущий текст", false, None::<&str>)?;
    let autostart_item = CheckMenuItem::with_id(
        app,
        "autostart",
        "Автозапуск",
        true,
        settings.autostart,
        None::<&str>,
    )?;
    let quit_item = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[&settings_item, &history, &undo_item, &sep, &autostart_item, &quit_item],
    )?;
    fill_history(app.handle(), &history, &[])?;
    app.manage(TrayMenuItems {
        autostart: autostart_item,
        undo: undo_item,
        history,
    });
    TrayIconBuilder::with_id("restyle-tray")
        .tooltip("Restyle")
        .icon(app.default_window_icon().expect("иконка в bundle").clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .build(app)?;
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id.as_ref();
    match id {
        "quit" => app.exit(0),
        "settings" => crate::open_settings(app),
        "undo" => send(app, ControlMsg::Undo),
        HISTORY_CLEAR => send(app, ControlMsg::ClearHistory),
        "autostart" => {
            // клик уже перещёлкнул галку — фиксируем в конфиге
            let mut s = Settings::load(app);
            s.autostart = !s.autostart;
            s.store(app);
            crate::apply_autostart(app, s.autostart);
            crate::sync_tray_autostart(app, s.autostart);
            let _ = app.emit("settings-changed", &s);
        }
        _ => {
            if let Some(n) = id.strip_prefix(HISTORY_PREFIX).and_then(|n| n.parse::<usize>().ok()) {
                send(app, ControlMsg::CopyHistory(n));
            }
        }
    }
}

fn send(app: &AppHandle, msg: ControlMsg) {
    if let Some(tx) = app.try_state::<CtrlTx>() {
        let _ = tx.0.send(msg);
    }
}

/// Перестроить подменю истории. Зовётся из control-цикла после любых
/// изменений истории; ошибки меню не фатальны — пишем в stderr.
pub fn refresh_history(app: &AppHandle, items: &[HistoryItem]) {
    let Some(state) = app.try_state::<TrayMenuItems>() else { return };
    let _ = state.undo.set_enabled(!items.is_empty());
    if let Err(e) = fill_history(app, &state.history, items) {
        eprintln!("[restyle] подменю истории: {e}");
    }
}

fn fill_history(app: &AppHandle, sub: &Submenu<Wry>, items: &[HistoryItem]) -> tauri::Result<()> {
    while sub.remove_at(0)?.is_some() {}
    if items.is_empty() {
        let empty = MenuItem::with_id(app, HISTORY_EMPTY, "Пока пусто", false, None::<&str>)?;
        sub.append(&empty)?;
        return Ok(());
    }
    let styles = Settings::load(app).styles;
    for (i, it) in items.iter().take(TRAY_MAX).enumerate() {
        let style = styles
            .iter()
            .find(|s| s.id == it.style_id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| it.style_id.clone());
        // Клик кладёт результат в буфер: окно, куда вставляли, давно могло
        // закрыться, и вставлять вслепую нельзя.
        let label = format!("{style} · {}", history::preview(&it.result, 42));
        let item = MenuItem::with_id(app, format!("{HISTORY_PREFIX}{i}"), label, true, None::<&str>)?;
        sub.append(&item)?;
    }
    let sep = PredefinedMenuItem::separator(app)?;
    let clear = MenuItem::with_id(app, HISTORY_CLEAR, "Очистить историю", true, None::<&str>)?;
    sub.append(&sep)?;
    sub.append(&clear)?;
    log_items(sub);
    Ok(())
}

/// Читаем меню обратно: видно, что подменю реально наполнено (проверка фазы 6).
fn log_items(sub: &Submenu<Wry>) {
    let Ok(items) = sub.items() else { return };
    let texts: Vec<String> = items
        .iter()
        .filter_map(|i| i.as_menuitem().map(|m| m.text().unwrap_or_default()))
        .collect();
    println!("[restyle] подменю истории: {}", texts.join(" | "));
}
