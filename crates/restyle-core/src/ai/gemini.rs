//! Gemini API: `models/{model}:streamGenerateContent?alt=sse`.
//! SSE-парсер чистый (`parse_sse_data`, `extract_text`) — юнит-тестируется.
//! `RESTYLE_GEMINI_BASE_URL` — override базового URL (прокси, локальный мок).

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

use super::{AiError, BoxFuture, ModelInfo, Provider, RewriteRequest, CONNECT_TIMEOUT, REQUEST_TIMEOUT};

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

/// Годится ли модель для переписывания текста. Чёрного списка мало: ключ
/// отдаёт картиночные (`nano-banana`), музыкальные (`lyria`), роботов,
/// «глубокое исследование» и прочее — половина из них умеет `generateContent`.
/// Поэтому белый список: только `gemini-*` семейств flash/pro, и уже из них
/// вычёркиваем нетекстовые ветки.
fn is_text_model(id: &str) -> bool {
    const SKIP: &[&str] = &[
        "embedding", "embed", "imagen", "image", "banana", "veo", "lyria", "tts",
        "audio", "speech", "transcribe", "live", "realtime", "aqa", "vision",
        "learnlm", "robotics", "computer-use", "customtools", "research",
        "antigravity", "guard", "safety",
    ];
    let id = id.to_ascii_lowercase();
    if !id.starts_with("gemini-") {
        return false;
    }
    if !(id.contains("flash") || id.contains("pro")) {
        return false;
    }
    !SKIP.iter().any(|bad| id.contains(bad))
}

/// Адрес Vertex AI: `…/projects/{project}/locations/{location}/publishers/google/models/…`
/// (express-адрес без проекта Google ведёт в europe-west1, где части моделей нет).
pub const VERTEX_BASE_URL: &str = "https://aiplatform.googleapis.com/v1beta1";

/// Текстовые модели Vertex (проверено вживую 2026-11-26): `gemini-3.6-flash` и все 2.x там 404.
pub const VERTEX_MODELS: &[&str] = &[
    "gemini-3.8-flash",
    "gemini-3.7-flash",
    "gemini-3.5-flash",
    "gemini-3.5-flash-lite",
    "gemini-3.1-flash-lite",
    "gemini-4-argon",
];

/// Модель, которой заменяется недоступная на Vertex (и по которой проверяется ключ).
pub const VERTEX_DEFAULT_MODEL: &str = "gemini-3.5-flash-lite";

/// Вид ключа Gemini: ключ Google AI Studio или ключ Vertex AI (`AQ.…`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    AiStudio,
    Vertex,
}

impl KeyKind {
    /// По самому ключу: после `trim()` пусто — `None`; `AQ.` (регистр важен: у
    /// настоящих ключей `aq.` не бывает, а ложное срабатывание спрятало бы
    /// ошибку ключа за «нет проекта») — Vertex; остальное — AI Studio.
    pub fn of(key: &str) -> Option<KeyKind> {
        let key = key.trim();
        if key.is_empty() {
            None
        } else if key.starts_with("AQ.") {
            Some(KeyKind::Vertex)
        } else {
            Some(KeyKind::AiStudio)
        }
    }
}

