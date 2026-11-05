use std::io;
use std::path::{Path, PathBuf};

use super::WriteError;

pub const EXAMPLE: &str = include_str!("../../config/config.toml.example");

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("{} already exists (use --force to overwrite)", .0.display())]
    AlreadyExists(PathBuf),
    // The io error is exposed via `source()`; repeating it here would make
    // anyhow print it twice.
    #[error("failed to write {}", path.display())]
    Io { path: PathBuf, source: io::Error },
}

/// Writes the example config to `path`, creating parent directories.
pub fn init(path: &Path, force: bool) -> Result<(), InitError> {
    super::write_text(path, EXAMPLE, force).map_err(|e| match e {
        WriteError::AlreadyExists => InitError::AlreadyExists(path.to_owned()),
        WriteError::Io(source) => InitError::Io {
            path: path.to_owned(),
            source,
        },
    })
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = crate::test_support::temp_dir(&format!("init-{name}"));
        std::fs::remove_dir_all(&dir).unwrap();
        dir.join("nested/trier/config.toml")
    }

    #[test]
    fn creates_parent_dirs_and_writes_example() {
        let path = temp_path("create");
        init(&path, false).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EXAMPLE);
    }

    #[test]
    fn refuses_to_overwrite_without_force() {
        let path = temp_path("refuse");
        init(&path, false).unwrap();
        std::fs::write(&path, "user edits").unwrap();
        let err = init(&path, false).unwrap_err();
        assert!(matches!(err, InitError::AlreadyExists(_)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "user edits");
    }

    #[test]
    fn force_overwrites_existing_file() {
        let path = temp_path("force");
        init(&path, false).unwrap();
        std::fs::write(&path, "user edits").unwrap();
        init(&path, true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EXAMPLE);
    }

    #[test]
    fn io_errors_name_the_path() {
        let dir = crate::test_support::temp_dir("init-io");
        let err = init(&dir, true).unwrap_err();
        assert!(matches!(&err, InitError::Io { path, .. } if *path == dir));
    }
}
