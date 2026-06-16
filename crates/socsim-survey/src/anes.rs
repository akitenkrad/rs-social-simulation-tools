//! Built-in ANES 2012 / 2016 / 2020 [`SurveySchema`]s.
//!
//! These port the **exact** V-variable column names and value maps from
//! sun2024's `common/anes.rs` (`year_schema` + the `*_label` valmaps +
//! `age_bin` + `actual_vote`). Mappings are reproduced verbatim so a later
//! sun2024 migration onto this crate is bit-parity; do **not** change any code
//! or label here.
//!
//! The eight ANES demographic variables are exposed as [`DemoVar`] builders
//! ([`race`], [`gender`], [`age`], [`ideology`], [`party_id`],
//! [`political_interest`], [`church_attendance`], [`discuss_politics`]) that
//! take the per-year column name, so the year schemas differ only in their
//! column strings. The shared valmaps live in the `*_valmap` helpers.
//!
//! The two outcome labels are `"Biden"` (Democratic slot, code 1) and
//! `"Trump"` (Republican slot, code 2), matching sun2024's `Vote` enum.
//!
//! # CES 2022 (not yet shipped)
//!
//! No CES schema lives here: the CES 2022 V-variable column names and codes are
//! not available and must not be fabricated. Declare a CES schema the same way
//! these ANES builders do (see [`crate::SurveySchema::builder`] for a worked
//! `// TODO(gong2026)` skeleton); CES is complete once gong2026 wires its data.

use crate::schema::{AgeBins, DemoVar, OutcomeMap, SurveySchema, ValMap};

/// The Democratic-slot outcome label (ANES vote code 1).
pub const OUTCOME_DEM: &str = "Biden";
/// The Republican-slot outcome label (ANES vote code 2).
pub const OUTCOME_REP: &str = "Trump";

// ---------------------------------------------------------------------------
// Shared value maps (verbatim from sun2024 `*_label` functions).
// ---------------------------------------------------------------------------

fn race_valmap() -> ValMap {
    ValMap::new(&[
        (1, "white"),
        (2, "black"),
        (3, "asian"),
        (4, "native American"),
        (5, "hispanic"),
    ])
}

fn gender_valmap() -> ValMap {
    ValMap::new(&[(1, "man"), (2, "woman")])
}

fn ideology_valmap() -> ValMap {
    ValMap::new(&[
        (1, "extremely liberal"),
        (2, "liberal"),
        (3, "slightly liberal"),
        (4, "moderate"),
        (5, "slightly conservative"),
        (6, "conservative"),
        (7, "extremely conservative"),
    ])
}

fn party_valmap() -> ValMap {
    ValMap::new(&[
        (1, "a strong democrat"),
        (2, "a weak Democrat"),
        (3, "an independent who leans Democratic"),
        (4, "an independent"),
        (5, "an independent who leans Republican"),
        (6, "a weak Republican"),
        (7, "a strong Republican"),
    ])
}

fn interest_valmap() -> ValMap {
    ValMap::new(&[
        (1, "very"),
        (2, "somewhat"),
        (3, "not very"),
        (4, "not at all"),
    ])
}

fn church_valmap() -> ValMap {
    ValMap::new(&[(1, "attend church"), (2, "do not attend church")])
}

fn discuss_valmap() -> ValMap {
    ValMap::new(&[
        (1, "I like to discuss politics with my family and friends."),
        (2, "I never discuss politics with my family or friends."),
    ])
}

// ---------------------------------------------------------------------------
// Per-variable DemoVar builders (column name supplied per-year).
// ---------------------------------------------------------------------------

/// `race` variable for the given raw column.
pub fn race(column: &str) -> DemoVar {
    DemoVar::valmap("race", column, race_valmap())
}
/// `gender` variable for the given raw column.
pub fn gender(column: &str) -> DemoVar {
    DemoVar::valmap("gender", column, gender_valmap())
}
/// `age` variable (decade bins) for the given raw column.
pub fn age(column: &str) -> DemoVar {
    DemoVar::age("age", column, AgeBins::anes_decade())
}
/// `ideology` variable for the given raw column.
pub fn ideology(column: &str) -> DemoVar {
    DemoVar::valmap("ideology", column, ideology_valmap())
}
/// `party_id` variable for the given raw column.
pub fn party_id(column: &str) -> DemoVar {
    DemoVar::valmap("party_id", column, party_valmap())
}
/// `political_interest` variable for the given raw column.
pub fn political_interest(column: &str) -> DemoVar {
    DemoVar::valmap("political_interest", column, interest_valmap())
}
/// `church_attendance` variable for the given raw column.
pub fn church_attendance(column: &str) -> DemoVar {
    DemoVar::valmap("church_attendance", column, church_valmap())
}
/// `discuss_politics` variable for the given raw column.
pub fn discuss_politics(column: &str) -> DemoVar {
    DemoVar::valmap("discuss_politics", column, discuss_valmap())
}

fn outcome_for(column: &str) -> OutcomeMap {
    OutcomeMap::new(column, &[(1, OUTCOME_DEM), (2, OUTCOME_REP)])
}

// ---------------------------------------------------------------------------
// Year schemas (column names verbatim from sun2024 `year_schema`).
// ---------------------------------------------------------------------------

/// ANES 2012 schema (column names from sun2024 `year_schema(2012)`).
pub fn anes_2012() -> SurveySchema {
    SurveySchema::builder("ANES 2012")
        .var(race("dem_raceeth_x"))
        .var(gender("gender_respondent_x"))
        .var(age("dem_age_r_x"))
        .var(ideology("libcpre_self"))
        .var(party_id("pid_x"))
        .var(political_interest("paprofile_interestpolit"))
        .var(church_attendance("relig_church"))
        .var(discuss_politics("discuss_disc"))
        .outcome(outcome_for("presvote2012_x"))
        .build()
}

/// ANES 2016 schema (column names from sun2024 `year_schema(2016)`).
pub fn anes_2016() -> SurveySchema {
    SurveySchema::builder("ANES 2016")
        .var(race("V161310x"))
        .var(gender("V161342"))
        .var(age("V161267"))
        .var(ideology("V161126"))
        .var(party_id("V161158x"))
        .var(political_interest("V162256"))
        .var(church_attendance("V161244"))
        .var(discuss_politics("V162174"))
        .outcome(outcome_for("V162062x"))
        .build()
}

/// ANES 2020 schema (column names from sun2024 `year_schema(2020)`).
pub fn anes_2020() -> SurveySchema {
    SurveySchema::builder("ANES 2020")
        .var(race("V201549x"))
        .var(gender("V201600"))
        .var(age("V201507x"))
        .var(ideology("V201200"))
        .var(party_id("V201231x"))
        .var(political_interest("V202406"))
        .var(church_attendance("V201452"))
        .var(discuss_politics("V202022"))
        .outcome(outcome_for("V202110x"))
        .build()
}

/// Built-in ANES schema for a supported year (2012 / 2016 / 2020).
pub fn anes(year: u16) -> Option<SurveySchema> {
    match year {
        2012 => Some(anes_2012()),
        2016 => Some(anes_2016()),
        2020 => Some(anes_2020()),
        _ => None,
    }
}
