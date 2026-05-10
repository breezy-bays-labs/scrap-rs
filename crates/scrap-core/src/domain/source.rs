//! Source-discovery domain types — language-agnostic projection of what
//! a `SourcePort` adapter returns from a single workspace walk.
//!
//! Mirrors the [`super::parsed`] pattern: POD-only, `serde`-derive-only,
//! `#[non_exhaustive]` on the diagnostic-kind enum (per
//! [`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-nested-json-envelope.md)),
//! canonical `::new` constructor (D10) so detector PRs and adapter
//! evolutions extend signatures additively.

use crate::domain::types::FilePath;
use serde::{Deserialize, Serialize};

/// Adapter output for one `SourcePort::discover_test_files` call —
/// the deterministic file list plus any non-fatal mid-walk diagnostics.
///
/// I/O failures that abort the walk surface as `Err(SourceError::Io)`
/// at the trait level; observations that the walk recovered from (e.g.
/// a permission-denied subdirectory the walker skipped past) appear in
/// `diagnostics`. Empty `diagnostics` is the clean-walk case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryOutcome {
    /// Discovered file paths in deterministic post-collect byte-wise
    /// order (per shaping E1 — sort happens in the adapter after the
    /// walk completes; downstream consumers can rely on stable
    /// iteration without sorting again).
    pub files: Vec<FilePath>,
    /// Non-fatal mid-walk observations. Walk continued past each one.
    pub diagnostics: Vec<SourceDiagnostic>,
}

impl DiscoveryOutcome {
    /// Canonical constructor (D10). Adapter evolutions extend this
    /// signature additively — never break the wire shape, never break
    /// the constructor call sites.
    #[must_use]
    pub fn new(files: Vec<FilePath>, diagnostics: Vec<SourceDiagnostic>) -> Self {
        Self { files, diagnostics }
    }
}

/// One non-fatal mid-walk observation from a `SourcePort` adapter.
///
/// `kind` classifies the failure mode; `path` attributes it to the
/// directory entry the walker was visiting; `message` carries the
/// adapter's human-readable detail (typically the underlying
/// `std::io::Error` rendered via `Display`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDiagnostic {
    /// Path the walker was visiting when the diagnostic fired.
    pub path: FilePath,
    /// Classification of the underlying failure.
    pub kind: SourceDiagnosticKind,
    /// Adapter-supplied human-readable detail.
    pub message: String,
}

impl SourceDiagnostic {
    /// Canonical constructor (D10).
    #[must_use]
    pub fn new(path: FilePath, kind: SourceDiagnosticKind, message: impl Into<String>) -> Self {
        Self {
            path,
            kind,
            message: message.into(),
        }
    }
}

/// Classification of a non-fatal mid-walk diagnostic.
///
/// `#[non_exhaustive]` so future adapters can add `LoopDetected`,
/// `SymlinkBroken`, etc., without an envelope schema bump. Wire format
/// is `snake_case`, matching the rest of `domain/`. Per-variant
/// `#[serde(rename)]` is belt-and-suspenders against future serde
/// version drift.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceDiagnosticKind {
    /// EACCES / EPERM — the walker could not enter a directory or read
    /// a file. The walk skipped the entry and continued.
    #[serde(rename = "permission_denied")]
    PermissionDenied,
    /// Other mid-walk I/O failure — the walker recovered (typically by
    /// skipping the entry) but flagged the failure for the caller.
    #[serde(rename = "midwalk_io")]
    MidwalkIo,
    /// Catch-all for non-I/O `ignore::Error` shapes the adapter could
    /// not classify more specifically (loop detection, glob compile
    /// failure mid-walk, partial errors). Future adapter PRs may
    /// introduce more specific variants.
    #[serde(rename = "other")]
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_diagnostic() -> SourceDiagnostic {
        SourceDiagnostic::new(
            FilePath::new("denied/sub"),
            SourceDiagnosticKind::PermissionDenied,
            "Permission denied (os error 13)",
        )
    }

    fn sample_outcome() -> DiscoveryOutcome {
        DiscoveryOutcome::new(
            vec![FilePath::new("a.rs"), FilePath::new("b.rs")],
            vec![sample_diagnostic()],
        )
    }

    // Wire-key pins. Symmetric round-trip tests would round-trip fine
    // even if a `#[serde(rename)]` slipped onto a field — these pin the
    // top-level JSON keys so a rename is caught at test time.

    #[test]
    fn discovery_outcome_wire_keys() {
        let json = serde_json::to_value(sample_outcome()).unwrap();
        for key in ["files", "diagnostics"] {
            assert!(json.get(key).is_some(), "missing wire key: {key}");
        }
    }

    #[test]
    fn source_diagnostic_wire_keys() {
        let json = serde_json::to_value(sample_diagnostic()).unwrap();
        for key in ["path", "kind", "message"] {
            assert!(json.get(key).is_some(), "missing wire key: {key}");
        }
    }

    #[test]
    fn source_diagnostic_kind_serializes_snake_case() {
        for (variant, wire) in [
            (SourceDiagnosticKind::PermissionDenied, "permission_denied"),
            (SourceDiagnosticKind::MidwalkIo, "midwalk_io"),
            (SourceDiagnosticKind::Other, "other"),
        ] {
            let json = serde_json::to_value(variant).unwrap();
            assert_eq!(json, serde_json::Value::String(wire.into()));
            let back: SourceDiagnosticKind = serde_json::from_value(json).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn discovery_outcome_serde_round_trips() {
        let outcome = sample_outcome();
        let json = serde_json::to_string(&outcome).unwrap();
        let back: DiscoveryOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(outcome, back);
    }

    #[test]
    fn source_diagnostic_serde_round_trips() {
        let diag = sample_diagnostic();
        let json = serde_json::to_string(&diag).unwrap();
        let back: SourceDiagnostic = serde_json::from_str(&json).unwrap();
        assert_eq!(diag, back);
    }
}
