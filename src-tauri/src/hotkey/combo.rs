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
}

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
}

impl ComboSet {
    /// Пустые комбинации пропускаются молча.
    pub fn new(combos: &[(String, Vec<u32>)]) -> Self {
        let entries = combos
            .iter()
            .filter(|(_, keys)| !keys.is_empty() && keys.len() <= 8)
            .map(|(id, keys)| (id.clone(), ComboTracker::new(keys)))
            .collect();
        Self { entries, mods_down: Vec::new() }
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Событие клавиатуры. `Some(id)` — комбинация `id` только что сложилась.
    pub fn on_key(&mut self, vk: u32, down: bool) -> Option<&str> {
        let nvk = normalize_vk(vk);
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
        for (i, (_, tracker)) in self.entries.iter_mut().enumerate() {
            if tracker.on_key(nvk, down) == Some(Edge::Show) && fired.is_none() {
                // точное совпадение модификаторов
                let extra_mod = self.mods_down.iter().any(|&m| !tracker.contains(m));
                if !extra_mod {
                    fired = Some(i);
                }
            }
        }
        fired.map(|i| self.entries[i].0.as_str())
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
    fn full_press_fires_once_and_release_is_silent() {
        let mut s = set();
        assert_eq!(s.on_key(CTRL, true), None);
        assert_eq!(s.on_key(ALT, true), None);
        assert_eq!(s.on_key(R, true), Some("main"));
        assert_eq!(s.on_key(R, true), None); // автоповтор
        assert_eq!(s.on_key(R, false), None);
        assert_eq!(s.on_key(ALT, false), None);
        assert_eq!(s.on_key(CTRL, false), None);
    }

    #[test]
    fn second_combo_fires_independently() {
        let mut s = set();
        s.on_key(CTRL, true);
        s.on_key(ALT, true);
        assert_eq!(s.on_key(F, true), Some("formal"));
        s.on_key(F, false);
        assert_eq!(s.on_key(R, true), Some("main"));
    }

    #[test]
    fn extra_modifier_selects_longer_combo_only() {
        let mut s = set();
        s.on_key(0xA2, true); // LCtrl
        s.on_key(0xA4, true); // LAlt
        s.on_key(0xA0, true); // LShift
        assert_eq!(s.on_key(R, true), Some("main-shift"));
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
        assert_eq!(s.on_key(R, true), Some("main"));
    }

    #[test]
    fn repress_after_release_fires_again() {
        let mut s = set();
        s.on_key(CTRL, true);
        s.on_key(ALT, true);
        assert_eq!(s.on_key(R, true), Some("main"));
        assert_eq!(s.on_key(R, false), None);
        assert_eq!(s.on_key(R, true), Some("main"));
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
