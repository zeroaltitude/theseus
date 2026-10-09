# Cloud report: Discord silent by default (theseus-l1y1)

Branch `cloud/20261008-discord-silent`, cut from `2eed643c` (with the task commit `081dae89`). Started 02:05 UTC,
report at the time of its commit. No store format change, no config key, no protocol type, no new dependency.

## The step: every write says whether it pings

### What I found

- The binding creates messages in exactly two places: the lane's raw `CreateMessage` (`Lane::create`, every outbox
  post and every live upsert) and the embed notice (`Lane::notice`, twilight's builder). Neither ever set `flags`, so
  every create notified. Edits (`update_message`) never notify; interaction answers (button acks, slash command
  replies) are responses to the person who acted, not messages that notify, and are untouched.
- The reply's first part is almost never created by its post: the stream creates it live (theseus-ck0n) and the post
  only edits it. So "the reply to the owner's own message pings" has to be decided on the live create too. The live
  path knows the turn answers a typed message only through `LaneMsg::Anchor` (and `LaneMsg::Author`, sent just
  before it for the same message).
- A turn asks one question at a time: three writes in one tool step give one card; the next call asks after the
  first is answered.
- `Closed::of(&question)` loses who answered from where (it said `by operator` for a CLI decline the settle post names
  `cli#1`), so a card written settled reads its words from the settle post waiting behind it.

### How each post kind and live write maps

| Write | Where it is made | Event | Pings |
|---|---|---|---|
| `card` (approval, budget, layer-1 change, extension ack, promotion) | courier `card` | Card | **yes** |
| `card`'s note beside a card sent to the DM, or a card's only note | courier `card` | CardNote | no |
| `card` whose question had closed when it was written | courier `card` | CardClosed | no (written settled) |
| `settle`, `jev_labeled` | edits only | CardClosed, Jev | (an edit never notifies) |
| `failed` (a failed turn) | `failed_post` | TurnFailed | **yes** |
| `report`, outcome `failed` | `report_post` | TaskFailed | **yes** |
| `report`, outcome `complete` or `cancelled` | `report_post` | TaskEnded | no |
| `reply`: first text part, to an owner's own message, not streamed | `reply` | Answer | **yes** |
| live text part while `owed` (the first one the stream creates) | `apply_live` | Answer | **yes** |
| `reply`'s later parts, its footer, any part when the turn answered no owner | `reply`, `apply_live` | ReplyPart | no |
| `reply` of a turn a wake or a task's report started | `reply` | Woken | no |
| live tool line (`<turn>:L<n>:tools`), live notice embed | `apply_live`, `notice` | ToolLine | no |
| `notice` (bind notice, publish, budget and loop-cap notices, hours, proposals) | `plan` | Note | no |
| `restarted` | `plan` | Restarted | no |
| `mcp_changed`, `mcp_prompt_changed` | `plan` | Mcp | no |
| `jev_notice`, `jev_paused` | `jev_post` | Jev | no |
| `glide` | `glide` | Glide | no |
| `hands` | `plan` | Hands | no |
| `disk`, state `below_floor` | `disk_post` | DiskCritical | **yes** |
| `disk`, state `low` or `ok` | `disk_post` | Disk | no |
| the task board (a task starts, moves, finishes) | `courier/board.rs` | Board | no |
| `refusal` | writes nothing | n/a | n/a |

Kinds the table did not name: glide, hands, the board, Jev's pause and label, the restart notice's kind `restarted`,
the cards' notes, and a closed card; all are Inform. **"Blocked" has no post of its own**: a task that is blocked on
a question shows as its card (which pings); parked tasks are health only. A cancelled task's report is silent (the
owner cancelled it, or `/stop` did); say if it should ping.

### What I changed (commits `9f77e903`, and `96398024`, the tests' wait for their rows)

