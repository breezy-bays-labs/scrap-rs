# scrap-examples

Curated bad-by-design fixture corpus + golden snapshot harness for
the scrap-rs detector ecosystem. `publish = false`; never reaches
crates.io.

## What this is

One artifact serving three audiences:

1. **Regression-protective CI guard** — snapshot drift = behaviour
   change, caught on every CI run via the `scrap-examples` job in
   `.github/workflows/ci.yml`.
2. **Consumer onboarding** — `git clone scrap-rs && cargo run -p scrap4rs -- crates/scrap-examples/examples/zero_assertion/`
   exhibits the tool's behaviour and output shape against a small,
   self-contained fixture. (The `cargo run` invocation lands when
   scrap-rs#21 ships the CLI; today the harness invokes scrap-rs
   programmatically.)
3. **Docs anchor** — each detector's rustdoc points to its
   corresponding fixture directory (smell pattern + fix pattern, in
   real Rust).

Distinct from the in-repo self-check tests at
`crates/scrap4rs/tests/self_check.rs` (those prove detectors work on
production-shaped code — scrap4rs's own source). This corpus proves
detectors work on isolated curated patterns and provides a public-
facing learning surface.

## Layout convention

One subdirectory per detector under `examples/`:

```
crates/scrap-examples/
├── Cargo.toml
├── README.md            ← this file
├── src/
│   └── lib.rs           ← empty (docstring only)
├── tests/
│   └── snapshots.rs     ← golden snapshot harness
└── examples/
    └── <smell>/
        ├── bad.rs       ← triggers the smell
        ├── good.rs      ← does NOT trigger (proves not over-eager)
        ├── README.md    ← prose: smell + fix in 1-2 short sections
        └── expected.json← golden snapshot of v0.1 JSON envelope
```

Each file is required. The harness skips directories without an
`expected.json` (so a half-finished fixture-in-progress doesn't
break CI), but a fully-empty `examples/` is a hard error — the
corpus must be non-empty.

## Adding a new fixture

Four-step recipe:

1. `mkdir crates/scrap-examples/examples/<smell>/` (use the
   `snake_case` form of the `SmellCategory` variant, e.g.
   `tautological_assertion/`).
2. Write `bad.rs` — minimal `#[test]` body that triggers the smell.
   Mirror the existing `zero_assertion/bad.rs` shape: one test fn,
   small body, no unnecessary scaffolding.
3. Write `good.rs` — minimal `#[test]` body that does NOT trigger
   (the fix pattern).
4. Write `README.md` — two short sections, "Smell" and "Fix".
5. Run `BLESS=1 cargo test -p scrap-examples` to generate
   `expected.json` from live output. Review the generated file in
   `git diff` before committing.

That's it. The harness auto-discovers the new fixture directory and
exercises it on the next test run.

## Bless workflow

```bash
# Regenerate every fixture's expected.json from live output.
BLESS=1 cargo test -p scrap-examples

# Verify (no env var): mismatches fail the test.
cargo test -p scrap-examples
```

Without `BLESS=1`, `expected.json` mismatches fail the test. The
harness:

- Walks `examples/` for subdirectories containing `expected.json`.
- For each fixture, parses `bad.rs` via `SynTestParser`, runs the
  zero-assertion detector (more detectors land as their PRs ship),
  builds a `Report`, emits the v0.1 JSON envelope via
  `scrap_core::adapters::reporters::json::emit`, and compares
  against `expected.json` via `serde_json::Value` equality.
- For `good.rs`: parses + runs detectors and asserts zero findings.

The harness uses a fixed `AdapterMeta` literal (`tool: "scrap4rs"`,
`language: "rust"`, `tool_version: "0.1.0"`) and a fixed timestamp
so that `expected.json` is deterministic across machines and
versions. `tool_version` is intentionally hard-coded — see
"Wire shape" below for the rationale.

Editor's auto-insert-final-newline will NOT cause drift. The harness
appends `\n` on write and parses via `from_slice` on read; the
round-trip through editor save produces zero `git diff`.

### Per-fixture filter ergonomics

`cargo test -p scrap-examples zero_assertion` will NOT filter to
just the `zero_assertion` fixture — it filters by test NAME, and
the test names are `bad_rs_emission_matches_expected_json` and
`good_rs_does_not_trigger`. The whole harness completes in <1s, so
this is not a performance concern; the constraint is acknowledged
here for future fixture-set growth.

## Wire shape

`expected.json` mirrors the v0.1 JSON envelope (`schema_version: 1`)
produced by `scrap_core::adapters::reporters::json::emit`. The same
envelope shape is also pinned by the in-source snapshot test at
`crates/scrap-core/tests/wire_envelope_snapshot.rs` (insta-based
harness-internal contract). Using the live `emit` here keeps the two
contracts in lock-step by construction — one source of truth.

`tool_version` is hard-coded to `"0.1.0"` in the harness rather than
reading `env!("CARGO_PKG_VERSION")`. Every scrap4rs version bump
would otherwise invalidate every fixture's `expected.json`. The
version on the wire here is a docs-anchor literal that demonstrates
the shape — it is NOT the live crate version. When scrap4rs publishes
externally (v1.0), this convention may be revisited.

Wire-envelope ADR: [`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-nested-json-envelope.md).

## What this is NOT

- **Not a binary.** No `[bin]` in `Cargo.toml`; no `main.rs`.
- **Not a library shipping logic.** `src/lib.rs` is empty
  (docstring + `#![warn(missing_docs)]` only).
- **Not a cargo-examples directory in the conventional sense.**
  `autoexamples = false` in `Cargo.toml`; files under `examples/`
  are fixture source files for the harness to parse via
  `std::fs::read_to_string`, NOT cargo binary targets to compile.
- **Not a replacement for the self-check tests.** The self-check at
  `crates/scrap4rs/tests/self_check.rs` proves detectors work on
  production-shaped code (scrap4rs's own source); this corpus proves
  detectors work on curated patterns.

## Cross-references

- Parent epic: scrap-rs#79
- This sub-issue: scrap-rs#80
- ADR: `adr-nested-json-envelope` (envelope wire shape)
- ADR: `adr-hexagonal-layout` (this crate is a CONSUMER, not a
  hexagon-layer member)
- Inspiration: [unclebob/scrap](https://github.com/unclebob/scrap)
  has a similar fixtures directory pattern
