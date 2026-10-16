use std::error::Error;
use std::io::{Error as IoError, ErrorKind};
use std::path::PathBuf;

#[path = "../tests/support/viz_ipd.rs"]
mod viz_ipd;

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os();
    let program = arguments.next().unwrap_or_else(|| "viz_ipd".into());
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

    viz_ipd::write_output(&PathBuf::from(output))
}
