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
