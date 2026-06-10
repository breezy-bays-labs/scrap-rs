# scrap-rs

Static test smell detector — Rust workspace.

```text
crates/scrap4rs   syn-based analyzer for Rust test bodies
crates/scrap-core (v1.0+) language-agnostic domain + ports + core
crates/scrap4ts   (v1.0+) TypeScript analyzer (oxc-based)
```

scrap-rs flags **structural junk in test code** — zero-assertion bodies,
tautological assertions (`assert!(true)`, `assert_eq!(x, x)`), no-op
I/O test bodies, surface-only I/O checks, and oversized test bodies.
It runs in milliseconds, complements (does not replace) `cargo-mutants`,
and is designed to give agentic-CI loops sub-second feedback on test
quality.

> **Status: v0.x — pre-release.** No crates.io publish, no GitHub
> Release tarballs, no `cargo install` path until **v1.0**. The v1.0
> gate is the triple-crate workspace (`scrap-core` + `scrap4rs` +
> `scrap4ts`) being live and proven through one full mokumo release
> cycle. See [the kickstart plan][plan] in the breezy-bays-labs ops
> repo for the full roadmap.
>
> During v0.x the only consumer is [mokumo][mokumo] via a composite
> GitHub Action — `scrap4rs` rebuilds inside the action on every CI run.

## Architecture

scrap4rs is a hexagonal (ports & adapters) crate. Strict dependency
direction: `domain → ports → adapters → core → cli`. The `domain/` and
`ports/` layers are language-agnostic and will extract into `scrap-core`
at v1.0 with no rename — `scrap4ts` (TypeScript) plugs into the same
core. See [`CLAUDE.md`](CLAUDE.md) for the full layering invariants.

## Sibling tools

scrap-rs sits in the **CRAP/SCRAP ecosystem**:

| Tool        | Repo                                       | What it gates              |
|-------------|--------------------------------------------|----------------------------|
| `crap4rs`   | <https://github.com/breezy-bays-labs/crap4rs> | production-code complexity (Rust) |
| `crap4ts`   | <https://github.com/breezy-bays-labs/crap4ts> | production-code complexity (TS)   |
| `scrap4rs`  | this repo                                  | test-code structural smells (Rust) |
| `scrap4ts`  | this repo (v1.0+)                          | test-code structural smells (TS)   |

`crap` answers "how risky is this production function?" — `scrap`
answers "is this test testing real behavior?"

## Detection rules (v0.1)

| Smell                     | Penalty | Detection                                                       |
|---------------------------|---------|-----------------------------------------------------------------|
| `zero-assertion`          | 10      | `#[test]` body has no `assert*!`/`should_panic`/`.expect`/`.unwrap` and no implicit-assertion source |
| `tautological-assertion`  | 10      | `assert!(true)`, `assert_eq!(x, x)`, literal-vs-literal compare |
| `no-op-io`                | 8       | All exprs are `let _ = ...;` with no follow-up check            |
| `surface-only-io`         | 6       | Calls `*.exists()` / `Path::is_file` post-create without read-back |
| `large-example`           | 4       | Body exceeds `[detectors.large_example.line_threshold]` (default 30) |

The `zero-assertion` detector recognizes the following idioms as
**implicit-assertion sources** (no false positive): `proptest!`,
`quickcheck!`, `quickcheck::quickcheck`, `kani::*`, `cucumber::run` /
`World::cucumber()`, `trybuild::TestCases::*`, `insta::assert_*!`,
`pretty_assertions::*`, and any function carrying `#[should_panic]`.

## Usage (v0.x — internal only)

Quick start:

```bash
# Bootstrap a starter config in the current directory:
cargo run -p scrap4rs -- init

# Run the analyzer against a source root + emit the JSON envelope:
cargo run -p scrap4rs -- --src crates/scrap-core --format json

# Plain stdout summary, top 20 findings, only-failing:
cargo run -p scrap4rs -- --src crates/scrap-core --format stdout --top 20 --only-failing

# Show full help (about + EXAMPLES):
cargo run -p scrap4rs -- --help
```

`scrap4rs init` writes a `scrap.toml` skeleton in the current
directory with commented-out detector blocks + exclude templates;
`crates/scrap4rs/scrap4rs.example.toml` is the canonical reference
copy committed to the repo.

### Configuration

The workspace-root [`scrap.toml`](scrap.toml) is the canonical live
example: it is the real, minimal config this repository dogfoods —
auto-discovered by the CLI (`discover_config` walks up from `--src`)
and consumed in CI by the `config-dogfood` job, which proves the
discovery + load + merge path end-to-end on every PR. Precedence:
CLI flags > `--config FILE` > auto-discovered `scrap.toml` >
adapter defaults.

The CLI surface accepts:
`--src`, `--config`, `-f/--format json|stdout|markdown|sarif`,
`--threshold-mode strict|default|lenient`, `--no-fail`, `--exclude`
(repeatable), `--no-gitignore`, `--top N`, `--only-failing`,
`--color auto|always|never`, `-q/--quiet`, `-v/--verbose`.
Subcommands: `init [--force] [--non-interactive]`,
`completions <SHELL>` (bash/zsh/fish/elvish/powershell/nushell).
Exit codes: 0 = passed, 1 = over threshold, 2 = config error,
3 = all files failed to parse.

Any downstream CI consumes scrap-rs via the composite action
published from this repo (mokumo is the first adopter; the surface is
generic):

```yaml
- uses: actions/checkout@v4
- uses: breezy-bays-labs/scrap-rs/.github/actions/scorecard@v0.2.0
  with:
    src: crates
    config: scrap.toml
```

The action builds `scrap4rs` from the pinned ref on every run. v1.0
adds `cargo binstall scrap4rs` so consumers can install the binary
once and skip the rebuild.

## Documentation

- [`CLAUDE.md`](CLAUDE.md) — architecture invariants and layering rules
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — how to contribute
- [`AGENTS.md`](AGENTS.md) — agent operating notes
- [`CHANGELOG.md`](CHANGELOG.md) — release notes (sparse during v0.x)

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

[plan]: https://github.com/breezy-bays-labs/ops (private — `ops/pipelines/scrap4rs/scrap4rs-20260504-kickstart-plan.md`)
[mokumo]: https://github.com/breezy-bays-labs/mokumo
