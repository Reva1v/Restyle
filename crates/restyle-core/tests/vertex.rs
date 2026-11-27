//! Ключи Vertex AI в `Gemini`: адрес, модель, тело, повтор, ошибки, проверка
//! ключа и каталог. Сеть — мок на loopback (`TcpListener`); окружение не
//! трогаем: адреса задаёт `Gemini::with_bases`, поэтому тесты идут параллельно.

use std::sync::{Arc, Mutex};

use restyle_core::ai::gemini::{Gemini, VertexTarget, VERTEX_MODELS};
use restyle_core::ai::{AiError, Provider, RewriteRequest};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const VERTEX_KEY: &str = "AQ.vertex-test-key-123";
const STUDIO_KEY: &str = "AIzaSy-studio-test-key";
const STREAM_PATH: &str = "/v1beta1/projects/p-123456/locations/global/publishers/google/models/gemini-3.5-flash-lite:streamGenerateContent?alt=sse";
const COUNT_PATH: &str = "/v1beta1/projects/p-123456/locations/global/publishers/google/models/gemini-3.5-flash-lite:countTokens";

// --- мок ---------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Recorded {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Recorded {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }
}

struct Mock {
    addr: String,
    seen: Arc<Mutex<Vec<Recorded>>>,
}

impl Mock {
    /// Адрес как база API: `http://127.0.0.1:port/v1beta1`.
    fn base(&self) -> String {
        format!("http://{}/v1beta1", self.addr)
    }
    fn seen(&self) -> Vec<Recorded> {
        self.seen.lock().unwrap().clone()
    }
}

/// Ответ мока: статус, Content-Type, тело.
type Reply = (u16, &'static str, String);

fn sse(text: &str) -> Reply {
    let event = json!({
        "candidates": [{ "content": { "role": "model", "parts": [{ "text": text }] } }],
        "usageMetadata": { "promptTokenCount": 3, "candidatesTokenCount": 1, "trafficType": "ON_DEMAND" },
    });
    (200, "text/event-stream", format!("data: {event}\r\n\r\n"))
}

fn error(status: u16, error_status: &str, message: &str) -> Reply {
    let body = json!({ "error": { "code": status, "message": message, "status": error_status } });
    (status, "application/json", body.to_string())
}

fn ok_json(body: Value) -> Reply {
    (200, "application/json", body.to_string())
}

/// Мок-сервер: n-й запрос получает n-й ответ, последний повторяется.
async fn mock(replies: Vec<Reply>) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let seen: Arc<Mutex<Vec<Recorded>>> = Arc::default();
    let seen2 = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let seen = seen2.clone();
            let replies = replies.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let head_end = loop {
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                    match sock.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                };
                let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
                let mut lines = head.split("\r\n");
                let mut first = lines.next().unwrap_or("").split(' ');
                let method = first.next().unwrap_or("").to_string();
                let path = first.next().unwrap_or("").to_string();
                let headers: Vec<(String, String)> = lines
                    .filter_map(|l| l.split_once(':'))
                    .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                    .collect();
                let len: usize = headers
                    .iter()
                    .find(|(k, _)| k == "content-length")
                    .and_then(|(_, v)| v.parse().ok())
                    .unwrap_or(0);
                while buf.len() < head_end + len {
                    match sock.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let body = String::from_utf8_lossy(&buf[head_end..]).into_owned();
                let index = {
                    let mut g = seen.lock().unwrap();
                    g.push(Recorded { method, path, headers, body });
                    g.len() - 1
                };
                let (status, ctype, payload) = replies[index.min(replies.len() - 1)].clone();
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    Mock { addr, seen }
}

/// Адрес, на котором никто не слушает: любой запрос — отказ в соединении.
async fn trap_base() -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    format!("http://{addr}/v1beta1")
}

fn target() -> VertexTarget {
    VertexTarget::new("p-123456", "global").unwrap()
}

fn request(model: &str) -> RewriteRequest {
    RewriteRequest {
        system_prompt: "sys".into(),
        style_name: "Formal".into(),
        style_instruction: "be formal".into(),
        text: "hello".into(),
        screenshot_jpeg_base64: None,
        model: model.into(),
    }
}

async fn rewrite(g: &Gemini, key: &str, model: &str) -> (Result<String, AiError>, Vec<String>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let req = request(model);
    let out = g.rewrite(key, &req, tx).await;
    let mut chunks = Vec::new();
    while let Ok(c) = rx.try_recv() {
        chunks.push(c);
    }
    (out, chunks)
}

// --- rewrite ----------------------------------------------------------------

