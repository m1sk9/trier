use std::fmt;

use serde::Deserialize;

/// A `host[/path]` pattern from `rules[].hosts`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct HostPattern {
    host: String,
    segments: Vec<String>,
    recursive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    #[error("host pattern `{0}` must not contain a scheme")]
    Scheme(String),
    #[error("host pattern `{0}` must not contain a port")]
    Port(String),
    #[error("host pattern `{0}` must not contain `*` (only a trailing `/**` is allowed)")]
    Wildcard(String),
    #[error("host pattern `{0}` may use `**` only as the last path segment")]
    MisplacedRecursive(String),
    #[error("host pattern `{0}` has an empty host")]
    EmptyHost(String),
    #[error(
        "host pattern `{0}` has an invalid host: use [a-zA-Z0-9._-] (punycode for IDNs) or a bracketed IPv6 address"
    )]
    InvalidHost(String),
    #[error("host pattern `{0}` must not contain a query, a fragment, or whitespace")]
    InvalidPath(String),
}

impl HostPattern {
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Whether the pattern ends with `/**`.
    pub fn is_recursive(&self) -> bool {
        self.recursive
    }
}

impl TryFrom<String> for HostPattern {
    type Error = PatternError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        raw.parse()
    }
}

impl std::str::FromStr for HostPattern {
    type Err = PatternError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        // Only a `://` inside the host part is a scheme; paths such as
        // `web.archive.org/web/https://...` may legitimately contain one.
        if raw
            .split_once("://")
            .is_some_and(|(scheme, _)| !scheme.contains('/'))
        {
            return Err(PatternError::Scheme(raw.to_owned()));
        }
        let (host, path) = raw.split_once('/').unwrap_or((raw, ""));
        if host.is_empty() {
            return Err(PatternError::EmptyHost(raw.to_owned()));
        }
        validate_host(host).map_err(|e| e(raw.to_owned()))?;

        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut segments = Vec::with_capacity(parts.len());
        let mut recursive = false;
        for (i, part) in parts.iter().enumerate() {
            if *part == "**" {
                if i + 1 != parts.len() {
                    return Err(PatternError::MisplacedRecursive(raw.to_owned()));
                }
                recursive = true;
            } else if part.contains('*') {
                return Err(PatternError::Wildcard(raw.to_owned()));
            } else if part.contains(['?', '#']) || part.contains(char::is_whitespace) {
                return Err(PatternError::InvalidPath(raw.to_owned()));
            } else {
                segments.push((*part).to_owned());
            }
        }

        Ok(Self {
            host: host.to_ascii_lowercase(),
            segments,
            recursive,
        })
    }
}

fn validate_host(host: &str) -> Result<(), fn(String) -> PatternError> {
    if let Some(rest) = host.strip_prefix('[') {
        return match rest.split_once(']') {
            Some((addr, ""))
                if !addr.is_empty()
                    && addr
                        .chars()
                        .all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '.')) =>
            {
                Ok(())
            }
            Some((_, port)) if port.starts_with(':') => Err(PatternError::Port),
            _ => Err(PatternError::InvalidHost),
        };
    }
    if host.contains(':') {
        return Err(PatternError::Port);
    }
    if host.contains('*') {
        return Err(PatternError::Wildcard);
    }
    if !host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
    {
        return Err(PatternError::InvalidHost);
    }
    Ok(())
}

