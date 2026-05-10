//! `FsWalker` — disk-backed `SourcePort` impl built on `ignore::WalkBuilder`.

use crate::domain::config::AnalysisConfig;
use crate::domain::source::DiscoveryOutcome;
use crate::domain::types::SourceRoot;
use crate::ports::source::{SourceError, SourcePort};
use ignore::overrides::{Override, OverrideBuilder};

/// Disk-backed `SourcePort` implementation.
///
/// Construction (`FsWalker::try_new`) eagerly compiles every user
/// exclude glob via `globset` and assembles an `ignore::overrides::Override`
/// matcher. Failures surface as `SourceError::InvalidGlob` (per-pattern
/// compile error) or `SourceError::Ignore` (the rare
/// `OverrideBuilder::build()` rejection — see the variant docstring).
///
/// The walk itself runs lazily inside
/// [`SourcePort::discover_test_files`] (lands in V5 of scrap-rs#13).
#[derive(Debug, Clone)]
pub struct FsWalker {
    /// Caller-supplied configuration. Stored verbatim — the walker
    /// re-reads `extensions`, `respect_gitignore`, etc. per call.
    config: AnalysisConfig,
    /// Pre-compiled negative-override matcher built from
    /// `config.exclude` at `try_new` time. `Override` is internally
    /// `Arc`-backed, so cloning the walker is cheap.
    override_matcher: Override,
}

impl FsWalker {
    /// Construct a walker, eagerly validating every entry in
    /// `config.exclude`.
    ///
    /// # Errors
    ///
    /// - [`SourceError::InvalidGlob`] when one of the user-supplied
    ///   exclude patterns fails `globset::Glob::new`. The variant's
    ///   `pattern` field carries the offending raw pattern.
    /// - [`SourceError::Ignore`] when `OverrideBuilder::build()`
    ///   rejects the assembled matcher despite each individual
    ///   `.add()` call having succeeded. Forward-compat hatch — see
    ///   the variant docstring.
    pub fn try_new(config: AnalysisConfig) -> Result<Self, SourceError> {
        // Validate each user exclude pattern via globset first so we
        // can surface the offending raw pattern as
        // SourceError::InvalidGlob (the OverrideBuilder swallows the
        // pattern text in its own error message).
        for pattern in &config.exclude {
            globset::Glob::new(pattern).map_err(|source| SourceError::InvalidGlob {
                pattern: pattern.clone(),
                source,
            })?;
        }

        let mut builder = OverrideBuilder::new(config.src.as_path());
        for pattern in &config.exclude {
            // Negative override (leading `!`) excludes matching paths.
            // Per-call .add() failure is treated the same as
            // OverrideBuilder::build() failure — forward-compat hatch.
            builder
                .add(&format!("!{pattern}"))
                .map_err(|source| SourceError::Ignore { source })?;
        }

        let override_matcher = builder
            .build()
            .map_err(|source| SourceError::Ignore { source })?;

        Ok(Self {
            config,
            override_matcher,
        })
    }
}

impl SourcePort for FsWalker {
    #[allow(clippy::unimplemented)]
    fn discover_test_files(&self, _root: &SourceRoot) -> Result<DiscoveryOutcome, SourceError> {
        // V5 replaces this placeholder with the real WalkBuilder loop.
        // Touch the fields so the placeholder does not regress to dead
        // code if V5 is delayed.
        let _ = (&self.config, &self.override_matcher);
        unimplemented!("scrap-rs#13 V5: implement discover_test_files");
    }
}

#[cfg(test)]
static_assertions::assert_impl_all!(FsWalker: Send, Sync);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::config::AnalysisConfig;
    use crate::domain::types::SourceRoot;

    fn cfg(exclude: Vec<String>) -> AnalysisConfig {
        AnalysisConfig::new(SourceRoot::new("."), exclude, vec!["rs".into()], true)
    }

    #[test]
    fn try_new_with_no_excludes_returns_ok() {
        let walker = FsWalker::try_new(cfg(vec![]));
        assert!(walker.is_ok(), "expected Ok, got {:?}", walker.err());
    }

    #[test]
    fn try_new_with_valid_excludes_returns_ok() {
        let walker = FsWalker::try_new(cfg(vec!["vendored/**".into(), "target/**".into()]));
        assert!(walker.is_ok(), "expected Ok, got {:?}", walker.err());
    }

    #[test]
    fn try_new_with_invalid_glob_returns_invalid_glob_error() {
        let walker = FsWalker::try_new(cfg(vec!["[unclosed".into()]));
        match walker {
            Err(SourceError::InvalidGlob { pattern, .. }) => {
                assert_eq!(pattern, "[unclosed");
            }
            other => panic!("expected SourceError::InvalidGlob, got {other:?}"),
        }
    }
}
