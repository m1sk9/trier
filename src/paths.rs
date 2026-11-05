//! XDG-style path resolution.
//!
//! `dirs`-style crates are deliberately not used: they map macOS to
//! `~/Library/Application Support`, while trier keeps its files under
//! `~/.config` and `~/.local/state` on every platform.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const CONFIG_ENV: &str = "TRIER_CONFIG";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathsError {
    #[error(
        "cannot locate the {what} directory: {xdg_var} is unset or not absolute, and HOME is not set"
    )]
    NoHome {
        what: &'static str,
        xdg_var: &'static str,
    },
}

/// `--config` > `$TRIER_CONFIG` > `$XDG_CONFIG_HOME/trier/config.toml` > `$HOME/.config/trier/config.toml`.
pub fn config_path(flag: Option<&Path>) -> Result<PathBuf, PathsError> {
    if let Some(path) = flag {
        return Ok(path.to_owned());
    }
    if let Some(path) = non_empty_var(CONFIG_ENV) {
        return Ok(PathBuf::from(path));
    }
    Ok(base_dir("XDG_CONFIG_HOME", ".config", "config")?.join("trier/config.toml"))
}

/// `$XDG_STATE_HOME/trier` > `$HOME/.local/state/trier`.
pub fn state_dir() -> Result<PathBuf, PathsError> {
    Ok(base_dir("XDG_STATE_HOME", ".local/state", "state")?.join("trier"))
}

pub fn log_file() -> Result<PathBuf, PathsError> {
    Ok(state_dir()?.join("trier.log"))
}

fn base_dir(
    xdg_var: &'static str,
    home_fallback: &str,
    what: &'static str,
) -> Result<PathBuf, PathsError> {
    // The XDG spec says relative values must be ignored.
    if let Some(dir) = non_empty_var(xdg_var).map(PathBuf::from)
        && dir.is_absolute()
    {
        return Ok(dir);
    }
    non_empty_var("HOME")
        .map(|home| PathBuf::from(home).join(home_fallback))
        .ok_or(PathsError::NoHome { what, xdg_var })
}

fn non_empty_var(key: &str) -> Option<OsString> {
    std::env::var_os(key).filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::*;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const VARS: [&str; 4] = [CONFIG_ENV, "XDG_CONFIG_HOME", "XDG_STATE_HOME", "HOME"];

    /// Holds the env lock and restores every variable in `VARS` on drop.
    struct EnvGuard {
        saved: Vec<(&'static str, Option<OsString>)>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        /// Clears all variables in `VARS`, then applies `vars`.
        fn new(vars: &[(&str, &str)]) -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let saved = VARS.iter().map(|k| (*k, std::env::var_os(k))).collect();
            // SAFETY: every test touching the environment holds ENV_LOCK.
            unsafe {
                for key in VARS {
                    std::env::remove_var(key);
                }
                for (key, value) in vars {
                    std::env::set_var(key, value);
                }
            }
            Self { saved, _lock: lock }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: ENV_LOCK is still held by `_lock`.
            unsafe {
                for (key, value) in &self.saved {
                    match value {
                        Some(v) => std::env::set_var(key, v),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }

    const ALL_SET: &[(&str, &str)] = &[
        (CONFIG_ENV, "/env/config.toml"),
        ("XDG_CONFIG_HOME", "/xdg/config"),
        ("XDG_STATE_HOME", "/xdg/state"),
        ("HOME", "/home/me"),
    ];

    #[test]
    fn flag_takes_precedence_over_everything() {
        let _env = EnvGuard::new(ALL_SET);
        assert_eq!(
            config_path(Some(Path::new("/flag/config.toml"))),
            Ok(PathBuf::from("/flag/config.toml"))
        );
    }

    #[test]
    fn env_var_takes_precedence_over_xdg_and_home() {
        let _env = EnvGuard::new(ALL_SET);
        assert_eq!(config_path(None), Ok(PathBuf::from("/env/config.toml")));
    }

    #[test]
    fn xdg_config_home_takes_precedence_over_home() {
        let _env = EnvGuard::new(&[("XDG_CONFIG_HOME", "/xdg/config"), ("HOME", "/home/me")]);
        assert_eq!(
            config_path(None),
            Ok(PathBuf::from("/xdg/config/trier/config.toml"))
        );
    }

    #[test]
    fn falls_back_to_home_dot_config() {
        let _env = EnvGuard::new(&[("HOME", "/home/me")]);
        assert_eq!(
            config_path(None),
            Ok(PathBuf::from("/home/me/.config/trier/config.toml"))
        );
    }

    #[test]
    fn empty_variables_are_treated_as_unset() {
        let _env = EnvGuard::new(&[
            (CONFIG_ENV, ""),
            ("XDG_CONFIG_HOME", ""),
            ("HOME", "/home/me"),
        ]);
        assert_eq!(
            config_path(None),
            Ok(PathBuf::from("/home/me/.config/trier/config.toml"))
        );
    }

    #[test]
    fn relative_xdg_config_home_is_ignored() {
        let _env = EnvGuard::new(&[("XDG_CONFIG_HOME", "rel/config"), ("HOME", "/home/me")]);
        assert_eq!(
            config_path(None),
            Ok(PathBuf::from("/home/me/.config/trier/config.toml"))
        );
    }

    #[test]
    fn config_path_fails_without_xdg_or_home() {
        let _env = EnvGuard::new(&[]);
        assert_eq!(
            config_path(None),
            Err(PathsError::NoHome {
                what: "config",
                xdg_var: "XDG_CONFIG_HOME"
            })
        );
    }

    #[test]
    fn state_dir_prefers_xdg_state_home() {
        let _env = EnvGuard::new(ALL_SET);
        assert_eq!(state_dir(), Ok(PathBuf::from("/xdg/state/trier")));
    }

    #[test]
    fn state_dir_falls_back_to_home_dot_local_state() {
        let _env = EnvGuard::new(&[("HOME", "/home/me")]);
        assert_eq!(
            state_dir(),
            Ok(PathBuf::from("/home/me/.local/state/trier"))
        );
    }

    #[test]
    fn log_file_lives_in_state_dir() {
        let _env = EnvGuard::new(&[("XDG_STATE_HOME", "/xdg/state")]);
        assert_eq!(log_file(), Ok(PathBuf::from("/xdg/state/trier/trier.log")));
    }

    #[test]
    fn state_dir_fails_without_xdg_or_home() {
        let _env = EnvGuard::new(&[]);
        assert_eq!(
            state_dir().unwrap_err().to_string(),
            "cannot locate the state directory: XDG_STATE_HOME is unset or not absolute, and HOME is not set"
        );
    }
}
