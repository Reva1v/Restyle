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

pub struct Gemini {
    client: reqwest::Client,
    base_url: String,
}

impl Gemini {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .expect("reqwest client");
        let base_url = std::env::var("RESTYLE_GEMINI_BASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Self { client, base_url: base_url.trim_end_matches('/').to_string() }
    }

    /// Каталог моделей ключа: `GET /models`. Ключи видят разный набор (2.5
    /// новым ключам недоступна), поэтому список тянем у API, а не хардкодим.
    pub async fn list_models(&self, key: &str) -> Result<Vec<ModelInfo>, AiError> {
        let url = format!("{}/models?pageSize=200&key={key}", self.base_url);
        let resp = self.client.get(url).send().await.map_err(|e| AiError::from_reqwest(&e))?;
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
        loop {
            let Some(idx) = self.buf.find("\n\n") else { break };
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
            let url = format!("{}/models/{}:streamGenerateContent?alt=sse", self.base_url, req.model);
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
                return Err(AiError::from_status(status, &body));
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
