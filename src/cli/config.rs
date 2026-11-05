use std::io;
use std::path::{Path, PathBuf};

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
    let io_err = |source| InitError::Io {
        path: path.to_owned(),
        source,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_err)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    // `create_new` instead of an `exists()` check so a file appearing in
    // between is never clobbered.
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let mut file = options.open(path).map_err(|e| {
        if e.kind() == io::ErrorKind::AlreadyExists {
            InitError::AlreadyExists(path.to_owned())
        } else {
            io_err(e)
        }
    })?;
    io::Write::write_all(&mut file, EXAMPLE.as_bytes()).map_err(io_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        // Not `std::env::temp_dir()`: it reads TMPDIR at runtime, racing with
        // the env-mutating tests in `paths`.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("target/tmp/init-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
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
}
