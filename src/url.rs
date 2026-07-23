//! The [`Url`] type: a parsed, always-valid URL.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use crate::host::{Host, HostInternal};
use crate::origin::{self, Origin};
use crate::parser::{self, Parser, SchemeType};
use crate::ParseError;

/// A parsed URL record, per the [WHATWG URL Standard](https://url.spec.whatwg.org/).
///
/// Internally, a `Url` is its own serialization (a `String`) plus byte
/// offsets marking each component's boundary — so `as_str`/`Display`/
/// `into<String>` are all free, and component accessors are cheap slices.
#[derive(Clone)]
pub struct Url {
    pub(crate) serialization: String,
    pub(crate) scheme_end: u32,
    pub(crate) username_end: u32,
    pub(crate) host_start: u32,
    pub(crate) host_end: u32,
    pub(crate) host: HostInternal,
    pub(crate) port: Option<u16>,
    pub(crate) path_start: u32,
    pub(crate) query_start: Option<u32>,
    pub(crate) fragment_start: Option<u32>,
}

impl Url {
    /// Parse an absolute URL from a string.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::RelativeUrlWithoutBase`] if `input` has no
    /// scheme (this crate does not yet support parsing relative to a base
    /// URL), or another [`ParseError`] variant if `input` is malformed.
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        Parser::parse_url(input)
    }

    /// Return the serialization of this URL.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.serialization
    }

    /// Consume this `Url`, returning its serialization.
    #[inline]
    pub fn into_string(self) -> String {
        self.serialization
    }

    /// Return the scheme, lowercased, without the trailing `:`.
    #[inline]
    pub fn scheme(&self) -> &str {
        &self.serialization[..self.scheme_end as usize]
    }

    /// Return whether the scheme is one of the WHATWG-defined special
    /// schemes (`http`, `https`, `ws`, `wss`, `ftp`, `file`).
    pub fn is_special(&self) -> bool {
        SchemeType::from(self.scheme()).is_special()
    }

    /// Return this URL's [origin](https://url.spec.whatwg.org/#origin).
    pub fn origin(&self) -> Origin {
        origin::url_origin(self)
    }

    /// Return whether the URL has an authority (`//` followed by an
    /// optional userinfo, a host, and an optional port).
    #[inline]
    pub fn has_authority(&self) -> bool {
        self.serialization[self.scheme_end as usize..].starts_with("://")
    }

    /// Return the `user:password@host:port` authority component, or `""`
    /// if the URL has no authority.
    pub fn authority(&self) -> &str {
        let sep = "://".len() as u32;
        if self.has_authority() && self.path_start > self.scheme_end + sep {
            &self.serialization[(self.scheme_end + sep) as usize..self.path_start as usize]
        } else {
            ""
        }
    }

    /// Return whether this is a "cannot-be-a-base" URL — one whose scheme
    /// is not followed by a `/`, such as `data:` or `mailto:` URLs. A
    /// relative reference cannot be resolved against a cannot-be-a-base
    /// base URL.
    #[inline]
    pub fn cannot_be_a_base(&self) -> bool {
        !self.serialization[self.scheme_end as usize + 1..].starts_with('/')
    }

    /// Return the percent-encoded username (the empty string if none).
    pub fn username(&self) -> &str {
        let sep = "://".len() as u32;
        if self.has_authority() && self.username_end > self.scheme_end + sep {
            &self.serialization[(self.scheme_end + sep) as usize..self.username_end as usize]
        } else {
            ""
        }
    }

    /// Return the percent-encoded password, if any.
    pub fn password(&self) -> Option<&str> {
        if self.has_authority()
            && self.username_end != self.serialization.len() as u32
            && self.serialization.as_bytes()[self.username_end as usize] == b':'
        {
            Some(&self.serialization[self.username_end as usize + 1..self.host_start as usize - 1])
        } else {
            None
        }
    }

    /// Equivalent to `url.host().is_some()`.
    pub fn has_host(&self) -> bool {
        !matches!(self.host, HostInternal::None)
    }

    /// Return the host (domain or IP address) as a string, if any.
    pub fn host_str(&self) -> Option<&str> {
        self.has_host()
            .then(|| &self.serialization[self.host_start as usize..self.host_end as usize])
    }

    /// Return the parsed [`Host`], if any.
    pub fn host(&self) -> Option<Host<&str>> {
        match self.host {
            HostInternal::None => None,
            HostInternal::Domain => Some(Host::Domain(
                &self.serialization[self.host_start as usize..self.host_end as usize],
            )),
            HostInternal::Ipv4(addr) => Some(Host::Ipv4(addr)),
            HostInternal::Ipv6(addr) => Some(Host::Ipv6(addr)),
        }
    }

    /// Return the host as a domain name, if it has a host and that host is
    /// a domain (not an IP address).
    pub fn domain(&self) -> Option<&str> {
        matches!(self.host, HostInternal::Domain)
            .then(|| &self.serialization[self.host_start as usize..self.host_end as usize])
    }

    /// Return the port, if it was explicit and differs from the scheme's
    /// default (a matching default port is never stored — see
    /// [`Url::port_or_known_default`]).
    #[inline]
    pub fn port(&self) -> Option<u16> {
        self.port
    }

    /// Return the port, falling back to the scheme's default port if known
    /// (`http`, `https`, `ws`, `wss`, `ftp`).
    #[inline]
    pub fn port_or_known_default(&self) -> Option<u16> {
        self.port.or_else(|| parser::default_port(self.scheme()))
    }

    /// Return the path.
    pub fn path(&self) -> &str {
        match (self.query_start, self.fragment_start) {
            (None, None) => &self.serialization[self.path_start as usize..],
            (Some(end), _) | (None, Some(end)) => {
                &self.serialization[self.path_start as usize..end as usize]
            }
        }
    }

    /// Return the query string (without the leading `?`), if any.
    pub fn query(&self) -> Option<&str> {
        let query_start = self.query_start?;
        let start = query_start as usize + 1;
        Some(match self.fragment_start {
            Some(end) => &self.serialization[start..end as usize],
            None => &self.serialization[start..],
        })
    }

    /// Return the fragment (without the leading `#`), if any.
    pub fn fragment(&self) -> Option<&str> {
        self.fragment_start
            .map(|start| &self.serialization[start as usize + 1..])
    }
}

