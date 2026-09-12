//! Типы IPC-границы (события backend → frontend). TS-типы генерируются ts-rs
//! в src/bindings/ (`cargo test export_bindings`).
//! serde(rename_all = "camelCase") везде: фронт видит camelCase.
//!
//! События:
//! - `overlay:show` — `ShowPayload`
//! - `overlay:hide` — null
//! - `overlay:key`  — vk-код клавиши оверлея (Enter/Tab/R/стрелки/1-9)
//! - `overlay:text` — `TextPayload` (захваченный исходный текст)
//! - `overlay:screenshot` — `ScreenshotPayload` (метаданные готового JPEG)
//! - `settings-changed` — `Settings`
//! - `rewrite:start` — `RewriteStart` (запрос ушёл)
//! - `rewrite:chunk` — `RewriteChunk` (кусок текста)
//! - `rewrite:done`  — `RewriteDone` (полный текст)
//! - `rewrite:error` — `RewriteError` (человекочитаемое сообщение)
//! Во всех rewrite-событиях `gen` — поколение сессии оверлея: фронт
//! игнорирует чужие.

use serde::Serialize;
use ts_rs::TS;

/// Payload событий `rewrite:*`.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/bindings/")]
pub struct RewriteStart {
    #[ts(type = "number")]
    pub gen: u64,
    pub style_id: String,
    pub with_screenshot: bool,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/bindings/")]
pub struct RewriteChunk {
    #[ts(type = "number")]
    pub gen: u64,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/bindings/")]
pub struct RewriteDone {
    #[ts(type = "number")]
    pub gen: u64,
    pub text: String,
    pub elapsed_ms: f32,
    pub first_chunk_ms: f32,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/bindings/")]
pub struct RewriteError {
    #[ts(type = "number")]
    pub gen: u64,
    pub message: String,
}

/// Payload события `overlay:show`.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ShowPayload {
    /// Поколение сессии — для корреляции с `rewrite:*`.
    #[ts(type = "number")]
    pub gen: u64,
    pub x: i32,
    pub y: i32,
    pub dpi_scale: f32,
    /// Быстрый стиль, если оверлей открыт его хоткеем (генерация стартует сразу).
    pub style_id: Option<String>,
    /// Режим тоста: короткое сообщение, оверлей сам спрячется.
    pub toast: Option<String>,
    /// Захват текста ещё идёт — `overlay:text` придёт позже.
    pub capturing: bool,
    /// Имя exe окна, в котором был курсор.
    pub target_exe: String,
    /// "pending" (кодируется, придёт `overlay:screenshot`) | "off" | "excluded" | "failed"
    pub screenshot: String,
}

/// Payload события `overlay:screenshot` (сами байты во фронтенд не идут).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ScreenshotPayload {
    pub width: u32,
    pub height: u32,
    pub bytes: u32,
    pub capture_ms: f32,
    pub encode_ms: f32,
}

/// Payload события `overlay:text`.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/bindings/")]
pub struct TextPayload {
    pub text: String,
    /// "uia" | "clipboard"
    pub source: String,
    pub selection_only: bool,
    /// Запись назад через UIA возможна (иначе вставка клипбордом).
    pub uia_writable: bool,
    pub elapsed_ms: f32,
}
