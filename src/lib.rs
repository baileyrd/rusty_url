//! `rusty_url` is a from-scratch implementation of the [WHATWG URL Living
//! Standard](https://url.spec.whatwg.org/), built to reach public-API parity
//! with the [`url`](https://crates.io/crates/url) crate. See `gap-analysis.md`
//! in the repository root for the tracked parity roadmap.

mod error;
mod host;
mod parser;
mod percent_encode;
mod url;

pub mod form_urlencoded;

pub use error::ParseError;
pub use host::Host;
pub use url::Url;
