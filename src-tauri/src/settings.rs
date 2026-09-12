//! Настройки приложения. Хранение — `tauri-plugin-store` (settings.json в
//! каталоге конфига приложения), один ключ `settings` с целой структурой.
//! API-ключ здесь НЕ живёт — он в `keyring` (фаза 4).
//!
//! TS-типы генерируются ts-rs в src/bindings/ (`cargo test export_bindings`).

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;
use ts_rs::TS;

use crate::hotkey::{COMBO_MAIN, COMBO_STYLE_PREFIX, COMBO_UNDO};

pub const STORE_FILE: &str = "settings.json";
/// Пресеты моделей (замер 12.09.2026 по живому API).
#[allow(dead_code)] // пресет продублирован списком в окне настроек (фронтенд)
pub const MODEL_FLASH: &str = "gemini-3.6-flash";
pub const MODEL_LITE: &str = "gemini-3.5-flash-lite";
const STORE_KEY: &str = "settings";

/// Описание комбинации: модификаторы + основная клавиша (VK-код). Как в HoldMix.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct HotkeyCombo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    /// VK-код основной клавиши (например 0x52 = R); 0 — не задана.
    pub key: u32,
    /// Доп. обычные клавиши (VK) для нестандартных комбо.
    pub extra_keys: Vec<u32>,
}

impl Default for HotkeyCombo {
    fn default() -> Self {
        Self { ctrl: false, alt: false, shift: false, win: false, key: 0, extra_keys: Vec::new() }
    }
}

impl HotkeyCombo {
    pub const fn new(ctrl: bool, alt: bool, shift: bool, key: u32) -> Self {
        Self { ctrl, alt, shift, win: false, key, extra_keys: Vec::new() }
    }

    /// Нормализованные VK для ComboSet (модификаторы — «обобщённые» коды:
    /// VK_CONTROL/VK_MENU/VK_SHIFT/VK_LWIN, трекер сам сводит L/R-варианты).
    pub fn to_vks(&self) -> Vec<u32> {
        let mut v = Vec::with_capacity(5);
        if self.ctrl {
            v.push(0x11);
        }
        if self.alt {
            v.push(0x12);
        }
        if self.shift {
            v.push(0x10);
        }
        if self.win {
            v.push(0x5B);
        }
        v.extend(&self.extra_keys);
        if self.key != 0 {
            v.push(self.key);
        }
        v
    }

    pub fn is_empty(&self) -> bool {
        self.key == 0 && self.extra_keys.is_empty()
    }

    /// Есть ли модификатор. Комбинация без него (просто `A`) глотала бы клавишу
    /// во всех приложениях — такие не принимаем.
    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.alt || self.shift || self.win
    }

    /// Годится для хука: непустая и с модификатором.
    pub fn is_valid(&self) -> bool {
        !self.is_empty() && self.has_modifier()
    }

    /// Набор клавиш без учёта порядка — для сравнения комбинаций между собой.
    fn key_set(&self) -> Vec<u32> {
        let mut v = self.to_vks();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// id стиля из имени: латиница/цифры, остальное — дефис. Кириллица отпадает,
/// поэтому пустой результат заменяется на `style`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let s = out.trim_matches('-').to_string();
    if s.is_empty() {
        "style".into()
    } else {
        s
    }
}

/// Стиль переписывания: имя, инструкция для промпта, необязательный хоткей.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct Style {
    pub id: String,
    pub name: String,
    pub instruction: String,
    pub hotkey: Option<HotkeyCombo>,
    /// Встроенный стиль: имя/инструкцию можно править, удалить нельзя.
    pub builtin: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self { id: String::new(), name: String::new(), instruction: String::new(), hotkey: None, builtin: false }
    }
}

fn builtin(id: &str, name: &str, instruction: &str) -> Style {
    Style { id: id.into(), name: name.into(), instruction: instruction.into(), hotkey: None, builtin: true }
}

