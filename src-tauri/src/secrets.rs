//! API-ключ — только в Windows Credential Manager через `keyring`.
//! Никогда не возвращается во фронтенд и не пишется в store/логи.
//! Вызовы keyring блокирующие — из async-кода оборачивать в `spawn_blocking`.

use keyring::{Entry, Error};

const SERVICE: &str = "Restyle";
const GEMINI_USER: &str = "gemini-api-key";

fn entry() -> Result<Entry, String> {
    Entry::new(SERVICE, GEMINI_USER).map_err(|e| format!("keyring: {e}"))
}

pub fn set_api_key(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return clear_api_key();
    }
    entry()?.set_password(key).map_err(|e| format!("keyring set: {e}"))
}

/// `Ok(None)` — ключ не задан.
pub fn get_api_key() -> Result<Option<String>, String> {
    match entry()?.get_password() {
        // cmdkey и другие внешние утилиты могут оставить хвостовой NUL/пробелы
        Ok(k) => {
            let k = k.trim_matches(|c: char| c.is_whitespace() || c == '\0' || c == '\u{fffd}');
            if k.is_empty() { Ok(None) } else { Ok(Some(k.to_string())) }
        }
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("keyring get: {e}")),
    }
}

pub fn has_api_key() -> bool {
    matches!(get_api_key(), Ok(Some(_)))
}

pub fn clear_api_key() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("keyring delete: {e}")),
    }
}
