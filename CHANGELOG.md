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
