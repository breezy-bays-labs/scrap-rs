# surface-only-io

## Smell

A `#[test]` body writes (or creates) a file and then inspects only its
*surface* — that it exists, is a file/dir, or has some length — without
ever reading the content back. The test always passes once the file is
on disk, but it conveys nothing about *what was written*: a regression
that wrote the wrong bytes, or empty bytes, would leave this test green.

This is the first **correlation** detector in scrap-rs. Unlike the
presence detectors (`zero-assertion`, `no-op-io`, `large-example`), it
cannot decide from a single fact — a write is fine, an existence check
is fine, a read is fine. The smell is the *relationship*:
write-then-surface-check-without-read **on the same path**.

The adapter (scrap4rs) emits three **located** filesystem facts — each
carrying a `path_key` identity the adapter computes plus the span of the
path argument:

- `FilesystemWrite` — `fs::write`, `File::create`, `fs::create_dir*`,
  `NamedTempFile::new()`, `OpenOptions…write(true)…open`.
- `FilesystemSurfaceCheck` — `Path::exists` / `is_file` / `is_dir`,
  `fs::metadata` (including length-only `metadata().len()`).
- `FilesystemRead` — `fs::read`, `fs::read_to_string`, `File::open`,
  `BufReader::new(File::open(...))`.

The detector groups these by `path_key` and emits a finding when, for
some key:

```
has_write(key) ∧ has_surface_check(key) ∧ ¬has_read(key)
```

When that holds, the detector emits one `SmellCategory::SurfaceOnlyIo`
finding at `Severity::Moderate` with `Actionability::AutoRefactor` and
penalty 6.

### Path-key resolution

The adapter resolves a path argument to a stable key with a light,
single-pass intra-body binding map: string/path literals →
`lit:<value>`; a `let p = ...;`-bound ident → its recorded key (else
`bind:<ident>`); a `NamedTempFile::new()` binding → `tempfile-handle:<N>`
(and `f.path()` aliases back to it). An unresolvable path (`format!(..)`,
a field path, an interprocedural value) gets a **distinct** `opaque:<N>`
per site, so two unresolved paths can never spuriously correlate. Richer
dataflow (reassignment, field paths, `format!` reduction) is a v0.3+
follow-up.

### Suppression reconciliation

`surface-only-io` deliberately does **not** consult the shared
`has_positive_check` predicate, and the parser does **not** fold
filesystem facts into it. `assert!(path.exists())` BOTH records a real
assertion (which correctly suppresses `zero-assertion` — the test *does*
assert) AND projects a `FilesystemSurfaceCheck` (which correctly fires
`surface-only-io` — it only checked existence). Both verdicts are right
simultaneously: a test can assert and still only look at the surface.

### Orthogonal to the assertion-based smells (stacks)

Like `large-example`, this detector reads only its own fact families and
never suppresses / is suppressed by the assertion-based smells. A
write-then-surface-check body that ALSO fails to assert anything would
co-fire `surface-only-io` (6) and `zero-assertion` (10), stacking into
one `Finding` (Option A; precedence policy deferred to the scrap-rs#32
score aggregator). The `bad.rs` fixture here DOES assert (on the
surface), so it trips ONLY `surface-only-io` — a clean single-smell
example.

## Fix

Read the content back and assert on the substantive payload, not just
the file's existence or metadata. The `good.rs` example replaces
`assert!(path.exists())` with `fs::read_to_string(path)` +
`assert_eq!(got, "important payload")` — the read projects a
`FilesystemRead` on the SAME path key, which disarms the correlation. No
finding emits.

Equivalent fixes:

- `assert_eq!(fs::read_to_string(path)?, expected)` — the canonical
  read-and-assert idiom.
- `assert_eq!(fs::read(path)?, expected_bytes)` for binary payloads.

## Wire shape

See `expected.json` for the canonical envelope emitted against `bad.rs`.
The relevant fields:

- `result.files[0].findings[0].smells` contains exactly ONE
  `surface_only_io` smell (`penalty == 6`, `severity == "moderate"`,
  `actionability == "auto_refactor"`).
- `scrap_score == 6.0`.
- `span` (under `test` and the smell) covers the whole test function
  from signature to closing brace.

The full v0.1 envelope shape is documented at
[`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-nested-json-envelope.md).
The detector contract is recorded at
[`adr-port-surface-and-domain-conventions`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-port-surface-and-domain-conventions.md)
(D1 adapter-says-what/core-says-is-bad · D2 atomic facts · D3 located
events · D4 core composes the correlation · D5 `Vec` storage).