impl fmt::Display for Url {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialization)
    }
}

impl fmt::Debug for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Url")
            .field("scheme", &self.scheme())
            .field("cannot_be_a_base", &self.cannot_be_a_base())
            .field("username", &self.username())
            .field("password", &self.password())
            .field("host", &self.host())
            .field("port", &self.port())
            .field("path", &self.path())
            .field("query", &self.query())
            .field("fragment", &self.fragment())
            .finish()
    }
}

impl From<Url> for String {
    fn from(url: Url) -> Self {
        url.serialization
    }
}

impl Eq for Url {}

impl PartialEq for Url {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.serialization == other.serialization
    }
}

impl Ord for Url {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.serialization.cmp(&other.serialization)
    }
}

impl PartialOrd for Url {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for Url {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.serialization.hash(state);
    }
}

impl AsRef<str> for Url {
    #[inline]
    fn as_ref(&self) -> &str {
        &self.serialization
    }
}

impl FromStr for Url {
    type Err = ParseError;

    #[inline]
    fn from_str(input: &str) -> Result<Self, ParseError> {
        Self::parse(input)
    }
}

impl<'a> TryFrom<&'a str> for Url {
    type Error = ParseError;

    #[inline]
    fn try_from(input: &'a str) -> Result<Self, ParseError> {
        Self::parse(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_as_str() {
        let s = "https://example.net/";
        assert_eq!(Url::parse(s).unwrap().as_str(), s);
    }

    #[test]
    fn scheme_is_lowercased() {
        assert_eq!(Url::parse("HTTP://example.com/").unwrap().scheme(), "http");
    }

    #[test]
    fn is_special_by_scheme() {
        assert!(Url::parse("http://x/").unwrap().is_special());
        assert!(Url::parse("file:///x").unwrap().is_special());
        assert!(!Url::parse("moz://x/").unwrap().is_special());
    }

    #[test]
    fn has_authority_and_authority_string() {
        let url = Url::parse("https://user:pass@example.com:8080/path").unwrap();
        assert!(url.has_authority());
        assert_eq!(url.authority(), "user:pass@example.com:8080");

        let url = Url::parse("unix:/run/foo.socket").unwrap();
        assert!(!url.has_authority());
        assert_eq!(url.authority(), "");
    }

    #[test]
    fn cannot_be_a_base_for_opaque_schemes() {
        assert!(Url::parse("data:text/plain,Stuff")
            .unwrap()
            .cannot_be_a_base());
        assert!(!Url::parse("ftp://rms@example.com")
            .unwrap()
            .cannot_be_a_base());
        assert!(!Url::parse("unix:/run/foo.socket")
            .unwrap()
            .cannot_be_a_base());
    }

    #[test]
    fn username_and_password() {
        let url = Url::parse("ftp://rms:secret@example.com").unwrap();
        assert_eq!(url.username(), "rms");
        assert_eq!(url.password(), Some("secret"));

        let url = Url::parse("ftp://:secret@example.com").unwrap();
        assert_eq!(url.username(), "");
        assert_eq!(url.password(), Some("secret"));

        let url = Url::parse("https://example.com").unwrap();
        assert_eq!(url.username(), "");
        assert_eq!(url.password(), None);
    }

    #[test]
    fn host_domain_and_ip() {
        let url = Url::parse("https://Example.COM/").unwrap();
        assert!(url.has_host());
        assert_eq!(url.host_str(), Some("example.com"));
        assert_eq!(url.domain(), Some("example.com"));

        let url = Url::parse("https://127.0.0.1/").unwrap();
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert_eq!(url.domain(), None);

        let url = Url::parse("data:text/plain,x").unwrap();
        assert!(!url.has_host());
        assert_eq!(url.host_str(), None);
    }

    #[test]
    fn port_and_known_default() {
        let url = Url::parse("https://example.com").unwrap();
        assert_eq!(url.port(), None);
        assert_eq!(url.port_or_known_default(), Some(443));

        let url = Url::parse("https://example.com:443/").unwrap();
        assert_eq!(url.port(), None, "default port is not stored");

        let url = Url::parse("ssh://example.com:22").unwrap();
        assert_eq!(url.port(), Some(22));

        let url = Url::parse("foo://example.com:1456").unwrap();
        assert_eq!(url.port_or_known_default(), Some(1456));
        let url = Url::parse("foo://example.com").unwrap();
        assert_eq!(url.port_or_known_default(), None);
    }

    #[test]
    fn path_query_fragment() {
        let url = Url::parse("https://example.com/a/b?x=1&y=2#frag").unwrap();
        assert_eq!(url.path(), "/a/b");
        assert_eq!(url.query(), Some("x=1&y=2"));
        assert_eq!(url.fragment(), Some("frag"));

        let url = Url::parse("https://example.com").unwrap();
        assert_eq!(url.path(), "/");
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), None);
    }