- `crates/theseus-discord/src/policy.rs` (new): `Urgency` (Interrupt / Inform, `theseus_protocol::notify`'s words),
  `Event`, `TABLE` (data), `pings`, the readers `of_report`, `of_disk`, `of_reply`, `is_text_part`, and `Pings`, the
  window. Its module doc says it is to be replaced by `theseus_protocol::notify` once both have joined. Every write
  site asks `policy::pings(Event::…)`, so the replacement is a mapping of `Event` to the shared policy's kinds.
- `courier.rs`: `Write` gains `ping`; `create` sends `"flags": 4096` unless it pings; the embed notice sends
  `MessageFlags::SUPPRESS_NOTIFICATIONS`. `Lane::owed` (set on `Anchor` when the last author is an owner by the place
  rule, taken by the answer's create, live or the post's); the reply's first text part pings only when it answers
  that message; a footer alone never does. `card` writes a closed question's card settled, silent, without buttons
  or mention, `detail.settled` set, and `settled` then plans nothing. `failed` and `report` moved into
  `failed_post`/`report_post` (clippy's 100-line limit on `plan`). The `discord.message.out` row gains `ping` and
  `held` (the table asked for a ping and the window took it).
- The window: `Shared::pings` (one field and its two initialisers in `runtime.rs`, 3,409 to 3,413 of 3,500): per
  channel, so a card a place's lane sends to the owner's DM shares the DM's window; checked before the create,
  marked only once the create lands (a create Discord never took leaves it open); at most 256 channels, the ones
  older than the window dropped first, then the oldest. No store read, nothing on the start path.
- `theseus-sim`: the fake keeps each create's `flags` (`Msg::flags`, `Msg::silent`, `SUPPRESS_NOTIFICATIONS`); the
  Discord proof prints each of the bot's creates as `(flags 4096: silent)` or `(flags 0: pings)` in its transcript,
  and its typed-message step fails when the answer to ana's message went out silent.
- `crates/theseus-discord/AGENTS.md`: an invariant "Silent by default" and the new test file.
- Line counts: courier.rs 1,712 (not listed), runtime.rs 3,413 of 3,500, render.rs untouched, tests_outbox.rs 1,694.

### How I proved it

- Tests (all in theseus-discord):
  - `policy::tests`: the table (exactly Card, TurnFailed, TaskFailed, Answer, DiskCritical ping), each body's
    reading, the text-part test, one ping per window per place, the map's bound.
  - `tests_silent.rs` (new, through the stand-in's gateway): `the_answer_to_the_owners_message_pings_once_and_its_tool_line_and_later_parts_do_not`
    (ana types in `#lab`: the answer's first part pings and replies to her message; the tool line, the second loop's
    part and the bind notice carry 4096; the rows say the table asked no ping for them, so the window hides nothing);
    `a_burst_of_three_cards_pings_once_and_every_card_keeps_its_buttons` (three cards in one place inside 30 s, each
    approved by its own button as it lands: one ping, the second and third `held`, three presses counted ok).
  - `tests_outbox.rs`: `a_failed_tasks_report_pings_and_a_finished_ones_and_notes_do_not` (bind notice, a notice, a
    complete and a cancelled report silent; a failed report pings), `a_failed_turns_post_pings`,
    `a_reply_to_no_owners_message_is_silent`; `each_disk_crossing_posts_one_note_in_the_dm_approvals_go_to` now
    asserts only `below_floor` pings; `a_cards_settle_waits_for_its_create_and_edits_it_by_id` (the closed-while-away
    scenario) now asserts the card is created once, already declined by `cli#1`, no buttons, no edit, silent.
  - `cargo nextest run -p theseus-discord -p theseus-sim`: 248 passed, 1 skipped. theseusd's `discord_proof`: passed.
- Planted reverts (each applied, the guarding tests run, the file restored and `touch`ed, `git status` clean of it):

  | Plant | Failed |
  |---|---|
  | Card row → Inform | burst (`one ping for the burst`: none) |
  | TurnFailed row → Inform | `a_failed_turns_post_pings` |
  | TaskFailed row → Inform | `a_failed_tasks_report_…` (`a failed task pings`) |
  | Answer row → Inform | `the_answer_…` (`the answer pings`) |
  | DiskCritical row → Inform | the disk test (`below_floor`) |
  | ToolLine row → Interrupt | `the_answer_…` (`:L0:tools` row) and the burst |
  | ReplyPart row → Interrupt | `the_answer_…` (`:L1:p0` row) and `a_reply_to_no_owners_message_is_silent` |
  | Note row → Interrupt | 5 tests (the bind notice first) |
  | TaskEnded row → Interrupt | `a_failed_tasks_report_…` (`a finished task`) |
  | Disk row → Interrupt | the disk test (`low`) |
  | `owed` never taken | `the_answer_…` (`:L1:p0` row) |
  | window off (`open` always true) | burst |
  | closed card written live | `a_cards_settle_waits_…` (`written once, settled`: 1 edit, 2 versions) |

  The first ReplyPart plant passed: the live path did not ask that row for a part of a turn answering no one. I made
  it ask (`apply_live`), and the plant then failed as above.
- Under load (4 busy loops at nice 0, tests at nice 19): the 17 tests above, 5 runs: 4 green, 1 failure of
  `the_answer_…`: the `discord.message.out` row is written by `binding_ledger_soon`, off the lane's path, and could lag
  the message the fake already held. Both new tests now wait for the rows they read (`96398024`); after it, the
  two tests ran 8 times under the same load: 8 of 8 green.
- `theseus-sim discord proof --theseusd target/debug/theseusd`: 12 of 12 steps. Its transcript: both bind notices
  `(flags 4096: silent)`, `"ready"` `(flags 0: pings)`, the tool line, the card, the footer and "Done" silent. The
  card is silent there because the window took it: the answer to ana's first message pinged in `#lab` under a second
  earlier (D3, as designed).
- `target/debug/theseus-sim bench lifecycle --runs 10 --check` on `9f77e903`: LIFECYCLE OK (cold start p95 20.1 ms,
  clean shutdown 22.9, the reply's post in flight 71.5 measured, SIGKILL restart 20.5, swap 21.1, cancel 9.7, 2
  frames). The binding started no earlier than before (continuation driver "before the Discord binding's token
  resolved: no wait for the binding").

### The live check (the maintainer's, with the test bot)

A scratch daemon of this build on a fresh state dir (never a copy of the owner's store), its own `--config`,
`--socket` and `--state-dir`, `[web]` off or on 7434 to 7439, `[discord]` on with the test bot's token reference as
the owner's Discord checks use it, and a bindings file of its own naming **only the test channel** (`private = true`,
`users = [the owner]`, no `[[dm]]` of the owner's): the AGENTS.md trap.

```sh
D=$(mktemp -d /tmp/silent.XXXX)    # its theseus.toml and bindings.toml written as above
target/release-thin/theseusd --config "$D/theseus.toml" --socket "$D/sock" --state-dir "$D/state" &
```

With the owner's phone at hand and the test channel's notifications set to "All messages":

1. Type a question that makes a tool call (`read README.md and say its first line`). Expect **one** notification:
   the answer's first part. The tool line and the footer post silently (no badge sound; Discord shows no bell).
2. Wait **31 s**, then type a request that writes outside the roots (a path on `approve_paths`). Expect one
   notification: the card, with its buttons. Approve it: the reply after it is silent.
3. Within 30 s of the card, cause a second card: it posts silently, its buttons still work.
4. `theseus --socket "$D/sock" ledger -n 50 -k discord.message.out`: each create's row has `ping` (what went out)
   and `held` (the table asked for a ping and the window took it): step 1's answer `ping: true`, its tool line both
   false, step 3's second card `held: true`.
5. `theseus --socket "$D/sock" shutdown`, then delete `$D`. Run the owner's daemon again so its slash commands are
   registered again (the AGENTS.md trap).

### What is left, and choices for the owner

- **The window covers the answer and the card of one exchange.** Typing "write X" pings with the answer's first
  part ("Writing it."); the card for that call, a second later, then goes out silent (D3: "whatever pings"). The
  owner just typed, so is likely looking, but say if a card should always ping (a card-only exception to the
  window is one line in `write`).
- **Cancelled task reports** are silent (TaskEnded). **A reply to someone else's message** in a channel (a
  collaborator the place lists) is silent: only an owner's message makes its answer ping.
- **A reply whose stream wrote nothing** (Discord away for the whole turn) pings with its post's first text part if
  the lane still holds `owed`; after a restart `owed` is gone and that reply is silent. Not tested end to end.
- `a_cards_settle_waits_for_its_create_and_edits_it_by_id` keeps its name (batch 10's theseus-0bq1/sj0t fixes and
  the flaky notes name it), though the settle now edits nothing in that scenario; its doc says so. Rename at the join
  if no branch still names it.
- Docs to change (not mine to edit): the spec's Part III item and `docs/status.md` for the step; P5 (Discord) in the
  spec could state the rule: silent by default, the table, the 30 s window.

## The gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the tree of `9f77e903`: fmt,
shape, features, clippy, the cockpit's lint/test/build, the test build, the reader rule: ok. Suite: 3,400 tests,
3,367 passed, **33 failed, all the known L1 root-daemon tests** (theseus-pv6i): theseus-sandbox's contract tests and
bench `spawn_100`, and theseusd's `sandbox` tests. No other failure. The phases after it, by hand: protocol types ok,
`theseus-sim bench turn --check --runs 5 --burst 0` ok (5 and 9 frames), `cargo deny --offline check` ok.

A first gate run failed widely (job, cancel, egress, broker tests in theseus-core and theseusd) because the VM's disk
allowance filled during it (`target/debug/incremental` had grown to 16 GB; ENOSPC). I deleted the incremental cache,
reran with `CARGO_INCREMENTAL=0`, and the run above is the clean one. Nothing of that run's failures reproduced.

On `96398024` (the tests' fix), the same gate again: the same 3,367 passed and the same 33 L1 failures, nothing
else; protocol types ok, the turn bench ok (5 and 9 frames), deny ok. No other test of the preamble's flaky list
failed in either run.

`CLOUD_REPORT.md` is this branch's last commit, `cloud report (not for main)`.
