//! `scrap-core` — language-agnostic library for the scrap test-smell
//! detector ecosystem.
//!
//! Houses the domain types, port traits, generic orchestration,
//! detector implementations, language-agnostic adapters (file walker,
//! reporters), and the CLI surface (clap derive, `AnalysisConfig`,
//! `ExitCode`, generic run loop). Every adapter binary in the
//! workspace — `scrap4rs` for Rust source via `syn`, `scrap4ts` for
//! TypeScript source via `swc`/`oxc`, future adapters for additional
//! source languages — links against this crate and provides only its
//! language-specific parser adapter.
//!
//! Per [`adr-hexagonal-layout`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-hexagonal-layout.md),
//! this crate must never depend on an AST library. `syn`, `swc_*`,
//! `oxc_*`, `tree-sitter*`, `proc-macro2`, and `quote` are banned
//! transitively from this crate's dep graph; that ban is the
//! structural mechanism that makes `scrap-core` shareable across
//! adapters without a domain rewrite at v1.0.
//!
//! Module roster:
//! - [`domain`] — types: `Smell`, `SmellCategory`, `Finding`, `Report`, `Span`, `TestIdentity`, etc.
//! - [`ports`] — trait definitions: `SourcePort`, `TestParserPort`, `OutputPort` (planned).
//! - [`core`] — generic orchestration over ports (planned).
//! - [`detectors`] — per-smell detector modules (planned).
//! - [`adapters`] — language-agnostic adapter implementations: file walker, reporters (planned).
//! - [`cli`] — CLI surface: clap derive struct, `AnalysisConfig`, `ExitCode`, generic run loop.

#![warn(missing_docs)]
#![warn(clippy::pedantic, clippy::cargo)]

pub mod adapters;
pub mod cli;
pub mod core;
pub mod detectors;
pub mod domain;
pub mod ports;

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_compiles() {
        // Bootstrap smoke test, mirroring scrap4rs's pre-split test.
        // Real domain tests live in `crates/scrap-core/tests/` and the
        // per-module `#[cfg(test)] mod tests` blocks under `domain/`.
        assert!(env!("CARGO_PKG_VERSION").starts_with("0."));
    }
}
