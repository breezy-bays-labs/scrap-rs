# Changelog

All notable changes to this project will be documented in this file. The
format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

scrap-rs follows a deliberate **no-public-release** policy through v0.x —
mokumo is the sole consumer; tags exist for git pinning only. The first
crates.io publish + GitHub Release lands at **v1.0**, gated on the
triple-crate workspace (`scrap-core` + `scrap4rs` + `scrap4ts`) being
live. See `ops/pipelines/scrap4rs/scrap4rs-20260504-kickstart-plan.md`
§3 for the full release roadmap.

## [Unreleased]

### Added

- `domain::source` module — POD types for source discovery:
  `DiscoveryOutcome` (files + non-fatal mid-walk diagnostics),
  `SourceDiagnostic` (path + kind + message), `SourceDiagnosticKind`
  (`#[non_exhaustive]` enum: `permission_denied` / `midwalk_io` /
  `other`).
- `domain::config::AnalysisConfig` — caller-supplied workspace
  configuration: `src: SourceRoot`, `exclude: Vec<String>`,
  `extensions: Vec<String>`, `respect_gitignore: bool`. Infallible
  `::new`; glob validation lives in the adapter (per shaping Shape A
  — adapter-owns-validation).
- `adapters::source::fs::FsWalker` — disk-backed `SourcePort`
  implementation built on `ignore::WalkBuilder` + `globset`. Eagerly
  pre-validates user globs at `try_new` (fatal
  `SourceError::InvalidGlob`); honours `.gitignore` / `.ignore` /
  `.git/info/exclude` when `respect_gitignore = true`
  (`require_git(false)` so VCS files are honoured outside a git repo,
  matching `rg`/`fd` ergonomics); mid-walk failures surface as
  `SourceDiagnostic` so the walk continues. Sorts the collected
  paths byte-wise on the underlying OsStr (E1 from shaping —
  matches the `crap4rs::core::discover_rust_files` reference).
- `adapters::source::memory::MemorySource` — test-only `SourcePort`
  implementation that returns a fixed `(files, diagnostics)` pair
  without touching disk. Two constructors: `::new(files, diagnostics)`
  (canonical, D10) and `::with_files(files)` (convenience for the
  diagnostics-empty test fixture path). **Ignores the `root`
  parameter** — see the type-level docstring.
- Executable behavioral contract for the file walker at
  `crates/scrap-core/tests/features/file_walker.feature`
  (13 scenarios + 1 Scenario Outline w/ 2 Examples), driven by a
  cucumber-rs 0.23 harness at `crates/scrap-core/tests/cucumber.rs`
  (`harness = false`).
- Compile-time invariant smoke tests at
  `crates/scrap-core/tests/source_walker.rs` — `assert_obj_safe!`
  on `SourcePort`, `assert_not_impl_any!(dyn SourcePort: Send, Sync)`
  on the trait, `assert_impl_all!(_: Send, Sync)` on both shipped
  adapters.
- `PartialOrd + Ord` derives on `domain::types::FilePath` so the
  walker can sort discovered files via `Vec::sort_by`.
- Hexagonal port traits in `scrap-core`: `SourcePort` (test-file
  discovery) and `TestParserPort` (source → `ParsedTestFile`), each with
  a `thiserror`-derived `#[non_exhaustive]` error enum (`SourceError`,
  `ParseError`). Object-safe (`Box<dyn ...>` works); `Send + Sync` is
  deliberately absent so parallelism bounds add at the `core::analyze`
  call site.
- `domain::parsed` module — language-agnostic structural facts every
  detector consumes: `ParsedTestFile`, `ParsedTest`, `ParsedAttribute`,
  `ParsedAssertion`, `ParseDiagnostic`, `ParseDiagnosticKind`. POD types
  for FFI portability; canonical `::new()` constructors are the
  documented entry point so detector follow-up PRs can add typed
  semantic-fact fields additively.
- `domain::SourceRoot` newtype — type-level boundary marker for the
  CLI/test entry into source discovery.
- `Display` impls on `FilePath`, `SourceRoot`, `QualifiedName` so
  operator-facing error and log strings render the wrapped path/name
  cleanly.
- Initial workspace bootstrap: `crates/scrap4rs` skeleton with hexagonal
  module layout (`domain/`, `ports/`, `adapters/`, `core/`, `cli/`).
- CI workflow with format / clippy / test matrix (Linux + macOS arm64 +
  macOS x86_64) / coverage jobs.
- Repo chrome: README, AGENTS.md, CLAUDE.md, CONTRIBUTING.md, dual
  MIT/Apache-2.0 license, default scrap4rs.toml stub.

### Changed

- `SourcePort::discover_test_files` return type changed from
  `Result<Vec<FilePath>, SourceError>` to
  `Result<DiscoveryOutcome, SourceError>`. Callers now receive the
  matching files and any non-fatal mid-walk diagnostics in one
  response. Existing `assert_obj_safe!` and `assert_not_impl_any!`
  smoke checks continue to hold; the trait surface remains
  bound-free.
- `SourceError` gained an `Ignore { source: ignore::Error }` variant
  for `OverrideBuilder::build()` setup failures (forward-compat hatch
  — exceedingly rare in practice; **NOT** fired by the infallible
  `WalkBuilder::build()`).
- Workspace MSRV bumped from `1.85` to `1.88`. Authorises Rust 1.88
  language features and the latest cucumber crate (the prior 0.21
  pin was lifted) inside the file-walker harness.
