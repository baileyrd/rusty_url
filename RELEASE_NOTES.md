# Release Notes

## Unreleased

- Bootstrap crate skeleton: `Cargo.toml`, `src/lib.rs`, CI (build/test/clippy/fmt),
  license files, `.gitignore`. (#1)
- Add `ParseError`: the 9-variant, `#[non_exhaustive]` URL-parsing error enum,
  with `Display`/`std::error::Error`/`Clone`/`Copy`/`PartialEq`/`Eq`. (#2)
- Add `Host<S>` and the WHATWG host parser (domain via `idna`, IPv4, IPv6,
  opaque host), plus `Display`/`Clone`/`Eq`/`Ord`/`Hash`. Adds a dependency
  on the `idna` crate for IDNA/punycode domain normalization (by design —
  see #3 discussion). (#3)
- Add the `form_urlencoded` module: `parse`/`Parse`/`ParseIntoOwned`,
  `byte_serialize`/`ByteSerialize`, and `Serializer`/`Target`/
  `EncodingOverride` for building `application/x-www-form-urlencoded`
  strings. Standalone — no dependency on `Url`. (#4)
- Add `Url`: the WHATWG basic URL parser (special schemes incl. `file:`
  with Windows drive-letter handling, non-special/opaque-path schemes,
  dot-segment path normalization, percent-encoding per component) plus
  `Url::parse`, `as_str`/`into_string`, `Display`/`Debug`/`Clone`/`Eq`/
  `Ord`/`Hash`/`AsRef<str>`/`From<Url> for String`/`FromStr`/
  `TryFrom<&str>`, and accessors `scheme`/`is_special`/`cannot_be_a_base`/
  `has_authority`/`authority`/`username`/`password`/`has_host`/`host`/
  `host_str`/`domain`/`port`/`port_or_known_default`/`path`/`query`/
  `fragment`. Scoped to `Url::parse` without a base URL — `join` and
  `ParseOptions::base_url` are later issues. Differentially tested against
  55 cases from the reference `url` crate (byte-for-byte identical output).
  (#5)
- Add `Origin`/`OpaqueOrigin` and `Url::origin()`: `(scheme, host, port)`
  tuple origins for `ftp`/`http`/`https`/`ws`/`wss`, recursive origin
  resolution for `blob:` URLs, and opaque (equal-only-to-itself) origins
  for everything else, plus `ascii_serialization`/`unicode_serialization`.
  (#6)
- Add `Url` setters: `set_scheme`/`set_username`/`set_password`/
  `set_host`/`set_ip_host`/`set_port`/`set_path`/`set_query`/
  `set_fragment`. Introduces a parser `Context` (URL-parser vs. setter) so
  re-parsing an isolated component (e.g. `set_path`) doesn't treat an
  embedded `?`/`#` as starting a new component. Matches the reference
  crate's documented quirks exactly, including `set_host(None)` leaving
  `has_host()` true with an empty-string domain rather than clearing to
  no host — verified against the reference `url` crate across 31
  setter-call cases (byte-for-byte identical output). (#7)
- Add `Url::join` and `Url::make_relative`. `join` required adding the
  base-relative branches to the parser that were deliberately deferred in
  #5 (`parse_relative`, `fragment_only`, and `file:`-scheme base
  inheritance, including the Windows-drive-letter case); `make_relative`
  is pure string logic over two already-parsed URLs, no parser changes
  needed. Verified against the reference `url` crate across 29 `join`
  cases and 12 `make_relative` cases (byte-for-byte identical output),
  including scheme-relative/absolute/dot-segment resolution, `file:`
  drive-letter inheritance, and cannot-be-a-base rejection. (#8)
