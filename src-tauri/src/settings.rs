//! Настройки приложения. Хранение — `tauri-plugin-store` (settings.json в
//! каталоге конфига приложения), один ключ `settings` с целой структурой.
//! API-ключ здесь НЕ живёт — он в `keyring` (фаза 4).
//!
//! TS-типы генерируются ts-rs в src/bindings/ (`cargo test export_bindings`).

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;
use ts_rs::TS;

use crate::hotkey::combo::VK_DOUBLE_TAP;
use crate::hotkey::{COMBO_LAYOUT, COMBO_MAIN, COMBO_STYLE_PREFIX, COMBO_UNDO};

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
    /// Двойное нажатие единственного модификатора («Ctrl, Ctrl»); `key` = 0.
    pub double: bool,
}

impl Default for HotkeyCombo {
    fn default() -> Self {
        Self { ctrl: false, alt: false, shift: false, win: false, key: 0, extra_keys: Vec::new(), double: false }
    }
}

impl HotkeyCombo {
    pub const fn new(ctrl: bool, alt: bool, shift: bool, key: u32) -> Self {
        Self { ctrl, alt, shift, win: false, key, extra_keys: Vec::new(), double: false }
    }

    /// «Дважды модификатор»: Ctrl, Ctrl.
    pub fn double_tap(ctrl: bool, alt: bool, shift: bool, win: bool) -> Self {
        Self { ctrl, alt, shift, win, key: 0, extra_keys: Vec::new(), double: true }
    }

    /// Нормализованные VK для ComboSet (модификаторы — «обобщённые» коды:
    /// VK_CONTROL/VK_MENU/VK_SHIFT/VK_LWIN, трекер сам сводит L/R-варианты).
    pub fn to_vks(&self) -> Vec<u32> {
        if self.double {
            let m = if self.ctrl { 0x11 } else if self.alt { 0x12 } else if self.shift { 0x10 } else { 0x5B };
            return vec![VK_DOUBLE_TAP, m];
        }
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
        self.key == 0 && self.extra_keys.is_empty() && !self.double
    }

    /// Есть ли модификатор. Комбинация без него (просто `A`) глотала бы клавишу
    /// во всех приложениях — такие не принимаем.
    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.alt || self.shift || self.win
    }

    /// Годится для хука: непустая и с модификатором.
    pub fn is_valid(&self) -> bool {
        if self.double {
            let mods = [self.ctrl, self.alt, self.shift, self.win].iter().filter(|m| **m).count();
            return mods == 1 && self.key == 0 && self.extra_keys.is_empty();
        }
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
    /// Прикладывать снимок экрана к запросу этого стиля. Правке грамматики
    /// контекст не нужен, и без кадра ответ приходит заметно быстрее.
    pub screenshot: bool,
    /// Переписывание моделью или перевод движком перевода.
    pub kind: StyleKind,
    /// Код языка для `kind = translate` (см. `TRANSLATE_LANGS`).
    pub target_lang: String,
    /// id стиля, которым причесать перевод (пусто — отдать перевод как есть).
    /// Так «перевести на английский и сделать формальным» — один стиль.
    pub post_style: String,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            instruction: String::new(),
            hotkey: None,
            builtin: false,
            screenshot: true,
            kind: StyleKind::Prompt,
            target_lang: String::new(),
            post_style: String::new(),
        }
    }
}

/// Что делает стиль: переписывает текст моделью или переводит отдельным
/// движком (DeepL). Перевод моделью медленнее и тратит квоту Gemini, поэтому
/// это разные виды стиля, а не разные промпты.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../src/bindings/")]
pub enum StyleKind {
    #[default]
    Prompt,
    Translate,
}

/// Языки перевода: код DeepL и подпись. `EN-US`/`EN-GB` у DeepL — разные цели.
pub const TRANSLATE_LANGS: &[(&str, &str)] = &[
    ("EN-US", "English (US)"),
    ("EN-GB", "English (UK)"),
    ("RU", "Русский"),
    ("UK", "Українська"),
    ("DE", "Deutsch"),
    ("FR", "Français"),
    ("ES", "Español"),
    ("IT", "Italiano"),
    ("PL", "Polski"),
    ("PT-PT", "Português"),
    ("TR", "Türkçe"),
    ("ZH", "中文"),
    ("JA", "日本語"),
];

