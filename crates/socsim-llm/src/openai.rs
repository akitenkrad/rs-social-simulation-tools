//! OpenAI backend (`/v1/chat/completions`), gated behind the `openai` feature.

use serde_json::json;

use crate::client::{
    reject_blank_response, CallMetadata, LlmClient, LlmConfig, LlmError, LlmResponse, TokenLogprob,
    DEFAULT_TOP_LOGPROBS,
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

    /// Build the request `messages`: an optional `{role: system}` message
    /// (only when `config.system` is `Some`) followed by the user message.
    /// With no system prompt this matches the previous single-message body.
    fn build_messages(prompt: &str, config: &LlmConfig) -> serde_json::Value {
        match &config.system {
            Some(system) => json!([
                { "role": "system", "content": system },
                { "role": "user", "content": prompt },
            ]),
            None => json!([{ "role": "user", "content": prompt }]),
        }
    }

    /// Build the base request body honouring `temperature`, `seed` (unless
    /// `config.omit_seed`), `system` and `max_tokens`.  Shared by the plain and
    /// logprob request paths.
    fn build_body(&self, prompt: &str, config: &LlmConfig) -> serde_json::Value {
        let mut body = json!({
            "model": self.model,
            "temperature": config.temperature,
            "messages": Self::build_messages(prompt, config),
        });
        if !config.omit_seed {
            body["seed"] = json!(config.seed);
        }
        if let Some(max) = config.max_tokens {
            body["max_tokens"] = json!(max);
        }
        body
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
        let body = self.build_body(prompt, config);

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
        // thinking trace) is surfaced as an error rather than passed through —
        // unless the caller opted into tolerating a blank response.
        let text = if config.allow_blank {
            text
        } else {
            reject_blank_response(text, &self.endpoint, &self.model)?
        };

        Ok(LlmResponse {
            text,
            metadata: CallMetadata {
                model: self.model.clone(),
                endpoint: self.endpoint.clone(),
                temperature: config.temperature,
                seed: config.seed,
                cache_hit: false,
            },
            logprobs: None,
        })
    }

    fn complete_with_logprobs(
        &self,
        prompt: &str,
        config: &LlmConfig,
    ) -> Result<LlmResponse, LlmError> {
        if self.api_key.is_empty() {
            return Err(LlmError::Config("OpenAI api_key is empty".into()));
        }
        let top_logprobs = config.top_logprobs.unwrap_or(DEFAULT_TOP_LOGPROBS).max(1);
        let mut body = self.build_body(prompt, config);
        body["logprobs"] = json!(true);
        body["top_logprobs"] = json!(top_logprobs);

        let resp = ureq::post(&self.endpoint)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .send_json(body)
            .map_err(|e| map_ureq_error(&self.endpoint, e))?;

        let value: serde_json::Value = resp.into_json().map_err(|e| LlmError::Backend {
            endpoint: self.endpoint.clone(),
            status: 0,
            message: format!("decoding response: {e}"),
        })?;

        let choice = value.get("choices").and_then(|c| c.get(0));
        let text = choice
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string();

        let logprobs = choice.and_then(parse_openai_logprobs);

        Ok(LlmResponse {
            text,
            metadata: CallMetadata {
                model: self.model.clone(),
                endpoint: self.endpoint.clone(),
                temperature: config.temperature,
                seed: config.seed,
                cache_hit: false,
            },
            logprobs,
        })
    }
}

/// Parse the first generated token's top alternatives out of an OpenAI
/// chat-completions choice (`choice.logprobs.content[0].top_logprobs`) into
/// [`TokenLogprob`]s, or `None` if absent.
///
/// OpenAI returns the token string but not raw bytes for `top_logprobs`
/// alternatives, so `bytes` is filled from the token's UTF-8 when the API does
/// not supply a `bytes` array.
fn parse_openai_logprobs(choice: &serde_json::Value) -> Option<Vec<TokenLogprob>> {
    let content = choice
        .get("logprobs")
        .and_then(|l| l.get("content"))
        .and_then(|c| c.as_array())?;
    let first = content.first()?;
    let tops = first.get("top_logprobs").and_then(|t| t.as_array())?;
    Some(
        tops.iter()
            .map(|t| {
                let token = t
                    .get("token")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let bytes = t
                    .get("bytes")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|b| b.as_u64().map(|n| n as u8))
                            .collect()
                    })
                    .unwrap_or_else(|| token.as_bytes().to_vec());
                TokenLogprob {
                    token,
                    bytes,
                    logprob: t
                        .get("logprob")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(f64::NEG_INFINITY),
                }
            })
            .collect(),
    )
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

    #[test]
    fn default_body_emits_seed_and_single_user_message() {
        let c = OpenAiClient::new("sk", "gpt-4o-mini");
        let body = c.build_body("hi", &LlmConfig::deterministic().with_seed(5));
        assert_eq!(body["seed"], json!(5));
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn omit_seed_drops_seed_and_system_prepends() {
        let c = OpenAiClient::new("sk", "gpt-4o-mini");
        let cfg = LlmConfig::sampling(1.0).with_system("be terse");
        let body = c.build_body("hi", &cfg);
        assert!(body.get("seed").is_none(), "{body}");
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "be terse");
    }

    #[test]
    fn parses_openai_logprobs() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"logprobs": {"content": [
                {"token": "Yes", "logprob": -0.1,
                 "top_logprobs": [
                   {"token": "Yes", "logprob": -0.1},
                   {"token": "No",  "logprob": -2.3, "bytes": [78,111]}
                 ]}
            ]}}"#,
        )
        .unwrap();
        let lp = parse_openai_logprobs(&value).expect("logprobs");
        assert_eq!(lp.len(), 2);
        assert_eq!(lp[0].token, "Yes");
        // bytes filled from token UTF-8 when API omits them.
        assert_eq!(lp[0].bytes, b"Yes".to_vec());
        assert_eq!(lp[1].bytes, vec![78, 111]);
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
