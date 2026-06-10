//! `init` subcommand — generates a starter `<adapter>.toml` in the
//! current working directory.
//!
//! Lives in `scrap-core::cli` (not the per-adapter binary crate) so
//! every adapter inherits the subcommand for free via `AdapterMeta`.
//! The generator is parameterized on three meta fields:
//!
//! - `config_file_name` — the config-file literal (the unified
//!   `scrap.toml`, shared by every adapter).
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

use std::fmt::Write as _;
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

    // Detect layout relative to the config path's parent (PR #91
    // Gemini HIGH fix). `config_path.parent()` returns `None` for
    // bare-filename paths (e.g. when the caller passes a `Path` built
    // from `meta.config_file_name` directly with no parent dir) — in
    // that case the empty path is equivalent to "current directory"
    // for `Path::join`/`is_dir` semantics, matching the prior CWD
    // behavior for the common bare-filename caller path.
    let base_dir = config_path.parent().unwrap_or_else(|| Path::new(""));
    let detection = detect_src_layout(base_dir);
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

/// Auto-detect the source directory relative to `base_dir`. `src/`
/// wins; `crates/` second; otherwise fall back to `"src"` with
/// `is_fallback = true` so the generator stamps a hint comment.
///
/// Takes `base_dir` as a parameter so detection runs against the
/// target directory (typically `config_path.parent()`) rather than
/// the process CWD (PR #91 Gemini HIGH fix).
pub(crate) fn detect_src_layout(base_dir: &Path) -> SrcDetection {
    if base_dir.join("src").is_dir() {
        SrcDetection {
            src_path: "src".to_string(),
            is_fallback: false,
        }
    } else if base_dir.join("crates").is_dir() {
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
    out.push_str("# ]\n\n");

    // Exhaustive commented reference for every remaining FileConfig
    // section (scrap-rs#107). Everything below is commented out so a
    // fresh `init` file is behavior-identical to defaults AND
    // round-trips through `load_config` cleanly (the
    // `handle_init_with_io_writes_loader_round_trip` guard). The
    // generated output doubles as the committed `scrap.example.toml`
    // — kept honest by a byte-identity sync test ("documentation
    // rots; CI doesn't").

    // Extensions.
    out.push_str("# File extensions the walker keeps (no leading dot). Defaults to the\n");
    out.push_str("# adapter's own list; an empty array means \"every file the walker visits\".\n");
    let extensions = meta
        .extensions
        .iter()
        .map(|ext| format!("\"{ext}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(out, "# extensions = [{extensions}]\n");

    // Opt-out policy.
    out.push_str("# Which per-test `#[allow(scrap::*)]` suppressions the project honors.\n");
    out.push_str("# Omit the table to honor all; `honor = []` is the strictest policy\n");
    out.push_str("# (every opt-out ignored, smells always fire).\n");
    out.push_str("# [opt_outs]\n");
    out.push_str("# honor = [\"no_asserts\", \"tautology\", \"no_op\"]\n\n");

    // Per-detector tables — penalties + threshold formatted from the
    // canonical `DEFAULT_*` consts in `crate::detectors::*` so the
    // annotated values can never drift from the code (Gemini review,
    // PR #129 — the same "documentation rots; CI doesn't" rationale as
    // the sync test, enforced at compile time instead).
    out.push_str("# Per-detector tunables. Every detector is enabled by default; the\n");
    out.push_str("# values shown ARE the defaults, so uncommenting without editing\n");
    out.push_str("# changes nothing. `penalty = 0` is rejected (silently-neutering);\n");
    out.push_str("# disable a detector with `enabled = false` instead.\n");
    let detector_defaults: [(&str, u32, Option<u32>); 5] = [
        (
            "zero_assertion",
            crate::detectors::zero_assertion::DEFAULT_PENALTY,
            None,
        ),
        (
            "tautological_assertion",
            crate::detectors::tautological_assertion::DEFAULT_PENALTY,
            None,
        ),
        (
            "no_op_io",
            crate::detectors::no_op_io::DEFAULT_PENALTY,
            None,
        ),
        (
            "surface_only_io",
            crate::detectors::surface_only_io::DEFAULT_PENALTY,
            None,
        ),
        (
            "large_example",
            crate::detectors::large_example::DEFAULT_PENALTY,
            Some(crate::detectors::large_example::DEFAULT_LINE_THRESHOLD),
        ),
    ];
    for (i, (name, penalty, line_threshold)) in detector_defaults.iter().enumerate() {
        if i > 0 {
            out.push_str("#\n");
        }
        let _ = writeln!(out, "# [detectors.{name}]");
        out.push_str("# enabled = true\n");
        let _ = writeln!(out, "# penalty = {penalty}");
        if let Some(threshold) = line_threshold {
            let _ = writeln!(
                out,
                "# line_threshold = {threshold}  # only valid on {name}"
            );
        }
    }
    out.push('\n');

    // Overrides.
    out.push_str("# Glob-matched overrides — tune or disable detectors for matching\n");
    out.push_str("# paths. Among multiple matching blocks the LAST one in document\n");
    out.push_str("# order wins per detector key.\n");
    out.push_str("# [[overrides]]\n");
    out.push_str("# match = [\"tests/fixtures/**\"]\n");
    out.push_str("# [overrides.detectors.large_example]\n");
    out.push_str("# line_threshold = 60\n");

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

    // No CwdGuard needed: PR #91 Gemini HIGH fix made
    // `detect_src_layout(base_dir)` parameterized. Every test below
    // uses a per-test tempdir as the `base_dir` arg, so concurrent
    // nextest runners never race on process cwd.

    // ── detect_src_layout ────────────────────────────────────────────

    #[test]
    fn detect_src_layout_prefers_src_over_crates() {
        let tempdir = tempfile::tempdir().unwrap();
        std::fs::create_dir(tempdir.path().join("src")).unwrap();
        std::fs::create_dir(tempdir.path().join("crates")).unwrap();
        let det = detect_src_layout(tempdir.path());
        assert_eq!(det.src_path, "src");
        assert!(!det.is_fallback);
    }

    #[test]
    fn detect_src_layout_falls_back_when_neither_exists() {
        let tempdir = tempfile::tempdir().unwrap();
        // Empty tempdir — no src/, no crates/.
        let det = detect_src_layout(tempdir.path());
        assert_eq!(det.src_path, "src");
        assert!(det.is_fallback);
    }

    #[test]
    fn detect_src_layout_finds_crates_when_only_crates_exists() {
        let tempdir = tempfile::tempdir().unwrap();
        std::fs::create_dir(tempdir.path().join("crates")).unwrap();
        let det = detect_src_layout(tempdir.path());
        assert_eq!(det.src_path, "crates");
        assert!(!det.is_fallback);
    }

    #[test]
    fn detect_src_layout_empty_base_dir_is_cwd_relative() {
        // PR #91 — `Path::new("")` is the conventional CWD-relative
        // base when `config_path.parent()` returns None for a bare
        // filename. Sanity-check the empty path doesn't panic; the
        // detection result depends on the test runner cwd so we just
        // assert the call returns a valid struct.
        let det = detect_src_layout(Path::new(""));
        assert!(!det.src_path.is_empty());
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
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("test-adapter.toml");
        let meta = fixture_meta();
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(false, &meta, &path, &mut stderr).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
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
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("test-adapter.toml");
        std::fs::write(&path, "legacy = true\n").unwrap();
        let meta = fixture_meta();
        let mut stderr: Vec<u8> = Vec::new();
        let err = handle_init_with_io(false, &meta, &path, &mut stderr)
            .expect_err("must bail without --force");
        match err {
            InitError::Exists { path: p } => {
                assert_eq!(p, path);
            }
            other => panic!("expected InitError::Exists, got {other:?}"),
        }
        // File content unchanged.
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "legacy = true\n");
    }

    #[test]
    fn handle_init_with_io_overwrites_with_force() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("test-adapter.toml");
        std::fs::write(&path, "legacy = true\n").unwrap();
        let meta = fixture_meta();
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(true, &meta, &path, &mut stderr).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(!contents.contains("legacy = true"));
        assert!(contents.contains("src = \"src\""));
    }

    #[test]
    fn handle_init_with_io_detects_crates_layout() {
        let tempdir = tempfile::tempdir().unwrap();
        // Create crates/ relative to the tempdir (the base_dir
        // computed from config_path.parent()).
        std::fs::create_dir(tempdir.path().join("crates")).unwrap();
        let path = tempdir.path().join("test-adapter.toml");
        let meta = fixture_meta();
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(false, &meta, &path, &mut stderr).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
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
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("test-adapter.toml");
        let meta = fixture_meta();
        let mut stderr: Vec<u8> = Vec::new();
        handle_init_with_io(false, &meta, &path, &mut stderr).unwrap();
        let loaded = load_config(&path)
            .expect("init's generated TOML must load via load_config without error");
        // src is set per detection (empty tempdir → fallback path).
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
