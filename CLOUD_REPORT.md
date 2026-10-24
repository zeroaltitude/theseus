# CLOUD_REPORT: p2f-threads (branch cloud/20261024-p2f-threads, base 64309ecb)

## 1. For the maintainer first

- **Store format: 49, unchanged.** Nothing stored was added.
- **Files near 2,500 lines:** none. `thread_route.rs` is 2,408; `google/drive.rs` is 1,558.
- **Signing:** the VM has no allowed-signers file. `git commit -S` ran, but `git log --format=%G?` shows `N`; treat the
  commits as unsigned. The gate's phases were not run as a whole (see section 3).
- **OWNER CALL: the needs-you post says the approve-list clause twice (why + held because).** Not changed (the writer's
  override: the owner reads this post in the DM). For an approve-list hold with no model line, `why` and `held` are the
  same clause, and `render/ask.rs` `card_text` prints both. Text of the post for a write the approve list holds
  (clause "/work/outside is on the approve list"):
  - Before:
    ```
    🔔 **Theseus** wants to **write reef.txt**
    <risk mark> **writes a file** · why: /work/outside is on the approve list
    ✅ Yes … ❎ No …
    -# held because /work/outside is on the approve list
    -# Needs you · expires … · tap, or reply yes or no
    ```
  - After (print the `held because` line only when `held` differs from `why`; the core fields stay as they are, so
    `an_approve_list_card_says_why` keeps its asserts):
    the same post without the `-# held because …` line.
  - Wording of the risk mark and who/what lines is illustrative; the two clause lines are the point.
- **OWNER CALL: a clause ending in "." prints "..".** L4 is also an owner call, not a code change: the `asks:` sentence
  (`needs_you/words.rs`, `Extension | Approval` arm, a request with no `plain`) becomes `Words.why`, and
  `card_text`'s non-plain branch prints `why` in the Discord needs-you post (`render/ask.rs`, the `else` branch).
  So it reaches a Discord message. Text for a clause "the folder is protected.":
  - Before: `Calls from harness asks: the folder is protected..`
  - After (trim one trailing `.` and whitespace from the held sentence before the format adds its own):
    `Calls from harness asks: the folder is protected.`
  - `rule_clause` is not touched in either version. M1 (the `asks:` wording itself) stays the owner's call, out of this row.
- The two tests the brief names for L3 and L4 (`a_needs_you_post_says_the_rule_once`,
  `a_clause_ending_in_a_stop_prints_one`) are therefore not written.

## 2. Per issue and finding

