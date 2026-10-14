<!-- CLOUD_REPORT.md: written by the cloud session for branch cloud/20261013-standing-capture. The maintainer drops this commit at the merge. -->

# standing-capture (theseus-slvl.5): preferences, facts and corrections captured live

Branch `cloud/20261013-standing-capture`, built on `e9ec29e` (main `0baeed6`, **store format 33**, unchanged here).

## What I found

- Standing holds records and nothing captured them: only `theseus standing add` and `/orders add` made one. The inbound
  point already had a batch (`classify.v1` + `role.v1`) that every private message rides, so the question costs no call.
- Jev answers choices, probabilities and Nouls, never text. So "the words to keep" cannot be a Jev answer: the pack
  says *that* a message is a directive, of what kind and for what scope; the line kept is the owner's own message, folded
  to one line (more than 240 characters: nothing is captured, nothing rewritten).
- Defect found in code I was in, fixed with a test (`doing more, filing less`): `judge.label` could not label a judgment
  until the judge's sink had written its `judge.call` row (up to 2 s), so an undo within seconds of a capture could not
  label it. `judgment_row` now also finds the `standing.captured` row (key `capture_<judgment>`), as it already found a
  notice's row. `tests_capture::a_sure_preference_…` fails without it.
- The correction rule's `owners()` (CLI, web, an owner's DM) is now one function, `capture::owner_alone`, that both use.

## What I changed

Commit `7bc3774` (the step), `6294921` (the sim flag) and `462486f` (the terminal notice), below.

