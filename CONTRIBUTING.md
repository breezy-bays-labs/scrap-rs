# Contributing to scrap-rs

scrap-rs is a private-org Rust workspace developed by Breezy Bays
Labs. The repo is public for free GitHub Actions / agent reviews;
external contributions are welcome at v1.0+ once the project ships
its first crates.io release.

## Quick Start

```bash
git clone git@github.com:breezy-bays-labs/scrap-rs.git
cd scrap-rs
lefthook install            # wires pre-commit + pre-push hooks
cargo build -p scrap4rs
cargo nextest run
```

`lefthook install` is one-time. After that, `cargo fmt --check` runs on
every commit; the full pre-push battery (fmt + pedantic clippy + tests +
cargo-deny + docs-as-errors) runs on every push and matches CI exactly.
See [`lefthook.yml`](lefthook.yml) for what each hook runs.

## Development Loop

| Step | Command |
|------|---------|
| Format | `cargo fmt --all` |
| Lint (with pedantic) | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| Test | `cargo nextest run --workspace --all-targets --locked` |
| Coverage | `cargo llvm-cov nextest --workspace --locked --fail-under-lines 85` |
| Supply chain | `cargo deny check` |
| Doc lint | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items --locked` |
| Quick verify | `lefthook run pre-push` |

`#![warn(clippy::pedantic, clippy::cargo)]` lives at the crate root
(`crates/scrap4rs/src/lib.rs`), so `clippy -D warnings` enforces
pedantic and cargo lints automatically — no extra flag needed.

CI runs the same chain on every PR. See `.github/workflows/ci.yml` for
the full job set (matrix test on Linux / macOS arm64 / macOS x86_64,
plus MSRV / cargo-deny / cargo doc / coverage gates).

## Branch + PR

- Always branch off `main`; never push directly. The repo enforces
  this for ongoing work.
- Use worktrees for parallel work:
  `git worktree add ../scrap-rs-issue-N -b feat/topic-name`.
- Title: `<conventional-prefix>(<scope>): <one-liner>` (e.g.
  `feat(adapters): zero-assertion detector`).
- Body: include `Closes #N` to link to the sub-issue.
- 1 PR closes exactly 1 sub-issue (per
  `ops/standards/issue-hierarchy.md`).

## Architecture Discipline

Read [`CLAUDE.md`](CLAUDE.md) and [`AGENTS.md`](AGENTS.md) before
touching code. The hexagonal layering rule is **strict**:

- `domain/` and `ports/` are language-agnostic. No `syn`, no `serde`-on-
  AST, no I/O. They will extract into `scrap-core` at v1.0.
- `adapters/` is where Rust-specific code lives — the syn walker,
  walkdir/ignore source, serde reporters.
- Never import inward.

## Detector Authoring Checklist

When adding a new smell:

1. Add the variant to `domain::SmellCategory` (`#[non_exhaustive]`
   enum) and the penalty/severity entries to `domain::policy`.
2. Add a property invariant covering the score-formula effect.
3. Add positive fixtures under
   `crates/scrap4rs/tests/fixtures/true_positives/<smell>/` — these
   MUST trigger the smell.
4. Add **runner-shell** fixtures under
   `tests/fixtures/runner_shells/` if the new smell could fire on
   cucumber-rs / proptest / quickcheck / trybuild idioms — these MUST
   NOT trigger.
5. Add an integration test
   `tests/integration_<smell>.rs` exercising the public CLI surface.
6. Update `domain/assertion_sources.rs` if the smell needs to suppress
   on new implicit-assertion sources.

## Exclusions and Tracking-Issue Rule

Every entry in `scrap4rs.toml`'s `exclude = [...]` array, every
`#[ignore]`, every `#[cfg(skip_in_ci)]` MUST carry an inline
`# tracked: scrap4rs#<n> — <reason>` comment OR `# adr: <path>` if
permanent. Quarterly grep audit. See
`~/.claude/rules/exclusions.md` for the full rule.

## Issue Discipline

- Every issue gets exactly one `type:*` label
  (`type:feature`/`type:bug`/`type:task`/etc.) and one `priority:*`
  label.
- Sub-issues use `--parent <epic-number>` (native GH sub-issues; not
  manual checkboxes).
- Body skeleton: `## Summary` / `## Acceptance Criteria` /
  `## Context` / `## Discovery`.
- Wire `blocked-by` edges at creation time, not later.

## Release Discipline (v0.x)

- **No `cargo publish`** until v1.0.
- **No GH Release** until v1.0.
- Tags during v0.x exist solely for git-pinning consumers (mokumo's
  composite-action ref). They do not trigger any workflow.
- See `CHANGELOG.md` for the deliberate-no-release policy and the
  v1.0 gate definition.

## License

By submitting a PR you agree your contributions are dual-licensed
under MIT OR Apache-2.0.