### theseus-hk0dk (commit 1b4359f)
- **L1, done.** Cause: `placed_asked` marked `owed` for any `ConfirmRequested` of a placed session that passed `ours_in`,
  and `ours_in` lets a turn-less event through. Now the mark is made only when the request's `turn_id` is one of
  `self.mine`, and is kept by correlation id (`Placed::owed` is a map correlation id -> session).
  *What the core shows about continuations:* an answer wakes the execution (`confirm` path) and an expiry does too
  (`rpc/confirms.rs` `expire_question`: `k.wake(&a.execution_id, "expired")`, then the driver's `continue_execution`),
  so those keep their mark. A question closed with no answer does not: `rpc/driver.rs:444` closes with `stopped` (a
  `/stop`) and `turn.rs:954` / `toolrun/resume.rs:231` with `superseded` (new input over it); these send a
  `confirm.resolved` with no `option` and not `expired`, and no continuation. `placed_closed` (called from the
  `ConfirmResolved` arm in `events.rs`) drops the mark for exactly those.
  Tests: `a_turnless_question_of_a_placed_session_is_not_owed`, `a_closed_question_leaves_nothing_owed`.
  Planted reverts: `let ours = true;` -> the first fails; `if false && …` in `placed_closed` -> the second fails.
- **L2, done.** `adopt` now sends `session.unwatch` (`Chat::unwatch_session`, in `chat/tasks.rs` beside `watch_session`)
  for the session it evicts from `also`, never the chat's own, and drops the evicted session's owed marks.
  Test: `an_evicted_placed_session_is_unwatched` (9 placements, one unwatch, for the first). Planted revert: dropping
  the `out.extend(…)` -> it fails.
- **L3, L4: OWNER CALLS** (section 1). **M1: OUT** (owner call).
- AGENTS.md: `crates/theseus-tui/AGENTS.md`, the chat-thread paragraph, one sentence added after "(theseus-nde5n).".

### theseus-axno0 (commit e13299c)
- **(1) done.** `apart_by_dir` compared `h.len()` after the retain against `p.hits.len()`, and `p.hits` holds the
  title; it now takes `h.len()` before the retain. Test: `a_title_only_apart_thread_is_not_dropped` (a thread named by
  its title only is not in the list; one whose file hit went still is). Planted revert (`h.len() < p.hits.len()`) ->
  that test fails, `an_apart_thread_keeps_its_named_title` still passes. Note: a thread whose root's last component is in
  the message (`lighthouse`) does lose a Root hit, so the existing title test's thread is rightly listed; my test uses a
  root that shares no word with the title.
- **(2) done.** The `theseus-4qdh8` line of `crates/theseus-core/AGENTS.md` (was one ~371-character line, ~604) is four
  lines of at most 119 characters; checked that the words joined are identical. No other line of that file touched.

### theseus-capcs (commit 61c5071)
- done. `families.v1.json`: the `tasks` `holds` line in all 7 cases now equals the table's. `families.v1.request.json`
  (golden) has no `tasks` line; left alone. No `expect` or message changed.
- Test `the_labelled_families_say_the_tables_lines` (`tests_families.rs`). Planted revert: the old fixture restored
  (via `git stash` of the file) -> the test fails.

### theseus-s48k6 (commit ef5d6eb)
- done, small (about 20 lines, no `Fit` change). `deferral_in` inserted `TASKS` into `extra` before `fit` existed. Now
  `fit` is built first, and `TASKS` is admitted on the Search path only if `want::fits(&fit, &loaded, TASKS)`; the same
  `fits` is the predicate `want::extra` now uses (one function, not two copies). Not admitted: it stays deferred and
  searchable. `PLAN_FAMILY` unchanged.
- Test `an_asked_family_on_tool_search_stays_under_the_ceiling` (`admit_ceiling = 0.01`: `task_create` stays deferred;
  the base config still loads it, and the two existing dispatch tests pass). Planted revert (`&& true` for the `fits`
  call) -> it fails.

### theseus-izyws (commit 9f79cdf)
- done. `drive.read` over 1 MB with no `save_to` now returns Ok from the metadata alone: name, type word, id, size in
  bytes, owner, modified time, and "`save_to` with a workspace path writes it whole." (`Info::aside`). Meta gets
  `"inline": false` and `"bytes"`. `finish(…, i.source())` is kept, so the source rule is the full read's. Nothing is
  downloaded (the test checks the fake's log for `alt`).
- **Paths changed:** (a) a non-Google file over the cap with no `save_to` (no download); (b) after a download, text over
  1 MB; (c) after a download, a body that is not text. (b) and (c) answer with the same helper (with the downloaded
  size). The tool's description is untouched.
- Tests: new `a_large_file_without_save_to_answers_with_its_metadata`; `a_large_binary_needs_save_to` keeps its name and
  its `save_to` and no-download asserts; its two `unwrap_err()` reads (the big file, and the small not-text binary) now
  assert the Ok answer (it names the size or "not text", and `save_to`). Planted revert (restore the refusal) -> both fail.
- Note on the owner field: for a file the account owns, Drive's `owner` reads `you`, so the text says `owner: you`.

## 3. Gate

The full `scripts/gate.sh` was **not run** (the VM's disk allowance is small: 2.8 GB left after the workspace build;
I cleared `target/debug/incremental` twice and built with `CARGO_INCREMENTAL=0`). What was run:
- `cargo nextest run -p theseus-tui`: 381 passed.
- `TZ=America/Phoenix cargo nextest run -p theseus-core -p theseus-judge -p theseus-discord`: 5,055 passed, 25 skipped,
  0 failed (this includes the core golden and the DM golden test).
- `cargo clippy -p theseus-tui --all-targets -- -D warnings`, and `-p theseus-core -p theseus-judge --all-targets`: clean.
  The whole-workspace clippy was not run.
- `cargo fmt --all --check`: clean.
- `cargo deny fetch` not run. Not run: the rest of `machine_checks`, the CLI and daemon tests, the protocol generation
  test (no protocol type changed, so no regeneration).
- Known failures on this VM (L1 sandbox, `tests_output` TZ) did not appear in these crates' runs.

## 4. Keel

`python3 scripts/keel-guard.py --base 64309ecb --head HEAD --quiet`: no output, exit 0.

## 5. DM golden

`git diff --exit-code 64309ecb -- crates/theseus-discord/src/golden/dm_flow.txt`: empty, exit 0. No Discord file was edited.

## 6. Join notes for integ/p2-follow

- AGENTS.md lines touched: `crates/theseus-core/AGENTS.md` the `theseus-4qdh8` line (~604), rewrapped to four lines;
  `crates/theseus-tui/AGENTS.md` one sentence added at the end of the chat-thread paragraph (~161).
- Files this row alone should touch: `crates/theseus-tui/src/chat/{thread_ask,tasks,events,tests_thread_ask}.rs`,
  `crates/theseus-core/src/{thread_route,tests_families,tests_dispatch}.rs`,
  `crates/theseus-core/src/tools_defer/want.rs`, `crates/theseus-core/src/turn/defer_step.rs`,
  `crates/theseus-core/src/google/{drive,tests_drive}.rs`, `crates/theseus-judge/fixtures/eval/families.v1.json`.
  `Cargo.lock` unchanged.

## 7. docs/status.md sentence, follow-ups

- Sentence: "p2f-threads: the TUI chat marks a placed thread's question owed only for its own turn and unwatches an
  evicted placed session; the apart debug line no longer names a thread kept by its title; the asked `tasks` family is
  held to the ceiling on tool search; the labelled families fixture says the table's lines; `drive.read` of a file over
  1 MB answers with its metadata."
- Follow-ups: the two owner calls above (the doubled approve-list clause, and "..") if the owner agrees; the tool's
  description for `drive.read` could say a large file answers with its metadata (not changed: pinned tool hashes).
