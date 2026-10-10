use std::collections::BTreeMap;
use std::sync::Arc;

use crate::{DecisionRule, ExperimentState, RuleError, RuleKindId, StageGame};

pub struct RuleBook<G: StageGame, X: ExperimentState> {
    pub(crate) rules: BTreeMap<String, Arc<dyn DecisionRule<G, X>>>,
}

impl<G: StageGame, X: ExperimentState> Clone for RuleBook<G, X> {
    fn clone(&self) -> Self {
        Self {
            rules: self.rules.clone(),
        }
    }
}

impl<G: StageGame, X: ExperimentState> Default for RuleBook<G, X> {
    fn default() -> Self {
        Self::new()
    }
}

impl<G: StageGame, X: ExperimentState> RuleBook<G, X> {
    pub fn new() -> Self {
        Self {
            rules: BTreeMap::new(),
        }
    }

    pub fn register(
        &mut self,
        kind: &str,
        rule: Arc<dyn DecisionRule<G, X>>,
    ) -> Result<(), RuleError> {
        if self.rules.contains_key(kind) {
            return Err(RuleError::DuplicateKind(kind.to_owned()));
        }
        self.rules.insert(kind.to_owned(), rule);
        Ok(())
    }

    pub(crate) fn get(&self, kind: &str) -> Result<&Arc<dyn DecisionRule<G, X>>, RuleError> {
        self.rules
            .get(kind)
            .ok_or_else(|| RuleError::UnknownKind(kind.to_owned()))
    }

    pub(crate) fn kinds(&self) -> Vec<String> {
        self.rules.keys().cloned().collect()
    }

    pub(crate) fn kind_id(&self, kind: &str) -> Result<RuleKindId, RuleError> {
        self.rules
            .keys()
            .position(|name| name == kind)
            .and_then(|index| u32::try_from(index).ok())
            .map(RuleKindId)
            .ok_or_else(|| RuleError::UnknownKind(kind.to_owned()))
    }
}