    #[test]
    fn dot_segments_are_normalized() {
        let url = Url::parse("http://example.com/a/b/../c").unwrap();
        assert_eq!(url.path(), "/a/c");

        let url = Url::parse("http://example.com/a/./b").unwrap();
        assert_eq!(url.path(), "/a/b");

        let url = Url::parse("http://example.com/../../a").unwrap();
        assert_eq!(url.path(), "/a");
    }

    #[test]
    fn percent_encodes_reserved_and_non_ascii() {
        let url = Url::parse("https://example.com/a b?<c>#<d>").unwrap();
        assert_eq!(url.path(), "/a%20b");
        assert_eq!(url.query(), Some("%3Cc%3E"));
        assert_eq!(url.fragment(), Some("%3Cd%3E"));
    }

    #[test]
    fn userinfo_is_percent_encoded() {
        let url = Url::parse("https://user name:p@ss@example.com/").unwrap();
        assert_eq!(url.username(), "user%20name");
        assert_eq!(url.password(), Some("p%40ss"));
    }

    #[test]
    fn idna_domain_round_trips_to_punycode() {
        let url = Url::parse("https://bücher.example/").unwrap();
        assert_eq!(url.host_str(), Some("xn--bcher-kva.example"));
    }

    #[test]
    fn ipv6_host_is_bracketed() {
        let url = Url::parse("http://[::1]:8080/").unwrap();
        assert_eq!(url.host_str(), Some("[::1]"));
        assert_eq!(url.port(), Some(8080));
    }

