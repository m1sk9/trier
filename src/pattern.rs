use std::fmt;

use serde::Deserialize;
use url::{Host, Url};

/// A `host[/path]` pattern from `rules[].hosts`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct HostPattern {
    host: String,
    /// IP literals have no subdomains; `include_subdomains` is ignored for them.
    is_ip: bool,
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
        "host pattern `{0}` has an invalid host: use a domain name, an IPv4 address, or a bracketed IPv6 address"
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

    /// Whether the pattern matches every path below its segments.
    ///
    /// A bare host is normalized to recursive, so `github.com` and
    /// `github.com/**` are the same pattern.
    pub fn is_recursive(&self) -> bool {
        self.recursive
    }

    /// Whether `host` and `path` of a URL match this pattern.
    ///
    /// Both must be as `url::Url::host_str` / `url::Url::path` return them;
    /// the pattern was canonicalized the same way when parsed, so no further
    /// decoding happens here. A trailing dot on `host` (FQDN form) is ignored.
    pub fn matches(&self, host: &str, path: &str, include_subdomains: bool) -> bool {
        let host = host.strip_suffix('.').unwrap_or(host);
        host_matches(&self.host, host, include_subdomains && !self.is_ip)
            && self.path_matches(&path_segments(path))
    }

    fn path_matches<S: AsRef<str>>(&self, segments: &[S]) -> bool {
        let len_ok = if self.recursive {
            segments.len() >= self.segments.len()
        } else {
            segments.len() == self.segments.len()
        };
        len_ok
            && self
                .segments
                .iter()
                .zip(segments)
                .all(|(own, other)| own == other.as_ref())
    }
}

/// Whether every URL matched by `b` is also matched by `a`.
pub fn covers(a: &HostPattern, a_subdomains: bool, b: &HostPattern, b_subdomains: bool) -> bool {
    let (a_subdomains, b_subdomains) = (a_subdomains && !a.is_ip, b_subdomains && !b.is_ip);
    let hosts = if b_subdomains {
        a_subdomains && host_matches(&a.host, &b.host, true)
    } else {
        host_matches(&a.host, &b.host, a_subdomains)
    };
    let paths = if b.recursive {
        a.recursive && b.segments.starts_with(&a.segments)
    } else {
        a.path_matches(&b.segments)
    };
    hosts && paths
}

fn host_matches(pattern: &str, host: &str, include_subdomains: bool) -> bool {
    host == pattern
        || (include_subdomains
            && host
                .strip_suffix(pattern)
                .is_some_and(|rest| rest.ends_with('.')))
}

fn path_segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
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
            }
        }
        let literal_path = parts[..parts.len() - usize::from(recursive)].join("/");

        // Canonicalize through the same parser that produces the URLs being
        // matched, so IDNs, IP literal spellings (`127.1`, `[0::1]`),
        // percent-encoding and dot segments compare equal on both sides.
        let url = Url::parse(&format!("http://{host}/{literal_path}"))
            .map_err(|_| PatternError::InvalidHost(raw.to_owned()))?;
        let canonical_host = url.host_str().unwrap_or_default();
        let canonical_host = canonical_host.strip_suffix('.').unwrap_or(canonical_host);
        if canonical_host.is_empty() {
            return Err(PatternError::InvalidHost(raw.to_owned()));
        }
        let segments: Vec<String> = path_segments(url.path())
            .into_iter()
            .map(str::to_owned)
            .collect();

        Ok(Self {
            host: canonical_host.to_owned(),
            is_ip: !matches!(url.host(), Some(Host::Domain(_))),
            recursive: recursive || segments.is_empty(),
            segments,
        })
    }
}