impl fmt::Display for HostPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.host)?;
        for segment in &self.segments {
            write!(f, "/{segment}")?;
        }
        if self.recursive {
            f.write_str("/**")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> Result<HostPattern, PatternError> {
        raw.parse()
    }

    #[test]
    fn bare_host_has_no_path() {
        let p = parse("github.com").unwrap();
        assert_eq!(p.host(), "github.com");
        assert!(p.segments().is_empty());
        assert!(!p.is_recursive());
    }

    #[test]
    fn host_is_lowercased() {
        assert_eq!(parse("GitHub.COM").unwrap().host(), "github.com");
    }

    #[test]
    fn path_segments_are_kept_verbatim() {
        let p = parse("github.com/Mk-System/Trier").unwrap();
        assert_eq!(p.segments(), ["Mk-System", "Trier"]);
    }

    #[test]
    fn trailing_double_star_marks_recursive() {
        let p = parse("github.com/mk-system/**").unwrap();
        assert_eq!(p.segments(), ["mk-system"]);
        assert!(p.is_recursive());
    }

    #[test]
    fn trailing_slash_is_ignored() {
        assert_eq!(
            parse("github.com/mk-system/"),
            parse("github.com/mk-system")
        );
    }

    #[test]
    fn display_round_trips_normalized_form() {
        assert_eq!(
            parse("GitHub.com/mk-system/**").unwrap().to_string(),
            "github.com/mk-system/**"
        );
    }

    #[test]
    fn rejects_scheme() {
        assert_eq!(
            parse("https://github.com"),
            Err(PatternError::Scheme("https://github.com".into()))
        );
    }

    #[test]
    fn rejects_port() {
        assert_eq!(
            parse("localhost:3000/**"),
            Err(PatternError::Port("localhost:3000/**".into()))
        );
    }

    #[test]
    fn rejects_star_in_host() {
        assert_eq!(
            parse("*.github.com"),
            Err(PatternError::Wildcard("*.github.com".into()))
        );
    }

    #[test]
    fn rejects_single_star_segment() {
        assert_eq!(
            parse("github.com/*"),
            Err(PatternError::Wildcard("github.com/*".into()))
        );
    }

    #[test]
    fn rejects_star_inside_segment() {
        assert_eq!(
            parse("github.com/mk-*"),
            Err(PatternError::Wildcard("github.com/mk-*".into()))
        );
    }

    #[test]
    fn rejects_double_star_before_last_segment() {
        assert_eq!(
            parse("github.com/**/trier"),
            Err(PatternError::MisplacedRecursive(
                "github.com/**/trier".into()
            ))
        );
    }

    #[test]
    fn rejects_scheme_without_path() {
        assert_eq!(
            parse("https://"),
            Err(PatternError::Scheme("https://".into()))
        );
    }

    #[test]
    fn scheme_like_text_in_path_is_allowed() {
        let p = parse("web.archive.org/web/https://example.com").unwrap();
        assert_eq!(p.segments(), ["web", "https:", "example.com"]);
    }

    #[test]
    fn accepts_bracketed_ipv6_host() {
        assert_eq!(parse("[::1]/**").unwrap().host(), "[::1]");
        assert_eq!(parse("[FE80::1]").unwrap().host(), "[fe80::1]");
    }

    #[test]
    fn rejects_port_on_ipv6_host() {
        assert_eq!(
            parse("[::1]:3000"),
            Err(PatternError::Port("[::1]:3000".into()))
        );
    }

    #[test]
    fn rejects_malformed_ipv6_host() {
        for bad in ["[]", "[::1", "[::g]", "[::1]x"] {
            assert_eq!(parse(bad), Err(PatternError::InvalidHost(bad.into())));
        }
    }

    #[test]
    fn rejects_host_characters_that_never_appear_in_a_url_host() {
        for bad in [
            "git hub.com",
            "user@github.com",
            "github.com?x",
            "github.com#x",
            "例え.jp",
        ] {
            assert_eq!(parse(bad), Err(PatternError::InvalidHost(bad.into())));
        }
    }

    #[test]
    fn rejects_query_fragment_or_whitespace_in_path() {
        for bad in ["github.com/a?b", "github.com/a#b", "github.com/a b"] {
            assert_eq!(parse(bad), Err(PatternError::InvalidPath(bad.into())));
        }
    }

    #[test]
    fn rejects_empty_pattern() {
        assert_eq!(parse(""), Err(PatternError::EmptyHost("".into())));
    }

    #[test]
    fn rejects_path_without_host() {
        assert_eq!(
            parse("/mk-system/**"),
            Err(PatternError::EmptyHost("/mk-system/**".into()))
        );
    }
}
