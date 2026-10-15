use std::error::Error;
use std::fs;
use std::io::{Error as IoError, ErrorKind};
use std::path::PathBuf;

use socsim_games::{write_game_spec, write_solutions};

#[path = "../tests/support/viz_games.rs"]
mod viz_games;

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os();
    let program = arguments.next().unwrap_or_else(|| "export_games".into());
    let output = arguments.next().ok_or_else(|| {
        IoError::new(
            ErrorKind::InvalidInput,
            format!("usage: {} <dir>", PathBuf::from(&program).display()),
        )
    })?;
    if arguments.next().is_some() {
        return Err(IoError::new(
            ErrorKind::InvalidInput,
            format!("usage: {} <dir>", PathBuf::from(&program).display()),
        )
        .into());
    }

    let output = PathBuf::from(output);
    for (name, (spec, solutions)) in [
        (
            "prisoners_dilemma",
            viz_games::prisoners_dilemma_artifacts()?,
        ),
        ("battle_of_sexes", viz_games::battle_of_sexes_artifacts()?),
        (
            "rock_paper_scissors",
            viz_games::rock_paper_scissors_artifacts()?,
        ),
    ] {
        let directory = output.join(name);
        fs::create_dir_all(&directory)?;
        write_game_spec(&spec, &directory.join("game_spec.json"))?;
        write_solutions(&solutions, &directory.join("solutions.json"))?;
    }
    Ok(())
}
