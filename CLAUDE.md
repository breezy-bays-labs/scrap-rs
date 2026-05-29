@AGENTS.md

# CLAUDE.md — scrap-rs

Static test smell detector. Multi-crate Rust workspace: `crates/scrap-core`
(lib — language-agnostic core shared by every adapter binary), and
`crates/scrap4rs` (lib + bin — Rust-source adapter via `syn`). A
`crates/scrap4ts` (lib + bin — TypeScript-source adapter via `swc` or
`oxc`, distributed to npm via `napi-rs`) joins the workspace at v0.6+.

## Architecture

Hexagonal (ports & adapters), strict dependency direction enforced by
Cargo crate boundaries: `scrap-core` lists no AST library in its
deps; adapter crates depend on `scrap-core` and add their own parser
library. A wrong inward import is a build error, not a review catch.
See [`adr-hexagonal-layout`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-hexagonal-layout.md)
for the full layering invariant + per-crate dep table.

## Phased Detector Roadmap

| Phase  | Adds                                                                                  | Release?  |
|--------|---------------------------------------------------------------------------------------|-----------|
| v0.1   | 5 detectors: zero-assertion, tautological, no-op-io, surface-only-io, large-example  | No — git tag only |
| v0.2   | `--format scorecard-row`; composite action; mokumo wiring                             | No — git tag only |
| v0.3   | Uncle Bob smell expansion: low-assertion-density, multiple-phases, high-mocking, helper-hidden, temp-resource, literal-heavy + saturating-complexity score | No |
| v0.4   | Three-channel duplication; F/I/V extraction-pressure; baseline diff verdicts          | No |
| v0.5   | Full Uncle-Bob actionability (5 classes, ranked recommendations)                      | No |
| **v1.0** | **scrap-core extraction + scrap4ts; first crates.io publish + GH Release**         | **YES**   |

## Detection Rules (v0.1)

| Smell                     | Penalty | Detection                                                       |
|---------------------------|---------|-----------------------------------------------------------------|
| `zero-assertion`          | 10      | `#[test]` body has no `assert*!`/`should_panic`/`.expect`/`.unwrap` (incl. `.unwrap_err`/`.expect_err` error-path siblings) and no implicit-assertion source (see `domain/assertion_sources.rs` for the runner-shell registry + `domain/behavioral_fact.rs::BehavioralFact::ResultAsserted` for the panic-chain method-ident recognition that lands at scrap-rs#30; v0.1 recognises the 4 panic-chain idents in `scrap4rs::parser::body::PANIC_CHAIN_METHOD_NAMES`; v0.3+ behavioral-fact variants land with scrap-rs#25) |
| `tautological-assertion`  | 10      | `assert!(true)`, `assert_eq!(x, x)`, literal-vs-literal compare |
| `no-op-io`                | 8       | All exprs are `let _ = ...;` with no follow-up check            |
| `surface-only-io`         | 6       | Calls `*.exists()` / `Path::is_file` post-create without read-back |
| `large-example`           | 4       | Body exceeds `[detectors.large_example.line_threshold]` (default 30, tunable per project) |

Modeled on Uncle Bob's [`unclebob/scrap`](https://github.com/unclebob/scrap)
(Clojure, Speclj). The Rust port intentionally trims the v0.1 surface to
the four #649 patterns + `large-example`; the full Speclj 8-smell
taxonomy lands in v0.3.

## Wire Envelope

Mirrors crap4rs's nested JSON envelope. See
[`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-nested-json-envelope.md)
for full forward-compat rules. Highlights:

- `schema_version: u32` — bumps only on breaking changes; additive
  fields allowed at any time.
- `result.*` is the **truthful gate** (cannot be reshaped by `--top`,
  `--only-failing`, `--no-fail`).
- `view.*` is the **shapeable display** — filtered, sorted, truncated.
- `delta.*`, `diagnostics.*` — additive optional, omitted when not in use.
- **`#[non_exhaustive]` policy**: every public *enum* in
  `scrap-core::domain` carries it (consumer pattern-match concern);
  result *structs* (`Finding`, `Report`, `Summary`, etc.) do not —
  they evolve via constructors (`Foo::new`, `Foo::try_new`,
  `Foo::default`) and serde versioning. Rationale lives in the
  envelope ADR's `#[non_exhaustive] discipline` section.
- `Option<T>` fields use `#[serde(skip_serializing_if = "Option::is_none")]`.

## Commands

| Task | Command |
|------|---------|
| Build | `cargo build -p scrap4rs` |
| Test | `cargo nextest run` (or `cargo test`) |
| Coverage | `cargo llvm-cov nextest --lcov --output-path lcov.info` |
| Lint | `cargo clippy --all-targets -- -D warnings` |
| Format | `cargo fmt` |
| Quick verify | `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo nextest run` |

## Property Test Invariants

Filled in as detectors and the score formula land:

| Function | Key invariants |
|----------|---------------|
| `score_example()` | `score >= 0.0`, monotonic in smell-penalty sum, never panics on empty bodies |
| Implicit-assertion recognition | every entry in `domain/assertion_sources.rs` has at least one fixture under `tests/fixtures/runner_shells/` that MUST NOT trigger |
| Detector idempotence | `detect(detect(ast))` produces the same finding set as `detect(ast)` |

## Commit Convention

```
feat(domain):  feat(ports):  feat(adapters):  feat(core):  feat(cli):
fix(domain):   test:         ci:              docs:        chore:
adr:           closeout:
```

## Worktree Setup

```bash
git worktree add ../scrap-rs-issue-N -b feat/topic-name
```

Shared target directory once configured under `.cargo/config.toml`
(arrives when worktrees are needed).

## v0.x → v1.0 Transition

The workspace already has the right shape: `scrap-core` (lib),
`scrap4rs` (lib + bin), with `scrap4ts` (lib + bin) joining at v0.6+
when its parser adapter is ready. v1.0 is therefore *publish*, not
*restructure*:

1. Land `release.yml` mirroring crap4rs (tri-platform tarballs,
   ordered `cargo publish` `scrap-core` → `scrap4rs` → `scrap4ts`,
   GH Release).
2. Add `[package.metadata.binstall]` to `crates/scrap4rs/Cargo.toml`
   and `crates/scrap4ts/Cargo.toml`.
3. `scrap4ts` distributes to npm via `napi-rs` — `npm install -D
   @scrap-rs/scrap4ts` pulls a per-platform native binary.
4. Mokumo migrates from action-ref consumption to
   `bins: scrap4rs@1.0.0` + composite action `@v1.0.0`. Composite
   action drops the build step (binary on PATH).

## Cross-References

- **Pipeline plan** (private ops vault):
  `ops/pipelines/scrap4rs/scrap4rs-20260504-kickstart-plan.md`
- **Adoption tracker**:
  [mokumo#649](https://github.com/breezy-bays-labs/mokumo/issues/649)
- **Quality-manifest slot**: Q1 ("Are my tests testing real behavior?")
  under `ops/standards/quality-manifest.md:48`
- **Sibling — production-code complexity**:
  [crap4rs](https://github.com/breezy-bays-labs/crap4rs)
- **Modeled on**: [unclebob/scrap](https://github.com/unclebob/scrap)

## Compact Instructions

Preserve: hexagonal layering, detector phasing (v0.1 → v1.0), wire
envelope invariants, property test contracts, false-positive guard
list, scrap-core extraction roadmap.
Discard: full file contents from old reads, search results not acted
on, completed PR details, intermediate license/visibility deliberations
already documented in pipeline plan §11.
