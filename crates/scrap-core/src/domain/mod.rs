//! Domain layer — language-agnostic types shared by every adapter
//! binary in the workspace.
//!
//! No AST library types here, ever — `syn`, `swc_*`, `oxc_*`,
//! `tree-sitter*`, `proc-macro2`, `quote` are banned from this crate
//! per [`adr-hexagonal-layout`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-hexagonal-layout.md).
//! Anything that mentions an AST library or any I/O type belongs in
//! an adapter crate (`scrap4rs::parser`, future `scrap4ts::parser`),
//! never here. The only external dependency permitted is `serde`
//! (derive only), so wire shapes round-trip identically across every
//! adapter binary that links `scrap-core`.
//!
//! Module roster (live):
//! - `types` — `Span`, `FilePath`, `QualifiedName`, `TestIdentity`, `InvertedSpan`, `SourceRoot`
//! - `parsed` — `ParsedTestFile`, `ParsedTest`, `ParsedAttribute`, `ParsedAssertion`, `ParseDiagnostic`, `ParseDiagnosticKind`
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
pub mod parsed;
pub mod report;
pub mod smell;
pub mod threshold;
pub mod types;
