# CLOUD_REPORT: discord-board (theseus-2j2ka)

Branch `cloud/20261013-discord-board`, on main as cloned (876cd6d: `cloud task`, over 7449db9). No store change, no new
dependency, no protocol type changed (no TypeScript to regenerate). Commits, oldest first:

| commit | what |
|---|---|
| `f195d27` | core: the Discord binding may watch the private places it speaks in; the fake Discord holds Components V2 |
| `ec7d8ec` | discord: the live task board, the needs-you card on Components V2, the bot's status line |
| `61685da` | discord: a task's view, Stop and the golden boards under test; Stop answers as V2 |
| `3e0d953` | discord: the board survives an outage; Clear done and Quiet 1 h under test |
| (last) | cloud report (not for main) |

## What I found

- **The binding could not hear its own private places.** `work.watch` from the Discord surface is *guarded* (it reads no
  private text): it must name roots and the core drops every root that is not a shared place's. A DM or a private channel
  (where the owner's tasks live) would have had an empty board. **This is the one core edit** (small, outside every
  in-flight area): `rpc/work_board.rs::may_watch` also lets a guarded **Discord** connection watch a conversation whose
  outbox target is a Discord place of any class. What it hears is still scrubbed and offers no answer (`may_answer`
  false); any other guarded surface still hears nothing of a private room. A core test holds both
  (`tests_push::work::the_discord_binding_watches_the_private_places_it_speaks_in_and_no_other_private_session`).
  Consequence: the binding does not rely on `may_answer`; an answer is judged at the press by `work.answer`.
- **`runtime.rs` was 3,497 of 3,500 lines.** The slash command list moved whole to `runtime/slash.rs` (no change to it),
  and the new code is in new modules; `runtime.rs` is 3,439 now. `shape.sh` is ok; no ceiling raised.
- **The card was already an outbox post** (durable, exactly once, settled by a settle post, reconciled after a restart).
  I kept that: the V2 card *is* that post, drawn as a V2 box (its words from `render::card`, unchanged), with its buttons
  from the question's `answer_options` and its press routed through `work.answer`. Rebuilding it on `work.changed` would
  have lost the durability the outbox gives and gained nothing.
- **Three existing tests assumed the text board** (`the_board_is_one_message_edited_in_place_and_pinned`,
  `a_refused_pin_leaves_the_board_unpinned_and_edited`, `a_change_from_an_unwatched_session_reaches_its_homes_board`):
  their configs now say `live_board = false`, i.e. they test the fallback, which stays. Nothing else existing changed.
- **A chat's own question is not on the board.** My first cut counted a conversation that needs you; the daemon's
  Discord proof (`theseusd::discord_proof`) failed because every approval in a chat made a board in that place. The board
  and the status line now count **tasks** only (task status only); the chat's approval is its card, as before.
- **A bug the tests found in my own code:** Stop's result went back as a classic text reply to a V2 message (Discord
  refuses `content` there). It is a V2 box now. V2 edits and the original-response edit carry the flag.
- **Disk:** this VM's writable allowance (about 40 GB) filled twice during full-suite builds (an incremental debug target
  is 18 GB); the first full-suite attempts died `No space left on device`. I ran with `CARGO_INCREMENTAL=0` and one
  `cargo clean`. Not a code finding; it explains the rerun.

## What I changed

1. **The binding hears the engine** (`runtime/tasks.rs`). One model for all places, seeded by `work.watch` from the
   binding's own connection (`interactive: false`; `roots` = the places' sessions; `limit` 20), kept by `work.changed`
   under the position rule, asked again after `events.lost` and whenever a place's session changes (one ask for a burst:
   `Tasks::rewatch`). A task goes to the place of its root's conversation (`PlaceMsg::Tasks`). Counts for the status
   line are the model's.
