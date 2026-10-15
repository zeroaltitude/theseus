# CLOUD_REPORT: self-pipeline (theseus-pw1q.16)

Branch `cloud/20261013-self-pipeline`, on main as cloned (`3f1f5248`, store format 33). Three commits on top of the task
commit, then this report:

- `4df00393` sim: the stand-in model fills `{item}` from a self item's objective
- `1c721ff4` rsi: the self pipeline: builds at once in their own trees, reviews and joins one at a time
- `908f26cc` rsi: trees opened at once on one repo each open; the pipeline's git and reads leave the worker

No store format bump (seven new `LedgerKind`s and one META key, `self.pipeline`). No new dependency. No keel finding
of mine (below).

## The state machine (drawn first; it is also the module doc of `rsi/pipeline.rs`)

```text
                  the night files and ranks (the backlog)          drop (the owner's word)
                              │                                ┌──────────────────────────► DROPPED
                              ▼                                │   its live build stopped,
   ┌───────────────────►  READY  ── slot + room + budget ──► BUILDING   its trees deleted (cleared)
   │                   (proposed, rank   (capacity.rs, the     (in_progress: its "self" turn
   │                    order now)        day line, rsi::gate)  runs, queues or waits on the owner,
   │                     ▲     │                                or inside the 5-minute grace)
   │   halt: every build │     │ held (slots/disk/memory/cpu/      │                   │
   │   stopped, back to  │     │ budget): one `held` row each      │ turn over with no green branch
   │   ready ────────────┘     │ time the reason changes            │ ──► ENDED (failed, trees deleted)
   │                           ▼                                    │
   │                     (stays ready)                              │ self.branch.built, gate green
   │                                                                ▼
   │                                                           REVIEWING (self-join: planted revert,
   │                                                                │  reviewer on another model)
   │      reject, 1st time: RETURNED (findings in the next build)   │
   └────────────────────────────────────────────────────────────────┤
          reject, 2nd time: PARKED (waiting_human, the owner's view)│ accept
                                                                    ▼
                                                               JOINING (queued: one pass at a time,
                                                                │  Shared::joins in-process,
                                                                │  [self] join_lock across processes)
                                                                ▼
                                       JOINED (self.joined) ── its leftover trees deleted (cleared)
                                       or HELD (keel card, red gate on the merge, no key, conflict)
```

Concurrency and locking, in one place:

- **Nothing in memory decides.** Every queue is derived each tick from the record: the backlog's task records and
  order, the item sessions' executions, and self-join's rows (`self.branch.built`, `self.review.verdict`,
  `self.joined`, `self.join.stopped`). A restart loses nothing. What the pipeline has *said* is kept in the META record
  `self.pipeline` (keys of transitions said, and the hold last said), written in the same frame as the task records
  and rows of that tick, so a row is written once.
