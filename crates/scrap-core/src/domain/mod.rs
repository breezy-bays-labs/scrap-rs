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
//! - `source` — `DiscoveryOutcome`, `SourceDiagnostic`, `SourceDiagnosticKind`
//! - `config` — `AnalysisConfig`
//! - `classification` — `Severity`, `Actionability`, `Confidence`, `RemediationMode`, `BaselineVerdict`
//! - `smell` — `SmellCategory`, `Smell`
//! - `finding` — `Finding`
//! - `report` — `Report`, `FileReport`, `Summary`, `Distribution`
//! - `threshold` — `ThresholdMode`
//! - `opt_outs` — `OptOut` enum (lands with scrap-rs#12)
//! - `assertion_sources` — `AssertionSource` enum + `recognise()` (folds in scrap-rs#4 surface via scrap-rs#12)
//! - `behavioral_fact` — `BehavioralFact` enum (folds in scrap-rs#25's projection surface via scrap-rs#30; first variant `ResultAsserted`)
//!
//! Module roster (planned, not yet implemented):
//! - `score` — saturating-curve helpers (v0.3+ per kickstart plan §3)

pub mod assertion_sources;
pub mod behavioral_fact;
pub mod classification;
pub mod config;
pub mod finding;
pub mod opt_outs;
pub mod parsed;
pub mod report;
pub mod smell;
pub mod source;
pub mod threshold;
pub mod types;
