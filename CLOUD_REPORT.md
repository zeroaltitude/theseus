# CLOUD_REPORT: mint-validate (theseus-pw1q.21.6, R7)

Branch `cloud/20261017-mint-validate`, built on `b95a2558` (main as the task describes it; **this clone's own `main` and
`origin/main` are at `96c592c0`, older**, see "The gate"). Code commit: `e8950ee7`. This file is the branch's last commit.
No store format change, no new config key, no new dependency (`Cargo.lock` and `cockpit/package-lock.json` untouched).

## One step: from a mined motif to a checked tool

### What I found

- The two modules the row stands on exist and were enough: R5's `Motif`, `Occurrence`, `SlotKind` and `Motif::spec`, R6's
  `flow::spec`, `flow::class`, `Board::freeze`. I added **no** accessor to either; one existing method was widened
  (`Core::mine_run`, `fn` to `pub(crate)`, in `rsi/mine/night.rs`).
- **R5's skeleton is not a valid R6 flow as it stands.** `Motif::spec()` carries `optional`, arm classes like
  `err:1+conflict+rerere-resolved`, positional `arg0..` for non-program tools and `$?` bindings; R6's parse refuses all of
  these. `mint/spec.rs` is the translation (it maps by R6's own types, so the digest and `Spec::parse` round-trip hold).
- **R5 reads `rerere-left` and `rerere-resolved` off a *merge's* text; R6's runtime produces those classes only from `git
  rerere status`** (`flow/class.rs` says the miner and the mint share one reading, but the miner kept its own,
  `shape::features`). A flow made straight from the skeleton sends every conflict to `step-failed` (the first version of my
  fixture tier showed it on the owner's example). Fix in this row: where arms differ by rerere, the spec gets a *synthetic*
  `git rerere status` step after the branching step (the shape §2.5's own example has), and the recorded side answers it
  from the merge's text (`mint/source.rs`, `Recording`). **Worth aligning in R5 or R6 later.**
- A flow's engine (`flow/run.rs`) is welded to `ToolRuntime`. The dry replay needs the same transitions with no model and
  no gate, so `validate/walk.rs` is a second copy of the walk over any runner of steps (`Steps`), held to the same cases
  by `tests_walk`. A later refactor should make `flow::run` generic over `Steps` and delete one copy.

### What I changed (all in `e8950ee7`)

| Part | Where |
|---|---|
| spec: motif to flow spec, placeholder name, constants kept only past the shape check and the scrubber | `rsi/mint/spec.rs` |
| kind rule (flow, owner card, Rust, MCP), `[self.mint] kinds` | `rsi/mint/kind.rs` |
| keyed split (META `self.mint.key`), the brief from the build half alone, check half on disk, `isolated` | `rsi/mint/brief.rs` |
| data rule by profile **origin** (`Config::profile_origin`), failing closed | `rsi/mint/mod.rs` (`data_rule`) |
| `ValidateReport` (public, documented), tiers, `assemble` | `rsi/mint/validate.rs` |
| a flow's engine over any steps | `rsi/mint/validate/walk.rs` |
| fixture tier: one fixture per arm, `GitFixtures` (clean, known and unknown conflict, red tests) | `validate/fixtures.rs` |
| dry replay: 3 instances, 90%, 0 deviations, remote steps stubbed | `validate/dry.rs` |
| A/B replay: pass rules, reservation per arm on `[self.budget]`, `[self.mint] usd` | `validate/ab.rs` |
| the daemon's side: writer turn (`MINT {json}`), pending record, validation, row | `rsi/mint/run.rs`, `source.rs` |
| arm B: `Minted` flow listed in the replayed request and run through the same hands; `Steps for Hands` | `own_bench/whole/{hands,runloop}.rs`, `own_bench/replay.rs` (`replay_turn_with`) |
| the mint start beside the fix start, own cap; item settle; tender spawns `mint_run` | `rsi/backlog/{start,tender}.rs`, `backlog.rs` (`Started.mint`, one call) |
| `self.mint.validated` | `fact/rsi_mint.rs`, `LedgerKind::SelfMintValidated`, a `SELF_ROWS` entry (`theseus-protocol/src/{ledger,rsi}.rs`, not `lib.rs`) |
| AGENTS.md paragraph | `crates/theseus-core/AGENTS.md` |

Edits outside the row's files, each small: `mine_run` visibility; `rpc/tests_self_patterns.rs` (helpers `pub(super)`, and
`core_with` sets `[self.mint] per_night = 0`: with the mint start on, R5's test "a pattern starts nothing" would start one
by design, so its helper keeps its premise: the miner's night alone starts nothing; no assertion was touched);
`own_bench/whole/tests.rs` (helpers visible to `tests_minted.rs`, module `pub(crate)`).

### How the flow works end to end

1. Night: R5 files the pattern item. `backlog_mint_start` (mine on, mode `act`, gate allowed) re-reads the motifs in memory
   (`mine_run(now, false)`), takes the top proposed items up to `per_night`, skips a motif whose
   `<state>/self/mint/<placeholder>/pending.json` exists, applies the kind rule (an owner card marks the item
   `waiting_human`, evidence `owner-card:…`), the data rule on the live profile, then `mint_prepare`: split, write
   `check/occurrences.json`, `brief.md`, `pending.json`; opens a `"self"` session whose objective is the brief.
2. The writer's reply ends with `MINT {"name", "description", "types"?}` (no new model tool). `mint_validate` builds the
   spec (scrubbed as a whole: a secret in the description refuses it), freezes it (`Board::freeze`), and runs the tiers.
3. Fixtures and the dry replay run through `Hands` (L1 for programs). The model tier runs **only if both pass and the
   isolation holds**, so a failing flow costs no model money. One `self.mint.validated` row; the item closes `done`,
   `failed`, or (tool never called) back to `proposed` with the evidence.

### How I proved it

Commands (this VM, `TZ=America/Phoenix`):

- `cargo nextest run -p theseus-core -E 'test(rsi::mint) or test(tests_self_mint) or test(whole::tests_minted) or test(tests_self_patterns)'`
  → **57 passed** (mint 35, rpc 10, whole 6, R5's patterns 6 beside). By file: `rsi/mint/tests.rs` 9, `validate/tests_ab.rs`
  10, `tests_dry.rs` 8, `tests_walk.rs` 8, `rpc/tests_self_mint.rs` 10, `own_bench/whole/tests_minted.rs` 6.
- `cargo clippy -p theseus-core --all-targets` clean (functions I wrote fit clippy's 100-line limit; no `#[allow]`).
- the full gate, below: 6,710 of 6,743 pass; the 33 failures are exactly the L1 set the task names.

What the tests hold (each of the task's list):

- *The brief holds no check-half id*: `tests::the_brief_holds_no_check_half_id` (bytes against every check id and turn id;
  a brief made from all occurrences fails `isolated`), and through the core
  `a_motif_starts_a_writer_session_whose_brief_holds_no_check_half_id` (the id sets read back from disk).
- *The dry replay reproduces recorded facts on fixtures, and a deviation fails it*: `tests_dry` records the flow's own run
  in a real temp git repository with an upstream (clean, rerere-known, rerere-unknown, red), then replays it:
  `the_dry_replay_reproduces_recorded_facts`, `a_deviation_an_extra_write_fails_it`, a step the recording never ran, an
  exit it never reached, under 90%, fewer than 3, a remote step run with its recorded result. Through the core:
  `the_check_half_replays_dry_from_recorded_sessions` reads six seeded sessions, rebuilds each directory, binds the inputs
  and replays through `Hands` with a stand-in for L1: 3 of 3 reproduce.
- *A/B with the stand-in model, pass and fail*: `tests_ab` (pass; each rule failing alone; never called, with evidence for
  the backlog). Through the core with the scripted model: arm B lists the flow and its step runs on the copy
  (`arm_b_lists_the_minted_flow_and_runs_its_steps_on_the_copy`), arm A lists none and refuses a call to it, a model that
  never calls it sends the motif back, and both arms are reserved and settled on the self budget.
- *Reservations*: `every_arm_reserves_on_the_self_budget_first_and_the_day_line_stops_it`, the `[self.mint] usd` cap, the
  mode off, and through the core `the_day_line_stops_the_replay_before_any_arm_runs` (no request is sent).
- *The data rule*: `a_private_motif_refuses_a_china_origin_profile` (a Claude model id on a China-origin endpoint, whose
  name says nothing; the built-in GLM; an unknown profile; bench-born allowed) and
  `a_private_motif_is_not_started_on_a_china_origin_live_profile` (nothing starts, no brief, no check half, no session).
- *Kind table and owner card*: `the_kind_rule_in_its_order`, and through the core `a_motif_that_would_need_the_network_waits_for_the_owner`.
- *Off*: `off_starts_nothing_and_writes_no_mint_state` (mine false, mode off, switch halted) and
  `turning_mine_off_stops_the_mint_start_of_items_already_filed` (items on the backlog, `mine` off: none start).
- *Secrets*: `a_secret_in_the_writers_description_is_never_frozen_or_recorded` (planted key; nothing frozen, no row, no
  stored byte holds it); a scrubber-changed constant becomes an input and is kept nowhere.

Planted reverts (file edited, test run, file restored and `touch`ed, `git status` clean after; script and log kept out of
the repo). Every plant failed the test named:

| Plant | Failed |
|---|---|
| brief assembled from all occurrences, isolation bail off (`run.rs`) | `tests_self_mint::a_motif_starts_a_writer_session_whose_brief_holds_no_check_half_id` |
| dry replay ignores writes outside the recorded set (`dry.rs`) | `tests_dry::a_deviation_an_extra_write_fails_it` |
| decision count dropped from the A/B rule (`ab.rs`) | `tests_ab::too_few_decisions_removed_fails_even_when_the_tokens_fall` |
| reserve skipped (reserves $0) (`ab.rs`) | `tests_ab::every_arm_reserves_…day_line_stops_it`, `whole::tests_minted::the_day_line_stops_the_replay_before_any_arm_runs` |
| data rule checks the model name, not the origin (`mod.rs`) | `tests::a_private_motif_refuses_a_china_origin_profile` |
| network/secret is no owner card (`kind.rs`) | `tests::the_kind_rule_in_its_order`, `tests_self_mint::a_motif_that_would_need_the_network_waits_for_the_owner` |
| a missing fixture counts as a pass (`fixtures.rs`) | `tests_walk::an_arm_no_fixture_builds_fails_the_tier_by_name` |
| spec not scrubbed (`run.rs`) | `tests_self_mint::a_secret_in_the_writers_description_is_never_frozen_or_recorded` |
| 90% to 50% (`dry.rs`) | `tests_dry::under_ninety_percent_reproducing_fails_without_any_deviation` |
| 3 instances to 1 (`dry.rs`) | `tests_dry::fewer_than_three_instances_cannot_pass` |
| `mine` ignored at the start (`start.rs`) | `tests_self_mint::turning_mine_off_stops_the_mint_start_of_items_already_filed` |
| arm B does not list the tool (`runloop.rs`) | `whole::tests_minted::arm_b_lists_the_minted_flow_and_runs_its_steps_on_the_copy` |
| model tier runs after a failed tier (`run.rs`) | `tests_self_mint::a_validation_runs_its_tiers_writes_one_row_and_loads_nothing` |
| never-called not sent back (`ab.rs`) | `tests_ab::a_tool_the_model_never_calls_goes_back_to_the_backlog_with_that_evidence` |

No load runs were asked of this row. The tests that touch time or L1 use a stand-in for L1 (`own_bench::whole`'s
`TEST_PROGRAMS`, as the existing whole-turn tests do) because L1 refuses a root daemon here; **none of my tests ran a program in real L1**.

### The live check for the maintainer

On the owner's machine (non-root, with a job cgroup), a scratch daemon on a copy of the store, never the operator's:

1. Config: `[self] mode = "act"`, `[self.mint] mine = true`, `kinds = ["flow"]`, `per_night = 1`, `replay_turns = 5`.
   `theseus self resume`, then `theseus self patterns --top 5` should list the owner's land motif.
2. Start the backlog night (the tender's hour, or `theseus self backlog` after one) and `theseus self backlog`: one pattern
   item `in_progress`, evidence ending `mint:mint_<12 hex>`.
3. `ls <state>/self/mint/mint_<hex>/` shows `brief.md`, `pending.json`, `check/occurrences.json`. Isolation by hand, expect
   **no output**: `jq -r '.[].id' <dir>/check/occurrences.json | while read i; do grep -F "$i" <dir>/brief.md; done`.
4. When the writer's turn ends (its reply ends `MINT {…}`): `theseus self log` shows a `self.mint.validated` row with four
   tiers; with real L1 the fixtures tier should pass on the land flow (3 of 3 arms), the dry replay's verdict depends on the
   real sessions. `<state>/self/mint/<name>/<digest>/spec.json` is frozen read-only and no `flow.loaded` row exists: nothing is loaded.
5. With `mine = false`, or `theseus self halt`, a later night starts nothing and writes no `self/mint` entry.
6. The one path I could not run is a program in real L1 (fixtures, dry replay and arm B all use `Hands`, which runs
   `proc.run` through `InL1`; here a stand-in answers). To see it, `theseus model replay <session> <turn> --scope whole_turn`
   on a session of a land flow with a scratch daemon, and confirm its `proc.run` calls say `ran`; the mint's fixtures
   tier then runs the same way.

### What is left, and design choices the owner should hear about

- **Only flows are validated in this row.** For MCP and Rust, the start writes the brief and the check half and opens the
  writer's session (the rule and `kinds` apply), but `mint_validate` refuses a non-flow kind: their fixtures are the
  extension's declared tests or the branch's gate, and their dry replay needs the extension called through the board, all
  of which is the deploy step's (R8). `[self.mint] kinds` defaults to `["flow"]`, so nothing else starts by default.
- **A motif is minted once.** A motif whose `pending.json` exists is not started again, so a failed validation (or a tool
  the model never called, which goes back to `proposed` with the evidence) is not retried by the night. This keeps a failing
  motif from spending a night's money every night; the owner can delete the pending directory to try again.
- **The dry replay cannot see a program's writes.** A recording's changed paths are the files its edit tools wrote; a merge's
  files are in no record, so an instance whose recording ran programs carries no path set, and `DryReport.unverified` counts
  it. For a repository flow, which is all programs, "0 deviations" therefore means: no step the recording never ran, no exit
  it never reached, exits and test results reproduced; paths are proved only by the fixtures (which snapshot the tree). The
  rebuilt directory is the session's directory now with the recorded edits taken back, which a program's own changes (a
  moved HEAD) may have moved: the whole-turn replay's own caveat.
- **Test runners run for real in the dry replay**, in L1 over the copy, with no network; a `cargo nextest` there may fail where
  the recording passed. Stubbing test runners with their recorded results, as remote steps are, would be a decision for
  the owner; I did not.
- **A/B arm granularity.** Each arm is one `[self.budget]` reservation at the whole-turn cap (`[route_bench] whole_usd`,
  trimmed to what `[self.mint] usd` has left) and is settled to the arm's cost; the calls inside an arm are reserved by the
  existing whole-turn loop on the day ceiling, as before, and not individually on the self budget. The arms use the live
  profile, `compare.v1` is off, and each arm writes the usual `turn.replayed` row (by `mint`).
- **The check dir is named by the placeholder** (the motif's digest), not the writer's final name, since the split happens
  before the writer names the tool; the frozen spec is under the final name as R6's `freeze` has it.
- `Steps for Hands` makes a flow step's result text the program's output without the hands' `$ argv` and `[exit N]` lines.
  Arm B's flow result is the flow's own text or a hand-back JSON with `step` and `output_tail` only (not
  `conflict_files`), a smaller state than the real `flow.run` gives.
- Docs that should change at review (I did not touch the spec, status, README or design): the spec's Part III item for
  theseus-pw1q.21.6 and its version line; `docs/status.md`; `docs/design/` roadmap-v2's R7 row; and the R5/R6 note above
  (the class mismatch). I did add one paragraph to `crates/theseus-core/AGENTS.md`.

## The gate

`THESEUS_KEEL_BASE=b95a2558 CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`:

- door, keel (0 findings), keel tests, fmt, shape, features, clippy `-D warnings`, clippy-probe, cockpit, test build,
  reader rule: **ok**.
- suite: **6,710 passed, 33 failed, 54 skipped**. The 33 are the known L1 set (the VM runs as root; theseus-pv6i): 20 in
  `theseus-sandbox` (`contract` clauses 01 to 12, `egress_18b_*` x3, `sigterm_is_forwarded_to_the_command`,
  `scratch_is_reported_and_discarded`, `exit_status_and_signals`, `a_job_that_cannot_start_says_why`, and `bench spawn_100`) and
  13 in `theseusd::sandbox` (the cancel, deadline, grant, no-list, listed-host, probe-script, AWS, health, `l1_argv`, jobs bench
  row, unlisted-and-private, and the two 30 s tunnel tests). Nothing else failed. The two flaky-list cases the task names
  (`a_filtered_page_equals_the_scans_answer`, `python3s_repl_computes_on_the_screen`) passed.
- Phases after the suite, run by hand as the task says: `protocol types` clean (no `cockpit/src/protocol.gen` change);
  `theseus-sim bench turn --check --runs 5 --burst 0` all rows ok (frames_tool 8 of 8, frames_batch 14 of 14, first-request tokens cli 7,332 of 8,000,
  dm 9,411 of 15,000); lifecycle and jobs benches and the size step skipped (`THESEUS_GATE_NO_BENCH`, no release build);
  `cargo deny`: **bans, licenses, sources ok; advisories FAILED** on three existing advisories for the `zip` crate in
  `Cargo.lock` (RUSTSEC-2026-0367, -0368, -0369) from the advisory database this VM fetched; `Cargo.lock` is untouched.
- **The keel guard**, the task's command: `python3 scripts/keel-guard.py --base b95a2558 --head HEAD --quiet` prints nothing
  (exit 0). With `--base origin/main` it prints 14 findings that are not this row's: **this clone's `origin/main` and `main`
  are at `96c592c0`, before wave A's joins** (`499fbb6a`, `6e3bee8695`, `e36570c810`, `746cd908a6` and a few renamed tests
  in other crates), so the guard sees their owner-acked keel changes (the acks' signatures do not verify here). No `Keel:`
  trailer is needed for this row. I once added `#[allow(async_fn_in_trait)]` on two traits, the guard flagged it
  (`allow-added`), and I removed it (the traits return `impl Future`).
- This VM's disk allowance (about 39 GB, spent) stopped two gate runs in `clippy` and `test build` with ENOSPC; I removed
  `target/debug/incremental` and the first build's stale artifacts, and ran with `CARGO_INCREMENTAL=0`. Nothing outside the
  repository's `target/` was deleted.
