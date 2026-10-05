//! Ollama implementation of the [`ModelCatalog`] port: `GET /api/tags`
//! against the configured endpoint (default `http://localhost:11434`).
//! Chat completions live in [`super::ollama_chat`]. The HTTP shell is
//! deliberately thin; the JSON translations are pure functions so they
//! are unit-testable without a network.

use std::time::Duration;

use serde::Deserialize;
use tracing::{debug, warn};

use crate::domain::config_report::{DEFAULT_LLM_ENDPOINT, DEFAULT_LLM_TIMEOUT_SECONDS};
use crate::ports::{LlmError, ModelCatalog, ModelInfo};

pub const DEFAULT_ENDPOINT: &str = DEFAULT_LLM_ENDPOINT;

/// Local models chew on large prompts (an implementation attempt
/// carries the whole project) for minutes, not seconds.
pub const DEFAULT_GENERATION_TIMEOUT: Duration = Duration::from_secs(DEFAULT_LLM_TIMEOUT_SECONDS);

/// How long Ollama keeps the model resident after a request. A loaded
/// model skips the multi-second startup cost on the next call.
///
/// Temperature is deliberately left at the model's default: the
/// implement loop escapes a failing answer by retrying with sampling
/// variety, and greedy decoding (temperature 0) was observed to
/// regenerate the same broken step definition 24 attempts in a row.
pub const KEEP_ALIVE: &str = "30m";

/// The full story of a request failure. reqwest's Display keeps the
/// cause (connection refused, timed out, ...) in the source chain,
/// which would otherwise be dropped - a timeout then reads like the
/// provider is down.
pub(crate) fn describe(error: &reqwest::Error, timeout: Duration) -> String {
    if error.is_timeout() {
        return format!(
            "no reply within {}s - large prompts can outlast the timeout while \
             the model is still generating; set timeout_seconds under [llm] in \
             .spec/config.toml to wait longer",
            timeout.as_secs()
        );
    }
    let mut messages = vec![error.to_string()];
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        messages.push(cause.to_string());
        source = cause.source();
    }
    messages.dedup();
    messages.join(" - ")
}

pub(crate) fn http_client(timeout: Duration) -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .expect("a timeout-only HTTP client configuration cannot fail to build")
}

pub struct OllamaCatalog {
    endpoint: String,
    timeout: Duration,
    client: reqwest::blocking::Client,
}

impl OllamaCatalog {
    pub fn new(endpoint: String) -> Self {
        Self::with_timeout(endpoint, Duration::from_secs(5))
    }

    pub fn with_timeout(endpoint: String, timeout: Duration) -> Self {
        Self {
            client: http_client(timeout),
            endpoint,
            timeout,
        }
    }
}

impl ModelCatalog for OllamaCatalog {
    fn capabilities(&self, model: &str) -> Option<Vec<String>> {
        let url = format!("{}/api/show", self.endpoint.trim_end_matches('/'));
        let body = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "model": model }))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(|response| response.text())
            .map_err(|e| {
                // Not a failure worth surfacing: every caller treats an
                // unreadable capability list as "unknown" and keeps the
                // behaviour it would have had anyway.
                debug!(model, error = %describe(&e, self.timeout), "capabilities unavailable");
            })
            .ok()?;
        let capabilities = parse_capabilities(&body)?;
        debug!(model, ?capabilities, "Ollama model capabilities");
        Some(capabilities)
    }

    fn models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        let url = format!("{}/api/tags", self.endpoint.trim_end_matches('/'));
        debug!(endpoint = %self.endpoint, "listing Ollama models");
        let body = self
            .client
            .get(&url)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(|response| response.text())
            .map_err(|e| {
                warn!(endpoint = %self.endpoint, error = %describe(&e, self.timeout),
                    "Ollama model discovery failed");
                LlmError(format!(
                    "Ollama at {} - {}",
                    self.endpoint,
                    describe(&e, self.timeout)
                ))
            })?;
        let models = parse_tags(&body)?;
        debug!(count = models.len(), "Ollama models discovered");
        Ok(models)
    }
}

#[derive(Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<Tag>,
}

#[derive(Deserialize)]
struct Tag {
    name: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    modified_at: Option<String>,
}

#[derive(Deserialize)]
struct ShowResponse {
    #[serde(default)]
    capabilities: Option<Vec<String>>,
}

/// The `capabilities` list from `/api/show`, when the provider reports
/// one.
///
/// `None` rather than an empty list when the key is absent: "this
/// provider does not tell me" and "this model can do nothing" lead to
/// opposite decisions about whether to leave a model in the generative
/// candidate set.
pub fn parse_capabilities(body: &str) -> Option<Vec<String>> {
    serde_json::from_str::<ShowResponse>(body)
        .ok()?
        .capabilities
}

