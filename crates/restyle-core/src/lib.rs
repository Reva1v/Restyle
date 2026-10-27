//! Restyle core: everything that does not need Tauri.
//!
//! Used by the Restyle app (`src-tauri`) and by Droplet Island (git dependency
//! pinned by `rev`). Rule: no `tauri` here; the apps own windows, tray, IPC
//! commands and the keyboard hook.

// Code moved from the 2021-edition app as is; let chains would only reshuffle it.
#![allow(clippy::collapsible_if)]

pub mod ai;
pub mod capture;
pub mod history;
pub mod hotkey;
pub mod screenshot;
pub mod secrets;
pub mod settings;
pub mod textfx;
pub mod translate;
