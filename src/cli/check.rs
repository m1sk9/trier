use std::io::{self, Write};
use std::path::Path;

use crate::browser::builtin_bundle_id;
use crate::config::{Config, ConfigError, Rule};
use crate::pattern::covers;

/// Loads and validates the config at `path`, reporting to `out` / `err`.
///
/// Returns whether the config is valid.
pub fn run(path: &Path, out: &mut dyn Write, err: &mut dyn Write) -> io::Result<bool> {
    let config = match Config::load(path) {
        Ok(config) => config,
        Err(e) => {
            report_config_error(path, &e, err)?;
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
    for (shadowed, by) in shadowed_rules(&config.rules) {
        writeln!(
            out,
            "warning: rules[{shadowed}] never matches: every host is already matched by rules[{by}]"
        )?;
    }
    writeln!(out, "OK: {}", path.display())?;
    Ok(true)
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

/// `(shadowed, by)` index pairs: rule `shadowed` can never match because the
/// earlier rule `by` covers every one of its host patterns.
///
/// A rule covered only by several earlier rules together is not reported;
/// first-match order keeps `explain` simple, and this check stays pairwise.
fn shadowed_rules(rules: &[Rule]) -> Vec<(usize, usize)> {
    rules
        .iter()
        .enumerate()
        .filter_map(|(j, later)| {
            let by = rules[..j].iter().position(|earlier| {
                later.hosts.iter().all(|b| {
                    earlier
                        .hosts
                        .iter()
                        .any(|a| covers(a, earlier.include_subdomains, b, later.include_subdomains))
                })
            })?;
            Some((j, by))
        })
        .collect()
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
        assert!(
            err.starts_with("error: failed to read /nonexistent/trier/config.toml: "),
            "{err}"
        );
        assert_eq!(
            err.matches("/nonexistent/trier/config.toml").count(),
            1,
            "{err}"
        );
    }

    #[test]
    fn rule_covered_by_an_earlier_rule_is_reported_as_shadowed() {
        let path = write_config(
            "shadow.toml",
            "version = 1\n\
             [[rules]]\nhosts = [\"github.com\"]\nbrowser = \"safari\"\ninclude_subdomains = true\n\
             [[rules]]\nhosts = [\"example.com\"]\nbrowser = \"safari\"\n\
             [[rules]]\nhosts = [\"gist.github.com\", \"github.com/mk-system/**\"]\nbrowser = \"firefox\"\n",
        );
        let (ok, out, _) = check(&path);
        assert!(ok, "shadowing is a warning, not an error");
        assert!(
            out.contains(
                "warning: rules[2] never matches: every host is already matched by rules[0]"
            ),
            "{out}"
        );
        assert_eq!(out.matches("warning:").count(), 1, "{out}");
    }

    #[test]
    fn rule_only_partly_covered_is_not_shadowed() {
        let path = write_config(
            "partial.toml",
            "version = 1\n\
             [[rules]]\nhosts = [\"github.com/mk-system/**\"]\nbrowser = \"safari\"\n\
             [[rules]]\nhosts = [\"github.com/mk-system/trier\", \"gitlab.com\"]\nbrowser = \"firefox\"\n",
        );
        let (ok, out, _) = check(&path);
        assert!(ok);
        assert!(!out.contains("warning:"), "{out}");
    }

    #[test]
    fn example_config_has_no_shadowed_rules() {
        let (_, out, _) = check(&example_path());
        assert!(!out.contains("warning:"), "{out}");
    }
}
