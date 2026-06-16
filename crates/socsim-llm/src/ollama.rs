//! Ollama backend (`/api/chat`), gated behind the `ollama` feature.

use serde_json::json;

use crate::client::{
    reject_blank_response, CallMetadata, LlmClient, LlmConfig, LlmError, LlmResponse,
};

/// A synchronous [`LlmClient`] for a local/remote [Ollama](https://ollama.com)
/// server.
///
/// Reads `OLLAMA_HOST` (default `http://localhost:11434`) and `OLLAMA_MODEL`
/// (default `llama3.1`) from the environment via [`OllamaClient::from_env`],
/// or take an explicit host/model with [`OllamaClient::new`].  Requests
/// `stream = false` and threads `temperature` / `seed` through
/// `options`.
pub struct OllamaClient {
    host: String,
    model: String,
    endpoint: String,
}

impl OllamaClient {
    /// Build a client for `model` on `host` (e.g. `http://localhost:11434`).
    pub fn new(host: impl Into<String>, model: impl Into<String>) -> Self {
        let host = host.into();
        let endpoint = format!("{}/api/chat", host.trim_end_matches('/'));
        Self {
            host,
            model: model.into(),
            endpoint,
        }
    }

    /// Build a client from `OLLAMA_HOST` / `OLLAMA_MODEL` (with defaults).
    pub fn from_env() -> Self {
        let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".into());
        let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.1".into());
        Self::new(host, model)
    }

    /// The configured host.
    pub fn host(&self) -> &str {
        &self.host
    }
}

impl LlmClient for OllamaClient {
    fn model(&self) -> &str {
        &self.model
    }

    fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn complete(&self, prompt: &str, config: &LlmConfig) -> Result<LlmResponse, LlmError> {
        let mut options = json!({
            "temperature": config.temperature,
            "seed": config.seed,
        });
        if let Some(max) = config.max_tokens {
            options["num_predict"] = json!(max);
        }
        let body = json!({
            "model": self.model,
            "stream": false,
            "messages": [{ "role": "user", "content": prompt }],
            "options": options,
        });

        let resp = ureq::post(&self.endpoint)
            .send_json(body)
            .map_err(|e| map_ureq_error(&self.endpoint, e))?;

        let value: serde_json::Value = resp.into_json().map_err(|e| LlmError::Backend {
            endpoint: self.endpoint.clone(),
            status: 0,
            message: format!("decoding response: {e}"),
        })?;

        let text = value
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| LlmError::Backend {
                endpoint: self.endpoint.clone(),
                status: 0,
                message: format!("missing message.content in response: {value}"),
            })?
            .to_string();

        // A successful call that produced no visible text (e.g. a reasoning
        // model that consumed its whole `num_predict` budget on a hidden
        // thinking trace) is surfaced as an error rather than passed through.
        let text = reject_blank_response(text, &self.endpoint, &self.model)?;

        Ok(LlmResponse {
            text,
            metadata: CallMetadata {
                model: self.model.clone(),
                endpoint: self.endpoint.clone(),
                temperature: config.temperature,
                seed: config.seed,
                cache_hit: false,
            },
        })
    }
}

/// Map a `ureq::Error` into an [`LlmError`], preserving the HTTP status when
/// the backend returned one.
fn map_ureq_error(endpoint: &str, e: ureq::Error) -> LlmError {
    match e {
        ureq::Error::Status(status, resp) => LlmError::Backend {
            endpoint: endpoint.to_string(),
            status,
            message: resp
                .into_string()
                .unwrap_or_else(|_| "<unreadable body>".into()),
        },
        ureq::Error::Transport(t) => LlmError::Transport {
            endpoint: endpoint.to_string(),
            message: t.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_is_built_from_host() {
        let c = OllamaClient::new("http://localhost:11434/", "llama3.1");
        assert_eq!(c.endpoint(), "http://localhost:11434/api/chat");
        assert_eq!(c.model(), "llama3.1");
    }

    /// Live network test — requires a running Ollama. Ignored by default.
    #[test]
    #[ignore = "requires a live Ollama server"]
    fn live_chat() {
        let c = OllamaClient::from_env();
        let r = c.complete(
            "Reply with the single word OK.",
            &LlmConfig::deterministic(),
        );
        assert!(r.is_ok(), "{r:?}");
    }
}
