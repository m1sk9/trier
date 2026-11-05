use std::io::{self, Write};
use std::path::{Path, PathBuf};

use url::Url;

use crate::browser::BundleId;
use crate::cli::report_config_error;
use crate::config::Config;
use crate::paths::PathsError;
use crate::resolve::{ResolveError, decide};
use crate::state::State;

/// Prints which browser `url` would open in, and why.
///
/// `state_path` is only consulted when no rule or `default_browser` decides,
/// so a missing or broken state never blocks a URL the config resolves.
///
/// Returns whether a browser was decided.
pub fn run(
    input: &str,
    config_path: &Path,
    state_path: Result<PathBuf, PathsError>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<bool> {
    let url = match Url::parse(input) {
        Ok(url) => url,
        Err(e) => {
            writeln!(
                err,
                "error: invalid URL `{input}`: {e}{}",
                scheme_hint(input)
            )?;
            return Ok(false);
        }
    };
    let config = match Config::load(config_path) {
        Ok(config) => config,
        Err(e) => {
            report_config_error(config_path, &e, err)?;
            return Ok(false);
        }
    };

    let registry = config.registry();
    let decision = match decide(&url, &config, &registry, None) {
        Err(ResolveError::NoFallback(_)) => {
            let recorded = recorded_default(state_path, err)?;
            decide(&url, &config, &registry, recorded.as_ref())
        }
        other => other,
    };
    match decision {
        Ok(decision) => {
            writeln!(out, "{decision}")?;
            Ok(true)
        }
        Err(e @ ResolveError::UnsupportedScheme(_)) if url.cannot_be_a_base() => {
            writeln!(err, "error: {e}{}", scheme_hint(input))?;
            Ok(false)
        }
        Err(e) => {
            writeln!(err, "error: {e}")?;
            Ok(false)
        }
    }
}

/// The recorded system default, or `None` with a warning when the state
/// cannot be located or read.
fn recorded_default(
    state_path: Result<PathBuf, PathsError>,
    err: &mut dyn Write,
) -> io::Result<Option<BundleId>> {
    let state = state_path
        .map_err(|e| e.to_string())
        .and_then(|path| State::load(&path).map_err(|e| e.to_string()));
    match state {
        Ok(state) => Ok(state.previous_default_browser_bundle_id),
        Err(e) => {
            writeln!(err, "warning: ignoring the recorded system default: {e}")?;
            Ok(None)
        }
    }
}

/// Suggests `https://` for input that looks like a bare host, such as
/// `github.com` or `localhost:3000`.
fn scheme_hint(input: &str) -> String {
    let looks_like_host = !input.contains("://")
        && Url::parse(&format!("https://{input}"))
            .is_ok_and(|url| url.username().is_empty() && url.password().is_none());
    if looks_like_host {
        format!(" (did you mean `https://{input}`?)")
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn example_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("config/config.toml.example")
    }

    fn temp_file(name: &str, content: &str) -> PathBuf {
        crate::test_support::temp_file("explain", name, content)
    }

    fn explain(url: &str, config: &Path, state: &Path) -> (bool, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let ok = run(url, config, Ok(state.to_owned()), &mut out, &mut err).unwrap();
        (
            ok,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    const NO_STATE: &str = "/nonexistent/trier/state.toml";

    #[test]
    fn prints_matched_rule_and_browser() {
        let (ok, out, err) = explain(
            "https://github.com/mk-system/trier",
            &example_path(),
            Path::new(NO_STATE),
        );
        assert!(ok, "{err}");
        assert!(
            out.contains("matched:   rules[0] (github.com/mk-system/**)"),
            "{out}"
        );
        assert!(
            out.contains("browser:   firefox@developer-edition"),
            "{out}"
        );
        assert!(
            out.contains("bundle id: org.mozilla.firefoxdeveloperedition"),
            "{out}"
        );
    }

    #[test]
    fn uses_recorded_default_from_state_file() {
        let config = temp_file("no-default.toml", "version = 1\n");
        let state = temp_file(
            "state.toml",
            "previous_default_browser_bundle_id = \"com.example.Browser\"\n",
        );
        let (ok, out, _) = explain("https://example.com/", &config, &state);
        assert!(ok);
        assert!(out.contains("matched:   recorded system default"), "{out}");
        assert!(out.contains("bundle id: com.example.Browser"), "{out}");
    }

    #[test]
    fn invalid_url_is_an_error() {
        let (ok, _, err) = explain("not a url", &example_path(), Path::new(NO_STATE));
        assert!(!ok);
        assert!(err.starts_with("error: invalid URL `not a url`"), "{err}");
    }

    #[test]
    fn unsupported_scheme_is_an_error() {
        let (ok, _, err) = explain(
            "mailto:me@example.com",
            &example_path(),
            Path::new(NO_STATE),
        );
        assert!(!ok);
        assert!(err.contains("unsupported URL scheme `mailto`"), "{err}");
    }

    #[test]
    fn config_errors_are_reported() {
        let config = temp_file("invalid.toml", "version = 2\n");
        let (ok, _, err) = explain("https://example.com/", &config, Path::new(NO_STATE));
        assert!(!ok);
        assert!(err.contains("unsupported config version 2"), "{err}");
    }

    fn broken_state(name: &str) -> PathBuf {
        temp_file(name, "previous_default_browser_bundle_id = 1\n")
    }

    #[test]
    fn broken_state_file_is_not_read_when_a_rule_decides() {
        let state = broken_state("broken-rule.toml");
        let (ok, _, err) = explain(
            "https://github.com/mk-system/trier",
            &example_path(),
            &state,
        );
        assert!(ok);
        assert_eq!(err, "");
    }

    #[test]
    fn broken_state_file_is_a_warning_when_the_fallback_is_needed() {
        let config = temp_file("no-default-broken.toml", "version = 1\n");
        let state = broken_state("broken-fallback.toml");
        let (ok, _, err) = explain("https://example.com/", &config, &state);
        assert!(!ok);
        assert!(
            err.contains("warning: ignoring the recorded system default: failed to parse"),
            "{err}"
        );
        assert!(err.contains("error: no rule matches"), "{err}");
    }

    #[test]
    fn unlocatable_state_dir_does_not_block_a_rule() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let no_home = Err(PathsError::NoHome {
            what: "state",
            xdg_var: "XDG_STATE_HOME",
        });
        let ok = run(
            "https://github.com/mk-system/trier",
            &example_path(),
            no_home,
            &mut out,
            &mut err,
        )
        .unwrap();
        assert!(ok);
    }

    #[test]
    fn unlocatable_state_dir_is_a_warning_when_the_fallback_is_needed() {
        let config = temp_file("no-default-no-home.toml", "version = 1\n");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let no_home = Err(PathsError::NoHome {
            what: "state",
            xdg_var: "XDG_STATE_HOME",
        });
        let ok = run("https://example.com/", &config, no_home, &mut out, &mut err).unwrap();
        let err = String::from_utf8(err).unwrap();
        assert!(!ok);
        assert!(
            err.contains(
                "warning: ignoring the recorded system default: cannot locate the state directory"
            ),
            "{err}"
        );
    }

    #[test]
    fn bare_host_input_suggests_https() {
        for input in ["github.com", "localhost:3000"] {
            let (ok, _, err) = explain(input, &example_path(), Path::new(NO_STATE));
            assert!(!ok);
            assert!(
                err.contains(&format!("(did you mean `https://{input}`?)")),
                "{err}"
            );
        }
    }

    #[test]
    fn mailto_gets_no_https_hint() {
        let (_, _, err) = explain(
            "mailto:me@example.com",
            &example_path(),
            Path::new(NO_STATE),
        );
        assert!(!err.contains("did you mean"), "{err}");
    }
}
