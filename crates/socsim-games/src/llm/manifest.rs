use serde::{Deserialize, Serialize};
use socsim_core::AgentId;

use super::{BackendIdentity, ReplayMode, ResponseStore};
use crate::{ExperimentState, GameError, StageGame};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LlmReplayManifest {
    pub store_id: String,
    pub base_digest: String,
    pub mode: ReplayMode,
    pub backend: BackendIdentity,
}

impl LlmReplayManifest {
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
pub struct WithLlm<X> {
    pub inner: X,
    pub llm: LlmReplayManifest,
}

pub trait HasLlmManifest: ExperimentState {
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
