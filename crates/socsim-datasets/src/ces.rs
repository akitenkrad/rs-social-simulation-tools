//! CES 2022 (gong2026) — metadata-only stub.
//!
//! The Cooperative Election Study (CES) 2022 schema is **not yet shipped**: the
//! real CES V-variable column names and value codes have not been obtained from
//! the codebook, and fabricating them would silently corrupt any downstream
//! recode. So this module deliberately ships **no schema builder and no column
//! codes** — only the provenance metadata in [`CES_2022_META`].
//!
//! The CES 2022 Common Content is distributed via the Harvard Dataverse, where
//! download requires accepting the dataset's terms; we therefore declare the
//! single common-content file as [`Source::Manual`] pointing at the dataset
//! landing page rather than auto-downloading it. Once gong2026 supplies the real
//! column names, add an `anes`-style `ces_2022()` schema builder here and (if a
//! direct Dataverse fileID + checksum become known) promote the file source to
//! [`Source::Dataverse`].

use crate::registry::{DataFile, DatasetMeta, Source};

const CES_2022_FILES: &[DataFile] = &[DataFile {
    // The CES 2022 Common Content release file (name as published on Dataverse).
    logical_name: "ces_2022_common.csv",
    source: Source::Manual {
        instructions_url: "https://doi.org/10.7910/DVN/PR4L8P",
    },
    sha256: None,
    expect_rows: None,
}];

/// Provenance/acquisition metadata for the CES 2022 Common Content.
///
/// Metadata only — there is no schema builder yet (see the module docs).
pub const CES_2022_META: DatasetMeta = DatasetMeta {
    key: "ces-2022",
    name: "CES 2022 Common Content",
    doi: Some("10.7910/DVN/PR4L8P"),
    source_url: "https://doi.org/10.7910/DVN/PR4L8P",
    citation: "Schaffner, Brian; Ansolabehere, Stephen; Shih, Marissa. \
               Cooperative Election Study Common Content, 2022. Harvard Dataverse.",
    license: "Distributed via the Harvard Dataverse; download requires accepting \
              the dataset's terms of use.",
    files: CES_2022_FILES,
};

// TODO(gong2026): real column names. Add a `ces_2022() -> SurveySchema` builder
// here (mirroring `crate::anes`) once the CES 2022 codebook V-variable column
// names and value codes are available. Until then, no schema is exposed so no
// fabricated codes can leak downstream.
