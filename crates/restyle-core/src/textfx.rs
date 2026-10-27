//! Локальные преобразования текста без модели: регистр и исправление
//! раскладки («ghbdtn» → «привет»). Чистые функции, покрыты тестами.

/// Префикс «стиля»-форматирования в `select_style` и истории: `format:upper`.
pub const FORMAT_PREFIX: &str = "format:";
/// id записи истории для исправленной раскладки.
pub const LAYOUT_ID: &str = "layout";

/// Регистр: `sentence` | `lower` | `upper` | `title` | `toggle`.
pub fn apply_case(kind: &str, text: &str) -> Option<String> {
    Some(match kind {
        "upper" => text.to_uppercase(),
        "lower" => text.to_lowercase(),
        "sentence" => sentence_case(text),
        "title" => title_case(text),
        "toggle" => text
            .chars()
            .flat_map(|c| -> Box<dyn Iterator<Item = char>> {
                if c.is_uppercase() {
                    Box::new(c.to_lowercase())
                } else {
                    Box::new(c.to_uppercase())
                }
            })
            .collect(),
        _ => return None,
    })
}

/// Всё строчными, заглавная — в начале текста, после `.!?…` и перевода строки.
fn sentence_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cap = true;
    for c in text.chars() {
        if c.is_alphabetic() {
            if cap {
                out.extend(c.to_uppercase());
                cap = false;
            } else {
                out.extend(c.to_lowercase());
            }
        } else {
            if matches!(c, '.' | '!' | '?' | '…' | '\n') {
                cap = true;
            } else if c.is_numeric() {
                cap = false;
            }
            out.push(c);
        }
    }
    out
}

/// Каждое слово с заглавной, остальное строчными.
fn title_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word_start = true;
    for c in text.chars() {
        if c.is_alphanumeric() {
            if word_start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            word_start = false;
        } else {
            // Апостроф внутри слова (don't, п'ять) слово не рвёт.
            word_start = !matches!(c, '\'' | '’' | 'ʼ');
            out.push(c);
        }
    }
    out
}

/// Итог локального преобразования с учётом выделения.
#[derive(Debug, PartialEq, Eq)]
pub struct Splice {
    /// Что вставлять: весь текст поля (или только выделение, см. ниже).
    pub paste: String,
    /// Что показать в панели: преобразованный фрагмент.
    pub shown: String,
    /// Вставлять только в выделение (Ctrl+V без Ctrl+A): выделение не нашлось
    /// в тексте поля по смещению.
    pub only_selection: bool,
    /// Длина не изменилась — каретку/выделение можно вернуть на место.
    pub keep_caret: bool,
}

/// Применить `f` к выделению (`sel` = текст и начало в символах `text`), а без
/// него — ко всему тексту. Непустое выделение вклеивается обратно в текст поля.
pub fn apply_to(
    text: &str,
    sel: Option<(&str, usize)>,
    mut f: impl FnMut(&str) -> Option<String>,
) -> Option<Splice> {
    if let Some((sel_text, start)) = sel.filter(|(t, _)| !t.trim().is_empty()) {
        let out = f(sel_text)?;
        let keep = out.chars().count() == sel_text.chars().count();
        let chars: Vec<char> = text.chars().collect();
        let end = start + sel_text.chars().count();
        if end <= chars.len() && chars[start..end].iter().copied().eq(sel_text.chars()) {
            let mut paste: String = chars[..start].iter().collect();
            paste.push_str(&out);
            paste.extend(&chars[end..]);
            return Some(Splice { paste, shown: out, only_selection: false, keep_caret: keep });
        }
        return Some(Splice { paste: out.clone(), shown: out, only_selection: true, keep_caret: false });
    }
    let out = f(text)?;
    let keep = out.chars().count() == text.chars().count();
    Some(Splice { paste: out.clone(), shown: out, only_selection: false, keep_caret: keep })
}

/// Какая кириллическая раскладка стоит у пользователя.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cyr {
    Ru,
    Uk,
}

/// Куда перевели текст — для переключения раскладки окна.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    En,
    Cyr(Cyr),
}

impl Layout {
    /// Первичный LANGID Windows (`PRIMARYLANGID`).
    pub fn primary_lang(self) -> u16 {
        match self {
            Layout::En => 0x09,
            Layout::Cyr(Cyr::Ru) => 0x19,
            Layout::Cyr(Cyr::Uk) => 0x22,
        }
    }
}

/// Клавиши US-QWERTY и то, что те же клавиши дают в ЙЦУКЕН (русская раскладка).
const EN: &str = "qwertyuiop[]asdfghjkl;'zxcvbnm,./`QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>?~@#$^&|";
const RU: &str = "йцукенгшщзхъфывапролджэячсмитьбю.ёЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,Ё\"№;:?/";

/// Украинская раскладка отличается от русской четырьмя клавишами.
fn uk_char(ru: char) -> char {
    match ru {
        'ы' => 'і',
        'Ы' => 'І',
        'ъ' => 'ї',
        'Ъ' => 'Ї',
        'э' => 'є',
        'Э' => 'Є',
        'ё' => '\'',
        other => other,
    }
}

