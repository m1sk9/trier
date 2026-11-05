use std::io::{self, Write};
use std::path::Path;

use crate::browser::builtin_bundle_id;
use crate::config::{Config, ConfigError};

/// Loads and validates the config at `path`, reporting to `out` / `err`.
///
/// Returns whether the config is valid.
pub fn run(path: &Path, out: &mut dyn Write, err: &mut dyn Write) -> io::Result<bool> {
    let config = match Config::load(path) {
        Ok(config) => config,
        Err(ConfigError::Invalid(violations)) => {
            for v in violations {
                writeln!(err, "error: {}: {v}", path.display())?;
            }
            return Ok(false);
        }
        Err(e) => {
            writeln!(err, "error: {}: {e}", path.display())?;
            return Ok(false);
        }
    };

    for (name, custom) in &config.browsers {
        if let Some(builtin) = builtin_bundle_id(name) {
            writeln!(
                out,
                "info: [browsers.\"{name}\"] overrides the built-in `{name}` ({builtin} -> {})",
                custom.bundle_id
            )?;
        }
    }
    writeln!(out, "OK: {}", path.display())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn example_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("config/config.toml.example")
    }

    fn write_config(name: &str, content: &str) -> PathBuf {
        // Not `std::env::temp_dir()`: it reads TMPDIR at runtime, racing with
        // the env-mutating tests in `paths`.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/tmp/check");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn check(path: &Path) -> (bool, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let ok = run(path, &mut out, &mut err).unwrap();
        (
            ok,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn valid_config_prints_ok_with_path() {
        let path = example_path();
        let (ok, out, err) = check(&path);
        assert!(ok);
        assert!(out.ends_with(&format!("OK: {}\n", path.display())), "{out}");
        assert_eq!(err, "");
    }

    #[test]
    fn overriding_builtin_browser_is_reported_as_info() {
        let path = write_config(
            "override.toml",
            "version = 1\n[browsers.safari]\nbundle_id = \"com.apple.SafariTechnologyPreview\"\n",
        );
        let (ok, out, _) = check(&path);
        assert!(ok);
        assert!(
            out.contains(
                "info: [browsers.\"safari\"] overrides the built-in `safari` (com.apple.Safari -> com.apple.SafariTechnologyPreview)"
            ),
            "{out}"
        );
    }

    #[test]
    fn each_violation_is_printed_on_its_own_line() {
        let path = write_config(
            "violations.toml",
            "version = 1\ndefault_browser = \"netscape\"\n[[rules]]\nhosts = []\nbrowser = \"safari\"\n",
        );
        let (ok, out, err) = check(&path);
        assert!(!ok);
        assert_eq!(out, "");
        let lines: Vec<_> = err.lines().collect();
        assert_eq!(lines.len(), 2, "{err}");
        assert!(lines.iter().all(|l| l.starts_with("error: ")));
        assert!(lines[0].contains("default_browser: unknown browser `netscape`"));
        assert!(lines[1].contains("rules[0].hosts must not be empty"));
    }

    #[test]
    fn toml_errors_include_the_source_span() {
        let path = write_config("syntax.toml", "version = 1\nrules = [\n");
        let (ok, _, err) = check(&path);
        assert!(!ok);
        assert!(err.contains("line 2"), "{err}");
    }

    #[test]
    fn missing_file_is_an_error() {
        let (ok, _, err) = check(Path::new("/nonexistent/trier/config.toml"));
        assert!(!ok);
        assert!(err.contains("failed to read"), "{err}");
    }
}
