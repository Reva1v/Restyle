//! История переписываний: последние `MAX` пар исходник/результат в памяти,
//! опционально на диске (`<config>/history.json`, настройка `historyToDisk`).
//! Undo берёт последнюю запись; повторный undo возвращает результат обратно.

use std::collections::VecDeque;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::capture::text::LineEnding;

/// Значение лимита по умолчанию (настройка `historyLimit`).
pub const MAX: usize = 20;
/// Потолок для настройки: больше держать в памяти незачем.
pub const HARD_MAX: usize = 100;
/// Сколько пунктов показывать в подменю трея.
/// Сколько записей показывает меню трея (остальное — в окне истории).
#[allow(dead_code)] // читается фронтендом меню через `get_history`
pub const TRAY_MAX: usize = 10;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// Unix-время в мс.
    pub at: u64,
    pub target_exe: String,
    /// HWND окна, куда вставляли — только в памяти (после перезапуска бесполезен).
    #[serde(skip)]
    pub target_hwnd: isize,
    pub style_id: String,
    pub original: String,
    pub result: String,
    /// "lf" | "crlf" | "cr"
    pub line_ending: String,
    pub select_only: bool,
    /// Вставляли через UIA SetValue (значит, undo тоже может через него).
    pub via_uia: bool,
    /// Сейчас в поле стоит исходник (после undo).
    pub showing_original: bool,
}

/// То же для фронтенда и трея: без HWND и переводов строк, новые — первыми.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/bindings/")]
pub struct HistoryItem {
    #[ts(type = "number")]
    pub at: u64,
    pub target_exe: String,
    pub style_id: String,
    pub original: String,
    pub result: String,
    pub select_only: bool,
    /// Сейчас в поле стоит исходник (после undo).
    pub showing_original: bool,
}

impl From<&HistoryEntry> for HistoryItem {
    fn from(e: &HistoryEntry) -> Self {
        Self {
            at: e.at,
            target_exe: e.target_exe.clone(),
            style_id: e.style_id.clone(),
            original: e.original.clone(),
            result: e.result.clone(),
            select_only: e.select_only,
            showing_original: e.showing_original,
        }
    }
}

/// Однострочное превью записи: без переводов строк, не длиннее `max`.
/// Меню трея теперь рисует фронтенд, но превью остаётся чистой функцией
/// с тестами — пригодится, когда понадобится обрезать текст на бэкенде.
#[cfg_attr(not(test), allow(dead_code))]
pub fn preview(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = flat.chars().take(max).collect();
    if flat.chars().count() > max {
        out.push('…');
    }
    out
}

pub fn line_ending_to_str(le: LineEnding) -> &'static str {
    match le {
        LineEnding::Lf => "lf",
        LineEnding::CrLf => "crlf",
        LineEnding::Cr => "cr",
    }
}

pub fn line_ending_from_str(s: &str) -> LineEnding {
    match s {
        "crlf" => LineEnding::CrLf,
        "cr" => LineEnding::Cr,
        _ => LineEnding::Lf,
    }
}

pub struct History {
    entries: VecDeque<HistoryEntry>,
    path: Option<PathBuf>,
    persist: bool,
    limit: usize,
}

impl History {
    /// `path` — файл истории; читается, только если `persist`.
    pub fn load(path: Option<PathBuf>, persist: bool, limit: usize) -> Self {
        let limit = limit.clamp(1, HARD_MAX);
        let mut entries = VecDeque::new();
        if persist {
            if let Some(p) = &path {
                if let Ok(s) = std::fs::read_to_string(p) {
                    if let Ok(v) = serde_json::from_str::<Vec<HistoryEntry>>(&s) {
                        entries = v.into_iter().rev().take(limit).collect::<Vec<_>>().into_iter().rev().collect();
                    }
                }
            }
        }
        Self { entries, path, persist, limit }
    }

