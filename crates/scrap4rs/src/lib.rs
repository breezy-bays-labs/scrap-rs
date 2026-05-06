//! `scrap4rs` — static test smell detector for Rust.
//!
//! Layout follows hexagonal architecture; module bodies fill in via
//! v0.1 sub-issue PRs. The strict layering rule is documented in
//! `CLAUDE.md` at the workspace root: `domain/` and `ports/` are
//! language-agnostic and will extract into `scrap-core` at v1.0
//! without rename.
//!
//! See <https://github.com/breezy-bays-labs/scrap-rs> for the epic
//! roadmap.

#![warn(missing_docs)]

pub mod adapters;
pub mod cli;
pub mod core;
pub mod domain;
pub mod ports;

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_compiles() {
        // Bootstrap-PR smoke test. Real domain tests land in P5.
        assert!(env!("CARGO_PKG_VERSION").starts_with("0."));
    }
}
