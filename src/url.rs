//! The [`Url`] type: a parsed, always-valid URL.

use std::cmp::Ordering;
use std::fmt::{self, Write as _};
use std::hash::{Hash, Hasher};
use std::mem;
use std::net::IpAddr;
use std::str::FromStr;

use crate::host::{Host, HostInternal};
use crate::origin::{self, Origin};
use crate::parser::{self, Parser, SchemeType, USERINFO};
use crate::percent_encode::utf8_percent_encode;
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

    /// Change this URL's scheme.
    ///
    /// Fails (leaving the URL unchanged) if: `scheme` isn't a valid scheme
    /// (`[a-zA-Z][a-zA-Z0-9+.-]*`); switching between a special and a
    /// non-special scheme; switching to `file` while the URL has
    /// credentials or a port; or switching to a special scheme on a URL
    /// with no host.
    #[allow(clippy::result_unit_err)]
    pub fn set_scheme(&mut self, scheme: &str) -> Result<(), ()> {
        let mut parser = Parser::for_setter(String::new());
        let remaining = parser.parse_scheme(parser::Input::new_no_trim(scheme))?;
        let new_scheme_type = SchemeType::from(parser.serialization.as_str());
        let old_scheme_type = SchemeType::from(self.scheme());
        if (new_scheme_type.is_special() && !old_scheme_type.is_special())
            || (!new_scheme_type.is_special() && old_scheme_type.is_special())
            || (new_scheme_type.is_file() && self.has_authority())
        {
            return Err(());
        }
        if !remaining.is_empty() || (!self.has_host() && new_scheme_type.is_special()) {
            return Err(());
        }

        let old_scheme_end = self.scheme_end;
        let new_scheme_end = parser.serialization.len() as u32;
        let adjust = |index: &mut u32| {
            *index = *index - old_scheme_end + new_scheme_end;
        };
        self.scheme_end = new_scheme_end;
        adjust(&mut self.username_end);
        adjust(&mut self.host_start);
        adjust(&mut self.host_end);
        adjust(&mut self.path_start);
        if let Some(ref mut index) = self.query_start {
            adjust(index);
        }
        if let Some(ref mut index) = self.fragment_start {
            adjust(index);
        }

        parser
            .serialization
            .push_str(&self.serialization[old_scheme_end as usize..]);
        self.serialization = parser.serialization;

        // A scheme change can make a stored port equal to the new scheme's
        // default (or stop being equal to the old one's) — re-normalize.
        let previous_port = self.port();
        let _ = self.set_port(previous_port);

        Ok(())
    }

    /// Change this URL's username. Percent-encodes `username` as needed.
    ///
    /// Fails if this URL is cannot-be-a-base, has no host, or is a `file:`
    /// URL (none of which have credentials).
    #[allow(clippy::result_unit_err)]
    pub fn set_username(&mut self, username: &str) -> Result<(), ()> {
        if !self.has_host() || self.host() == Some(Host::Domain("")) || self.scheme() == "file" {
            return Err(());
        }
        let username_start = self.scheme_end + 3;
        if &self.serialization[username_start as usize..self.username_end as usize] == username {
            return Ok(());
        }
        let after_username = self.serialization[self.username_end as usize..].to_owned();
        self.serialization.truncate(username_start as usize);
        self.serialization
            .push_str(&utf8_percent_encode(username, USERINFO));

        let mut removed_bytes = self.username_end;
        self.username_end = self.serialization.len() as u32;
        let mut added_bytes = self.username_end;

        let new_username_is_empty = self.username_end == username_start;
        match (new_username_is_empty, after_username.chars().next()) {
            (true, Some('@')) => {
                removed_bytes += 1;
                self.serialization.push_str(&after_username[1..]);
            }
            (false, Some('@')) | (_, Some(':')) | (true, _) => {
                self.serialization.push_str(&after_username);
            }
            (false, _) => {
                added_bytes += 1;
                self.serialization.push('@');
                self.serialization.push_str(&after_username);
            }
        }

        let adjust = |index: &mut u32| {
            *index = *index - removed_bytes + added_bytes;
        };
        adjust(&mut self.host_start);
        adjust(&mut self.host_end);
        adjust(&mut self.path_start);
        if let Some(ref mut index) = self.query_start {
            adjust(index);
        }
        if let Some(ref mut index) = self.fragment_start {
            adjust(index);
        }
        Ok(())
    }

    /// Change this URL's password, or remove it (`None`). Percent-encodes
    /// the password as needed.
    ///
    /// Fails if this URL is cannot-be-a-base, has no host, or is a `file:`
    /// URL.
    #[allow(clippy::result_unit_err)]
    pub fn set_password(&mut self, password: Option<&str>) -> Result<(), ()> {
        if !self.has_host() || self.host() == Some(Host::Domain("")) || self.scheme() == "file" {
            return Err(());
        }
        let password = password.unwrap_or_default();
        if !password.is_empty() {
            let host_and_after = self.serialization[self.host_start as usize..].to_owned();
            self.serialization.truncate(self.username_end as usize);
            self.serialization.push(':');
            self.serialization
                .push_str(&utf8_percent_encode(password, USERINFO));
            self.serialization.push('@');

            let old_host_start = self.host_start;
            let new_host_start = self.serialization.len() as u32;
            let adjust = |index: &mut u32| {
                *index = *index - old_host_start + new_host_start;
            };
            self.host_start = new_host_start;
            adjust(&mut self.host_end);
            adjust(&mut self.path_start);
            if let Some(ref mut index) = self.query_start {
                adjust(index);
            }
            if let Some(ref mut index) = self.fragment_start {
                adjust(index);
            }

            self.serialization.push_str(&host_and_after);
        } else if self.byte_at(self.username_end) == b':' {
            // There is a password to remove.
            let username_start = self.scheme_end + 3;
            let empty_username = username_start == self.username_end;
            let start = self.username_end;
            let end = if empty_username {
                self.host_start // also remove the separating '@'
            } else {
                self.host_start - 1 // keep '@' separating username from host
            };
            self.serialization.drain(start as usize..end as usize);
            let offset = end - start;
            self.host_start -= offset;
            self.host_end -= offset;
            self.path_start -= offset;
            if let Some(ref mut index) = self.query_start {
                *index -= offset;
            }
            if let Some(ref mut index) = self.fragment_start {
                *index -= offset;
            }
        }
        Ok(())
    }

    /// Change this URL's host.
    ///
    /// Passing `None` removes the host, and also removes any username,
    /// password, and port. Fails if this URL is cannot-be-a-base, if
    /// `host` fails to parse, or if removing the host on a special
    /// (non-`file`) scheme, since those always require a host.
    pub fn set_host(&mut self, host: Option<&str>) -> Result<(), ParseError> {
        if self.cannot_be_a_base() {
            return Err(ParseError::SetHostOnCannotBeABaseUrl);
        }
        let scheme_type = SchemeType::from(self.scheme());

        if let Some(host) = host {
            if host.is_empty() && scheme_type.is_special() && !scheme_type.is_file() {
                return Err(ParseError::EmptyHost);
            }
            let mut host_substr = host;
            if !host.starts_with('[') || !host.ends_with(']') {
                match host.find(':') {
                    Some(0) => return Err(ParseError::InvalidDomainCharacter),
                    Some(colon_index) => host_substr = &host[..colon_index],
                    None => {}
                }
            }
            let parsed = if scheme_type.is_special() {
                Host::parse(host_substr)?
            } else {
                Host::parse_opaque(host_substr)?
            };
            self.set_host_internal(parsed, None);
        } else if self.has_host() {
            if scheme_type.is_special() && !scheme_type.is_file() {
                return Err(ParseError::EmptyHost);
            } else if self.serialization.len() == self.path_start as usize {
                self.serialization.push('/');
            }

            let new_path_start = if scheme_type.is_file() {
                self.scheme_end + 3
            } else {
                self.scheme_end + 1
            };
            self.serialization
                .drain(new_path_start as usize..self.path_start as usize);
            let offset = self.path_start - new_path_start;
            self.path_start = new_path_start;
            self.username_end = new_path_start;
            self.host_start = new_path_start;
            self.host_end = new_path_start;
            // Deliberately not reset to `HostInternal::None` here, matching
            // the reference crate: this leaves an empty-domain host, so
            // `has_host()` stays `true` and `host_str()` returns `Some("")`
            // rather than `None` after removing a host this way.
            self.port = None;
            if let Some(ref mut index) = self.query_start {
                *index -= offset;
            }
            if let Some(ref mut index) = self.fragment_start {
                *index -= offset;
            }
        }
        Ok(())
    }

    /// `opt_new_port`: `None` leaves the port unchanged, `Some(None)` removes it.
    fn set_host_internal(&mut self, host: Host<String>, opt_new_port: Option<Option<u16>>) {
        let old_suffix_pos = if opt_new_port.is_some() {
            self.path_start
        } else {
            self.host_end
        };
        let suffix = self.serialization[old_suffix_pos as usize..].to_owned();
        self.serialization.truncate(self.host_start as usize);
        if !self.has_authority() {
            self.serialization.push('/');
            self.serialization.push('/');
            self.username_end += 2;
            self.host_start += 2;
        }
        write!(&mut self.serialization, "{host}").unwrap();
        self.host_end = self.serialization.len() as u32;
        self.host = host.into();

        if let Some(new_port) = opt_new_port {
            self.port = new_port;
            if let Some(port) = new_port {
                write!(&mut self.serialization, ":{port}").unwrap();
            }
        }
        let new_suffix_pos = self.serialization.len() as u32;
        self.serialization.push_str(&suffix);

        let adjust = |index: &mut u32| {
            *index = *index - old_suffix_pos + new_suffix_pos;
        };
        adjust(&mut self.path_start);
        if let Some(ref mut index) = self.query_start {
            adjust(index);
        }
        if let Some(ref mut index) = self.fragment_start {
            adjust(index);
        }
    }

    /// Change this URL's host to an IP address directly, skipping the host
    /// parser. Fails if this URL is cannot-be-a-base.
    #[allow(clippy::result_unit_err)]
    pub fn set_ip_host(&mut self, address: IpAddr) -> Result<(), ()> {
        if self.cannot_be_a_base() {
            return Err(());
        }
        let host = match address {
            IpAddr::V4(addr) => Host::Ipv4(addr),
            IpAddr::V6(addr) => Host::Ipv6(addr),
        };
        self.set_host_internal(host, None);
        Ok(())
    }

    /// Change this URL's port, or remove it (`None`). A port equal to the
    /// scheme's default is normalized away, matching [`Url::port`]. Fails
    /// if the URL has no host, has an empty host, or is a `file:` URL.
    #[allow(clippy::result_unit_err)]
    pub fn set_port(&mut self, mut port: Option<u16>) -> Result<(), ()> {
        if !self.has_host() || self.host() == Some(Host::Domain("")) || self.scheme() == "file" {
            return Err(());
        }
        if port.is_some() && port == parser::default_port(self.scheme()) {
            port = None;
        }
        self.set_port_internal(port);
        Ok(())
    }

    fn set_port_internal(&mut self, port: Option<u16>) {
        match (self.port, port) {
            (None, None) => {}
            (Some(_), None) => {
                self.serialization
                    .drain(self.host_end as usize..self.path_start as usize);
                let offset = self.path_start - self.host_end;
                self.path_start = self.host_end;
                if let Some(ref mut index) = self.query_start {
                    *index -= offset;
                }
                if let Some(ref mut index) = self.fragment_start {
                    *index -= offset;
                }
            }
            (Some(old), Some(new)) if old == new => {}
            (_, Some(new)) => {
                let path_and_after = self.serialization[self.path_start as usize..].to_owned();
                self.serialization.truncate(self.host_end as usize);
                write!(&mut self.serialization, ":{new}").unwrap();
                let old_path_start = self.path_start;
                let new_path_start = self.serialization.len() as u32;
                self.path_start = new_path_start;
                let adjust = |index: &mut u32| {
                    *index = *index - old_path_start + new_path_start;
                };
                if let Some(ref mut index) = self.query_start {
                    adjust(index);
                }
                if let Some(ref mut index) = self.fragment_start {
                    adjust(index);
                }
                self.serialization.push_str(&path_and_after);
            }
        }
        self.port = port;
    }

    /// Change this URL's path. Percent-encodes `path` as needed (without
    /// double-encoding anything already percent-encoded).
    pub fn set_path(&mut self, mut path: &str) {
        let after_path = self.take_after_path();
        let old_after_path_pos = self.serialization.len() as u32;
        let cannot_be_a_base = self.cannot_be_a_base();
        let scheme_type = SchemeType::from(self.scheme());
        self.serialization.truncate(self.path_start as usize);
        self.mutate(|parser| {
            if cannot_be_a_base {
                if let Some(stripped) = path.strip_prefix('/') {
                    parser.serialization.push_str("%2F");
                    path = stripped;
                }
                parser.parse_cannot_be_a_base_path(parser::Input::new_no_trim(path));
            } else {
                let mut has_host = true;
                parser.parse_path_start(
                    scheme_type,
                    &mut has_host,
                    parser::Input::new_no_trim(path),
                );
            }
        });
        self.restore_after_path(old_after_path_pos, &after_path);
    }

    fn take_after_path(&mut self) -> String {
        match (self.query_start, self.fragment_start) {
            (Some(i), _) | (None, Some(i)) => {
                let after_path = self.serialization[i as usize..].to_owned();
                self.serialization.truncate(i as usize);
                after_path
            }
            (None, None) => String::new(),
        }
    }

    fn restore_after_path(&mut self, old_after_path_position: u32, after_path: &str) {
        let new_after_path_position = self.serialization.len() as u32;
        let adjust = |index: &mut u32| {
            *index = *index - old_after_path_position + new_after_path_position;
        };
        if let Some(ref mut index) = self.query_start {
            adjust(index);
        }
        if let Some(ref mut index) = self.fragment_start {
            adjust(index);
        }
        self.serialization.push_str(after_path);
    }

    /// Change this URL's query string, or remove it (`None`).
    pub fn set_query(&mut self, query: Option<&str>) {
        let fragment = self.take_fragment();

        if let Some(start) = self.query_start.take() {
            self.serialization.truncate(start as usize);
        }
        if let Some(input) = query {
            self.query_start = Some(self.serialization.len() as u32);
            self.serialization.push('?');
            let scheme_type = SchemeType::from(self.scheme());
            self.mutate(|parser| {
                parser.parse_query(scheme_type, parser::Input::new_no_trim(input))
            });
        } else {
            self.query_start = None;
            if fragment.is_none() {
                self.strip_trailing_spaces_from_opaque_path();
            }
        }

        self.restore_already_parsed_fragment(fragment);
    }

    /// Change this URL's fragment, or remove it (`None`).
    pub fn set_fragment(&mut self, fragment: Option<&str>) {
        if let Some(start) = self.fragment_start {
            self.serialization.truncate(start as usize);
        }
        if let Some(input) = fragment {
            self.fragment_start = Some(self.serialization.len() as u32);
            self.serialization.push('#');
            self.mutate(|parser| parser.parse_fragment(parser::Input::new_no_trim(input)));
        } else {
            self.fragment_start = None;
            self.strip_trailing_spaces_from_opaque_path();
        }
    }

    fn take_fragment(&mut self) -> Option<String> {
        self.fragment_start.take().map(|start| {
            let fragment = self.serialization[start as usize + 1..].to_owned();
            self.serialization.truncate(start as usize);
            fragment
        })
    }

    fn restore_already_parsed_fragment(&mut self, fragment: Option<String>) {
        if let Some(fragment) = fragment {
            debug_assert!(self.fragment_start.is_none());
            self.fragment_start = Some(self.serialization.len() as u32);
            self.serialization.push('#');
            self.serialization.push_str(&fragment);
        }
    }

    /// <https://url.spec.whatwg.org/#potentially-strip-trailing-spaces-from-an-opaque-path>
    fn strip_trailing_spaces_from_opaque_path(&mut self) {
        if !self.cannot_be_a_base() || self.fragment_start.is_some() || self.query_start.is_some() {
            return;
        }
        let trailing_spaces = self
            .serialization
            .chars()
            .rev()
            .take_while(|&c| c == ' ')
            .count();
        let start = self.serialization.len() - trailing_spaces;
        self.serialization.truncate(start);
    }

    fn mutate<F: FnOnce(&mut Parser) -> R, R>(&mut self, f: F) -> R {
        let mut parser = Parser::for_setter(mem::take(&mut self.serialization));
        let result = f(&mut parser);
        self.serialization = parser.serialization;
        result
    }

    fn byte_at(&self, i: u32) -> u8 {
        self.serialization.as_bytes()[i as usize]
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

    #[test]
    fn set_fragment_add_change_remove() {
        let mut url = Url::parse("https://example.com/data.csv").unwrap();
        url.set_fragment(Some("cell=4,1-6,2"));
        assert_eq!(url.as_str(), "https://example.com/data.csv#cell=4,1-6,2");
        assert_eq!(url.fragment(), Some("cell=4,1-6,2"));
        url.set_fragment(None);
        assert_eq!(url.as_str(), "https://example.com/data.csv");
        assert!(url.fragment().is_none());
    }

    #[test]
    fn set_query_add_change_remove() {
        let mut url = Url::parse("https://example.com/products").unwrap();
        url.set_query(Some("page=2"));
        assert_eq!(url.as_str(), "https://example.com/products?page=2");
        assert_eq!(url.query(), Some("page=2"));
        url.set_query(None);
        assert_eq!(url.as_str(), "https://example.com/products");
    }

    #[test]
    fn set_query_preserves_fragment() {
        let mut url = Url::parse("https://example.net?lang=fr#nav").unwrap();
        url.set_query(Some("lang=en"));
        assert_eq!(url.as_str(), "https://example.net/?lang=en#nav");
    }

    #[test]
    fn set_query_hash_is_literal_not_a_fragment_separator() {
        let mut url = Url::parse("https://host/oldpath").unwrap();
        url.set_query(Some("a#b"));
        assert_eq!(url.query(), Some("a%23b"));
        assert_eq!(url.fragment(), None);
    }

    #[test]
    fn set_path_basic_and_percent_encoding() {
        let mut url = Url::parse("https://example.com").unwrap();
        url.set_path("api/comments");
        assert_eq!(url.as_str(), "https://example.com/api/comments");

        let mut url = Url::parse("https://example.com").unwrap();
        url.set_path("api/some comments");
        assert_eq!(url.path(), "/api/some%20comments");

        let mut url = Url::parse("https://example.com").unwrap();
        url.set_path("api/some%20comments");
        assert_eq!(url.path(), "/api/some%20comments");
    }

    #[test]
    fn set_path_question_and_hash_are_literal_not_terminators() {
        let mut url = Url::parse("https://host/oldpath").unwrap();
        url.set_path("a?b#c");
        assert_eq!(url.path(), "/a%3Fb%23c");
    }

    #[test]
    fn set_port_change_and_remove() {
        let mut url = Url::parse("ssh://example.net:2048/").unwrap();
        url.set_port(Some(4096)).unwrap();
        assert_eq!(url.as_str(), "ssh://example.net:4096/");
        url.set_port(None).unwrap();
        assert_eq!(url.as_str(), "ssh://example.net/");
    }

    #[test]
    fn set_port_to_known_default_is_not_stored() {
        let mut url = Url::parse("https://example.org/").unwrap();
        url.set_port(Some(443)).unwrap();
        assert!(url.port().is_none());
    }

    #[test]
    fn set_port_fails_for_cannot_be_a_base() {
        let mut url = Url::parse("mailto:rms@example.net").unwrap();
        assert!(url.set_port(Some(80)).is_err());
        assert!(url.set_port(None).is_err());
    }

    #[test]
    fn set_host_change_and_remove() {
        let mut url = Url::parse("https://example.net").unwrap();
        url.set_host(Some("rust-lang.org")).unwrap();
        assert_eq!(url.as_str(), "https://rust-lang.org/");

        let mut url = Url::parse("foo://example.net").unwrap();
        url.set_host(None).unwrap();
        assert_eq!(url.as_str(), "foo:/");
    }

    #[test]
    fn set_host_none_fails_for_special_scheme() {
        let mut url = Url::parse("https://example.net").unwrap();
        assert!(url.set_host(None).is_err());
        assert_eq!(url.as_str(), "https://example.net/");
    }

    #[test]
    fn set_host_fails_for_cannot_be_a_base() {
        let mut url = Url::parse("mailto:rms@example.net").unwrap();
        assert!(url.set_host(Some("rust-lang.org")).is_err());
        assert!(url.set_host(None).is_err());
    }

    #[test]
    fn set_ip_host() {
        let mut url = Url::parse("http://example.com").unwrap();
        url.set_ip_host("127.0.0.1".parse().unwrap()).unwrap();
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert_eq!(url.as_str(), "http://127.0.0.1/");
    }

    #[test]
    fn set_ip_host_fails_for_cannot_be_a_base() {
        let mut url = Url::parse("mailto:rms@example.com").unwrap();
        assert!(url.set_ip_host("127.0.0.1".parse().unwrap()).is_err());
    }

    #[test]
    fn set_password_and_username() {
        let mut url = Url::parse("ftp://user1:secret1@example.com").unwrap();
        url.set_password(Some("secret_password")).unwrap();
        assert_eq!(url.password(), Some("secret_password"));

        let mut url = Url::parse("ftp://:secre1@example.com/").unwrap();
        url.set_username("user1").unwrap();
        assert_eq!(url.username(), "user1");
        assert_eq!(url.as_str(), "ftp://user1:secre1@example.com/");
    }

    #[test]
    fn set_username_password_fail_for_cannot_be_a_base() {
        let mut url = Url::parse("mailto:rmz@example.com").unwrap();
        assert!(url.set_username("user1").is_err());
        assert!(url.set_password(Some("x")).is_err());
    }

    #[test]
    fn set_scheme_basic() {
        let mut url = Url::parse("https://example.net").unwrap();
        url.set_scheme("http").unwrap();
        assert_eq!(url.as_str(), "http://example.net/");

        let mut url = Url::parse("foo://example.net").unwrap();
        url.set_scheme("bar").unwrap();
        assert_eq!(url.as_str(), "bar://example.net");
    }

    #[test]
    fn set_scheme_rejects_invalid_and_incompatible_changes() {
        let mut url = Url::parse("https://example.net").unwrap();
        assert!(url.set_scheme("fo\u{f5}").is_err());
        assert_eq!(url.as_str(), "https://example.net/");

        let mut url = Url::parse("mailto:rms@example.net").unwrap();
        assert!(url.set_scheme("https").is_err());

        let mut url = Url::parse("foo://example.net").unwrap();
        assert!(url.set_scheme("https").is_err());

        let mut url = Url::parse("http://example.net").unwrap();
        assert!(url.set_scheme("foo").is_err());
    }
}