fn builtin(id: &str, name: &str, instruction: &str, screenshot: bool) -> Style {
    Style {
        id: id.into(),
        name: name.into(),
        instruction: instruction.into(),
        builtin: true,
        screenshot,
        ..Style::default()
    }
}

/// Встроенный перевод: отдельный движок, кадр экрана не нужен.
fn builtin_tr(id: &str, name: &str, lang: &str, instruction: &str) -> Style {
    Style {
        id: id.into(),
        name: name.into(),
        instruction: instruction.into(),
        builtin: true,
        screenshot: false,
        kind: StyleKind::Translate,
        target_lang: lang.into(),
        ..Style::default()
    }
}

pub fn builtin_styles() -> Vec<Style> {
    vec![
        builtin("formal", "Formal", "Rewrite in a formal, professional register. Polite, precise, no slang or emoji.", true),
        builtin("casual", "Casual", "Rewrite in a relaxed, friendly, conversational tone, as between people who know each other.", true),
        builtin("shorter", "Shorter", "Make it noticeably shorter. Keep every fact; cut filler, repetition and hedging.", true),
        builtin("longer", "Longer", "Expand it: add natural detail, transitions and clarity without inventing new facts.", true),
        // Правке грамматики и переводу контекст с экрана не нужен — без кадра быстрее.
        builtin("fix", "Fix grammar", "Fix spelling, grammar and punctuation only. Keep wording, tone and formatting as close to the original as possible.", false),
        // Перевод идёт через DeepL; instruction — запасной промпт на случай,
        // когда ключа DeepL нет и переводить приходится модели.
        builtin_tr("to-en", "Translate to English", "EN-US", "Translate into natural, fluent English. Keep tone, formatting, names and numbers."),
        builtin_tr("to-ru", "Translate to Russian", "RU", "Translate into natural, fluent Russian. Keep tone, formatting, names and numbers."),
        builtin_tr("to-uk", "Translate to Ukrainian", "UK", "Translate into natural, fluent Ukrainian. Keep tone, formatting, names and numbers."),
    ]
}

/// Пресеты акцентного цвета (сами цвета — в `src/lib/theme.ts`).
pub const ACCENTS: &[&str] = &["amber", "blue", "green", "violet", "rose", "teal"];

