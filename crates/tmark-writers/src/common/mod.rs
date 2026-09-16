//! Helpers the three writers share: the output buffer with its source map,
//! the zero-width collapse, media filtering, reference text, Unicode and
//! key-label tables, slugs. Design 07: "LaTeX and Typst share helpers, not
//! a base class."

pub mod abbr;
pub mod epigraph;
pub mod logos;
pub mod media;
pub mod out;
pub mod refs;
pub mod text;
pub mod zero;

pub use out::Out;
