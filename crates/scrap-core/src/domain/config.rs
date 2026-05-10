//! Caller-supplied workspace analysis configuration.
//!
//! Lives in `domain/` because the same struct is consumed by every
//! adapter binary — `scrap4rs` builds it from clap-derived CLI args,
//! future `scrap4ts` will build it from a TS-side runner. Validation
//! of `exclude` globs lives in the adapter (per shaping Shape A —
//! `FsWalker::try_new`); this struct stays infallible POD.

use crate::domain::types::SourceRoot;
use serde::{Deserialize, Serialize};

/// Caller-supplied configuration for one analysis run.
///
/// `src` carries the workspace root the adapter walks; `exclude` is a
/// list of raw user globs the adapter compiles into a negative
/// override matcher; `extensions` is the bare-extension whitelist
/// (`["rs"]` for `scrap4rs`, `["ts", "tsx", ...]` for future
/// `scrap4ts`); `respect_gitignore` toggles `.gitignore` /
/// `.ignore` / `.git/info/exclude` honouring at the walk layer.
///
/// Construction is infallible — the canonical `::new` constructor
/// stores the raw `exclude` strings without compilation. Glob
/// validation lives in `FsWalker::try_new`, which surfaces invalid
/// patterns as `SourceError::InvalidGlob`. Keeping validation in the
/// adapter keeps `domain/` free of `globset::Error` (per shaping
/// Shape A — adapter-owns-validation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisConfig {
    /// Workspace root passed into `SourcePort::discover_test_files`.
    pub src: SourceRoot,
    /// Raw user-supplied exclude globs. Adapter compiles to an
    /// `ignore::overrides::Override` at `try_new` time.
    pub exclude: Vec<String>,
    /// Bare extensions (no leading dot, case-insensitive) the walker
    /// keeps. Empty list means include every file the walker visits.
    pub extensions: Vec<String>,
    /// When `true`, the walker honours `.gitignore`, `.ignore`, and
    /// `.git/info/exclude` (rg/fd default behaviour).
    pub respect_gitignore: bool,
}

impl AnalysisConfig {
    /// Canonical constructor (D10). Infallible — adapter validates
    /// `exclude` patterns when building the walker.
    #[must_use]
    pub fn new(
        src: SourceRoot,
        exclude: Vec<String>,
        extensions: Vec<String>,
        respect_gitignore: bool,
    ) -> Self {
        Self {
            src,
            exclude,
            extensions,
            respect_gitignore,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::SourceRoot;

    #[test]
    fn analysis_config_wire_keys() {
        let cfg = AnalysisConfig::new(
            SourceRoot::new("crates/scrap-core"),
            vec!["vendored/**".into()],
            vec!["rs".into()],
            true,
        );
        let json = serde_json::to_value(&cfg).unwrap();
        for key in ["src", "exclude", "extensions", "respect_gitignore"] {
            assert!(json.get(key).is_some(), "missing wire key: {key}");
        }
    }
}
