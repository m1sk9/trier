//! Routing for URLs that arrive in handler mode, i.e. clicked links.
//!
//! Kept apart from the AppKit glue in `handler.rs` so it can be tested.

use std::path::{Path, PathBuf};

use url::Url;

use crate::browser::BundleId;
use crate::cli::open::Opener;
use crate::config::{Config, ConfigError};
use crate::paths::PathsError;
use crate::resolve::decide;
use crate::state::State;

/// Shows a message to the user; there is no terminal in handler mode.
pub type Alert<'a> = &'a mut dyn FnMut(&str);

/// Opens each clicked URL in its decided browser.
///
/// A click is never dropped silently: when the config cannot be loaded, no
/// browser can be decided, or the decided browser fails to open, the URL
/// still opens in the recorded system default (with an alert explaining
/// why), and only without one is the user left with just the alert. Local
/// files (`file://`, from trier being the default for HTML documents) open
/// in `default_browser`, else the recorded default. Returns whether every
/// URL was routed normally.
pub fn dispatch(
    inputs: &[String],
    config_path: &Path,
    state_path: Result<PathBuf, PathsError>,
    opener: Opener,
    alert: Alert,
) -> bool {
    let config = Config::load(config_path);
    let recorded = recorded_default(state_path);
    let mut all_ok = true;
    for input in inputs {
        let url = match Url::parse(input) {
            Ok(url) => url,
            Err(e) => {
                let message = format!("invalid URL `{input}`: {e}");
                log::error!("{message}");
                alert(&message);
                all_ok = false;
                continue;
            }
        };
        let routed = if url.scheme() == "file" {
            open_local_file(&url, config.as_ref().ok(), recorded.as_ref(), opener, alert)
        } else {
            route(&url, &config, config_path, recorded.as_ref(), opener, alert)
        };
        all_ok &= routed;
    }
    all_ok
}

fn route(
    url: &Url,
    config: &Result<Config, ConfigError>,
    config_path: &Path,
    recorded: Option<&BundleId>,
    opener: Opener,
    alert: Alert,
) -> bool {
    let decision = match config {
        Err(e) => Err(format!("cannot load {}: {e}", config_path.display())),
        Ok(config) => decide(url, config, &config.registry(), recorded).map_err(|e| e.to_string()),
    };
    let decision = match decision {
        Ok(decision) => decision,
        Err(message) => {
            log::error!("{url}: {message}");
            alert(&fall_back(url, &message, recorded, opener));
            return false;
        }
    };
    log::info!(
        "{} -> {} ({}) by {}",
        decision.url,
        decision
            .browser
            .as_ref()
            .map_or("recorded system default".to_owned(), ToString::to_string),
        decision.bundle_id,
        decision.source
    );
    let Err(e) = opener(&decision.bundle_id, url) else {
        return true;
    };
    let message = format!("could not open {url} in {}: {e}", decision.bundle_id);
    log::error!("{message}");
    // Retrying the same app would fail the same way.
    let other = recorded.filter(|id| **id != decision.bundle_id);
    alert(&fall_back(url, &message, other, opener));
    false
}

fn open_local_file(
    url: &Url,
    config: Option<&Config>,
    recorded: Option<&BundleId>,
    opener: Opener,
    alert: Alert,
) -> bool {
    let configured = config.and_then(|config| {
        let name = config.default_browser.as_ref()?;
        config.registry().resolve(name).cloned()
    });
    let Some(bundle_id) = configured.as_ref().or(recorded) else {
        let message =
            format!("no browser to open the local file {url}: set `default_browser` in the config");
        log::error!("{message}");
        alert(&message);
        return false;
    };
    log::info!("{url} -> {bundle_id} (local file)");
    match opener(bundle_id, url) {
        Ok(()) => true,
        Err(e) => {
            let message = format!("could not open {url} in {bundle_id}: {e}");
            log::error!("{message}");
            alert(&message);
            false
        }
    }
}

