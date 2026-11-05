use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::browser::{BrowserName, BrowserRegistry, BundleId};
use crate::pattern::HostPattern;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: ConfigVersion,
    pub default_browser: Option<BrowserName>,
    #[serde(default)]
    pub browsers: BTreeMap<BrowserName, CustomBrowser>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub hosts: Vec<HostPattern>,
    pub browser: BrowserName,
    #[serde(default)]
    pub include_subdomains: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomBrowser {
    pub bundle_id: BundleId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u64")]
pub struct ConfigVersion;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unsupported config version {0}: only version 1 is supported")]
pub struct UnsupportedVersion(u64);

impl TryFrom<u64> for ConfigVersion {
    type Error = UnsupportedVersion;

    fn try_from(version: u64) -> Result<Self, Self::Error> {
        match version {
            1 => Ok(Self),
            other => Err(UnsupportedVersion(other)),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Parse(#[from] toml::de::Error),
    #[error("{}", Violations(.0))]
    Invalid(Vec<Violation>),
}

/// A cross-field rule that deserialization alone cannot enforce.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Violation {
    #[error("rules[{index}].hosts must not be empty")]
    EmptyHosts { index: usize },
    #[error(
        "{location}: unknown browser `{name}` (use a built-in name or define it under [browsers])"
    )]
    UnknownBrowser { location: String, name: BrowserName },
}

struct Violations<'a>(&'a [Violation]);

impl fmt::Display for Violations<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, v) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{v}")?;
        }
        Ok(())
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        std::fs::read_to_string(path)
            .map_err(|source| ConfigError::Read {
                path: path.to_owned(),
                source,
            })?
            .parse()
    }

    pub fn registry(&self) -> BrowserRegistry {
        BrowserRegistry::new(&self.browsers)
    }

    fn violations(&self) -> Vec<Violation> {
        let registry = self.registry();
        let unknown = |location: String, name: &BrowserName| {
            registry
                .resolve(name)
                .is_none()
                .then(|| Violation::UnknownBrowser {
                    location,
                    name: name.clone(),
                })
        };

        let mut violations: Vec<Violation> = self
            .default_browser
            .iter()
            .filter_map(|name| unknown("default_browser".to_owned(), name))
            .collect();
        for (index, rule) in self.rules.iter().enumerate() {
            if rule.hosts.is_empty() {
                violations.push(Violation::EmptyHosts { index });
            }
            violations.extend(unknown(format!("rules[{index}].browser"), &rule.browser));
        }
        violations
    }
}

impl std::str::FromStr for Config {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: Self = toml::from_str(s)?;
        let violations = config.violations();
        if violations.is_empty() {
            Ok(config)
        } else {
            Err(ConfigError::Invalid(violations))
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../config/config.toml.example");

    fn parse(s: &str) -> Result<Config, ConfigError> {
        s.parse()
    }

    fn parse_error(s: &str) -> String {
        parse(s).unwrap_err().to_string()
    }

    fn name(s: &str) -> BrowserName {
        BrowserName::try_from(s.to_owned()).unwrap()
    }

    #[test]
    fn accepts_example_file() {
        let config = parse(EXAMPLE).unwrap();
        assert!(!config.rules.is_empty());
        assert!(config.default_browser.is_some());
    }

    #[test]
    fn accepts_minimal_config() {
        let config = parse("version = 1").unwrap();
        assert_eq!(config.default_browser, None);
        assert!(config.browsers.is_empty());
        assert!(config.rules.is_empty());
    }

    #[test]
    fn include_subdomains_defaults_to_false() {
        let config = parse(
            r#"
            version = 1
            [[rules]]
            hosts = ["github.com"]
            browser = "safari"
            "#,
        )
        .unwrap();
        assert!(!config.rules[0].include_subdomains);
    }

    #[test]
    fn requires_version() {
        assert!(parse_error("").contains("missing field `version`"));
    }

    #[test]
    fn rejects_version_other_than_1() {
        assert!(parse_error("version = 2").contains("unsupported config version 2"));
    }

    #[test]
    fn rejects_unknown_top_level_key() {
        assert!(parse_error("version = 1\nfoo = 1").contains("unknown field `foo`"));
    }

    #[test]
    fn rejects_unknown_rule_key() {
        let err = parse_error(
            r#"
            version = 1
            [[rules]]
            hosts = ["github.com"]
            browser = "safari"
            subdomains = true
            "#,
        );
        assert!(err.contains("unknown field `subdomains`"), "{err}");
    }

    #[test]
    fn rejects_unknown_custom_browser_key() {
        let err = parse_error(
            r#"
            version = 1
            [browsers.foo]
            bundle_id = "com.example.foo"
            path = "/Applications/Foo.app"
            "#,
        );
        assert!(err.contains("unknown field `path`"), "{err}");
    }

    #[test]
    fn rejects_empty_hosts() {
        let err = parse(
            r#"
            version = 1
            [[rules]]
            hosts = []
            browser = "safari"
            "#,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid(v) if v == [Violation::EmptyHosts { index: 0 }]
        ));
    }

