//! Subcommand implementations. Argument parsing lives in `main.rs`.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use url::Url;

use crate::browser::{BrowserRegistry, BundleId};
use crate::config::{Config, ConfigError};
use crate::macos::MacosError;
use crate::paths::PathsError;
use crate::resolve::{Decision, ResolveError, decide};
use crate::state::State;

pub mod browsers;
pub mod check;
pub mod config;
pub mod explain;
pub mod open;

/// Finds where the app with a bundle id is installed.
///
/// Injected so the subcommands can be tested off macOS;
/// `main.rs` passes [`crate::macos::app_path_for_bundle_id`].
pub type AppLookup<'a> = &'a dyn Fn(&BundleId) -> Result<Option<PathBuf>, MacosError>;

/// Loads the config, writing `error:` lines and returning `None` on failure.
pub fn load_config(path: &Path, err: &mut dyn Write) -> io::Result<Option<Config>> {
    match Config::load(path) {
        Ok(config) => Ok(Some(config)),
        Err(e) => {
            report_config_error(path, &e, err)?;
            Ok(None)
        }
    }
}

/// Writes one `error:` line per problem in `e`.
pub fn report_config_error(path: &Path, e: &ConfigError, err: &mut dyn Write) -> io::Result<()> {
    match e {
        ConfigError::Invalid(violations) => {
            for v in violations {
                writeln!(err, "error: {}: {v}", path.display())?;
            }
            Ok(())
        }
        ConfigError::Read { .. } => writeln!(err, "error: {e}"),
        ConfigError::Parse(_) => writeln!(err, "error: {}: {e}", path.display()),
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
    state_path: Option<Result<PathBuf, PathsError>>,
    recorded_default: Option<Option<BundleId>>,
}

impl<'a> UrlResolver<'a> {
    pub fn new(config: &'a Config, state_path: Result<PathBuf, PathsError>) -> Self {
        Self {
            config,
            registry: config.registry(),
            state_path: Some(state_path),
            recorded_default: None,
        }
    }

    /// Parses and decides `input`; the error is a message ready to print.
    pub fn resolve(
        &mut self,
        input: &str,
        err: &mut dyn Write,
    ) -> io::Result<Result<Decision, String>> {
        let url = match Url::parse(input) {
            Ok(url) => url,
            Err(e) => {
                return Ok(Err(format!(
                    "invalid URL `{input}`: {e}{}",
                    scheme_hint(input)
                )));
            }
        };
        let decision = match decide(&url, self.config, &self.registry, None) {
            Err(ResolveError::NoFallback(_)) => {
                let recorded = self.recorded_default(err)?;
                decide(&url, self.config, &self.registry, recorded.as_ref())
            }
            other => other,
        };
        Ok(decision.map_err(|e| match e {
            ResolveError::UnsupportedScheme(_) if url.cannot_be_a_base() => {
                format!("{e}{}", scheme_hint(input))
            }
            e => e.to_string(),
        }))
    }

    /// The recorded system default, or `None` with a warning when the state
    /// cannot be located or read.
    fn recorded_default(&mut self, err: &mut dyn Write) -> io::Result<Option<BundleId>> {
        if let Some(recorded) = &self.recorded_default {
            return Ok(recorded.clone());
        }
        let state = self
            .state_path
            .take()
            .expect("state path is consumed only once")
            .map_err(|e| e.to_string())
            .and_then(|path| State::load(&path).map_err(|e| e.to_string()));
        let recorded = match state {
            Ok(state) => state.previous_default_browser_bundle_id,
            Err(e) => {
                writeln!(err, "warning: ignoring the recorded system default: {e}")?;
                None
            }
        };
        self.recorded_default = Some(recorded.clone());
        Ok(recorded)
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
