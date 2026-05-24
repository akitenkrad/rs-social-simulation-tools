//! A client that tries a primary backend first and falls back to a secondary.

use crate::client::{LlmClient, LlmConfig, LlmError, LlmResponse};

/// Tries `primary` first; on **any** error falls back to `secondary`.
///
/// The canonical socsim configuration is "Ollama first, OpenAI fallback", but
/// `FallbackClient` is generic over any two [`LlmClient`]s, so it is fully
/// testable offline by composing mock clients (see the crate tests).
pub struct FallbackClient<P: LlmClient, S: LlmClient> {
    primary: P,
    secondary: S,
}

impl<P: LlmClient, S: LlmClient> FallbackClient<P, S> {
    /// Build a fallback over `primary` then `secondary`.
    pub fn new(primary: P, secondary: S) -> Self {
        Self { primary, secondary }
    }
}

impl<P: LlmClient, S: LlmClient> LlmClient for FallbackClient<P, S> {
    fn model(&self) -> &str {
        // Report the primary's model; the metadata of each call records which
        // backend actually answered.
        self.primary.model()
    }

    fn endpoint(&self) -> &str {
        self.primary.endpoint()
    }

    fn complete(&self, prompt: &str, config: &LlmConfig) -> Result<LlmResponse, LlmError> {
        match self.primary.complete(prompt, config) {
            Ok(resp) => Ok(resp),
            Err(primary_err) => match self.secondary.complete(prompt, config) {
                Ok(resp) => Ok(resp),
                Err(secondary_err) => Err(LlmError::AllBackendsFailed(format!(
                    "primary ({}): {primary_err}; secondary ({}): {secondary_err}",
                    self.primary.endpoint(),
                    self.secondary.endpoint(),
                ))),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{AlwaysFailClient, ScriptedClient};

    #[test]
    fn uses_primary_when_it_succeeds() {
        let fb = FallbackClient::new(
            ScriptedClient::constant("primary", "from-primary"),
            ScriptedClient::constant("secondary", "from-secondary"),
        );
        let r = fb.complete("hi", &LlmConfig::deterministic()).unwrap();
        assert_eq!(r.text, "from-primary");
        assert_eq!(r.metadata.model, "primary");
    }

    #[test]
    fn falls_back_when_primary_fails() {
        let fb = FallbackClient::new(
            AlwaysFailClient::new("primary", "http://primary"),
            ScriptedClient::constant("secondary", "from-secondary"),
        );
        let r = fb.complete("hi", &LlmConfig::deterministic()).unwrap();
        assert_eq!(r.text, "from-secondary");
        assert_eq!(r.metadata.model, "secondary");
    }

    #[test]
    fn errors_when_both_fail() {
        let fb = FallbackClient::new(
            AlwaysFailClient::new("primary", "http://primary"),
            AlwaysFailClient::new("secondary", "http://secondary"),
        );
        let err = fb.complete("hi", &LlmConfig::deterministic()).unwrap_err();
        assert!(matches!(err, LlmError::AllBackendsFailed(_)));
    }
}
