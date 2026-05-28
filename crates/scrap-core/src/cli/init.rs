//! `init` subcommand — generates a starter `<adapter>.toml` in the
//! current working directory.
//!
//! Lives in `scrap-core::cli` (not the per-adapter binary crate) so
//! every adapter inherits the subcommand for free via `AdapterMeta`.
//! The generator is parameterized on three meta fields:
//!
//! - `config_file_name` — the per-adapter literal (e.g.,
//!   `scrap4rs.toml`, future `scrap4ts.toml`).
//! - `tool_name` — surfaced in the header comment + the next-step
//!   hint printed to stderr after a successful write.
//! - `default_excludes` — commented-out exclude patterns the
//!   generator stamps into the template.
//!
//! Auto-detect rules for `src` (in order):
//!   1. `src/` exists → use `"src"` (single-crate Rust layout).
//!   2. `crates/` exists → use `"crates"` (Cargo workspace).
//!   3. Neither → fall back to `"src"` with a hint comment so users
//!      see the toggle point.
//!
//! Per FORK-5, v0.1 `init` is **non-interactive only**.
//! `--non-interactive` is accepted as a no-op forward-compat flag;
//! the future v0.2 interactive prompt will consume it. No TTY
//! detection; no stdin reads.
//!
//! `init` always emits `# threshold_mode = "default"` COMMENTED in
//! the template. `FileConfig` does NOT yet have a `threshold_mode`
//! field (CLI-only flag in v0.1); uncommenting would trip
//! `#[serde(deny_unknown_fields)]` at first `load_config()` call —
//! the "I just ran init; why does this fail?" UX bug the cabinet
//! advisor pass caught. Round-trip-clean is verified via a
//! dedicated unit test.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::adapter_meta::AdapterMeta;
use crate::cli::error::InitError;

/// Handle the `init` subcommand. Writes a starter config to
/// `meta.config_file_name` in the current directory.
///
/// Returns `Ok(())` on success. Returns
/// [`InitError::Exists`] when the file already exists and `--force`
/// was not passed; [`InitError::Io`] when the write fails.
///
/// `_non_interactive` is accepted-but-ignored at v0.1 per FORK-5;
/// reserved for the v0.2 interactive-prompt addition.
///
/// # Errors
///
/// See [`InitError`] for the typed surface.
pub fn handle_init(
    force: bool,
    _non_interactive: bool,
    meta: &AdapterMeta,
) -> Result<(), InitError> {
    handle_init_with_io(
        force,
        meta,
        Path::new(meta.config_file_name),
        &mut io::stderr(),
    )
}

/// Inner handler that takes the config-file path + writer as
/// parameters so unit tests drive the file write + stderr capture
/// against a tempdir without spawning a subprocess.
///
/// Mirrors crap-rs's `handle_init_with_io` pattern minus the
/// `R: BufRead` stdin param — v0.1 has no interactive prompt
/// (FORK-5). The reader param can be added additively when the v0.2
/// prompt lands.
///
/// # Errors
///
/// See [`InitError`].
pub fn handle_init_with_io<W: Write>(
    force: bool,
    meta: &AdapterMeta,
    config_path: &Path,
    stderr: &mut W,
) -> Result<(), InitError> {
    if config_path.exists() && !force {
        return Err(InitError::Exists {
            path: config_path.to_path_buf(),
        });
    }

    let detection = detect_src_layout();
    let content = render_config(meta, &detection);

    fs::write(config_path, &content).map_err(|source| InitError::Io {
        path: config_path.to_path_buf(),
        source,
    })?;

    writeln!(
        stderr,
        "wrote {name} (src = \"{src}\")",
        name = meta.config_file_name,
        src = detection.src_path,
    )
    .ok();
    writeln!(
        stderr,
        "  next: run `{name} --src {src} --format json` to analyze, or edit the generated TOML to tune detectors.",
        name = meta.tool_name,
        src = detection.src_path,
    )
    .ok();

    Ok(())
}

/// Result of [`detect_src_layout`] — the path string to write into
/// the TOML, plus whether the value came from a real directory or
/// from the fallback. The fallback flag drives a hint comment in
/// the generated file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SrcDetection {
    /// Path string to write as `src = "..."`.
    pub src_path: String,
    /// `true` when neither `src/` nor `crates/` exists; the
    /// generator emits a hint comment so the user sees the toggle
    /// point.
    pub is_fallback: bool,
}

