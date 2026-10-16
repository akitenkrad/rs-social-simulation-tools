use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "support/viz_ipd.rs"]
mod viz_ipd;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const REGEN_ENV: &str = "SOCSIM_GAMES_REGEN_FIXTURES";
const FILES: [&str; 7] = [
    "game_spec.json",
    "seed_0.jsonl",
    "seed_1.jsonl",
    "seed_2.jsonl",
    "seed_3.jsonl",
    "seed_4.jsonl",
    "solutions.json",
];

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/viz/ipd")
}

fn file_names(directory: &Path) -> TestResult<Vec<String>> {
    let mut names = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    Ok(names)
}

#[test]
fn ipd_records_match_fixtures_byte_for_byte() -> TestResult {
    let generated = tempfile::tempdir()?;
    viz_ipd::write_output(generated.path())?;
    assert_eq!(file_names(generated.path())?, FILES);

    let fixtures = fixture_dir();
    if std::env::var(REGEN_ENV).as_deref() == Ok("1") {
        fs::create_dir_all(&fixtures)?;
        for name in FILES {
            fs::write(fixtures.join(name), fs::read(generated.path().join(name))?)?;
        }
    } else {
        assert_eq!(file_names(&fixtures)?, FILES);
        for name in FILES {
            assert_eq!(
                fs::read(generated.path().join(name))?,
                fs::read(fixtures.join(name))?,
                "fixture differs for {name}"
            );
        }
    }

    assert_eq!(
        fs::read(generated.path().join("game_spec.json"))?,
        fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/viz/prisoners_dilemma/game_spec.json")
        )?,
        "IPD game spec differs from the prisoner's dilemma fixture"
    );
    Ok(())
}
