# Cloud report: the review's small changes to 28b, L3, 16 and 40 (theseus-ext.12; theseus-mgw.10)

Branch `cloud/20261004-smalls-tools`, from `main` at e27405a (store format unchanged: no stored record gained a
field; the new META marks are new keys). Started 18:55 UTC, done 20:25 UTC.

| Part | Commit | Issue |
|---|---|---|
| 1. categorize.v1 discovers topics, reads after its mark; a Choice reserves its options | e7232b2 | theseus-ext.12 (gky0, q0rn) |
| 2. Language servers: start_on_edit defaults, rust-analyzer's target dir, diagnostics once | b396783 | theseus-ext.12 |
| 3. The restore's policy: `StringLikeIfExists` | ac433e5 | theseus-mgw.10 |
| 4. Runaway-train mode (waits on the owner's last word; its own commit, droppable) | 5a56997 | theseus-ext.12 |

## 1. categorize.v1 (e7232b2)

**Found.** As the brief says: `prepare_categorize` returned `None` when no topic was declared, so the mark never
moved and every exchange end reread the session from its start (gky0); with fewer than ten human messages since the
mark it read `session_nodes`. The pack offers `new_topic` and `none` already (pack.rs builds a Choice from an empty
`topics` source plus the fixed options), so an empty ontology needs no pack change, and none was made.

**Changed.**
- `judge/categorize.rs`: the empty-ontology return is gone; the Choice is then `new_topic` and `none`, the mark moves.
  With no topic declared the input is the human messages after the mark alone (never `session_nodes`); with topics
  declared and under ten since the mark, the input still reaches back for the last ten, as before. The module doc says
  both. The decisions count the session records they read (`JudgeService::categorize_records_read`, a field on the
  point; no edit to judge/mod.rs).
- `theseus-judge/src/price.rs`: `INPUT_PER_OPTION = 10` input tokens per Choice option (`input_allowance`), added in
  `reserve_request` through a new `reserve_tokens`; `reserve_micros(bytes, output)` keeps its meaning. Choice only:
  Score levels are unchanged (no live evidence; say if they should follow).
- `judge.rs`: `part` is `pub(crate)` so the test can build the batched request.

**Proved.**
- `tests_categorize::an_empty_ontology_still_judges_and_reads_only_after_the_mark`: on an empty ontology, ten
  messages dispatch a judgment with `candidates: 0`, Jev offered `new_topic` and `none` and no topic, the mark names
  it; with the mark's message set 31 minutes back, the next message is a `quiet` judgment whose state holds only that
  one message, and the records read are at most those after the mark (fewer than the session's); a later not-due
  exchange end reads only records after the new mark; the `new_topic` proposal is listed, and accepting it with
  `--topic garden --desc …` makes the ontology's first topic.
- `theseus-judge tests::a_choice_reserves_input_for_its_options`: a 52-option and a 4-option `categorize.v1` Choice,
  built so the old formula reserves 82 and 37 µ$ as the live calls did (±1), now reserve **104** and **39** µ$ (pinned),
  at least their billed 96 and 32. No other pinned reservation moved (`every_live_call_fits_inside_its_reservation`
  and price.rs's tests pass unchanged; `tests_learning`'s literal 50 is a row fixture, not a computed figure).
- Planted reverts: the early return on an empty ontology put back → the test fails "0 of 1 judgments recorded";
  `session_nodes` for under ten again (the `!declared` dropped) → "read 40: 18 after the mark, 179 in all";
  `INPUT_PER_OPTION = 0` → "50 topics: 83 < 96". Each restored and touched; `git status` clean of them.
- `tests_categorize` whole: passes (the 70-test run of categorize, lsp, restore and template tests).

## 2. Language servers (b396783)

**Changed.**
- `config/lsp.rs`: `start_on_edit: Option<bool>`. `lsp::START_ON_EDIT = ["rust-analyzer", "ty", "tsgo"]`; `Spec::all`
  takes the config's value or, unset, whether the preset is listed. Other presets and custom servers stay off.
- The template's `[lsp]` example: `[lsp.servers.rust-analyzer] start_on_edit = false` with a comment naming the
  three defaults, and `start_on_edit = true` under pyright; the template test asserts both.
- `theseus-lsp/src/servers.rs`: rust-analyzer's `initialization_options` are `{"cargo": {"targetDir": true}}`, and
  its preset `settings` (what `workspace/configuration` answers from) carry `{"rust-analyzer": {"cargo":
  {"targetDir": true}}}`, since rust-analyzer replaces its initialization options with that answer. An operator's own
  `[lsp.servers.rust-analyzer] settings` replaces the preset's (as for any preset) and must keep `cargo.targetDir`;
  the preset's comment says so. The fake records `initializationOptions` in `fake/seen`.
- `lsp/tools.rs`: `lsp.diagnostics` puts the files it lists in `meta.files`; `lsp/edits.rs`: a done pending wait for
  one of those files is taken without being said; other files' arrivals still ride; a still-running wait is kept.
  Pending waits stay in memory.

**Proved.**
- `tests_lsp_edits::the_three_presets_start_on_an_edit_and_false_stops_it`: edits of `m.rs`, `m.py`, `m.ts` (with
  `Cargo.toml`, `pyproject.toml`, `tsconfig.json`) start rust-analyzer, ty and tsgo (3 spawns); rust-analyzer's fake
  saw `initialization_options == {"cargo": {"targetDir": true}}`; with `start_on_edit = Some(false)` an `.rs` edit
  starts nothing. `config::lsp::tests` checks the defaults and the `false`.
- `tests_lsp_edits::lsp_diagnostics_after_a_pending_edit_lists_each_error_once` (paused clock): two pending edits; an
  `lsp.diagnostics` of `a.fake` lists its error once, and its attach carries only `b.fake`'s arrival.
- Planted reverts: rust-analyzer's options back to `Null` → "left: Null"; the `listed` skip removed → "a is listed
  by the result itself". Restored and touched.
- `theseus-lsp` and every `lsp` test: 111 passed (with durable, restore and template tests).

## 3. The restore's policy (ac433e5)

**Changed.** `aws/durable/read.rs`: `ListItsPrefix` uses `StringLikeIfExists` on `s3:prefix`; the comment says why
and that a list with no prefix now passes (key names in the bucket, never an object's contents outside the prefix).
The tests' fake (`tests_durable.rs`) gives each `AssumeRole` its own access key, keeps the session's inline policy,
and answers a missing `GetObject` key 404 only when that policy grants `s3:ListBucket` with no condition needing
`s3:prefix` (an `…IfExists` operator passes), 403 `AccessDenied` otherwise; the key's own requests stay 404.

**Proved.** `tests_restore::a_missing_blob_and_a_gap_are_said` (one blob's object deleted): the restore says the blob
is missing and restores the rest; the policy test pins the condition. Planted revert: `StringLike` put back → the
restore fails whole: "s3 GetObject: AccessDenied (HTTP 403) … nothing was restored".

**Uncertain, for the owner.** The durability *tender*'s session policy (`durable.rs::policy`) has no
`s3:ListBucket` at all, and it uses `HeadObject` to ask whether an object is already there. Real S3 answers a HEAD of
a missing key 403 to such a principal, not 404. The fake answers HEAD 404 whatever the policy, so the tests cannot
see it. If the tender reads 403 as "not there", it works; if it reads it as a failure, it never ships a first object.
I left it alone (the durability tender is changing in another branch). Worth checking in step 15's live check.

## 4. Runaway-train mode (5a56997, its own commit)

**Changed.** New `aws/hands/runaway.rs` (one `pub mod` line and a doc line in aws/hands/mod.rs):
- `[aws.accounts.<id>] runaway_factor` (default 10.0, at least 2, checked in `validate_aws` through
  `check_runaway_factor`), beside `hourly_alert_usd` in config/aws.rs and the template, with the owner's words. The
  template's `daily_budget_usd` example is now 10.
- The figure is the hour's meter's (reserved by dispatched hands still running, cost of settled ones), computed at
  admission from the group records (`spend_since`; `watch::groups_from` shared with the poller), per clock hour and
  per local day (from local midnight via `wake::local`; a DST day is taken as 24 h).
- `toolrun/hands.rs`: after the session-budget check, `Sink::admit(account, n × reserve, now)`. A group is refused
  when figure + its own worst case ≥ factor × line. That refusal **enters runaway mode** until the period turns: META
  `aws.runaway.<account>` and an `aws.runaway` row (new `LedgerKind::AwsRunaway`) in one frame, announced, and one
  notice where approvals go. While the mark holds, every new reserving action is refused. The poller's pass
  (`watch::refresh` → `runaway::into_health`) also enters it when the figure alone is at a line, and fills health.
- The refusal names the account, the period's figure (with the call's worst case), `runaway_factor`, the line's key
  and dollars, when it ends (local HH:MM), and the keys to raise and the restart.
- Never refused: cancels and `/stop`, lists, status reads, the reaper, settling, and an admitted group's later waves
  (those are dispatched by `group::step`, which has no check).
- `AwsHandsStatus` gains `runaway` (words) and `runaway_until_unix_ms`; protocol.gen regenerated; the CLI's health
  prints `aws: <id> RUNAWAY: …`. The cockpit's SystemsCards does not show it yet.

**Design choices to hear about.**
- A group refused because its own worst case alone reaches factor × line latches the mode for the rest of the
  period, so smaller groups are then refused too. That follows "refused when its own reservation would add to the
  runaway figure" and the live check's "one small group's reservation passes their product". If the owner prefers
  no latch for a refusal that was only prospective, it is a two-line change in `Sink::admit`.
- Two groups admitted at the same instant can both pass (the check is not inside the group's frame). Best effort, as
  the meter itself.
- `daily_budget_usd` is whole dollars (u32), so the day's runaway line is at least factor × $1.

**Proved** (`aws/hands/tests_part2.rs`):
- `runaway_mode_refuses_at_ten_times_the_hours_line`: a two-hand group at 9.9 × the line runs; the next (19.8 ×) is
  refused with its words, one `aws.runaway` row (`line: hour`, `factor: 10`), one notice; a third is refused with no
  second row or notice; health's line shows it after a poller pass; the running group's hands complete and the group
  settles Succeeded; at the next hour (a later clock passed to `admit`) a group is admitted.
- `ten_times_refuses_and_a_cancel_still_runs`: at exactly 10 × the first group is refused and nothing launches; in
  runaway mode a `/stop` of a running group's execution runs and settles its call.
- `the_days_line_trips_runaway_mode_the_same_way`: factor 2, $1 a day: 1.99 × admitted, 2 × refused (`today`,
  `daily_budget_usd`), one `line: day` row, then any reserving action refused; the next day admits; nothing reserved is
  never refused; `runaway_factor = 1.5` fails validation, 10 is the default.
- Planted revert: the `admit` call skipped → both through-the-turn tests fail ("Started a group of 2 hands…" where
  the refusal was expected).
- Under load (4 busy loops at nice 0, the tests at nice 19), three runs of every `tests_part2` and `tests_hands` test
  plus the part 1 and 2 tests: 30 of 30 passed each run (51 s, 48 s, 50 s).

## The maintainer's live checks (scratch daemon, Discord and the web off)

1. **categorize**: `[judge] enabled = true`, `jev_api_key` and a model key, no topic. `theseus ask` ten messages about
   one subject in one session. `theseus judge log` shows `categorize.v1` with `candidates: 0` answering `new_topic`;
   `theseus ontology proposals` lists it; `theseus ontology accept <jdg> --topic garden --desc "…"` makes the topic.
   Add 49 topics, another ten-message session: `theseus ledger -k judge.call --json` shows
   `reserve_micros >= cost_micros` (expect about 104 reserved against about 96).
2. **LSP**: `[lsp] enabled = true`, a scratch cargo crate, rust-analyzer on PATH. Ask for an `fs.edit` that breaks a
   function: rust-analyzer starts on the edit (no config line needed), its error is pending then arrives, and
   `target/rust-analyzer` appears; meanwhile `cargo build` in the crate never prints "Blocking waiting for file lock
   on build directory". `lsp.diagnostics` on the file right after lists the error once, and its result has no
   "Diagnostics that arrived" block for that file.
3. **Restore** (under a cent, after the owner's go): step 16's live check with `theseus ask --attach <image>` so a
   blob ships; delete one object under the probe's `blobs/` with the owner's credentials; restore: it says that blob
   is missing and restores the rest; CloudTrail shows that `GetObject` as NoSuchKey, not AccessDenied. Also note what
   the tender's `HeadObject` gets for a missing key (see part 3's uncertainty).
4. **Runaway** (a few cents, an account set up for hands): set `hourly_alert_usd` and `runaway_factor` so one small
   group's worst case passes their product (e.g. `hourly_alert_usd = 0.002`, `runaway_factor = 2` for a two-hand
   Lambda group of about 4 cents). `aws.hands.run` is refused with the words, one `aws.runaway` row
   (`theseus ledger -k aws.runaway`), one notice, and `theseus health` shows the RUNAWAY line; raise the line, restart,
   and the group runs.

## Docs the maintainer should change (I edited none)

- crates/theseus-core/AGENTS.md: the categorize bullet ("no topic, the point still judges; reads after the mark");
  the hands bullet (`watch.rs` "alert only" → plus `runaway.rs`, the one refusal); the language-server bullet
  (start_on_edit defaults).
- crates/theseus-lsp/AGENTS.md, Traps: rust-analyzer's `cargo.targetDir` in both initialization options and settings.
- docs/design/m5-judgment.md §2.12 (categorize on an empty ontology; the reservation's per-option input);
  docs/design/aws-toolset.md §3.7 (runaway mode) and §3.5/step 16 (the `IfExists` list); Part III items; status.md.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the final tree (5a56997): fmt, shape, features, clippy, cockpit, test
build and the reader rule pass; the suite ran **2327 tests: 2293 passed, 34 failed, 17 skipped**. The 34:
- 33 sandbox tests (`theseus-sandbox::contract` and `::bench`, `theseusd::sandbox`): this VM runs as root (theseus-pv6i).
- `theseus-core tests_output::the_cores_output_matches_its_golden`: the golden holds a wake's local offset `-#:#`;
  this VM is UTC (`+#:#`). With `TZ=America/Los_Angeles` it passes. Not one of the brief's listed cases, but
  environmental (the only difference is the offset sign).

After the suite: protocol types clean (protocol.gen committed), `cargo deny --offline check` ok (advisories, bans,
licences, sources). No new dependency; Cargo.lock and package-lock.json unchanged. Benches not run (NO_BENCH).
The gate's earlier run on parts 1–3 alone (ac433e5) had the same 34 failures and nothing else. Parts 1–3 were each
committed after their targeted tests, clippy and fmt; the full gate ran once for parts 1–3 together, then for part 4.
