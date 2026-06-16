//! [`SurveySchema`] and its building blocks: per-variable column names, value
//! maps, the age-binning rule, the outcome map, and the generic recode that
//! turns one raw [`Record`] into a [`RecodedRow`].

use std::collections::BTreeMap;
use std::collections::HashMap;

use crate::{raw_code, Record};

/// A value-code -> canonical-label map for one demographic variable.
///
/// Codes are the ANES/CES integer codes; labels are the canonical category
/// strings (the same label space the downstream prompt templates use). A code
/// absent from the map (or any negative/missing code) recodes to `None`.
#[derive(Debug, Clone, Default)]
pub struct ValMap {
    map: BTreeMap<i64, &'static str>,
}

impl ValMap {
    /// Build a value map from `(code, label)` pairs.
    pub fn new(pairs: &[(i64, &'static str)]) -> Self {
        ValMap {
            map: pairs.iter().copied().collect(),
        }
    }

    /// Look up the canonical label for a raw code (`None` if unmapped).
    pub fn label(&self, code: i64) -> Option<&'static str> {
        self.map.get(&code).copied()
    }
}

/// How a continuous age column is folded into a categorical age bin.
///
/// Each entry is an inclusive `[lo, hi]` range mapped to a label; the first
/// matching range wins (ranges are tested in declaration order). Codes outside
/// every range recode to `None`. This ports sun2024's `age_bin` exactly when
/// constructed via [`AgeBins::anes_decade`].
#[derive(Debug, Clone, Default)]
pub struct AgeBins {
    bins: Vec<(i64, i64, &'static str)>,
}

impl AgeBins {
    /// Build an age-binning rule from `(lo, hi, label)` inclusive ranges.
    pub fn new(bins: &[(i64, i64, &'static str)]) -> Self {
        AgeBins {
            bins: bins.to_vec(),
        }
    }

    /// The exact decade bins sun2024 uses (`age_bin`): 18-29, 30-39, 40-49,
    /// 50-59, 60-69, 70+ (70..=120). Ages outside `[18, 120]` recode to `None`.
    pub fn anes_decade() -> Self {
        AgeBins::new(&[
            (18, 29, "18-29"),
            (30, 39, "30-39"),
            (40, 49, "40-49"),
            (50, 59, "50-59"),
            (60, 69, "60-69"),
            (70, 120, "70+"),
        ])
    }

    /// Fold a raw age code into its bin label (`None` if outside every range).
    pub fn bin(&self, code: i64) -> Option<&'static str> {
        self.bins
            .iter()
            .find(|(lo, hi, _)| code >= *lo && code <= *hi)
            .map(|(_, _, label)| *label)
    }
}

/// One demographic variable in a schema: how to read it and how to recode it.
#[derive(Debug, Clone)]
pub struct DemoVar {
    /// Stable snake_case key (column key, map key, sampling/scan order key).
    pub key: &'static str,
    /// Raw CSV column name for this variable in the target survey-year.
    pub column: String,
    /// How the raw code becomes a category label.
    pub recode: Recode,
}

/// The recode strategy for a [`DemoVar`].
#[derive(Debug, Clone)]
pub enum Recode {
    /// Discrete code -> label map (race, gender, ideology, party, ...).
    ValMap(ValMap),
    /// Continuous code folded into age bins.
    Age(AgeBins),
}

impl DemoVar {
    /// A value-mapped demographic variable.
    pub fn valmap(key: &'static str, column: impl Into<String>, map: ValMap) -> Self {
        DemoVar {
            key,
            column: column.into(),
            recode: Recode::ValMap(map),
        }
    }

    /// An age-binned demographic variable.
    pub fn age(key: &'static str, column: impl Into<String>, bins: AgeBins) -> Self {
        DemoVar {
            key,
            column: column.into(),
            recode: Recode::Age(bins),
        }
    }

