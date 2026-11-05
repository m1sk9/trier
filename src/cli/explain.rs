use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::{AppLookup, UrlResolver, load_config};
use crate::paths::PathsError;

/// Prints which browser `input` would open in, why, and where that app is.
///
/// Returns whether a browser was decided.
pub fn run(
    input: &str,
    config_path: &Path,
    state_path: Result<PathBuf, PathsError>,
    lookup: AppLookup,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<bool> {
    let Some(config) = load_config(config_path, err)? else {
        return Ok(false);
    };
    match UrlResolver::new(&config, state_path).resolve(input, err)? {
        Ok(decision) => {
            writeln!(out, "{decision}")?;
            match lookup(&decision.bundle_id) {
                Some(path) => writeln!(out, "app:       {}", path.display())?,
                None => writeln!(out, "app:       not installed")?,
            }
            Ok(true)
        }
        Err(message) => {
            writeln!(err, "error: {message}")?;
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::test_support::lookup_installed;

    fn example_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("config/config.toml.example")
    }

    fn temp_file(name: &str, content: &str) -> PathBuf {
        crate::test_support::temp_file("explain", name, content)
    }

    fn explain(url: &str, config: &Path, state: &Path) -> (bool, String, String) {
        explain_with(url, config, state, &lookup_installed(&[]))
    }

    fn explain_with(
        url: &str,
        config: &Path,
        state: &Path,
        lookup: AppLookup,
    ) -> (bool, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let ok = run(
            url,
            config,
            Ok(state.to_owned()),
            lookup,
            &mut out,
            &mut err,
        )
        .unwrap();
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
            &lookup_installed(&[]),
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
        let ok = run(
            "https://example.com/",
            &config,
            no_home,
            &lookup_installed(&[]),
            &mut out,
            &mut err,
        )
        .unwrap();
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

    #[test]
    fn prints_install_path_of_the_chosen_app() {
        let (_, out, _) = explain_with(
            "https://example.com/",
            &example_path(),
            Path::new(NO_STATE),
            &lookup_installed(&["com.apple.Safari"]),
        );
        assert!(
            out.ends_with("app:       /Applications/com.apple.Safari.app\n"),
            "{out}"
        );
    }

    #[test]
    fn reports_a_chosen_app_that_is_not_installed() {
        let (ok, out, _) = explain_with(
            "https://example.com/",
            &example_path(),
            Path::new(NO_STATE),
            &lookup_installed(&[]),
        );
        assert!(ok);
        assert!(out.ends_with("app:       not installed\n"), "{out}");
    }
}
