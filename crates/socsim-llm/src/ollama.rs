//! Ollama backend (`/api/chat` or `/api/generate`), gated behind the `ollama`
//! feature.

use serde_json::json;

use crate::client::{
    reject_blank_response, CallMetadata, LlmClient, LlmConfig, LlmError, LlmResponse, TokenLogprob,
    DEFAULT_TOP_LOGPROBS,
};

/// Which Ollama API surface an [`OllamaClient`] talks to.
///
/// Ollama exposes two single-turn generation endpoints with different request
/// and response shapes:
///
/// - [`Chat`](OllamaApi::Chat) → `/api/chat`, a role-structured chat completion
///   (`messages: [{role, content}, …]`, answer in `message.content`).  This is
///   the default and matches the historical behaviour.
/// - [`Generate`](OllamaApi::Generate) → `/api/generate`, a completion-style
///   endpoint (`prompt` plus an optional top-level `system` string, answer in
///   `response`).
///
/// A client picks **one** endpoint for its whole lifetime (a replication runs
/// against a single endpoint), selected via [`OllamaClient::with_api`].  Both
/// endpoints share identical `options` semantics (`temperature` / `seed` /
/// `num_predict`) via [`OllamaClient::build_options`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OllamaApi {
    /// The role-structured `/api/chat` endpoint (the default).
    #[default]
    Chat,
    /// The completion-style `/api/generate` endpoint.
    Generate,
}

impl OllamaApi {
    /// The path this API surface lives at (`/api/chat` or `/api/generate`).
    fn path(self) -> &'static str {
        match self {
            OllamaApi::Chat => "/api/chat",
            OllamaApi::Generate => "/api/generate",
        }
    }
}

/// A synchronous [`LlmClient`] for a local/remote [Ollama](https://ollama.com)
/// server.
///
/// Reads `OLLAMA_HOST` (default `http://localhost:11434`) and `OLLAMA_MODEL`
/// (default `llama3.1`) from the environment via [`OllamaClient::from_env`],
/// or take an explicit host/model with [`OllamaClient::new`].  Requests
/// `stream = false` and threads `temperature` / `seed` through
/// `options`.
///
/// Defaults to the [`Chat`](OllamaApi::Chat) endpoint (`/api/chat`); call
/// [`with_api`](OllamaClient::with_api) to select the completion-style
/// [`Generate`](OllamaApi::Generate) endpoint (`/api/generate`) instead.
///
/// By default no per-request timeout is set (`None`), preserving the historical
/// behaviour where a request blocks until the server responds; set one with
/// [`with_timeout`](OllamaClient::with_timeout) /
/// [`with_timeout_secs`](OllamaClient::with_timeout_secs) so a hung server
/// surfaces a transport error instead of blocking forever.
pub struct OllamaClient {
    host: String,
    model: String,
    endpoint: String,
    api: OllamaApi,
    timeout: Option<std::time::Duration>,
}

impl OllamaClient {
    /// Build a client for `model` on `host` (e.g. `http://localhost:11434`).
    ///
    /// Defaults to the [`Chat`](OllamaApi::Chat) endpoint; chain
    /// [`with_api`](Self::with_api) to switch to `/api/generate`.
    pub fn new(host: impl Into<String>, model: impl Into<String>) -> Self {
        let host = host.into();
        let api = OllamaApi::default();
        let endpoint = Self::endpoint_for(&host, api);
        Self {
            host,
            model: model.into(),
            endpoint,
            api,
            timeout: None,
        }
    }

    /// Build a client from `OLLAMA_HOST` / `OLLAMA_MODEL` (with defaults).
    ///
    /// Defaults to the [`Chat`](OllamaApi::Chat) endpoint.
    pub fn from_env() -> Self {
        let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".into());
        let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.1".into());
        Self::new(host, model)
    }

    /// Select which Ollama API surface this client talks to (builder style),
    /// recomputing the [`endpoint`](Self::endpoint) to `/api/chat` or
    /// `/api/generate` accordingly.
    pub fn with_api(mut self, api: OllamaApi) -> Self {
        self.endpoint = Self::endpoint_for(&self.host, api);
        self.api = api;
        self
    }

    /// The Ollama API surface this client talks to.
    pub fn api(&self) -> OllamaApi {
        self.api
    }

    /// Set a per-request overall timeout (builder style).
    ///
    /// Applies to every request this client makes; when unset (the default,
    /// `None`) requests have no timeout and block until the server responds,
    /// matching the historical behaviour.  A request that exceeds `dur`
    /// surfaces as an [`LlmError::Transport`].
    pub fn with_timeout(mut self, dur: std::time::Duration) -> Self {
        self.timeout = Some(dur);
        self
    }

    /// Set a per-request overall timeout in whole seconds (builder style),
    /// a convenience wrapper over [`with_timeout`](Self::with_timeout) that maps
    /// `secs` to [`Duration::from_secs`](std::time::Duration::from_secs).
    pub fn with_timeout_secs(self, secs: u64) -> Self {
        self.with_timeout(std::time::Duration::from_secs(secs))
    }

