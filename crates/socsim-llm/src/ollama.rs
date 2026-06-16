//! Ollama backend (`/api/chat`), gated behind the `ollama` feature.

use serde_json::json;

use crate::client::{
    reject_blank_response, CallMetadata, LlmClient, LlmConfig, LlmError, LlmResponse, TokenLogprob,
    DEFAULT_TOP_LOGPROBS,
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

    /// Build the `options` object honouring `temperature`, `seed` (unless
    /// `config.omit_seed`) and `max_tokens`.  Factored out so the plain and
    /// logprob request paths share identical option semantics, and unit-tested
    /// below.
    fn build_options(config: &LlmConfig) -> serde_json::Value {
        let mut options = json!({ "temperature": config.temperature });
        if !config.omit_seed {
            options["seed"] = json!(config.seed);
        }
        if let Some(max) = config.max_tokens {
            options["num_predict"] = json!(max);
        }
        options
    }

    /// Build the `messages` array: an optional `{role: system}` message
    /// (present only when `config.system` is `Some`) followed by the user
    /// message.  With no system prompt this is byte-identical to the previous
    /// single-user-message body.
    fn build_messages(prompt: &str, config: &LlmConfig) -> serde_json::Value {
        match &config.system {
            Some(system) => json!([
                { "role": "system", "content": system },
                { "role": "user", "content": prompt },
            ]),
            None => json!([{ "role": "user", "content": prompt }]),
        }
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
        let body = json!({
            "model": self.model,
            "stream": false,
            "messages": Self::build_messages(prompt, config),
            "options": Self::build_options(config),
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
        let top_logprobs = config.top_logprobs.unwrap_or(DEFAULT_TOP_LOGPROBS).max(1);
        let body = json!({
            "model": self.model,
            "stream": false,
            "messages": Self::build_messages(prompt, config),
            "options": Self::build_options(config),
            "logprobs": true,
            "top_logprobs": top_logprobs,
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
            .unwrap_or_default()
            .to_string();

        let logprobs = parse_logprobs(&value);

        // Logprob callers (argyle2023) request a single token and care about the
        // distribution, not the visible text — tolerate a blank completion by
        // default here, but still honour an explicit `allow_blank = false` only
        // when the caller has not asked for logprobs … keep it simple: do not
        // reject, since a logprobs request that returned a distribution is a
        // success even with empty text.
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

/// Parse the first generation position's `top_logprobs` out of an Ollama
/// `/api/chat` response into [`TokenLogprob`]s, or `None` if absent.
///
/// Ollama exposes per-position logprobs under either a top-level `logprobs`
/// array or `message.logprobs` depending on version; we try both.  Each
/// position carries a `top_logprobs` list of `{token, bytes, logprob}`.
fn parse_logprobs(value: &serde_json::Value) -> Option<Vec<TokenLogprob>> {
    let positions = value
        .get("logprobs")
        .or_else(|| value.get("message").and_then(|m| m.get("logprobs")))
        .and_then(|l| l.as_array())?;
    let first = positions.first()?;
    let tops = first.get("top_logprobs").and_then(|t| t.as_array())?;
    Some(
        tops.iter()
            .map(|t| TokenLogprob {
                token: t
                    .get("token")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                bytes: t
                    .get("bytes")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|b| b.as_u64().map(|n| n as u8))
                            .collect()
                    })
                    .unwrap_or_default(),
                logprob: t
                    .get("logprob")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(f64::NEG_INFINITY),
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
    fn endpoint_is_built_from_host() {
        let c = OllamaClient::new("http://localhost:11434/", "llama3.1");
        assert_eq!(c.endpoint(), "http://localhost:11434/api/chat");
        assert_eq!(c.model(), "llama3.1");
    }

    #[test]
    fn default_options_emit_seed_and_no_system() {
        // Backward-compat: default config emits the seed and sends only a single
        // user message (no system role) — byte-identical to the old behaviour.
        let cfg = LlmConfig::deterministic().with_seed(7);
        let opts = OllamaClient::build_options(&cfg);
        assert_eq!(opts["seed"], json!(7));
        assert_eq!(opts["temperature"], json!(0.0));

        let msgs = OllamaClient::build_messages("hi", &cfg);
        assert_eq!(msgs.as_array().unwrap().len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "hi");
    }

    #[test]
    fn omit_seed_drops_seed_from_options() {
        let cfg = LlmConfig::sampling(1.0); // omit_seed = true
        let opts = OllamaClient::build_options(&cfg);
        assert!(opts.get("seed").is_none(), "{opts}");
        assert_eq!(opts["temperature"], json!(1.0));
    }

    #[test]
    fn system_prompt_prepends_system_message() {
        let cfg = LlmConfig::deterministic().with_system("Racially, I am white.");
        let msgs = OllamaClient::build_messages("Who did you vote for?", &cfg);
        let arr = msgs.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Racially, I am white.");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"], "Who did you vote for?");
    }

    #[test]
    fn parses_chat_logprobs() {
        // Mirrors Ollama's /api/chat logprobs schema (top-level `logprobs`).
        let value: serde_json::Value = serde_json::from_str(
            r#"{
              "message": {"content": " Donald"},
              "logprobs": [
                {"token": " Donald", "logprob": -0.32, "bytes": [32,68],
                 "top_logprobs": [
                   {"token": " Donald", "logprob": -0.32, "bytes": [32,68]},
                   {"token": " Joe",    "logprob": -1.99, "bytes": [32,74,111,101]}
                 ]}
              ]
            }"#,
        )
        .unwrap();
        let lp = parse_logprobs(&value).expect("logprobs present");
        assert_eq!(lp.len(), 2);
        assert_eq!(lp[0].token, " Donald");
        assert!((lp[1].logprob - (-1.99)).abs() < 1e-9);
        assert_eq!(lp[1].token, " Joe");
        assert_eq!(lp[1].bytes, vec![32, 74, 111, 101]);
    }

    #[test]
    fn parses_logprobs_from_message_nested_form() {
        // Alternate schema: logprobs nested under message.
        let value: serde_json::Value = serde_json::from_str(
            r#"{"message": {"content": "x", "logprobs": [
                 {"top_logprobs": [{"token":"a","logprob":-0.1,"bytes":[97]}]}
               ]}}"#,
        )
        .unwrap();
        let lp = parse_logprobs(&value).expect("logprobs present");
        assert_eq!(lp.len(), 1);
        assert_eq!(lp[0].token, "a");
    }

    #[test]
    fn parse_logprobs_absent_is_none() {
        let value: serde_json::Value =
            serde_json::from_str(r#"{"message": {"content": "x"}}"#).unwrap();
        assert!(parse_logprobs(&value).is_none());
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
