//! Чистые автоматы распознавания комбинаций клавиш. Без WinAPI — юнит-тестируется.
//!
//! `ComboTracker` взят из HoldMix (edge-triggered, автоповтор гасится,
//! L/R-модификаторы нормализуются). Поверх него — `ComboSet`: набор именованных
//! комбинаций (основная + по одной на «быстрый» стиль + undo). В отличие от
//! HoldMix комбинация здесь — «нажатие», а не «удержание»: интересен только
//! фронт `Show`, отпускание игнорируется.
//!
//! Точное совпадение модификаторов: если зажат модификатор, которого нет в
//! комбинации (Ctrl+Alt+Shift+R при комбинации Ctrl+Alt+R), срабатывания нет —
//! иначе более длинная комбинация никогда не сможет существовать рядом с короткой.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Show,
    Hide,
    /// Модификатор нажат дважды подряд (Ctrl, Ctrl). Приходит на отпускании —
    /// глотать нечего, приложение уже видело оба нажатия.
    Double,
}

/// Маркер в списке VK: `[VK_DOUBLE_TAP, модификатор]` — «дважды модификатор».
pub const VK_DOUBLE_TAP: u32 = 0x1_0000;
/// Нажатие короче этого — «тап», а не удержание.
const TAP_MAX_MS: u64 = 300;
/// Между отпусканиями двух тапов.
const DOUBLE_GAP_MS: u64 = 400;

pub const VK_SHIFT: u32 = 0x10;
pub const VK_CONTROL: u32 = 0x11;
pub const VK_MENU: u32 = 0x12;
pub const VK_LWIN: u32 = 0x5B;

/// L/R-варианты модификаторов -> обобщённый VK.
pub fn normalize_vk(vk: u32) -> u32 {
    match vk {
        0xA0 | 0xA1 => VK_SHIFT,
        0xA2 | 0xA3 => VK_CONTROL,
        0xA4 | 0xA5 => VK_MENU,
        0x5C => VK_LWIN, // VK_RWIN -> VK_LWIN
        v => v,
    }
}

pub fn is_modifier(vk: u32) -> bool {
    matches!(normalize_vk(vk), VK_SHIFT | VK_CONTROL | VK_MENU | VK_LWIN)
}

#[derive(Debug)]
pub struct ComboTracker {
    keys: Vec<u32>, // нормализованные VK, максимум 8
    down: u8,       // битовая маска зажатых клавиш комбинации
    active: bool,
}

impl ComboTracker {
    pub fn new(keys: &[u32]) -> Self {
        assert!(!keys.is_empty() && keys.len() <= 8, "комбинация: 1..=8 клавиш");
        Self {
            keys: keys.iter().map(|&k| normalize_vk(k)).collect(),
            down: 0,
            active: false,
        }
    }

    fn full_mask(&self) -> u8 {
        (1u16 << self.keys.len()) as u8 - 1
    }

    pub fn contains(&self, vk: u32) -> bool {
        self.keys.contains(&normalize_vk(vk))
    }

    /// Событие клавиатуры. `down=false` — отпускание. Возвращает фронт, если есть.
    pub fn on_key(&mut self, vk: u32, down: bool) -> Option<Edge> {
        let vk = normalize_vk(vk);
        let idx = self.keys.iter().position(|&k| k == vk)?;
        let bit = 1u8 << idx;
        if down {
            if self.down & bit != 0 {
                return None; // автоповтор
            }
            self.down |= bit;
            if self.down == self.full_mask() && !self.active {
                self.active = true;
                return Some(Edge::Show);
            }
            None
        } else {
            if self.down & bit == 0 {
                return None; // отпускание клавиши, зажатой до старта трекинга
            }
            self.down &= !bit;
            if self.active {
                self.active = false;
                return Some(Edge::Hide);
            }
            None
        }
    }

}

/// Набор именованных комбинаций. `on_key` возвращает id сработавшей комбинации;
/// вызывающий код в этом случае глотает нажатие (`return 1` из хука).
#[derive(Debug, Default)]
pub struct ComboSet {
    entries: Vec<(String, ComboTracker)>,
    /// Зажатые сейчас модификаторы (нормализованные VK) — для точного совпадения.
    mods_down: Vec<u32>,
    /// Двойные нажатия модификатора: (id, нормализованный VK).
    doubles: Vec<(String, u32)>,
    /// Модификатор нажат в одиночку (VK, время) — кандидат в тап.
    solo: Option<(u32, u64)>,
    /// Последний завершённый тап (VK, время отпускания).
    last_tap: Option<(u32, u64)>,
}