pub fn builtin_styles() -> Vec<Style> {
    vec![
        builtin("formal", "Formal", "Rewrite in a formal, professional register. Polite, precise, no slang or emoji."),
        builtin("casual", "Casual", "Rewrite in a relaxed, friendly, conversational tone, as between people who know each other."),
        builtin("shorter", "Shorter", "Make it noticeably shorter. Keep every fact; cut filler, repetition and hedging."),
        builtin("longer", "Longer", "Expand it: add natural detail, transitions and clarity without inventing new facts."),
        builtin("fix", "Fix grammar", "Fix spelling, grammar and punctuation only. Keep wording, tone and formatting as close to the original as possible."),
        builtin("to-en", "Translate to English", "Translate into natural, fluent English. Keep tone, formatting, names and numbers."),
        builtin("to-ru", "Translate to Russian", "Translate into natural, fluent Russian. Keep tone, formatting, names and numbers."),
    ]
}

/// Настройки приложения.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct Settings {
    /// Основная комбинация: открыть панель выбора стиля.
    pub main_hotkey: HotkeyCombo,
    /// Вернуть предыдущий текст (пустая — выключено).
    pub undo_hotkey: HotkeyCombo,
    pub styles: Vec<Style>,
    /// Имя модели Gemini.
    pub model: String,
    pub autostart: bool,
    /// "dark" | "light" | "system"
    pub theme: String,
    /// Для быстрых стилей: вставлять сразу, не ждать Enter.
    pub auto_paste: bool,
    /// Глобальный выключатель скриншота.
    pub screenshot_enabled: bool,
    /// "window" — окно с фокусом; "monitor" — активный монитор.
    pub screenshot_mode: String,
    /// Имена процессов (exe), для которых скриншот не делается никогда.
    pub screenshot_excluded_processes: Vec<String>,
    /// Имена процессов, где шлётся только Ctrl+C без Ctrl+A.
    pub select_only_processes: Vec<String>,
    /// Писать историю переписываний на диск.
    pub history_to_disk: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            main_hotkey: HotkeyCombo::new(true, true, false, 0x52), // Ctrl+Alt+R
            undo_hotkey: HotkeyCombo::new(true, true, false, 0x5A), // Ctrl+Alt+Z
            styles: builtin_styles(),
            // 2.5 недоступна новым ключам (API: 404 «no longer available»);
            // 3.5-flash-lite даёт первый чанк ≈0,7 с против ≈2,9 с у 3.6-flash.
            model: MODEL_LITE.into(),
            autostart: false,
            theme: "system".into(),
            auto_paste: false,
            screenshot_enabled: true,
            screenshot_mode: "window".into(),
            screenshot_excluded_processes: [
                "KeePass.exe", "KeePassXC.exe", "1Password.exe", "Bitwarden.exe",
                "LastPass.exe", "Dashlane.exe", "NordPass.exe", "Enpass.exe",
                "Proton Pass.exe", "RoboForm.exe", "Keeper.exe",
                "SberBank.exe", "Tinkoff.exe", "AlfaBank.exe",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            select_only_processes: [
                "Code.exe", "idea64.exe", "webstorm64.exe", "pycharm64.exe", "rider64.exe",
                "clion64.exe", "goland64.exe", "devenv.exe", "notepad++.exe", "sublime_text.exe",
                "WINWORD.EXE", "Obsidian.exe", "Notion.exe",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            history_to_disk: false,
        }
    }
}

impl Settings {
    /// Читает из store; при отсутствии/битом JSON — дефолт. Прочитанное сразу
    /// нормализуется: файл могли править руками или он остался от старой версии,
    /// а весь код дальше рассчитывает на непустые id стилей и валидные комбо.
    pub fn load(app: &AppHandle) -> Self {
        let Ok(store) = app.store(STORE_FILE) else { return Self::default() };
        let mut s: Self = store
            .get(STORE_KEY)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        s.normalize();
        s
    }

    /// Пишет в store (плагин сам сохраняет на диск). Ошибки — в stderr.
    pub fn store(&self, app: &AppHandle) {
        match app.store(STORE_FILE) {
            Ok(store) => {
                store.set(STORE_KEY, serde_json::to_value(self).unwrap());
                if let Err(e) = store.save() {
                    eprintln!("[restyle] не удалось сохранить настройки: {e}");
                }
            }
            Err(e) => eprintln!("[restyle] store недоступен: {e}"),
        }
    }

