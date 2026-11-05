use std::io::{self, Write};
use std::path::Path;

use url::Url;

use crate::cli::check::report_config_error;
use crate::config::Config;
use crate::resolve::decide;
use crate::state::State;

/// Prints which browser `url` would open in, and why.
///
/// Returns whether a browser was decided.
pub fn run(
    url: &str,
    config_path: &Path,
    state_path: &Path,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<bool> {
    let url = match Url::parse(url) {
        Ok(url) => url,
        Err(e) => {
            writeln!(err, "error: invalid URL `{url}`: {e}")?;
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
    let state = match State::load(state_path) {
        Ok(state) => state,
        Err(e) => {
            writeln!(err, "error: {e}")?;
            return Ok(false);
        }
    };

    match decide(
        &url,
        &config,
        &config.registry(),
        state.previous_default_browser_bundle_id.as_ref(),
    ) {
        Ok(decision) => {
            writeln!(out, "{decision}")?;
            Ok(true)
        }
        Err(e) => {
            writeln!(err, "error: {e}")?;
            Ok(false)
        }
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
        // Not `std::env::temp_dir()`: it reads TMPDIR at runtime, racing with
        // the env-mutating tests in `paths`.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/tmp/explain");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn explain(url: &str, config: &Path, state: &Path) -> (bool, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let ok = run(url, config, state, &mut out, &mut err).unwrap();
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

    #[test]
    fn broken_state_file_is_an_error() {
        let state = temp_file(
            "broken-state.toml",
            "previous_default_browser_bundle_id = 1\n",
        );
        let (ok, _, err) = explain("https://example.com/", &example_path(), &state);
        assert!(!ok);
        assert!(err.contains("failed to parse"), "{err}");
    }
}
