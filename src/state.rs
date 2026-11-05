use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::browser::BundleId;

/// Machine-written state kept in `<state dir>/state.toml`.
///
/// Unlike `Config`, unknown fields are tolerated: a newer trier may add
/// fields, and an older binary must still read the file after a downgrade.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct State {
    pub previous_default_browser_bundle_id: Option<BundleId>,
    pub registered_version: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("failed to read {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}

impl State {
    /// Loads the state at `path`; a missing file is the default state.
    pub fn load(path: &Path) -> Result<Self, StateError> {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => {
                return Err(StateError::Read {
                    path: path.to_owned(),
                    source,
                });
            }
        };
        toml::from_str(&content).map_err(|source| StateError::Parse {
            path: path.to_owned(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, content: &str) -> PathBuf {
        // Not `std::env::temp_dir()`: it reads TMPDIR at runtime, racing with
        // the env-mutating tests in `paths`.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/tmp/state");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn missing_file_is_default_state() {
        let state = State::load(Path::new("/nonexistent/trier/state.toml")).unwrap();
        assert_eq!(state, State::default());
    }

    #[test]
    fn loads_recorded_default_browser() {
        let path = temp_file(
            "recorded.toml",
            "previous_default_browser_bundle_id = \"com.apple.Safari\"\nregistered_version = \"0.1.0\"\n",
        );
        let state = State::load(&path).unwrap();
        assert_eq!(
            state
                .previous_default_browser_bundle_id
                .as_ref()
                .map(BundleId::as_str),
            Some("com.apple.Safari")
        );
        assert_eq!(state.registered_version.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        let path = temp_file("future.toml", "added_in_a_later_version = true\n");
        assert_eq!(State::load(&path).unwrap(), State::default());
    }

    #[test]
    fn malformed_file_is_an_error() {
        let path = temp_file("broken.toml", "previous_default_browser_bundle_id = 1\n");
        assert!(matches!(State::load(&path), Err(StateError::Parse { .. })));
    }
}