    /// Все комбинации для hotkey-потока: (id, VK). Пустые и без модификатора
    /// пропускаются; при конфликте выживает первая (порядок: main, undo, стили).
    pub fn combos(&self) -> Vec<(String, Vec<u32>)> {
        let mut v = Vec::new();
        for (id, h) in self.bindings() {
            if h.is_valid() {
                v.push((id, h.to_vks()));
            }
        }
        v
    }

    /// Все привязки в порядке приоритета: (id комбинации, комбинация).
    fn bindings(&self) -> Vec<(String, &HotkeyCombo)> {
        let mut v: Vec<(String, &HotkeyCombo)> = vec![
            (COMBO_MAIN.to_string(), &self.main_hotkey),
            (COMBO_UNDO.to_string(), &self.undo_hotkey),
        ];
        for s in &self.styles {
            if let Some(h) = &s.hotkey {
                v.push((format!("{COMBO_STYLE_PREFIX}{}", s.id), h));
            }
        }
        v
    }

    /// Пары действий с одинаковым набором клавиш. Хук отдаёт комбинацию первому
    /// подходящему id, поэтому второе действие молча не сработает — показываем.
    pub fn hotkey_conflicts(&self) -> Vec<(String, String)> {
        let all: Vec<(String, Vec<u32>)> = self
            .bindings()
            .into_iter()
            .filter(|(_, h)| h.is_valid())
            .map(|(id, h)| (id, h.key_set()))
            .collect();
        let mut out = Vec::new();
        for (i, (id_a, keys_a)) in all.iter().enumerate() {
            for (id_b, keys_b) in all.iter().skip(i + 1) {
                if keys_a == keys_b {
                    out.push((id_a.clone(), id_b.clone()));
                }
            }
        }
        out
    }

    /// Приводит пришедшие из окна настроек данные в пригодный вид: id стилей,
    /// обрезка пробелов, допустимые значения перечислений, хоткеи без
    /// модификатора. Вызывается перед сохранением — фронт не может испортить store.
    pub fn normalize(&mut self) {
        self.model = self.model.trim().to_string();
        if self.model.is_empty() {
            self.model = MODEL_LITE.into();
        }
        if !matches!(self.theme.as_str(), "system" | "dark" | "light") {
            self.theme = "system".into();
        }
        if !matches!(self.screenshot_mode.as_str(), "window" | "monitor") {
            self.screenshot_mode = "window".into();
        }
        self.screenshot_excluded_processes = clean_list(&self.screenshot_excluded_processes);
        self.select_only_processes = clean_list(&self.select_only_processes);

        // Основной хоткей без модификатора сломал бы ввод во всех программах;
        // пустой оставил бы приложение без входа — возвращаем дефолт.
        if !self.main_hotkey.is_valid() {
            eprintln!("[restyle] основной хоткей без модификатора — вернул дефолт");
            self.main_hotkey = Settings::default().main_hotkey;
        }
        if !self.undo_hotkey.is_empty() && !self.undo_hotkey.has_modifier() {
            self.undo_hotkey = HotkeyCombo::default();
        }

        let builtin_ids: Vec<String> = builtin_styles().into_iter().map(|s| s.id).collect();
        let mut seen: Vec<String> = Vec::with_capacity(self.styles.len());
        for st in &mut self.styles {
            st.name = st.name.trim().to_string();
            st.instruction = st.instruction.trim().to_string();
            if st.name.is_empty() {
                st.name = "Без имени".into();
            }
            let mut id = st.id.trim().to_ascii_lowercase();
            if id.is_empty() {
                id = slug(&st.name);
            }
            // Коллизии id ломают и промпт, и хоткей `style:<id>`.
            if seen.contains(&id) {
                let base = id.clone();
                let mut n = 2;
                while seen.contains(&id) {
                    id = format!("{base}-{n}");
                    n += 1;
                }
            }
            st.builtin = builtin_ids.contains(&id);
            st.id = id.clone();
            seen.push(id);
            if let Some(h) = &st.hotkey {
                if !h.is_valid() {
                    st.hotkey = None;
                }
            }
        }
        if self.styles.is_empty() {
            self.styles = builtin_styles();
        }
    }
}

