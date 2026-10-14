# CLOUD_REPORT: self-exams (theseus-pw1q.11)

Branch `cloud/20261013-self-exams`, cut from main at `9fa84735` (store format 32; no format bump here).

## Step: self-exams

### What I found

- `theseus-exam` depends on `theseus-core`, so the core cannot use its item type. The self-exam item is therefore its
  own versioned format (`self-exam-v1`, `rsi::exams::Item`) with theseus-exam's **check language** in its `check` lines
  (the exam crate can score them unchanged) and its own four families: `self_correction`, `self_wrong_label`,
  `self_failed_task`, `self_over_budget`.
- There is no self-backlog "nightly pass" on main yet. The pass rides the learning tender's night (`learning/tender.rs`:
  after the report and the learning loop), so it runs after the judge's nightly work and, once self-backlog joins, can be
  ordered after its pass at the same call site. It therefore runs only when `[judge] enabled` is on (the tender is off
  otherwise). `Core::self_exams_nightly` asks `rsi::gate` first and returns at once for `Off` or `Halted`.
- The ledger pages by kind are keyed by the row's **write time**, not `at_unix_ms`; a pass whose `until` is before the
  rows were written misses them. The pass uses `now` after the day's rows exist (true at night); the tests do the same.
- Seam S5 has not joined: there is no `rsi::sealed::put`. `SealedSink` is defined in `rsi/exams.rs` with that shape, and
  `Core::sealed_sink()` returns `None` (`rsi/exams/pass.rs`); the second row to join replaces that body with its
  five-line impl. Until then a sealed item costs no review and is dropped with a count (`self.exam.pass.sealed_dropped`).

### What I changed

Commit `f914582` (the step) and `4826c55` (the surface rule below and a stronger seeded-day test).

- `crates/theseus-core/src/rsi/exams.rs` and `rsi/exams/{harvest,pass,review,shelf,show,tests}.rs`:
  - **Candidates** (`harvest.rs`): `route.corrected` rows and `fact.corrected` notes (a correction note on a remembered
    fact) → `self_correction`; operator `judge.label`s that contradict the judge, on the **train split** (judgment older
    than `local_midnight - [judge] holdout_days`) → `self_wrong_label`; `task.ended` with `failed` or `budget_exhausted`
    → `self_failed_task`; `budget.reached` → `self_over_budget`. Provenance is `ledger:<position>` of each source row.
  - **Scrub** (`exams.rs`): the daemon's scrubber (`tools.scrubber`, secrets), then handle shapes (`@x`, emails,
    `discord:` / `slack:` / `person:` / `name:` / `email:`), then the held people's display names as whole words. Item
    ids are a digest of the **scrubbed** text, so the same mistake is the same item.
  - **Split** (`half`): parity of a keyed SHA-256 of (key, family, id). The key is a 32-byte META record
    `self.exams.key` made once; nothing the writer controls picks the half.
  - **Open half**: `<state>/self/exams/open/<family>/<id>.json` (`Shelf`). Only a counted item is ever written there.
    **Sealed half**: `SealedSink::put(set, family, bytes) -> Result<ItemId>`, set `self-exams-YYYY-MM-DD`.
  - **Review** (`review.rs`): `CoreReviewer`, a forced-tool call (`judge_exam_item`: three booleans and a reason) on
    profile `fable` (`REVIEWER_PROFILE`, until `[self] reviewer_profile` exists), priced with `audit_reserve`, held
    against the day ceiling with `day_hold`, settled at cost. It sees the item and nothing of the session that made the
    mistake. Fair means all three answers true. A reviewer error leaves the item unwritten and uncounted.
  - **Rows** (`fact/rsi_exams.rs`, three new `LedgerKind`s, `SELF_ROWS` gains the two new names): `self.exam.counted`
    `{item, family, sealed, reviewer}`, `self.exam.rejected` `{…, reason}`, and one `self.exam.pass` a night with the
    numbers. A sealed item's row carries its id, family and reviewer only, and a sealed rejection's reason names the
    failed questions without the reviewer's words (they may quote the item).
  - **Cap**: `[self] exams_day_max` (default 10; `config/rsi.rs`, the template's `[self]` block and its test). It
    bounds *reviews* a local day, counted from the day's `counted` + `rejected` rows, so it holds across passes and
    restarts. A sealed item dropped for want of a sink costs no review and does not use the cap.
  - **Proposer rule**: every item records the `"self"`-principal session or task (`Item.proposer`, found through the
    session's execution authority) whose mistake made it. `Shelf::put` / `delete` refuse a `Writer::SelfBranch` (and
    nobody may change or delete an item through the daemon); `Writer::of_principal("self", branch)` maps seam S1's
    principal to it.
- `rpc/rsi.rs`, `protocol` (`SELF_EXAMS`, `SelfExams*` types, `cockpit/src/protocol.gen/`): `self.exams`.
  `show.rs` builds counts by family and half from the `counted` rows, and today's candidates. A surface that reads no
  private text sees ids and verdicts but not an open item's task (`conn.surface.reads_private()`); a sealed item is
  never anything but an id and a number.
- `crates/theseus/src/self_cmd/exams.rs` and one arm in `self_cmd.rs`: `theseus self exams`.
- `learning/tender.rs`: three lines, the call after the night's report.
- `crates/theseus-core/AGENTS.md`: a paragraph under "Self-improvement's spine".

**Paths for the keel guard's watched list** (the proposer rule's paths): `<state>/self/exams/` (the open half, and
`self/exams/open/`), and `crates/theseus-exam/exam/` (the exam crate's data: `exam-v2.toml`). `exams::PROTECTED` holds
both and `exams::touches_exams(path)` is the test self-pr's push should call on every changed path (state-relative or
tree-relative). I did not touch `scripts/keel-guard.py` or `scripts/long-files.txt`.

