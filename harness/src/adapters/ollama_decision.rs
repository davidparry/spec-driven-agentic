//! Ollama implementation of the [`DecisionModel`] port: `POST
//! /v1/systemone` against the configured endpoint. Added in Ollama 0.35
//! and shaped by TypeSafe's Jev API — one request carries a brief and a
//! set of named questions, and comes back with a typed answer for each.
//!
//! The HTTP shell is thin on purpose; the JSON translation and the
//! status-code mapping are pure functions so every failure path is unit
//! testable without a network.

use std::time::Duration;

use serde::Serialize;
use tracing::{debug, warn};

use super::ollama::{describe, http_client};
use crate::domain::decision::{KEEP_ALIVE, MAX_REQUEST_BYTES, Outcome, Request, SYSTEMONE_PATH};
use crate::ports::{DecisionError, DecisionModel};

/// Decisions are a single forward pass with no reasoning step, so they
/// return in well under a second once the model is loaded. The budget
/// is for the load, which every call pays (the model is released as
/// soon as it answers - see [`KEEP_ALIVE`]) and which takes seconds
/// from disk; it is still far below the generative timeout — a decision
/// that takes minutes is a broken setup, not a long answer.
pub const DEFAULT_DECISION_TIMEOUT_SECONDS: u64 = 60;
pub const DEFAULT_DECISION_TIMEOUT: Duration =
    Duration::from_secs(DEFAULT_DECISION_TIMEOUT_SECONDS);

/// The wire body. `state` and `questions` come straight from the domain
/// request; `model` and `keep_alive` are the adapter's business.
#[derive(Serialize)]
struct Body<'a> {
    model: &'a str,
    state: &'a serde_json::Value,
    questions: &'a std::collections::BTreeMap<String, crate::domain::decision::Question>,
    keep_alive: &'static str,
}

/// `Clone` shares the connection pool rather than copying it, so a
/// copy per caller costs nothing.
#[derive(Clone)]
pub struct OllamaDecision {
    endpoint: String,
    timeout: Duration,
    client: reqwest::blocking::Client,
}

impl OllamaDecision {
    pub fn new(endpoint: String) -> Self {
        Self::with_timeout(endpoint, DEFAULT_DECISION_TIMEOUT)
    }

    pub fn with_timeout(endpoint: String, timeout: Duration) -> Self {
        Self {
            client: http_client(timeout),
            endpoint,
            timeout,
        }
    }

    fn url(&self) -> String {
        format!("{}{SYSTEMONE_PATH}", self.endpoint.trim_end_matches('/'))
    }
}

impl DecisionModel for OllamaDecision {
    fn decide(&self, model: &str, request: &Request) -> Result<Outcome, DecisionError> {
        if model.trim().is_empty() {
            return Err(DecisionError::Invalid("model is required".into()));
        }
        // Local faults first: the server's 400 names the question but
        // not the project's wording, and a brief the caller built wrong
        // should not cost a round trip to find out about.
        if let Some(fault) = request.fault() {
            return Err(DecisionError::Invalid(fault));
        }
        let body = Body {
            model,
            state: &request.state,
            questions: &request.questions,
            keep_alive: KEEP_ALIVE,
        };
        let payload = serde_json::to_vec(&body).map_err(|e| {
            DecisionError::Invalid(format!("the request is not serializable - {e}"))
        })?;
        if payload.len() > MAX_REQUEST_BYTES {
            return Err(DecisionError::TooLarge(format!(
                "{} bytes exceeds the {MAX_REQUEST_BYTES} byte limit - summarize the brief \
                 further; the server does not truncate",
                payload.len()
            )));
        }
        debug!(
            model,
            endpoint = %self.endpoint,
            questions = request.questions.len(),
            bytes = payload.len(),
            "asking the decision model"
        );
        let response = self
            .client
            .post(self.url())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(payload)
            .send()
            .map_err(|e| transport_error(&e, &self.endpoint, self.timeout))?;
        let status = response.status();
        let text = response.text().map_err(|e| {
            DecisionError::Malformed(format!(
                "the reply body was unreadable - {}",
                describe(&e, self.timeout)
            ))
        })?;
        if !status.is_success() {
            let error = status_error(status.as_u16(), &text, model, &self.endpoint);
            warn!(model, status = status.as_u16(), error = %error, "decision request refused");
            return Err(error);
        }
        let outcome = parse_outcome(&text)?;
        check_answers(request, &outcome)?;
        debug!(
            model = %outcome.model,
            answers = outcome.answers.len(),
            input_tokens = outcome.usage.input_tokens,
            "decision answered"
        );
        Ok(outcome)
    }
}