/// Список имён процессов: обрезка, без пустых, без повторов (регистр не важен).
fn clean_list(src: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(src.len());
    for s in src {
        let s = s.trim();
        if s.is_empty() {
            continue;
        }
        if !out.iter().any(|e| e.eq_ignore_ascii_case(s)) {
            out.push(s.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_main_combo_is_ctrl_alt_r() {
        assert_eq!(Settings::default().main_hotkey.to_vks(), vec![0x11, 0x12, 0x52]);
    }

    #[test]
    fn combos_include_main_undo_and_style_hotkeys() {
        let mut s = Settings::default();
        s.styles[0].hotkey = Some(HotkeyCombo::new(true, true, false, 0x46));
        let c = s.combos();
        assert_eq!(c[0].0, "main");
        assert_eq!(c[1].0, "undo");
        assert_eq!(c[2], ("style:formal".to_string(), vec![0x11, 0x12, 0x46]));
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn empty_undo_hotkey_is_skipped() {
        let mut s = Settings::default();
        s.undo_hotkey = HotkeyCombo::default();
        assert!(s.combos().iter().all(|(id, _)| id != "undo"));
    }

    #[test]
    fn combo_without_modifier_is_not_bound() {
        let mut s = Settings::default();
        s.styles[0].hotkey = Some(HotkeyCombo::new(false, false, false, 0x41)); // просто A
        assert!(s.combos().iter().all(|(id, _)| id != "style:formal"));
        s.normalize();
        assert_eq!(s.styles[0].hotkey, None, "нормализация убирает такой хоткей");
    }

    #[test]
    fn conflicting_hotkeys_are_reported() {
        let mut s = Settings::default();
        s.styles[0].hotkey = Some(HotkeyCombo::new(true, true, false, 0x52)); // = main
        s.styles[1].hotkey = Some(HotkeyCombo::new(true, true, true, 0x52)); // +Shift — другой
        let c = s.hotkey_conflicts();
        assert_eq!(c, vec![("main".to_string(), "style:formal".to_string())]);
    }

    #[test]
    fn normalize_fills_ids_and_keeps_them_unique() {
        let mut s = Settings::default();
        s.styles = vec![
            Style { id: String::new(), name: "  Sales pitch ".into(), instruction: " x ".into(), ..Style::default() },
            Style { id: String::new(), name: "Sales pitch".into(), instruction: "y".into(), ..Style::default() },
            Style { id: String::new(), name: "Вежливо".into(), instruction: "z".into(), ..Style::default() },
            Style { id: "formal".into(), name: "Formal".into(), instruction: "f".into(), ..Style::default() },
        ];
        s.normalize();
        let ids: Vec<&str> = s.styles.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, ["sales-pitch", "sales-pitch-2", "style", "formal"]);
        assert_eq!(s.styles[0].name, "Sales pitch", "имя обрезано");
        assert!(s.styles[3].builtin, "id встроенного стиля возвращает флаг builtin");
        assert!(!s.styles[0].builtin);
    }

    #[test]
    fn normalize_fixes_enums_and_lists() {
        let mut s = Settings::default();
        s.theme = "neon".into();
        s.screenshot_mode = "screen".into();
        s.model = "   ".into();
        s.select_only_processes = vec![" Code.exe ".into(), "code.EXE".into(), "".into()];
        s.main_hotkey = HotkeyCombo::new(false, false, false, 0x41);
        s.undo_hotkey = HotkeyCombo::new(false, false, false, 0x5A);
        s.normalize();
        assert_eq!(s.theme, "system");
        assert_eq!(s.screenshot_mode, "window");
        assert_eq!(s.model, MODEL_LITE);
        assert_eq!(s.select_only_processes, vec!["Code.exe".to_string()]);
        assert_eq!(s.main_hotkey, Settings::default().main_hotkey);
        assert!(s.undo_hotkey.is_empty());
    }

    #[test]
    fn settings_roundtrip_camel_case_and_defaults() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"mainHotkey\""), "{json}");
        assert!(!json.contains("apiKey"), "ключ не должен быть в настройках");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        let partial: Settings = serde_json::from_str(r#"{"autostart":true}"#).unwrap();
        assert!(partial.autostart);
        assert_eq!(partial.styles.len(), 7);
    }
}