/// Auto-detect the source directory. `src/` wins; `crates/`
/// second; otherwise fall back to `"src"` with `is_fallback = true`
/// so the generator stamps a hint comment.
pub(crate) fn detect_src_layout() -> SrcDetection {
    if Path::new("src").is_dir() {
        SrcDetection {
            src_path: "src".to_string(),
            is_fallback: false,
        }
    } else if Path::new("crates").is_dir() {
        SrcDetection {
            src_path: "crates".to_string(),
            is_fallback: false,
        }
    } else {
        SrcDetection {
            src_path: "src".to_string(),
            is_fallback: true,
        }
    }
}

/// Render the starter config TOML. Hand-templated rather than
/// serialized via the `toml` crate because the generated file is
/// intentionally commented (the `toml` crate drops comments); a
/// constant template with a few substitutions (src + commented
/// excludes from `meta.default_excludes`) is simpler and gives us
/// complete control over comment placement.
///
/// `threshold_mode = "default"` is emitted COMMENTED OUT because
/// `FileConfig` doesn't have the field yet (CLI-only v0.1).
/// Uncommenting would break `load_config` on first run. Unit-tested
/// via `handle_init_with_io_writes_loader_round_trip`.
pub(crate) fn render_config(meta: &AdapterMeta, detection: &SrcDetection) -> String {
    let mut out = String::with_capacity(1024);

    // Header — anchors generated files so future audits can grep for
    // "generated by `<tool> init`" to find untouched starter configs.
    out.push_str("# ");
    out.push_str(meta.config_file_name);
    out.push_str(" — generated by `");
    out.push_str(meta.tool_name);
    out.push_str(" init`\n");
    out.push_str("# Edit freely; the analyzer re-reads this file on every run.\n\n");

    // Source root.
    out.push_str("# Source root the analyzer walks.\n");
    if detection.is_fallback {
        out.push_str("# (auto-detect found no `src/` or `crates/` directory — adjust if your sources live elsewhere)\n");
    }
    out.push_str("src = \"");
    out.push_str(&detection.src_path);
    out.push_str("\"\n\n");

    // Threshold mode — COMMENTED per advisor-pass fix.
    out.push_str("# Threshold mode for the gate verdict. Currently a CLI-only flag\n");
    out.push_str("# (`--threshold-mode strict|default|lenient`); the config-file mirror\n");
    out.push_str("# lands in a follow-up PR. Uncomment to set when the field arrives.\n");
    out.push_str("# threshold_mode = \"default\"\n\n");

    // Excludes — emitted as a single commented-out array so users
    // can uncomment + tweak in one step. Per-adapter defaults come
    // from `AdapterMeta.default_excludes`.
    out.push_str("# Glob patterns matched against project-relative file paths.\n");
    out.push_str("# Uncomment to ignore these directories:\n");
    out.push_str("# exclude = [\n");
    for pattern in meta.default_excludes {
        out.push_str("#     \"");
        out.push_str(pattern);
        out.push_str("\",\n");
    }
    out.push_str("# ]\n");

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter_meta::AdapterMeta;
    use crate::cli::config::load_config;

    /// Test-fixture meta. Adapter-name-agnostic per the source-only
    /// purity CI gate — uses `test-adapter` placeholder. 13 fields.
    fn fixture_meta() -> AdapterMeta {
        AdapterMeta {
            tool_name: "test-adapter",
            language: "rust",
            tool_version: "0.1.0",
            long_version: "0.1.0 (test 2026-05-27)",
            about: "init-test fixture",
            long_about: "Test-fixture AdapterMeta for cli::init tests.",
            after_help: "",
            extensions: &["rs"],
            tool_info_uri: "https://example.invalid/scrap",
            rule_help_uri: "https://example.invalid/scrap/rules",
            config_file_name: "test-adapter.toml",
            default_excludes: &["tests/**", "benches/**", "examples/**"],
            parse_hint: "ensure --src points at a workspace with test files",
        }
    }

    /// Scope-guarded chdir + tempdir RAII guard. Captures the
    /// process cwd on construction, chdirs into the tempdir, and
    /// restores cwd on Drop (even on panic) per
    /// `feedback_pristine-test-output`. Mirrors the `PermissionGuard`
    /// pattern in `cli/config.rs::tests` for the chmod analog.
    ///
    /// `cargo test` runs tests on a single thread by default in some
    /// runners; nextest parallelizes per-test. Tests that mutate
    /// process cwd MUST serialize via this guard or risk other
    /// tests racing on the mutated cwd. We accept that risk for the
    /// `detect_src_layout` tests because (a) they're scoped to this
    /// module and (b) they construct their own tempdirs inside the
    /// guard. The kernel cwd mutation is the unavoidable cost of
    /// testing `Path::new("src").is_dir()` semantics.
    struct CwdGuard {
        original: std::path::PathBuf,
        _tempdir: tempfile::TempDir,
    }

    impl CwdGuard {
        fn enter() -> Self {
            let tempdir = tempfile::tempdir().expect("tempdir creation");
            let original = std::env::current_dir().expect("current_dir");
            std::env::set_current_dir(tempdir.path()).expect("chdir to tempdir");
            Self {
                original,
                _tempdir: tempdir,
            }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            // Best-effort restore — ignore failure so panics in the
            // test body propagate cleanly.
            let _ = std::env::set_current_dir(&self.original);
        }
    }

    // ── detect_src_layout ────────────────────────────────────────────

    #[test]
    fn detect_src_layout_prefers_src_over_crates() {
        let _guard = CwdGuard::enter();
        std::fs::create_dir("src").unwrap();
        std::fs::create_dir("crates").unwrap();
        let det = detect_src_layout();
        assert_eq!(det.src_path, "src");
        assert!(!det.is_fallback);
    }

    #[test]
    fn detect_src_layout_falls_back_when_neither_exists() {
        let _guard = CwdGuard::enter();
        // Empty tempdir — no src/, no crates/.
        let det = detect_src_layout();
        assert_eq!(det.src_path, "src");
        assert!(det.is_fallback);
    }

    // ── render_config ───────────────────────────────────────────────

    #[test]
    fn render_config_emits_commented_threshold_mode() {
        // Cabinet advisor-pass fix: threshold_mode MUST be commented
        // because FileConfig doesn't have the field yet
        // (#[serde(deny_unknown_fields)] would reject the unknown key
        // on first load_config call). Pinning this assertion guards
        // the bug closed permanently.
        let meta = fixture_meta();
        let det = SrcDetection {
            src_path: "src".to_string(),
            is_fallback: false,
        };
        let out = render_config(&meta, &det);
        assert!(
            out.contains("# threshold_mode = \"default\""),
            "threshold_mode MUST be commented; got:\n{out}",
        );
        assert!(
            !out.contains("\nthreshold_mode = \"default\""),
            "threshold_mode MUST NOT be uncommented; got:\n{out}",
        );
    }

    #[test]
    fn render_config_emits_commented_excludes_from_meta() {
        let meta = fixture_meta();
        let det = SrcDetection {
            src_path: "src".to_string(),
            is_fallback: false,
        };
        let out = render_config(&meta, &det);
        assert!(
            out.contains("# exclude = ["),
            "expected `# exclude = [` line"
        );
        assert!(out.contains("tests/**"));
        assert!(out.contains("benches/**"));
        assert!(out.contains("examples/**"));
    }

    #[test]
    fn render_config_emits_fallback_hint_only_when_detection_failed() {
        let meta = fixture_meta();
        let detected = SrcDetection {
            src_path: "src".to_string(),
            is_fallback: false,
        };
        let fallback = SrcDetection {
            src_path: "src".to_string(),
            is_fallback: true,
        };
        let with_detect = render_config(&meta, &detected);
        let with_fallback = render_config(&meta, &fallback);
        assert!(!with_detect.contains("adjust if your sources live elsewhere"));
        assert!(with_fallback.contains("adjust if your sources live elsewhere"));
    }

    // ── handle_init_with_io ─────────────────────────────────────────

    #[test]
    fn handle_init_with_io_writes_default_in_empty_tempdir() {
        let _guard = CwdGuard::enter();
        let meta = fixture_meta();
        let path = Path::new("test-adapter.toml");
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(false, &meta, path, &mut stderr).unwrap();
        let contents = std::fs::read_to_string(path).unwrap();
        assert!(contents.contains("src = \"src\""));
        assert!(contents.contains("# threshold_mode = \"default\""));
        let stderr_str = String::from_utf8(stderr).unwrap();
        assert!(
            stderr_str.contains("wrote test-adapter.toml"),
            "stderr must surface the write; got: {stderr_str}",
        );
    }

    #[test]
    fn handle_init_with_io_bails_when_exists_without_force() {
        let _guard = CwdGuard::enter();
        let path = Path::new("test-adapter.toml");
        std::fs::write(path, "legacy = true\n").unwrap();
        let meta = fixture_meta();
        let mut stderr: Vec<u8> = Vec::new();
        let err = handle_init_with_io(false, &meta, path, &mut stderr)
            .expect_err("must bail without --force");
        match err {
            InitError::Exists { path: p } => {
                assert_eq!(p, path.to_path_buf());
            }
            other => panic!("expected InitError::Exists, got {other:?}"),
        }
        // File content unchanged.
        let contents = std::fs::read_to_string(path).unwrap();
        assert_eq!(contents, "legacy = true\n");
    }

    #[test]
    fn handle_init_with_io_overwrites_with_force() {
        let _guard = CwdGuard::enter();
        let path = Path::new("test-adapter.toml");
        std::fs::write(path, "legacy = true\n").unwrap();
        let meta = fixture_meta();
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(true, &meta, path, &mut stderr).unwrap();
        let contents = std::fs::read_to_string(path).unwrap();
        assert!(!contents.contains("legacy = true"));
        assert!(contents.contains("src = \"src\""));
    }

    #[test]
    fn handle_init_with_io_detects_crates_layout() {
        let _guard = CwdGuard::enter();
        std::fs::create_dir("crates").unwrap();
        let meta = fixture_meta();
        let path = Path::new("test-adapter.toml");
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(false, &meta, path, &mut stderr).unwrap();
        let contents = std::fs::read_to_string(path).unwrap();
        assert!(
            contents.contains("src = \"crates\""),
            "crates/ detection failed; got:\n{contents}",
        );
    }

    #[test]
    fn handle_init_with_io_writes_loader_round_trip() {
        // Cabinet advisor-pass fix: the generated TOML must
        // load_config()-round-trip cleanly. If render_config emits any
        // key that FileConfig doesn't recognize, deny_unknown_fields
        // trips here. This test pins the "init's output is loadable"
        // contract.
        let _guard = CwdGuard::enter();
        let meta = fixture_meta();
        let path = Path::new("test-adapter.toml");
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(false, &meta, path, &mut stderr).unwrap();
        // Use absolute path so load_config is independent of cwd
        // (CwdGuard could restore cwd before load_config fires if
        // the test panics inside load_config; absolute path
        // sidesteps that).
        let abs = std::fs::canonicalize(path).unwrap();
        let loaded = load_config(&abs)
            .expect("init's generated TOML must load via load_config without error");
        // src is set per detection (default = "src" in this empty
        // tempdir → fallback path).
        assert_eq!(loaded.src.as_deref(), Some(Path::new("src")));
    }

    // ── CQO SF-1 (cabinet fold) — unwritable-path InitError::Io ────

    #[cfg(unix)]
    struct ChmodGuard {
        path: std::path::PathBuf,
        restore_mode: u32,
    }

    #[cfg(unix)]
    impl Drop for ChmodGuard {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                &self.path,
                std::fs::Permissions::from_mode(self.restore_mode),
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn handle_init_with_io_returns_io_error_on_unwritable_path() {
        // CQO SF-1: exercise the InitError::Io production path
        // end-to-end. Chmod-strip write permission from the target's
        // parent directory so fs::write fails with PermissionDenied.
        // RAII guard restores chmod on drop (panic-safe) per
        // feedback_pristine-test-output.
        use std::os::unix::fs::PermissionsExt;
        let tempdir = tempfile::tempdir().unwrap();
        let locked_dir = tempdir.path().join("locked");
        std::fs::create_dir(&locked_dir).unwrap();
        // Strip write perm; keep read+execute so we can stat.
        std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let _guard = ChmodGuard {
            path: locked_dir.clone(),
            restore_mode: 0o755,
        };
        let target = locked_dir.join("test-adapter.toml");
        let meta = fixture_meta();
        let mut stderr: Vec<u8> = Vec::new();
        let err = handle_init_with_io(false, &meta, &target, &mut stderr)
            .expect_err("unwritable parent must surface InitError::Io");
        match err {
            InitError::Io { path, source } => {
                assert_eq!(path, target);
                assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied);
            }
            other => panic!("expected InitError::Io, got {other:?}"),
        }
    }
}