    #[test]
    fn rejects_unknown_rule_browser() {
        let err = parse(
            r#"
            version = 1
            [[rules]]
            hosts = ["github.com"]
            browser = "netscape"
            "#,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "rules[0].browser: unknown browser `netscape` (use a built-in name or define it under [browsers])"
        );
    }

    #[test]
    fn rejects_unknown_default_browser() {
        let err = parse_error("version = 1\ndefault_browser = \"netscape\"");
        assert!(err.starts_with("default_browser: unknown browser `netscape`"));
    }

    #[test]
    fn reports_every_violation_at_once() {
        let err = parse(
            r#"
            version = 1
            default_browser = "netscape"
            [[rules]]
            hosts = []
            browser = "mosaic"
            "#,
        )
        .unwrap_err();
        let ConfigError::Invalid(violations) = err else {
            panic!("expected Invalid, got {err:?}");
        };
        assert_eq!(violations.len(), 3);
    }

    #[test]
    fn rejects_invalid_browser_name_characters() {
        let err = parse_error("version = 1\ndefault_browser = \"Firefox\"");
        assert!(err.contains("invalid browser name `Firefox`"), "{err}");
    }

    #[test]
    fn rejects_invalid_custom_browser_key() {
        let err = parse_error(
            r#"
            version = 1
            [browsers."My Browser"]
            bundle_id = "com.example.browser"
            "#,
        );
        assert!(err.contains("invalid browser name `My Browser`"), "{err}");
    }

    #[test]
    fn rejects_empty_bundle_id() {
        let err = parse_error(
            r#"
            version = 1
            [browsers.foo]
            bundle_id = ""
            "#,
        );
        assert!(err.contains("bundle_id must not be empty"), "{err}");
    }

    #[test]
    fn rejects_invalid_host_pattern() {
        let err = parse_error(
            r#"
            version = 1
            [[rules]]
            hosts = ["https://github.com"]
            browser = "safari"
            "#,
        );
        assert!(err.contains("must not contain a scheme"), "{err}");
    }

    #[test]
    fn custom_browser_can_be_referenced_by_rules() {
        let config = parse(
            r#"
            version = 1
            default_browser = "firefox@dev"
            [browsers."firefox@dev"]
            bundle_id = "org.mozilla.firefoxdeveloperedition"
            "#,
        )
        .unwrap();
        assert_eq!(
            config
                .registry()
                .resolve(&name("firefox@dev"))
                .map(BundleId::as_str),
            Some("org.mozilla.firefoxdeveloperedition")
        );
    }

    #[test]
    fn custom_browser_overrides_builtin() {
        let config = parse(
            r#"
            version = 1
            [browsers.safari]
            bundle_id = "com.apple.SafariTechnologyPreview"
            "#,
        )
        .unwrap();
        assert_eq!(
            config
                .registry()
                .resolve(&name("safari"))
                .map(BundleId::as_str),
            Some("com.apple.SafariTechnologyPreview")
        );
    }

    #[test]
    fn load_reports_missing_file() {
        let err = Config::load(Path::new("/nonexistent/trier/config.toml")).unwrap_err();
        assert!(matches!(err, ConfigError::Read { .. }));
        assert!(
            err.to_string()
                .starts_with("failed to read /nonexistent/trier/config.toml")
        );
    }
}
