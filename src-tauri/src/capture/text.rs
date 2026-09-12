//! Чистые хелперы захвата текста (без WinAPI — юнит-тестируются).

/// Форматы клипборда, которые НЕ снимаются в снапшот: их хендлы — не HGLOBAL
/// (HBITMAP, HENHMETAFILE, HPALETTE…) либо это owner-display. Растровые данные
/// всё равно сохраняются через синтезируемые CF_DIB/CF_DIBV5, а при восстановлении
/// система синтезирует CF_BITMAP обратно.
pub fn is_snapshot_format(fmt: u32) -> bool {
    !matches!(
        fmt,
        2 /* CF_BITMAP */ | 3 /* CF_METAFILEPICT */ | 9 /* CF_PALETTE */
        | 14 /* CF_ENHMETAFILE */ | 0x80 /* CF_OWNERDISPLAY */
        | 0x82 /* CF_DSPBITMAP */ | 0x83 /* CF_DSPMETAFILEPICT */
        | 0x8E /* CF_DSPENHMETAFILE */
    )
}

/// Верхняя граница одного формата в снапшоте (гигантские delayed-render данные
/// Excel и т.п. не тянем — лучше потерять экзотический формат, чем зависнуть).
pub const MAX_FORMAT_BYTES: usize = 32 * 1024 * 1024;

/// Имя процесса из списка настроек совпадает с exe окна с фокусом
/// (без учёта регистра, суффикс `.exe` необязателен).
pub fn matches_process(list: &[String], exe: &str) -> bool {
    let exe = exe.to_ascii_lowercase();
    let stem = exe.strip_suffix(".exe").unwrap_or(&exe);
    list.iter().any(|p| {
        let p = p.trim().to_ascii_lowercase();
        !p.is_empty() && (p == exe || p == stem || p.strip_suffix(".exe") == Some(stem))
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
    Cr,
}

/// Стиль переводов строк исходника — чтобы при вставке вернуть такой же.
pub fn detect_line_ending(s: &str) -> LineEnding {
    if s.contains("\r\n") {
        LineEnding::CrLf
    } else if s.contains('\r') {
        LineEnding::Cr
    } else {
        LineEnding::Lf
    }
}

/// `\r\n` и `\r` → `\n` (для UI и модели).
pub fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Обратное преобразование для вставки (фаза 5).
#[allow(dead_code)]
pub fn apply_line_ending(s: &str, le: LineEnding) -> String {
    match le {
        LineEnding::Lf => s.to_string(),
        LineEnding::CrLf => s.replace('\n', "\r\n"),
        LineEnding::Cr => s.replace('\n', "\r"),
    }
}

/// Wide-строка из буфера до первого NUL.
pub fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_like_formats_are_skipped_but_dib_and_text_kept() {
        assert!(!is_snapshot_format(2));
        assert!(!is_snapshot_format(14));
        assert!(is_snapshot_format(1)); // CF_TEXT
        assert!(is_snapshot_format(8)); // CF_DIB
        assert!(is_snapshot_format(13)); // CF_UNICODETEXT
        assert!(is_snapshot_format(15)); // CF_HDROP
        assert!(is_snapshot_format(0xC004)); // зарегистрированный формат
    }

    #[test]
    fn process_matching_is_case_insensitive_and_exe_optional() {
        let list = vec!["Code.exe".to_string(), "idea64".to_string(), " ".to_string()];
        assert!(matches_process(&list, "code.exe"));
        assert!(matches_process(&list, "CODE.EXE"));
        assert!(matches_process(&list, "idea64.exe"));
        assert!(!matches_process(&list, "notepad.exe"));
        assert!(!matches_process(&list, ""));
    }

    #[test]
    fn line_endings_roundtrip() {
        let crlf = "a\r\nb\r\n";
        assert_eq!(detect_line_ending(crlf), LineEnding::CrLf);
        let n = normalize_newlines(crlf);
        assert_eq!(n, "a\nb\n");
        assert_eq!(apply_line_ending(&n, LineEnding::CrLf), crlf);
        assert_eq!(detect_line_ending("a\rb"), LineEnding::Cr);
        assert_eq!(normalize_newlines("a\rb"), "a\nb");
        assert_eq!(detect_line_ending("plain"), LineEnding::Lf);
    }

    #[test]
    fn wide_stops_at_nul() {
        let w: Vec<u16> = "hi\0junk".encode_utf16().collect();
        assert_eq!(wide_to_string(&w), "hi");
        let w: Vec<u16> = "no nul".encode_utf16().collect();
        assert_eq!(wide_to_string(&w), "no nul");
    }
}