### How I proved it

- `cargo test -p theseus-core --lib -- exams self_exam a_wrong_label`: 14 pass (11 in `rsi/exams/tests.rs` and
  `rpc/tests_self_exams.rs`, plus the config and template test). The tests:
  - `a_seeded_day_makes_four_candidates_in_the_right_families` (a correction, a wrong label, a failed task, an
    over-budget turn → four candidates, one per family, with the right checks; under a key that splits them both ways
    the open ones are reviewed, shelved and counted, the sealed ones dropped with a count, a second pass finds nothing
    new, and a non-private surface sees no task);
  - `the_split_is_stable_balanced_and_keyed`; `a_sealed_items_bytes_never_reach_the_open_dir_or_a_row`;
    `an_unreviewed_item_does_not_count`; `a_self_branchs_edit_to_an_item_is_refused`; `exams_day_max_caps_the_day` and
    `the_day_cap_bounds_the_reviews`; `mode_off_and_a_thrown_switch_make_nothing` (no row, no file, the store's last
    position unmoved); `a_wrong_label_inside_the_holdout_window_makes_no_candidate`;
    `an_item_never_quotes_a_secret_or_a_name`; `a_closed_gate_stops_the_pass_between_reviews`;
    `a_sealed_item_without_a_store_is_dropped_and_counted`; `an_item_records_the_self_session_whose_mistake_made_it`;
    `self_exams_shows_a_sealed_item_as_a_number_alone`; and a CLI test that a sealed item prints no text.
