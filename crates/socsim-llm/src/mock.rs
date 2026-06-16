//! Network-free [`LlmClient`](crate::LlmClient) implementations for tests and
//! offline development.

use crate::client::{CallMetadata, LlmClient, LlmConfig, LlmError, LlmResponse};

/// A deterministic, in-memory "model" that answers via a closure — no network.
///
/// Useful for unit tests and for running a network-based mechanism offline.
pub struct ScriptedClient {
    model: String,
    answer: Box<dyn Fn(&str) -> String + Send + Sync>,
}

impl ScriptedClient {
    /// Build a scripted client for `model` whose answer is `answer(prompt)`.
    pub fn new(
        model: impl Into<String>,
        answer: impl Fn(&str) -> String + Send + Sync + 'static,
    ) -> Self {
        Self {
            model: model.into(),
            answer: Box::new(answer),
        }
    }

    /// A scripted client that always returns the same `reply`.
    pub fn constant(model: impl Into<String>, reply: impl Into<String>) -> Self {
        let reply = reply.into();
        Self::new(model, move |_| reply.clone())
    }
}

impl LlmClient for ScriptedClient {
    fn model(&self) -> &str {
        &self.model
    }

    fn endpoint(&self) -> &str {
        "mock://scripted"
    }

    fn complete(&self, prompt: &str, config: &LlmConfig) -> Result<LlmResponse, LlmError> {
        Ok(LlmResponse {
            text: (self.answer)(prompt),
            metadata: CallMetadata {
                model: self.model.clone(),
                endpoint: self.endpoint().to_string(),
                temperature: config.temperature,
                seed: config.seed,
                cache_hit: false,
            },
            logprobs: None,
        })
    }
}

/// An [`LlmClient`] that always fails — used to exercise fallback selection.
pub struct AlwaysFailClient {
    model: String,
    endpoint: String,
}

impl AlwaysFailClient {
    /// Build a failing client identifying as `model` at `endpoint`.
    pub fn new(model: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            endpoint: endpoint.into(),
        }
    }
}

impl LlmClient for AlwaysFailClient {
    fn model(&self) -> &str {
        &self.model
    }

    fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn complete(&self, _prompt: &str, _config: &LlmConfig) -> Result<LlmResponse, LlmError> {
        Err(LlmError::Transport {
            endpoint: self.endpoint.clone(),
            message: "AlwaysFailClient always fails".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_echoes() {
        let c = ScriptedClient::new("m", |p| format!("echo:{p}"));
        let r = c.complete("hi", &LlmConfig::deterministic()).unwrap();
        assert_eq!(r.text, "echo:hi");
        assert_eq!(r.metadata.model, "m");
        assert!(!r.metadata.cache_hit);
    }

    #[test]
    fn always_fail_errors() {
        let c = AlwaysFailClient::new("m", "http://x");
        assert!(c.complete("hi", &LlmConfig::deterministic()).is_err());
    }
}
