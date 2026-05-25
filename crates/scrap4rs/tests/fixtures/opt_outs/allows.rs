// Fixture: every v0.1 OptOut variant for scrap-rs#12 S2.1.
//
// The parser must scan `#[allow(scrap::*)]` attributes on each test
// fn and populate `ParsedTest::opt_outs: BTreeSet<OptOut>`. The
// three v0.1 keys map to the three v0.1 detectors:
//   - `scrap::no_asserts`  → OptOut::NoAsserts → suppresses zero-assertion (#30)
//   - `scrap::tautology`   → OptOut::Tautology → suppresses tautological-assertion (#24)
//   - `scrap::no_op`       → OptOut::NoOp      → suppresses no-op-io (#25)
//
// This fixture is parsed by the cucumber harness and the insta
// snapshot at `tests/parser_snapshots.rs::snapshot_opt_outs`.

#[test]
#[allow(scrap::no_asserts)]
fn allows_no_asserts() {}

#[test]
#[allow(scrap::tautology)]
fn allows_tautology() {}

#[test]
#[allow(scrap::no_op)]
fn allows_no_op() {}

// Multi-key allow — both opt-outs must land in the BTreeSet, in
// the canonical declaration order (NoAsserts < Tautology < NoOp).
#[test]
#[allow(scrap::tautology, scrap::no_op)]
fn allows_two_keys() {}

// Sanity check: an unrelated `#[allow(...)]` (not under the `scrap::*`
// namespace) must NOT register as an OptOut. The parser only matches
// the three v0.1 keys.
#[test]
#[allow(dead_code)]
fn no_opt_outs() {}
