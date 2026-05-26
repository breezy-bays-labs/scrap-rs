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
//! from this crate's source. The ban is enforced structurally
//! (`Cargo.toml` does not list any AST library, so a wrong `use`
//! line cannot resolve) and via a source-level `ast-purity` CI grep
//! plus the matching `lefthook` pre-push hook.
//!
//! Module roster:
//! - [`domain`] — types: `Smell`, `SmellCategory`, `Finding`, `Report`, `Span`, `TestIdentity`, etc.
//! - [`ports`] — trait definitions and per-port error enums:
//!   `SourcePort`/`SourceError`, `TestParserPort`/`ParseError`.
//! - [`core`] — generic orchestration over ports (planned).
//! - [`detectors`] — per-smell detector modules (planned).
//! - [`adapters`] — language-agnostic adapter implementations: file walker, reporters.
//!   - [`adapters::reporters::json`] — v0.1 JSON envelope reporter (`scrap-rs#14`):
//!     `emit()` free function + `EmitOptions`, `ViewSpec`, `ViewBlock`,
//!     `DeltaBlock`, `DiagnosticsBlock`. Wire shape per `adr-nested-json-envelope`.
//! - [`adapter_meta`] — `AdapterMeta` adapter-binary identity bundle
//!   (`scrap-rs#14`); top-level placement so reporters and the future
//!   `cli::run` consume the same type without crossing module boundaries.
//! - [`cli`] — CLI surface: clap derive struct, `AnalysisConfig`, `ExitCode`, generic run loop.
//!   - [`cli::config`] — project-level TOML config schema (`scrap-rs#18`):
//!     `FileConfig`, `OptOutPolicy`, `DetectorConfig`, `Override`,
//!     `ConfigError`; loader (`load_config`, `discover_config`); canonical
//!     overrides resolver (`resolve_detector_for_path`).

#![warn(missing_docs)]

pub mod adapter_meta;
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
