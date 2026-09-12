//! Перевод отдельным движком — DeepL (Free API).
//!
//! Почему не моделью: перевод у DeepL и точнее на коротких сообщениях, и
//! отвечает за ~0,3 с вместо секунды-двух, и не тратит квоту Gemini. Модель
//! остаётся запасным вариантом (`translator = "gemini"` или нет ключа DeepL) —
//! тогда работает обычный промпт стиля.
//!
//! Ключ живёт только в Credential Manager (`secrets.rs`), как и ключ Gemini.
//! Free-ключи оканчиваются на `:fx` и ходят на другой хост, чем платные.

use serde::Deserialize;

use crate::ai::{AiError, CONNECT_TIMEOUT, REQUEST_TIMEOUT};

const FREE_HOST: &str = "https://api-free.deepl.com";
const PRO_HOST: &str = "https://api.deepl.com";

/// Хост по виду ключа: у бесплатных аккаунтов ключ оканчивается на `:fx`.
/// `RESTYLE_DEEPL_BASE_URL` — мок в тестах.
fn base_url(key: &str) -> String {
    if let Some(url) = crate::ai::base_url_override("RESTYLE_DEEPL_BASE_URL") {
        return url;
    }
    if key.trim_end().ends_with(":fx") { FREE_HOST.into() } else { PRO_HOST.into() }
}

#[derive(Deserialize)]
struct TranslateResponse {
    translations: Vec<Translation>,
}

#[derive(Deserialize)]
struct Translation {
    text: String,
}

/// Остаток бесплатной квоты: DeepL считает символы за расчётный период.
#[derive(Clone, Copy, Debug, Deserialize, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/bindings/")]
pub struct DeeplUsage {
    // DeepL отдаёт snake_case, фронту отдаём camelCase — отсюда alias.
    #[serde(alias = "character_count")]
    #[ts(type = "number")]
    pub character_count: u64,
    #[serde(alias = "character_limit")]
    #[ts(type = "number")]
    pub character_limit: u64,
}

fn client() -> Result<reqwest::Client, AiError> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| AiError::Other(e.to_string()))
}

/// Коды ошибок DeepL отличаются от общих HTTP: 456 — кончилась квота.
fn map_status(code: u16, body: &str) -> AiError {
    match code {
        401 | 403 => AiError::Auth,
        429 => AiError::RateLimit,
        456 => AiError::Other("Квота DeepL исчерпана".into()),
        400 => AiError::Other(format!("DeepL отклонил запрос: {}", body.trim())),
        c => AiError::Server(c, body.trim().to_string()),
    }
}

fn map_send(e: reqwest::Error) -> AiError {
    if e.is_timeout() {
        AiError::Timeout
    } else if e.is_connect() || e.is_request() {
        AiError::Network
    } else {
        AiError::Other(e.to_string())
    }
}

/// Переводит текст целиком. Переводы строк DeepL сохраняет сам
/// (`preserve_formatting`), поэтому исходную разбивку возвращать не нужно.
pub async fn translate(key: &str, text: &str, target_lang: &str) -> Result<String, AiError> {
    if text.trim().is_empty() {
        return Ok(text.to_string());
    }
    let url = format!("{}/v2/translate", base_url(key));
    let body = serde_json::json!({
        "text": [text],
        "target_lang": target_lang,
        "preserve_formatting": true,
    });
    let resp = client()?
        .post(url)
        .header("Authorization", format!("DeepL-Auth-Key {key}"))
        .json(&body)
        .send()
        .await
        .map_err(map_send)?;
    let status = resp.status().as_u16();
    let text_body = resp.text().await.map_err(map_send)?;
    if status != 200 {
        return Err(map_status(status, &text_body));
    }
    let parsed: TranslateResponse =
        serde_json::from_str(&text_body).map_err(|e| AiError::Other(format!("DeepL: {e}")))?;
    parsed
        .translations
        .into_iter()
        .next()
        .map(|t| t.text)
        .ok_or_else(|| AiError::Other("DeepL вернул пустой ответ".into()))
}

/// Проверка ключа + остаток квоты (окно настроек и мастер).
pub async fn usage(key: &str) -> Result<DeeplUsage, AiError> {
    let url = format!("{}/v2/usage", base_url(key));
    let resp = client()?
        .get(url)
        .header("Authorization", format!("DeepL-Auth-Key {key}"))
        .send()
        .await
        .map_err(map_send)?;
    let status = resp.status().as_u16();
    let body = resp.text().await.map_err(map_send)?;
    if status != 200 {
        return Err(map_status(status, &body));
    }
    serde_json::from_str(&body).map_err(|e| AiError::Other(format!("DeepL: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_key_goes_to_free_host() {
        std::env::remove_var("RESTYLE_DEEPL_BASE_URL");
        assert_eq!(base_url("abc-123:fx"), FREE_HOST);
        assert_eq!(base_url("abc-123"), PRO_HOST);
    }

    #[test]
    fn status_codes_map_to_user_errors() {
        assert_eq!(map_status(403, ""), AiError::Auth);
        assert_eq!(map_status(429, ""), AiError::RateLimit);
        assert!(matches!(map_status(456, ""), AiError::Other(_)));
        assert!(matches!(map_status(503, "oops"), AiError::Server(503, _)));
    }

    #[test]
    fn parses_translation_body() {
        let body = r#"{"translations":[{"detected_source_language":"RU","text":"Hello"}]}"#;
        let parsed: TranslateResponse = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.translations[0].text, "Hello");
    }

    #[test]
    fn parses_usage_body() {
        let u: DeeplUsage =
            serde_json::from_str(r#"{"character_count":1200,"character_limit":500000}"#).unwrap();
        assert_eq!(u.character_count, 1200);
        assert_eq!(u.character_limit, 500_000);
    }
}