/// Настройки приложения.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct Settings {
    /// Основная комбинация: открыть панель выбора стиля.
    pub main_hotkey: HotkeyCombo,
    /// Вернуть предыдущий текст (пустая — выключено).
    pub undo_hotkey: HotkeyCombo,
    /// Исправить раскладку выделения/поля (пустая — выключено).
    pub layout_hotkey: HotkeyCombo,
    pub styles: Vec<Style>,
    /// Имя модели Gemini.
    pub model: String,
    pub autostart: bool,
    /// "dark" | "light" | "system"
    pub theme: String,
    /// Акцентный цвет интерфейса: id пресета из `ACCENTS`.
    pub accent: String,
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
    /// Сколько последних пар «исходник — результат» держать.
    pub history_limit: u32,
    /// "system" | "ru" | "uk" | "en" — язык интерфейса.
    pub language: String,
    /// Стиль короткого нажатия основного хоткея (пусто — сразу открывать панель).
    pub quick_style: String,
    /// Короткое нажатие применяет `quick_style`, удержание открывает панель.
    pub long_press: bool,
    /// Через сколько мс удержания показать кольцо-индикатор у курсора.
    pub long_press_ring_ms: u32,
    /// Через сколько мс удержания открыть панель выбора.
    pub long_press_open_ms: u32,
    /// Движок перевода: "deepl" | "gemini".
    pub translator: String,
    /// Возвращать прежнее содержимое буфера после вставки через клипборд.
    pub restore_clipboard: bool,
    /// Мастер первого запуска пройден (или пропущен).
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            main_hotkey: HotkeyCombo::new(true, true, false, 0x52), // Ctrl+Alt+R
            undo_hotkey: HotkeyCombo::new(true, true, false, 0x5A), // Ctrl+Alt+Z
            // Дважды Ctrl: одной рукой и не пересекается с сочетаниями программ.
            layout_hotkey: HotkeyCombo::double_tap(true, false, false, false),
            styles: builtin_styles(),
            // 2.5 недоступна новым ключам (API: 404 «no longer available»);
            // 3.5-flash-lite даёт первый чанк ≈0,7 с против ≈2,9 с у 3.6-flash.
            model: MODEL_LITE.into(),
            autostart: false,
            theme: "system".into(),
            accent: "amber".into(),
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
            history_limit: crate::history::MAX as u32,
            language: "system".into(),
            quick_style: String::new(),
            long_press: false,
            long_press_ring_ms: 300,
            long_press_open_ms: 600,
            translator: "deepl".into(),
            restore_clipboard: true,
            onboarded: false,
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
            (COMBO_LAYOUT.to_string(), &self.layout_hotkey),
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
        if !ACCENTS.contains(&self.accent.as_str()) {
            self.accent = "amber".into();
        }
        if !matches!(self.screenshot_mode.as_str(), "window" | "monitor") {
            self.screenshot_mode = "window".into();
        }
        if !matches!(self.language.as_str(), "system" | "ru" | "uk" | "en") {
            self.language = "system".into();
        }
        self.history_limit = self.history_limit.clamp(1, crate::history::HARD_MAX as u32);
        if !matches!(self.translator.as_str(), "deepl" | "gemini") {
            self.translator = "deepl".into();
        }
        // Кольцо должно появляться заметно раньше панели, иначе удержание
        // выглядит как залипание.
        self.long_press_ring_ms = self.long_press_ring_ms.clamp(100, 1500);
        self.long_press_open_ms = self.long_press_open_ms.clamp(200, 3000);
        if self.long_press_open_ms <= self.long_press_ring_ms {
            self.long_press_open_ms = self.long_press_ring_ms + 200;
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
        if !self.layout_hotkey.is_empty() && !self.layout_hotkey.has_modifier() {
            self.layout_hotkey = HotkeyCombo::default();
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

        // Ссылки между стилями чиним после того, как id устаканились.
        for st in &mut self.styles {
            match st.kind {
                StyleKind::Translate => {
                    if !TRANSLATE_LANGS.iter().any(|(code, _)| *code == st.target_lang) {
                        st.target_lang = "EN-US".into();
                    }
                }
                StyleKind::Prompt => {
                    st.target_lang = String::new();
                    st.post_style = String::new();
                }
            }
        }
        let ids: Vec<String> = self.styles.iter().map(|s| s.id.clone()).collect();
        let prompt_ids: Vec<String> = self
            .styles
            .iter()
            .filter(|s| s.kind == StyleKind::Prompt)
            .map(|s| s.id.clone())
            .collect();
        for st in &mut self.styles {
            // Причесать перевод можно только обычным стилем и не самим собой.
            if !st.post_style.is_empty()
                && (!prompt_ids.contains(&st.post_style) || st.post_style == st.id)
            {
                st.post_style = String::new();
            }
        }
        if !self.quick_style.is_empty() && !ids.contains(&self.quick_style) {
            self.quick_style = String::new();
        }
        if self.long_press && self.quick_style.is_empty() {
            // Без стиля короткому нажатию нечего делать — берём первый.
            self.quick_style = ids.first().cloned().unwrap_or_default();
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
        assert_eq!(c[2], ("layout".to_string(), vec![VK_DOUBLE_TAP, 0x11]));
        assert_eq!(c[3], ("style:formal".to_string(), vec![0x11, 0x12, 0x46]));
        assert_eq!(c.len(), 4);
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
    fn normalize_language_and_history_limit() {
        let mut s = Settings::default();
        s.language = "de".into();
        s.history_limit = 0;
        s.normalize();
        assert_eq!(s.language, "system");
        assert_eq!(s.history_limit, 1, "меньше одной пары истории не бывает");
        s.language = "uk".into();
        s.history_limit = 10_000;
        s.normalize();
        assert_eq!(s.language, "uk");
        assert_eq!(s.history_limit, crate::history::HARD_MAX as u32);
    }

    #[test]
    fn builtin_styles_decide_about_screenshot() {
        let by_id = |id: &str| builtin_styles().into_iter().find(|s| s.id == id).unwrap();
        assert!(by_id("formal").screenshot, "тон письма зависит от контекста");
        assert!(!by_id("fix").screenshot, "правке грамматики кадр не нужен");
        // Старый settings.json без поля: serde подставляет дефолт структуры.
        let s: Style = serde_json::from_str(r#"{"id":"x","name":"X"}"#).unwrap();
        assert!(s.screenshot);
    }

    /// Перевод и «причёска после перевода» — единственные ссылки между
    /// стилями: битую ссылку легко получить, переименовав или удалив стиль.
    #[test]
    fn translate_fields_are_normalized() {
        let mut s = Settings::default();
        // несуществующий язык → дефолтный
        s.styles.push(Style {
            id: "tr".into(),
            name: "Перевод".into(),
            kind: StyleKind::Translate,
            target_lang: "КЛИНГОНСКИЙ".into(),
            post_style: "formal".into(),
            ..Style::default()
        });
        // обычный стиль не должен тащить поля перевода
        s.styles.push(Style {
            id: "plain".into(),
            name: "Обычный".into(),
            target_lang: "RU".into(),
            post_style: "formal".into(),
            ..Style::default()
        });
        s.normalize();
        let tr = s.styles.iter().find(|x| x.id == "tr").unwrap();
        assert_eq!(tr.target_lang, "EN-US");
        assert_eq!(tr.post_style, "formal");
        let plain = s.styles.iter().find(|x| x.id == "plain").unwrap();
        assert_eq!(plain.target_lang, "");
        assert_eq!(plain.post_style, "");
    }

    #[test]
    fn post_style_must_point_at_existing_prompt_style() {
        let mut s = Settings::default();
        s.styles.push(Style {
            id: "tr".into(),
            name: "Перевод".into(),
            kind: StyleKind::Translate,
            target_lang: "RU".into(),
            post_style: "нет-такого".into(),
            ..Style::default()
        });
        s.normalize();
        assert_eq!(s.styles.last().unwrap().post_style, "");

        // ссылка на сам себя и на другой перевод тоже недопустима
        let mut s2 = Settings::default();
        s2.styles.push(Style {
            id: "tr".into(),
            name: "Перевод".into(),
            kind: StyleKind::Translate,
            target_lang: "RU".into(),
            post_style: "to-en".into(),
            ..Style::default()
        });
        s2.normalize();
        assert_eq!(s2.styles.last().unwrap().post_style, "");
    }

    #[test]
    fn long_press_thresholds_are_ordered_and_have_a_style() {
        let mut s = Settings { long_press: true, ..Settings::default() };
        s.long_press_ring_ms = 5;
        s.long_press_open_ms = 5;
        s.normalize();
        assert_eq!(s.long_press_ring_ms, 100, "кольцо не может появиться мгновенно");
        assert!(
            s.long_press_open_ms > s.long_press_ring_ms,
            "панель обязана открываться позже кольца, иначе кольца не видно"
        );
        assert!(!s.quick_style.is_empty(), "короткому нажатию нужен стиль");

        // стиль, которого нет, сбрасывается
        let mut s2 = Settings { quick_style: "нет-такого".into(), ..Settings::default() };
        s2.normalize();
        assert_eq!(s2.quick_style, "");
    }

    #[test]
    fn translator_enum_is_guarded() {
        let mut s = Settings { translator: "yandex".into(), ..Settings::default() };
        s.normalize();
        assert_eq!(s.translator, "deepl");
    }

    #[test]
    fn builtin_translations_cover_three_languages() {
        let tr: Vec<_> = builtin_styles()
            .into_iter()
            .filter(|s| s.kind == StyleKind::Translate)
            .map(|s| s.target_lang)
            .collect();
        assert_eq!(tr, vec!["EN-US", "RU", "UK"]);
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
        assert_eq!(partial.styles.len(), 8); // 5 переписывающих + 3 перевода
    }
}