/// A request that never reached a reply. A timeout is told apart from an
/// unreachable provider because the fixes are different keys in
/// different sections of the config file.
fn transport_error(error: &reqwest::Error, endpoint: &str, timeout: Duration) -> DecisionError {
    if error.is_timeout() {
        return DecisionError::Timeout {
            seconds: timeout.as_secs(),
        };
    }
    DecisionError::Unavailable(format!(
        "Ollama at {endpoint} - {}",
        describe(error, timeout)
    ))
}

/// The provider's `{"error": "..."}` sentence, when the body carries one.
fn error_sentence(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .get("error")?
        .as_str()
        .map(str::to_string)
}

/// A non-2xx reply read into the typed reason it happened.
///
/// The status alone is not enough. A 404 is "that model is not pulled"
/// when the body is the provider's JSON, and "this Ollama has no
/// decision route at all" when it is the router's own `404 page not
/// found` — the same status, and two completely different things for a
/// user to do about it.
pub(crate) fn status_error(status: u16, body: &str, model: &str, endpoint: &str) -> DecisionError {
    let sentence = error_sentence(body);
    match status {
        404 => match sentence {
            Some(detail) if detail.contains("not found") => DecisionError::ModelMissing {
                model: model.to_string(),
            },
            Some(detail) => DecisionError::Invalid(detail),
            None => DecisionError::Unsupported {
                endpoint: endpoint.to_string(),
            },
        },
        413 => DecisionError::TooLarge(
            sentence.unwrap_or_else(|| "the request body is over the server's limit".into()),
        ),
        500..=599 => DecisionError::ScoringFailed(
            sentence.unwrap_or_else(|| format!("the provider answered {status}")),
        ),
        _ => match sentence {
            // Two different wordings for the same user mistake: a chat
            // model, and a model whose weights the scoring runner
            // cannot use. Both mean "this is not the decision role".
            Some(detail)
                if detail.contains("does not support decision")
                    || detail.contains("not supported by System One") =>
            {
                DecisionError::NotADecisionModel {
                    model: model.to_string(),
                    detail,
                }
            }
            Some(detail) => DecisionError::Invalid(detail),
            None => DecisionError::Invalid(format!("the provider answered {status}")),
        },
    }
}

/// Translates the reply JSON into the domain outcome.
pub fn parse_outcome(body: &str) -> Result<Outcome, DecisionError> {
    serde_json::from_str(body).map_err(|e| {
        DecisionError::Malformed(format!(
            "{SYSTEMONE_PATH} replied with {e} - body: {}",
            preview(body)
        ))
    })
}

/// Enough of an unexpected body to recognize it in a log, without
/// pasting a whole model reply into one line.
fn preview(body: &str) -> String {
    const LIMIT: usize = 200;
    let trimmed = body.trim();
    match trimmed.char_indices().nth(LIMIT) {
        None => trimmed.to_string(),
        Some((at, _)) => format!("{}…", &trimmed[..at]),
    }
}