    /// Recode a raw record for this variable (`None` if missing/unmapped).
    pub fn recode_label(&self, rec: &Record) -> Option<String> {
        let code = raw_code(rec, &self.column)?;
        let label = match &self.recode {
            Recode::ValMap(m) => m.label(code),
            Recode::Age(b) => b.bin(code),
        }?;
        Some(label.to_string())
    }
}

/// Outcome (vote) recode: raw column + code -> outcome-label map.
///
/// Ports sun2024's `actual_vote`: in ANES, code `1` is the Democratic slot
/// (Biden/Clinton/Obama) and code `2` is the Republican slot (Trump/Romney);
/// any other code (third party, missing) is `None`. The labels are
/// caller-supplied so a schema can name its own outcome categories.
#[derive(Debug, Clone)]
pub struct OutcomeMap {
    /// Raw CSV column holding the outcome code.
    pub column: String,
    map: BTreeMap<i64, &'static str>,
}

impl OutcomeMap {
    /// Build an outcome map from a column and `(code, label)` pairs.
    pub fn new(column: impl Into<String>, pairs: &[(i64, &'static str)]) -> Self {
        OutcomeMap {
            column: column.into(),
            map: pairs.iter().copied().collect(),
        }
    }

    /// Recode the outcome for a raw record (`None` if missing/unmapped).
    pub fn outcome(&self, rec: &Record) -> Option<&'static str> {
        let code = raw_code(rec, &self.column)?;
        self.map.get(&code).copied()
    }
}

/// A full survey-year schema: the variable set + outcome recode.
///
/// This is the config that replaces sun2024's per-year match arms. Build one
/// with [`SurveySchema::builder`] (or use the built-in [`crate::anes`]
/// schemas). The variable *set* is whatever the schema declares, so newer
/// surveys (CES) extend by declaring their own [`DemoVar`]s.
#[derive(Debug, Clone)]
pub struct SurveySchema {
    /// Human label for the survey-year (e.g. `"ANES 2020"`).
    pub name: String,
    /// Demographic variables, in scan order (distribution/sampling order).
    pub vars: Vec<DemoVar>,
    /// Outcome (vote) recode.
    pub outcome: OutcomeMap,
}

impl SurveySchema {
    /// Start a [`SurveySchemaBuilder`].
    ///
    /// # Example: declaring a new survey schema (CES skeleton shape)
    ///
    /// ```
    /// use socsim_survey::{SurveySchema, DemoVar, ValMap, AgeBins, OutcomeMap};
    ///
    /// // TODO(gong2026): replace these placeholder column names + codes with
    /// // the real CES 2022 V-variables once the CES data is wired.
    /// let ces = SurveySchema::builder("CES 2022")
    ///     .var(DemoVar::valmap(
    ///         "gender",
    ///         "ces_gender_col", // <- real CES column name goes here
    ///         ValMap::new(&[(1, "man"), (2, "woman")]),
    ///     ))
    ///     .var(DemoVar::age("age", "ces_age_col", AgeBins::anes_decade()))
    ///     .outcome(OutcomeMap::new(
    ///         "ces_vote_col",
    ///         &[(1, "Democrat"), (2, "Republican")],
    ///     ))
    ///     .build();
    /// assert_eq!(ces.name, "CES 2022");
    /// ```
    pub fn builder(name: impl Into<String>) -> SurveySchemaBuilder {
        SurveySchemaBuilder {
            name: name.into(),
            vars: Vec::new(),
            outcome: None,
        }
    }

    /// The demographic variable with key `key`, if present.
    pub fn var(&self, key: &str) -> Option<&DemoVar> {
        self.vars.iter().find(|v| v.key == key)
    }

    /// Stable scan-order list of variable keys.
    pub fn var_keys(&self) -> Vec<&'static str> {
        self.vars.iter().map(|v| v.key).collect()
    }
}

/// Builder for [`SurveySchema`].
#[derive(Debug, Clone)]
pub struct SurveySchemaBuilder {
    name: String,
    vars: Vec<DemoVar>,
    outcome: Option<OutcomeMap>,
}

impl SurveySchemaBuilder {
    /// Add one demographic variable (declaration order is scan order).
    pub fn var(mut self, v: DemoVar) -> Self {
        self.vars.push(v);
        self
    }

    /// Set the outcome (vote) recode.
    pub fn outcome(mut self, o: OutcomeMap) -> Self {
        self.outcome = Some(o);
        self
    }

    /// Finish the schema.
    ///
    /// # Panics
    ///
    /// Panics if no outcome map was set.
    pub fn build(self) -> SurveySchema {
        SurveySchema {
            name: self.name,
            vars: self.vars,
            outcome: self
                .outcome
                .expect("SurveySchema requires an outcome map (call .outcome(...))"),
        }
    }
}

/// One respondent's recoded demographics (variable key -> category label).
///
/// Missing/unmapped variables are simply absent from the map (matching
/// sun2024's `RecodedRow`, where the distribution estimator normalizes each
/// variable over its own non-missing sample).
#[derive(Debug, Clone, Default)]
pub struct RecodedRow {
    /// Variable key (e.g. `"race"`) -> canonical category label.
    pub attrs: HashMap<String, String>,
}

impl RecodedRow {
    /// Whether every variable in `schema` is present (non-missing) for this row.
    pub fn is_complete(&self, schema: &SurveySchema) -> bool {
        schema.vars.iter().all(|v| self.attrs.contains_key(v.key))
    }
}

/// Recode one demographic variable from a raw record (`None` if the schema has
/// no such variable, or the value is missing/unmapped).
///
/// Generic port of sun2024's `demo_label`.
pub fn demo_label(rec: &Record, schema: &SurveySchema, var_key: &str) -> Option<String> {
    schema.var(var_key)?.recode_label(rec)
}

/// Recode one raw record into a [`RecodedRow`] (missing variables omitted).
///
/// Generic port of sun2024's `recode_row`.
pub fn recode_row(rec: &Record, schema: &SurveySchema) -> RecodedRow {
    let mut attrs = HashMap::new();
    for v in &schema.vars {
        if let Some(label) = v.recode_label(rec) {
            attrs.insert(v.key.to_string(), label);
        }
    }
    RecodedRow { attrs }
}

/// Recode the outcome (vote) for one raw record (`None` if missing/unmapped).
///
/// Generic port of sun2024's `actual_vote`.
pub fn actual_outcome(rec: &Record, schema: &SurveySchema) -> Option<&'static str> {
    schema.outcome.outcome(rec)
}