- **One tick at a time** (`Shared::tick`, a tokio mutex). The tender ticks 60 s after serving, then every 120 s, or at
  once when kicked (a drop, a move, a halt, a night that filed, a join pass's end). Only with `[self] mode = "act"`:
  with the mode off no task and no timer exist, and the night starts items as before.
- **One self-join pass at a time in the daemon** (`Shared::joins`, taken *inside* `self_join_pass`, so the join
  tender's own 15-minute pass and the pipeline's are serialized), and the pipeline never spawns a second pass while
  one runs (`Shared::joining`). Across processes, the mkdir lock `[self] join_lock` (self-join's) keeps a self-join off
  the owner's own joins; the pipeline adds nothing there.
- **One tree open or close at a time** (`rsi::tree`'s process-wide lock, commit `908f26cc`): two builds start in one
  tick, and two `git worktree add`s on one repo race for its locks (found live, below).
- **rsi::gate** is asked at each tick's start and before each start; a halt (anything but `Allowed`) at the next tick
  stops every build in flight (`stop_execution`) and puts its item back to `proposed`, once per halt
  (`Shared::halt_swept`), and nothing starts until the owner's resume.
- **While the pipeline runs, the night files and ranks and starts nothing** (`backlog_starts` applies only with the
  pipeline off), so the two never start the same item; `backlog_start_item` takes the backlog's own lock and starts
  only a still-`proposed` item.

## Step 1: the pipeline, the guard, the builder seam, the view (`1c721ff4`)

**What I built**

- `rsi/pipeline.rs` (+ `pipeline/queues.rs`, `tender.rs`, `view.rs`, `tests.rs`): the loop above. A start goes while
  `building < [self] pipeline_builds` **and** fewer self trees are open **and** the self budget's day line is not
  reached **and** the capacity guard finds room, every check every time. A held start writes one
  `self.pipeline.held` row when the reason changes. A rejected review (self-join puts the item back to `proposed` with
  its reasons in a `review:<branch>@<head>` evidence): the first time `self.pipeline.returned` and the next build's
  objective carries the findings; the second time `self.pipeline.parked` and the item goes to `waiting_human` (the
  owner's `theseus self move <item> 1` is the undo the row names). A build whose turn ended with no green branch is
  `ended` (task `failed`), a dropped one is stopped; both, and a joined item, have their trees deleted (`cleared`:
  regenerable caches, removed, never moved to a trash).
- `rsi/capacity.rs`: the guard. Slots (builds in flight and open trees, so a tree a red build left holds its slot),
  disk (`statvfs` on the trees' filesystem against `[self] min_free_gb`), memory and CPU PSI (`some avg10`, through a
  new `theseus_store::pressure::some_avg10_of`, the same reader the background passes use), thresholds 10 % and 60 %.
- **Builders as a trait** (`Builder`): `Local` is the only one (the backlog's start: a `"self"` session and its first
  turn, which calls `self.propose_change`). The seam is documented on the trait; see "Remote builders" below.
- `self.pipeline` (`rpc/rsi.rs`), `theseus self pipeline [--week]` (`self_cmd/pipeline.rs`): ready, building,
  reviewing, joining, held for the owner with why; the room (slots, trees, free GB, PSI) and the next start's hold;
  today's spend and `self.pipeline.*` rows; `--week` adds rows a day, the accept rate, regressions (`self.reverted`,
  `self.vetoed`), the cost a joined row over the week and today's. Wire types at the end of
  `theseus-protocol/src/rsi.rs` under a comment naming the row; TypeScript regenerated.
- The owner's words act at once: `drop` (the pipeline stops its build and deletes its trees at the kicked tick),
  `move` (reorders ready: the queue reads the backlog's order now), `halt self` (above).
- Config: `[self] pipeline_builds = 1`, `min_free_gb = 40`, in `config/rsi.rs` and the template's `[self]` block,
  with a template test.
- Facts: `fact/rsi_pipeline.rs`, seven kinds in `SELF_ROWS` and `LedgerKind`, each carrying what/why/numbers/undo so
  `theseus self log` shows them unchanged.
- Shared files took only my lines: `rsi/mod.rs` (mod lines, the `Switch` field), the backlog's start (a
  `backlog_start_item` for one id, the night starting none while the pipeline runs), `join.rs` (the in-process lock, one
  line), `join/tests.rs` and `tests_tree.rs` (made `pub(crate)` for reuse, and a slow reviewer stand-in),
  `theseusd/src/main.rs` (one call after serving), `self_cmd.rs` (one arm).

**How it was proved**

- `rsi::pipeline::tests` (9) over fake builders, a stand-in host and self-join's rig: `n_builds_run_at_once_and_never_n_plus_one`;
  `the_guard_holds_on_low_disk_pressure_and_trees_with_the_slots_free`; `the_budgets_day_line_stops_new_starts_and_says_so`;
  `a_rejected_review_returns_once_then_parks_for_the_owner`; `a_halt_stops_every_build_at_the_next_tick`;
  `ended_and_dropped_builds_free_their_slot_and_their_trees`; `mode_off_does_nothing`; `one_join_pass_at_a_time`;
  `a_joined_items_tree_is_deleted`. Plus `rsi::capacity::tests` (3), the config key test, the fact's test.
- **Planted reverts**:
  1. *The guard skipped when a slot is free* (`check` returning no hold once the slots pass): 
     `capacity::tests::each_hold_says_why_and_a_free_slot_still_reads_the_host` and
     `pipeline::tests::the_guard_holds_on_low_disk_pressure_and_trees_with_the_slots_free` failed.
  2. *Two joins at once* (the `Shared::joins` lock removed from `self_join_pass`): `one_join_pass_at_a_time` failed at
     `no second join tried` (a `self.join.stopped` row: the second pass tried the same join). Both files restored and
     touched; `git status` clean of them.
- `one_join_pass_at_a_time` was order-dependent in its first form (a pass with nothing to do could end before the
  second was polled): it now holds the join lock so the first pass is surely in flight; `--stress-count 10` passed.
- The rsi, self, config, protocol, CLI and fake-model suites: 700 tests passed.

## Step 2: a defect the live check found, fixed (`908f26cc`)

The first live run started two builds in one tick; one failed at `self.propose_change`'s first call:
`git worktree add -q -b self/… origin/main failed: error: could not lock config file .git/config: File exists`. Two
`git worktree add -b` on one repo at once race for the repo's config lock (the branch's upstream tracking is written
there); the loser left a half-made tree, and its build ended with no branch. Self-pr's `tree::open` had never been
called twice at once before this row.

- **Fix**: `rsi::tree::open` and `close` take one process-wide lock, and the branch is made with `--no-track` (the
  push names its refspec, so nothing read the upstream; an open now writes no config at all).
- **Test first**: `rsi::tests_tree::trees_opened_at_once_each_open` (six trees at once, four rounds) failed with the
  same config-lock error before the fix, and passes after; the second live run built all three items with no failure.
- In the same commit: the tick's queue read (a ledger read that grows with history) and the tree deletion (git and
  the disk) now go through `theseus_store::blocking`, never on a runtime worker.

## The live check (this VM)

Rig: `/tmp/pipe-live`, made by a script (kept in the session's scratchpad; the commands are below): a toy Cargo crate
as `[self] repo`, cloned from a bare `origin.git`, its `scripts/gate.sh` `cargo test -q`, a stand-in gpg (the join
tests' own) as `gpg.program` with `user.signingkey`; `theseus-sim fake-model --rules` as `[model] api_base` with two
profiles (`default`, Sonnet 5.5; `fable`, Fable 5.1, the reviewer); a scratch daemon on its own socket and state dir,
`[self] mode = "act"`, `remote = "origin"`, `pipeline_builds = 2`, `min_free_gb = 1`; the switch resumed on the
scratch only. Three items planted by three turns whose stand-in sends `fs.read`, `fs.list` and `fs.write` invalid
input twice each (`tool.invalid_input` needs two sightings); the night (10 minutes after the start) filed them. Each
item's build turn is scripted: `self_propose_change` (opens the tree), a `proc_run` that writes `fix/<slug>.txt` and
`tests/<slug>.rs` (a test that fails without the fix file), `self_propose_change` again (commit, gate, report,
push). The reviewer's stand-in answers `VERDICT: accept`.

Three runs (each a fresh rig):

1. **Found the tree race** (before `908f26cc`): two builds started at once (`self.pipeline.started` twice, then
   `self.pipeline.held` "slots" for the third); one failed in `git worktree add` on the repo's config lock and its build
   `ended` with its tree `cleared`; the third started at the next tick when a slot freed. Its reviews were all
   `refused` because my rig's config named no `fable` profile (the review said so, and the pipeline showed both
   branches under "Held for the owner" with that reason).
2. **The return-then-park path, by accident**: my rig's scripted build wrote no test (the toy had no `tests/`
   directory), so the planted revert found `no-tests` and every review rejected: each item went back to ready once
   (`self.pipeline.returned`, rebuilt with the findings in its objective, on branch `…-2`) and then parked on its
   second rejection (`self.pipeline.parked` ×3; `theseus self pipeline` listed all three under "Held for the owner:
   rejected twice"). All three trees opened with no failure: the fix held.
3. **The whole path** (the run the task asks for), at 03:30 UTC:
   - the night filed three items; the tick **started two at once** (03:30:46.557 and .561) and **held the third**:
     `self.pipeline.held` "every slot is taken: 2 building and 0 self trees open, of [self] pipeline_builds = 2";
   - the third started at the next tick (03:32:46.592), once a build had pushed its green branch;
   - **reviews and joins one at a time**: a join pass reviewed two heads (both `accept`, the planted revert `failed`
     as it should: the test fails with the fix file removed) and joined one; then the next; the rows are strictly
     ordered: verdict 03:32:47.693, `self.joined` 48.369, verdict 49.444, joined 50.124, verdict 51.229, joined
     51.919; never two joins in flight;
   - the toy repo's `main` gained three `--no-ff` merges, each signed with the stand-in key, one per branch, with
     `fix/<slug>.txt` and `tests/<slug>.rs` each; a fresh clone's `cargo test` passes its three tests;
   - no tree or build tree left (`state/self/trees` and `state/self/target` empty: self-pr closes the tree after a
     green build, and the pipeline found nothing to clear); `theseus self pipeline` ends at "Ready: none … Held for
     the owner: none", $0.43 of $30 spent, 6 pipeline rows.

## Measures and benches

- `theseus-sim bench lifecycle --runs 10 --check` (a debug build, on this 4-core VM): every phase within its budget
  but one, **health after the seed, p95 5.6 ms and then 5.9 ms on a 5 ms budget** (p50 3.5 and 4.0 ms), both runs.
  This branch changes nothing on that path: with `[self] mode = "off"` (the bench's config) `pipeline_after_serving`
  returns at once and no task or timer exists, and health has no new field. The budgets are measured on the owner's
  release build; please run the bench there.
- FAST: a plain turn's frames are unchanged (no turn-path code touched); no new timer on a turn's path; the pipeline's
  own timer exists only with mode act.

## Remote builders (the seam, not built)

`Builder::start(core, item, findings) -> session` is the whole seam: a remote builder starts the same `"self"` work
elsewhere and must end in the same `self.branch.built` row (seam S4) with a branch pushed to `[self] remote`, so the
review and join need nothing new.

- **An AWS hand** (`aws.hands.run`, Fargate): the repo, a build image with the toolchain, the item's brief, and a
  push credential for `self/*` only. Needs the hands' account, a spend line for its hours, and a scoped push key.
- **A cloud session** (as the outer loop launches them): a branch, a brief like CLOUD_TASK.md, and that service's
  credential and spend.

Each needs a credential or a spend line, which is the hard keel: the owner's decision.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` stops in its **keel** phase on a finding that is not
this branch's: `crates/theseus-sim/src/lifecycle.rs:155: budget-raised: "seed"`, from `1c2beb27` on main, whose
`Keel:` ack the guard refuses here because the owner's signature does not verify on this VM (`%G?` is `E`: no key).
`python3 scripts/keel-guard.py` gives the same single finding; none of my commits touches a test, budget, cap or
ceiling. So I ran the later phases myself, as the task says, before each commit (the last run after `908f26cc`'s
change):

- fmt, shape, cockpit lint/test/build, the test build, the reader rule (15/15): ok. protocol types: ok.
- **suite**: 4628 run, 4594 passed, 34 failed: 33 are the known L1 tests (theseus-sandbox's contract and
  `spawn_100`, theseusd's sandbox tests; a root daemon, theseus-pv6i) and one is
  `theseus-core tests_work::the_substrate_scenario_reads_as_one_tree_and_pings_for_the_right_reasons`, which failed
  in both full runs and passes alone: under load the golden's snapshot catches the conversation `Running` where the
  golden has `Ready` (a timing race in the work board's scenario; not on the known list; nothing of this branch is
  on the board's path). In the first full run `theseusd::stop_checkpoint a_clean_stop_with_every_service_on_leaves_nothing_to_replay`
  and `theseusd::gone_jobs a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran` failed too, and passed
  alone.
- **deny**: fails on a new advisory, RUSTSEC-2026-0346 (`rustls` 0.23.45, `Debug` of client session values), against
  `Cargo.lock` as cloned; this branch changes no dependency.

## Keel findings expected

None from this branch. The one the guard reports (above) is main's, awaiting the owner's signed ack.

## Left, uncertain, and for the owner

- **Docs to write** (not mine to edit): Part III's item and `docs/status.md`; crates/theseus-core/AGENTS.md's
  self-improvement paragraph should gain "**The self pipeline** (theseus-pw1q.16; `rsi/pipeline.rs`,
  `rsi/capacity.rs`, `fact/rsi_pipeline.rs`)" with the locking notes above, and that `rsi::tree::open`/`close` are
  serialized in-process.
- The TUI's self view and Discord's `self pipeline` word have not joined, so only the CLI shows the pipeline.
- The capacity thresholds (memory PSI 10 %, CPU PSI 60 %) are first guesses; `min_free_gb = 40` is the task's.
- A parked item stays `waiting_human`; the owner's `theseus self move <item> 1` puts it back, and the pipeline then
  counts its two earlier rejections, so it would park again on one more. Whether a move should reset the count is the
  owner's call.
- `self.pipeline.held` is said when the reason *changes*, so a hold that clears and returns writes again: on a busy
  host this could be a row per tick in the worst case (CPU PSI oscillating around 60 %). Not seen; worth a look.
- The fake model's `{item}` (theseus-sim) is a small addition outside my crate, for the live check alone.

## Live check commands for the maintainer

Build the branch (`cargo build -p theseusd -p theseus -p theseus-sim`), then (paths are examples; the stand-in key is
any string, never a real one):

```bash
R=/tmp/pipe-live; B=target/debug
# the toy repo: a bare origin with a Cargo crate whose scripts/gate.sh is `cd "${THESEUS_GATE_ROOT:-.}" && exec cargo test -q`,
# its fix/ and tests/ directories present; a clone as $R/repo with gpg.program set to the join tests' FAKE_GPG
# script and user.signingkey FIXTUREKEY.
# rules.json for `theseus-sim fake-model`, in this order:
#   {"when": "You are the independent reviewer", "steps": [{"text": "...\nVERDICT: accept\n- ..."}]}
#   {"when": "self.propose_change { item:", "steps": [
#      {"calls": [{"name": "self_propose_change", "input": {"item": "{item}", "issue": "x", "why": "Open the tree."}}]},
#      {"calls": [{"name": "proc_run", "input": {"command": "<write fix/<slug>.txt and tests/<slug>.rs in $R/state/self/trees/<slug>>"}}]},
#      {"calls": [{"name": "self_propose_change", "input": {"item": "{item}", "issue": "x", "why": "The fix is written."}}]},
#      {"text": "Built."}]}
#   three planting rules: {"when": "plant-N", "calls": [two invalid calls of fs_read / fs_list / fs_write]}
# theseus.toml: [model] api_base = "http://127.0.0.1:9461"; [secrets] anthropic_api_key = "env:FAKE_ANTHROPIC_KEY";
#   [server] state_dir; [discord]/[web]/[index]/[judge] enabled = false; [tools] projects_dir;
#   [policy] enforcement = "notify";
#   [self] mode = "act", repo = "$R/repo", remote = "origin", pipeline_builds = 2, min_free_gb = 1;
#   [profiles.fable] provider = "anthropic", model = "claude-fable-5-1"
$B/theseus-sim fake-model --addr 127.0.0.1:9461 --rules $R/rules.json &
FAKE_ANTHROPIC_KEY=stand-in $B/theseusd --config $R/theseus.toml --socket $R/sock &
$B/theseus --socket $R/sock self resume
for i in 0 1 2; do $B/theseus --socket $R/sock ask "plant-$i"; done
# about 10 minutes later the night files three items; then:
$B/theseus --socket $R/sock self pipeline        # two building, one ready, held: slots
$B/theseus --socket $R/sock ledger -k self.joined  # three, a second or so apart, never overlapping
git -C $R/origin.git log --oneline main          # three merges of self/* branches
$B/theseus --socket $R/sock self halt            # not run live here (a_halt_stops_every_build_at_the_next_tick holds it):
                                                 # a build in flight is stopped and back to ready
$B/theseus --socket $R/sock shutdown
```

It should show what run 3 above shows. On the owner's machine, the same with `pipeline_builds = 1` (the default)
shows one build at a time and `held: slots` for the rest.
