use std::io::{self, Write};
use std::path::Path;

use crate::browser::Origin;
use crate::cli::{AppLookup, load_config};

/// Prints every known browser: name, origin, bundle id and install path.
///
/// Returns whether the config could be loaded.
pub fn run(
    config_path: &Path,
    lookup: AppLookup,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<bool> {
    let Some(config) = load_config(config_path, err)? else {
        return Ok(false);
    };
    let registry = config.registry();
    let rows: Vec<[String; 4]> = registry
        .iter()
        .map(|(name, bundle_id, origin)| {
            let origin = match origin {
                Origin::BuiltIn => "built-in",
                Origin::Custom => "custom",
            };
            // `-` instead of "not installed" when the lookup is unsupported
            // (off macOS): that would be a false claim.
            let app = match lookup(bundle_id) {
                Ok(Some(path)) => path.display().to_string(),
                Ok(None) => "not installed".to_owned(),
                Err(_) => "-".to_owned(),
            };
            [
                name.to_string(),
                origin.to_owned(),
                bundle_id.to_string(),
                app,
            ]
        })
        .collect();

    let header = ["NAME", "ORIGIN", "BUNDLE ID", "APP"].map(str::to_owned);
    let mut widths = [0; 3];
    for row in std::iter::once(&header).chain(&rows) {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.len());
        }
    }
    for [name, origin, bundle_id, app] in std::iter::once(&header).chain(&rows) {
        writeln!(
            out,
            "{name:<w0$}  {origin:<w1$}  {bundle_id:<w2$}  {app}",
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
        )?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::test_support::{lookup_installed, temp_file, unsupported_lookup};

    fn browsers(config: &Path, lookup: AppLookup) -> (bool, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let ok = run(config, lookup, &mut out, &mut err).unwrap();
        (
            ok,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    /// `name` must be unique per test: tests run in parallel and would
    /// otherwise read each other's half-written file.
    fn config_with_custom(name: &str) -> PathBuf {
        temp_file(
            "browsers",
            name,
            "version = 1\n[browsers.\"firefox@dev\"]\nbundle_id = \"org.mozilla.firefoxdeveloperedition\"\n",
        )
    }

    fn row<'a>(out: &'a str, name: &str) -> Vec<&'a str> {
        out.lines()
            .map(|l| l.split_whitespace().collect::<Vec<_>>())
            .find(|cols| cols[0] == name)
            .unwrap_or_else(|| panic!("no row for {name} in\n{out}"))
    }

    #[test]
    fn lists_builtin_and_custom_browsers_sorted_by_name() {
        let (ok, out, _) = browsers(&config_with_custom("sorted.toml"), &unsupported_lookup);
        assert!(ok);
        let names: Vec<&str> = out
            .lines()
            .skip(1)
            .map(|l| l.split_whitespace().next().unwrap())
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        assert_eq!(
            row(&out, "firefox@dev"),
            [
                "firefox@dev",
                "custom",
                "org.mozilla.firefoxdeveloperedition",
                "-"
            ]
        );
        assert_eq!(
            row(&out, "safari"),
            ["safari", "built-in", "com.apple.Safari", "-"]
        );
    }

    #[test]
    fn shows_install_path_or_not_installed() {
        let (_, out, _) = browsers(
            &config_with_custom("paths.toml"),
            &lookup_installed(&["com.apple.Safari"]),
        );
        assert_eq!(row(&out, "safari")[3], "/Applications/com.apple.Safari.app");
        assert_eq!(&row(&out, "firefox")[3..], ["not", "installed"]);
    }

    #[test]
    fn columns_are_aligned() {
        let (_, out, _) = browsers(&config_with_custom("aligned.toml"), &unsupported_lookup);
        let origin_column: Vec<usize> = out
            .lines()
            .map(|l| {
                l.find("built-in")
                    .or_else(|| l.find("custom"))
                    .or_else(|| l.find("ORIGIN"))
                    .unwrap()
            })
            .collect();
        assert!(origin_column.windows(2).all(|w| w[0] == w[1]), "{out}");
    }

    #[test]
    fn config_errors_are_reported() {
        let config = temp_file("browsers", "invalid.toml", "version = 2\n");
        let (ok, _, err) = browsers(&config, &unsupported_lookup);
        assert!(!ok);
        assert!(err.contains("unsupported config version 2"), "{err}");
    }
}