/// Every question asked got an answer, and nothing answered a question
/// that was never asked.
///
/// Worth its own check: reading `answers` by position or by hope is how
/// a judgment plane ends up confidently judging a different question
/// from the one it meant to ask.
pub(crate) fn check_answers(request: &Request, outcome: &Outcome) -> Result<(), DecisionError> {
    let missing: Vec<String> = request
        .questions
        .keys()
        .filter(|name| !outcome.answers.contains_key(*name))
        .cloned()
        .collect();
    let unexpected: Vec<String> = outcome
        .answers
        .keys()
        .filter(|name| !request.questions.contains_key(*name))
        .cloned()
        .collect();
    if missing.is_empty() && unexpected.is_empty() {
        return Ok(());
    }
    Err(DecisionError::UnexpectedAnswers {
        missing,
        unexpected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::decision::{Answer, CRITERION_MEASURABLE, Question, measurable_state};
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    fn request() -> Request {
        Request::single(
            CRITERION_MEASURABLE.answer_key(),
            CRITERION_MEASURABLE.question(),
            measurable_state("Given \"1,2\", when add is called, then the result is 3"),
        )
    }

    /// A one-shot HTTP server returning a canned status and body, and
    /// handing back whatever request it was sent.
    fn serve(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 8192];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let reason = if (200..300).contains(&status) {
                "OK"
            } else {
                "ERROR"
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            request
        });
        (format!("http://127.0.0.1:{port}"), handle)
    }

    #[test]
    fn a_documented_reply_parses_into_typed_answers() {
        let body = r#"{"model":"nimble:latest",
                       "answers":{"measurable":{"type":"noul","noul":0.9904}},
                       "usage":{"input_tokens":151,"output_tokens":1}}"#;
        let (endpoint, server) = serve(200, body);
        let outcome = OllamaDecision::new(endpoint)
            .decide("nimble:latest", &request())
            .unwrap();
        let sent = server.join().unwrap();
        assert_eq!(outcome.model, "nimble:latest");
        assert_eq!(outcome.answers["measurable"], Answer::Noul { noul: 0.9904 });
        assert_eq!(outcome.usage.input_tokens, 151);
        assert!(sent.starts_with("POST /v1/systemone "), "got: {sent}");
        assert!(sent.contains("\"model\":\"nimble:latest\""));
        assert!(sent.contains("\"type\":\"noul\""));
        assert!(sent.contains("\"acceptance_criterion\""));
        assert!(
            sent.contains("\"keep_alive\":\"0\""),
            "the decision model is released as soon as it has answered: {sent}"
        );
    }

    #[test]
    fn the_request_names_the_configured_endpoint_without_doubling_the_slash() {
        let (endpoint, server) = serve(
            200,
            r#"{"model":"m","answers":{"measurable":{"type":"noul","noul":0.5}},
                "usage":{"input_tokens":1,"output_tokens":1}}"#,
        );
        let client = OllamaDecision::new(format!("{endpoint}/"));
        assert!(client.url().ends_with("/v1/systemone"));
        assert!(!client.url().contains("//v1"));
        client.decide("m", &request()).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn an_empty_model_name_is_refused_before_any_request() {
        let error = OllamaDecision::new("http://127.0.0.1:9".into())
            .decide("  ", &request())
            .unwrap_err();
        assert_eq!(error, DecisionError::Invalid("model is required".into()));
    }

    #[test]
    fn a_locally_invalid_request_never_leaves_the_process() {
        let bad = Request::single(
            "measurable",
            Question::Noul {
                instructions: String::new(),
                criteria: None,
            },
            serde_json::json!("x"),
        );
        // Port 9 is reliably closed: reaching the network would be a
        // transport error, not the validation error asserted here.
        let error = OllamaDecision::new("http://127.0.0.1:9".into())
            .decide("nimble", &bad)
            .unwrap_err();
        assert!(
            matches!(&error, DecisionError::Invalid(detail) if detail.contains("instructions")),
            "got {error:?}"
        );
    }

    #[test]
    fn a_brief_over_the_servers_body_limit_is_refused_locally_with_the_budget() {
        let huge = Request::single(
            CRITERION_MEASURABLE.answer_key(),
            CRITERION_MEASURABLE.question(),
            measurable_state(&"x".repeat(MAX_REQUEST_BYTES + 1)),
        );
        let error = OllamaDecision::new("http://127.0.0.1:9".into())
            .decide("nimble", &huge)
            .unwrap_err();
        assert!(
            matches!(&error, DecisionError::TooLarge(detail)
                if detail.contains("65536") && detail.contains("does not truncate")),
            "got {error:?}"
        );
    }

    #[test]
    fn an_unreachable_provider_names_the_endpoint() {
        let error =
            OllamaDecision::with_timeout("http://127.0.0.1:9".into(), Duration::from_millis(300))
                .decide("nimble", &request())
                .unwrap_err();
        assert!(
            matches!(&error, DecisionError::Unavailable(detail)
                if detail.contains("http://127.0.0.1:9")),
            "got {error:?}"
        );
        assert!(
            error
                .to_string()
                .contains("cannot reach the decision model")
        );
    }

    #[test]
    fn a_slow_provider_is_a_timeout_naming_the_configurable_budget() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);
            std::thread::sleep(Duration::from_millis(600));
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
        });
        let error = OllamaDecision::with_timeout(
            format!("http://127.0.0.1:{port}"),
            Duration::from_millis(150),
        )
        .decide("nimble", &request())
        .unwrap_err();
        let _ = server.join();
        assert_eq!(error, DecisionError::Timeout { seconds: 0 });
        assert!(
            error
                .to_string()
                .contains("timeout_seconds under [decision]")
        );
    }

    #[test]
    fn a_model_that_is_not_pulled_is_told_apart_from_a_missing_route() {
        let (endpoint, server) = serve(
            404,
            r#"{"error":"model \"absent:test\" not found, try pulling it first"}"#,
        );
        let error = OllamaDecision::new(endpoint)
            .decide("absent:test", &request())
            .unwrap_err();
        server.join().unwrap();
        assert_eq!(
            error,
            DecisionError::ModelMissing {
                model: "absent:test".into()
            }
        );
        assert!(error.to_string().contains("ollama pull absent:test"));
    }

    /// Ollama before 0.35 has no decision route, and its router answers
    /// a plain-text 404 rather than the provider's JSON.
    #[test]
    fn a_provider_without_the_decision_route_says_to_upgrade_ollama() {
        let (endpoint, server) = serve(404, "404 page not found");
        let error = OllamaDecision::new(endpoint.clone())
            .decide("nimble", &request())
            .unwrap_err();
        server.join().unwrap();
        assert_eq!(error, DecisionError::Unsupported { endpoint });
        assert!(error.to_string().contains("Ollama 0.35 or newer"));
        assert!(error.to_string().contains("/v1/systemone"));
    }

    #[test]
    fn a_chat_model_asked_for_a_decision_is_refused_with_the_capability_named() {
        let (endpoint, server) = serve(
            400,
            r#"{"error":"registry.ollama.ai/library/llama3.2:latest does not support decision"}"#,
        );
        let error = OllamaDecision::new(endpoint)
            .decide("llama3.2:latest", &request())
            .unwrap_err();
        server.join().unwrap();
        assert!(
            matches!(&error, DecisionError::NotADecisionModel { model, detail }
                if model == "llama3.2:latest" && detail.contains("does not support decision")),
            "got {error:?}"
        );
        let shown = error.to_string();
        assert!(shown.contains("cannot answer decisions"));
        assert!(shown.contains("'decision'"));
        assert!(shown.contains("spec judge models"));
    }

    /// The other wording for the same mistake: the tag is a decision
    /// model's name but the weights are MLX, which the scoring runner
    /// cannot use.
    #[test]
    fn weights_the_scoring_runner_cannot_use_are_the_same_refusal() {
        let (endpoint, server) = serve(
            400,
            r#"{"error":"model \"qwen3.6:35b-mlx\" is not supported by System One; use a local GGUF model"}"#,
        );
        let error = OllamaDecision::new(endpoint)
            .decide("qwen3.6:35b-mlx", &request())
            .unwrap_err();
        server.join().unwrap();
        assert!(
            matches!(&error, DecisionError::NotADecisionModel { detail, .. }
                if detail.contains("GGUF")),
            "got {error:?}"
        );
    }

    #[test]
    fn a_rejected_question_carries_the_providers_own_sentence() {
        let (endpoint, server) = serve(
            400,
            r#"{"error":"question \"q\": type must be choice, noul, or score"}"#,
        );
        let error = OllamaDecision::new(endpoint)
            .decide("nimble", &request())
            .unwrap_err();
        server.join().unwrap();
        assert_eq!(
            error,
            DecisionError::Invalid("question \"q\": type must be choice, noul, or score".into())
        );
    }

    #[test]
    fn an_oversize_body_reported_by_the_server_is_a_size_error() {
        let (endpoint, server) = serve(
            413,
            r#"{"error":"request body must not exceed 64 KiB without images"}"#,
        );
        let error = OllamaDecision::new(endpoint)
            .decide("nimble", &request())
            .unwrap_err();
        server.join().unwrap();
        assert!(
            matches!(&error, DecisionError::TooLarge(detail) if detail.contains("64 KiB")),
            "got {error:?}"
        );
    }

    #[test]
    fn a_scoring_failure_is_reported_as_one() {
        let (endpoint, server) = serve(500, r#"{"error":"failed to load model"}"#);
        let error = OllamaDecision::new(endpoint)
            .decide("nimble", &request())
            .unwrap_err();
        server.join().unwrap();
        assert_eq!(
            error,
            DecisionError::ScoringFailed("failed to load model".into())
        );
        assert!(error.to_string().contains("failed to answer"));
    }

    #[test]
    fn a_body_that_is_not_the_documented_shape_is_malformed() {
        let (endpoint, server) = serve(200, "not json at all");
        let error = OllamaDecision::new(endpoint)
            .decide("nimble", &request())
            .unwrap_err();
        server.join().unwrap();
        assert!(
            matches!(&error, DecisionError::Malformed(detail)
                if detail.contains("/v1/systemone replied with")
                    && detail.contains("not json at all")),
            "got {error:?}"
        );
    }

    /// A `noul` answer with no probability is not a probability of zero.
    #[test]
    fn an_answer_missing_its_value_is_malformed_rather_than_a_false_verdict() {
        let (endpoint, server) = serve(
            200,
            r#"{"model":"nimble","answers":{"measurable":{"type":"noul"}},
                "usage":{"input_tokens":1,"output_tokens":1}}"#,
        );
        let error = OllamaDecision::new(endpoint)
            .decide("nimble", &request())
            .unwrap_err();
        server.join().unwrap();
        assert!(
            matches!(error, DecisionError::Malformed(_)),
            "got {error:?}"
        );
    }

    #[test]
    fn an_answer_to_a_question_that_was_never_asked_is_refused() {
        let (endpoint, server) = serve(
            200,
            r#"{"model":"nimble","answers":{"urgency":{"type":"noul","noul":0.9}},
                "usage":{"input_tokens":1,"output_tokens":1}}"#,
        );
        let error = OllamaDecision::new(endpoint)
            .decide("nimble", &request())
            .unwrap_err();
        server.join().unwrap();
        assert_eq!(
            error,
            DecisionError::UnexpectedAnswers {
                missing: vec!["measurable".into()],
                unexpected: vec!["urgency".into()],
            }
        );
        let shown = error.to_string();
        assert!(shown.contains("unanswered: measurable"));
        assert!(shown.contains("never asked: urgency"));
    }

    #[test]
    fn an_unanswered_question_is_refused_even_when_nothing_extra_came_back() {
        let (endpoint, server) = serve(
            200,
            r#"{"model":"nimble","answers":{},"usage":{"input_tokens":1,"output_tokens":1}}"#,
        );
        let error = OllamaDecision::new(endpoint)
            .decide("nimble", &request())
            .unwrap_err();
        server.join().unwrap();
        assert!(
            matches!(&error, DecisionError::UnexpectedAnswers { missing, unexpected }
                if missing == &["measurable".to_string()] && unexpected.is_empty()),
            "got {error:?}"
        );
    }

    #[test]
    fn a_reply_without_usage_still_parses() {
        let outcome =
            parse_outcome(r#"{"model":"m","answers":{"measurable":{"type":"noul","noul":0.5}}}"#)
                .unwrap();
        assert_eq!(outcome.usage.input_tokens, 0);
    }

    #[test]
    fn a_long_malformed_body_is_previewed_not_pasted_whole() {
        let error = parse_outcome(&"x".repeat(5000)).unwrap_err();
        let DecisionError::Malformed(detail) = error else {
            panic!("expected malformed");
        };
        assert!(detail.len() < 400, "preview was {} chars", detail.len());
        assert!(detail.contains('…'));
    }

    #[test]
    fn a_multibyte_body_is_previewed_on_a_character_boundary() {
        let error = parse_outcome(&"é".repeat(5000)).unwrap_err();
        assert!(matches!(error, DecisionError::Malformed(_)));
    }

    #[test]
    fn the_status_mapping_covers_an_unlabelled_client_error() {
        assert_eq!(
            status_error(418, "teapot", "m", "http://e"),
            DecisionError::Invalid("the provider answered 418".into())
        );
        assert_eq!(
            status_error(502, "gateway", "m", "http://e"),
            DecisionError::ScoringFailed("the provider answered 502".into())
        );
        assert_eq!(
            status_error(413, "nope", "m", "http://e"),
            DecisionError::TooLarge("the request body is over the server's limit".into())
        );
        assert_eq!(
            status_error(
                404,
                r#"{"error":"something else entirely"}"#,
                "m",
                "http://e"
            ),
            DecisionError::Invalid("something else entirely".into())
        );
    }

    #[test]
    fn the_default_budget_is_well_under_the_generative_one() {
        assert_eq!(DEFAULT_DECISION_TIMEOUT, Duration::from_secs(60));
        assert!(DEFAULT_DECISION_TIMEOUT < super::super::ollama::DEFAULT_GENERATION_TIMEOUT);
    }
}
