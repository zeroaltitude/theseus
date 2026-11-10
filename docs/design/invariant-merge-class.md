# Per-file numeric invariants as a merge hazard (theseus-4vsrz, 1.0.5 B7)

*An inventory of every per-file and whole-tree numeric invariant this repository carries, a test that says which
of them a merge can break, and the answer per invariant. Measured on base `12889bcf` and re-measured after
merging `origin/main` at `5e71c15`; the measuring command is `scripts/invariant-pressure.py`, added with this
work.*

## What happened, with the numbers

Two join reds on 2026-11-08 were the same shape: an invariant computed against a branch's own base and false of
the merged tree.

- **A file's line ceiling.** `turn.rs` had a 3,525-line ceiling in `scripts/long-files.txt` with 25 lines of
  headroom. `p5/footer-session-labels` added 7 lines and `p5/harness-nudge-not-posted` added 23. Each fit alone;
  together they were 30 over 25, and the 23:00 departure died in its `shape` phase in 13 seconds. `68f8fe2b`
  raised the ceiling to 3,560 and changed no code, and its own message says the diagnosis: "25 lines of headroom
  was recolliding every cut".
- **A completeness census.** `outbox::tests::COMPUTED_SITES` went stale twice in four hours. `95023099`:
  `rule_notice.rs` reached main after the branch's base with one `to_operator` call whose kind the span-finder
  cannot read, so the merged tree walked 26 files where the list held 25. `9d9aa6b8`: `sweep_notice.rs`, from a
  **sibling in the same cut**, made it 27 against 26.

Two corrections to how that second one is usually described. **Both events were the `add` direction** -- the walk
found a file the list did not hold -- so `stale_fails` was not what fired either time; it is the guard that must
not be loosened, not the mechanism that broke. And **the count inside that census was already fixed**:
`b48905212` (2026-11-09) took the per-file *counts* out of `COMPUTED_SITES` and left only the file names, deriving
each count at test time, because "a per-file count that two branches could bump to the same wrong total and merge
clean". That is Direction B of the checklist line, already applied to the invariant the line names, one day after
the reds. What is left in that census is irreducible, and the last section says why.

## The test: is it a property of the branch or of the batch?

A numeric invariant is a property of the **batch** exactly when three things hold at once.

1. **Separation.** The written-down number and the quantity it bounds live apart -- different files, or regions a
   merge will not collide in -- so git merges a branch's growth and another branch's growth with no conflict and
   no one to notice.
2. **Additivity.** The quantity is a *sum* over the tree (lines in a file, lines in a function, tokens in a
   request, milliseconds of a start, frames of a turn), so two branches' contributions add.
3. **Direction.** Branches move the quantity *toward* the bound.

Where all three hold, every branch in the cut passes its own gate and the merge fails. Where any one fails, a
merge cannot break the invariant silently:

- **Separation fails** for `assert_eq!(SWEEPS.len(), 7, "D7, D8, D9, D10, D16, D17 and D18")`
  (`sweep_notice/tests.rs:32`): the number sits beside the table it counts, so a second branch adding a sweep
  conflicts textually in both places. Likewise `THE_AWS_CORPUS.len(), 24` and `THE_CREDENTIALS.len(), 6`
  (`aws/value_shape.rs:147,171`).
- **Additivity fails** for `complete::MustBeComplete`: it compares a *set* against a *walk of the same tree*, so
  an item a branch adds appears on both sides. That is why the shape is merge-stable, and it is the shape the
  checklist line recommends.
- **Direction fails** for a `least` floor with slack: branches that grow the walk move it *away* from the floor.
  `outbox`'s `least: 20` against 27 walked files is 7 of slack; `seccomp`'s `least: 45` against 49; `mcp_server`'s
  `least: 200` against `method::ALL`.

**A pinned number over a test's own fixture is not in this class at all.** 303 `.len() == <literal>` sites exist
under `crates/` (grepped `\.len\(\) == [0-9_]+` over `crates/ --include=*.rs`), and nearly all of them count a
collection the test itself built: a sibling branch cannot move it, so condition 1 fails trivially. The class is
only the invariants whose quantity is derived from a part of the tree another branch may edit.

## The inventory

