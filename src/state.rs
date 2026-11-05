use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::browser::BundleId;
use crate::paths::PathsError;

/// Machine-written state kept in `<state dir>/state.toml`.
///
/// Unlike `Config`, unknown fields are tolerated: a newer trier may add
/// fields, and an older binary must still read the file after a downgrade.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct State {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_default_browser_bundle_id: Option<BundleId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registered_version: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("failed to read {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("failed to parse {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("failed to write {}: {source}", path.display())]
    Write { path: PathBuf, source: io::Error },
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

    /// [`State::load`] for a path that may not have been locatable; the
    /// error is a message ready to print.
    pub fn load_located(path: Result<&Path, &PathsError>) -> Result<Self, String> {
        Self::load(path.map_err(ToString::to_string)?).map_err(|e| e.to_string())
    }

    /// Writes the state to `path`, creating its directory.
    ///
    /// Written to a temporary file in the same directory and renamed, so a
    /// crash never leaves a truncated `state.toml` behind.
    pub fn save(&self, path: &Path) -> Result<(), StateError> {
        let write_err = |source| StateError::Write {
            path: path.to_owned(),
            source,
        };
        let content = toml::to_string(self).expect("State always serializes");
        let tmp = path.with_extension(format!("toml.tmp.{}", std::process::id()));
        path.parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .map_err(write_err)?;
        std::fs::write(&tmp, content).map_err(write_err)?;
        std::fs::rename(&tmp, path).map_err(write_err)
    }

    /// Records `current`, the default browser just before trier takes over,
    /// so it can be restored and used as a fallback.
    ///
    /// When trier is already the default (a re-register), the existing
    /// record is kept: recording trier itself would lose the real browser.
    pub fn record_previous_default(&mut self, current: Option<&BundleId>, own: &BundleId) {
        if let Some(current) = current.filter(|id| *id != own) {
            self.previous_default_browser_bundle_id = Some(current.clone());
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::test_support;

    fn temp_file(name: &str, content: &str) -> PathBuf {
        test_support::temp_file("state", name, content)
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

    #[test]
    fn read_error_message_includes_the_cause() {
        let dir = test_support::temp_dir("state/unreadable");
        let err = State::load(&dir).unwrap_err();
        assert!(matches!(err, StateError::Read { .. }));
        let prefix = format!("failed to read {}: ", dir.display());
        let message = err.to_string();
        assert!(
            message
                .strip_prefix(&prefix)
                .is_some_and(|cause| !cause.is_empty()),
            "{message}"
        );
    }

    fn bundle_id(s: &str) -> BundleId {
        BundleId::try_from(s.to_owned()).unwrap()
    }

    #[test]
    fn unlocatable_state_is_a_printable_error() {
        let err = State::load_located(Err(&PathsError::NoHome {
            what: "state",
            xdg_var: "XDG_STATE_HOME",
        }))
        .unwrap_err();
        assert!(
            err.starts_with("cannot locate the state directory"),
            "{err}"
        );
    }

    #[test]
    fn located_state_loads_normally() {
        let path = temp_file(
            "located.toml",
            "previous_default_browser_bundle_id = \"com.apple.Safari\"\n",
        );
        assert_eq!(
            State::load_located(Ok(&path))
                .unwrap()
                .previous_default_browser_bundle_id,
            Some(bundle_id("com.apple.Safari"))
        );
    }

    #[test]
    fn save_then_load_round_trips() {
        let path = test_support::temp_dir("state").join("nested/round-trip.toml");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        let state = State {
            previous_default_browser_bundle_id: Some(bundle_id("com.apple.Safari")),
            registered_version: Some("0.1.0".to_owned()),
        };
        state.save(&path).unwrap();
        assert_eq!(State::load(&path).unwrap(), state);
    }

    #[test]
    fn empty_state_saves_without_fields() {
        let path = test_support::temp_dir("state").join("empty-save.toml");
        State::default().save(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn save_leaves_no_temporary_file_behind() {
        let dir = test_support::temp_dir("state").join("no-tmp");
        let _ = std::fs::remove_dir_all(&dir);
        State::default().save(&dir.join("state.toml")).unwrap();
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["state.toml"]);
    }

    #[test]
    fn save_reports_unwritable_location() {
        let file = temp_file("not-a-dir", "");
        let err = State::default().save(&file.join("state.toml")).unwrap_err();
        assert!(matches!(err, StateError::Write { .. }), "{err:?}");
    }

    const TRIER: &str = "dev.m1sk9.trier";

    #[test]
    fn records_the_current_default_browser() {
        let mut state = State::default();
        state.record_previous_default(Some(&bundle_id("com.apple.Safari")), &bundle_id(TRIER));
        assert_eq!(
            state.previous_default_browser_bundle_id,
            Some(bundle_id("com.apple.Safari"))
        );
    }

    #[test]
    fn previous_default_is_not_overwritten_when_trier_is_already_the_default() {
        let mut state = State {
            previous_default_browser_bundle_id: Some(bundle_id("com.apple.Safari")),
            ..State::default()
        };
        state.record_previous_default(Some(&bundle_id(TRIER)), &bundle_id(TRIER));
        assert_eq!(
            state.previous_default_browser_bundle_id,
            Some(bundle_id("com.apple.Safari"))
        );
    }

    #[test]
    fn a_newer_default_browser_replaces_the_record() {
        let mut state = State {
            previous_default_browser_bundle_id: Some(bundle_id("com.apple.Safari")),
            ..State::default()
        };
        state.record_previous_default(Some(&bundle_id("org.mozilla.firefox")), &bundle_id(TRIER));
        assert_eq!(
            state.previous_default_browser_bundle_id,
            Some(bundle_id("org.mozilla.firefox"))
        );
    }

    #[test]
    fn unknown_current_default_keeps_the_record() {
        let mut state = State {
            previous_default_browser_bundle_id: Some(bundle_id("com.apple.Safari")),
            ..State::default()
        };
        state.record_previous_default(None, &bundle_id(TRIER));
        assert_eq!(
            state.previous_default_browser_bundle_id,
            Some(bundle_id("com.apple.Safari"))
        );
    }

    #[test]
    fn save_reports_a_temporary_file_that_cannot_be_written() {
        let dir = test_support::temp_dir("state").join("tmp-blocked");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("state.toml");
        let tmp = path.with_extension(format!("toml.tmp.{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let err = State::default().save(&path).unwrap_err();
        assert!(matches!(err, StateError::Write { .. }), "{err:?}");
        assert!(!path.exists());
    }
}
