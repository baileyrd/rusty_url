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
