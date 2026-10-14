# CLOUD_REPORT: owner-ask (theseus-j7cje)

Branch `cloud/20261013-owner-ask`, on the cloud-task commit 6f287c4 (main with `work-board` and `work-answer` joined, store format 33).

## What I found

- `QuestionKind::Ask`, `Question.choices`, `answer_options` and `work.answer`'s "not answerable yet" arm were in place, but nothing asked
  one: no tool, no expiry of its own, no way to carry a chosen option back to a call.
- The push's board is seeded by the first client that watches (`Push::ensure`). The case that matters most for the always-on
  channel is the one with no client at all, so the escalation reads the store's truth (`Kernel::pending_confirms`), not
  `WorkBoard::open_questions()` as the brief suggested.
- `task_graph::tools::authority_for` cleared `Plan::authority` for every tool that is not a layer-1 task edit. `owner_ask` rides on that
  authority to ask at every posture, so it had to be exempted (the first task test showed it running at once under `notify`; the
  `tests_m3` rigs use `enforcement = approve`, which hid it, so the task test is what guards it).
- Each tool added to the tools block moves `tests_route_keep` and `tests_tiering`'s windows (the established "N tokens more" widening);
  `owner_ask` took 300 and 500 tokens.

## What I changed (commits)

1. `50ec05f` ask: `owner_ask` and the always-on channel (the whole feature; one commit because the pieces compile and test together).
   - **Tool** (`core/ask.rs`, pure parts in `protocol/src/work/ask.rs`): `question` ≤ 500 chars, 2 to 6 options (`label` ≤ 60, `detail?`),
     `default?` (an option's number from **1**), `expires_in?` (**minutes**, default 30, at most 1,440). A harness tool, private places only.
     One sentence in its description, one in the task brief (`task::brief_text`).
   - **Answer**: `work.answer` takes `2`, `choice:1`, or `other` with a note (`rpc/answer.rs::answer_ask`). It binds the confirm in one frame
     with the answer's row and a META record `ask.answer.<id>` (no store format); the continuation "runs" the call, which only reads that back:
     `The owner chose 2, "Ship it behind the flag"; note: …`. A plain `action.confirm` yes on an ask is refused (it chooses nothing).
     `confirm_action_with` gained a sixth parameter for the record (5 call sites; the `#[expect]` stays on the same function).
   - **Expiry**: `ask::expiry_of` (plan time plus the question's own life) feeds the existing tick (`expire_questions`, `question_due`);
     `expire_ask` writes the same record with `expired`, and the result says the default applied or that no one answered.
   - **Always-on channel** (`core/needs_you.rs`, `[notify]` in `config/notify.rs`): the driver's tick posts one `needs_you` outbox post to the
     owner's DM after `escalate_after_secs` (60) with no screen (`interactive_watchers() == 0`), `escalate_with_screen_secs` (300) with one.
     It is indexed as the question's card, so the answer's `card_closed` (and the heartbeat's `reconcile_cards`) settle it by editing it, no ping.
     A question with a card in a Discord place gets no second post. No DM bound: nothing is written.
   - **Half-way reminder** (the policy's rule 2): `WorkBoard::remind` from `Push::remind_questions` on the same tick, reading nothing until `remind_at`.
   - **Surfaces**: `theseus answer <id> 2` (the daemon reads a bare number); the TUI card lists the options and keys 1 to 6 answer (`y`/`n` say
     "press a number"); Discord: an ask card's options are buttons (`ask:<n>:<id>`, `Buttons::Ask`, new arms in child modules because
     `render.rs`/`runtime.rs` are at their ceilings), pressed as `work.answer`, and the needs-you post carries the same buttons.
   - **`task.create { notify_done }`**: stored on `TaskOf` (**store format 33 → 34**, the old layout's sample and a store test are in
     `tests_layouts`); the task's work view is `flagged`, and the policy's report row (rule 8) interrupts instead of informing.
2. `b53fd88` cli: `theseus confirm` and the event stream draw an ask with its numbered options and `theseus answer <id> <number>`
   (before, it printed approve/decline commands that cannot answer it); the Discord needs-you text leads with who needs the owner.
3. (third commit) pacer: the owner's decision 4 (one pacing rule for unprompted posts) is built here, as the first row that needs it:
   `core/pacer.rs` (`Pacer`, `UNPROMPTED`), kept by the outbox (`Outbox::pacer`, seeded from the posts at a start). A needs-you goes out at most
   once per `[notify] unprompted_pace_secs` (1,800; 0 turns it off); questions due while the window is closed wait and go in **one** batch
   post (no buttons, no settle: a nudge to open Theseus). Later rows add their kind to `UNPROMPTED` and ask `Pacer::open`.
4. `cloud report (not for main)` (this file).

## How I proved it

Tests added (all pass): core `tests_m3/ask.rs` (answer by number with a note; "something else" needs a note; a plain yes refused; 2 and 6 options
accepted, 1 and 7 refused; expiry with and without a default, and not early; the 30-minute default life; the place rule (MCP refused); 60 s
with no screen and settle on answer; 300 s with one; answered-in-window posts nothing; a Discord-place question (carded) gets no second post;
no DM, no post; an approval pings too and once; the half-way reminder once; the pacer holds, batches, and `0` turns it off), core `tests_ask.rs`
(a **task** asks, the CLI answers by number, it resumes with the option, and `notify_done` makes its report interrupt; unflagged informs),
`tests_layouts` (format 33 task session sample), `tests_explain` (the tool joined the table), the Discord crate's `tests_gateway/ask.rs`
(a CLI question reaches ana's DM pinging with one button per option, a press answers it and the post is edited; a question asked in the DM has
its card and no second post), `runtime/ask.rs` (button ids), the TUI's `tests_answer.rs` (the keys), the protocol's `work/ask.rs` (limits, ids,
words) and `notices` row 8 (flagged), the CLI's `render/ask.rs`, `config/notify.rs`, `pacer.rs`, and theseusd's `tests/ask.rs`
(a real daemon and the real `theseus answer <id> 2`: a task asks, is answered, and its result reads "The owner chose 2").

Planted reverts (file restored and `touch`ed after each; `git status` clean of the plants):

| Plant | Caught by |
|---|---|
| escalation ignores screens (`wait_ms(false)`) | `a_question_with_a_screen_open_waits_five_minutes` |
| the settle skipped (the needs-you post not indexed as a card) | `a_question_with_no_screen_open_pings_after_sixty_seconds_and_settles_when_answered` |
| the reminder never made (`remind_questions` returns) | `a_question_at_half_its_life_is_reminded_of_once` |
| expiry ignores the question's own life | `an_unanswered_question_expires_to_its_default_or_to_nothing`, `a_question_that_names_no_life_waits_thirty_minutes` |
| `notify_done` ignored by the policy | `row_8_a_flagged_tasks_report_interrupts`, `a_task_asks_the_owner_and_resumes_with_the_choice` |
| `authority_for` drops `owner_ask`'s authority | both `tests_ask` task tests ("no the task's question in 20 s") |

Live check on this VM (a scratch daemon on `theseus-sim discord rig` with a `fake-model` rule that calls `owner_ask`): `theseus ask "PROOF-ASK: decide"`
parked on the question; **61 s later** a pinging message (flags 0) with buttons `1. Careful`, `2. Ship it behind the flag` appeared in ana's DM on the fake;
`theseus answer <id> 2` answered it, the message was edited to `✅ Answered by the CLI: 2. Ship it behind the flag · …` with its buttons gone, and the turn
resumed (`The owner chose 2, "Ship it behind the flag".`). (That run was before the pacer; the pacer's first post is not held.)

## The live check for the maintainer (exact commands)

```
theseus-sim discord rig --dir /tmp/live1
theseus-sim fake-discord --addr 127.0.0.1:9447 --gateway 127.0.0.1:9449 --guild /tmp/live1/guild.json --log /tmp/live1/fake.log &
cat > /tmp/live1/rules.json <<'JSON'
[{"when":"PROOF-ASK","calls":[{"name":"owner_ask","input":{"question":"Which plan?","options":[{"label":"Careful"},{"label":"Ship it behind the flag"}],"default":1}}],"then":"Went on."}]
JSON
theseus-sim fake-model --addr 127.0.0.1:9448 --rules /tmp/live1/rules.json &
PATH=/tmp/live1/bin:$PATH OP_SERVICE_ACCOUNT_TOKEN=x theseusd --config /tmp/live1/config.toml --socket /tmp/live1/sock --state-dir /tmp/live1/state &
theseus --socket /tmp/live1/sock ask "PROOF-ASK: decide"      # parks; nothing in the DM yet
# after 60 s: theseus-sim discord read --fake 127.0.0.1:9447   -> a "Needs you" message in channel ...102 with two buttons, flags 0
theseus --socket /tmp/live1/sock answer <id> 2                # the message is edited "Answered", buttons gone, the turn resumes
```
(`ana`'s DM is channel 900000000000000102.) For a task: `theseus ask "start a task that ..."` with a rule that calls `task_create` whose brief makes the child call `owner_ask`.

## Gate

`TZ=America/Phoenix THESEUS_KEEL_BASE=6f287c4 THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, on the tree of `fecebda` (the third commit; the first two ran the
same gate, the first with one more failure, below). Phases that passed: door (main's copy), keel (`0 findings`), keel tests, fmt, shape, features,
clippy (`-D warnings`), cockpit (lint, test, build), test build, reader rule. The suite phase fails, as the brief says it does on this VM:
**33 tests of 4,546 failed, all L1** (theseus-sandbox's `contract`, `bench spawn_100`, and theseusd's `sandbox`: the VM runs as root, theseus-pv6i). Run by
hand after it, as the brief says: the generated TypeScript is unchanged (`cockpit/src/protocol.gen` clean, the protocol's types did not change); the turn bench
passes (`frames_plain 4/4`, `frames_tool 8/8`, `syncs_plain 2/2`, `first_request_kb 39.35` of 40: **the new tool added about 1 KB to the first request, leaving
0.65 KB of headroom**); the lifecycle, jobs and size phases are skipped (`THESEUS_GATE_NO_BENCH`, no release build); `cargo deny`: licenses, bans and sources ok,
advisories not checked (the advisory database could not be fetched here). Earlier runs of this gate showed `gone_jobs::a_restart_settles…` failing (see below).
I did not complete a run under load (the recipe's four busy loops starve a niced build: it was recompiling when I stopped it); the new tests ran in the
three full-suite runs on this 4-core VM and passed in each.

## Keel findings expected

None of mine. `python3 scripts/keel-guard.py` with the default base reports one finding that is not in my range: `crates/theseus-sim/src/lifecycle.rs`
`budget-raised` and an unverifiable signature on `1c2beb2747` (a main commit; this clone's `origin/main` is older than the branch point, so the
range reaches back over main's own history, and the owner's key is not here). I ran the gate with `THESEUS_KEEL_BASE=6f287c4`, which judges only my commits:
`keel: ok (… 0 findings)`. A test was renamed and rewritten, not deleted: `a_kind_with_no_handler_is_not_answerable_yet` is now
`a_question_with_options_is_answered_by_choosing_one` (it asserts one more thing than before).

## Left, uncertain, and for the owner

- **"Shows at once, silently, on the DM's board."** I did not post a silent message at once for a question that has no card. The question shows at once
  on every Theseus screen (`confirm.list`, the work board) and as a card in a Discord place; the DM's *board* is `discord-board`'s row. What I built is the
  ping (and its settle). If the owner wants a silent DM post at the moment of asking, followed by the pinging one, it is a small change in `needs_you.rs`.
- **The pacer holds a needs-you for up to 30 minutes** after the previous unprompted post (decision 4). Acceptance test 2 (one ping within 60 s with no
  screen open) holds for the first; a second question inside the window is batched. `unprompted_pace_secs = 0` turns the rule off if this is wrong.
- **Numbering**: options and `default` are numbered from 1 (what people type); the answer ids stay `choice:<i>` from 0. `expires_in` is in minutes.
- **Cockpit**: no change; the web UI does not draw an ask's options (it shows the generic card). `theseus ask`'s "[parked: …`theseus confirm`…]" line is generic.
- **Failures outside the known list**: `theseusd::gone_jobs::a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran` failed in the full suite
  (2 of 3 full runs, "the live wrapper's job runs on": 2 dispatched, expected 1) and passes alone. It has nothing of mine in its path (no ask, no board); I could not
  establish whether it is the load of the full suite on this 4-core VM or a base problem. And `tests_kept::fs_read_of_a_long_file_is_a_contiguous_window…`
  failed once in a full run and passes alone (timing). Both are explained by neither the known list nor my change.
- `cargo deny`: `advisories FAILED` (the advisory db on this VM is stale/unfetchable); bans, licenses and sources are ok. No dependency changed.
- Docs to change at review: spec Part III item (this row), `docs/status.md`, §1.8.3/§1.8.4 (what was built: the 1-based numbers, minutes, `needs_you`, the pacer);
  the notices doc's rule 8 now reads "(Interrupt if the owner flagged it)" in `notices.rs`; the `[notify]` section is in the template.
- Disk: this VM's allowance is small for a cold workspace build (~25 GB of `target/`); I deleted `target/debug/incremental` between runs.
