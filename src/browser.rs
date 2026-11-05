use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

use crate::config::CustomBrowser;

/// Built-in browsers keyed by Homebrew cask token.
///
/// Only entries whose bundle identifier was verified with
/// `mdls -name kMDItemCFBundleIdentifier -raw /Applications/<App>.app`
/// belong here; unverified ids would silently open nothing.
pub const BUILTIN: &[(&str, &str)] = &[
    ("firefox", "org.mozilla.firefox"),
    (
        "firefox@developer-edition",
        "org.mozilla.firefoxdeveloperedition",
    ),
    ("firefox@nightly", "org.mozilla.nightly"),
    ("google-chrome", "com.google.Chrome"),
    ("google-chrome@dev", "com.google.Chrome.dev"),
    // Not a cask: ships with macOS.
    ("safari", "com.apple.Safari"),
    ("tor-browser", "org.torproject.torbrowser"),
];

/// A browser name usable in `default_browser`, `rules[].browser` and `[browsers]`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct BrowserName(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid browser name `{0}`: must start with [a-z0-9] and contain only [a-z0-9@+._-]")]
pub struct InvalidBrowserName(String);

impl BrowserName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BrowserName {
    type Error = InvalidBrowserName;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        let is_lead = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
        let mut chars = name.chars();
        let valid = chars.next().is_some_and(is_lead)
            && chars.all(|c| is_lead(c) || matches!(c, '@' | '+' | '.' | '_' | '-'));
        if valid {
            Ok(Self(name))
        } else {
            Err(InvalidBrowserName(name))
        }
    }
}

impl fmt::Display for BrowserName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A macOS bundle identifier such as `com.apple.Safari`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct BundleId(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("bundle_id must not be empty")]
pub struct EmptyBundleId;

impl BundleId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BundleId {
    type Error = EmptyBundleId;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        if id.trim().is_empty() {
            Err(EmptyBundleId)
        } else {
            Ok(Self(id))
        }
    }
}

impl fmt::Display for BundleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    BuiltIn,
    Custom,
}

/// Built-in browsers merged with the config's `[browsers]` table.
#[derive(Debug, Clone)]
pub struct BrowserRegistry {
    entries: BTreeMap<BrowserName, (BundleId, Origin)>,
}

impl BrowserRegistry {
    pub fn new(custom: &BTreeMap<BrowserName, CustomBrowser>) -> Self {
        let builtin = BUILTIN.iter().map(|(name, id)| {
            (
                BrowserName((*name).to_owned()),
                (BundleId((*id).to_owned()), Origin::BuiltIn),
            )
        });
        let custom = custom
            .iter()
            .map(|(name, b)| (name.clone(), (b.bundle_id.clone(), Origin::Custom)));
        Self {
            entries: builtin.chain(custom).collect(),
        }
    }

    pub fn resolve(&self, name: &BrowserName) -> Option<&BundleId> {
        self.entries.get(name).map(|(id, _)| id)
    }

    /// Entries sorted by name.
    pub fn iter(&self) -> impl Iterator<Item = (&BrowserName, &BundleId, Origin)> {
        self.entries
            .iter()
            .map(|(name, (id, origin))| (name, id, *origin))
    }
}

/// The bundle id a built-in entry maps `name` to, if any.
pub fn builtin_bundle_id(name: &BrowserName) -> Option<&'static str> {
    BUILTIN
        .iter()
        .find(|(n, _)| *n == name.as_str())
        .map(|(_, id)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(s: &str) -> BrowserName {
        BrowserName::try_from(s.to_owned()).unwrap()
    }

    fn custom(id: &str) -> CustomBrowser {
        CustomBrowser {
            bundle_id: BundleId::try_from(id.to_owned()).unwrap(),
        }
    }

    #[test]
    fn every_builtin_name_is_a_valid_browser_name() {
        for (n, _) in BUILTIN {
            assert!(BrowserName::try_from((*n).to_owned()).is_ok(), "{n}");
        }
    }

    #[test]
    fn builtin_table_is_sorted_and_unique() {
        assert!(BUILTIN.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn browser_name_accepts_cask_tokens() {
        for ok in [
            "firefox",
            "firefox@developer-edition",
            "a+b._-c",
            "1password",
        ] {
            assert!(BrowserName::try_from(ok.to_owned()).is_ok(), "{ok}");
        }
    }

    #[test]
    fn browser_name_rejects_uppercase_leading_symbols_and_empty() {
        for bad in ["", "Firefox", "-firefox", "@dev", "fire fox", "fire/fox"] {
            assert!(BrowserName::try_from(bad.to_owned()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn bundle_id_rejects_blank() {
        assert_eq!(BundleId::try_from("  ".to_owned()), Err(EmptyBundleId));
    }

    #[test]
    fn resolves_builtin_browser() {
        let registry = BrowserRegistry::new(&BTreeMap::new());
        assert_eq!(
            registry.resolve(&name("safari")).map(BundleId::as_str),
            Some("com.apple.Safari")
        );
    }

    #[test]
    fn unknown_browser_does_not_resolve() {
        let registry = BrowserRegistry::new(&BTreeMap::new());
        assert_eq!(registry.resolve(&name("netscape")), None);
    }

    #[test]
    fn custom_entry_overrides_builtin() {
        let registry = BrowserRegistry::new(&BTreeMap::from([(
            name("safari"),
            custom("com.apple.SafariTechnologyPreview"),
        )]));
        assert_eq!(
            registry.resolve(&name("safari")).map(BundleId::as_str),
            Some("com.apple.SafariTechnologyPreview")
        );
        let origin = registry
            .iter()
            .find(|(n, _, _)| n.as_str() == "safari")
            .map(|(_, _, o)| o);
        assert_eq!(origin, Some(Origin::Custom));
    }

    #[test]
    fn iter_yields_builtin_and_custom_entries_sorted_by_name() {
        let registry = BrowserRegistry::new(&BTreeMap::from([(
            name("firefox@dev"),
            custom("org.mozilla.firefoxdeveloperedition"),
        )]));
        let names: Vec<_> = registry.iter().map(|(n, _, _)| n.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        assert_eq!(names.len(), BUILTIN.len() + 1);
        assert!(
            registry
                .iter()
                .any(|(n, _, o)| n.as_str() == "firefox@dev" && o == Origin::Custom)
        );
        assert!(
            registry
                .iter()
                .any(|(n, _, o)| n.as_str() == "firefox" && o == Origin::BuiltIn)
        );
    }

    #[test]
    fn builtin_bundle_id_looks_up_builtin_table_only() {
        assert_eq!(
            builtin_bundle_id(&name("google-chrome")),
            Some("com.google.Chrome")
        );
        assert_eq!(builtin_bundle_id(&name("firefox@dev")), None);
    }
}
