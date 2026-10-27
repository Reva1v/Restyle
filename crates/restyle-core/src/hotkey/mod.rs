//! Модель сочетаний клавиш (без хука). Сам `WH_KEYBOARD_LL` живёт в приложении:
//! у Restyle — свой поток (`src-tauri/src/hotkey`), у острова — `island_hotkey`.

pub mod combo;

/// Идентификаторы комбинаций в `ComboSet`. Стили — `style:<id>`.
pub const COMBO_MAIN: &str = "main";
pub const COMBO_UNDO: &str = "undo";
/// Исправить раскладку («ghbdtn» → «привет»).
pub const COMBO_LAYOUT: &str = "layout";
pub const COMBO_STYLE_PREFIX: &str = "style:";
