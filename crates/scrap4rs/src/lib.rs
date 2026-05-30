//! `scrap4rs` — Rust-source adapter for the scrap test-smell detector.
//!
//! Owns the syn-based parser adapter (lands with scrap-rs#5). Domain
//! types, port traits, detectors, reporters, file walker, and the
//! entire CLI surface live in [`scrap_core`]; this crate provides
//! only what is genuinely Rust-source-specific.
//!
//! For consumer convenience the `scrap_core` modules are re-exported
//! here, so downstream code that wants the full analyzer surface can
//! depend on `scrap4rs` alone. This makes `scrap4rs`'s public API a
//! strict superset of `scrap_core`'s — every top-level `scrap_core`
//! module is re-exported here (the `pub use` list below is maintained
//! by hand; any module added to `scrap_core` must be added to it).
//! Re-exported types are identical (no newtype wrap):
//! `scrap4rs::domain::Finding` and `scrap_core::domain::Finding` are
//! the same type.

#![warn(missing_docs)]

pub use scrap_core::{adapter_meta, adapters, cli, core, detectors, domain, ports};

pub mod parser;

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_compiles() {
        assert!(env!("CARGO_PKG_VERSION").starts_with("0."));
    }
}
