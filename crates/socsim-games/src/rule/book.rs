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

    /// Registers one shared rule under an explicit `kind`.
    ///
    /// Use [`add`](Self::add) when one rule corresponds to one kind and its
    /// [`DecisionRule::name`] is the desired kind name. Use `register` when
    /// the kind needs a distinct label, for example when registering two
    /// differently parameterized instances of the same rule type.
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

    /// Registers one shared rule under its [`DecisionRule::name`].
    ///
    /// This is the convenient form when one rule corresponds to one kind. Use
    /// [`register`](Self::register) with distinct kind names when, for example,
    /// two differently parameterized instances of the same rule type must be
    /// available in the same rule book.
    ///
    /// # Errors
    ///
    /// Returns [`RuleError::DuplicateKind`] when the rule's name is already
    /// present.
    pub fn add(&mut self, rule: Arc<dyn DecisionRule<G, X>>) -> Result<(), RuleError> {
        let kind = rule.name().to_owned();
        self.register(&kind, rule)
    }

    pub(crate) fn get(&self, kind: &str) -> Result<&Arc<dyn DecisionRule<G, X>>, RuleError> {
        self.rules
            .get(kind)
            .ok_or_else(|| RuleError::unknown_kind(kind, self.rules.keys().map(String::as_str)))
    }

    pub(crate) fn get_optional(&self, kind: &str) -> Option<&Arc<dyn DecisionRule<G, X>>> {
        self.rules.get(kind)
    }
}
