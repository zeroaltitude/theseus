# It fixes itself, and tells you after

*One page on self-improvement (RSI). Numbers come from the data files and tests it cites. Updated 2026-10-28.*

## What it is

Each night Theseus reads its own record for what went wrong and for work it keeps repeating. It fixes the faults and
turns a repeated unit of work into a tool of its own. A new MCP tool loads that night, and a new built-in tool ships
in the next nightly build. In the morning it tells you what changed, why, and what it cost. A change that makes
things worse reverts itself. The owner's rule is act first and tell after, inside a short hard keel that still needs
a yes.

## How it works

- **Loop 1, the nightly dream** (`crates/theseus-core/src/rsi/`): fix mining reads the ledger for faults (`mine/`);
  pattern mining finds repeated work worth a tool; each candidate becomes a row in Theseus's own backlog
  (`backlog.rs`) with a budget ($10 a branch, $30 a day; `budget.rs`).
- **Build, review, join, announce** (`rsi/pipeline.rs`, `join/`): the build runs in the sandbox, where it cannot see
  the held-out exams that score it (`sealed.rs`); a green branch is reviewed by a different model than the one that
  wrote it; the join is signed and announced once.
- **Revert** (`rsi/revert/`): a regression found after a self-made join is bisected and reverted on its own, and
  the morning's notice says so.
- **The keel** (`scripts/keel-guard.py`, run by `scripts/gate.sh`): the gate fails any commit range that deletes or
  loosens a test, a budget or a ceiling unless a signed commit in the range carries a `Keel:` trailer for exactly
  that erosion, and the signer list is read from the base, never the branch, so a branch cannot ack itself. The
  self-improvement loop never holds the key. The spec is
  [§3.21 Self-extension: planks, never the keel](../spec/04-part1-s3.10.md#321-self-extension-planks-never-the-keel).
- **What always needs your yes:** loosening a test, a budget or a ceiling; security, policy, secret and permission
  changes; spend limits; one-way store format bumps; and the off switch, which only the owner turns back on. The
  rest fails open: no answer means go ahead, and you are told.

## What you see on each surface

- **Discord:** the morning's notice of what the night landed, and a weekly digest in your DM: counts by kind, each
  join with its numbers and its undo, what it cost (`crates/theseus-core/src/rsi/digest.rs`).
- **Terminal:** `theseus self digest`; `theseus extend list` shows proposed and loaded extensions with their frozen
  digests, and `extend revoke` removes one.

## The proof

- **On since 2026-10-18** on the owner's daemon, the day v1 was cut, so the v1 benchmarks measured Theseus before
  it started changing itself.
- **Tests walk each step:** a green self-made branch is reviewed by a different model, joined, signed, and
  announced once (`crates/theseus-core/src/rsi/join/tests.rs`); a regression planted after a self-made join is
  found, bisected and reverted (`crates/theseus-core/src/rsi/revert/tests.rs`); the build runs in the sandbox and
  cannot see the held-out exams (`crates/theseusd/tests/self_build.rs`); the keel guard's own tests
  (`scripts/test_keel_guard.py`).
- **The keel guard works on people too:** on 2026-10-28 it failed a reviewed lane in one second, before compiling
  anything, because a commit turned two exact assertions into weaker ones with no trailer. Two human-driven reviews
  had read the change and missed the paperwork.
- **The learning loop beneath it** (judgment, not code) rewrites a pack's wording nightly under a frozen holdout
  ([Jev in the core](jev-in-the-core.md)).

## The limits today

- Loop 1 mines fixes and patterns; choosing what to improve next across the whole backlog, and writing its own
  yardsticks, are still done for Theseus by an outer loop of agents, and the plan moves them inward.
- A self-made change is measured by tests and the gate, not yet by a benchmark run before and after.
- The filed defects are sometimes the owner's intent: one nightly row called a long turn a fault, and the rule is
  now that turn length alone is never a defect ([It trusts its environment](trusted-environment.md)).

## What is coming

- **1.0.4:** the self-gates inventory ([docs/design/self-gates-inventory.md](../design/self-gates-inventory.md))
  applied, so each stop rule says whether it would cut off long good work.
- **After 1.0.4:** the next rungs of [the RSI plan](../design/rsi-plan.md): its own yardsticks, and its own choice
  of what to improve.

Related: [Jev in the core](jev-in-the-core.md), [It trusts its environment](trusted-environment.md).