impl ComboSet {
    /// Пустые комбинации пропускаются молча.
    pub fn new(combos: &[(String, Vec<u32>)]) -> Self {
        let entries = combos
            .iter()
            .filter(|(_, keys)| !keys.is_empty() && keys.len() <= 8 && !keys.contains(&VK_DOUBLE_TAP))
            .map(|(id, keys)| (id.clone(), ComboTracker::new(keys)))
            .collect();
        let doubles = combos
            .iter()
            .filter(|(_, keys)| keys.len() == 2 && keys[0] == VK_DOUBLE_TAP && is_modifier(keys[1]))
            .map(|(id, keys)| (id.clone(), normalize_vk(keys[1])))
            .collect();
        Self { entries, mods_down: Vec::new(), doubles, solo: None, last_tap: None }
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Зажат ли сейчас хоть один модификатор (по событиям хука). Пока он
    /// зажат, клавиши панели (Tab, Enter, стрелки, цифры, R) не наши:
    /// Alt+Tab и Ctrl+R должны уйти в систему и приложение.
    pub fn modifiers_held(&self) -> bool {
        !self.mods_down.is_empty()
    }

    /// Событие клавиатуры. `Some((id, Show))` — комбинация сложилась (её надо
    /// проглотить), `Some((id, Hide))` — её отпустили. Отпускание нужно для
    /// «короткое нажатие — стиль, удержание — панель»; глотать его не надо,
    /// иначе приложение под оверлеем останется с зажатой клавишей.
    #[cfg(test)]
    pub fn on_key(&mut self, vk: u32, down: bool) -> Option<(&str, Edge)> {
        self.on_key_at(vk, down, 0)
    }

    /// То же с временем события (мс) — для двойных нажатий.
    pub fn on_key_at(&mut self, vk: u32, down: bool, t: u64) -> Option<(&str, Edge)> {
        let nvk = normalize_vk(vk);
        let mut double_hit: Option<usize> = None;
        if is_modifier(nvk) {
            if down {
                if !self.mods_down.contains(&nvk) {
                    // Тап — только модификатор в одиночку.
                    if self.mods_down.is_empty() {
                        self.solo = Some((nvk, t));
                    } else {
                        self.solo = None;
                        self.last_tap = None;
                    }
                }
            } else if let Some((m, t_down)) = self.solo.take() {
                if m == nvk && t.saturating_sub(t_down) <= TAP_MAX_MS {
                    match self.last_tap {
                        Some((lm, lt)) if lm == nvk && t.saturating_sub(lt) <= DOUBLE_GAP_MS => {
                            self.last_tap = None;
                            double_hit = self.doubles.iter().position(|(_, v)| *v == nvk);
                        }
                        _ => self.last_tap = Some((nvk, t)),
                    }
                } else {
                    self.last_tap = None;
                }
            }
        } else if down {
            self.solo = None;
            self.last_tap = None;
        }
        if is_modifier(nvk) {
            if down {
                if !self.mods_down.contains(&nvk) {
                    self.mods_down.push(nvk);
                }
            } else {
                self.mods_down.retain(|&m| m != nvk);
            }
        }
        let mut fired: Option<usize> = None;
        let mut released: Option<usize> = None;
        for (i, (_, tracker)) in self.entries.iter_mut().enumerate() {
            match tracker.on_key(nvk, down) {
                Some(Edge::Show) if fired.is_none() => {
                    // точное совпадение модификаторов
                    let extra_mod = self.mods_down.iter().any(|&m| !tracker.contains(m));
                    if !extra_mod {
                        fired = Some(i);
                    }
                }
                // Отпускание отдаём только той комбинации, что реально сработала:
                // `active` у трекера ставится лишь после полного нажатия.
                Some(Edge::Hide) if released.is_none() => released = Some(i),
                _ => {}
            }
        }
        if let Some(i) = double_hit {
            return Some((self.doubles[i].0.as_str(), Edge::Double));
        }
        if let Some(i) = fired {
            return Some((self.entries[i].0.as_str(), Edge::Show));
        }
        released.map(|i| (self.entries[i].0.as_str(), Edge::Hide))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTRL: u32 = 0x11;
    const ALT: u32 = 0x12;
    const SHIFT: u32 = 0x10;
    const R: u32 = 0x52;
    const F: u32 = 0x46;

    fn set() -> ComboSet {
        ComboSet::new(&[
            ("main".into(), vec![CTRL, ALT, R]),
            ("formal".into(), vec![CTRL, ALT, F]),
            ("main-shift".into(), vec![CTRL, ALT, SHIFT, R]),
        ])
    }

    #[test]
    fn modifiers_held_tracks_physical_state() {
        let mut s = set();
        assert!(!s.modifiers_held());
        s.on_key(0xA4, true); // LAlt
        assert!(s.modifiers_held());
        s.on_key(0x09, true); // Tab при зажатом Alt — не клавиша панели
        assert!(s.modifiers_held());
        s.on_key(0x09, false);
        s.on_key(0xA4, false);
        assert!(!s.modifiers_held());
    }

    #[test]
    fn full_press_fires_once_and_release_is_reported() {
        let mut s = set();
        assert_eq!(s.on_key(CTRL, true), None);
        assert_eq!(s.on_key(ALT, true), None);
        assert_eq!(s.on_key(R, true), Some(("main", Edge::Show)));
        assert_eq!(s.on_key(R, true), None); // автоповтор
        assert_eq!(s.on_key(R, false), Some(("main", Edge::Hide)));
        assert_eq!(s.on_key(ALT, false), None);
        assert_eq!(s.on_key(CTRL, false), None);
    }

    #[test]
    fn second_combo_fires_independently() {
        let mut s = set();
        s.on_key(CTRL, true);
        s.on_key(ALT, true);
        assert_eq!(s.on_key(F, true), Some(("formal", Edge::Show)));
        s.on_key(F, false);
        assert_eq!(s.on_key(R, true), Some(("main", Edge::Show)));
    }

    #[test]
    fn extra_modifier_selects_longer_combo_only() {
        let mut s = set();
        s.on_key(0xA2, true); // LCtrl
        s.on_key(0xA4, true); // LAlt
        s.on_key(0xA0, true); // LShift
        assert_eq!(s.on_key(R, true), Some(("main-shift", Edge::Show)));
    }

    #[test]
    fn extra_modifier_without_longer_combo_blocks() {
        let mut s = ComboSet::new(&[("main".into(), vec![CTRL, ALT, R])]);
        s.on_key(CTRL, true);
        s.on_key(ALT, true);
        s.on_key(SHIFT, true);
        assert_eq!(s.on_key(R, true), None);
        s.on_key(R, false);
        s.on_key(SHIFT, false);
        assert_eq!(s.on_key(R, true), Some(("main", Edge::Show)));
    }

    #[test]
    fn repress_after_release_fires_again() {
        let mut s = set();
        s.on_key(CTRL, true);
        s.on_key(ALT, true);
        assert_eq!(s.on_key(R, true), Some(("main", Edge::Show)));
        assert_eq!(s.on_key(R, false), Some(("main", Edge::Hide)));
        assert_eq!(s.on_key(R, true), Some(("main", Edge::Show)));
    }

    /// Отпускание любой клавиши комбинации закрывает её: пользователь может
    /// отпустить Ctrl раньше R, и «короткое нажатие» обязано это заметить.
    #[test]
    fn release_of_any_key_reports_hide_once() {
        let mut s = set();
        s.on_key(CTRL, true);
        s.on_key(ALT, true);
        assert_eq!(s.on_key(R, true), Some(("main", Edge::Show)));
        assert_eq!(s.on_key(CTRL, false), Some(("main", Edge::Hide)));
        // повторных Hide быть не должно
        assert_eq!(s.on_key(ALT, false), None);
        assert_eq!(s.on_key(R, false), None);
    }

    /// Отпускание комбинации, которая не срабатывала (лишний модификатор),
    /// молчит: иначе короткое нажатие Ctrl+Alt+Shift+R применило бы стиль.
    #[test]
    fn release_without_fire_is_silent() {
        let mut s = ComboSet::new(&[("main".into(), vec![CTRL, ALT, R])]);
        s.on_key(CTRL, true);
        s.on_key(ALT, true);
        s.on_key(SHIFT, true);
        assert_eq!(s.on_key(R, true), None);
        assert_eq!(s.on_key(R, false), Some(("main", Edge::Hide)));
    }

    #[test]
    fn double_ctrl_fires_on_second_release() {
        let mut s = ComboSet::new(&[("layout".into(), vec![VK_DOUBLE_TAP, CTRL]), ("main".into(), vec![CTRL, ALT, R])]);
        assert_eq!(s.on_key_at(0xA2, true, 0), None);
        assert_eq!(s.on_key_at(0xA2, false, 80), None);
        assert_eq!(s.on_key_at(0xA3, true, 200), None); // правый Ctrl — тот же
        assert_eq!(s.on_key_at(0xA3, false, 260), Some(("layout", Edge::Double)));
        // третий тап сразу — не двойное (счёт начался заново)
        s.on_key_at(CTRL, true, 300);
        assert_eq!(s.on_key_at(CTRL, false, 350), None);
    }

    #[test]
    fn double_tap_is_broken_by_other_keys_slowness_and_holds() {
        let mut s = ComboSet::new(&[("layout".into(), vec![VK_DOUBLE_TAP, CTRL])]);
        // Ctrl+C между тапами
        s.on_key_at(CTRL, true, 0);
        s.on_key_at(CTRL, false, 50);
        s.on_key_at(CTRL, true, 100);
        s.on_key_at(0x43, true, 120);
        s.on_key_at(0x43, false, 150);
        assert_eq!(s.on_key_at(CTRL, false, 180), None);
        // слишком медленно
        s.on_key_at(CTRL, true, 1000);
        s.on_key_at(CTRL, false, 1050);
        s.on_key_at(CTRL, true, 1600);
        assert_eq!(s.on_key_at(CTRL, false, 1650), None);
        // долгое удержание — не тап
        s.on_key_at(CTRL, true, 3000);
        s.on_key_at(CTRL, false, 3050);
        s.on_key_at(CTRL, true, 3100);
        assert_eq!(s.on_key_at(CTRL, false, 3500), None);
    }

    #[test]
    fn empty_combos_are_skipped() {
        let s = ComboSet::new(&[("x".into(), vec![])]);
        assert!(s.is_empty());
    }

    #[test]
    fn release_of_key_pressed_before_tracking_is_ignored() {
        let mut s = set();
        assert_eq!(s.on_key(R, false), None);
        assert_eq!(s.on_key(CTRL, false), None);
    }
}
