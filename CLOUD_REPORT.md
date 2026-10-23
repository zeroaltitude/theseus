# CLOUD_REPORT: tail7b-golden (theseus-hd8l2)

Branch `cloud/20261021-tail7b-golden`, base `ae420a49`. Tests and docs only; no runtime code changed. Store format 49, untouched.
No file went over a ceiling (`lib.rs` of the crate gained 2 lines; no listed file was touched).

## What I changed (commit `f378ca27`)
- NEW `crates/theseus-discord/src/tests_dm_flow_golden.rs`, one `mod` line in `lib.rs`.
- NEW golden `crates/theseus-discord/src/golden/dm_flow.txt` (with the owner's-lock header).
- `crates/theseus-discord/AGENTS.md`: two lines saying the flow is locked (beside the task-flow sections).
- The fake Discord and every existing rig and test are untouched (I copied `home_up`, `gated_asks`'s shape and the card/board finders into the new file).

## The drive (`the_dm_task_flow_is_as_the_owner_approved_it`)
1. Rig: `rig_in(guild(false), …)` of the default forum, with `task_home = "theseus-tasks"` (the default config), the trusted guild, ana's DM.
2. Wait for the forum and the task home to stand.
3. The owner types `ASK` in the DM; wait for the reply `Started.`; the stand-in model makes the task `Reef-chart`.
4. Wait for the task's forum post; open the gate; wait for the approval card in the post (the task writes outside its tree) and for the board in the home.
5. Snapshot the card (`asked`), press its `:approve` button as the owner.
6. Wait for: the card settled, the report in the home, the post renamed `✅` and tagged done, the pinned board saying `done`.
7. Build the record, normalize it, compare with the golden (or write it on `THESEUS_GOLDEN=write`).

One task showed every part, so no second task was driven. No channel-per-task or index card appears (those need a channel or a plan).

## What the record holds
Per place (DM, the task's post, the task home), each message in creation order: author, flags (`v2`, `silent`, `pinned`), "(a reply)", the text it ended on, its buttons as `[label (style) id]` (style from the components as sent). Plus the post's names and tag history, and the approval card as it asked (create text and buttons), snapshotted before the press.

Limits of the fake that shaped this: `Msg` keeps content versions but not their interleaving across messages, and it stores no embeds (the flow has none visible). The fake cannot say when an edit happened relative to other messages, so I record the **final** text of each message, not the stream's first text or the edits between: a stream's first text is timing (a reply was seen as `Star` then `Starting.`), and the board/card create text depends on which state it was made in. The exception is the approval card, made whole, recorded as it asked. No test-only helper was added to the fake.

## Normalized, and why each is unstable
- Snowflake ids (15+ digits): `<owner>`, `<bot>`, `<post>`, `<home>`, `<forum>`, else `<id-N>` by first appearance (ids are ordinal per fake).
- The task's short id (last 6 of its id, from the clock/ULID): `task-1`. The question id in `wa:<last6>:…`: `wa:<question>`. Session and turn ids in the inspect link: `<ses>`, `<turn>`.
- `<t:…:R>` times: `<time>`.
- Durations (`56 ms`, `0.1 s`): `<dur>`.
- The rig's temp directory: `<tmp>`.
Kept as they are: every emoji, word and line break, and the dollar amounts (`$0.0577`, `$0.0705`, `$0.07 of $25`): the stand-in model makes them deterministic (they did not vary in 25+ runs). The model name and the `localhost:7433` link also stay.

## The golden's full text
```
# The Discord DM task flow, locked by the owner on 2026-10-22. Change this file only with the
# owner's OK (`THESEUS_GOLDEN=write cargo nextest run -p theseus-discord dm_flow`).
# Each place lists its messages in the order it shows them: its text when the test ended, its buttons as `[label (style) id]`, and its flags. Ids (a task's, a question's,
# a session's), times, durations and the temp path are normalized (see `Names`); everything else is as the fake Discord saw it.
# The DM's lines after the owner's message arrive in a different order from run to run, so they are sorted.
=== the owner's DM (after the owner's message, sorted by text)
--- 1 bot []
🔗 Theseus is bound here; a fresh session starts with your next message. Talk to me in this place. `/stop` halts what I am doing and keeps the conversation, `/new` starts a fresh one, `/trust` trusts the conversation again after it reads web text, and `/status`, `/tasks` and `/wakes` show this place's; everything shows in the web UI.
--- 2 owner []
START: run this as a background task, please
--- 3 bot []
Started.
-# sonnet · claude-sonnet-5-5 · 2 loops · 1 tool call · $0.0577 total ($0.0577 this reply) · <dur> · [inspect](<http://localhost:7433/session/<ses>/turn/<turn>>)
--- 4 bot []
(a reply)
Starting.
--- 5 bot []
✅ `task.create` {"arrangement":{"pieces":[{"quote":"run this as a background task, please","role":"object… · 🔔 notified (enforcement = notify) · <dur>
[Should I have asked? (0) tighten]
--- 6 bot [silent]
🔔 started task `task-1` · Reef-chart: chart the reef · <#<post>>
=== the task's forum post
--- 1 bot [v2 silent]
**Reef-chart: chart the reef** `…task-1`
✅ done · started <time> · $0.07 of $25
**🔧 Last calls**
`sh -c echo $$ > <tmp>/gate.pid; n=0; while [ ! -f <tmp>/go ] && [ -d …`
**Report** · done: reported
--- 2 bot [silent]
-# ↩ from the DM
--- 3 bot []
✅ `proc.run` sh -c echo $$ > <tmp>/gate.pid; n=0; while [ ! -f <tmp>/go ] && [ -d … · <dur>
--- 4 bot []
✅ `fs.write` <tmp>/outside/reef.txt · <dur>
--- 5 bot [v2]
✅ **Approved** by discord:ana · task `task-1` · *Reef-chart: chart the reef* wants to **write** `outside/reef.txt` in Theseus's state folder (new file, 4 bytes)
--- 6 bot []
Written.
post names: ⚙️ task-1 · Reef-chart: chart the reef -> ✅ task-1 · Reef-chart: chart the reef
post tags: running -> done
=== the approval card, as it asked
--- card bot [v2]
<@<owner>> ✋ **Approve?** task `task-1` · *Reef-chart: chart the reef* wants to **write** `outside/reef.txt` in Theseus's state folder (new file, 4 bytes)
risk: writes a file · why it asks: <tmp>/outside/reef.txt is protected: <tmp>/outside is on the approve list
```diff
<tmp>/outside/reef.txt:1
@@ -0,0 +1,1 @@
+reef
\ No newline at end of file
```
**Approve** runs this one call · **Decline** refuses it; the task is told and carries on
-# expires <time> · or `theseus answer task-1 approve`
[Approve (3) wa:<question>:approve]
[Decline (2) wa:<question>:decline]
=== the task home
--- 1 bot [v2 silent pinned]
📋 **Tasks** · ✅ 1 done since you looked
**✅ Done, not yet seen**
✅ **Reef-chart: chart the reef** `…task-1` · done <time> · [↗ thread](<https://discord.com/channels/<id-1>/<post>>)
[✓ Clear done (2) tb:clear]
[🔕 Quiet 1 h (2) tb:quiet]
[➕ New task (2) tb:new]
[Open a task… (0) tb:pick]
--- 2 bot [silent]
🧵 <#<post>> · Reef-chart: chart the reef
--- 3 bot []
📋 **Task `task-1` finished** · Reef-chart: chart the reef · <#<post>>
Written.
-# 2 turns · $0.0705 of $25 · <dur>
```

## Found while building (not fixed; for the owner)
- **The DM's order is not deterministic.** After the owner's message, the reply (`Starting.` then `Started.`), the `🔔 started task` line and the `task.create` notice land in a different order from run to run (8 of 32 early runs failed on it). The golden therefore sorts those lines by text and says so in its header. A real order drift among them is not caught. If the owner wants the order pinned, the lanes' ordering would have to be made deterministic: a runtime change, not made here.
- The `🔔 started task` line and `🧵` pointer are silent; the `Approve?` card pings (`<@owner>`), not silent. As expected, not a bug.
- No duplicate post or missing line seen. The approval card's layout is the plain-words one (`the_approval_card_reads_as_the_owner_approved_it`), recorded as it is.

## Proof
- Determinism: after the final version, `cargo nextest run -p theseus-discord dm_flow` 20 times (loop, passes counted): **20 of 20 passed**; before the DM sort, 6 of 20 failed (the order above); 25 of 25 after the sort. Under the load recipe (`nice -n 19` beside four busy `sh -c 'while :; do :; done'` loops at nice 0, killed by pid): **5 of 5 passed**. Waits are `until` loops, no sleeps.
- Plants (each restored, `touch`ed, `git status` clean after); each fails the golden test with a line-numbered diff:
  - (a) `runtime/threads.rs` "started task" -> "begun task": `differs … at line 21: got: 🔔 begun task `task-1` · … want: 🔔 started task `task-1` · …`
  - (b) `render/answer_v2.rs` buttons `.iter().rev()`: `at line 53: got: [Decline (2) wa:<question>:decline]  want: [Approve (3) wa:<question>:approve]`
  - (c) `render/board_v2.rs` header "📋 **Tasks**" -> "**Tasks**": `at line 57: got: **Tasks** · ✅ 1 done since you looked  want: 📋 **Tasks** · ✅ 1 done since you looked`
  - (d) `courier.rs` `report_post` writes a second report post: `at line 70: got: --- 4 bot []  want: <end>`
- `cargo nextest run -p theseus-discord -p theseus-sim --no-fail-fast`: 728 passed, 2 failed, 1 skipped; the 2 (`the_judge_benchs_every_pack_arm_counts_a_plain_turns_own_frames`, `the_restore_rows_store_is_the_same_with_the_cancel_row_selected`) failed only because `target/debug/theseusd` was not yet built ("not built: build the workspace first"); the full gate below built it and ran them.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`: clean (also the gate's phases).
- Gate: `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`. **I turned debuginfo off because the VM's disk allowance (about 39 GB) filled twice with the gate's default `line-tables-only` and I had to delete `target/debug`.** Result: door, keel, keel tests, fmt, shape, features, clippy, cockpit (lint, tests, build), test build, reader rule: ok. The suite phase ended "FAILED" with 7,960 tests: the only failures are the known L1 root-VM cases: 38 `theseus-sandbox::contract`, 2 `theseus-sandbox::bench`, 28 `theseusd::sandbox` (theseus-pv6i); no other test failed (theseus-discord whole, including the golden, theseus-core's output golden, theseus-store and theseus-core's flaky cases all passed). I then ran the phases after the suite by hand: protocol types (no diff in `cockpit/src/protocol.gen`), `theseus-sim bench turn --check --runs 5 --burst 0` (ok), and `cargo deny --offline check` after `cargo deny fetch` ("advisories ok, bans ok, licenses ok, sources ok"). The lifecycle, jobs and size benches were skipped (`THESEUS_GATE_NO_BENCH`, no release build), as the brief says.
- Keel guard: `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing (rc 0).
- Commits are unsigned (no key on the VM), with the cloud trailer.

## Live check for the maintainer
No keys needed. On the owner's machine, after the merge:
`THESEUS_GATE_NO_BENCH=1 TZ=America/Phoenix cargo nextest run -p theseus-discord dm_flow` shows 1 passed. To see a drift fail: change one word of the `🔔 started task` format in `crates/theseus-discord/src/runtime/threads.rs` (near line 721), rerun, and read `differs … at line 21`; restore and `touch` the file. `THESEUS_GOLDEN=write` rewrites the golden only with the owner's OK.

## For the integrator
- `docs/status.md`: "The Discord DM task flow is locked by one golden test, `tests_dm_flow_golden::the_dm_task_flow_is_as_the_owner_approved_it` (golden `crates/theseus-discord/src/golden/dm_flow.txt`): any change to its text, buttons or order needs the owner's OK (theseus-hd8l2)."
- Spec Part III: one item for this step; the keel/convention is `THESEUS_GOLDEN=write`.
- tail7b-messages and every later row must leave this golden unchanged: run `cargo nextest run -p theseus-discord dm_flow` after each merge.
- The golden's DM order is sorted (see above); the DM has no embed text because the fake stores none.
