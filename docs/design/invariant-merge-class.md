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

## The answer

**There is no single answer, and the right response is per-invariant.** The two directions the checklist line
gives do not divide the inventory evenly: one of them does not apply to the invariant that reddened the join.

| invariant | a total? | a set of kinds? | derived at test time? | the answer |
|---|---|---|---|---|
| `long-files.txt` ceiling | worse (below) | no: a line count has no kinds | no: the limit is a *decision*, not a measurement | **Direction A, mechanised** |
| clippy's 100 | worse (below) | no | no | already answered by `line-pressure.py` |
| `MustBeComplete` set vs walk | n/a | **already is** | n/a | leave it; the residue is the guard working |
| a census's `least` | n/a | n/a | yes, and two of twelve already do | leave it; never lower one |
| `assert_eq!(TABLE.len(), N)` beside its table | n/a | n/a | n/a | already branch-local |
| first-request token budgets | no | no | no | Direction A: measured at the join through `private_requests.txt` |
| bench millisecond budgets | no | no | no | already answered: benched at the join, not on the branch |
| a turn's frame counts | no | no | no | already answered: counted on both trees |

### The two shapes to reject, with the arithmetic

**A total is worse than a per-file number.** Replace the five per-file ceilings with one bound on all Rust under
`crates/` and conditions 1 to 3 still hold, while condition 1's *reach* widens from "two branches touched the same
listed file" to "any two branches at all". At the 09:00 departure fifteen branches landed in one cut; the chance
that two of them touch the same ceilinged file is a fraction, and the chance that two of them add Rust lines is 1.
A total maximises the collision rate it was meant to remove. It is also worse to read: the failure has no file to
name, so a cut learns "the tree is 300 lines over" and nothing about which branch to defer.

**"No file may grow by more than N in one branch" is worse, and this is the shape to refuse in terms.** It is a
property of the diff, so it is merge-*additive*, not merge-stable: *k* branches each legal at N give the merged
file +*kN*, and *k* is the cut size -- fifteen at the 09:00 departure. The bound you then need on the file is N·k,
a number nobody wrote down and that changes with every cut, so the hazard has moved rather than gone. And it
abandons the absolute bound, which is the whole point of the shape budget: theseus-goa8's own reason for
`shape.sh` is that "nothing held the shape, so every split the first review made regrew". Under a growth bound
`tests_m3.rs` reaches any length at all, forty legal lines at a time, and no commit ever has to say why.

**Why `long-files.txt` cannot be derived.** Its ceiling is not a measurement of the tree; it is a *decision about
a file*, and each entry carries the decision's reason beside it -- "split by subject (review 2, C7: deferred)", "a
split by subject is a 1.0.6 line". No tree-derived quantity encodes a decision. "This file's length at the last
landing plus a margin" is derivable from git, but it is still a number, and it ratchets upward forever, which is
the regrowth the ceiling exists to stop. So Direction B does not apply to the invariant that reddened the join,
and Direction A is the whole answer for it.

### Direction A: what a cut can detect, and the part that cannot be detected from a branch's diff

The checklist line says "sequence such a branch last". The detection rule, written for the conductor:

> A branch carries a batch-property invariant when its diff touches any of: (1) `scripts/long-files.txt`,
> `scripts/shape.sh`, or any file `long-files.txt` lists; (2) `clippy.toml`, or any Rust file holding a function
> within 5 lines of 100 that the diff grows; (3) a file holding a `MustBeComplete` spec, or a file that adds or
> removes a call site one of those specs walks; (4) `crates/theseus-core/src/first_request.rs`, or anything that
> grows a model request; (5) `.config/nextest.toml`, `theseus-sim/src/{lifecycle,perf,jobs}.rs`.

**It is detectable from the diff alone, and being last does not fix it.** Two limits, both load-bearing:

1. **The branch that reds is often the innocent one.** Classes 1, 3 and 4 fire on the branch that *grew*
   something, but the merge breaks because of the *pair*. In `9d9aa6b8` the sibling added the call site and the
   census lived in a file neither branch's diff touched. A predicate over one branch's diff cannot see a pair.
2. **Last is not a position when two branches qualify.** The 09:00 departure landed fifteen branches; the 23:00
   red of 2026-11-08 was two branches that each fit. Sequencing both "last" puts them beside each other, which is
   exactly the failing arrangement.

So the rule a cut can actually run is the predicate evaluated **on the merge, not on the branch**: merge one
branch at a time and measure after each; the branch that crosses a limit is the one deferred, whatever its
position. The measurement must be cheap or nobody will run it -- `shape.sh` and clippy already hold these limits
at the join, but only after a nine-minute build, and they answer "over" rather than "four lines left".

### The instrument: `scripts/invariant-pressure.py`

One command, about a second, no build, run in whatever tree it is in -- so a cut runs it on the staged merge:

- every `long-files.txt` entry and every unlisted Rust file within `--margin` lines of its limit, closest first,
  with both numbers and the headroom, and `OVER by N` when a limit is already crossed;
- the functions at or over clippy's 100, by calling `line-pressure.py` rather than counting again;
- the census roster -- each spec's `item`, `least` and `stale_fails` -- **named, not measured**;
- with `--since <rev>`, `(+N in this range)` against each invariant the range moved, which answers "which branch
  spent the room".

Two rules it keeps, both inherited:

- **It always exits 0 on a report**, exiting 2 only when it read nothing. This is `line-pressure.py`'s reason,
  and it holds here for the same cause: a second gate on these numbers would be a second ceiling that the next
  merge has to argue with, and `shape.sh` and clippy are the ones that hold them.
- **It never re-implements an invariant.** It reads numbers already written down and measures the tree the cheap
  way. The censuses are listed and not walked: a second walk of the same world here would be a second copy of the
  rule, free to disagree with the test that holds it. What a cut needs from them is not a number but a roster --
  which tests to re-run on the staged merge.

## What was deliberately not changed

The checklist line's prohibition, kept: **no census was deleted, no `stale_fails` was set to false, and no `least`
was lowered.** One argument is recorded here rather than acted on: `tests_day_ceiling`'s `least: FOUND = 19` is
the inventory's one zero-slack floor, so a sibling that removes a `cost_usd`-bearing fact row reds the merge
rather than the branch. That is what a tight floor is for and the right reading is that it is working; if anyone
ever wants slack there it is an owner call, and this paragraph is the whole case for it.

The three `shape.sh` ceilings with single-digit or three-line headroom (`runtime.rs` 4, `kernel.rs` 9,
`protocol/src/lib.rs` 3 with no legal entry) are each one cut away from the 2026-11-08 red. Nothing here raises
one: `invariant-pressure.py` makes them visible in a second instead.
