//! Registry + `Source::download_url` behaviour.

use socsim_datasets::anes;
use socsim_datasets::registry::Source;

#[test]
fn anes_meta_lookup() {
    let m = anes::meta(2020).expect("2020 meta present");
    assert_eq!(m.key, "anes-2020");
    assert_eq!(m.name, "ANES 2020 Time Series Study");
    // meta(2020) returns the ANES_2020 const's data.
    assert_eq!(m.key, anes::ANES_2020.key);
    assert_eq!(m.files.len(), anes::ANES_2020.files.len());
    // Unsupported year -> None.
    assert!(anes::meta(1999).is_none());
}

#[test]
fn anes_meta_identity() {
    // meta(year) returns the corresponding const.
    assert_eq!(anes::meta(2012).unwrap().key, anes::ANES_2012.key);
    assert_eq!(anes::meta(2016).unwrap().key, anes::ANES_2016.key);
    assert_eq!(anes::meta(2020).unwrap().key, anes::ANES_2020.key);
}

#[test]
fn dataverse_download_url() {
    let s = Source::Dataverse {
        base: "https://dataverse.harvard.edu",
        file_id: 6711704,
    };
    assert_eq!(
        s.download_url().as_deref(),
        Some("https://dataverse.harvard.edu/api/access/datafile/6711704?format=original")
    );
}

#[test]
fn url_download_url() {
    let s = Source::Url {
        url: "https://example.org/data.csv",
    };
    assert_eq!(
        s.download_url().as_deref(),
        Some("https://example.org/data.csv")
    );
}

#[test]
fn manual_download_url_is_none() {
    let s = Source::Manual {
        instructions_url: "https://electionstudies.org/data-center/",
    };
    assert!(s.download_url().is_none());
}

#[test]
fn all_has_four_entries() {
    assert_eq!(socsim_datasets::all().len(), 4);
}

#[test]
fn by_key_anes_2020() {
    let m = socsim_datasets::by_key("anes-2020").expect("anes-2020 present");
    assert_eq!(m.key, anes::ANES_2020.key);
    assert_eq!(m.name, anes::ANES_2020.name);
}

#[test]
fn by_key_unknown_is_none() {
    assert!(socsim_datasets::by_key("nope").is_none());
}