#[tokio::test]
async fn vertex_rewrite_uses_project_path_and_substitutes_the_model() {
    let studio = mock(vec![sse("studio")]).await;
    let vertex = mock(vec![sse("Hi")]).await;
    let g = Gemini::with_bases(&studio.base(), &vertex.base()).with_vertex(Some(target()));

    // gemini-3.6-flash на Vertex нет → gemini-3.5-flash-lite.
    let (out, chunks) = rewrite(&g, VERTEX_KEY, "gemini-3.6-flash").await;
    assert_eq!(out, Ok("Hi".to_string()), "ответ с usageMetadata.trafficType разбирается");
    assert_eq!(chunks, vec!["Hi".to_string()]);

    let seen = vertex.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].path, STREAM_PATH);
    assert_eq!(seen[0].header("x-goog-api-key"), Some(VERTEX_KEY));
    assert!(!seen[0].path.contains(VERTEX_KEY), "ключа нет в адресе");
    assert!(!seen[0].path.contains("gemini-3.6-flash"));
    assert_eq!(seen[0].json()["contents"][0]["role"], "user");
    assert!(studio.seen().is_empty(), "AI Studio не трогали");
}

#[tokio::test]
async fn vertex_rewrite_keeps_a_listed_model_and_strips_the_models_prefix() {
    for (given, used) in [
        ("gemini-3.7-flash", "gemini-3.7-flash"),
        ("models/gemini-3.5-flash", "gemini-3.5-flash"),
        (" gemini-4-argon ", "gemini-4-argon"),
    ] {
        let vertex = mock(vec![sse("ok")]).await;
        let g = Gemini::with_bases(&trap_base().await, &vertex.base()).with_vertex(Some(target()));
        let (out, _) = rewrite(&g, VERTEX_KEY, given).await;
        assert_eq!(out, Ok("ok".to_string()), "{given}");
        let want = format!(
            "/v1beta1/projects/p-123456/locations/global/publishers/google/models/{used}:streamGenerateContent?alt=sse"
        );
        assert_eq!(vertex.seen()[0].path, want, "{given}");
    }
}

#[tokio::test]
async fn vertex_rewrite_builds_the_body_for_the_substituted_model() {
    let vertex = mock(vec![sse("ok")]).await;
    let g = Gemini::with_bases(&trap_base().await, &vertex.base()).with_vertex(Some(target()));
    // 2.5 → подмена на 3.5-flash-lite: thinkingLevel (3.x), а не thinkingBudget (2.5).
    let (out, _) = rewrite(&g, VERTEX_KEY, "gemini-2.5-flash").await;
    assert!(out.is_ok());
    let cfg = &vertex.seen()[0].json()["generationConfig"];
    assert_eq!(cfg["thinkingConfig"]["thinkingLevel"], "minimal");
    assert!(cfg["thinkingConfig"].get("thinkingBudget").is_none());

    // gemini-4-argon не из семейства gemini-3: размышления не настраиваем.
    let vertex = mock(vec![sse("ok")]).await;
    let g = Gemini::with_bases(&trap_base().await, &vertex.base()).with_vertex(Some(target()));
    let (out, _) = rewrite(&g, VERTEX_KEY, "gemini-4-argon").await;
    assert!(out.is_ok());
    assert!(vertex.seen()[0].json()["generationConfig"].get("thinkingConfig").is_none());
}

#[tokio::test]
async fn vertex_rewrite_retries_without_thinking_config_on_400() {
    let vertex = mock(vec![
        error(400, "INVALID_ARGUMENT", "Unsupported thinkingLevel for this model"),
        sse("done"),
    ])
    .await;
    let g = Gemini::with_bases(&trap_base().await, &vertex.base()).with_vertex(Some(target()));
    let (out, _) = rewrite(&g, VERTEX_KEY, "gemini-3.8-flash").await;
    assert_eq!(out, Ok("done".to_string()));
    let seen = vertex.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].json()["generationConfig"]["thinkingConfig"]["thinkingLevel"], "minimal");
    assert!(seen[1].json()["generationConfig"].get("thinkingConfig").is_none());
    assert_eq!(seen[0].path, seen[1].path);
}

#[tokio::test]
async fn vertex_400_without_thinking_in_the_body_is_not_retried() {
    let vertex = mock(vec![error(400, "INVALID_ARGUMENT", "Bad request")]).await;
    let g = Gemini::with_bases(&trap_base().await, &vertex.base()).with_vertex(Some(target()));
    let (out, _) = rewrite(&g, VERTEX_KEY, "gemini-3.5-flash-lite").await;
    assert!(matches!(out, Err(AiError::Other(_))), "{out:?}");
    assert_eq!(vertex.seen().len(), 1);
}

