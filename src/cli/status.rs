use std::path::PathBuf;

use crate::browser::BundleId;
use crate::cli::Report;
use crate::config::Config;
use crate::paths::PathsError;
use crate::register::{BUNDLE_ID, is_trier_bundle};
use crate::state::State;

/// What `status` inspects; gathered by `main.rs` so this stays testable.
pub struct Inputs {
    pub config_path: PathBuf,
    pub bundle: Result<PathBuf, String>,
    pub state_path: Result<PathBuf, PathsError>,
    pub log_path: Result<PathBuf, PathsError>,
    /// The app macOS currently opens `https` URLs with.
    pub https_handler: Option<BundleId>,
}

/// Reports the config, the bundle, the registration and the default browser.
///
/// Returns `false` when anything needs the user's attention.
pub fn run(inputs: Inputs, report: &mut Report) -> bool {
    let mut healthy = true;

    match Config::load(&inputs.config_path) {
        Ok(_) => report.out(format_args!(
            "config:     {} (OK)",
            inputs.config_path.display()
        )),
        Err(e) => {
            report.out(format_args!("config:     {}", inputs.config_path.display()));
            report.err(format_args!("warning: the config does not load: {e}"));
            healthy = false;
        }
    }

    match &inputs.bundle {
        Ok(bundle) if is_trier_bundle(bundle) => {
            report.out(format_args!("bundle:     {}", bundle.display()));
        }
        Ok(bundle) if bundle.exists() => {
            report.out(format_args!(
                "bundle:     {} (not a trier bundle)",
                bundle.display()
            ));
            healthy = false;
        }
        Ok(bundle) => {
            report.out(format_args!("bundle:     {} (missing)", bundle.display()));
            healthy = false;
        }
        Err(e) => {
            report.out(format_args!("bundle:     unknown ({e})"));
            healthy = false;
        }
    }

    let state = State::load_located(inputs.state_path.as_deref()).unwrap_or_else(|e| {
        report.err(format_args!("warning: cannot read the state: {e}"));
        healthy = false;
        State::default()
    });
    let version = env!("CARGO_PKG_VERSION");
    match &state.registered_version {
        Some(registered) if registered == version => {
            report.out(format_args!("registered: {registered}"));
        }
        Some(registered) => {
            report.out(format_args!("registered: {registered}"));
            report.err(format_args!(
                "warning: the bundle was registered by trier {registered}, this is {version}; run `trier register` to update the bundle"
            ));
            healthy = false;
        }
        None => {
            report.out("registered: no (run `trier register`)");
            healthy = false;
        }
    }

    match &inputs.https_handler {
        Some(id) if id.as_str() == BUNDLE_ID => report.out("default:    trier"),
        Some(id) => {
            report.out(format_args!("default:    {id} (not trier)"));
            healthy = false;
        }
        None => {
            report.out("default:    none");
            healthy = false;
        }
    }

    match &state.previous_default_browser_bundle_id {
        Some(id) => report.out(format_args!("previous:   {id}")),
        None => report.out("previous:   none recorded"),
    }
    match &inputs.log_path {
        Ok(path) => report.out(format_args!("log:        {}", path.display())),
        Err(e) => report.out(format_args!("log:        unknown ({e})")),
    }
    healthy
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::test_support::{self, NO_STATE, example_path, temp_dir, temp_file};

    fn bundle_id(s: &str) -> BundleId {
        BundleId::try_from(s.to_owned()).unwrap()
    }

    fn installed_bundle(name: &str) -> PathBuf {
        let dir = temp_dir("status").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        test_support::fake_trier_bundle(&dir)
    }

    fn registered_state(name: &str, version: &str) -> PathBuf {
        temp_file(
            "status",
            name,
            &format!(
                "previous_default_browser_bundle_id = \"com.apple.Safari\"\nregistered_version = \"{version}\"\n"
            ),
        )
    }

    fn healthy_inputs(name: &str) -> Inputs {
        Inputs {
            config_path: example_path(),
            bundle: Ok(installed_bundle(name)),
            state_path: Ok(registered_state(
                &format!("{name}.toml"),
                env!("CARGO_PKG_VERSION"),
            )),
            log_path: Ok(PathBuf::from("/state/trier/trier.log")),
            https_handler: Some(bundle_id(BUNDLE_ID)),
        }
    }

    fn status(inputs: Inputs) -> (bool, String, String) {
        let mut report = Report::default();
        let ok = run(inputs, &mut report);
        (ok, report.stdout(), report.stderr())
    }

    #[test]
    fn fully_registered_install_is_healthy() {
        let (ok, out, err) = status(healthy_inputs("healthy"));
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert!(
            out.contains("config:     ") && out.contains("(OK)"),
            "{out}"
        );
        assert!(out.contains("default:    trier"), "{out}");
        assert!(out.contains("previous:   com.apple.Safari"), "{out}");
        assert!(out.contains("log:        /state/trier/trier.log"), "{out}");
    }

    #[test]
    fn version_mismatch_asks_to_register_again() {
        let inputs = Inputs {
            state_path: Ok(registered_state("old-version.toml", "0.0.0-old")),
            ..healthy_inputs("old")
        };
        let (ok, _, err) = status(inputs);
        assert!(!ok);
        assert!(
            err.contains("run `trier register` to update the bundle"),
            "{err}"
        );
    }

    #[test]
    fn another_default_browser_is_reported() {
        let inputs = Inputs {
            https_handler: Some(bundle_id("com.apple.Safari")),
            ..healthy_inputs("not-default")
        };
        let (ok, out, _) = status(inputs);
        assert!(!ok);
        assert!(
            out.contains("default:    com.apple.Safari (not trier)"),
            "{out}"
        );
    }

    #[test]
    fn unregistered_install_is_reported() {
        let inputs = Inputs {
            config_path: PathBuf::from("/nonexistent/trier/config.toml"),
            bundle: Ok(PathBuf::from("/nonexistent/Trier.app")),
            state_path: Ok(PathBuf::from(NO_STATE)),
            log_path: Err(PathsError::NoHome {
                what: "state",
                xdg_var: "XDG_STATE_HOME",
            }),
            https_handler: None,
        };
        let (ok, out, err) = status(inputs);
        assert!(!ok);
        assert!(err.contains("warning: the config does not load"), "{err}");
        assert!(
            out.contains("bundle:     /nonexistent/Trier.app (missing)"),
            "{out}"
        );
        assert!(
            out.contains("registered: no (run `trier register`)"),
            "{out}"
        );
        assert!(out.contains("default:    none"), "{out}");
        assert!(out.contains("previous:   none recorded"), "{out}");
        assert!(out.contains("log:        unknown"), "{out}");
    }

    #[test]
    fn a_foreign_bundle_is_not_reported_as_missing() {
        let dir = temp_dir("status").join("foreign");
        let _ = std::fs::remove_dir_all(&dir);
        let bundle = test_support::foreign_bundle(&dir);
        let inputs = Inputs {
            bundle: Ok(bundle.clone()),
            ..healthy_inputs("foreign-state")
        };
        let (ok, out, _) = status(inputs);
        assert!(!ok);
        assert!(
            out.contains(&format!(
                "bundle:     {} (not a trier bundle)",
                bundle.display()
            )),
            "{out}"
        );
    }

    #[test]
    fn unknown_locations_are_reported() {
        let inputs = Inputs {
            bundle: Err("HOME is not set".to_owned()),
            state_path: Err(PathsError::NoHome {
                what: "state",
                xdg_var: "XDG_STATE_HOME",
            }),
            ..healthy_inputs("unknown")
        };
        let (ok, out, err) = status(inputs);
        assert!(!ok);
        assert!(
            out.contains("bundle:     unknown (HOME is not set)"),
            "{out}"
        );
        assert!(err.contains("warning: cannot read the state"), "{err}");
    }
}
