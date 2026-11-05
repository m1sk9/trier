use std::fmt;

use url::Url;

use crate::browser::{BrowserName, BrowserRegistry, BundleId};
use crate::config::Config;
use crate::pattern::HostPattern;

/// Why a browser was chosen for a URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionSource {
    Rule { index: usize, pattern: HostPattern },
    DefaultBrowser,
    RecordedSystemDefault,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub url: Url,
    pub source: DecisionSource,
    /// `None` when the bundle id comes from the recorded system default.
    pub browser: Option<BrowserName>,
    pub bundle_id: BundleId,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    #[error("unsupported URL scheme `{0}`: only http and https are handled")]
    UnsupportedScheme(String),
    #[error("URL `{0}` has no host")]
    NoHost(Url),
    #[error("browser `{0}` is not defined")]
    UnknownBrowser(BrowserName),
    #[error(
        "no rule matches `{0}` and there is no fallback: set `default_browser` in the config or run `trier register`"
    )]
    NoFallback(Url),
}

/// Chooses the browser for `url`: the first matching rule in file order,
/// then `default_browser`, then the system default recorded at registration.
pub fn decide(
    url: &Url,
    config: &Config,
    registry: &BrowserRegistry,
    recorded_default: Option<&BundleId>,
) -> Result<Decision, ResolveError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ResolveError::UnsupportedScheme(url.scheme().to_owned()));
    }
    let host = url
        .host_str()
        .ok_or_else(|| ResolveError::NoHost(url.clone()))?;

    let by_name = |source: DecisionSource, name: &BrowserName| {
        let bundle_id = registry
            .resolve(name)
            .ok_or_else(|| ResolveError::UnknownBrowser(name.clone()))?;
        Ok(Decision {
            url: url.clone(),
            source,
            browser: Some(name.clone()),
            bundle_id: bundle_id.clone(),
        })
    };

    for (index, rule) in config.rules.iter().enumerate() {
        if let Some(pattern) = rule
            .hosts
            .iter()
            .find(|p| p.matches(host, url.path(), rule.include_subdomains))
        {
            let source = DecisionSource::Rule {
                index,
                pattern: pattern.clone(),
            };
            return by_name(source, &rule.browser);
        }
    }
    if let Some(name) = &config.default_browser {
        return by_name(DecisionSource::DefaultBrowser, name);
    }
    if let Some(bundle_id) = recorded_default {
        return Ok(Decision {
            url: url.clone(),
            source: DecisionSource::RecordedSystemDefault,
            browser: None,
            bundle_id: bundle_id.clone(),
        });
    }
    Err(ResolveError::NoFallback(url.clone()))
}

impl fmt::Display for DecisionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rule { index, pattern } => write!(f, "rules[{index}] ({pattern})"),
            Self::DefaultBrowser => f.write_str("default_browser"),
            Self::RecordedSystemDefault => f.write_str("recorded system default"),
        }
    }
}

impl fmt::Display for Decision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "url:       {}", self.url)?;
        writeln!(f, "matched:   {}", self.source)?;
        match &self.browser {
            Some(name) => writeln!(f, "browser:   {name}")?,
            None => writeln!(f, "browser:   -")?,
        }
        write!(f, "bundle id: {}", self.bundle_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
        version = 1
        default_browser = "safari"

        [[rules]]
        hosts = ["github.com"]
        browser = "google-chrome"

        [[rules]]
        hosts = ["example.org", "github.com/mk-system/**"]
        browser = "firefox"
    "#;

    fn config(s: &str) -> Config {
        s.parse().unwrap()
    }

    fn bundle_id(s: &str) -> BundleId {
        BundleId::try_from(s.to_owned()).unwrap()
    }

    fn decide_str(
        url: &str,
        config: &Config,
        recorded: Option<&BundleId>,
    ) -> Result<Decision, ResolveError> {
        decide(
            &Url::parse(url).unwrap(),
            config,
            &config.registry(),
            recorded,
        )
    }

    #[test]
    fn first_matching_rule_wins_over_a_later_more_specific_one() {
        let d = decide_str("https://github.com/mk-system/trier", &config(CONFIG), None).unwrap();
        assert_eq!(
            d.source,
            DecisionSource::Rule {
                index: 0,
                pattern: "github.com".parse().unwrap()
            }
        );
        assert_eq!(d.bundle_id.as_str(), "com.google.Chrome");
    }

    #[test]
    fn rule_reports_the_pattern_that_matched() {
        let d = decide_str("http://example.org/", &config(CONFIG), None).unwrap();
        assert_eq!(
            d.source,
            DecisionSource::Rule {
                index: 1,
                pattern: "example.org".parse().unwrap()
            }
        );
        assert_eq!(d.browser.map(|b| b.to_string()).as_deref(), Some("firefox"));
    }

    #[test]
    fn default_browser_is_used_when_no_rule_matches() {
        let d = decide_str("https://example.com/", &config(CONFIG), None).unwrap();
        assert_eq!(d.source, DecisionSource::DefaultBrowser);
        assert_eq!(d.bundle_id.as_str(), "com.apple.Safari");
    }

    #[test]
    fn default_browser_wins_over_recorded_default() {
        let recorded = bundle_id("org.mozilla.firefox");
        let d = decide_str("https://example.com/", &config(CONFIG), Some(&recorded)).unwrap();
        assert_eq!(d.source, DecisionSource::DefaultBrowser);
    }

    #[test]
    fn recorded_default_is_used_when_default_browser_is_absent() {
        let recorded = bundle_id("com.example.Browser");
        let d = decide_str(
            "https://example.com/",
            &config("version = 1"),
            Some(&recorded),
        )
        .unwrap();
        assert_eq!(d.source, DecisionSource::RecordedSystemDefault);
        assert_eq!(d.browser, None);
        assert_eq!(d.bundle_id, recorded);
    }

    #[test]
    fn no_fallback_when_neither_default_is_available() {
        let err = decide_str("https://example.com/", &config("version = 1"), None).unwrap_err();
        assert!(matches!(err, ResolveError::NoFallback(_)));
        let message = err.to_string();
        assert!(message.contains("default_browser") && message.contains("trier register"));
    }

    #[test]
    fn non_http_scheme_is_unsupported() {
        assert_eq!(
            decide_str("mailto:me@example.com", &config(CONFIG), None),
            Err(ResolveError::UnsupportedScheme("mailto".to_owned()))
        );
    }

    #[test]
    fn browser_missing_from_registry_is_an_error() {
        // Bypasses `Config::from_str`, which would reject the unknown name.
        let mut config = config(CONFIG);
        config.default_browser = Some(BrowserName::try_from("netscape".to_owned()).unwrap());
        assert!(matches!(
            decide(
                &Url::parse("https://example.com/").unwrap(),
                &config,
                &config.registry(),
                None
            ),
            Err(ResolveError::UnknownBrowser(_))
        ));
    }

    #[test]
    fn display_lists_source_browser_and_bundle_id() {
        let d = decide_str("https://github.com/", &config(CONFIG), None).unwrap();
        assert_eq!(
            d.to_string(),
            "url:       https://github.com/\n\
             matched:   rules[0] (github.com)\n\
             browser:   google-chrome\n\
             bundle id: com.google.Chrome"
        );
    }

    #[test]
    fn display_shows_dash_for_recorded_default_browser_name() {
        let recorded = bundle_id("com.example.Browser");
        let d = decide_str(
            "https://example.com/",
            &config("version = 1"),
            Some(&recorded),
        )
        .unwrap();
        assert!(
            d.to_string()
                .contains("matched:   recorded system default\nbrowser:   -\n")
        );
    }
}
