# Release Notes

## Unreleased

- Bootstrap crate skeleton: `Cargo.toml`, `src/lib.rs`, CI (build/test/clippy/fmt),
  license files, `.gitignore`. (#1)
- Add `ParseError`: the 9-variant, `#[non_exhaustive]` URL-parsing error enum,
  with `Display`/`std::error::Error`/`Clone`/`Copy`/`PartialEq`/`Eq`. (#2)