- **Planted reverts** (each: plant, run its test, see it fail, restore, `touch`); all ten failed as intended
  (`/tmp/plant.log` in the session):
  1. split ignores key and item (always open) → `the_split_is_stable_balanced_and_keyed`;
  2. an unreviewed item counts → `an_unreviewed_item_does_not_count`;
  3. a self branch may write → `a_self_branchs_edit_to_an_item_is_refused`;
  4. the cap never spent → `the_day_cap_bounds_the_reviews`;
  5. the cap ignores the day's earlier reviews → `exams_day_max_caps_the_day`;
  6. no gate at the nightly pass → `mode_off_and_a_thrown_switch_make_nothing`;
  7. a sealed rejection quotes the reviewer → `a_sealed_items_bytes_never_reach_the_open_dir_or_a_row`;
  8. the holdout window is train data → `a_wrong_label_inside_the_holdout_window_makes_no_candidate`;
  9. an item is not scrubbed → `an_item_never_quotes_a_secret_or_a_name`;
  10. a sealed item written to the open dir → `a_sealed_items_bytes_never_reach_the_open_dir_or_a_row`.
- No test runs "under load": nothing here races or times (the pass is sequential and the tests are on in-memory
  reviewers); I did not claim one.
- **FAST**: `target/debug/theseus-sim bench turn --check --runs 5 --burst 0` passed: plain turn 4 frames / 2 syncs,
  tool turn 8, batch 14; the pass adds no frame to a turn. With the mode off the only code on any path is one
  `rsi::gate` read (a cached mode compare) in the nightly tender.
- **Live check on a scratch daemon** (my build, a fresh state dir, `[self] mode = "act"`): the daemon started and
  `theseus --socket … self exams` printed the four families at `open 0 sealed 0`, "Today: no candidates", and the
  state line "the kill switch is halted (never resumed)". It was stopped with `theseus --socket … shutdown`.
- Keel guard (`python3 scripts/keel-guard.py`): `keel: ok … 0 findings acked`.

### Live check for the maintainer

On a scratch state dir with a key (the reviewer calls a real model, so it needs the keys):

```
theseusd --config <scratch.toml> --socket /tmp/x.sock --state-dir /tmp/xstate   # [self] mode = "act", [judge] enabled = true
theseus --socket /tmp/x.sock self resume          # the owner's, from your shell
# correct a route once (`theseus judge correct …` or a message), fail a task, then wait for the night or restart past learning_hour
theseus --socket /tmp/x.sock self exams           # counts by family and half; today's reviews; open tasks only on the CLI
theseus --socket /tmp/x.sock self log             # self.exam.counted / .rejected / .pass rows with what, why, undo
ls /tmp/xstate/self/exams/open/*/                 # only counted open items; grep it for a secret or a sealed id: nothing
```

It should show: with the mode `off`, `self exams` stays at zeros and no `self/` directory appears; with `act` and a
thrown switch, nothing runs; after a resume, one `self.exam.pass` row a night, `sealed_dropped` equal to the sealed
candidates (until `rsi::sealed` joins), and `self.exam.counted` for each open item the reviewer called fair.

### What is left or uncertain

- **Sealed sink**: `Core::sealed_sink()` returns `None`. The join with sealed-holdouts replaces its body with
  `rsi::sealed::put`'s five-line impl of `SealedSink`. Review of sealed items then also costs spend inside the pass.
- **Budget**: the reviewer's spend goes through the day ceiling (`day_hold`, purpose `self-exams`) but not yet through
  `rsi::budget::reserve`/`settle` (self-budget, not joined): `CoreReviewer::review` in `rsi/exams/review.rs` is where
  to call them, and the reviewer is not a turn in a `"self"` session, so self-budget's turn hook will not see it.
- **Reviewer config**: `[self] reviewer_profile` does not exist; `REVIEWER_PROFILE` is `"fable"`. The reviewer is
  "another model" only by configuration; nothing compares it with the model that made the mistake. self-join should
  pick a profile different from the author's.
- **The check task** (`check.rs`) is the claim-only task for a *delegated* task's report. A self-exam item has no task to
  check, so the reviewer is a one-shot call with the same independence property (it sees the item alone) rather than a
  `task.create { check_of }`.
- **Item quality**: the checks are deterministic regexes over a reply (e.g. a recovery vocabulary for a failed task),
  which a reviewer may well reject as gameable; rejection is the intended outcome for weak ones. Sources that carry no
  words to test (a correction with no content words, a note that does not parse) make no candidate.
