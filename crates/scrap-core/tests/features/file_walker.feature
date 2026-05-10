# Executable behavioral contract for the file walker adapter (scrap-rs#13).
# Run via the cucumber-rs harness at `crates/scrap-core/tests/cucumber.rs`
# (`cargo test -p scrap-core --test cucumber`). Trait-surface compile-time
# invariants live in `crates/scrap-core/tests/source_walker.rs` (per V6).
# Mid-walk SourceDiagnosticKind branches `MidwalkIo` and `Other` are
# unit-tested in `crates/scrap-core/src/adapters/source/fs.rs` against
# hand-constructed `ignore::Error` values (per shaping A7b).

Feature: SourcePort file walker discovers test files under a SourceRoot
  As a caller of `core::analyze` (or an integration-test author)
  I want a `SourcePort` implementation that enumerates candidate test files
  So that detectors and the future orchestration loop can consume a
  deterministic, language-agnostic list of paths to parse.

  Background:
    Given a fresh test World

  # ─── Happy path: empty / single / nested trees ─────────────────────────

  Scenario: empty directory yields an empty DiscoveryOutcome
    Given a temporary directory containing no files
    And an `AnalysisConfig` with `extensions = ["rs"]` and `respect_gitignore = true`
    And an `FsWalker` constructed from that config
    When the caller invokes `discover_test_files(root)` against the temporary directory
    Then the result is `Ok` and `files` is empty
    And `diagnostics` is empty

  Scenario: single-file directory returns the file
    Given a temporary directory containing exactly `a.rs`
    And an `AnalysisConfig` with `extensions = ["rs"]`
    When the caller invokes `discover_test_files(root)`
    Then the result is `Ok` and `files` equals (in order):
      | path |
      | a.rs |
    And `diagnostics` is empty

  Scenario: nested tree returns all matching files in deterministic post-collect byte-wise order
    Given a temporary directory with the following structure:
      | path        |
      | a.rs        |
      | a/b.rs      |
      | a/sub/c.rs  |
      | a/sub/d.rs  |
      | b.rs        |
    And an `AnalysisConfig` with `extensions = ["rs"]`
    When the caller invokes `discover_test_files(root)` twice
    Then both invocations return `Ok` with the exact same `files` (in order):
      | path        |
      | a.rs        |
      | a/b.rs      |
      | a/sub/c.rs  |
      | a/sub/d.rs  |
      | b.rs        |
    # Pinned to the post-collect Vec::sort() order from shaping E1. A buggy
    # implementation using WalkBuilder::sort_by_file_path would produce
    # depth-first sibling-sorted output (different ordering for sibling
    # file/dir pairs like `a.rs` vs `a/b.rs`) and would fail this assertion.

  # ─── VCS ignore semantics ──────────────────────────────────────────────

  Scenario Outline: respect_gitignore controls whether .gitignore-listed files are skipped
    Given a temporary directory containing `keep.rs`, `skip.rs`, and a `.gitignore` listing `skip.rs`
    And an `AnalysisConfig` with `extensions = ["rs"]` and `respect_gitignore = <respect>`
    When the caller invokes `discover_test_files(root)`
    Then the result is `Ok` and `files` contains exactly:
      | path             |
      | <expected_files> |

    Examples:
      | respect | expected_files       |
      | true    | keep.rs              |
      | false   | keep.rs;skip.rs      |
    # Step definition splits the `expected_files` cell on `;` to get the
    # multi-row case under a single Examples row.

  Scenario: hidden files (dot-prefixed) are skipped by default
    Given a temporary directory containing `visible.rs` and `.hidden.rs`
    And an `AnalysisConfig` with `extensions = ["rs"]`
    When the caller invokes `discover_test_files(root)`
    Then the result is `Ok` and `files` contains exactly:
      | path       |
      | visible.rs |

  # ─── User-glob exclude semantics ───────────────────────────────────────

  Scenario: user-glob exclude pattern filters matching files out
    Given a temporary directory containing `keep.rs` and `vendored/skip.rs`
    And an `AnalysisConfig` with `exclude = ["vendored/**"]` and `extensions = ["rs"]`
    When the caller constructs `FsWalker::try_new(config)`
    And the caller invokes `discover_test_files(root)`
    Then the result is `Ok` and `files` contains exactly:
      | path    |
      | keep.rs |

  Scenario: invalid user-glob pattern is rejected at adapter construction (pre-walk fatal)
    Given an `AnalysisConfig` with `exclude = ["[unclosed"]`
    When the caller constructs `FsWalker::try_new(config)`
    Then the result is `Err(SourceError::InvalidGlob)` with `pattern = "[unclosed"`
    And the underlying `source` is a `globset::Error`
    And no walk has begun

  # ─── Extension filter semantics ────────────────────────────────────────

  Scenario: extension filter is case-insensitive and bare ("rs", not ".rs")
    Given a temporary directory containing `a.rs`, `b.RS`, and `c.txt`
    And an `AnalysisConfig` with `extensions = ["rs"]`
    When the caller invokes `discover_test_files(root)`
    Then the result is `Ok` and `files` contains exactly:
      | path |
      | a.rs |
      | b.RS |
    And `files` does NOT contain `c.txt`

  Scenario: empty extensions list includes all files
    Given a temporary directory containing `a.rs`, `b.txt`, and `c.md`
    And an `AnalysisConfig` with `extensions = []`
    When the caller invokes `discover_test_files(root)`
    Then the result is `Ok` and `files` contains exactly:
      | path |
      | a.rs |
      | b.txt |
      | c.md |

  # ─── Pre-walk fatal failures (R6) ─────────────────────────────────────
  # Three separate scenarios because the failures fire at different
  # lifecycle phases: InvalidGlob at FsWalker::try_new (config-time);
  # missing-root and root-is-file at discover_test_files pre-flight.

  Scenario: missing root surfaces as fatal SourceError::Io
    Given a `SourceRoot` pointing at a non-existent path under the test temp directory
    And an `FsWalker` constructed from a valid `AnalysisConfig`
    When the caller invokes `discover_test_files(missing_root)`
    Then the result is `Err(SourceError::Io)` with `path` equal to the missing-root `FilePath`
    And the underlying `source` is a `std::io::Error`

  Scenario: root that is a regular file (not a directory) surfaces as fatal SourceError::Io
    Given a `SourceRoot` pointing at a regular file under the test temp directory
    And an `FsWalker` constructed from a valid `AnalysisConfig`
    When the caller invokes `discover_test_files(file_root)`
    Then the result is `Err(SourceError::Io)` with `path` equal to the file-root `FilePath`
    And the underlying `source` is a `std::io::Error`

  # ─── Mid-walk non-fatal diagnostics (R5) ──────────────────────────────
  # Only PermissionDenied is exercised at the BDD layer (cross-platform-relevant
  # via @unix tag). MidwalkIo and Other are exercised by unit tests in
  # adapters/source/fs.rs against hand-constructed ignore::Error values
  # (per shaping A7b: classify_walk_error helper, branch-by-branch).

  @unix
  Scenario: permission-denied subdirectory yields a diagnostic and the walk continues
    Given a temporary directory containing `accessible/a.rs` and `denied/b.rs`
    And `denied` has been chmod'd to 0o000
    And an `FsWalker` constructed from a valid `AnalysisConfig` with `extensions = ["rs"]`
    When the caller invokes `discover_test_files(root)`
    Then the result is `Ok` and `files` contains exactly:
      | path           |
      | accessible/a.rs |
    And `diagnostics` contains exactly one `SourceDiagnostic`
    And that diagnostic has `kind = PermissionDenied`
    And that diagnostic's `path` includes the `denied` subdirectory

  # ─── MemorySource (test fixture) ──────────────────────────────────────

  Scenario: MemorySource returns the configured files regardless of root (root-ignored contract)
    Given a `MemorySource` constructed via `MemorySource::with_files` with the files:
      | path |
      | x.rs |
      | y.rs |
    When the caller invokes `discover_test_files` against any `SourceRoot`
    Then the result is `Ok` and `files` equals (in order):
      | path |
      | x.rs |
      | y.rs |
    And `diagnostics` is empty
    # Per the type-level docstring contract on MemorySource: the `root`
    # parameter is intentionally ignored; the configured files are returned
    # regardless. This scenario pins that contract for downstream test authors
    # who might pass varying roots and expect root-sensitive behavior.

  Scenario: MemorySource carries diagnostics through when constructed with the full constructor
    Given a `MemorySource` constructed via `MemorySource::new` with the files:
      | path |
      | a.rs |
    And the diagnostics:
      | kind             | path  | message              |
      | PermissionDenied | denied | could not read entry |
    When the caller invokes `discover_test_files` against any `SourceRoot`
    Then the result is `Ok` and `files` equals (in order):
      | path |
      | a.rs |
    And `diagnostics` equals (in order):
      | kind             | path  | message              |
      | PermissionDenied | denied | could not read entry |
