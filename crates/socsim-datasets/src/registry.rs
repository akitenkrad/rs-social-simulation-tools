//! Machine-readable dataset metadata + acquisition registry.
//!
//! A [`DatasetMeta`] captures the citation/provenance side of a survey dataset
//! (DOI, source URL, citation, license) plus the list of [`DataFile`]s that make
//! it up. Each file records *where* it comes from ([`Source`]) and *how* to
//! verify it (`sha256` / `expect_rows`). This promotes the per-paper download
//! scripts (e.g. argyle2023's hard-coded Dataverse fileIDs) into a single,
//! machine-readable source of truth.
//!
//! All string fields are `&'static str` so registry entries can be declared as
//! `const`s. No raw data is ever stored here — only the metadata needed to fetch
//! and validate it.

/// Provenance + file list for one survey dataset.
#[derive(Debug, Clone)]
pub struct DatasetMeta {
    /// Stable key, e.g. `"anes-2020"`.
    pub key: &'static str,
    /// Human-readable name, e.g. `"ANES 2020 Time Series"`.
    pub name: &'static str,
    /// DOI if one is known (e.g. `"10.3886/ICPSR..."`), else `None`.
    pub doi: Option<&'static str>,
    /// Landing page for the dataset (Dataverse / ICPSR / publisher).
    pub source_url: &'static str,
    /// Recommended citation string.
    pub citation: &'static str,
    /// License / access note (e.g. whether an account or data-use agreement is
    /// required to obtain the files).
    pub license: &'static str,
    /// The files that make up this dataset.
    pub files: &'static [DataFile],
}

/// One downloadable (or manually-obtained) file within a [`DatasetMeta`].
#[derive(Debug, Clone)]
pub struct DataFile {
    /// Logical (post-conversion) name to store the file under, e.g.
    /// `"anes_2020.csv"`.
    pub logical_name: &'static str,
    /// Where the file comes from.
    pub source: Source,
    /// Expected SHA-256 (lowercase hex) of the stored file, when known. Used to
    /// verify downloads and detect cache hits.
    pub sha256: Option<&'static str>,
    /// Expected number of data rows (respondents), when known. Inherited from
    /// the existing `--expect-rows` checks; only meaningful for the CSV form.
    pub expect_rows: Option<usize>,
}

/// How a [`DataFile`] is obtained.
#[derive(Debug, Clone)]
pub enum Source {
    /// A Harvard-Dataverse-style access endpoint: the file is fetched from
    /// `{base}/api/access/datafile/{file_id}?format=original`.
    Dataverse {
        /// Dataverse access base, e.g.
        /// `"https://dataverse.harvard.edu"`.
        base: &'static str,
        /// Numeric Dataverse file id.
        file_id: u64,
    },
    /// A plain direct-download URL.
    Url {
        /// The full URL to GET.
        url: &'static str,
    },
    /// Not auto-downloadable (license-gated): the consumer must obtain the file
    /// manually by following the instructions URL.
    Manual {
        /// Where to go to obtain the file (account / data-use agreement, etc.).
        instructions_url: &'static str,
    },
}

impl Source {
    /// The URL to GET for this source, or `None` for [`Source::Manual`].
    ///
    /// For [`Source::Dataverse`] this builds the original-format access URL
    /// `{base}/api/access/datafile/{file_id}?format=original`.
    pub fn download_url(&self) -> Option<String> {
        match self {
            Source::Dataverse { base, file_id } => Some(format!(
                "{base}/api/access/datafile/{file_id}?format=original"
            )),
            Source::Url { url } => Some((*url).to_string()),
            Source::Manual { .. } => None,
        }
    }
}
