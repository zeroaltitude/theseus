# Cloud report: tail-tooling (theseus-qfohm)

Branch `cloud/20261021-tail-tooling`, from `main` at `6918e3c1`. One step, one commit: `a928bc7`.
The step touches only `scripts/keel-guard.py` and `scripts/test_keel_guard.py`, no Rust, no budget value, and no
line ceiling. Nothing was added to Cargo.lock or any package-lock.json.

**One addition beyond the brief, for the owner to hear first.** The brief expected the guard, over `94eb8a7d`, to
print that commit's `Keel:` line as refused because `budget-raise` is not a rule. It could not have: git never read
that line as a trailer. It sits in its own paragraph above `Co-Authored-By:`, and git takes trailers only from a
message's last paragraph, so `%(trailers:key=Keel)` is empty for that commit, and the guard saw no ack at all. The
same is true of four more commits on `main`: `10f0dc7`, `70b0c82`, `c9cfda3` and `2f74d06` (the last, "keel: ack
the pattern miner's [self.mint] field", was an ack by intent). So item 3 now also refuses a `Keel:` line that is
not a trailer, saying so, and names any fault of its form in the same line. It only prints; an ack that git does
read is judged exactly as before.

## What I found

- `compare_rows` pairs each base line with a number to a head line of the same shape. A line that is new at the
  head has no partner, so it is never judged. `margin_ms` is a `match` whose `_ => 0.0` gives every unnamed phase a
  margin, so a new arm over it (`"seed" => 2.0`) raises that phase's limit and passes silently. `94eb8a7d` did this
  twice. Before this change the base's guard printed `keel: ok (1d092ded..94eb8a7d, …; 3 files changed, 0 findings
  acked, 0.1 s)`.
- `budget_ms`'s `"surfaces"` arm reads `surfaces::BUDGET_MS` (50.0) in `crates/theseus-sim/src/lifecycle/surfaces.rs`,
  and that file was in no rule.
- `acks` took any `[\w-]+` as a rule. A line naming no rule, or a path with a word after it, matched nothing, and
  nobody was told.

## What changed, rule by rule (`a928bc7`)

1. **A new arm is judged against the fall-through it replaced** (`match_arms`, `compare_arms`, both called from
   `budget_rule`). For a keel `fn` whose body holds a `match` over string literals with a `_ =>` arm, at both base
   and head, each arm line is read as a value per pattern: the arm that names the pattern, else the fall-through.
   - A head arm is judged on the patterns it names, against each pattern's value at the base. So a new arm is
     judged against the base's fall-through. The old side reads `_ => 0.0 (the fall-through)`. A split or merged
     arm keeps its value (`"cold" | "vault" => 7.0` as two arms of 7.0 is clean; 7.0 and 8.0 is caught on the 8.0
     arm, its old side `"cold" | "vault" => 7.0`).
   - A pattern the base gave no number (`_ => None`, or an arm such as `Some(cold)`) is unbudgeted there, so a new
     budget for it is no finding.
   - A base arm with a number is `(gone or rewritten)` as before when one of its patterns is no longer named by a
     numbered head arm. This keeps `test_a_cancel_budget_dropped`'s finding and text. It also keeps the old
     reading of an arm deleted over a numeric fall-through (a margin arm deleted lowers that phase to 0.0, but it is
     still reported as a removed line, as today). I kept that on purpose: it is the conservative side.
   - Every other line of the fn (`let cold = 50.0 …`, the fall-through itself, `match phase {`) goes through
     `compare_rows` exactly as before, so `_ => 0.0` to `_ => 1.0` is still caught there. `gate.sh`, `const` items
     and fns that are not such a match are unchanged.
   - The docstring's `budget-raised` paragraph gains the line "a new arm of a keel fn's match is judged against the
     base's fall-through for it".
2. **The sim crate's budget constants, by name.** `SIM_BUDGET_CONST` and `sim_budget_items`: a `const` whose name
   holds `BUDGET`, `MARGIN`, `SLACK`, `TOLERANCE` or `ALLOWANCE`, in `crates/theseus-sim/src/**/*.rs` except test
   files (`TEST_FILE`, the sealed-reach rule's pattern, and any path under a `/tests/` directory). These go through
   `cap_code_rule`, which now takes the rule name and the item reader as arguments (its defaults are today's, so its
   own call and findings are unchanged). Pairing is by name across the range's changed files: a raise or a constant
   that is gone is a finding, a moved one keeps its value, and a new one is no finding. The finding's path is the
   head's. The docstring says the scope stays in the sim crate, and why. `perf.rs`'s `BATCH_OVER_ONE_CALL_MS`
   (300.0) joins `BUDGETS` by hand.
3. **An ack that names nothing is refused, saying so** (`ack_fault`). After the signature checks, which keep their
   order (an unsigned line is still refused as "unsigned"), a line whose rule is not in `RULES` is refused with
   `` `budget-raise` is not a rule (the rules: test-removed, test-ignored, …, sealed-reach) ``, and one with a space
   inside a comma-separated path with `` a path holds a space: `crates/theseus-sim/src/lifecycle.rs margin_ms`
   (paths are comma-separated) ``. Both print through the existing `keel: refused the ack …` line. A `Keel:` line in
   the body that git did not read as a trailer is refused with `not a trailer: git reads trailers from the message's
   last paragraph only (put the Keel: lines beside Co-Authored-By, with no blank line between)`, followed by `; and
   <fault>` when its form has one. The docstring's ack paragraph says all three.

## New tests and their plants

The `LIFECYCLE` fixture gains the brief's `margin_ms`; `BASE` gains `crates/theseus-sim/src/lifecycle/surfaces.rs`
(`BUDGET_MS = 50.0`) and `crates/theseus-sim/src/perf.rs` (`BATCH_OVER_ONE_CALL_MS = 300.0`). No existing case was
edited, and all of them pass unchanged (107 before, 107 of the 123 now).

New class `BudgetArms(Case)`:
- `test_a_margin_arm_added_over_the_fall_through` (the key test): `"seed" => 2.0` before `_ => 0.0` is caught as
  `budget-raised` on `crates/theseus-sim/src/lifecycle.rs`, with the trailer line `Keel: budget-raised
  crates/theseus-sim/src/lifecycle.rs`, and `_ => 0.0 (the fall-through)` as its old side.
- `test_a_margin_arm_added_at_zero_is_clean`: `"seed" => 0.0` is clean, and so is a new `"seed" => Some(2.0)` in
  `budget_ms` under its `_ => None`.
- `test_a_margin_arm_split_keeps_its_value`, `test_a_margin_arm_split_and_raised_is_caught_on_the_raise` (exactly one
  finding, `"cold" | "vault" => 7.0 -> "vault" => 8.0`).
- `test_a_margin_lowered`, `test_a_fall_through_raised`.
- `test_the_lane_margin_shape_is_caught`: both of `94eb8a7d`'s arms at once under a new doc comment; exactly two
  `budget-raised` lines, one per arm.
- `test_a_sim_budget_const_raised` (50.0 to 60.0, caught), `test_a_sim_budget_const_lowered` (40.0, clean),
  `test_a_sim_budget_const_moved` (unchanged to `crates/theseus-sim/src/surfaces.rs`, clean),
  `test_a_new_sim_slack_const` (clean), `test_a_budget_const_raised_outside_the_sim_crate` (`BUDGET_MS` raised in
  `crates/demo/src/lib.rs`, clean).
- `test_a_batch_allowance_raised`: 300.0 to 400.0, caught.

New class `AckLines(Case)` (no gpg needed): `test_a_keel_line_above_the_trailers_is_refused`, `94eb8a7d`'s shape.

In the signed class `Acks` (gpg): `test_an_ack_with_no_such_rule_is_refused` (a commit signed by the listed key
carries `Keel: budget-raise crates/theseus-sim/src/lifecycle.rs — the owner's yes`; the guard prints `refused the
ack` and `is not a rule`, the finding stays open, exit 1) and `test_an_ack_with_a_space_in_a_path_is_refused`.

Plants (each: plant, run the named tests, restore the file from a saved copy, `touch` it, `git status`; the restored
file was `cmp`-identical to the saved one each time):

| Plant | Failed |
|---|---|
| 1. unpaired arms judged as today (`barms.get(p, bfall)` → `barms.get(p, ([], 0, ''))`) | the key test, `test_the_lane_margin_shape_is_caught` |
| 2. the match reading off (`if kind == "fn"` → `if False`) | the key test, `test_a_margin_arm_split_keeps_its_value` |
| 3. the name rule dropped (the sim call given no `out`) | `test_a_sim_budget_const_raised` |
| 4. `BATCH_OVER_ONE_CALL_MS` dropped from `BUDGETS` | `test_a_batch_allowance_raised` |
| 5. any rule word accepted (`if m.group("rule") not in RULES` → `if False`) | `test_an_ack_with_no_such_rule_is_refused`, `test_a_keel_line_above_the_trailers_is_refused` |
| 6. a spaced path accepted (`return spaced and f…` → `return False and f…`) | `test_an_ack_with_a_space_in_a_path_is_refused` |
| 7. body `Keel:` lines unread (the not-a-trailer check → `if False`) | `test_a_keel_line_above_the_trailers_is_refused` |

## The suite

`python3 -m unittest -v scripts/test_keel_guard.py`: **Ran 123 tests, OK (skipped=1)**. gpg is on this VM, so the
signed class ran, the two new ack cases included. The one skip is `Acks.test_an_ssh_signed_ack_is_refused_even_where_
it_verifies` (`no ssh-keygen: the ssh-signed ack is not tested here`), which existed before. For the maintainer:
`python3 -m unittest scripts.test_keel_guard -k ack` from the repo root runs the ack cases.

The tail of `python3 -m unittest -v scripts/test_keel_guard.py`:

```
test_an_ack_with_a_space_in_a_path_is_refused (scripts.test_keel_guard.Acks...) ... ok
test_an_ack_with_no_such_rule_is_refused (scripts.test_keel_guard.Acks...) ... ok
test_an_ssh_signed_ack_is_refused_even_where_it_verifies (scripts.test_keel_guard.Acks...) ... skipped 'no ssh-keygen: the ssh-signed ack is not tested here'
...
----------------------------------------------------------------------
Ran 123 tests in 62.972s

OK (skipped=1)
```

122 cases passed, 1 skipped (107 cases before this change, all of which still pass).

## The guard over real history

`python3 scripts/keel-guard.py --base 1d092ded --head 94eb8a7d` (exit 1):

```
keel: refused the ack `Keel: budget-raise crates/theseus-sim/src/lifecycle.rs margin_ms — the owner, Discord DM 2026-10-20 23:09: "I would like for all the work you articulated to be done ... from your entire list"` on 94eb8a7d7a: not a trailer: git reads trailers from the message's last paragraph only (put the Keel: lines beside Co-Authored-By, with no blank line between); and `budget-raise` is not a rule (the rules: test-removed, test-ignored, assert-removed, assert-weakened, allow-added, budget-raised, cap-raised, ceiling-raised, ci-loosened, keel-file, sealed-reach)
keel: 2 erosions of a test, a budget or a ceiling (1d092ded..94eb8a7d, 1d092dede7):
  crates/theseus-sim/src/lifecycle.rs:238: budget-raised: _ => 0.0 (the fall-through) -> "seed" => 2.0
  crates/theseus-sim/src/lifecycle.rs:239: budget-raised: _ => 0.0 (the fall-through) -> "health" => 3.0
keel: each passes only with the owner's yes: a commit in the range, signed by a key in scripts/keel-signers (at the base), whose message carries these trailers, each with its reason:
  Keel: budget-raised crates/theseus-sim/src/lifecycle.rs — <why>
```

The signature line on this VM reads `%G?` = `E` (the owner's public key is not here), but the not-a-trailer refusal
comes before any signature check, because git hands the guard no trailer for that commit at all. On the owner's
machine the line should read the same.

The last 20 first-parent merges of `main` (`git log --first-parent --merges -20 origin/main`, from `8a6743c` to
`832c5da`), each `--base M^1 --head M`, the base's copy of the guard against this branch's, findings and refusals
compared: **one merge differs, `b87a46c` (Merge lane/lifecycle-margin)**, which gains the two real arms above and the
refusal of `94eb8a7d`'s line. That is the approved raise, not a false positive. The other 19 merges, among them
`88f2e48` (lane/lifecycle-stalls, which edits `lifecycle.rs`), print the same with both copies. No false positive, so
no rule needed tuning.

A check by hand on the real file (the working tree, restored after): `"health" => Some(5.0)` raised to 6.0 is caught
with the old side `"health" => Some(5.0)`; a new `"fresh" => Some(1.0)` under `_ => None` is clean.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` stops in its `keel` phase on the expected `keel-file`
finding (the output below), as the brief says it will: `gate: FAILED in keel`. So I ran the phases after it by hand,
in the gate's order, with the same environment (and `CARGO_INCREMENTAL=0`, see the disk note):

| Phase | Result |
|---|---|
| keel tests | ok (123 cases, 1 skipped) |
| fmt | ok |
| shape | ok |
| clippy (`--workspace --all-targets -D warnings`) | ok |
| clippy-probe | ok |
| cockpit (lint, test, build, `dist/index.html`) | ok |
| test build (`cargo nextest run --workspace --no-run`) | ok |
| reader rule (`tests_registry`) | ok, 15 tests |
| suite (`cargo nextest run --workspace --no-fail-fast`) | 7,535 run: 7,500 passed, 35 failed, 53 skipped |
| protocol types | ok (no change) |
| turn bench (`--runs 5 --burst 0`) | ok |
| size | ok (no release build: skipped by its own rule) |
| deny (`--offline`) | ok (`cargo deny fetch` ran after the disk was freed) |

`features` was not run by hand (the step changes no manifest). The 35 suite failures:
- **34 are the known L1 failures as root** (theseus-pv6i): 20 in `theseus-sandbox` (`bench spawn_100` and 19
  `contract` cases) and 14 in `theseusd::sandbox`.
- **1 is a load flake, not this change's:** `theseus-core tests_horizon::the_agenda_expands_a_series_by_the_kernels_
  arithmetic_and_skips_what_is_paused`, at `tests_horizon.rs:528` (`busy.lines.len()` was 1, not `AGENDA_MAX` 200:
  the "every 1m" routine's agenda). Alone it passes under both `TZ=America/Phoenix` and `TZ=UTC`. Under the load
  recipe (nice 19, four busy loops at nice 0) it failed 1 run of 5, taking 29 s against 3 to 5 s for a pass. It is not
  on the flaky list and is not one of the brief's known cases. This step changes only Python, so the maintainer may
  want an issue for it.

**Disk.** This VM's writable allowance filled twice: once during `cargo deny fetch` after the cold build (target 17 GB
plus incremental), and once when `CARGO_INCREMENTAL=0` made cargo rebuild everything beside the first build. I deleted
`target/` and rebuilt with `CARGO_INCREMENTAL=0` throughout (about 29 GB used at the end). Nothing outside the
repository and `/tmp` was touched.

## The keel guard on this branch

`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` before the report's commit:

```
keel: 2 erosions of a test, a budget or a ceiling (origin/main..HEAD, 6918e3c1e2):
  scripts/keel-guard.py:14: keel-file: (`--` or ` - ` may stand for the dash; a path may be a glob). An unsigned commit's trailer counts for nothing, so a -> (`--` or ` - ` may stand for the dash; a path may be a glob). A line whose rule is not one of the rules below, whose
  scripts/test_keel_guard.py:57: keel-file: pub const CANCEL_FRAMES: usize = 2; -> pub fn margin_ms(phase: &str) -> f64 {
keel: each passes only with the owner's yes: a commit in the range, signed by a key in scripts/keel-signers (at the base), whose message carries these trailers, each with its reason:
  Keel: keel-file scripts/keel-guard.py, scripts/test_keel_guard.py — <why>
```

It names nothing but `keel-file` on the two guard files, as the brief says. The trailer the maintainer adds at the
join, quoting the owner's yes:

```
Keel: keel-file scripts/keel-guard.py, scripts/test_keel_guard.py — the owner, <where and when>: "<his words>"
```

## Live check for the maintainer

No daemon behaviour changes, so the live check is the guard itself, on the owner's machine, from the repo root of this
branch:

1. `python3 -m unittest scripts/test_keel_guard.py`: 123 tests, OK; with ssh-keygen present, no skip.
2. `python3 scripts/keel-guard.py --base 1d092ded --head 94eb8a7d`: the two `budget-raised` lines on
   `lifecycle.rs:238` and `:239`, the refused line for `94eb8a7d` (not a trailer; and `budget-raise` is not a rule),
   exit 1.
3. `for m in $(git log --first-parent --merges --format=%h -20 origin/main); do python3 scripts/keel-guard.py --base
   $m^1 --head $m --quiet; done`: only `b87a46c` names a `budget-raised` finding.

## Left, uncertain, and for the owner

- **The `Keel:` lines git never read.** Five commits on `main` carry one above their last paragraph (listed at the
  top). Nothing to change in them; but the agents' commit recipe should put `Keel:` lines in the same paragraph as
  `Co-Authored-By:`. The keel section of `scripts/AGENTS.md` ("The ack") could say so in one sentence.
- **`70b0c82` and `c9cfda3` name `policy-gate`,** which is not one of the guard's rules. If a policy gate is meant to
  be keel-held, that is a rule this guard does not have.
- **A base arm deleted over a numeric fall-through** is still `(gone or rewritten)`, though for `margin_ms` it lowers
  the margin. Conservative; say if the owner wants it judged by value instead.
- **Arms the reading skips:** an arm spread over several lines, or one with a guard (`"x" if … =>`), is read line by
  line by `compare_rows` as before. A base arm with no number (`Some(surfaces::BUDGET_MS)`) is unbudgeted to this
  reading; the constant it names is held by item 2 instead.
- **Constants outside the sim crate** with those names: 27 at this base. None is a bench limit the gate measures, so
  none should go into `BUDGETS` by hand. The nearest: `theseus-core/src/rsi/backlog/read.rs`'s `TURN_BUDGET_MS`
  (120,000) and `FIRST_TOKEN_BUDGET_MS` (10,000), the self-improvement reader's time limits; `memory_lookup.rs`'s
  `BUDGET` (1,500) and `BUDGET_MAX` (6,000), token budgets; `attach.rs`'s `DOCUMENT_BUDGET_BYTES`; and
  `compiler.rs`'s `MARGIN_PERCENT` (40), where a raise tightens. `aws/bootstrap.rs`'s `DEFAULT_BUDGET_USD` is a spend
  cap and is already held by `cap-raised`. The rest are names, intervals, or margins whose raise is a tightening
  (`REFRESH_MARGIN`, `FIRE_MARGIN_MS`, `REPORT_MARGIN_SECS`, `SCRUB_MARGIN`), or strings.
- **Docs.** `scripts/AGENTS.md`'s keel paragraph lists `budget-raised` as "the lifecycle, turn and jobs benches'
  limits, the busy allowance's default"; it could add "a sim-crate budget constant by name". Not edited here, as the
  brief asks.