2. **The live board** (`render/v2.rs` builder and validator; `render/board_v2.rs`; `courier/v2.rs`;
   `runtime/live_board.rs`). One V2 message a place, pinned, edited and never posted anew (it carries
   `SUPPRESS_NOTIFICATIONS` on its one create, and an edit never notifies). Header counts, **Needs you** (title, short id,
   the question, `[ Answer ↗ ]`), **Now** (what it does, turn, `$spent of $limit`, since), **Done, not yet seen** (24 h,
   until Clear), `[ ✓ Clear done ] [ 🔕 Quiet 1 h ]` and the select "Open a task…". Times are `<t:…:R>`. At most 40
   components and 4,000 characters (`v2::validate`, which the board's renderer lowers its parts' caps to meet): a part
   folds its tail into `-# +N more · /tasks` (needs you keeps the longest waiting first; now and done their newest). The
   lane writes it **at most once in 2 s** (the latest state replaces those waiting; the lane wakes for it without a
   message), finds its pin after a restart (`find_board_v2`), writes an *idle* board only where one is pinned
   (`LaneMsg::BoardIdle`), keeps the latest state through an outage, and **falls back to the text board and the classic
   card for the process** when Discord answers a V2 create with a 400 (`Tasks::refuse_v2`). `[discord] live_board`
   (default on; in the template).
3. **The needs-you card** (`render/card_v2.rs`). Today's wording in a V2 box; buttons from `answer_options` on
   `wa:<last 6>:<option>` (rows of five, at most 20); a press is `work.answer` as the presser (author, `DiscordOrigin`);
   a refused one is a follow-up to the presser alone and the card keeps its buttons; a counted one is settled by the
   outbox's settle post on every surface, so an answer from the CLI settles the Discord card (and the settle edits the V2
   card as a V2 box, `detail.v2`). A card posted before this build settles as the classic card it is. A closed-at-write
   card is silent (today's rule, `Event::CardClosed`). `owner-ask`'s `Ask` arm needs nothing more here: its choices are
   `answer_options` and draw as buttons (`choice:N`); the core's `work.answer` handler for it is that row's.
   **Ping rule (30 s a place):** a card pings unless another card that pinged less than 30 s ago still waits
   (`State::card_may_ping`); the later cards of a burst are created silent, and when the window ends with any still open
   one message says `🔴 N questions need you`, which is edited to "answered" when none is. A question answered and a next
   one asked in the same half minute pings again (so `tests_silent::every_card_notifies_by_default…` is unchanged).
   Quiet 1 h silences a place's cards for an hour.
4. **The status line** (`runtime/presence.rs`). Opcode 3, a custom status `🔴 1 needs you · ⚙️ 3 running`, counts only,
   cleared when nothing works or waits, never twice inside 10 s (`Pacer`: first change at once, the next waits out the
   gap and sends the latest, a change that undoes itself sends nothing), sent again after READY. `[discord] status_line`
   (default on; in the template). The About Me text is in `bindings.example.toml`'s comment (the binding cannot set it).
5. **A task's view** (`render/task_view.rs`): the select and Answer ↗ open an ephemeral V2 view (title, state, doing,
   plan, last words, the commands of its last jobs, its question with the answer buttons, Stop for a task). Stop asks
   once more, then the same `task.cancel` as `/cancel`.
6. **The fake Discord** (`theseus-sim`): V2 messages held to Discord's rules in a validator written apart from the
   binding's; nested buttons and selects; `press_values` (a select), `press_reply` (a button on an ephemeral view the
   binding answered with); presence updates recorded with times.