### 1. `scripts/long-files.txt` -- per-file line ceilings

`bash scripts/shape.sh` says `shape: ok (no file over 2500 lines but the 5 listed)`. Five entries after the 10:00
departure of 2026-11-10 deleted `theseus-discord/src/render.rs`'s (a test module moved to `render/tests.rs`);
there were six at `12889bcf` and eight before the 09:00 departure. Its rule runs both ways: a listed file past its
ceiling fails, **and** an entry left standing for a file that has fallen to 2,500 lines or fewer fails, as does an
entry for a file that is gone.

| file | ceiling | lines | headroom |
|---|---|---|---|
| `crates/theseus-core/src/tests_m3.rs` | 8050 | 8010 | 40 |
| `crates/theseus-discord/src/runtime.rs` | 3500 | 3496 | **4** |
| `crates/theseus-core/src/turn.rs` | 3560 | 3533 | 27 |
| `crates/theseus-kernel/src/kernel.rs` | 3030 | 3021 | **9** |
| `crates/theseus-core/src/thread_route.rs` | 2600 | 2574 | 26 |

Unlisted and tight against `shape.sh`'s default of 2,500, where **no entry is legal at all** (the script refuses
one for a file at or under 2,500):

| file | lines | headroom |
|---|---|---|
| `crates/theseus-protocol/src/lib.rs` | 2497 | **3** |
| `crates/theseus/tests/golden.rs` | 2465 | 35 |
| `crates/theseus-store/src/wal.rs` | 2443 | 57 |
| `crates/theseus-core/src/catalog.rs` | 2436 | 64 |
| `crates/theseus-kernel/src/tests.rs` | 2431 | 69 |
| `crates/theseus-sim/src/kernel_sim.rs` | 2428 | 72 |

How hard these are pushed: of the 613 non-merge commits on `main` since 2026-11-01, 59 touch one of the five
ceilinged files (`turn.rs` 18, `thread_route.rs` 13, `runtime.rs` 11, `tests_m3.rs` 9, `kernel.rs` 8), and
`scripts/long-files.txt` itself was edited 12 times in those 9 days -- a ceiling raised or an entry deleted about
every 19 hours. **The number is being fought with, not merely held.**

**Verdict: BATCH, and the worst case in the repository.** All three conditions hold, and the quantity is a *file*,
which several branches in one cut routinely append to.

### 2. clippy's function-line ceiling

`clippy.toml` sets `too-many-lines-threshold = 100` (and `cognitive-complexity-threshold = 25`); `Cargo.toml`'s
workspace lints make `too_many_lines` a warning and the gate runs clippy with `-D warnings`, so 100 is green and
101 is red. `python3 scripts/line-pressure.py --margin 0 crates`: **37,654 functions scanned, 17 at exactly 100**,
unchanged across the `5e71c15` merge (which moved 1,645 lines of tests out of `render.rs` without moving any
function across the line). `--margin 15`: 388 within 15.

**Verdict: BATCH, at a finer grain.** All three conditions hold, but the quantity is one *function*, so a collision
needs two branches inside the same function rather than the same file. `scripts/line-pressure.py` is already the
answer to this one, and its design constrains everything else here: *"It always exits 0 on a report. It is a
warning instrument, not a gate failure: making it one would be a second ceiling."*

### 3. `MustBeComplete` censuses

Twelve `MustBeComplete` specs, nine with `stale_fails: true`. (Two further specs are built from another with
`..spec()` -- `complete.rs`'s `loose` and `tests_children_complete.rs:221`'s `least: 1` -- and are the same census
re-run with one field changed.)