- The pass runs only when the learning tender runs (`[judge] enabled`). If self-backlog adds its own nightly hook, call
  `self_exams_nightly` after it.
- A failed-task item uses only the `task.ended` row (state, reason, spend); it does not read the task's brief.
- `self.exams` is an owner-ish read: the open task text is shown only to a surface that `reads_private`.
- Docs for the maintainer: `docs/spec` Part III should record `self.exam.*` rows and the proposer rule's paths;
  `docs/status.md` the roadmap row; `docs/design/rsi-plan.md` the sealed-sink seam is as the task defined it.

### Keel findings expected

None. The follow-up commit lowers no assertion, deletes no test, adds no `#[allow]`, and raises no ceiling or cap.

## Defects found in code I was in

- `SelfConfig` derived `Default`, which would have made a new `u32` key default to 0; it now has a hand-written
  `Default` that agrees with the serde default (tested: `none.self_improve.exams_day_max == 10`).

## The gate

Run as `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` with `CARGO_INCREMENTAL=0` (this VM's disk
allowance filled with incremental caches twice).

Final tree (`4826c55`), full gate, `phase times` (seconds): door 0, keel 0, keel tests 0, fmt 11, shape 5, features 1,
clippy 416 (clean, `-D warnings`), cockpit 37, test build 640, reader rule 4, suite 334 **failed here**.

The suite ran 4,351 tests; the failures were:

- **Known, L1 on a root VM (theseus-pv6i)**: 20 in `theseus-sandbox::contract` and its bench `spawn_100`, and 11 in
  `theseusd::sandbox`. Nothing of mine is near them.
- **`theseus-core tests_kept::fs_read_of_a_long_file_is_a_contiguous_window_and_other_tools_keep_both_ends`**: not on the
  known list; I could not explain it. It failed (`numbers(mid)` empty, `tests_kept.rs:343`: the 60 KB `fs_read`'s lines
  were not found in its result) in both full-workspace gate runs and in one full `cargo nextest run -p theseus-core`
  (all three had my tests in the run), and **passed** in a third full core run (the one that instead tripped the known
  `tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn`, theseus-b38m), in a core run with my
  tests filtered out, alone 8 times (also with `TZ=America/Phoenix`, with the workspace features, and under four
  nice-0 busy loops with the test at nice 19). Ran 3 times with `tests_kept` and my two test files together: pass.
  It reads nothing of the self-exam code. I treat it as a load-dependent flake in `fs_read`'s test; the maintainer
  should run it on the owner's machine, and it deserves a look as a finding if it recurs there.
- `theseusd::gone_jobs a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran` failed once in the first
  gate run (6.6 s) while the disk was full (see below), and passed alone afterwards (6.5 s).

The phases after the suite I ran myself (`machine_checks`' tail): `protocol_types` (the TypeScript is staged and
`git diff --quiet -- cockpit/src/protocol.gen` holds), `cargo deny --offline check` (`advisories ok, bans ok,
licenses ok, sources ok`), and the turn bench (`theseus-sim bench turn --check --runs 5 --burst 0`: plain 4 frames and
2 syncs, tool 8, batch 14, all within budget). The lifecycle and jobs benches are skipped under
`THESEUS_GATE_NO_BENCH`, as in a lane's gate. `compiled_under_lock` reported no crates compiled under the lock.

**On this VM**: the writable-disk allowance filled twice from `target/debug/incremental` (16 GB) and from stale
test binaries in `target/debug/deps` (27 GB); I deleted `target/debug/incremental` and older duplicate artifacts inside
the repository's `target/` (nothing outside the repository and `/tmp`), and ran with `CARGO_INCREMENTAL=0`. The first
gate run's suite thus ran with a full disk for its last test binaries; its results were rechecked in the second gate
run on the final tree above. Rustfmt reflowed three files (formatted before the final run).