- **`directive.v1`** (`crates/theseus-judge/packs/directive.v1.toml`, a golden request, `Action::Capture`): point
  `inbound`, the same state as classify/role/route (`classify_and_role_build_byte_identical_states` now holds it too).
  Questions: `kind` (none, order, preference, fact, correction; decides) and `scope` (session, place, everywhere,
  unsure). Bands are the owner's: act 0.80, confirm 0.50, on both. Rollback rule: three `wrong capture` labels a day.
  Live in `WIRED` (the owner's "live, not shadow"). Rollback: `theseus packs rollback directive.v1`, or `[standing]
  capture = false`.
- **Asked only for the owner's own words** (`capture::expect`, `rpc/capture.rs`): `turn.submit` notes a message that
  came from the CLI, the web UI or a binding by a person (never a job's `opened_from`, an MCP client's, an unnamed
  surface's, a prompt's, or one with files); the inbound point takes the note, and asks the pack only in a place only the
  owner writes in (`capture::owner_alone`: no place, or an owner's DM) and never for a task. A guild channel bound private
  and every shared place are never heard. `capture_now` checks the place again (a belt).
- **The bands** (`capture::decide`, pure; `capture/act.rs`, the writes, one frame each):
  - ≥ 0.80: kept as a standing record, source `capture:<node>`, scope Jev's when its `scope` answer reached 0.80, else
    where it was said (a Discord place, else the session). A correction is kept as a preference.
  - 0.50 to 0.80: asked (`standing.asked`, an open ask).
  - below 0.50: nothing, except a correction (the pack's `correction` probability from 0.15) is noted
    (`standing.candidate`) and, when a second one in seven days shares its words (`capture::words::same`: most of the
    shorter line's content words, and a third of all), proposed as an ask `why: twice`.
  - **an order is always asked**, at any confidence.
  - **said again**: a message of the same kind as a live record of its scope that shares its words writes
    `standing.repeated {record, node, judgment, week}` and no record; `standing.list` carries the week's count per record
    (`repeats`) and `theseus standing` prints `[said again Nx this week]`.
- **Notice and undo.** The kept record's `standing.captured` fact says `📌 Kept as a standing preference everywhere
  (terminal): short answers, code first` (narrative line to the session). A record's undo is `standing.revoke`, which now
  also labels the judgment `wrong capture` (`learning::labels::WHOLE` gained it; it grades as a `wrong`). Discord-origin
  words get a `standing_card` in the owner's DM (`runtime/capture_card.rs`, `Buttons::Standing`): **Undo** on a kept record;
  **Keep**, **Keep everywhere** (when it would hold narrower) and **Not this** on an ask; presses are `standing.revoke` /
  `standing.answer` as the presser, judged by the place rule. `standing.answer {id, answer: keep|everywhere|not}` (CLI:
  `theseus standing answer ID keep|everywhere|not`; in `client::OPERATORS`).
- **State**: one META key `standing.capture` (candidates, open asks, repeats, which judgment kept which record), rewritten in
  the frame that changed it. No store format change: a new META key and five ledger kinds (`standing.captured`, `.asked`,
  `.answered`, `.candidate`, `.repeated`), each written. Asks lapse after a week; repeats are kept a month.
- `[standing] capture = true` (config/standing.rs, the template, its test). `standing.list` also returns the open `asks`.
- Protocol: `StandingAsk`, `StandingAnswerParams/Result`, `StandingInfo.repeats/captured_by`,
  `StandingListResult.asks`; `cockpit/src/protocol.gen/` regenerated. Docs: the three `AGENTS.md` (core, CLI, Discord).
- The tests that count Jev's calls turn `directive.v1` off (`theseusd/tests/judge.rs`, `stop_nudge.rs`, the sim's judge
  bench); four health pack lists and the pack-shape tests name it; the pack-thresholds test now expects (0.80, 0.50) for it.
- Commit `6294921` adds `theseus-sim fake-jev --choose QUESTION=OPTION:P` (scripted Choice verdicts for a live check).
- Commit `462486f` (after this report's first version, `ce68414`: **drop both `cloud report (not for main)` commits at the merge**) adds **`standing.noticed`**, a notification
  (`Event::StandingNoticed`, `standing::StandingNoticed`) that `standing.captured` and `standing.asked` send to the
  session's watchers; `theseus watch` and the TUI's event lines print it: the `📌 Kept as a standing preference … (terminal)`
  line, then `Undo: theseus standing revoke std_…`, or the ask and `Answer: theseus standing answer ask_… keep|everywhere|not`.
  Test: `tests_capture::the_notice_and_the_ask_reach_the_sessions_watchers`; `standing::noticed_tests`.

## How I proved it

Commands run in this VM (4 cores; `CARGO_PROFILE_DEV_STRIP=debuginfo CARGO_INCREMENTAL=0`, because the 21 GB of debug
test binaries did not fit the disk allowance):

- Pure: `cargo test -p theseus-core --lib capture::` → 13 passed (each band on planted verdicts, an order always asked,
  the twice-in-a-week proposal and its edges, repeats, the scope rule, the one-line words, the owner-alone rule).
- Core, through the protocol server on a CLI surface with the fake Jev: `tests_capture.rs`, 8 passed:
  a sure preference kept (and **one Jev request for the batch plus route's: `jev.connections() == 2`, and the batch
  holds `classify.v1/kind` beside `directive.v1/*`: no call was added**), its rows, undo revokes and labels;
  the unsure ask and each answer (keep / everywhere / not, by id prefix); an order always asked; a correction twice;
  said again counted not duplicated (list `repeats == 2`); a shared place never captured (the pack not asked, and the belt
  alone with a verdict); capture off, a job's and an MCP client's message hear nothing; an answer from a shared place refused.
- Gateway: `tests_gateway/standing.rs` (a real core, the fake Discord): the kept card has Undo, an ask has Keep and Not
  this, the owner's Undo ends the record, another member's press counts for nothing, the owner's Keep writes it.
- Daemon: `theseusd/tests/capture.rs`, 3 passed, a real `theseusd` with the stand-in model and fake Jev: kept, the model's
  next request carries `- std_…: no tables with me` in its system, said again counted, undone and labeled; an order asked
  and its answer; `capture = false` asks Jev nothing.
- Pack: golden request `directive.v1.request.json` (~254 tokens of questions added to the judge's batch, none to the
  model's prompt); the loader rules hold on it.

**Planted reverts** (plant, show the test fail, `git checkout` and `touch`, `git status` clean after each):

| plant | caught by |
|---|---|
| an order kept at the act band (decide's Order arm) | `capture::tests::an_order_is_asked_at_every_confidence_and_never_kept`, `tests_capture::an_order_is_always_asked_never_kept_on_confidence_alone` |
| a shared place's message captured (`owner_alone` always true) | `capture::tests::only_a_place_the_owner_alone_writes_in_is_theirs`, `tests_capture::a_shared_places_message_is_never_captured` |
| an undo that labels nothing | `tests_capture::a_sure_preference_is_kept_with_a_notice_and_an_undo_revokes_it` |
| a repeat that writes a second record | `capture::tests::a_message_that_says_a_live_record_again_is_counted_not_kept`, `tests_capture::said_again_is_counted_and_never_duplicated` |
| a correction made twice never proposed | `capture::tests::a_correction_below_the_band_is_noted_and_proposed_when_said_twice_in_a_week`, `tests_capture::a_correction_made_twice_is_proposed_though_each_was_below_the_band` |
| a job's message noted as the owner's | `tests_capture::capture_off_and_a_jobs_message_hear_nothing` |
| capture off: one of its two gates removed | **nothing** (`expect_capture` and `inbound_step` each check it, and `capture_now` a third time: defence in depth) |
| capture off: both asking gates removed | `tests_capture::capture_off_and_a_jobs_message_hear_nothing` |

**FAST.** Plain turn frames are unchanged: `theseus-sim bench turn` on this build and on the base (`e9ec29e`), same VM:
plain 4 / tool-call 8 / batch 14 frames in both, plain wall p50 60.2 / 57.9 ms (mine) against 58.9 / 58.6 ms (base).
Nothing on a turn's path is added but one mutex push in `turn.submit` and one take at the inbound point; the pack's state
is built and the verdict acted on in the inbound task, off the path; no timer. `theseus-sim bench lifecycle --runs 10
--check` here: **MISSED on `health, after the seed` (p50 4.8 ms against 5 ms; p95 6.8)**, every other row ok; the base
build on the same VM gives the identical miss (p50 4.8, p95 6.7), so it is the VM, not this step.

**Header cost.** None in the model's prompt beyond the records the owner keeps (each a line, about 15 to 25 tokens, in the
second system block of its scope, as a hand-made record is). The judge's batch grows by the pack's two questions, about
254 tokens of input per owner message in a private place, at Jev's price; no added call.

## Live check on this VM (scratch daemon on the stand-in)

`theseusd` on a scratch config (`/tmp/live`), `theseus-sim fake-model`, `theseus-sim fake-jev --choose
directive.v1/kind=preference:0.93 --choose directive.v1/scope=everywhere:0.9`, the real `theseus` CLI:

```
$ theseus ask "no tables with me"          → Noted.
$ theseus standing
std_4c9371a9  preference everywhere 2026-10-14 10:43  capture:msg_01a1…  no tables with me [kept from your words]
$ theseus ask -s SESSION "I said no tables with me"
$ theseus standing
std_4c9371a9  preference everywhere …  no tables with me [kept from your words] [said again 1x this week]
```

Health said `directive.v1: live`. Undo and the label are `theseusd/tests/capture.rs` (`standing.revoke` → one
`judge.label` `wrong capture`). **A gap**: the notice's line reaches the cockpit's narrative and the ledger, but
`theseus watch` and the TUI do not print narrative lines. I found this during the live check above, after the first version of
this report, and added `standing.noticed` (see "What I changed") so the terminal prints the notice and its undo; the live
check above predates it (re-run it with `theseus watch SESSION` open: the `📌` line and `Undo:` line appear).

### The maintainer's live check (the owner's daemon, a week of use)

1. `theseus packs` → `directive.v1: live`; `theseus standing` → asks and records.
2. State a preference once in the terminal ("no tables with me"): `theseus standing` shows it `[kept from your words]`;
   `theseus ledger -n 40 | grep standing` shows `standing.added` and `standing.captured` in one frame.
3. Say a rule ("never push to main"): it is asked, never kept (`theseus standing` lists `ask_…`);
   `theseus standing answer ask_… keep`.
4. Undo one: `theseus standing revoke std_…`, then `theseus judge list --pack directive.v1` and the label row
   (`judge.label`, `wrong capture`). Three in a day roll the pack to shadow (`theseus packs`).
5. In a Discord DM: the same, with the card's buttons.
6. After a week: `theseus ledger -n 2000 | grep -c standing.repeated` and `theseus standing`'s `[said again Nx this week]`
   per record. **A non-zero week is a finding**: the record's wording or its delivery failed. Also `standing.captured` vs
   `.asked` vs `judge.label wrong capture` for the false-capture rate.

### The exam family it feeds (px-exam P3 and P4)

- **Preference family (P3)**: plant "no tables with me" (and a fact, and an order) in a fresh store with distractor history;
  expect `standing.captured` (preference, fact) or `standing.asked` (order) on that node with the pack's judgment; then the
  six contexts read the manifest (`Manifest.standing`) of a new session, after a compaction, on another surface.
- **Correction family (P4)**: plant a correction once (expect `standing.candidate` or `standing.captured`), then the same one
  again within a week (expect `standing.asked why=twice`); score *captured ≥ 90%* = planted items that end as a live record
  or an ask; the **repeat-yourself count** is `standing.repeated` rows per week, which should fall toward 0 by week 4.
  Negative items (a question, a one-off instruction) must produce none of these rows.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run on `7bc3774` and again on the final code commit `462486f`:
door, keel, fmt, shape, features, clippy, cockpit, test build, reader rule: pass (both runs). Suite on `462486f`: 4,504 tests,
4,470 passed, **34 failed** (the same 34 on `7bc3774`, of 4,502):

- 33 are the known L1 failures (20 in theseus-sandbox's contract tests and `spawn_100`, 13 in theseusd's sandbox tests: root,
  no job cgroup, theseus-pv6i);
- `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer` fails here at 21 s and
  **fails identically on the base `e9ec29e`** (checked in a worktree); not this step's, and not in the known list.

Then by hand: `cargo deny --offline check` → advisories, bans, licenses, sources ok; the protocol-types test passes and its
generated files are committed. The benches were not part of the gate (NO_BENCH): see FAST above for the two I ran.
`python3 scripts/keel-guard.py`: `keel: ok … 0 findings acked`.

**Keel findings expected:** none. (The pack-thresholds test and the `act = 0.90` edit in the loader-rules test were rewritten,
not removed: same assertions, per-pack expectation.)

## Design choices the owner should hear about

- **The scope has no "surface".** `StandingScope` is session, place or everywhere, and a CLI or web session has no place. So
  "this place/surface" is a Discord place or, in the terminal, the session; and an unsure scope defaults to where it was said
  (a terminal's session), which is narrow: "no tables with me" said once in the CLI and not judged `everywhere` at 0.80 holds
  only in that session until the owner keeps it wider (Keep everywhere on an ask, or `standing add --scope everywhere`).
  Making the terminal default `everywhere` is a one-line change in `capture::scope_of`, and the north star ("honoured
  everywhere") argues for it; I kept to the brief's "where it was said".
- A correction is kept as a **preference** (a corrected habit), never an order.
- The owner's words are kept verbatim as one line, capped at 240 characters; no model rewrites them.
- Asks lapse after a week (nothing kept). `owner-ask` (theseus-j7cje) had not joined: the ask is today's card (Discord) and
  `theseus standing answer` (CLI); when owner-ask joins, `capture/act.rs::ask` is the one place to route through it.
- The routed bench arm (`bench/theseus-bench-routed.toml`, `[judge]` on) now also asks `directive.v1`; a trial's one message is
  a task, and a record it keeps would only reach that trial's later turns (there are none). I left the file alone;
  `[standing] capture = false` there would keep the arm's Jev cost and prompt exactly as before if the maintainer prefers.

## What is left or uncertain

- The TUI's flash line / ring for a notice is not wired (it prints the lines in its event stream like the CLI); whether that
  is loud enough for a terminal owner is for use to say.
- The cockpit shows the new fields' types (regenerated) but has no panel for asks; the TUI and the cockpit have no ask
  buttons; Discord's `/orders` list does not list open asks.
- Docs to change at review: spec Part III (a new item: the pack, the bands, the notice and undo, `standing.answer`, the
  `standing.*` ledger kinds and META key), `docs/status.md` (standing-capture landed; its roadmap row), `docs/design/north-star-plan.md`
  row `standing-capture`.
- A message the steer path joins to a running turn is captured like any other; a thread's session (no place) is treated
  like a terminal's. Unverified live on the owner's daemon: how Jev's `kind` confidence behaves on real messages (the bands
  are the owner's, set before data; the first week's labels are the data).
- The 2 s sink window means a label made while the `judge.call` row is unwritten is found through the `standing.captured`
  row; the learning loop reads the judgment's own row once it is written.
