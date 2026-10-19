use serde::{Deserialize, Serialize};
use socsim_core::AgentId;

use super::{BackendIdentity, ReplayMode, ResponseStore};
use crate::{ExperimentState, GameError, StageGame};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// Replay inputs embedded in experiment state to validate snapshot resumption.
pub struct LlmReplayManifest {
    /// Stable identifier declared by the response store.
    pub store_id: String,
    /// Digest of the store's immutable base records at open time.
    pub base_digest: String,
    /// Whether misses are forbidden or may be recorded.
    pub mode: ReplayMode,
    /// Model and endpoint identity expected for backend misses.
    pub backend: BackendIdentity,
}

impl LlmReplayManifest {
    /// Captures the identity and immutable base digest of an open store.
    pub fn for_store(store: &ResponseStore, backend: BackendIdentity) -> Self {
        Self {
            store_id: store.store_id().to_owned(),
            base_digest: store.base_digest().to_owned(),
            mode: store.mode(),
            backend,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
/// Experiment-state wrapper that persists an LLM replay manifest.
pub struct WithLlm<X> {
    /// User-defined experiment state and private views.
    pub inner: X,
    /// Replay contract checked when assembling or resuming LLM decisions.
    pub llm: LlmReplayManifest,
}

/// Gives the LLM driver access to the replay manifest stored in world state.
///
/// Experiment-state authors may implement this directly, though [`WithLlm`]
/// is the usual implementation. Assembly and snapshot verification call the
/// accessor before any backend work; it must return the manifest serialized
/// with the world rather than reconstructing current external state.
pub trait HasLlmManifest: ExperimentState {
    /// Borrows the persistent replay contract for this experiment.
    fn llm_manifest(&self) -> &LlmReplayManifest;
}

impl<X: ExperimentState> ExperimentState for WithLlm<X> {
    type PolicyState = X::PolicyState;
    type View<'a>
        = X::View<'a>
    where
        Self: 'a;

    fn view_for(&self, me: AgentId) -> Self::View<'_> {
        self.inner.view_for(me)
    }
}

impl<X: ExperimentState> HasLlmManifest for WithLlm<X> {
    fn llm_manifest(&self) -> &LlmReplayManifest {
        &self.llm
    }
}

pub(crate) fn verify_manifest(
    actual: &LlmReplayManifest,
    store: &ResponseStore,
    identity: &BackendIdentity,
) -> Result<(), GameError> {
    let expected = LlmReplayManifest::for_store(store, identity.clone());
    if actual == &expected {
        return Ok(());
    }
    Err(GameError::ManifestMismatch(format!(
        "LLM replay manifest differs: snapshot={actual:?}, driver={expected:?}"
    )))
}

#[cfg(feature = "engine")]
/// Verifies that a snapshot's replay contract matches a response store and backend.
///
/// # Errors
///
/// Returns [`GameError::ManifestMismatch`] if the store identifier, base
/// digest, replay mode, model, or endpoint differs.
pub fn verify_snapshot<G, X>(
    snapshot: &socsim_engine::Snapshot<crate::GameWorld<G, X>>,
    store: &ResponseStore,
    identity: &BackendIdentity,
) -> Result<(), GameError>
where
    G: StageGame,
    X: HasLlmManifest,
{
    verify_manifest(snapshot.world.x().llm_manifest(), store, identity)
}
