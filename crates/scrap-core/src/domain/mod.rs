//! Domain layer — pure Rust types, no `syn`, no `serde`-on-AST, no I/O.
//!
//! Designed for future extraction into `scrap-core` at v1.0. Anything
//! that mentions `syn`, `walkdir`, `proc-macro2`, or any I/O type
//! belongs in `adapters/`, never here. The only external dependency
//! permitted is `serde` (derive only), so wire shapes round-trip
//! identically across the v1.0 split.
//!
//! Module roster (live):
//! - `types` — `Span`, `FilePath`, `QualifiedName`, `TestIdentity`, `InvertedSpan`
//! - `classification` — `Severity`, `Actionability`, `Confidence`, `RemediationMode`, `BaselineVerdict`
//! - `smell` — `SmellCategory`, `Smell`
//! - `finding` — `Finding`
//! - `report` — `Report`, `FileReport`, `Summary`, `Distribution`
//! - `threshold` — `ThresholdMode`
//!
//! Module roster (planned, not yet implemented):
//! - `assertion_sources` — implicit-assertion recognition list (lands with scrap-rs#4 / P6)
//! - `score` — saturating-curve helpers (v0.3+ per kickstart plan §3)

pub mod classification;
pub mod finding;
pub mod report;
pub mod smell;
pub mod threshold;
pub mod types;
