//! AI-клиент: трейт `Provider` (Gemini сейчас; Anthropic/OpenAI — позже без
//! переписывания control-цикла), запрос/ошибки, системный промпт.
//!
//! Системный промпт — `prompts/system.txt` (вшивается при сборке); если в
//! каталоге конфига приложения лежит `system_prompt.txt`, он имеет приоритет
//! (правка без пересборки).

pub mod gemini;

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedSender;

/// Таймаут всего запроса (включая стрим).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(6);

pub const DEFAULT_SYSTEM_PROMPT: &str = include_str!("../../prompts/system.txt");

/// Системный промпт: override из `<config>/system_prompt.txt`, иначе вшитый.
pub fn system_prompt(config_dir: Option<std::path::PathBuf>) -> String {
    config_dir
        .map(|d| d.join("system_prompt.txt"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim_start_matches('\u{feff}').trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_SYSTEM_PROMPT.trim().to_string())
}

/// Модель из каталога провайдера (окно настроек).
#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ModelInfo {
    /// Имя для API (`gemini-3.5-flash-lite`).
    pub id: String,
    pub display_name: String,
    /// «Быстрая» модель (flash/lite) — их и стоит выбирать для переписывания.
    pub fast: bool,
}

#[derive(Clone, Debug)]
pub struct RewriteRequest {
    pub system_prompt: String,
    pub style_name: String,
    pub style_instruction: String,
    pub text: String,
    /// JPEG base64 (без data:-префикса), если скриншот есть.
    pub screenshot_jpeg_base64: Option<String>,
    pub model: String,
}

impl RewriteRequest {
    /// Пользовательское сообщение для модели.
    pub fn user_message(&self) -> String {
        format!(
            "Целевой стиль: {}\nИнструкция стиля: {}\n\nТекст для переписывания:\n{}",
            self.style_name, self.style_instruction, self.text
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiError {
    NoApiKey,
    Auth,
    RateLimit,
    Network,
    Timeout,
    /// Ответ заблокирован фильтрами (причина от провайдера).
    Blocked(String),
    Server(u16, String),
    Other(String),
}

impl AiError {
    /// Человекочитаемый текст для оверлея.
    pub fn user_message(&self) -> String {
        match self {
            AiError::NoApiKey => "Не задан API-ключ — открой настройки".into(),
            AiError::Auth => "Проверь API-ключ".into(),
            AiError::RateLimit => "Лимит запросов — попробуй позже".into(),
            AiError::Network => "Нет соединения".into(),
            AiError::Timeout => "Сервис не ответил за 20 с".into(),
            AiError::Blocked(r) => format!("Ответ заблокирован фильтром ({r})"),
            AiError::Server(code, _) => format!("Сервис недоступен ({code})"),
            AiError::Other(m) => format!("Ошибка: {m}"),
        }
    }

    /// HTTP-статус → ошибка (общая для провайдеров).
    pub fn from_status(code: u16, body: &str) -> Self {
        match code {
            400 if body.contains("API_KEY_INVALID") || body.contains("API key") => AiError::Auth,
            401 | 403 => AiError::Auth,
            429 => AiError::RateLimit,
            500..=599 => AiError::Server(code, body.chars().take(200).collect()),
            _ => AiError::Other(format!("HTTP {code}: {}", body.chars().take(200).collect::<String>())),
        }
    }

    pub fn from_reqwest(e: &reqwest::Error) -> Self {
        if e.is_timeout() {
            AiError::Timeout
        } else if e.is_connect() || e.is_request() {
            AiError::Network
        } else if e.is_body() || e.is_decode() {
            AiError::Network
        } else {
            // URL из текста ошибки вырезаем: в нём могут быть параметры запроса.
            let mut msg = e.to_string();
            if let Some(url) = e.url() {
                msg = msg.replace(url.as_str(), "<url>");
            }
            AiError::Other(msg)
        }
    }
}

/// Подмена адреса API из переменной окружения (мок в тестах и замерах).
/// В релизной сборке — только на локальный адрес: иначе переменная окружения
/// могла бы увести ключ на чужой хост.
pub fn base_url_override(var: &str) -> Option<String> {
    let url = std::env::var(var).ok()?.trim().trim_end_matches('/').to_string();
    if url.is_empty() {
        return None;
    }
    if cfg!(debug_assertions) || is_loopback_url(&url) {
        Some(url)
    } else {
        eprintln!("[restyle] {var} игнорируется: в релизе разрешён только localhost");
        None
    }
}

fn is_loopback_url(url: &str) -> bool {
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://")).unwrap_or("");
    let host = rest.split(['/', ':']).next().unwrap_or("");
    matches!(host, "127.0.0.1" | "localhost") || rest.starts_with("[::1]")
}

#[cfg(test)]
mod override_tests {
    use super::is_loopback_url;

    #[test]
    fn only_loopback_hosts_pass() {
        assert!(is_loopback_url("http://127.0.0.1:8765"));
        assert!(is_loopback_url("http://localhost:8765/v1"));
        assert!(is_loopback_url("http://[::1]:8765"));
        assert!(!is_loopback_url("https://evil.example.com"));
        assert!(!is_loopback_url("http://127.0.0.1.evil.com"));
        assert!(!is_loopback_url("http://localhost.evil.com"));
    }
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Провайдер переписывания. Чанки текста шлются в `chunks` по мере прихода;
/// результат — полный текст. Отмена = drop future (JoinHandle::abort):
/// HTTP-стрим рвётся вместе с ним.
pub trait Provider: Send + Sync {
    #[allow(dead_code)]
    fn name(&self) -> &'static str;
    fn rewrite<'a>(
        &'a self,
        api_key: &'a str,
        req: &'a RewriteRequest,
        chunks: UnboundedSender<String>,
    ) -> BoxFuture<'a, Result<String, AiError>>;
}

/// Провайдер по имени модели (пока всегда Gemini).
pub fn provider_for(_model: &str) -> Box<dyn Provider> {
    Box::new(gemini::Gemini::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping() {
        assert_eq!(AiError::from_status(401, ""), AiError::Auth);
        assert_eq!(AiError::from_status(403, ""), AiError::Auth);
        assert_eq!(AiError::from_status(400, r#"{"error":{"status":"INVALID_ARGUMENT","message":"API key not valid","details":[{"reason":"API_KEY_INVALID"}]}}"#), AiError::Auth);
        assert_eq!(AiError::from_status(429, ""), AiError::RateLimit);
        assert!(matches!(AiError::from_status(503, "x"), AiError::Server(503, _)));
        assert!(matches!(AiError::from_status(418, "x"), AiError::Other(_)));
    }

    #[test]
    fn embedded_prompt_is_used_without_override() {
        let p = system_prompt(Some(std::env::temp_dir().join("restyle-no-such-dir")));
        assert!(p.contains("Верни только переписанный текст"));
    }

    #[test]
    fn user_message_contains_style_and_text() {
        let r = RewriteRequest {
            system_prompt: String::new(),
            style_name: "Formal".into(),
            style_instruction: "be formal".into(),
            text: "привет".into(),
            screenshot_jpeg_base64: None,
            model: "m".into(),
        };
        let m = r.user_message();
        assert!(m.contains("Formal") && m.contains("be formal") && m.ends_with("привет"));
    }
}
