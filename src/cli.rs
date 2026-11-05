//! Subcommand implementations. Argument parsing lives in `main.rs`.

use std::io::{self, Write};
use std::path::Path;

use crate::config::ConfigError;

pub mod check;
pub mod config;
pub mod explain;

/// Writes one `error:` line per problem in `e`.
pub fn report_config_error(path: &Path, e: &ConfigError, err: &mut dyn Write) -> io::Result<()> {
    match e {
        ConfigError::Invalid(violations) => {
            for v in violations {
                writeln!(err, "error: {}: {v}", path.display())?;
            }
            Ok(())
        }
        ConfigError::Read { .. } => writeln!(err, "error: {e}"),
        ConfigError::Parse(_) => writeln!(err, "error: {}: {e}", path.display()),
    }
}
