//! Subcommand implementations. Argument parsing lives in `main.rs`.

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use url::Url;

use crate::browser::{BrowserRegistry, BundleId};
use crate::config::{Config, ConfigError};
use crate::paths::PathsError;
use crate::resolve::{Decision, ResolveError, decide};
use crate::state::State;

pub mod browsers;
pub mod check;
pub mod config;
pub mod explain;
pub mod open;
pub mod register;
pub mod status;
pub mod unregister;

/// Finds where the app with a bundle id is installed.
///
/// Injected so tests do not depend on which apps this machine has;
/// `main.rs` passes [`crate::macos::app_path_for_bundle_id`].
pub type AppLookup<'a> = &'a dyn Fn(&BundleId) -> Option<PathBuf>;

/// Which stream a [`Report`] line goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

/// What a subcommand prints, in the order it was produced.
///
/// Appending to it cannot fail, so subcommands carry no I/O error paths;
/// `main.rs` writes it out once the command finishes.
#[derive(Debug, Default)]
pub struct Report {
    lines: Vec<(Stream, String)>,
    live: bool,
}

impl Report {
    /// A report that prints each line immediately instead of keeping it.
    ///
    /// For commands whose progress must be visible while they run, such as
    /// `register`, which may block on a macOS confirmation dialog. Write
    /// errors are ignored: there is nowhere left to report them.
    pub fn live() -> Self {
        Self {
            lines: Vec::new(),
            live: true,
        }
    }

    pub fn out(&mut self, line: impl fmt::Display) {
        self.push(Stream::Out, line.to_string());
    }

    pub fn err(&mut self, line: impl fmt::Display) {
        self.push(Stream::Err, line.to_string());
    }

    fn push(&mut self, stream: Stream, line: String) {
        if !self.live {
            self.lines.push((stream, line));
            return;
        }
        let _ = match stream {
            Stream::Out => writeln!(io::stdout(), "{line}"),
            Stream::Err => writeln!(io::stderr(), "{line}"),
        };
    }

    pub fn lines(&self) -> &[(Stream, String)] {
        &self.lines
    }

    /// Everything written to stdout, one line each.
    pub fn stdout(&self) -> String {
        self.joined(Stream::Out)
    }

    /// Everything written to stderr, one line each.
    pub fn stderr(&self) -> String {
        self.joined(Stream::Err)
    }

    fn joined(&self, stream: Stream) -> String {
        self.lines
            .iter()
            .filter(|(s, _)| *s == stream)
            .map(|(_, line)| format!("{line}\n"))
            .collect()
    }
}

/// Loads the config, reporting `error:` lines and returning `None` on failure.
pub fn load_config(path: &Path, report: &mut Report) -> Option<Config> {
    Config::load(path)
        .inspect_err(|e| report_config_error(path, e, report))
        .ok()
}

/// Reports one `error:` line per problem in `e`.
pub fn report_config_error(path: &Path, e: &ConfigError, report: &mut Report) {
    match e {
        ConfigError::Invalid(violations) => {
            for v in violations {
                report.err(format_args!("error: {}: {v}", path.display()));
            }
        }
        ConfigError::Read { .. } => report.err(format_args!("error: {e}")),
        ConfigError::Parse(_) => report.err(format_args!("error: {}: {e}", path.display())),
    }
}

/// Decides browsers for URL arguments.
///
/// The state file is read only when no rule or `default_browser` decides, and
/// at most once, so a missing or broken state never blocks a URL the config
/// resolves and its warning is not repeated per URL.
pub struct UrlResolver<'a> {
    config: &'a Config,
    registry: BrowserRegistry,
    state_path: Result<PathBuf, PathsError>,
    recorded_default: Option<Option<BundleId>>,
}

impl<'a> UrlResolver<'a> {
    pub fn new(config: &'a Config, state_path: Result<PathBuf, PathsError>) -> Self {
        Self {
            config,
            registry: config.registry(),
            state_path,
            recorded_default: None,
        }
    }

    /// Parses and decides `input`; the error is a message ready to print.
    pub fn resolve(&mut self, input: &str, report: &mut Report) -> Result<Decision, String> {
        let url = match Url::parse(input) {
            Ok(url) => url,
            Err(e) => {
                return Err(format!("invalid URL `{input}`: {e}{}", scheme_hint(input)));
            }
        };
        let decision = match decide(&url, self.config, &self.registry, None) {
            Err(ResolveError::NoFallback(_)) => {
                let recorded = self.recorded_default(report);
                decide(&url, self.config, &self.registry, recorded.as_ref())
            }
            other => other,
        };
        decision.map_err(|e| match e {
            ResolveError::UnsupportedScheme(_) if url.cannot_be_a_base() => {
                format!("{e}{}", scheme_hint(input))
            }
            e => e.to_string(),
        })
    }

    /// The recorded system default, or `None` with a warning when the state
    /// cannot be located or read.
    fn recorded_default(&mut self, report: &mut Report) -> Option<BundleId> {
        if let Some(recorded) = &self.recorded_default {
            return recorded.clone();
        }
        let recorded = match State::load_located(self.state_path.as_deref()) {
            Ok(state) => state.previous_default_browser_bundle_id,
            Err(e) => {
                report.err(format_args!(
                    "warning: ignoring the recorded system default: {e}"
                ));
                None
            }
        };
        self.recorded_default = Some(recorded.clone());
        recorded
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
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn report_keeps_lines_in_the_order_they_were_produced() {
        let mut report = Report::default();
        report.out("a");
        report.err("b");
        report.out("c");
        assert_eq!(
            report.lines(),
            [
                (Stream::Out, "a".to_owned()),
                (Stream::Err, "b".to_owned()),
                (Stream::Out, "c".to_owned()),
            ]
        );
        assert_eq!(report.stdout(), "a\nc\n");
        assert_eq!(report.stderr(), "b\n");
    }

    #[test]
    fn live_report_keeps_nothing() {
        let mut report = Report::live();
        report.out("live stdout line from a test");
        report.err("live stderr line from a test");
        assert!(report.lines().is_empty());
    }
}
