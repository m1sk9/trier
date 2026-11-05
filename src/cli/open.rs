use std::path::{Path, PathBuf};

use url::Url;

use crate::browser::BundleId;
use crate::cli::{Report, UrlResolver, load_config};
use crate::macos::MacosError;
use crate::paths::PathsError;

/// Opens `url` with the app identified by a bundle id.
///
/// Injected so tests do not launch browsers;
/// `main.rs` passes [`crate::macos::open_url_with`].
pub type Opener<'a> = &'a mut dyn FnMut(&BundleId, &Url) -> Result<(), MacosError>;

/// Opens each of `inputs` in its decided browser.
///
/// A failing URL does not stop the remaining ones. Returns whether every URL
/// was opened.
pub fn run(
    inputs: &[String],
    config_path: &Path,
    state_path: Result<PathBuf, PathsError>,
    opener: Opener,
    report: &mut Report,
) -> bool {
    let Some(config) = load_config(config_path, report) else {
        return false;
    };
    let mut resolver = UrlResolver::new(&config, state_path);
    let mut all_ok = true;
    for input in inputs {
        let decision = match resolver.resolve(input, report) {
            Ok(decision) => decision,
            Err(message) => {
                report.err(format_args!("error: {input}: {message}"));
                all_ok = false;
                continue;
            }
        };
        let browser = decision
            .browser
            .as_ref()
            .map_or_else(|| "recorded system default".to_owned(), ToString::to_string);
        match opener(&decision.bundle_id, &decision.url) {
            Ok(()) => report.out(format_args!(
                "{} -> {browser} ({})",
                decision.url, decision.bundle_id
            )),
            Err(e) => {
                report.err(format_args!("error: {input}: {e}"));
                all_ok = false;
            }
        }
    }
    all_ok
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::cli::Stream;
    use crate::test_support::{NO_STATE, example_path, temp_file};

    /// Runs `open`, recording what would have been launched; URLs whose
    /// host is `fail.example` make the fake opener fail.
    fn open(inputs: &[&str], config: &Path, state: &Path) -> (bool, Vec<String>, String, String) {
        let mut launched = Vec::new();
        let mut opener = |id: &BundleId, url: &Url| {
            if url.host_str() == Some("fail.example") {
                return Err(MacosError::Spawn {
                    program: "fake-open".to_owned(),
                    source: std::io::Error::other("refused"),
                });
            }
            launched.push(format!("{id} {url}"));
            Ok(())
        };
        let inputs: Vec<String> = inputs.iter().map(|s| (*s).to_owned()).collect();
        let mut report = Report::default();
        let ok = run(
            &inputs,
            config,
            Ok(state.to_owned()),
            &mut opener,
            &mut report,
        );
        (ok, launched, report.stdout(), report.stderr())
    }

    #[test]
    fn opens_each_url_in_its_decided_browser() {
        let (ok, launched, out, err) = open(
            &["https://github.com/mk-system/trier", "https://example.com"],
            &example_path(),
            Path::new(NO_STATE),
        );
        assert!(ok, "{err}");
        assert_eq!(
            launched,
            [
                "org.mozilla.firefoxdeveloperedition https://github.com/mk-system/trier",
                "com.apple.Safari https://example.com/",
            ]
        );
        assert_eq!(
            out,
            "https://github.com/mk-system/trier -> firefox@developer-edition (org.mozilla.firefoxdeveloperedition)\n\
             https://example.com/ -> safari (com.apple.Safari)\n"
        );
    }

    #[test]
    fn failing_url_does_not_stop_the_rest() {
        let (ok, launched, _, err) = open(
            &[
                "mailto:me@example.com",
                "https://fail.example/",
                "https://example.com",
            ],
            &example_path(),
            Path::new(NO_STATE),
        );
        assert!(!ok);
        assert_eq!(launched, ["com.apple.Safari https://example.com/"]);
        assert!(
            err.contains("error: mailto:me@example.com: unsupported URL scheme"),
            "{err}"
        );
        assert!(
            err.contains("error: https://fail.example/: failed to run fake-open: refused"),
            "{err}"
        );
    }

    #[test]
    fn recorded_default_is_labelled_in_output() {
        let config = temp_file("open", "no-default.toml", "version = 1\n");
        let state = temp_file(
            "open",
            "state.toml",
            "previous_default_browser_bundle_id = \"com.example.Browser\"\n",
        );
        let (ok, _, out, _) = open(&["https://example.com/"], &config, &state);
        assert!(ok);
        assert_eq!(
            out,
            "https://example.com/ -> recorded system default (com.example.Browser)\n"
        );
    }

    #[test]
    fn broken_state_is_warned_about_once() {
        let config = temp_file("open", "no-default-broken.toml", "version = 1\n");
        let state = temp_file(
            "open",
            "broken.toml",
            "previous_default_browser_bundle_id = 1\n",
        );
        let (ok, _, _, err) = open(
            &["https://a.example/", "https://b.example/"],
            &config,
            &state,
        );
        assert!(!ok);
        assert_eq!(
            err.matches("warning: ignoring the recorded system default")
                .count(),
            1,
            "{err}"
        );
        assert_eq!(err.matches("error: https://").count(), 2, "{err}");
    }

    #[test]
    fn config_error_opens_nothing() {
        let config = temp_file("open", "invalid.toml", "version = 2\n");
        let (ok, launched, _, err) = open(&["https://example.com/"], &config, Path::new(NO_STATE));
        assert!(!ok);
        assert!(launched.is_empty());
        assert!(err.contains("unsupported config version 2"), "{err}");
    }

    #[test]
    fn errors_stay_in_order_with_results() {
        let inputs = [
            "https://example.com/a".to_owned(),
            "mailto:me@example.com".to_owned(),
            "https://example.com/b".to_owned(),
        ];
        let mut report = Report::default();
        run(
            &inputs,
            &example_path(),
            Ok(PathBuf::from(NO_STATE)),
            &mut |_: &BundleId, _: &Url| Ok(()),
            &mut report,
        );
        let streams: Vec<Stream> = report.lines().iter().map(|(s, _)| *s).collect();
        assert_eq!(streams, [Stream::Out, Stream::Err, Stream::Out]);
    }
}