/// Opens `url` in the recorded default, returning the alert text.
fn fall_back(url: &Url, message: &str, recorded: Option<&BundleId>, opener: Opener) -> String {
    let Some(bundle_id) = recorded else {
        return message.to_owned();
    };
    match opener(bundle_id, url) {
        Ok(()) => {
            log::info!("{url} -> {bundle_id} (recorded system default, after an error)");
            format!("{message}\n\nOpened {url} in {bundle_id} instead.")
        }
        Err(e) => {
            log::error!("could not open {url} in {bundle_id}: {e}");
            format!("{message}\n\nCould not open {url} in {bundle_id} either: {e}")
        }
    }
}

/// The recorded system default; a state that cannot be read only loses the
/// fallback, it does not stop routing.
fn recorded_default(state_path: Result<PathBuf, PathsError>) -> Option<BundleId> {
    match State::load_located(state_path.as_deref()) {
        Ok(state) => state.previous_default_browser_bundle_id,
        Err(e) => {
            log::warn!("ignoring the recorded system default: {e}");
            None
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::macos::MacosError;
    use crate::test_support::{self, NO_STATE, example_path, temp_file};

    struct Run {
        ok: bool,
        launched: Vec<String>,
        alerts: Vec<String>,
        logs: Vec<String>,
    }

    /// Dispatches `inputs`; the fake opener fails for host `fail.example`
    /// and for the bundle id `com.example.Broken`.
    fn run(inputs: &[&str], config: &Path, state: &Path) -> Run {
        test_support::capture_logs();
        let mut launched = Vec::new();
        let mut alerts = Vec::new();
        let inputs: Vec<String> = inputs.iter().map(|s| (*s).to_owned()).collect();
        let ok = dispatch(
            &inputs,
            config,
            Ok(state.to_owned()),
            &mut |id: &BundleId, url: &Url| {
                if url.host_str() == Some("fail.example") || id.as_str() == "com.example.Broken" {
                    return Err(MacosError::Spawn {
                        program: "fake-open".to_owned(),
                        source: std::io::Error::other("refused"),
                    });
                }
                launched.push(format!("{id} {url}"));
                Ok(())
            },
            &mut |message: &str| alerts.push(message.to_owned()),
        );
        Run {
            ok,
            launched,
            alerts,
            logs: test_support::captured_logs(),
        }
    }

    fn state_with(name: &str, bundle_id: &str) -> PathBuf {
        temp_file(
            "dispatch",
            name,
            &format!("previous_default_browser_bundle_id = \"{bundle_id}\"\n"),
        )
    }

    #[test]
    fn routes_every_url_in_one_event() {
        let r = run(
            &[
                "https://github.com/ab-company/trier",
                "https://example.com/",
            ],
            &example_path(),
            Path::new(NO_STATE),
        );
        assert!(r.ok);
        assert_eq!(
            r.launched,
            [
                "org.mozilla.firefoxdeveloperedition https://github.com/ab-company/trier",
                "com.apple.Safari https://example.com/",
            ]
        );
        assert!(r.alerts.is_empty());
    }

    #[test]
    fn broken_config_still_opens_in_the_recorded_default_with_an_alert() {
        let config = temp_file("dispatch", "broken.toml", "version = 2\n");
        let state = state_with("recorded.toml", "com.example.Previous");
        let r = run(&["https://example.com/"], &config, &state);
        assert!(!r.ok);
        assert_eq!(r.launched, ["com.example.Previous https://example.com/"]);
        assert_eq!(r.alerts.len(), 1);
        assert!(
            r.alerts[0].contains("unsupported config version 2"),
            "{:?}",
            r.alerts
        );
        assert!(
            r.alerts[0].contains("Opened https://example.com/ in com.example.Previous instead.")
        );
    }

    #[test]
    fn undecidable_url_opens_in_the_recorded_default() {
        let state = state_with("recorded-scheme.toml", "com.example.Previous");
        let r = run(&["ftp://example.com/file"], &example_path(), &state);
        assert!(!r.ok);
        assert_eq!(r.launched, ["com.example.Previous ftp://example.com/file"]);
        assert!(
            r.alerts[0].contains("unsupported URL scheme `ftp`"),
            "{:?}",
            r.alerts
        );
    }

    #[test]
    fn without_a_recorded_default_only_the_alert_remains() {
        let config = temp_file("dispatch", "broken-no-state.toml", "version = 2\n");
        let r = run(&["https://example.com/"], &config, Path::new(NO_STATE));
        assert!(!r.ok);
        assert!(r.launched.is_empty());
        assert_eq!(r.alerts.len(), 1);
        assert!(!r.alerts[0].contains("instead"), "{:?}", r.alerts);
    }

    #[test]
    fn invalid_url_is_alerted_without_opening() {
        let state = state_with("recorded-invalid.toml", "com.example.Previous");
        let r = run(&["not a url"], &example_path(), &state);
        assert!(!r.ok);
        assert!(r.launched.is_empty());
        assert!(
            r.alerts[0].starts_with("invalid URL `not a url`"),
            "{:?}",
            r.alerts
        );
    }

    #[test]
    fn failed_open_is_alerted_and_the_rest_continue() {
        let r = run(
            &["https://fail.example/", "https://example.com/"],
            &example_path(),
            Path::new(NO_STATE),
        );
        assert!(!r.ok);
        assert_eq!(r.launched, ["com.apple.Safari https://example.com/"]);
        assert_eq!(r.alerts.len(), 1);
        assert!(
            r.alerts[0].starts_with("could not open https://fail.example/"),
            "{:?}",
            r.alerts
        );
    }

    #[test]
    fn failed_fallback_is_reported_in_the_alert() {
        let config = temp_file("dispatch", "broken-fallback.toml", "version = 2\n");
        let state = state_with("recorded-broken.toml", "com.example.Broken");
        let r = run(&["https://example.com/"], &config, &state);
        assert!(r.launched.is_empty());
        assert!(
            r.alerts[0]
                .contains("Could not open https://example.com/ in com.example.Broken either"),
            "{:?}",
            r.alerts
        );
    }

    #[test]
    fn unreadable_state_only_loses_the_fallback() {
        let state = temp_file(
            "dispatch",
            "broken-state.toml",
            "previous_default_browser_bundle_id = 1\n",
        );
        let r = run(&["https://example.com/"], &example_path(), &state);
        assert!(r.ok);
        assert_eq!(r.launched, ["com.apple.Safari https://example.com/"]);
    }

    #[test]
    fn unlocatable_state_only_loses_the_fallback() {
        test_support::capture_logs();
        let mut launched = 0;
        let ok = dispatch(
            &["https://example.com/".to_owned()],
            &example_path(),
            Err(PathsError::NoHome {
                what: "state",
                xdg_var: "XDG_STATE_HOME",
            }),
            &mut |_: &BundleId, _: &Url| {
                launched += 1;
                Ok(())
            },
            &mut |_: &str| {},
        );
        assert!(ok);
        assert_eq!(launched, 1);
        assert!(
            test_support::captured_logs()[0]
                .starts_with("WARN ignoring the recorded system default")
        );
    }

    #[test]
    fn recorded_default_decision_is_routed() {
        let config = temp_file("dispatch", "no-default.toml", "version = 1\n");
        let state = state_with("recorded-decide.toml", "com.example.Previous");
        let r = run(&["https://example.com/"], &config, &state);
        assert!(r.ok);
        assert_eq!(r.launched, ["com.example.Previous https://example.com/"]);
    }

    #[test]
    fn failed_browser_falls_back_to_the_recorded_default() {
        let config = temp_file(
            "dispatch",
            "broken-browser.toml",
            "version = 1\n[browsers.gone]\nbundle_id = \"com.example.Broken\"\n\
             [[rules]]\nhosts = [\"example.com\"]\nbrowser = \"gone\"\n",
        );
        let state = state_with("recorded-after-failure.toml", "com.example.Previous");
        let r = run(&["https://example.com/"], &config, &state);
        assert!(!r.ok);
        assert_eq!(r.launched, ["com.example.Previous https://example.com/"]);
        assert!(
            r.alerts[0].contains("Opened https://example.com/ in com.example.Previous instead."),
            "{:?}",
            r.alerts
        );
    }

    #[test]
    fn failed_recorded_default_is_not_retried() {
        let config = temp_file(
            "dispatch",
            "no-default-broken-recorded.toml",
            "version = 1\n",
        );
        let state = state_with("recorded-broken-decide.toml", "com.example.Broken");
        let r = run(&["https://example.com/"], &config, &state);
        assert!(!r.ok);
        assert_eq!(r.alerts.len(), 1);
        assert!(!r.alerts[0].contains("either"), "{:?}", r.alerts);
    }

    #[test]
    fn local_file_opens_in_default_browser_without_an_alert() {
        let r = run(
            &["file:///Users/me/report.html"],
            &example_path(),
            Path::new(NO_STATE),
        );
        assert!(r.ok);
        assert_eq!(
            r.launched,
            ["com.apple.Safari file:///Users/me/report.html"]
        );
        assert!(r.alerts.is_empty());
    }

    #[test]
    fn local_file_uses_the_recorded_default_without_default_browser() {
        let config = temp_file("dispatch", "file-no-default.toml", "version = 1\n");
        let state = state_with("file-recorded.toml", "com.example.Previous");
        let r = run(&["file:///Users/me/report.html"], &config, &state);
        assert!(r.ok);
        assert_eq!(
            r.launched,
            ["com.example.Previous file:///Users/me/report.html"]
        );
    }

    #[test]
    fn local_file_without_any_browser_is_alerted() {
        let config = temp_file("dispatch", "file-nothing.toml", "version = 1\n");
        let r = run(
            &["file:///Users/me/report.html"],
            &config,
            Path::new(NO_STATE),
        );
        assert!(!r.ok);
        assert!(r.launched.is_empty());
        assert!(
            r.alerts[0].contains("set `default_browser`"),
            "{:?}",
            r.alerts
        );
    }

    #[test]
    fn local_file_that_fails_to_open_is_alerted() {
        let config = temp_file("dispatch", "file-broken.toml", "version = 1\n");
        let state = state_with("file-broken-state.toml", "com.example.Broken");
        let r = run(&["file:///Users/me/report.html"], &config, &state);
        assert!(!r.ok);
        assert!(
            r.alerts[0].starts_with("could not open file:///Users/me/report.html"),
            "{:?}",
            r.alerts
        );
    }

    #[test]
    fn each_routing_decision_is_logged_with_its_reason() {
        let r = run(
            &["https://github.com/ab-company/trier"],
            &example_path(),
            Path::new(NO_STATE),
        );
        assert_eq!(
            r.logs,
            [
                "INFO https://github.com/ab-company/trier -> firefox@developer-edition (org.mozilla.firefoxdeveloperedition) by rules[0] (github.com/ab-company/**)"
            ]
        );
    }

    #[test]
    fn routing_errors_and_fallbacks_are_logged() {
        let config = temp_file("dispatch", "broken-logged.toml", "version = 2\n");
        let state = state_with("recorded-logged.toml", "com.example.Previous");
        let r = run(&["https://example.com/"], &config, &state);
        assert!(
            r.logs[0].starts_with("ERROR https://example.com/: cannot load"),
            "{:?}",
            r.logs
        );
        assert_eq!(
            r.logs[1],
            "INFO https://example.com/ -> com.example.Previous (recorded system default, after an error)"
        );
    }
}