/// Translates Ollama's `/api/tags` JSON into the port's model list.
pub fn parse_tags(body: &str) -> Result<Vec<ModelInfo>, LlmError> {
    let response: TagsResponse = serde_json::from_str(body)
        .map_err(|e| LlmError(format!("unexpected /api/tags response - {e}")))?;
    Ok(response
        .models
        .into_iter()
        .map(|tag| ModelInfo {
            name: tag.name,
            size_bytes: tag.size,
            modified_at: tag.modified_at,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_json_translates_to_model_infos() {
        let body = r#"{"models":[
            {"name":"llama3:latest","size":4661224676,"modified_at":"2026-08-01T10:00:00Z"},
            {"name":"qwen3:8b"}
        ]}"#;
        let models = parse_tags(body).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "llama3:latest");
        assert_eq!(models[0].size_bytes, Some(4661224676));
        assert_eq!(models[1].name, "qwen3:8b");
        assert_eq!(models[1].size_bytes, None);
    }

    #[test]
    fn an_empty_models_array_is_an_empty_list_not_an_error() {
        assert!(parse_tags(r#"{"models":[]}"#).unwrap().is_empty());
        assert!(parse_tags("{}").unwrap().is_empty());
    }

    #[test]
    fn malformed_json_is_a_structured_error() {
        let error = parse_tags("nope").unwrap_err();
        assert!(error.0.starts_with("unexpected /api/tags response -"));
    }

    #[test]
    fn a_reachable_endpoint_returns_the_parsed_models() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let body = r#"{"models":[{"name":"llama3:latest"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        let catalog = OllamaCatalog::new(format!("http://127.0.0.1:{port}/"));
        let models = catalog.models().unwrap();
        server.join().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "llama3:latest");
    }

    #[test]
    fn an_unreachable_endpoint_reports_the_endpoint_in_the_error() {
        // Port 9 (discard) is reliably closed/unroutable for a fast failure.
        let catalog =
            OllamaCatalog::with_timeout("http://127.0.0.1:9".into(), Duration::from_millis(300));
        let error = catalog.models().unwrap_err();
        assert!(error.0.contains("http://127.0.0.1:9"), "got: {}", error.0);
        assert!(
            error.0.matches(" - ").count() >= 2 || error.0.contains("refused"),
            "got: {}",
            error.0
        );
    }

    #[test]
    fn a_slow_reply_is_named_a_timeout_with_the_configured_budget() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            std::thread::sleep(Duration::from_millis(600));
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
        });
        let catalog = OllamaCatalog::with_timeout(
            format!("http://127.0.0.1:{port}"),
            Duration::from_millis(200),
        );
        let error = catalog.models().unwrap_err();
        server.join().unwrap();
        assert!(error.0.contains("no reply within 0s"), "got: {}", error.0);
        assert!(error.0.contains("timeout_seconds"), "got: {}", error.0);
    }

    #[test]
    fn the_default_generation_timeout_allows_long_completions() {
        assert_eq!(DEFAULT_GENERATION_TIMEOUT, Duration::from_secs(300));
    }

    #[test]
    fn a_shown_capability_list_is_read_off_the_reply() {
        assert_eq!(
            parse_capabilities(r#"{"capabilities":["decision"]}"#),
            Some(vec!["decision".to_string()])
        );
        assert_eq!(
            parse_capabilities(r#"{"capabilities":["completion","tools"]}"#),
            Some(vec!["completion".to_string(), "tools".to_string()])
        );
    }

    /// "The provider did not say" and "this model can do nothing" have
    /// to stay distinguishable: only the second would justify dropping a
    /// model from the generative candidates.
    #[test]
    fn a_reply_without_capabilities_is_unknown_rather_than_empty() {
        assert_eq!(parse_capabilities(r#"{"details":{}}"#), None);
        assert_eq!(parse_capabilities("not json"), None);
        assert_eq!(
            parse_capabilities(r#"{"capabilities":[]}"#),
            Some(Vec::new())
        );
    }

    #[test]
    fn capabilities_come_back_from_a_reachable_provider() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let read = stream.read(&mut request).unwrap_or(0);
            let body = r#"{"capabilities":["decision"]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&request[..read]).to_string()
        });
        let catalog = OllamaCatalog::new(format!("http://127.0.0.1:{port}"));
        let capabilities = catalog.capabilities("nimble:latest");
        let sent = server.join().unwrap();
        assert_eq!(capabilities, Some(vec!["decision".to_string()]));
        assert!(sent.starts_with("POST /api/show "), "got: {sent}");
        assert!(sent.contains("nimble:latest"));
    }

    #[test]
    fn an_unreachable_provider_reports_unknown_capabilities_rather_than_failing() {
        let catalog =
            OllamaCatalog::with_timeout("http://127.0.0.1:9".into(), Duration::from_millis(200));
        assert_eq!(catalog.capabilities("anything"), None);
    }
}
