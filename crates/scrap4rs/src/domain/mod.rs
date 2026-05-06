//! Domain layer — pure Rust types, no `syn`, no `serde`-on-AST, no I/O.
//!
//! Designed for future extraction into `scrap-core` at v1.0. Anything
//! that mentions `syn`, `walkdir`, `proc-macro2`, or any I/O type
//! belongs in `adapters/`, never here. The only external dependency
//! permitted is `serde` (derive only), so wire shapes round-trip
//! identically across the v1.0 split.
//!
//! Module roster:
//! - `types` — `Span`, `FilePath`, `QualifiedName`, `Location`, `TestIdentity`
//! - `smell` — `SmellCategory`, `Smell`
//! - `finding` — `Severity`, `Actionability`, `Finding`
//! - `report` — `Report`, `FileReport`, `ExampleReport`, `Summary`, `Distribution`
//! - `threshold` — `ThresholdMode`
//! - `assertion_sources` — implicit-assertion recognition list (lands with P6)
//! - `score` — saturating-curve helpers (v0.3+)

pub mod finding;
pub mod report;
pub mod smell;
pub mod threshold;
pub mod types;
