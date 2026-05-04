//! Domain layer — pure Rust types, no `syn`, no `serde`-on-AST, no I/O.
//!
//! Designed for future extraction into `scrap-core` at v1.0. Anything
//! that mentions `syn`, `walkdir`, `proc-macro2`, or any I/O type
//! belongs in `adapters/`, never here.
//!
//! Module skeleton:
//! - `finding.rs` — `Finding`, `Severity`, `Actionability` (v0.1 P5)
//! - `smell.rs` — `SmellCategory` enum (v0.1 P5)
//! - `report.rs` — `Report`, `FileReport`, `Summary` (v0.1 P5)
//! - `threshold.rs` — `ThresholdMode` (v0.1 P5)
//! - `assertion_sources.rs` — implicit-assertion recognition list (v0.1 P6)
//! - `score.rs` — saturating-curve helpers (v0.3+)
//! - `types.rs` — `Location`, `Span`, `FilePath`, `QualifiedName` (v0.1 P5)