    /// Лимит из настроек; лишние записи отбрасываются сразу.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.clamp(1, HARD_MAX);
        if self.trim() {
            self.save();
        }
    }

    fn trim(&mut self) -> bool {
        let before = self.entries.len();
        while self.entries.len() > self.limit {
            self.entries.pop_front();
        }
        before != self.entries.len()
    }

    pub fn set_persist(&mut self, persist: bool) {
        self.persist = persist;
        if persist {
            self.save();
        } else if let Some(p) = &self.path {
            let _ = std::fs::remove_file(p); // выключили — с диска убираем
        }
    }

    pub fn push(&mut self, e: HistoryEntry) {
        self.entries.push_back(e);
        self.trim();
        self.save();
    }

    /// Запись по индексу из `items()` (0 — самая свежая), изменяемая.
    pub fn nth_newest_mut(&mut self, index: usize) -> Option<&mut HistoryEntry> {
        let n = self.entries.len();
        if index >= n {
            return None;
        }
        self.entries.get_mut(n - 1 - index)
    }

    #[cfg(test)]
    pub fn last_mut(&mut self) -> Option<&mut HistoryEntry> {
        self.entries.back_mut()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Записи для трея и окна настроек: новые первыми.
    pub fn items(&self) -> Vec<HistoryItem> {
        self.entries.iter().rev().map(HistoryItem::from).collect()
    }

    /// `index` — как в `items()` (0 = самая свежая).
    pub fn nth_newest(&self, index: usize) -> Option<&HistoryEntry> {
        self.entries.iter().nth_back(index)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        if let Some(p) = &self.path {
            let _ = std::fs::remove_file(p);
        }
    }

    pub fn save(&self) {
        if !self.persist {
            return;
        }
        let Some(p) = &self.path else { return };
        let v: Vec<&HistoryEntry> = self.entries.iter().collect();
        let write = || -> std::io::Result<()> {
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(p, serde_json::to_string_pretty(&v).unwrap())
        };
        if let Err(e) = write() {
            eprintln!("[restyle] история не сохранена: {e}");
        }
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(i: u64) -> HistoryEntry {
        HistoryEntry {
            at: i,
            target_exe: "notepad.exe".into(),
            target_hwnd: 42,
            style_id: "formal".into(),
            original: format!("o{i}"),
            result: format!("r{i}"),
            line_ending: "crlf".into(),
            select_only: false,
            via_uia: true,
            showing_original: false,
        }
    }

    #[test]
    fn keeps_only_last_max_entries() {
        let mut h = History::load(None, false, MAX);
        for i in 0..(MAX as u64 + 5) {
            h.push(entry(i));
        }
        assert_eq!(h.len(), MAX);
        assert_eq!(h.last_mut().unwrap().at, MAX as u64 + 4);
    }

    #[test]
    fn disk_roundtrip_and_removal_when_disabled() {
        let dir = std::env::temp_dir().join(format!("restyle-hist-{}", std::process::id()));
        let path = dir.join("history.json");
        let mut h = History::load(Some(path.clone()), true, MAX);
        h.push(entry(1));
        h.push(entry(2));
        assert!(path.exists());
        let h2 = History::load(Some(path.clone()), true, MAX);
        assert_eq!(h2.len(), 2);
        let mut h2 = h2;
        assert_eq!(h2.last_mut().unwrap().result, "r2");
        assert_eq!(h2.last_mut().unwrap().target_hwnd, 0, "hwnd не сериализуется");
        h2.set_persist(false);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn items_are_newest_first_and_clear_wipes_disk() {
        let dir = std::env::temp_dir().join(format!("restyle-hist-items-{}", std::process::id()));
        let path = dir.join("history.json");
        let mut h = History::load(Some(path.clone()), true, MAX);
        h.push(entry(1));
        h.push(entry(2));
        let items = h.items();
        assert_eq!(items[0].result, "r2");
        assert_eq!(h.nth_newest(1).unwrap().result, "r1");
        assert!(h.nth_newest(2).is_none());
        h.clear();
        assert_eq!(h.len(), 0);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn preview_flattens_and_truncates() {
        assert_eq!(preview("Привет,\n  мир", 40), "Привет, мир");
        assert_eq!(preview("абвгде", 3), "абв…");
    }

    #[test]
    fn line_ending_strings_roundtrip() {
        for le in [LineEnding::Lf, LineEnding::CrLf, LineEnding::Cr] {
            assert_eq!(line_ending_from_str(line_ending_to_str(le)), le);
        }
    }
}
