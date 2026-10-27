//! Настройки: тип, чтение и запись файла — в `restyle_core::settings` (общие с
//! островом). Здесь только каталог от Tauri: `app_data_dir` — тот же путь, куда
//! писал `tauri-plugin-store` (`%APPDATA%\dev.reva1v.restyle\settings.json`).

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

pub use restyle_core::settings::*;

/// Каталог `settings.json` и `history.json`.
pub fn dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok()
}

/// Настройки с диска (нормализованные); нет каталога — дефолт.
pub fn load(app: &AppHandle) -> Settings {
    match dir(app) {
        Some(d) => Settings::load(&d),
        None => Settings::default(),
    }
}

/// Записать настройки. Ошибки — в stderr.
pub fn store(app: &AppHandle, s: &Settings) {
    match dir(app) {
        Some(d) => {
            if let Err(e) = s.save(&d) {
                eprintln!("[restyle] не удалось сохранить настройки: {e}");
            }
        }
        None => eprintln!("[restyle] каталог настроек недоступен"),
    }
}
