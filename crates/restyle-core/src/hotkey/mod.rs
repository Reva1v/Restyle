//! Модель сочетаний клавиш (без хука). Сам `WH_KEYBOARD_LL` живёт в приложении:
//! у Restyle — свой поток (`src-tauri/src/hotkey`), у острова — `island_hotkey`.

use std::time::Instant;

pub mod combo;

pub use crate::settings::HotkeyCombo;

/// Идентификаторы комбинаций в `ComboSet`. Стили — `style:<id>`.
pub const COMBO_MAIN: &str = "main";
pub const COMBO_UNDO: &str = "undo";
/// Исправить раскладку («ghbdtn» → «привет»).
pub const COMBO_LAYOUT: &str = "layout";
pub const COMBO_STYLE_PREFIX: &str = "style:";

/// События hotkey-потока для control-цикла.
#[derive(Debug)]
pub enum HotkeyEvent {
    /// Сработала комбинация `id` (см. константы выше); `at` — момент события
    /// в колбэке хука (точка отсчёта бюджета 100 мс до оверлея).
    Combo { id: String, at: Instant },
    /// Комбинацию `id` отпустили. Для основной комбинации это развилка
    /// «короткое нажатие» / «удержание»: см. `session.rs`.
    ComboReleased { id: String },
    /// Клавиша оверлея при видимой панели (проглочена хуком): Enter/Esc/Tab/R/стрелки/1-9.
    PanelKey(u32),
}