    /// The configured per-request timeout, or `None` when no timeout is set
    /// (the default).
    pub fn timeout(&self) -> Option<std::time::Duration> {
        self.timeout
    }

    /// Build a `POST` request to this client's endpoint, applying the
    /// configured [`timeout`](Self::timeout) when one is set.  Factored out so
    /// both request paths (`complete` / `complete_with_logprobs`) apply the
    /// optional timeout identically; with no timeout the request is byte- and
    /// behaviour-identical to the historical `ureq::post(&self.endpoint)`.
    fn post(&self) -> ureq::Request {
        let req = ureq::post(&self.endpoint);
        match self.timeout {
            Some(dur) => req.timeout(dur),
            None => req,
        }
    }

    /// Compute the full endpoint URL for `host` and `api`.
    fn endpoint_for(host: &str, api: OllamaApi) -> String {
        format!("{}{}", host.trim_end_matches('/'), api.path())
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

    /// Build the request body for the selected [`api`](Self::api).
    ///
    /// - [`Chat`](OllamaApi::Chat) → `{model, stream:false, messages, options}`
    ///   (byte-identical to the historical body).
    /// - [`Generate`](OllamaApi::Generate) →
    ///   `{model, prompt, system?, stream:false, options}`, where the top-level
    ///   `system` string is present **only when `config.system` is `Some`**.
    ///
    /// Both share [`build_options`](Self::build_options), so
    /// `temperature` / `seed` (honouring `omit_seed`) / `num_predict`
    /// (`max_tokens`) are identical across the two endpoints.
    fn build_body(&self, prompt: &str, config: &LlmConfig) -> serde_json::Value {
        match self.api {
            OllamaApi::Chat => json!({
                "model": self.model,
                "stream": false,
                "messages": Self::build_messages(prompt, config),
                "options": Self::build_options(config),
            }),
            OllamaApi::Generate => {
                let mut body = json!({
                    "model": self.model,
                    "prompt": prompt,
                    "stream": false,
                    "options": Self::build_options(config),
                });
                if let Some(system) = &config.system {
                    body["system"] = json!(system);
                }
                body
            }
        }
    }

    /// Extract the completion text for the selected [`api`](Self::api): the
    /// chat endpoint returns it under `message.content`, the generate endpoint
    /// under a top-level `response` string.
    fn response_text(&self, value: &serde_json::Value) -> Option<String> {
        match self.api {
            OllamaApi::Chat => value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .map(str::to_string),
            OllamaApi::Generate => value
                .get("response")
                .and_then(|r| r.as_str())
                .map(str::to_string),
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
        let body = self.build_body(prompt, config);

        let resp = self
            .post()
            .send_json(body)
            .map_err(|e| map_ureq_error(&self.endpoint, e))?;

        let value: serde_json::Value = resp.into_json().map_err(|e| LlmError::Backend {
            endpoint: self.endpoint.clone(),
            status: 0,
            message: format!("decoding response: {e}"),
        })?;

        let text = self
            .response_text(&value)
            .ok_or_else(|| LlmError::Backend {
                endpoint: self.endpoint.clone(),
                status: 0,
                message: format!("missing response text in response: {value}"),
            })?;

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
        // Start from the endpoint-appropriate base body (chat messages vs
        // generate prompt/system), then add the logprob request fields the same
        // way for both — Ollama accepts `logprobs` / `top_logprobs` on
        // `/api/generate` exactly as on `/api/chat`.
        let mut body = self.build_body(prompt, config);
        body["logprobs"] = json!(true);
        body["top_logprobs"] = json!(top_logprobs);

        let resp = self
            .post()
            .send_json(body)
            .map_err(|e| map_ureq_error(&self.endpoint, e))?;

        let value: serde_json::Value = resp.into_json().map_err(|e| LlmError::Backend {
            endpoint: self.endpoint.clone(),
            status: 0,
            message: format!("decoding response: {e}"),
        })?;

        let text = self.response_text(&value).unwrap_or_default();

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
/// `/api/chat` or `/api/generate` response into [`TokenLogprob`]s, or `None`
/// if absent.
///
/// Ollama exposes per-position logprobs under either a top-level `logprobs`
/// array or `message.logprobs` depending on version/endpoint; we try both.
/// `/api/generate` reports them under the top-level `logprobs` array, which the
/// first lookup covers.  Each position carries a `top_logprobs` list of
/// `{token, bytes, logprob}`.
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
        // Default API surface is Chat (backward compat).
        assert_eq!(c.api(), OllamaApi::Chat);
    }

    #[test]
    fn with_api_generate_switches_endpoint() {
        let c =
            OllamaClient::new("http://localhost:11434/", "llama3.1").with_api(OllamaApi::Generate);
        assert_eq!(c.endpoint(), "http://localhost:11434/api/generate");
        assert_eq!(c.api(), OllamaApi::Generate);
        // Switching back to Chat restores the chat endpoint.
        let c = c.with_api(OllamaApi::Chat);
        assert_eq!(c.endpoint(), "http://localhost:11434/api/chat");
        assert_eq!(c.api(), OllamaApi::Chat);
    }

    #[test]
    fn ollama_api_default_is_chat() {
        assert_eq!(OllamaApi::default(), OllamaApi::Chat);
    }

    #[test]
    fn chat_body_is_byte_unchanged() {
        // The Chat body must equal the historical `{model, stream, messages,
        // options}` shape exactly (no new fields), preserving every existing
        // request byte-for-byte.
        let c = OllamaClient::new("http://localhost:11434", "m");
        let cfg = LlmConfig::deterministic().with_seed(7);
        let body = c.build_body("hi", &cfg);
        let expected = json!({
            "model": "m",
            "stream": false,
            "messages": [{ "role": "user", "content": "hi" }],
            "options": { "temperature": 0.0, "seed": 7 },
        });
        assert_eq!(body, expected);
    }

    #[test]
    fn generate_body_matches_required_shape() {
        // CRITICAL fidelity test: with temperature=1.0, omit_seed, max_tokens=16,
        // system=Some, the Generate body must be byte-identical to sun2024's
        // current request (field presence + names), so swapping in socsim-llm
        // produces byte-identical generation. NOTE: no "seed" when omit_seed.
        let c = OllamaClient::new("http://localhost:11434", "m").with_api(OllamaApi::Generate);
        let cfg = LlmConfig::sampling(1.0) // temperature=1.0, omit_seed=true
            .with_max_tokens(16)
            .with_system("s")
            .allow_blank();
        let body = c.build_body("p", &cfg);
        let expected = json!({
            "model": "m",
            "system": "s",
            "prompt": "p",
            "stream": false,
            "options": { "temperature": 1.0, "num_predict": 16 },
        });
        assert_eq!(body, expected);
    }

    #[test]
    fn generate_body_omits_seed_when_omit_seed() {
        let c = OllamaClient::new("http://h", "m").with_api(OllamaApi::Generate);
        let cfg = LlmConfig::sampling(1.0); // omit_seed = true
        let body = c.build_body("p", &cfg);
        assert!(body["options"].get("seed").is_none(), "{body}");
        assert_eq!(body["options"]["temperature"], json!(1.0));
    }

    #[test]
    fn generate_body_includes_seed_when_not_omitted() {
        let c = OllamaClient::new("http://h", "m").with_api(OllamaApi::Generate);
        let cfg = LlmConfig::deterministic().with_seed(9);
        let body = c.build_body("p", &cfg);
        assert_eq!(body["options"]["seed"], json!(9));
    }

    #[test]
    fn generate_body_includes_system_only_when_some() {
        let c = OllamaClient::new("http://h", "m").with_api(OllamaApi::Generate);
        // No system → no top-level "system" field.
        let no_sys = c.build_body("p", &LlmConfig::deterministic());
        assert!(no_sys.get("system").is_none(), "{no_sys}");
        // System present → top-level string field.
        let with_sys = c.build_body("p", &LlmConfig::deterministic().with_system("you are X"));
        assert_eq!(with_sys["system"], json!("you are X"));
    }

    #[test]
    fn generate_response_text_parses_response_field() {
        let c = OllamaClient::new("http://h", "m").with_api(OllamaApi::Generate);
        let value: serde_json::Value =
            serde_json::from_str(r#"{"response": " Donald", "done": true}"#).unwrap();
        assert_eq!(c.response_text(&value).as_deref(), Some(" Donald"));
    }

    #[test]
    fn chat_response_text_parses_message_content() {
        let c = OllamaClient::new("http://h", "m"); // Chat
        let value: serde_json::Value =
            serde_json::from_str(r#"{"message": {"content": "hi"}}"#).unwrap();
        assert_eq!(c.response_text(&value).as_deref(), Some("hi"));
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

    #[test]
    fn timeout_defaults_to_none() {
        // Backward-compat: no timeout is set unless asked for.
        let c = OllamaClient::new("http://localhost:11434", "m");
        assert_eq!(c.timeout(), None);
        // from_env / with_api also leave the timeout unset.
        assert_eq!(OllamaClient::from_env().timeout(), None);
        assert_eq!(
            OllamaClient::new("http://h", "m")
                .with_api(OllamaApi::Generate)
                .timeout(),
            None
        );
    }

    #[test]
    fn with_timeout_sets_the_field() {
        let c =
            OllamaClient::new("http://h", "m").with_timeout(std::time::Duration::from_millis(1500));
        assert_eq!(c.timeout(), Some(std::time::Duration::from_millis(1500)));
    }

    #[test]
    fn with_timeout_secs_maps_to_duration() {
        let c = OllamaClient::new("http://h", "m").with_timeout_secs(30);
        assert_eq!(c.timeout(), Some(std::time::Duration::from_secs(30)));
    }

    #[test]
    fn with_api_preserves_timeout() {
        // Selecting an API surface must not drop a previously-set timeout.
        let c = OllamaClient::new("http://h", "m")
            .with_timeout_secs(5)
            .with_api(OllamaApi::Generate);
        assert_eq!(c.timeout(), Some(std::time::Duration::from_secs(5)));
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
