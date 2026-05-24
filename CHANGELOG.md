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

- CI: `zizmor` workflow-security audit job in `.github/workflows/ci.yml`
  (mirrors crap4rs precedent). Runs `pipx run 'zizmor>=1.5,<2'
  .github/` on every PR + push to main. `unpinned-uses` is the
  primary gate; `excessive-permissions`, `template-injection`,
  `artipacked`, `cache-poisoning` ride along as secondary gates at
  no marginal cost. Major-version pinned (`<2`) so a zizmor release
  tightening rules doesn't flip CI red between PRs.
- CI: `.github/dependabot.yml` covering both `github-actions` and
  `cargo` ecosystems. Weekly cadence (Mondays), grouped minor+patch
  bumps per ecosystem, major bumps as separate PRs, 7-day cooldown
  to surface bad-release reports before automation pulls them.
  Commit prefix `ci(deps):` for github-actions, `chore(deps):` for
  cargo (per the workspace's commit convention).
- CI: workflow-level `permissions: contents: read` default in
  `ci.yml` (least-privilege). All 9 jobs are read-only; no per-job
  overrides needed today. Closes the `excessive-permissions` zizmor
  audit.
- CI: `persist-credentials: false` on every `actions/checkout` call
  (9 sites — 8 pre-existing jobs + 1 in the new zizmor job). Keeps
  the GH App checkout token out of the runner's `.git/config`.
  Closes the `artipacked` zizmor audit.
- CI: every external `uses:` ref in `ci.yml` SHA-pinned with trailing
  `# vX` (tagged releases: `actions/checkout`, `Swatinem/rust-cache`,
  `taiki-e/install-action`, `actions/upload-artifact`) or
  `# tracks @<branch> branch` (branch-pinned:
  `dtolnay/rust-toolchain@{stable,1.88}` — Dependabot can't bump
  these per AGENTS.md "Supply-chain hygiene" Rule 2; manual quarterly
  refresh cadence). `EmbarkStudios/cargo-deny-action` was already
  pinned.
- Docs: `AGENTS.md` adds `## Supply-chain hygiene` section (8 rules;
  mirrors crap4rs Rules 1–8). Rules 9 (cache-poisoning fix for
  release workflows) and 10 (gh CLI for release artifacts) deferred
  to v1.0 release-workflow work per `CLAUDE.md > v0.x → v1.0
  Transition`. Follow-up: scrap-rs#51 (extract `setup-rust` composite
  action — mirror crap4rs pattern).
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
  pre-validates user globs at `try_new` (fatal `SourceError::InvalidGlob`
  for malformed globs, `SourceError::EmptyExcludePattern` for
  empty/whitespace-only globs that `globset` would silently accept and
  `OverrideBuilder` would rewrite into a global whitelist); honours
  `.gitignore` / `.ignore` / `.git/info/exclude` when
  `respect_gitignore = true` (`require_git(false)` so VCS files are
  honoured outside a git repo, matching `rg`/`fd` ergonomics);
  mid-walk failures surface as `SourceDiagnostic` so the walk
  continues; symlinks emit a `SourceDiagnosticKind::Other` diagnostic
  rather than being silently dropped. Emits paths **relative to
  `AnalysisConfig::src`** so reports and snapshots are stable across
  machines. Sorts the collected paths byte-wise on the underlying
  OsStr (E1 from shaping — matches the
  `crap4rs::core::discover_rust_files` reference).
- `adapters::source::memory::MemorySource` — test-only `SourcePort`
  implementation that returns a fixed `(files, diagnostics)` pair
  without touching disk. Fields are private; access via
  `::files()` / `::diagnostics()` accessors. Two constructors:
  `::new(files, diagnostics)` (canonical, D10) and
  `::with_files(files)` (convenience for the diagnostics-empty test
  fixture path).
- Executable behavioral contract for the file walker at
  `crates/scrap-core/tests/features/file_walker.feature`
  (15 scenarios + 1 Scenario Outline w/ 2 Examples — 17 scenario rows
  total), driven by a cucumber-rs 0.23 harness at
  `crates/scrap-core/tests/cucumber.rs` (`harness = false`).
- Compile-time invariant smoke tests at
  `crates/scrap-core/tests/source_walker.rs` — `assert_obj_safe!`
  on `SourcePort`, `assert_not_impl_any!(dyn SourcePort: Send, Sync)`
  on the trait, `assert_impl_all!(_: Send, Sync)` on both shipped
  adapters.
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

- **Breaking**: `SourcePort::discover_test_files` signature changed
  from `fn(&self, root: &SourceRoot) -> Result<Vec<FilePath>, SourceError>`
  to `fn(&self) -> Result<DiscoveryOutcome, SourceError>`. The
  redundant `root` parameter has been dropped — adapters source the
  walked root from internal state (`AnalysisConfig::src` for
  `FsWalker`, owned files for `MemorySource`). Callers now receive
  the matching files and any non-fatal mid-walk diagnostics in one
  response. Existing `assert_obj_safe!` and
  `assert_not_impl_any!(dyn SourcePort: Send, Sync)` smoke checks
  continue to hold; the trait surface remains bound-free.
- `SourceError` gained two new variants (`#[non_exhaustive]`):
  `Ignore { source: ignore::Error }` for `OverrideBuilder::build()`
  setup failures (forward-compat hatch — exceedingly rare in
  practice; **NOT** fired by the infallible `WalkBuilder::build()`),
  and `EmptyExcludePattern { pattern: String }` for empty or
  whitespace-only exclude globs (caught eagerly at `FsWalker::try_new`
  so a config typo or empty env-var interpolation doesn't silently
  delete every walk result).
- Workspace MSRV bumped from `1.85` to `1.93`. An intermediate 1.88
  bump (file-walker pipeline) authorised cucumber 0.23 by lifting
  the prior 0.21 ceiling inside the harness. The 1.93 floor aligns
  with the sibling `crap-rs` workspace, where `oxc` 0.127+ drives
  the pin for the `crap4ts` adapter — the same constraint will apply
  to `scrap4ts` when it joins this workspace at v0.6+. An idiom-
  adoption audit (1.89–1.93 language-feature sweep across
  `crates/scrap-core/src/`) is tracked separately as a follow-up.

### Removed

- `PartialOrd + Ord` derives on `domain::types::FilePath` — never
  consumed; `PathBuf`'s natural component-wise ordering clashes
  with the byte-wise sort `FsWalker` uses for its post-collect
  ordering. Future call sites that need ordering must choose
  explicitly between component-wise and byte-wise (and document the
  choice).
