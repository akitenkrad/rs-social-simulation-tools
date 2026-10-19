use std::collections::BTreeMap;
use std::sync::Arc;

use crate::{
    DecisionRule, ExperimentState, RuleBook, RuleError, RuleInitContext, RuleKindId, StageGame,
};

const RULE_DRIVER_NAME: &str = "game_decide";

enum CatalogRule<G: StageGame, X: ExperimentState> {
    Rule(Arc<dyn DecisionRule<G, X>>),
    #[cfg(feature = "llm")]
    Llm(Arc<dyn crate::llm::LlmDecisionRule<G, X>>),
}

struct CatalogEntry<G: StageGame, X: ExperimentState> {
    driver_name: &'static str,
    rule: CatalogRule<G, X>,
}

/// Canonical registry of policy kinds, their drivers, and their state initializers.
pub struct KindCatalog<G: StageGame, X: ExperimentState> {
    entries: BTreeMap<String, CatalogEntry<G, X>>,
}

impl<G: StageGame, X: ExperimentState> Default for KindCatalog<G, X> {
    fn default() -> Self {
        Self::new()
    }
}

impl<G: StageGame, X: ExperimentState> KindCatalog<G, X> {
    /// Creates an empty combined native/LLM kind catalog.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Registers every kind in `rules` as owned by the `game_decide` driver.
    pub fn from_rules(rules: &RuleBook<G, X>) -> Self {
        let entries = rules
            .rules
            .iter()
            .map(|(kind, rule)| {
                (
                    kind.clone(),
                    CatalogEntry {
                        driver_name: RULE_DRIVER_NAME,
                        rule: CatalogRule::Rule(Arc::clone(rule)),
                    },
                )
            })
            .collect();
        Self { entries }
    }

    /// Adds every kind in `rules` as owned by the `game_decide` driver.
    pub fn with_rules(mut self, rules: &RuleBook<G, X>) -> Result<Self, RuleError> {
        for (kind, rule) in &rules.rules {
            if self.entries.contains_key(kind) {
                return Err(RuleError::DuplicateKind(kind.clone()));
            }
            self.entries.insert(
                kind.clone(),
                CatalogEntry {
                    driver_name: RULE_DRIVER_NAME,
                    rule: CatalogRule::Rule(Arc::clone(rule)),
                },
            );
        }
        Ok(self)
    }

    /// Adds every kind in `rules` as owned by the `game_llm_decide` driver.
    #[cfg(feature = "llm")]
    pub fn with_llm(mut self, rules: &crate::llm::LlmRuleBook<G, X>) -> Result<Self, RuleError> {
        for (kind, rule) in &rules.rules {
            if self.entries.contains_key(kind) {
                return Err(RuleError::DuplicateKind(kind.clone()));
            }
            self.entries.insert(
                kind.clone(),
                CatalogEntry {
                    driver_name: "game_llm_decide",
                    rule: CatalogRule::Llm(Arc::clone(rule)),
                },
            );
        }
        Ok(self)
    }

    /// Returns kind names in lexicographic order, which is also kind-ID order.
    pub fn kinds(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.keys().map(String::as_str)
    }

    /// Returns the interned ID of `kind`, or `None` when it is not registered.
    pub fn kind_id(&self, kind: &str) -> Option<RuleKindId> {
        self.entries
            .keys()
            .position(|name| name == kind)
            .and_then(|index| u32::try_from(index).ok())
            .map(RuleKindId)
    }

    /// Returns the mechanism name responsible for `kind`.
    pub fn driver_name(&self, kind: &str) -> Option<&'static str> {
        self.entries.get(kind).map(|entry| entry.driver_name)
    }

    pub(crate) fn init_state(
        &self,
        kind: &str,
        context: &RuleInitContext,
        game: &G,
    ) -> Result<X::PolicyState, RuleError> {
        let entry = self
            .entries
            .get(kind)
            .ok_or_else(|| RuleError::unknown_kind(kind, self.kinds()))?;
        match &entry.rule {
            CatalogRule::Rule(rule) => rule.init_state(context, game),
            #[cfg(feature = "llm")]
            CatalogRule::Llm(rule) => rule.init_state(context, game),
        }
    }

    pub(crate) fn needs_pair_history(&self, kind: &str) -> Result<bool, RuleError> {
        let entry = self
            .entries
            .get(kind)
            .ok_or_else(|| RuleError::unknown_kind(kind, self.kinds()))?;
        Ok(match &entry.rule {
            CatalogRule::Rule(rule) => rule.needs_pair_history(),
            #[cfg(feature = "llm")]
            CatalogRule::Llm(_) => false,
        })
    }
}
