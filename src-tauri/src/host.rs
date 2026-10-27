//! `Host` сессии (`restyle_core::session`) на Tauri: события — `Emitter::emit`,
//! окна оверлея и меню — WinAPI на главном потоке (`run_on_main_thread`, окна
//! принадлежат ему), клавиши панели и меню — свой hotkey-поток.

use std::path::PathBuf;

use restyle_core::ai::BoxFuture;
use restyle_core::session::{Host, PxRect, Surface};
use serde_json::value::RawValue;
use tauri::{AppHandle, Emitter, Manager};

use crate::settings::{self, Settings};
use crate::{hotkey, window};

pub struct TauriHost {
    app: AppHandle,
    /// HWND окна оверлея (панель, тост, кольцо).
    panel: isize,
    /// HWND своего меню трея (живёт скрытым).
    menu: isize,
}

impl TauriHost {
    pub fn new(app: AppHandle, panel: isize, menu: isize) -> Self {
        Self { app, panel, menu }
    }

    fn hwnd(&self, s: Surface) -> isize {
        match s {
            Surface::Panel => self.panel,
            Surface::Menu => self.menu,
        }
    }
}

impl Host for TauriHost {
    fn emit(&self, event: &str, payload: &RawValue) {
        let _ = self.app.emit(event, payload);
    }

    fn show_panel(&self, surface: Surface, r: PxRect) {
        let hwnd = self.hwnd(surface);
        let _ = self.app.run_on_main_thread(move || window::show_at(hwnd, r.x, r.y, r.w, r.h));
    }

    fn move_panel(&self, surface: Surface, r: PxRect) {
        let hwnd = self.hwnd(surface);
        let _ = self.app.run_on_main_thread(move || window::move_to(hwnd, r.x, r.y, r.w, r.h));
    }

    fn hide_panel(&self, surface: Surface) {
        let hwnd = self.hwnd(surface);
        let _ = self.app.run_on_main_thread(move || window::hide(hwnd));
    }

    fn panel_keys(&self, on: bool) {
        hotkey::set_panel_visible(on);
    }

    fn menu_keys(&self, rect: Option<PxRect>) {
        match rect {
            Some(r) => hotkey::set_menu_visible(true, (r.x, r.y, r.w, r.h)),
            None => hotkey::set_menu_visible(false, (0, 0, 0, 0)),
        }
    }

    fn settings(&self) -> Settings {
        settings::load(&self.app)
    }

    fn config_dir(&self) -> Option<PathBuf> {
        self.app.path().app_config_dir().ok()
    }

    fn spawn(&self, f: BoxFuture<'static, ()>) {
        tauri::async_runtime::spawn(f);
    }

    fn tick(&self) {
        hotkey::watchdog_check();
    }
}