#[tokio::test]
async fn vertex_rewrite_maps_statuses() {
    for (reply, want) in [
        (error(403, "PERMISSION_DENIED", "Permission denied"), AiError::NoRole),
        (error(401, "UNAUTHENTICATED", "API key not valid. Please pass a valid API key."), AiError::Auth),
        (error(401, "UNAUTHENTICATED", "API keys are not supported by this API."), AiError::Auth),
        (error(429, "RESOURCE_EXHAUSTED", "Quota exceeded"), AiError::RateLimit),
    ] {
        let vertex = mock(vec![reply.clone()]).await;
        let g = Gemini::with_bases(&trap_base().await, &vertex.base()).with_vertex(Some(target()));
        let (out, _) = rewrite(&g, VERTEX_KEY, "gemini-3.5-flash-lite").await;
        assert_eq!(out, Err(want), "{reply:?}");
    }
    let vertex = mock(vec![error(404, "NOT_FOUND", "Publisher model was not found")]).await;
    let g = Gemini::with_bases(&trap_base().await, &vertex.base()).with_vertex(Some(target()));
    let (out, _) = rewrite(&g, VERTEX_KEY, "gemini-3.5-flash-lite").await;
    assert!(matches!(out, Err(AiError::Other(_))), "{out:?}");
    let vertex = mock(vec![error(503, "UNAVAILABLE", "down")]).await;
    let g = Gemini::with_bases(&trap_base().await, &vertex.base()).with_vertex(Some(target()));
    let (out, _) = rewrite(&g, VERTEX_KEY, "gemini-3.5-flash-lite").await;
    assert!(matches!(out, Err(AiError::Server(503, _))), "{out:?}");
}

#[tokio::test]
async fn vertex_key_without_target_fails_before_any_request() {
    let studio = mock(vec![sse("x")]).await;
    let vertex = mock(vec![sse("x")]).await;
    let g = Gemini::with_bases(&studio.base(), &vertex.base());
    let (out, chunks) = rewrite(&g, VERTEX_KEY, "gemini-3.5-flash-lite").await;
    assert_eq!(out, Err(AiError::NoProject));
    assert!(chunks.is_empty());
    let g = g.with_vertex(None);
    assert_eq!(rewrite(&g, "  AQ.x  ", "m").await.0, Err(AiError::NoProject));
    assert!(vertex.seen().is_empty(), "0 запросов на Vertex");
    assert!(studio.seen().is_empty(), "и 0 запросов на AI Studio");
}

#[tokio::test]
async fn vertex_key_with_spaces_is_recognised_and_sent_trimmed() {
    let vertex = mock(vec![sse("ok")]).await;
    let g = Gemini::with_bases(&trap_base().await, &vertex.base()).with_vertex(Some(target()));
    let (out, _) = rewrite(&g, "  AQ.padded-key\n", "gemini-3.5-flash-lite").await;
    assert_eq!(out, Ok("ok".to_string()));
    assert_eq!(vertex.seen()[0].header("x-goog-api-key"), Some("AQ.padded-key"));
}

#[tokio::test]
async fn studio_key_with_a_target_keeps_the_old_path() {
    let studio = mock(vec![sse("Hi")]).await;
    let vertex = mock(vec![sse("x")]).await;
    let g = Gemini::with_bases(&studio.base(), &vertex.base()).with_vertex(Some(target()));
    // Модель не подменяется: gemini-3.6-flash с ключом AI Studio остаётся собой.
    let (out, _) = rewrite(&g, STUDIO_KEY, "gemini-3.6-flash").await;
    assert_eq!(out, Ok("Hi".to_string()));
    let seen = studio.seen();
    assert_eq!(seen[0].path, "/v1beta1/models/gemini-3.6-flash:streamGenerateContent?alt=sse");
    assert_eq!(seen[0].header("x-goog-api-key"), Some(STUDIO_KEY));
    assert!(vertex.seen().is_empty());

    // 403 для AI Studio — по-прежнему «Проверь ключ».
    let studio = mock(vec![error(403, "PERMISSION_DENIED", "denied")]).await;
    let g = Gemini::with_bases(&studio.base(), &trap_base().await).with_vertex(Some(target()));
    assert_eq!(rewrite(&g, STUDIO_KEY, "gemini-3.5-flash-lite").await.0, Err(AiError::Auth));
}

// --- check_key ---------------------------------------------------------------

#[tokio::test]
async fn check_key_vertex_posts_count_tokens() {
    let vertex = mock(vec![ok_json(json!({ "totalTokens": 1 }))]).await;
    let g = Gemini::with_bases(&trap_base().await, &vertex.base()).with_vertex(Some(target()));
    assert_eq!(g.check_key(VERTEX_KEY).await, Ok(()));
    let seen = vertex.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].path, COUNT_PATH);
    assert!(!seen[0].path.contains('?'), "у countTokens нет query");
    assert_eq!(seen[0].header("x-goog-api-key"), Some(VERTEX_KEY));
    assert_eq!(
        seen[0].json(),
        json!({ "contents": [{ "role": "user", "parts": [{ "text": "ping" }] }] })
    );
}