fn en_to_cyr(c: char, cyr: Cyr) -> char {
    match EN.chars().position(|e| e == c) {
        Some(i) => {
            let ru = RU.chars().nth(i).unwrap();
            if cyr == Cyr::Uk { uk_char(ru) } else { ru }
        }
        None => c,
    }
}

fn cyr_to_en(c: char) -> char {
    let ru = match c {
        'і' => 'ы',
        'І' => 'Ы',
        'ї' => 'ъ',
        'Ї' => 'Ъ',
        'є' => 'э',
        'Є' => 'Э',
        'ґ' => return 'u',
        'Ґ' => return 'U',
        other => other,
    };
    match RU.chars().position(|r| r == ru) {
        Some(i) => EN.chars().nth(i).unwrap(),
        None => c,
    }
}

fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}')
}

/// Текст, набранный не в той раскладке. Направление — по буквам: латиницы
/// больше — это кириллица, набранная на английской раскладке, и наоборот.
/// Повторное нажатие возвращает как было. `None` — букв нет вовсе.
pub fn fix_layout(text: &str, cyr: Cyr) -> Option<(String, Layout)> {
    let latin = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let cyrillic = text.chars().filter(|c| is_cyrillic(*c)).count();
    if latin == 0 && cyrillic == 0 {
        return None;
    }
    if latin > cyrillic {
        Some((text.chars().map(|c| en_to_cyr(c, cyr)).collect(), Layout::Cyr(cyr)))
    } else {
        Some((text.chars().map(cyr_to_en).collect(), Layout::En))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_aligned() {
        assert_eq!(EN.chars().count(), RU.chars().count());
    }

    #[test]
    fn fixes_russian_typed_on_english_layout() {
        let (out, to) = fix_layout("ghbdtn? rfr ltkf? e vtyz yjhvfkmyj", Cyr::Ru).unwrap();
        assert_eq!(out, "привет, как дела, у меня нормально");
        assert_eq!(to, Layout::Cyr(Cyr::Ru));
        let (out, _) = fix_layout("Ghbdtn/ Xnj ltkftim& @Jr@", Cyr::Ru).unwrap();
        assert_eq!(out, "Привет. Что делаешь? \"Ок\"");
    }

    #[test]
    fn fixes_english_typed_on_russian_layout_and_back() {
        let (out, to) = fix_layout("руддщ цщкдв", Cyr::Ru).unwrap();
        assert_eq!(out, "hello world");
        assert_eq!(to, Layout::En);
        // Второе нажатие — обратно.
        let (back, _) = fix_layout(&out, Cyr::Ru).unwrap();
        assert_eq!(back, "руддщ цщкдв");
    }

    #[test]
    fn ukrainian_letters() {
        let (out, _) = fix_layout("ghbdsn? zr cghfdb", Cyr::Uk).unwrap();
        assert_eq!(out, "привіт, як справи");
        let (en, _) = fix_layout("їжак є", Cyr::Uk).unwrap();
        assert_eq!(en, "];fr '");
    }

    #[test]
    fn keeps_digits_and_nothing_to_fix() {
        let (out, _) = fix_layout("123 ntcn", Cyr::Ru).unwrap();
        assert_eq!(out, "123 тест");
        assert_eq!(fix_layout("123 !", Cyr::Ru), None);
    }

    #[test]
    fn selection_is_spliced_into_text() {
        let s = apply_to("hello WORLD bye", Some(("WORLD", 6)), |t| apply_case("lower", t)).unwrap();
        assert_eq!(s.paste, "hello world bye");
        assert_eq!(s.shown, "world");
        assert!(!s.only_selection && s.keep_caret);
        // Смещение не совпало — вставляем только во выделение.
        let s = apply_to("abc", Some(("zz", 1)), |t| apply_case("upper", t)).unwrap();
        assert_eq!((s.paste.as_str(), s.only_selection), ("ZZ", true));
        // Пустое выделение (каретка) — весь текст.
        let s = apply_to("ntcn", Some(("", 2)), |t| fix_layout(t, Cyr::Ru).map(|x| x.0)).unwrap();
        assert_eq!(s.paste, "тест");
    }

    #[test]
    fn cases() {
        assert_eq!(apply_case("upper", "Привет, мир").unwrap(), "ПРИВЕТ, МИР");
        assert_eq!(apply_case("lower", "ПРИВЕТ World").unwrap(), "привет world");
        assert_eq!(
            apply_case("sentence", "пРИВЕТ. как ДЕЛА?да\nНУ ладно").unwrap(),
            "Привет. Как дела?Да\nНу ладно"
        );
        assert_eq!(apply_case("title", "hello WORLD, don't п'ять").unwrap(), "Hello World, Don't П'ять");
        assert_eq!(apply_case("toggle", "Hello Мир").unwrap(), "hELLO мИР");
        assert_eq!(apply_case("nope", "x"), None);
    }
}