/// Rejects what `Url::parse` would silently reinterpret (a port, userinfo,
/// query or fragment) instead of reporting.
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
        .all(|c| !c.is_ascii() || c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
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
        if self.recursive && !self.segments.is_empty() {
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
    fn bare_host_is_the_same_pattern_as_host_double_star() {
        let p = parse("github.com").unwrap();
        assert_eq!(p.host(), "github.com");
        assert!(p.segments().is_empty());
        assert!(p.is_recursive());
        assert_eq!(parse("github.com/**"), Ok(p));
    }

    #[test]
    fn bare_host_displays_without_double_star() {
        assert_eq!(parse("github.com/**").unwrap().to_string(), "github.com");
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

    fn url_matches(pattern: &str, url: &str, include_subdomains: bool) -> bool {
        let url = url::Url::parse(url).unwrap();
        parse(pattern)
            .unwrap()
            .matches(url.host_str().unwrap(), url.path(), include_subdomains)
    }

    #[test]
    fn bare_host_matches_root() {
        assert!(url_matches("github.com", "https://github.com/", false));
    }

    #[test]
    fn bare_host_matches_any_path_ignoring_query_and_fragment() {
        assert!(url_matches(
            "github.com",
            "https://github.com/a/b?x=1#f",
            false
        ));
    }

    #[test]
    fn subdomain_does_not_match_without_include_subdomains() {
        assert!(!url_matches(
            "github.com",
            "https://gist.github.com/",
            false
        ));
    }

    #[test]
    fn subdomain_matches_with_include_subdomains() {
        assert!(url_matches("github.com", "https://gist.github.com/", true));
    }

    #[test]
    fn include_subdomains_never_matches_a_suffix_that_is_not_a_label() {
        assert!(!url_matches("github.com", "https://evilgithub.com/", true));
    }

    #[test]
    fn host_comparison_is_case_insensitive() {
        assert!(url_matches("GitHub.com", "https://GITHUB.COM/", false));
    }

    #[test]
    fn double_star_matches_the_prefix_itself() {
        assert!(url_matches(
            "github.com/mk-system/**",
            "https://github.com/mk-system",
            false
        ));
    }

    #[test]
    fn double_star_matches_paths_below_the_prefix() {
        assert!(url_matches(
            "github.com/mk-system/**",
            "https://github.com/mk-system/trier",
            false
        ));
    }

    #[test]
    fn double_star_respects_segment_boundaries() {
        assert!(!url_matches(
            "github.com/mk-system/**",
            "https://github.com/mk-systems/x",
            false
        ));
    }

    #[test]
    fn exact_path_matches_with_or_without_trailing_slash() {
        assert!(url_matches(
            "github.com/mk-system",
            "https://github.com/mk-system",
            false
        ));
        assert!(url_matches(
            "github.com/mk-system",
            "https://github.com/mk-system/",
            false
        ));
    }

    #[test]
    fn exact_path_does_not_match_deeper_paths() {
        assert!(!url_matches(
            "github.com/mk-system",
            "https://github.com/mk-system/trier",
            false
        ));
    }

    #[test]
    fn port_is_ignored() {
        assert!(url_matches(
            "localhost/**",
            "http://localhost:3000/foo",
            false
        ));
    }

    #[test]
    fn bracketed_ipv6_pattern_matches_ipv6_url() {
        assert!(url_matches("[::1]", "http://[::1]:8080/", false));
    }

    #[test]
    fn path_is_not_percent_decoded() {
        assert!(url_matches(
            "example.com/a%2Fb",
            "https://example.com/a%2Fb",
            false
        ));
        assert!(!url_matches(
            "example.com/a%2Fb",
            "https://example.com/a/b",
            false
        ));
    }

    fn covers_str(a: &str, a_sub: bool, b: &str, b_sub: bool) -> bool {
        covers(&parse(a).unwrap(), a_sub, &parse(b).unwrap(), b_sub)
    }

    #[test]
    fn identical_patterns_cover_each_other() {
        assert!(covers_str("github.com/a", false, "github.com/a", false));
        assert!(covers_str(
            "github.com/a/**",
            false,
            "github.com/a/**",
            false
        ));
    }

    #[test]
    fn double_star_parent_covers_child() {
        assert!(covers_str(
            "github.com/a/**",
            false,
            "github.com/a/b",
            false
        ));
        assert!(covers_str(
            "github.com/a/**",
            false,
            "github.com/a/b/**",
            false
        ));
        assert!(covers_str("github.com", false, "github.com/a", false));
    }

    #[test]
    fn child_does_not_cover_parent() {
        assert!(!covers_str(
            "github.com/a/b/**",
            false,
            "github.com/a/**",
            false
        ));
    }

    #[test]
    fn exact_path_does_not_cover_double_star() {
        assert!(!covers_str("github.com/a", false, "github.com/a/**", false));
    }

    #[test]
    fn include_subdomains_parent_covers_subdomain_child() {
        assert!(covers_str("github.com", true, "gist.github.com", false));
    }

    #[test]
    fn host_without_include_subdomains_does_not_cover_subdomain() {
        assert!(!covers_str("github.com", false, "gist.github.com", false));
    }

    #[test]
    fn rule_without_include_subdomains_does_not_cover_one_with_it() {
        assert!(!covers_str("github.com", false, "github.com", true));
    }

    #[test]
    fn include_subdomains_covers_narrower_include_subdomains() {
        assert!(covers_str("github.com", true, "gist.github.com", true));
        assert!(!covers_str("gist.github.com", true, "github.com", true));
    }

    #[test]
    fn different_hosts_do_not_cover() {
        assert!(!covers_str("github.com", true, "gitlab.com", false));
    }

    #[test]
    fn idn_host_is_stored_as_punycode() {
        assert_eq!(parse("例え.jp").unwrap().host(), "xn--r8jz45g.jp");
        assert!(url_matches("例え.jp", "https://例え.jp/", false));
    }

    #[test]
    fn trailing_dot_on_pattern_host_is_dropped() {
        assert_eq!(parse("github.com.").unwrap().host(), "github.com");
    }

    #[test]
    fn trailing_dot_on_url_host_is_ignored() {
        assert!(url_matches("github.com", "https://github.com./x", false));
        assert!(url_matches("github.com", "https://gist.github.com./", true));
    }

    #[test]
    fn lone_dot_host_is_rejected() {
        assert_eq!(parse("."), Err(PatternError::InvalidHost(".".into())));
    }

    #[test]
    fn ipv4_spellings_match_the_canonical_address() {
        assert_eq!(parse("127.1").unwrap().host(), "127.0.0.1");
        assert!(url_matches("0x7f.0.0.1", "http://127.0.0.1/", false));
    }

    #[test]
    fn ipv6_spellings_match_the_canonical_address() {
        assert_eq!(parse("[0:0:0:0:0:0:0:1]").unwrap().host(), "[::1]");
        assert!(url_matches(
            "[::ffff:127.0.0.1]",
            "http://[::ffff:7f00:1]/",
            false
        ));
    }

    #[test]
    fn include_subdomains_is_ignored_for_ip_literals() {
        assert!(!url_matches("0.0.1", "http://127.0.0.1/", true));
        assert!(url_matches("127.0.0.1", "http://127.0.0.1/", true));
    }

    #[test]
    fn invalid_ipv4_like_host_is_rejected() {
        assert_eq!(
            parse("256.0.0.1"),
            Err(PatternError::InvalidHost("256.0.0.1".into()))
        );
    }

    #[test]
    fn non_ascii_path_matches_its_percent_encoded_url_path() {
        assert_eq!(
            parse("example.com/日本").unwrap().segments(),
            ["%E6%97%A5%E6%9C%AC"]
        );
        assert!(url_matches(
            "example.com/日本",
            "https://example.com/日本",
            false
        ));
        assert!(url_matches(
            "example.com/{x}",
            "https://example.com/{x}",
            false
        ));
    }

    #[test]
    fn dot_segments_in_pattern_path_are_resolved() {
        assert_eq!(parse("example.com/a/../b").unwrap().segments(), ["b"]);
    }

    #[test]
    fn ip_pattern_with_include_subdomains_covers_only_itself() {
        assert!(!covers_str("0.0.1", true, "127.0.0.1", false));
        assert!(covers_str("127.0.0.1", false, "127.0.0.1", true));
    }
}