| spec | item | `least` | `stale_fails` |
|---|---|---|---|
| `theseus-core/src/outbox.rs:1681` | a file holding an operator call whose kind no sweep can read | 20 | true |
| `theseus-core/src/mcp_server.rs:461` | a protocol method, classified for the MCP surface | 200 | true |
| `theseus-core/src/content.rs:1459` | a file that writes a block into a model request | 12 | true |
| `theseus-core/src/tests_children_complete.rs:153` | a file that starts a child outside `children::spawn` | 10 | true |
| `theseus-core/src/tests_judge_budget.rs:160` | a file that enables the judge with no budget | 5 | true |
| `theseus-core/src/aws/tests_secret_shapes.rs:1272` | a marked, wordless member of a secret-bearing answer | 60 | true |
| `theseus-core/src/rpc/tests_imported_fields.rs:99` | a pub field of an imported session's wire type | *derived* | true |
| `theseus-sandbox/src/seccomp.rs:724` | a system call L1 must refuse | 45 | true |
| `crates/complete.rs:226` | (the judge's own test fixture) | 2 | true |
| `theseus-core/src/tests_day_ceiling.rs:649` | a fact row carrying `cost_usd` | `FOUND` (19) | false |
| `theseus-core/src/aws/tests_presigned.rs:527` | a presigned URL parameter carrying signature material | 3 | false |
| `theseus-core/src/aws/tests_presigned.rs:585` | a paginator's output-token expression | 30 | false |

**Verdict: BRANCH for the comparison, BATCH for one residue.** The set-against-walk comparison is merge-stable
(condition 2 fails). The residue is the **non-conflicting cross-edit**: branch A removes file F's last walked call
site and deletes F's row from the list; branch B, at its own base, adds a call in F and changes no list. Neither
branch conflicts and the merge walks F with no row for it. `stale_fails: true` makes that two-directional, which
is why these nine carry the surface and the three without it do not.

`least` is the merge-stable form of a count and should be read as a floor, not a size: a walk that found fewer read
the wrong tree. Two of the twelve derive it at test time, which is stronger still. `tests_day_ceiling`'s
`least: FOUND = 19` is the one tight floor -- zero slack, deliberately ("one lost is a red") -- so a sibling that
removes a `cost_usd` row reds it at the merge. That is the intended behaviour of a tight floor and not a defect.

### 4. Budgets, caps and bench limits

Written down in code, measured against the merged tree at the join:

| invariant | where | value |
|---|---|---|
| first request, CLI task | `theseus-core/src/first_request.rs:24` | 8,000 tokens |
| first request, DM | `:26` | 15,000 tokens |
| first request, channel | `:34` | 8,900 tokens |
| first request, headless | `:35` | 8,600 tokens |
| a plain turn's frames | `theseus-sim/src/perf.rs:72` | 4.0 |
| a tool turn's frames | `:89` | 8.0 |
| a batch over one call | `:102` | 300 ms |
| the binary | `:115` | 60 MB |
| an L1 start's p95 | `theseus-sim/src/jobs.rs:36` | 25 ms |
| a cancel's frames | `theseus-sim/src/lifecycle.rs:439` | 2 |
| the lifecycle phases | `lifecycle.rs` `budget_ms` | cold 50 + 200·(sessions/10,000), shutdown 100, kill cold+100, swap 200, health 5 |
| nextest retries, timeouts | `.config/nextest.toml` | `slow-timeout` 60 s × 2; retries on named tests only |

The keel guard watches the first four files by name (`BUDGETS` in `scripts/keel-guard.py:1075`), and
`ceiling-raised` watches `long-files.txt`, `shape.sh`'s `limit=` and `.config/nextest.toml`.

**Verdict: all BATCH, and the token and frame budgets are the live ones.** Two branches each adding 300 tokens of
harness text pass alone and exceed 8,000 merged; two branches each adding a frame to a turn exceed 4. The
millisecond budgets are **already answered**: a lane's gate skips the bench (`THESEUS_GATE_NO_BENCH=1`) and the
join gate measures the merged tree. The frame counts run in a lane's gate too, because a count needs no quiet
machine -- so they are measured on both trees, which is the stronger arrangement.

### 5. Not in this class

- `THESEUS_GATE_MIN_FREE_GB` (default 10, `disk_floor` in `scripts/gate.sh`): a property of the machine, not of
  the tree. No branch moves it.
- The 303 `.len() == <literal>` assertions over test-local fixtures (see above).
- `scripts/long-files.txt`'s *stale-entry* direction: a merge cannot make a file shrink that neither branch
  shrank, and the branch that shrinks it is told by `shape.sh` in the same commit.

The answer per invariant, the two shapes to refuse, and the instrument that measures the
headroom on a merged tree are the next sections of this document.
