# scrap-rs Agent Notes

Cross-provider agent operating guide. Both Claude Code and Codex
should read this before touching code.

## Repo Identity

- `scrap-rs` is a Rust **workspace** in `breezy-bays-labs` org. Public
  visibility from day one; **no crates.io publish, no GitHub Release
  tarballs, no `cargo install` path until v1.0**. Tags exist for git
  pinning only — internal versions like `v0.1.0` do not trigger any
  workflow.
- The v1.0 release gate is the triple-crate workspace being live:
  `crates/scrap-core/` + `crates/scrap4rs/` + `crates/scrap4ts/`. Until
  then, mokumo is the sole consumer via composite GitHub Action.

## Architecture

Hexagonal (ports & adapters), strict dependency direction:

```
domain/ → ports/ → adapters/ → core/ → cli/
```

| Layer | Purpose | External crates? |
|-------|---------|-----------------|
| `domain/` | Smell taxonomy, score, threshold, types | None — pure logic |
| `ports/` | Trait defs (`SourcePort`, `TestParserPort`, `OutputPort`) | Domain types only |
| `adapters/` | syn AST walker, walkdir/ignore source, reporters | syn, serde, comfy-table, ignore |
| `core/` | Wires adapters via ports, exposes `analyze()` | Ports + adapters |
| `cli/` | clap argument parsing, ExitCode shaping | clap, core |

**Never import inward.** `domain/` and `ports/` must stay
language-agnostic — no `syn`, no `walkdir`, no `serde-on-AST`. Designed
for extraction into `scrap-core` at v1.0; same `crates/` directory, no
rename.

## Working Rules

- **TDD** — tests before implementation for all domain and adapter code.
- **Domain purity** — `crates/scrap4rs/src/domain/` must never import
  external crates or perform I/O.
- **Self-referential test** — once detectors land, scrap4rs must
  analyze its own source as an integration test (the `self-check`
  CI job).
- **Symmetric dogfood** — scrap4rs's CI also runs `crap4rs` against its
  own production code (`crap-self` job, gated on the production-code
  CC ladder).
- **No release workflow during v0.x** — `release.yml` arrives at v1.0
  prep. Tags are git-pinning markers only.
- **No `tools.toml` Warden pin** in mokumo during v0.x — mokumo
  consumes scrap-rs via composite action ref (`@v0.x.0`); the action
  self-builds scrap4rs from the ref.
- **No direct push to main** — branch + PR for all work after the
  initial bootstrap commit.
- **Worktrees** for parallel work: `git worktree add ../scrap-rs-issue-N -b feat/topic-name`.
- **Property tests required** for the smell scoring formula and the
  parser's assertion-recognition list (see `domain/assertion_sources.rs`).
- **Regression files committed** — any `proptest-regressions/` dirs
  go into git, never gitignored. Commit the regression file + fix in
  the same PR.

## Test Framework Recognition

scrap4rs's `zero-assertion` detector treats the following as
**implicit-assertion sources** and suppresses the smell:

- Macros: `proptest!`, `quickcheck!`, `kani::*`, any `*_proptest!`
- Method-call chains ending in `.await` on `World::cucumber()` or
  `cucumber::run` / `cucumber::Cucumber::run`
- Function calls to `quickcheck::quickcheck`, `trybuild::TestCases::*`
- Macros: `insta::assert_*!`, `pretty_assertions::*`
- `#[should_panic]` attribute on the enclosing fn

The list lives in `domain/assertion_sources.rs` (data-driven). New
idioms join via PR with a fixture under
`crates/scrap4rs/tests/fixtures/runner_shells/` that MUST NOT trigger
any detector.

## Idiomatic Rust Test Mapping

For v0.3+ block-level reports (Speclj `describe`/`context` analog):

- `block` = `mod tests { ... }` under `#[cfg(test)]`, with arbitrary
  nesting (`mod auth { mod login { ... } }`).
- `describe-path[]` = the fully-qualified module path
  (`crate::auth::tests::login`).
- `rstest::rstest` parameterization is **orthogonal** — table-drive
  within one test, captured under `table-driven?` flag.
- Free-function `#[test]` siblings without an enclosing `mod` are a
  single anonymous block per file.
- Doc-tests are out of scope at v0.1 (separate parser pipeline,
  v0.3+ follow-up).

## Cross-References

- **Pipeline plan**: `ops/pipelines/scrap4rs/scrap4rs-20260504-kickstart-plan.md`
  (in the private ops vault — full architecture, detector phasing, CI
  templates, sub-issue tree)
- **Adoption tracker**: [mokumo#649](https://github.com/breezy-bays-labs/mokumo/issues/649)
- **Parent epic (mokumo)**: [mokumo#370](https://github.com/breezy-bays-labs/mokumo/issues/370)
- **Quality manifest Q1 slot**: `ops/standards/quality-manifest.md:48`
- **Sibling — production-code complexity**: [crap4rs](https://github.com/breezy-bays-labs/crap4rs)
- **Modeled on**: [unclebob/scrap](https://github.com/unclebob/scrap) (Clojure, Speclj)