/// Допустим ли id проекта GCP: `^[a-z][a-z0-9-]{4,61}[a-z0-9]$` после `trim()`.
pub fn valid_project(s: &str) -> bool {
    let b = s.trim().as_bytes();
    (6..=63).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b[b.len() - 1] != b'-'
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Допустим ли регион: `^[a-z0-9-]{1,40}$` (без пробелов по краям).
pub(crate) fn valid_location(s: &str) -> bool {
    (1..=40).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

/// Куда слать запросы Vertex: проект GCP и регион (`global`, `europe-west1`…).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VertexTarget {
    pub project: String,
    pub location: String,
}

impl VertexTarget {
    /// `None`, если проект не по правилу `valid_project`. Регион обрезается;
    /// пустой или недопустимый — `global` (как в `Settings::normalize`).
    pub fn new(project: &str, location: &str) -> Option<Self> {
        if !valid_project(project) {
            return None;
        }
        let location = location.trim();
        let location = if valid_location(location) { location } else { "global" };
        Some(Self { project: project.trim().to_string(), location: location.to_string() })
    }
}

/// Модель, которая ответит на Vertex: из `VERTEX_MODELS` — она сама (после
/// `trim()` и без префикса `models/`), иначе `VERTEX_DEFAULT_MODEL`.
pub fn vertex_model(model: &str) -> &str {
    let model = model.trim();
    let model = model.strip_prefix("models/").unwrap_or(model);
    VERTEX_MODELS.iter().copied().find(|m| *m == model).unwrap_or(VERTEX_DEFAULT_MODEL)
}

/// Каталог Vertex: фиксированный список (`models.list` по ключу там не работает).
/// Порядок как у `Gemini::list_models`: быстрые сверху, затем по id.
pub fn vertex_models() -> Vec<ModelInfo> {
    let mut out: Vec<ModelInfo> = VERTEX_MODELS
        .iter()
        .map(|id| ModelInfo {
            id: id.to_string(),
            display_name: id.to_string(),
            fast: id.contains("flash") || id.contains("lite"),
        })
        .collect();
    out.sort_by(|a, b| b.fast.cmp(&a.fast).then_with(|| a.id.cmp(&b.id)));
    out
}

/// Адрес метода модели на Vertex. Для `streamGenerateContent` — `?alt=sse`.
/// Ключ в адрес не попадает (он в заголовке).
pub fn vertex_url(base: &str, target: &VertexTarget, model: &str, method: &str) -> String {
    let model = model.trim();
    let model = model.strip_prefix("models/").unwrap_or(model);
    let mut url = format!(
        "{}/projects/{}/locations/{}/publishers/google/models/{}:{}",
        base.trim_end_matches('/'),
        target.project,
        target.location,
        model,
        method
    );
    if method == "streamGenerateContent" {
        url.push_str("?alt=sse");
    }
    url
}

pub struct Gemini {
    client: reqwest::Client,
    base_url: String,
    vertex_base_url: String,
    vertex: Option<VertexTarget>,
}

impl Default for Gemini {
    fn default() -> Self {
        Self::new()
    }
}

impl Gemini {
    pub fn new() -> Self {
        let base_url = super::base_url_override("RESTYLE_GEMINI_BASE_URL")
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let vertex_base_url = super::base_url_override("RESTYLE_VERTEX_BASE_URL")
            .unwrap_or_else(|| VERTEX_BASE_URL.to_string());
        Self::with_bases(&base_url, &vertex_base_url)
    }

    /// Свои адреса без чтения окружения — для тестов (мок на loopback).
    #[doc(hidden)]
    pub fn with_bases(ai_studio_base: &str, vertex_base: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .expect("reqwest client");
        Self {
            client,
            base_url: ai_studio_base.trim_end_matches('/').to_string(),
            vertex_base_url: vertex_base.trim_end_matches('/').to_string(),
            vertex: None,
        }
    }

    /// Проект и регион для ключа Vertex. Без цели такой ключ даёт
    /// `AiError::NoProject` до всякого запроса. Ключу AI Studio цель не нужна.
    pub fn with_vertex(mut self, vertex: Option<VertexTarget>) -> Self {
        self.vertex = vertex;
        self
    }

    /// Проверка ключа. AI Studio — `list_models`, как раньше. Vertex: нет
    /// проекта — `NoProject` без сети; иначе `countTokens` от «ping» на
    /// `VERTEX_DEFAULT_MODEL` (квоту генерации не тратит; `models.list` по
    /// ключу Vertex не работает).
    pub async fn check_key(&self, key: &str) -> Result<(), AiError> {
        match KeyKind::of(key) {
            None => Err(AiError::NoApiKey),
            Some(KeyKind::AiStudio) => self.list_models(key).await.map(|_| ()),
            Some(KeyKind::Vertex) => {
                let target = self.vertex.as_ref().ok_or(AiError::NoProject)?;
                let url = vertex_url(&self.vertex_base_url, target, VERTEX_DEFAULT_MODEL, "countTokens");
                let resp = self
                    .client
                    .post(url)
                    .header("x-goog-api-key", key.trim())
                    .json(&json!({ "contents": [{ "role": "user", "parts": [{ "text": "ping" }] }] }))
                    .send()
                    .await
                    .map_err(|e| AiError::from_reqwest(&e))?;
                let status = resp.status().as_u16();
                let body = resp.text().await.map_err(|e| AiError::from_reqwest(&e))?;
                if status == 200 {
                    Ok(())
                } else {
                    Err(AiError::from_status_vertex(status, &body))
                }
            }
        }
    }

    /// Каталог моделей ключа: `GET /models`. Ключи видят разный набор (2.5
    /// новым ключам недоступна), поэтому список тянем у API, а не хардкодим.
    /// Ключ Vertex — фиксированный `vertex_models()` без сети.
    pub async fn list_models(&self, key: &str) -> Result<Vec<ModelInfo>, AiError> {
        if KeyKind::of(key) == Some(KeyKind::Vertex) {
            return Ok(vertex_models());
        }
        // Ключ — заголовком, не в URL: текст ошибки reqwest содержит URL целиком
        // и уходил бы в интерфейс и логи.
        let url = format!("{}/models?pageSize=200", self.base_url);
        let resp = self
            .client
            .get(url)
            .header("x-goog-api-key", key)
            .send()
            .await
            .map_err(|e| AiError::from_reqwest(&e))?;
        let status = resp.status().as_u16();
        let body = resp.text().await.map_err(|e| AiError::from_reqwest(&e))?;
        if status != 200 {
            return Err(AiError::from_status(status, &body));
        }
        let parsed: Value = serde_json::from_str(&body).map_err(|e| AiError::Other(e.to_string()))?;
        let mut out = Vec::new();
        for m in parsed["models"].as_array().into_iter().flatten() {
            let id = m["name"].as_str().unwrap_or_default().trim_start_matches("models/");
            let methods = m["supportedGenerationMethods"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                .unwrap_or_default();
            if id.is_empty() || !methods.contains(&"generateContent") || !is_text_model(id) {
                continue;
            }
            out.push(ModelInfo {
                fast: id.contains("flash") || id.contains("lite"),
                display_name: m["displayName"].as_str().unwrap_or(id).to_string(),
                id: id.to_string(),
            });
        }
        // Быстрые — наверх: для переписывания в поле ввода важна задержка.
        out.sort_by(|a, b| b.fast.cmp(&a.fast).then_with(|| a.id.cmp(&b.id)));
        Ok(out)
    }

    /// Конфиг «минимум размышлений»: первый чанк быстрее, задача не требует
    /// рассуждений. 2.5 — `thinkingBudget: 0`; 3.x — `thinkingLevel: minimal`
    /// (3.5-flash-lite, 3.6-flash принимают; 3.8-flash отвечает 400 — тогда
    /// `rewrite` повторяет запрос без thinkingConfig). Старые модели — ничего.
    pub fn thinking_config(model: &str) -> Option<Value> {
        if model.contains("2.5") {
            Some(json!({ "thinkingBudget": 0 }))
        } else if model.starts_with("gemini-3") {
            Some(json!({ "thinkingLevel": "minimal" }))
        } else {
            None
        }
    }

    pub fn build_body(req: &RewriteRequest, with_thinking: bool) -> Value {
        let mut parts = Vec::new();
        if let Some(b64) = &req.screenshot_jpeg_base64 {
            parts.push(json!({ "inlineData": { "mimeType": "image/jpeg", "data": b64 } }));
        }
        parts.push(json!({ "text": req.user_message() }));

        let mut generation_config = json!({ "temperature": 0.3 });
        if with_thinking {
            if let Some(tc) = Self::thinking_config(&req.model) {
                generation_config["thinkingConfig"] = tc;
            }
        }
        json!({
            "systemInstruction": { "parts": [{ "text": req.system_prompt }] },
            "contents": [{ "role": "user", "parts": parts }],
            "generationConfig": generation_config,
        })
    }
}

/// Текст из одного SSE-события (`data: {...}`): все text-части первого кандидата.
/// `Err` — событие сообщает о блокировке.
pub fn extract_text(event: &Value) -> Result<String, AiError> {
    if let Some(reason) = event.pointer("/promptFeedback/blockReason").and_then(Value::as_str) {
        return Err(AiError::Blocked(reason.to_string()));
    }
    let mut out = String::new();
    if let Some(parts) = event.pointer("/candidates/0/content/parts").and_then(Value::as_array) {
        for p in parts {
            if let Some(t) = p.get("text").and_then(Value::as_str) {
                out.push_str(t);
            }
        }
    }
    if out.is_empty() {
        if let Some(fr) = event.pointer("/candidates/0/finishReason").and_then(Value::as_str) {
            if fr != "STOP" && fr != "MAX_TOKENS" {
                return Err(AiError::Blocked(fr.to_string()));
            }
        }
    }
    Ok(out)
}

/// Инкрементальный SSE-парсер: кормим байтами, получаем `data:`-payload'ы
/// завершённых событий (событие заканчивается пустой строкой).
#[derive(Default)]
pub struct SseParser {
    buf: String,
}

impl SseParser {
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        // CRLF (Gemini шлёт именно так) сводим к LF; одиночный CR на границе
        // чанка доклеится следующим push и тоже нормализуется.
        self.buf.push_str(chunk);
        if self.buf.contains("\r\n") {
            self.buf = self.buf.replace("\r\n", "\n");
        }
        let mut events = Vec::new();
        // событие = блок до "\n\n" (терпим и "\r\n\r\n")
        while let Some(idx) = self.buf.find("\n\n") {
            let block = self.buf[..idx].to_string();
            self.buf.drain(..idx + 2);
            let mut data = String::new();
            for line in block.lines() {
                let line = line.trim_end_matches('\r');
                if let Some(rest) = line.strip_prefix("data:") {
                    if !data.is_empty() {
                        data.push('\n');
                    }
                    data.push_str(rest.trim_start());
                }
            }
            if !data.is_empty() {
                events.push(data);
            }
        }
        events
    }
}

impl Provider for Gemini {
    fn name(&self) -> &'static str {
        "gemini"
    }

    fn rewrite<'a>(
        &'a self,
        api_key: &'a str,
        req: &'a RewriteRequest,
        chunks: UnboundedSender<String>,
    ) -> BoxFuture<'a, Result<String, AiError>> {
        Box::pin(async move {
            let vertex = KeyKind::of(api_key) == Some(KeyKind::Vertex);
            // Ключ Vertex: нет проекта — ошибка до сети; модель, которой на Vertex нет,
            // заменяется (настройки не меняются), тело и thinkingConfig считаются по ней.
            let substituted;
            let (url, req, api_key) = if vertex {
                let target = self.vertex.as_ref().ok_or(AiError::NoProject)?;
                let model = vertex_model(&req.model);
                let req = if req.model == model {
                    req
                } else {
                    eprintln!("[restyle] vertex: {} → {model}", req.model.trim());
                    substituted = RewriteRequest { model: model.to_string(), ..req.clone() };
                    &substituted
                };
                (vertex_url(&self.vertex_base_url, target, model, "streamGenerateContent"), req, api_key.trim())
            } else {
                (format!("{}/models/{}:streamGenerateContent?alt=sse", self.base_url, req.model), req, api_key)
            };
            let mut with_thinking = Self::thinking_config(&req.model).is_some();
            let resp = loop {
                let resp = self
                    .client
                    .post(&url)
                    .header("x-goog-api-key", api_key)
                    .json(&Self::build_body(req, with_thinking))
                    .send()
                    .await
                    .map_err(|e| AiError::from_reqwest(&e))?;
                let status = resp.status().as_u16();
                if (200..300).contains(&status) {
                    break resp;
                }
                let body = resp.text().await.unwrap_or_default();
                // Модель не принимает наш thinkingConfig — повторяем без него.
                if status == 400 && with_thinking && body.to_ascii_lowercase().contains("thinking") {
                    eprintln!("[restyle] {}: thinkingConfig отвергнут, повтор без него", req.model);
                    with_thinking = false;
                    continue;
                }
                return Err(if vertex {
                    AiError::from_status_vertex(status, &body)
                } else {
                    AiError::from_status(status, &body)
                });
            };

            let mut stream = resp.bytes_stream();
            let mut parser = SseParser::default();
            let mut full = String::new();
            let mut pending = Vec::new(); // неполный UTF-8 на границе чанка
            while let Some(item) = stream.next().await {
                let bytes = item.map_err(|e| AiError::from_reqwest(&e))?;
                pending.extend_from_slice(&bytes);
                let valid_up_to = match std::str::from_utf8(&pending) {
                    Ok(_) => pending.len(),
                    Err(e) => e.valid_up_to(),
                };
                let text: String = String::from_utf8_lossy(&pending[..valid_up_to]).into_owned();
                pending.drain(..valid_up_to);
                for data in parser.push(&text) {
                    if data == "[DONE]" {
                        continue;
                    }
                    let event: Value = match serde_json::from_str(&data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let piece = extract_text(&event)?;
                    if !piece.is_empty() {
                        full.push_str(&piece);
                        let _ = chunks.send(piece);
                    }
                }
            }
            Ok(full)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_parser_splits_events_and_handles_partial_chunks() {
        let mut p = SseParser::default();
        let crlf = p.push("data: {\"a\":1}\r\n\r\ndata: {\"b\":");
        assert_eq!(crlf, vec!["{\"a\":1}".to_string()]);
        assert_eq!(p.push("2}\r\n\r\n"), vec!["{\"b\":2}".to_string()]);
        let mut p = SseParser::default();
        let e1 = p.push("data: {\"a\":1}\n\ndata: {\"b\":");
        assert_eq!(e1, vec!["{\"a\":1}".to_string()]);
        let e2 = p.push("2}\n\n");
        assert_eq!(e2, vec!["{\"b\":2}".to_string()]);
        let e3 = p.push(": comment\nevent: x\ndata: one\ndata: two\n\n");
        assert_eq!(e3, vec!["one\ntwo".to_string()]);
    }

    #[test]
    fn extract_text_joins_parts_and_detects_block() {
        let ev = json!({"candidates":[{"content":{"parts":[{"text":"Hel"},{"text":"lo"}]}}]});
        assert_eq!(extract_text(&ev).unwrap(), "Hello");
        let blocked = json!({"promptFeedback":{"blockReason":"SAFETY"}});
        assert_eq!(extract_text(&blocked), Err(AiError::Blocked("SAFETY".into())));
        let fin = json!({"candidates":[{"finishReason":"STOP","content":{"parts":[]}}]});
        assert_eq!(extract_text(&fin).unwrap(), "");
        let safety = json!({"candidates":[{"finishReason":"SAFETY"}]});
        assert!(matches!(extract_text(&safety), Err(AiError::Blocked(_))));
    }

    #[test]
    fn body_has_image_first_then_text_and_thinking_per_model() {
        let req = RewriteRequest {
            system_prompt: "sys".into(),
            style_name: "Formal".into(),
            style_instruction: "f".into(),
            text: "t".into(),
            screenshot_jpeg_base64: Some("AAAA".into()),
            model: "gemini-2.5-flash".into(),
        };
        let b = Gemini::build_body(&req, true);
        assert_eq!(b["systemInstruction"]["parts"][0]["text"], "sys");
        assert_eq!(b["contents"][0]["parts"][0]["inlineData"]["mimeType"], "image/jpeg");
        assert!(b["contents"][0]["parts"][1]["text"].as_str().unwrap().contains("Formal"));
        assert_eq!(b["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);

        let req3 = RewriteRequest { model: "gemini-3.5-flash-lite".into(), ..req.clone() };
        let b3 = Gemini::build_body(&req3, true);
        assert_eq!(b3["generationConfig"]["thinkingConfig"]["thinkingLevel"], "minimal");
        assert!(Gemini::build_body(&req3, false)["generationConfig"].get("thinkingConfig").is_none());

        let req2 = RewriteRequest { screenshot_jpeg_base64: None, model: "gemini-2.0-flash".into(), ..req };
        let b2 = Gemini::build_body(&req2, true);
        assert_eq!(b2["contents"][0]["parts"].as_array().unwrap().len(), 1);
        assert!(b2["generationConfig"].get("thinkingConfig").is_none());
    }
}

#[cfg(test)]
mod catalog_tests {
    use super::is_text_model;

    /// Ключ отдаёт десятки моделей, и половина из них — не про текст.
    #[test]
    fn catalog_keeps_only_text_gemini() {
        for good in [
            "gemini-3.5-flash-lite",
            "gemini-3.6-flash",
            "gemini-flash-lite-latest",
            "gemini-omni-1.1-flash",
            "gemini-2.5-pro",
            "gemini-pro-latest",
        ] {
            assert!(is_text_model(good), "{good} должна остаться");
        }
        for bad in [
            "nano-banana-pro-preview",
            "gemini-2.5-flash-image",
            "imagen-4.0-generate",
            "lyria-3-pro-preview",
            "veo-3.0-generate",
            "gemma-4-31b-it",
            "deep-research-pro-preview-12-2025",
            "antigravity-preview-05-2026",
            "gemini-2.5-computer-use-preview-10-2025",
            "gemini-3.5-transcribe",
            "gemini-robotics-er-2-preview",
            "gemini-3.1-pro-preview-customtools",
            "gemini-embedding-001",
            "gemini-2.5-flash-live",
        ] {
            assert!(!is_text_model(bad), "{bad} не должна попасть в список");
        }
    }
}

#[cfg(test)]
mod vertex_tests {
    use super::*;

    #[test]
    fn key_kind_table() {
        for none in ["", "   ", "\n", " \t\r\n "] {
            assert_eq!(KeyKind::of(none), None, "{none:?}");
        }
        for vertex in ["AQ.x", "  AQ.x\n", "AQ.", "AQ.Ab12-_cd", "\tAQ.x"] {
            assert_eq!(KeyKind::of(vertex), Some(KeyKind::Vertex), "{vertex:?}");
        }
        for studio in [
            "aq.x", "Aq.x", "aQ.x", "AQ,x", "AQx", "AQ", "A", "AIzaSyDummy", "xAQ.y",
            "AIzaSy-AQ.tail", "bad-key", "AQ .x",
        ] {
            assert_eq!(KeyKind::of(studio), Some(KeyKind::AiStudio), "{studio:?}");
        }
    }

    #[test]
    fn valid_project_boundaries() {
        let of = |n: usize| format!("a{}", "b".repeat(n - 1));
        assert!(!valid_project(&of(5)), "5 символов");
        assert!(valid_project(&of(6)), "6 символов");
        assert!(valid_project(&of(63)), "63 символа");
        assert!(!valid_project(&of(64)), "64 символа");
        assert!(valid_project("my-proj-1"));
        assert!(valid_project("abcde1"));
        assert!(valid_project("  my-proj-1  "), "пробелы по краям обрезаются");
        assert!(valid_project("a----b"), "дефисы внутри допустимы");
        assert!(!valid_project("a-----"), "а в конце — нет");
        assert!(!valid_project(""));
        assert!(!valid_project("      "));
        assert!(!valid_project("1abcdef"), "цифра в начале");
        assert!(!valid_project("-abcdef"), "дефис в начале");
        assert!(!valid_project("abcdef-"), "дефис в конце");
        assert!(valid_project("abcdef1"), "цифра в конце");
        assert!(!valid_project("Abcdef"), "заглавная в начале");
        assert!(!valid_project("abcDef"), "заглавная внутри");
        assert!(!valid_project("abc_def"), "_");
        assert!(!valid_project("abc def"), "пробел внутри");
        assert!(!valid_project("abc.def"), ".");
        assert!(!valid_project("abc/def"), "/");
        assert!(!valid_project("проект-1"), "не ASCII");
        assert!(!valid_project(&format!("a{}", "я".repeat(5))), "6 байт, но не ASCII");
    }

    #[test]
    fn target_new_validates_project_and_defaults_location() {
        let t = VertexTarget::new("  my-proj-1 ", "").unwrap();
        assert_eq!(t, VertexTarget { project: "my-proj-1".into(), location: "global".into() });
        assert_eq!(VertexTarget::new("my-proj-1", "  ").unwrap().location, "global");
        assert_eq!(VertexTarget::new("my-proj-1", " europe-west1 ").unwrap().location, "europe-west1");
        assert_eq!(VertexTarget::new("my-proj-1", "EU WEST").unwrap().location, "global", "мусор в регионе");
        assert_eq!(VertexTarget::new("my-proj-1", "a/b").unwrap().location, "global", "слэш не уходит в адрес");
        assert_eq!(VertexTarget::new("Bad_Project", "global"), None);
        assert_eq!(VertexTarget::new("", "global"), None);
        assert_eq!(VertexTarget::new("abcde", "global"), None);
    }

    #[test]
    fn vertex_model_keeps_listed_and_replaces_the_rest() {
        assert_eq!(VERTEX_MODELS.len(), 6);
        for m in VERTEX_MODELS {
            assert_eq!(vertex_model(m), *m);
        }
        for unknown in ["gemini-3.6-flash", "gemini-2.5-flash", "", "  ", "gemini-3.5-flash-lite-x", "GEMINI-3.5-FLASH"] {
            assert_eq!(vertex_model(unknown), VERTEX_DEFAULT_MODEL, "{unknown:?}");
        }
        assert_eq!(vertex_model("models/gemini-3.5-flash"), "gemini-3.5-flash");
        assert_eq!(vertex_model(" gemini-3.7-flash "), "gemini-3.7-flash");
        assert_eq!(vertex_model(" models/gemini-3.8-flash "), "gemini-3.8-flash");
        assert_eq!(vertex_model("models/gemini-3.6-flash"), VERTEX_DEFAULT_MODEL);
        assert_eq!(VERTEX_DEFAULT_MODEL, "gemini-3.5-flash-lite");
        assert!(VERTEX_MODELS.contains(&VERTEX_DEFAULT_MODEL));
    }

    #[test]
    fn vertex_models_are_six_fast_first() {
        let list = vertex_models();
        let ids: Vec<&str> = list.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "gemini-3.1-flash-lite",
                "gemini-3.5-flash",
                "gemini-3.5-flash-lite",
                "gemini-3.7-flash",
                "gemini-3.8-flash",
                "gemini-4-argon",
            ]
        );
        assert!(list.iter().all(|m| m.display_name == m.id));
        let argon = list.iter().find(|m| m.id == "gemini-4-argon").unwrap();
        assert!(!argon.fast, "gemini-4-argon не flash и не lite");
        assert_eq!(list.iter().filter(|m| m.fast).count(), 5);
    }

    #[test]
    fn vertex_url_per_method() {
        let t = VertexTarget::new("p-123456", "global").unwrap();
        let base = "https://example.test/v1beta1";
        let stream = "https://example.test/v1beta1/projects/p-123456/locations/global/publishers/google/models/gemini-3.5-flash-lite:streamGenerateContent?alt=sse";
        assert_eq!(vertex_url(base, &t, "gemini-3.5-flash-lite", "streamGenerateContent"), stream);
        assert_eq!(
            vertex_url(base, &t, "gemini-3.5-flash-lite", "generateContent"),
            "https://example.test/v1beta1/projects/p-123456/locations/global/publishers/google/models/gemini-3.5-flash-lite:generateContent"
        );
        assert_eq!(
            vertex_url(base, &t, "gemini-3.5-flash-lite", "countTokens"),
            "https://example.test/v1beta1/projects/p-123456/locations/global/publishers/google/models/gemini-3.5-flash-lite:countTokens"
        );
        assert_eq!(vertex_url("https://example.test/v1beta1/", &t, "gemini-3.5-flash-lite", "streamGenerateContent"), stream);
        assert_eq!(vertex_url(base, &t, "models/gemini-3.5-flash-lite", "streamGenerateContent"), stream);
        let eu = VertexTarget::new("p-123456", "europe-west1").unwrap();
        assert!(vertex_url(base, &eu, "m", "countTokens").contains("/locations/europe-west1/"));
        assert!(!stream.contains("key"), "ключа в адресе нет");
    }
}
