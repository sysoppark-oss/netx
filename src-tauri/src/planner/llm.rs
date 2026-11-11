//! LLM 호출. JSON 스키마를 함께 보내 정해진 형식으로만 답하게 한다(구조화 출력).
//!
//! - Ollama: `POST {주소}/api/chat`, `format`에 스키마
//! - OpenAI 호환: `POST {주소}/chat/completions`, `response_format`에 스키마

use std::time::Duration;

use serde_json::{Value, json};

use crate::settings::{Provider, Settings};

const TIMEOUT: Duration = Duration::from_secs(120);
const MAX_RESPONSE_BYTES: usize = 200_000;

/// 시스템 지시와 사용자 내용을 보내고, 스키마에 맞는 JSON을 돌려받는다.
pub async fn call_json(settings: &Settings, api_key: Option<&str>, system: &str, user: &str, schema: &Value) -> Result<Value, String> {
    let base = settings.base_url.trim().trim_end_matches('/');
    let messages = json!([{ "role": "system", "content": system }, { "role": "user", "content": user }]);

    let (url, body) = match settings.provider {
        Provider::None => return Err("AI가 설정되어 있지 않습니다".into()),
        Provider::Ollama => (
            format!("{base}/api/chat"),
            json!({ "model": settings.model, "messages": messages, "stream": false, "format": schema, "options": { "temperature": 0 } }),
        ),
        Provider::Openai => (
            format!("{base}/chat/completions"),
            json!({
                "model": settings.model,
                "messages": messages,
                "temperature": 0,
                "response_format": { "type": "json_schema", "json_schema": { "name": "netx", "strict": true, "schema": schema } }
            }),
        ),
    };

    let client = reqwest::Client::builder().timeout(TIMEOUT).build().map_err(|e| format!("연결 준비에 실패했습니다: {e}"))?;
    let mut req = client.post(&url).json(&body);
    if let Some(key) = api_key.filter(|_| settings.provider == Provider::Openai) {
        req = req.bearer_auth(key);
    }

    let resp = req.send().await.map_err(|e| {
        if e.is_timeout() {
            "AI 응답이 시간 안에 오지 않았습니다".to_string()
        } else if e.is_connect() {
            "AI 서버에 연결하지 못했습니다. 주소와 네트워크를 확인하세요".to_string()
        } else {
            format!("AI 요청에 실패했습니다: {}", e.without_url())
        }
    })?;
    let status = resp.status();
    let bytes = resp.bytes().await.map_err(|e| format!("AI 응답을 읽지 못했습니다: {}", e.without_url()))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err("AI 응답이 너무 깁니다".into());
    }
    if !status.is_success() {
        let detail: String = String::from_utf8_lossy(&bytes).chars().filter(|c| !c.is_control()).take(300).collect();
        return Err(format!("AI 서버가 오류를 돌려주었습니다 ({}): {detail}", status.as_u16()));
    }

    let outer: Value = serde_json::from_slice(&bytes).map_err(|_| "AI 응답이 JSON 형식이 아닙니다".to_string())?;
    let content = match settings.provider {
        Provider::Ollama => outer.pointer("/message/content"),
        _ => outer.pointer("/choices/0/message/content"),
    }
    .and_then(Value::as_str)
    .ok_or("AI 응답에 내용이 없습니다")?;

    parse_content(content)
}

/// 모델이 돌려준 본문에서 JSON 객체를 꺼낸다. 코드 블록 표시로 감싼 경우도 받아 준다.
fn parse_content(content: &str) -> Result<Value, String> {
    let trimmed = content.trim();
    let inner = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .unwrap_or(trimmed)
        .trim();
    let value: Value = serde_json::from_str(inner).map_err(|_| "AI가 정해진 형식으로 답하지 않았습니다".to_string())?;
    if value.is_object() { Ok(value) } else { Err("AI가 정해진 형식으로 답하지 않았습니다".into()) }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// 요청 하나를 받아 정해진 응답을 돌려주는 가짜 서버. 받은 요청(헤더와 본문)을 돌려준다.
    pub(crate) async fn fake_server(status: u16, body: String) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut data = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = sock.read(&mut buf).await.unwrap();
                data.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&data);
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let len = text[..head_end]
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if data.len() >= head_end + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let resp = format!("HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            sock.write_all(resp.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&data).into_owned()
        });
        (format!("http://{addr}"), handle)
    }

    fn settings(provider: Provider, base: &str) -> Settings {
        Settings { provider, base_url: base.into(), model: "test-model".into(), redact: false, manual_update: false }
    }

    #[tokio::test]
    async fn ollama_request_and_response() {
        let reply = json!({ "message": { "role": "assistant", "content": "{\"ok\":true}" } }).to_string();
        let (base, server) = fake_server(200, reply).await;
        let schema = json!({ "type": "object" });
        let out = call_json(&settings(Provider::Ollama, &base), Some("secret"), "sys", "user", &schema).await.unwrap();
        assert_eq!(out, json!({ "ok": true }));
        let request = server.await.unwrap();
        assert!(request.starts_with("POST /api/chat "));
        assert!(request.contains("\"format\":{\"type\":\"object\"}"));
        assert!(request.contains("\"stream\":false"));
        assert!(!request.to_lowercase().contains("authorization"), "Ollama에는 키를 보내지 않는다");
    }

    #[tokio::test]
    async fn openai_request_and_response() {
        let reply = json!({ "choices": [{ "message": { "content": "```json\n{\"a\":1}\n```" } }] }).to_string();
        let (base, server) = fake_server(200, reply).await;
        let out = call_json(&settings(Provider::Openai, &format!("{base}/v1/")), Some("secret"), "sys", "user", &json!({})).await.unwrap();
        assert_eq!(out, json!({ "a": 1 }));
        let request = server.await.unwrap();
        assert!(request.starts_with("POST /v1/chat/completions "));
        assert!(request.to_lowercase().contains("authorization: bearer secret"));
        assert!(request.contains("\"json_schema\""));
    }

    #[tokio::test]
    async fn errors_are_reported_in_plain_words() {
        let (base, _s) = fake_server(401, "{\"error\":\"bad key\"}".into()).await;
        let err = call_json(&settings(Provider::Openai, &base), None, "s", "u", &json!({})).await.unwrap_err();
        assert!(err.contains("401"), "{err}");

        let (base, _s) = fake_server(200, json!({ "message": { "content": "not json" } }).to_string()).await;
        let err = call_json(&settings(Provider::Ollama, &base), None, "s", "u", &json!({})).await.unwrap_err();
        assert!(err.contains("형식"), "{err}");

        let err = call_json(&settings(Provider::Ollama, "http://127.0.0.1:9"), None, "s", "u", &json!({})).await.unwrap_err();
        assert!(err.contains("연결"), "{err}");

        assert!(call_json(&settings(Provider::None, ""), None, "s", "u", &json!({})).await.is_err());
    }
}
