use std::collections::BTreeMap;
use std::sync::Arc;

use crate::{DecisionRule, ExperimentState, RuleError, StageGame};

/// Native decision rules indexed by canonical policy-kind name.
///
/// Names are stored in lexicographic order and become [`crate::RuleKindId`]
/// order when a world is built.
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
    /// Creates an empty native rule registry.
    pub fn new() -> Self {
        Self {
            rules: BTreeMap::new(),
        }
    }

    /// Registers one shared rule under `kind`.
    ///
    /// # Errors
    ///
    /// Returns [`RuleError::DuplicateKind`] when the name is already present.
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

    pub(crate) fn get_optional(&self, kind: &str) -> Option<&Arc<dyn DecisionRule<G, X>>> {
        self.rules.get(kind)
    }
}