    #[test]
    fn file_url_without_host() {
        let url = Url::parse("file:///tmp/foo").unwrap();
        assert_eq!(url.scheme(), "file");
        assert!(!url.has_host());
        assert_eq!(url.path(), "/tmp/foo");
    }

    #[test]
    fn file_url_windows_drive_letter() {
        let url = Url::parse("file:///C:/Users/").unwrap();
        assert_eq!(url.path(), "/C:/Users/");
    }

    #[test]
    fn empty_host_on_special_scheme_is_an_error() {
        // Leading slashes beyond "//" are consumed as part of the authority
        // marker (`http:///path` == `http://path/`, host "path"), so this
        // needs an authority that is present but truly empty.
        assert_eq!(Url::parse("http://"), Err(ParseError::EmptyHost));
        assert_eq!(Url::parse("http://@/"), Err(ParseError::EmptyHost));
    }

    #[test]
    fn extra_leading_slashes_become_part_of_the_authority() {
        // Matches the reference `url` crate: all of "///" after "http:" is
        // treated as the authority-slashes marker, so "path" is the host.
        let url = Url::parse("http:///path").unwrap();
        assert_eq!(url.host_str(), Some("path"));
        assert_eq!(url.path(), "/");
    }

    #[test]
    fn invalid_port_is_an_error() {
        assert_eq!(
            Url::parse("http://example.com:notaport/"),
            Err(ParseError::InvalidPort)
        );
        assert_eq!(
            Url::parse("http://example.com:99999/"),
            Err(ParseError::InvalidPort)
        );
    }

    #[test]
    fn relative_reference_without_scheme_is_an_error() {
        assert_eq!(
            Url::parse("/just/a/path"),
            Err(ParseError::RelativeUrlWithoutBase)
        );
    }

    #[test]
    fn equality_ordering_and_hash_follow_serialization() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher as _;

        let a = Url::parse("https://a.example/").unwrap();
        let b = Url::parse("https://b.example/").unwrap();
        let a2 = Url::parse("https://a.example/").unwrap();
        assert_eq!(a, a2);
        assert_ne!(a, b);
        assert!(a < b);

        let mut ha = DefaultHasher::new();
        a.hash(&mut ha);
        let mut ha2 = DefaultHasher::new();
        a2.hash(&mut ha2);
        assert_eq!(ha.finish(), ha2.finish());
    }

    #[test]
    fn from_str_and_try_from_and_into_string() {
        let url: Url = "https://example.com/".parse().unwrap();
        assert_eq!(url.as_str(), "https://example.com/");

        let url = Url::try_from("https://example.com/").unwrap();
        let s: String = url.into();
        assert_eq!(s, "https://example.com/");
    }

    #[test]
    fn debug_includes_components() {
        let url = Url::parse("https://user@example.com/path?q#f").unwrap();
        let debug = format!("{url:?}");
        assert!(debug.contains("example.com"));
        assert!(debug.contains("/path"));
    }
}
