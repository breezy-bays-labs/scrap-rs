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
//! strict superset of `scrap_core`'s — every module added to
//! `scrap_core` becomes immediately public on `scrap4rs`. Re-exported
//! types are identical (no newtype wrap): `scrap4rs::domain::Finding`
//! and `scrap_core::domain::Finding` are the same type.

#![warn(missing_docs)]
#![warn(clippy::pedantic, clippy::cargo)]

pub use scrap_core::{adapters, cli, core, detectors, domain, ports};

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_compiles() {
        assert!(env!("CARGO_PKG_VERSION").starts_with("0."));
    }
}
