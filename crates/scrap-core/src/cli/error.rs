//! `InitError` — typed error surface for the `init` subcommand.
//!
//! Fresh enum (not a wrap of [`crate::cli::config::ConfigError`]) —
//! init's failure modes (`Exists` vs `Io`) are semantically distinct
//! from the loader's (`Io` vs `Parse` vs `InvalidGlob` vs
//! `InvalidValue`) even though both touch the same `<adapter>.toml`
//! file. The init module owns its own error enum to keep the
//! diagnostic shape independent of loader churn.
//!
//! `#[non_exhaustive]` per `adr-nested-json-envelope` enum discipline —
//! adapters / future variants land additively without breaking
//! pattern-matching consumers.

use std::path::PathBuf;

/// Errors produced by `cli::init::handle_init` /
/// `handle_init_with_io` (lands in scrap-rs#21 W3).
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    /// Config file exists; pass `--force` to overwrite. The
    /// human-facing remediation hint is part of the Display string so
    /// `eprintln!("{err}")` in the CLI surface carries the actionable
    /// next-step without further wrapping.
    #[error("{} already exists; pass --force to overwrite", path.display())]
    Exists {
        /// Path the caller attempted to write.
        path: PathBuf,
    },
    /// I/O failure writing the generated config.
    #[error("failed to write {}", path.display())]
    Io {
        /// Path the writer attempted to create.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn init_error_exists_display_contains_path_and_force_hint() {
        let err = InitError::Exists {
            path: PathBuf::from("/tmp/test-adapter.toml"),
        };
        let display = err.to_string();
        assert!(
            display.contains("/tmp/test-adapter.toml"),
            "Display must surface the path; got: {display}",
        );
        assert!(
            display.contains("--force"),
            "Display must hint the --force escape hatch; got: {display}",
        );
        // No #[source] on Exists — no underlying error to chain.
        assert!(err.source().is_none());
    }

    #[test]
    fn init_error_io_preserves_source_via_std_error_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "boom");
        let err = InitError::Io {
            path: PathBuf::from("/tmp/test-adapter.toml"),
            source: io_err,
        };
        let display = err.to_string();
        assert!(display.contains("/tmp/test-adapter.toml"));
        // Source chain reaches the underlying io::Error so anyhow /
        // eyre callers walking #[source] see the root cause.
        let src = err.source().expect("Io variant must chain #[source]");
        assert!(
            src.to_string().contains("boom"),
            "source chain must reach the underlying io::Error; got: {src}",
        );
    }
}