7. **Docs in the tree:** `crates/theseus-discord/AGENTS.md`, `crates/theseus-sim/AGENTS.md`,
   `crates/theseus-core/AGENTS.md` (the work board's place rule), `bindings.example.toml`, the config template.

## How I proved it

Tests added, 51 in the discord crate (291 in it now) and one in core: `render/v2.rs` 6, `render/board_v2.rs` 10 (one a golden,
`render/golden/board_v2.txt`: the board at 0, 1, 8 and 30 tasks, a needs-you board and an all-done one; one a proptest: any
board is at most 40 components and 4,000 characters with unique ids), `render/card_v2.rs` 4, `render/task_view.rs` 2,
`runtime/tasks.rs` 6, `runtime/presence.rs` 4, `courier/tests_v2.rs` 8 (the lane against the stand-in), `tests_board.rs` 10
(end to end: the real core and work engine, the stand-in gateway and REST, a model stand-in that opens tasks),
`policy::tests::the_chats_pings_are_unchanged_by_the_live_board` (the table, literally), and in core
`tests_push::work::the_discord_binding_watches_…`.

- Worst minute: `courier::tests_v2::two_hundred_transitions_edit_the_board_at_most_once_in_two_seconds` feeds 200
  transitions over ten task counts in 4 s: at most 2 edits; none posted anew. By construction a place's board costs at
  most **30 edits and 1 create a minute** (60 s / 2 s), against Discord's 5 edits in 5 s a channel; the status line at
  most 6 updates a minute; cards are the outbox's creates.
- **Planted reverts** (the file put back and `touch`ed; `git status` clean after each; the same tests pass restored):

| plant | caught by |
|---|---|
| the board posts anew (a new key each write) | `tests_v2::{the_board_is_made_once…, states_inside_the_interval…, two_hundred_transitions…, after_a_restart…, an_idle_board…}` (5) |
| the 30 s card rule off | `tests_v2::a_card_pings_unless_another…`, `tests_board::a_burst_of_questions_pings_once…` |
| the V2 card's settle sent as a classic edit (the card keeps its V2 shape and buttons / the edit is refused) | `tests_board::{an_answer_from_the_cli_settles…, an_answer_pressed_on_the_card…, a_refused_press_is_told…}` |
| the board's 2 s pace off | `tests_v2::{states_inside_the_interval…, two_hundred_transitions…}` |
| the binding cannot watch a private place | the core test and `tests_board::three_tasks_one_asking…` |
| the status line's gap removed | `presence::tests::{two_updates_are_never_inside_ten_seconds…, a_fresh_session_sends_again…}` |

- **Under load** (AGENTS.md's recipe: nice 19 beside four busy loops, test binaries built first): the 22 new timing-
  sensitive tests (`tests_board`, `courier::tests_v2`, `presence`) 22 of 22, three runs. An earlier run found two flaws in
  my tests (a press before its card landed; a gap judged to the millisecond on the stand-in's receive times); both fixed.
- **Gate.** `scripts/gate.sh` stops at its keel phase on this checkout for a finding **the base carries, not mine**:
  `crates/theseus-sim/src/lifecycle.rs:155 budget-raised "seed" => Some(250.0)`, whose `Keel:` ack (`1c2beb2747`) does not
  verify here (`%G? is E`: no signing key on this VM). `python3 scripts/keel-guard.py` lists that one and nothing of mine.
  So I ran the gate's later phases by hand, in its order, on the final tree: `cargo fmt --all -- --check` ok;
  `scripts/shape.sh` ok; `cargo clippy --workspace --all-targets -q -- -D warnings` ok (twice); cockpit `lint` (warnings
  only, as before), `test`, `build` ok; `cargo nextest run --workspace --no-fail-fast`: 4,565 tests, 4,532 passed (1 slow), 33 failed: exactly the known L1 ones below; protocol types: no
  change to `cockpit/src/protocol.gen`; `cargo deny --offline check bans licenses sources` ok (advisories **not**
  checked: no advisory database on this VM and `cargo deny fetch` printed nothing). The benches (`THESEUS_GATE_NO_BENCH`)
  and the features phase (no dependency or feature changed) were not run.
- **Known failures, not mine:** the 33 L1 tests on this VM (theseus-sandbox contract ×22 and `bench spawn_100`, and
  theseusd's `sandbox` ×10), root-daemon L1 refusal (theseus-pv6i). No test outside that list failed in the last run.

## The live check

On this VM, debug builds, a fresh state dir, `theseus-sim discord rig`: a scratch daemon, `fake-discord --gateway`, and
`fake-model --rules` standing in for the model. Ana (the rig's owner) says
`START: run this as a background task, please` in her DM; the model opens three tasks (a write outside the roots that
waits on a card, two programs that sleep 40 and 50 s). What the stand-in held:

```
board  flags 36864 (V2 + silent) pinned  edits 7 over ~60 s
  first state: 📋 Tasks · 🔴 1 needs you · ⚙️ 2 running / 🔴 Needs you / Reef-chart: chart the reef …2b4074 / fs.write create … · asked <t:…:R> / ⚙️ Now / Tide-count … proc.run · turn 1 · $0.03 of $25 · since <t:…:R> / Buoy-paint …
  buttons: tb:o:exe_…2b4074  tb:clear  tb:quiet  tb:pick
card   flags 32768 (V2, no silent bit: it pings once)  buttons wa:266d41:approve  wa:266d41:decline
$ theseus --socket … answer 266d41 approve      → answered act_…266d41 · approve
card   (same message, edited) ✅ Approved by the CLI · task `2b4074`: `fs.write` …   buttons: none
board  ⚙️ 2 running · ✅ 1 done since you looked …  → finally ✅ 3 done since you looked
status (cleared) · 🔴 1 needs you · ⚙️ 2 running · ⚙️ 3 running · ⚙️ 2 running · ⚙️ 1 running · (cleared)
       (gaps of 10.0, 16.5, 10.0, 10.0, 10.0 s)
```
The chat kept its pings (the reply, each task's "finished" post, the tool lines all `flags 0`).

**For the maintainer (exact commands; two terminals).** From the tree, after `cargo build -p theseusd -p theseus-sim -p theseus`:
```
B=target/debug; D=/tmp/lc-rig; rm -rf $D; $B/theseus-sim discord rig --dir $D --theseusd $B/theseusd
# rules: the appendix's, with OUTSIDE replaced by $D/outside, saved as $D/rules.json
$B/theseus-sim fake-discord --addr 127.0.0.1:9447 --gateway 127.0.0.1:9449 --guild $D/guild.json --log $D/fake.log &
$B/theseus-sim fake-model --rules $D/rules.json --addr 127.0.0.1:9448 &
(cd $D && PATH=$D/bin:$PATH OP_SERVICE_ACCOUNT_TOKEN=proof-not-a-token $B/theseusd --config $D/config.toml --socket $D/sock --state-dir $D/state) &
$B/theseus-sim discord say --fake 127.0.0.1:9447 --user 900000000000000101 --name ana "START: run this as a background task, please"
$B/theseus-sim discord read --fake 127.0.0.1:9447        # the board (flags 36864, pinned), the card (wa: ids)
$B/theseus --socket $D/sock answer <last 6 of the card's id> approve
$B/theseus-sim discord read --fake 127.0.0.1:9447 gateway # presences, ≥ 10 s apart
```
Rules (first match wins; the task rules must come before the START rule, since a task's context quotes the ask):
Reef-chart → `fs_write` `$D/outside/reef.txt`; Buoy-paint → `proc_run sleep 40`; Tide-count → `proc_run sleep 50`; START →
three `task_create`s quoting "run this as a background task, please" (see `crates/theseus-discord/src/tests_board.rs`
`start_all` for the shape).

**The owner's check, in #theseus-test (real Discord, the real bot):** (1) start five tasks, two of them long, one that
will ask (a write outside the roots): the DM gets **one** pinned 📋 message, and it follows them (it edits, no new post,
no buzz); a **question** buzzes once as a card with Approve / Decline. (2) Look at the bot in the member list: its status
reads `🔴 1 needs you · ⚙️ 4 running`, and it clears when the last task ends. (3) Press `Answer ↗`: an ephemeral view only
you see; press Approve in it, or answer with `theseus answer` from a shell: the card and the board settle. (4) Press
`Quiet 1 h`, start a task that asks: its card arrives silent; `Unquiet` ends it. (5) Restart the daemon: the same pinned
board is edited, not a second one. **If the status text does not show,** Discord may not show a *custom* status for a
bot: change `presence::frame` to type 3 (Watching) with the text as `name`; nothing else reads it.

## What is left or uncertain

- **Real Discord has not seen any of this.** V2 behaviour is built to the documented rules and held in the stand-in; two
  things I could not confirm: whether an edit of a V2 message needs the flag (I send it on every edit) and whether a bot's
  custom status shows (above).
- Design choices the owner should hear about: the **board is pinned in every place that has tasks** (the brief says
  the DM and private channels; I pin shared channels too, because an unpinned board cannot be found after a restart and
  would be posted twice); the board and status line count **only the tasks of the places this bot serves** (a CLI session's
  tasks are invisible to the Discord surface by the place rule); the 30 s rule is **per burst of open cards**, not a flat
  30 s (see 3); **"since you looked" is one in-memory time a place** (a restart shows the last 24 h of done tasks once
  more); a **task's "brief" and "last tool lines"** in its view are its title, plan and last jobs' commands (the work view
  has no transcript, and the guarded surface may not read one); the **task-end report posts still ping** as the `reports`
  category says (the owner's 2026-10-08 decision keeps the chat lively), so "the needs-you card is the only task ping" is
  true of status, not of those reports: silence them with `silent = ["reports"]` if wanted.
- `work.notice` is not used (the board is drawn from `work.changed`); `work-digest` can read the model's views.
- The rig prints `start` lines for the proof's model; `discord rig` has no option for the task rules, so the live check
  above supplies them by hand.

## Keel findings expected

None from this work: no test deleted, ignored, or weakened; no cap, budget or ceiling raised (`long-files.txt` untouched;
`runtime.rs` shrank); no `#[allow]`. The three tests that now set `live_board = false` keep their assertions. The
guard's one finding is the base's (above).

## Where the docs should change (I edited none of the maintainer's)

- Spec Part III: a new item for theseus-2j2ka (the live board, the card on V2, the status line, the core's Discord-surface
  watch rule, the 30 s rule's per-burst reading); its version line; `docs/status.md` (Updated, the recently landed step,
  the roadmap row `discord-board`).
- `docs/design`: the Discord investigators' §1.6/§4.3 are not in the repo; the roadmap row's note that the owner-ask arm
  rides the V2 card.
- `docs/technical-overview.md`: one line under the Discord binding that the task board is the work engine's.

## Keel guard, pasted (`python3 scripts/keel-guard.py` on the final tree)

```
keel: the same test: crates/theseus-store/src/store/tests.rs::a_checkpoint_for_close_is_made_durable_by_the_close -> crates/theseus-store/src/store/tests.rs::a_checkpoint_for_close_is_durable_without_the_close (renamed and rewritten)
keel: refused the ack `Keel: budget-raised crates/theseus-sim/src/lifecycle.rs — the owner, 2026-10-13 23:24: "i'm happy for yes to all until we're at testing time to eval" (seed tightened by store size; push budget above the 20 us target until theseus-oy9zq)` on 1c2beb2747: its signature does not verify (%G? is E)
keel: 1 erosions of a test, a budget or a ceiling (a branch: merge-base(HEAD, origin/main)..HEAD, 9fa84735c0):
  crates/theseus-sim/src/lifecycle.rs:155: budget-raised: "seed" => Some(250.0), -> (gone or rewritten)
keel: each passes only with the owner's yes: a commit in the range, signed by a key in scripts/keel-signers (at the base), whose message carries these trailers, each with its reason:
  Keel: budget-raised crates/theseus-sim/src/lifecycle.rs — <why>
```
Everything in it is from the base (main's commits before this branch's), none from this work.

## Appendix: the live check's model rules (`fake-model --rules`)

First match wins; a task's own context quotes the ask, so its rule comes first. `OUTSIDE` is `$D/outside`.

```json
[
 {
  "when": "Reef-chart",
  "then": "Charted.",
  "calls": [
   {
    "name": "fs_write",
    "input": {
     "path": "OUTSIDE/reef.txt",
     "content": "reef"
    }
   }
  ]
 },
 {
  "when": "Buoy-paint",
  "then": "Painted.",
  "calls": [
   {
    "name": "proc_run",
    "input": {
     "argv": [
      "sleep",
      "40"
     ]
    }
   }
  ]
 },
 {
  "when": "Tide-count",
  "then": "Counted.",
  "calls": [
   {
    "name": "proc_run",
    "input": {
     "argv": [
      "sleep",
      "50"
     ]
    }
   }
  ]
 },
 {
  "when": "START:",
  "text": "Starting them.",
  "then": "Started all three.",
  "calls": [
   {
    "name": "task_create",
    "input": {
     "brief": "Reef-chart: chart the reef",
     "arrangement": {
      "pieces": [
       {
        "quote": "run this as a background task, please",
        "role": "objective"
       }
      ]
     }
    }
   },
   {
    "name": "task_create",
    "input": {
     "brief": "Buoy-paint: paint the buoy",
     "arrangement": {
      "pieces": [
       {
        "quote": "run this as a background task, please",
        "role": "objective"
       }
      ]
     }
    }
   },
   {
    "name": "task_create",
    "input": {
     "brief": "Tide-count: count the tides",
     "arrangement": {
      "pieces": [
       {
        "quote": "run this as a background task, please",
        "role": "objective"
       }
      ]
     }
    }
   }
  ]
 }
]
```