#[tokio::test]
async fn check_key_vertex_maps_statuses() {
    for (reply, want) in [
        (error(401, "UNAUTHENTICATED", "API key not valid"), AiError::Auth),
        (error(403, "PERMISSION_DENIED", "denied"), AiError::NoRole),
        (error(429, "RESOURCE_EXHAUSTED", "quota"), AiError::RateLimit),
    ] {
        let vertex = mock(vec![reply.clone()]).await;
        let g = Gemini::with_bases(&trap_base().await, &vertex.base()).with_vertex(Some(target()));
        assert_eq!(g.check_key(VERTEX_KEY).await, Err(want), "{reply:?}");
    }
    let vertex = mock(vec![error(404, "NOT_FOUND", "Publisher model was not found")]).await;
    let g = Gemini::with_bases(&trap_base().await, &vertex.base()).with_vertex(Some(target()));
    assert!(matches!(g.check_key(VERTEX_KEY).await, Err(AiError::Other(_))));
}

#[tokio::test]
async fn check_key_vertex_without_target_makes_no_request() {
    let vertex = mock(vec![ok_json(json!({}))]).await;
    let g = Gemini::with_bases(&trap_base().await, &vertex.base());
    assert_eq!(g.check_key(VERTEX_KEY).await, Err(AiError::NoProject));
    assert!(vertex.seen().is_empty());
}

#[tokio::test]
async fn check_key_with_an_empty_key_makes_no_request() {
    let studio = mock(vec![ok_json(json!({ "models": [] }))]).await;
    let vertex = mock(vec![ok_json(json!({}))]).await;
    let g = Gemini::with_bases(&studio.base(), &vertex.base()).with_vertex(Some(target()));
    for empty in ["", "   ", "\n"] {
        assert_eq!(g.check_key(empty).await, Err(AiError::NoApiKey), "{empty:?}");
    }
    assert!(studio.seen().is_empty() && vertex.seen().is_empty());
}

#[tokio::test]
async fn check_key_studio_lists_models_as_before() {
    let studio = mock(vec![ok_json(json!({ "models": [] }))]).await;
    let vertex = mock(vec![ok_json(json!({}))]).await;
    let g = Gemini::with_bases(&studio.base(), &vertex.base()).with_vertex(Some(target()));
    assert_eq!(g.check_key(STUDIO_KEY).await, Ok(()));
    let seen = studio.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "GET");
    assert!(seen[0].path.starts_with("/v1beta1/models"));
    assert!(vertex.seen().is_empty());

    let studio = mock(vec![error(400, "INVALID_ARGUMENT", "API key not valid")]).await;
    let g = Gemini::with_bases(&studio.base(), &trap_base().await);
    assert_eq!(g.check_key(STUDIO_KEY).await, Err(AiError::Auth));
}

// --- list_models -------------------------------------------------------------

#[tokio::test]
async fn list_models_for_a_vertex_key_is_the_fixed_list_without_network() {
    let g = Gemini::with_bases(&trap_base().await, &trap_base().await);
    let list = g.list_models("AQ.x").await.unwrap();
    assert_eq!(list.len(), 6);
    let mut ids: Vec<&str> = list.iter().map(|m| m.id.as_str()).collect();
    ids.sort_unstable();
    let mut want: Vec<&str> = VERTEX_MODELS.to_vec();
    want.sort_unstable();
    assert_eq!(ids, want);
    // Даже с проектом сети нет.
    let g = g.with_vertex(Some(target()));
    assert_eq!(g.list_models(" AQ.x ").await.unwrap().len(), 6);
}

#[tokio::test]
async fn list_models_for_a_studio_key_still_goes_to_the_network() {
    let body = json!({ "models": [
        { "name": "models/gemini-3.5-flash-lite", "displayName": "Lite", "supportedGenerationMethods": ["generateContent"] },
        { "name": "models/gemini-embedding-001", "supportedGenerationMethods": ["embedContent"] },
    ]});
    let studio = mock(vec![ok_json(body)]).await;
    let g = Gemini::with_bases(&studio.base(), &trap_base().await);
    let list = g.list_models(STUDIO_KEY).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "gemini-3.5-flash-lite");
    assert_eq!(studio.seen().len(), 1);
    assert_eq!(studio.seen()[0].header("x-goog-api-key"), Some(STUDIO_KEY));

    // Ловушка вместо AI Studio: ключ AI Studio обязан пойти в сеть и упасть.
    let g = Gemini::with_bases(&trap_base().await, &trap_base().await);
    assert_eq!(g.list_models(STUDIO_KEY).await.unwrap_err(), AiError::Network);
}
