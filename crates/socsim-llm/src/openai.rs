//! OpenAI backend (`/v1/chat/completions`), gated behind the `openai` feature.

use serde_json::json;

use crate::client::{
    reject_blank_response, CallMetadata, LlmClient, LlmConfig, LlmError, LlmResponse,
};

/// A synchronous [`LlmClient`] for the OpenAI chat-completions API.
///
/// Reads `OPENAI_API_KEY` and `OPENAI_MODEL` (default `gpt-4o-mini`) from the
/// environment via [`OpenAiClient::from_env`], or take explicit values with
/// [`OpenAiClient::new`].  `temperature` is sent and `seed` is forwarded
/// best-effort (OpenAI's `seed` is documented as best-effort determinism).
pub struct OpenAiClient {
    api_key: String,
    model: String,
    base_url: String,
    endpoint: String,
}

impl OpenAiClient {
    /// Build a client for `model` using `api_key` against the default base URL
    /// (`https://api.openai.com`).
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::with_base_url("https://api.openai.com", api_key, model)
    }

    /// Build a client against a custom `base_url` (e.g. an OpenAI-compatible
    /// gateway).
    pub fn with_base_url(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        let base_url = base_url.into();
        let endpoint = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url,
            endpoint,
        }
    }

    /// Build a client from `OPENAI_API_KEY` / `OPENAI_MODEL`.
    ///
    /// Returns [`LlmError::Config`] if `OPENAI_API_KEY` is unset.
    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| LlmError::Config("OPENAI_API_KEY is not set".into()))?;
        let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
        Ok(Self::new(api_key, model))
    }

    /// The configured base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl LlmClient for OpenAiClient {
    fn model(&self) -> &str {
        &self.model
    }

    fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn complete(&self, prompt: &str, config: &LlmConfig) -> Result<LlmResponse, LlmError> {
        if self.api_key.is_empty() {
            return Err(LlmError::Config("OpenAI api_key is empty".into()));
        }
        let mut body = json!({
            "model": self.model,
            "temperature": config.temperature,
            "seed": config.seed,
            "messages": [{ "role": "user", "content": prompt }],
        });
        if let Some(max) = config.max_tokens {
            body["max_tokens"] = json!(max);
        }

        let resp = ureq::post(&self.endpoint)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .send_json(body)
            .map_err(|e| map_ureq_error(&self.endpoint, e))?;

        let value: serde_json::Value = resp.into_json().map_err(|e| LlmError::Backend {
            endpoint: self.endpoint.clone(),
            status: 0,
            message: format!("decoding response: {e}"),
        })?;

        let text = value
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| LlmError::Backend {
                endpoint: self.endpoint.clone(),
                status: 0,
                message: format!("missing choices[0].message.content in response: {value}"),
            })?
            .to_string();

        // A successful call that produced no visible text (e.g. a reasoning
        // model that consumed its whole `max_tokens` budget on a hidden
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
    fn endpoint_is_built_from_base_url() {
        let c = OpenAiClient::new("sk-test", "gpt-4o-mini");
        assert_eq!(c.endpoint(), "https://api.openai.com/v1/chat/completions");
        assert_eq!(c.model(), "gpt-4o-mini");
    }

    #[test]
    fn empty_key_is_a_config_error() {
        let c = OpenAiClient::new("", "gpt-4o-mini");
        let err = c.complete("hi", &LlmConfig::deterministic()).unwrap_err();
        assert!(matches!(err, LlmError::Config(_)));
    }

    /// Live network test — requires a real API key. Ignored by default.
    #[test]
    #[ignore = "requires a live OpenAI API key"]
    fn live_chat() {
        let c = OpenAiClient::from_env().unwrap();
        let r = c.complete(
            "Reply with the single word OK.",
            &LlmConfig::deterministic(),
        );
        assert!(r.is_ok(), "{r:?}");
    }
}
