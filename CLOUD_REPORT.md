# Cloud report: Discord stays lively; silence is opt-in, and each place shows or hides thinking and tools (theseus-l1y1)

Branch `cloud/20261008-discord-silent`, cut from `2eed643c` (the task commit `081dae89`). Two sessions built it. This
report covers both, whole; it replaces the first session's report (`abc66cca`).

## The owner's redirection, and what it changed

The first session (02:05 to 04:07 UTC) built CLOUD_TASK.md as written: Discord silent by default, only a card, a
failure, disk below the floor and the answer to the owner's own message pinging, and at most one ping per place per
30 s. After it started, the owner read the plan and overruled the default: "I think we still want a lively chat
interface, replete with thinking and tool messages -- that experience today is quite nice, though a non-dev might
want to turn off thinking and tools -- so that should be configurable, and in our configuration, it should be on",
and "I haven't found anything theseus has said in channel yet as not being worth buzzing my phone."

So the second session (from 04:21 UTC) turned the work around:

- **The defaults are main's.** With no config, every create notifies, as on main: no create carries flag 4096 that
  main sends without it, and there is no 30 s window.
- **Silence is opt-in, per kind**: `[discord] silent`, a list of categories, empty by default.
- **Each place shows or hides its tool lines and its thinking**: `show_tools` and `show_thinking` in the bindings
  file, per `[[channel]]` and `[[dm]]`, both on by default.
- **The binding posted no thinking before**: the CLI and TUI showed it, Discord dropped `model.thinking`. A loop's
  thinking is now a message of its own (step 3: one commit with the two words, so it can be reviewed, or dropped, apart from step 2).

## Step 1 (first session): every write says whether it pings

Commits `9f77e903` and `96398024` (its tests wait for the ledger rows they read). What it found still holds:

- The binding creates messages in exactly two places: the lane's raw `CreateMessage` (`Lane::create`: every outbox
  post and every live upsert) and the embed notice (`Lane::notice`, twilight's builder). Neither set `flags`, so every
  create notified. Edits never notify; interaction answers (button acks, slash command replies) are responses to the
  person who acted, untouched.
- The reply's first part is almost never created by its post: the stream creates it live (theseus-ck0n) and the post
  only edits it, so "the answer to the owner's message" has to be decided on the live create too. The live path knows
  the turn answers a typed message only through `LaneMsg::Anchor` (and `LaneMsg::Author`, sent just before it).
- A turn asks one question at a time: three writes in one tool step give one card; the next call asks after the
  first is answered.
- `Closed::of(&question)` loses who answered from where, so a card written settled reads its words from the settle
  post waiting behind it.

What it built: `Write.ping`; `create` sends `"flags": 4096` unless it pings, and the embed notice
`MessageFlags::SUPPRESS_NOTIFICATIONS`; `policy.rs` (the table, as data); `Lane::owed`; the 30 s window
(`Shared::pings`); a card whose question closed while Discord was away written once, settled; the `ping` and `held`
fields on the `discord.message.out` row; the fake's recorded `flags` (`Msg::flags`, `Msg::silent`); the proof's
printed flags. Its gate was green but for the 33 known L1 cases, `theseus-sim discord proof` passed 12 of 12, and the
lifecycle bench was OK.

## Step 2 (second session): every write notifies by default; `[discord] silent` opts kinds out

### What I found

- Every create on main notifies, so "today's behaviour" is: no flag on any create, the embed notice included.
- The first cut's `owed` was taken only when the answer's create pinged. With the answer silenced by config, `owed`
  would stay set and the next text part would be read as the answer (silenced too, though it is a `replies` part):
  it must be taken when the part is written, ping or not. Planted below.
- `config.rs` is at its ceiling (2,927 of 2,927). Moving `DiscordConfig`'s `Default` impl (16 lines) into the new
  module `config/discord.rs` pays for the field and the `mod` line: config.rs is now 2,917.

### Which categories, and why daemon-wide

`[discord] silent` names any of: `answer`, `replies`, `woken`, `tools`, `thinking`, `cards`, `failures`, `tasks`,
`notes`, `disk` (`theseus_core::config::discord::Category`; serde snake_case, so an unknown name is refused at load).
It is **daemon-wide**, in the config, not per place: a ping is about the owner's phone, not about a place; a shared
place's card lands in the owner's DM, so a place's own word would leave open whose applies; and Discord's own
per-channel notification settings already mute a whole place. What a place *shows* is per place (step 3).

### How each write maps (the table, `policy.rs`)

| Write | Where it is made | Event | Category |
|---|---|---|---|
| `card` (approval, budget, layer-1 change, extension ack, promotion) | courier `card` | Card | `cards` |
| the note beside a card sent to the DM, or a card's only note | courier `card` | CardNote | `cards` |
| `card` whose question had closed when it was written | courier `card` | CardClosed | `cards` |
| `settle`, `jev_labeled` | edits only | CardClosed, Jev | (an edit never notifies) |
| `failed` (a failed turn) | `failed_post` | TurnFailed | `failures` |
| `report`, outcome `failed` (any but complete/cancelled) | `report_post` | TaskFailed | `failures` |
| `report`, outcome `complete` or `cancelled` | `report_post` | TaskEnded | `tasks` |
| the first text part of the reply to an owner's own message, live or the post's | `apply_live`, `reply` | Answer | `answer` |
| a reply's later parts, its footer, any part of a reply to no owner's message | `apply_live`, `reply` | ReplyPart | `replies` |
| the reply of a turn a wake or a task's report started | `reply` | Woken | `woken` |
| live tool line (`<turn>:L<n>:tools`), notice embed | `apply_live`, `notice` | ToolLine | `tools` |
| live thinking (`<turn>:L<n>:think`, step 3) | `apply_live` | Thinking | `thinking` |
| `notice` (bind, publish, budget and loop-cap notices, hours, proposals) | `plan` | Note | `notes` |
| `restarted` | `plan` | Restarted | `notes` |
| `mcp_changed`, `mcp_prompt_changed` | `plan` | Mcp | `notes` |
| `jev_notice`, `jev_paused` | `jev_post` | Jev | `notes` |
| `glide` | `glide` | Glide | `notes` |
| `hands` | `plan` | Hands | `notes` |
| `disk`, any state | `disk_post` | DiskCritical, Disk | `disk` |
| the task board (a task starts, moves, finishes) | `courier/board.rs` | Board | `tasks` |
| `refusal` | writes nothing | n/a | n/a |

With nothing named, every row pings. The table stays data: `theseus_protocol::notices` (work-types' shared policy)
replaces it once both have joined, by mapping each `Event` to its kinds (policy.rs's module doc says so).

### The first session's work, item by item

| Item | Now |
|---|---|
| `Write.ping`, `create`'s `flags: 4096`, the embed notice's flag | **kept**; with no config, never set |
| `policy.rs`, its `Event`s and readers (`of_report`, `of_disk`, `of_reply`, `is_text_part`) | **kept** |
| the table's `Urgency` (Interrupt/Inform) column and its old defaults | **changed**: each row is now its `Category`; `Urgency` removed (nothing read it once the default is "ping") |
| `Lane::owed` (the answer's first part) | **kept**, and **changed** to be taken when that part is written, ping or not |
| the 30 s window (`policy::Pings`, `WINDOW`, `PLACES_KEPT`, `Shared::pings`) | **removed** (the owner: every message is worth a buzz); not kept as an opt-in, to remove the complexity |
| the row's `held` field | **removed** with the window; `ping` kept |
| a closed question's card written once, settled, without buttons or mention | **kept** (it now notifies by default, as main's create did) |
| the fake's `Msg::flags`/`Msg::silent`; the proof's printed flags | **kept**; the proof now also fails a silent card |
| `policy::tests::only_what_needs_the_owner_pings`, `a_place_pings_once_per_window`, `the_windows_map_is_bounded` | **removed** (they test the old default and the window); replaced by `with_nothing_silent_every_write_pings`, `each_category_silences_only_its_own` |
| `tests_silent`'s `the_answer_to_the_owners_message_pings_once_and_its_tool_line_and_later_parts_do_not` | **changed** into `with_no_config_the_answer_its_tool_line_and_its_later_parts_all_notify` and `each_category_silences_only_its_own_message` |
| `tests_silent`'s `a_burst_of_three_cards_pings_once_and_every_card_keeps_its_buttons` | **changed** into `every_card_notifies_by_default_and_cards_silences_them_all` |
| `tests_outbox`'s `a_failed_tasks_report_pings_and_a_finished_ones_and_notes_do_not`, `a_failed_turns_post_pings` | **changed** into `every_post_notifies_by_default_and_each_category_silences_only_its_own` (notice, three reports, failed turn, restart, MCP, hands, disk) |
| `tests_outbox`'s `a_reply_to_no_owners_message_is_silent` | **changed** into `a_reply_to_no_owners_message_is_a_reply_not_the_answer` |
| the disk test's "only below the floor pings" | **changed**: every crossing notifies (the `disk` category is in the test above) |
| `a_cards_settle_waits_for_its_create_and_edits_it_by_id` (written once, settled) | **kept**, its silence assertion flipped: it notifies |
| the AGENTS.md invariant "Silent by default" | **changed** to "Every write notifies unless the config says it is silent" |

### What I changed

Commit `7dcfa4ca` (`discord: every write notifies by default, and [discord] silent opts kinds out`).

- `crates/theseus-core/src/config/discord.rs` (new): `Category`, `Category::ALL`, `DiscordConfig`'s `Default` (moved),
  and its tests. `config.rs`: the field `silent` and `pub mod discord;` (2,917 lines). The template:
  `silent = []` under `[discord]`, its comment naming every category.
- `policy.rs`: the table maps each `Event` to its `Category`; `pings(e, silent)`; `of_live(key, owed)` (a live key's
  event: answer, reply part, thinking or tool line). The window is gone.
- `courier.rs`: `Lane::pings(e)` reads `[discord] silent`; `owed` taken when the part is written; the embed notice's
  flag only under `tools`; the row's `held` gone. `runtime.rs`: `Shared::pings` gone (back to 3,409 lines in this
  commit).
- `theseus-sim`'s proof: a silent answer or a silent card fails it; the card's flags printed.
- `tests_gateway::Rig::start_tweaked` is `pub(crate)`, so tests_silent can set the config.

## Step 3 (second session): each place shows or hides its tool lines and its thinking

### What I found

- A place's settings live in the bindings file (`[[channel]]`, `[[dm]]`: `users`, `mention_only`, `private`,
  `voice`, the ceiling), read live (`runtime/live.rs`, theseus-ocwt: a changed place is updated in place by
  `PlaceMsg::Rebound`). The two words go there.
- **The binding posted no thinking messages.** The renderer ignores `model.thinking`; only the CLI and TUI show it
  (`theseus/src/render.rs`, `show.thinking`). The core streams it (the profile's `thinking_display`, `summarized` by
  default). So what was missing was the thinking message itself, as well as the word.
- `render.rs` is at 3,006 of 3,009: the thinking renderer is its own module (`render/thinking.rs`), and the place
  holds it beside the renderer, so render.rs gains only its `mod` line (3,007).

### What I changed

Commit `9c323191` (`discord: each place shows or hides tool lines and thinking`).

- `bindings.rs`: `show_tools`, `show_thinking` on `ChannelBinding` and `DmBinding` (default true); the example's
  commented lines for both.
- `runtime/show.rs` (new): `Show` (the two words, from a binding) and `View` (the words, and the thinking messages
  while shown). `Place::apply` filters what the place does not show before it reaches the lane: a tool line
  (`…:tools`) and a notice embed under `show_tools`, a thinking message (`…:think`) under `show_thinking`.
  `Rebound` carries the words, so a rewritten file changes them with no restart.
- `render/thinking.rs` (new): a loop's thinking as `<turn>:L<n>:think`, its first text at once, the rest at the
  place's edit tick, and flushed by any other event of its turn so it lands before the loop's text; one message, the
  first 1,800 characters, saying how many it left out (`… (N more characters)`); it follows the renderer's held turns
  (`RECENT_TURNS`, by the same `turn.started`), so the lane keeps and forgets its keys with theirs. The lane's bound
  grows by one key a loop: 1,225 keys (from 905) when every loop thinks (AGENTS.md says so).
- `theseus-core`'s `FakeProvider` streams a `thinking` block as `Delta::Thinking` (the gateway tests drive it).
- runtime.rs: 3,415 of 3,500; render.rs 3,007 of 3,009.

## How I proved it

### Tests

- `policy::tests` (4): every write pings with nothing silent, the table names each event once and every category a
  write; each category silences only its own; each body's and each live key's reading; the text-part test.
- `theseus_core::config::discord::tests` (2): nothing silent by default, the template's `silent = []` and every
  category named in it; each category loads by name, an unknown one (`"tool_lines"`) is refused.
- `tests_outbox.rs`: `every_post_notifies_by_default_and_each_category_silences_only_its_own` (a notice, a finished,
  a cancelled and a failed task's report, a failed turn, the restart notice, an MCP notice, a hands line and a disk
  crossing, the bind notice too: all notify with no config; `tasks`, `failures`, `notes` and `disk` each silence
  exactly theirs); `a_reply_to_no_owners_message_is_a_reply_not_the_answer` (notifies with none and with `answer`,
  silent with `replies`); the disk test and the closed-card test, flipped to notify.
- `tests_silent.rs` (through the gateway): `with_no_config_the_answer_its_tool_line_and_its_later_parts_all_notify`
  (the answer replies to the owner's message; each create's flags agree with its row's `ping`);
  `each_category_silences_only_its_own_message` (`tools`, `answer`, `replies`, `notes`, and `cards`, which touches
  none of them); `every_card_notifies_by_default_and_cards_silences_them_all` (three cards in a row, each approved by
  its own button: all notify with no config, none with `cards`, three presses counted either way).
- `bindings::tests::a_place_shows_tools_and_thinking_unless_it_says_not_to`; `runtime::show::tests` (2: each word
  hides only its own messages; thinking off makes none, and on again shows the turn it still follows);
  `render::thinking::tests` (2: at once then at the tick, flushed by the loop's text; the cut says what it left out,
  and only the renderer's recent turns are held).
- `tests_show.rs` (through the gateway): `a_place_that_hides_tools_and_thinking_hides_them_alone` (`#lab` bound
  `show_tools = false`, `show_thinking = false`: its turn reads the chart and replies, with no tool line and no
  thinking made at all, not even a create's row; ana's DM, saying nothing, shows both, the thinking before the
  loop's text and notifying); `a_rewritten_file_changes_what_a_place_shows` (the same channel shows both, the file
  is rewritten to hide both, and its next turn shows neither, with no restart).
- `cargo nextest run -p theseus-discord -p theseus-sim`: 254 passed, 1 skipped. theseusd's `discord_proof` and
  core's `config::discord` tests: passed. The core's template and sparse tests (`example_template_*`,
  `config::sparse`, theseusd's `default_config`): 20 of 20.

### Planted reverts

Each applied with a script (one exact replacement), the guarding tests run, the file restored and `touch`ed, and
`git status` showing it as before.

| Plant | Failed |
|---|---|
| every create silent (`if !ping` to `if true` in `create`: the first cut's old default, everywhere) | 6 of 6: `with_no_config_…`, `each_category_…`, `every_card_…`, `every_post_…`, `a_reply_to_no_owners_…`, `tests_show::a_place_that_hides_…` |
| the config ignored (`Lane::pings` reads `&[]`) | 4: `each_category_silences_only_its_own_message`, `every_card_…` (cards), `every_post_…`, `a_reply_to_no_owners_…`; the no-config test passed, as it should |
| the table's ToolLine row under `notes` | `policy::with_nothing_silent_every_write_pings` (tools names no write), `policy::each_category_silences_only_its_own`, `tests_silent::each_category_silences_only_its_own_message` |
| `owed` taken only when the answer pinged (the first cut's rule) | `each_category_silences_only_its_own_message`: with `answer` silent, the later part went silent too (`later: true`) |
| the place's filter off (`Place::apply` passes every op) | `tests_show::a_place_that_hides_tools_and_thinking_hides_them_alone` ("no tool line in #lab") |
| `View::on_event` ignores `show_thinking` | `show::tests::thinking_off_makes_none_and_on_again_shows_the_turn`; the gateway test passed, since the filter in `apply` also drops a thinking op: two guards |
| the thinking not flushed by its turn's next event | `thinking::tests::a_loops_thinking_shows_at_once_then_at_the_tick`; the gateway test passed, since a loop's first thinking shows at once |
| `Rebound` does not set the words | `tests_show::a_rewritten_file_changes_what_a_place_shows` ("the second turn shows neither") |

### Under load

AGENTS.md's recipe: four `while :; do :; done` loops at nice 0, the tests at nice 19. The nine gateway and outbox
tests this row owns (tests_silent's three, tests_show's two, `every_post_…`, `a_reply_to_no_owners_…`,
`a_cards_settle_…`, `each_disk_…`), five runs: 45 of 45 passed (about 41 s a run, against 6 s unloaded).

### The Discord proof

`target/debug/theseus-sim discord proof --theseusd target/debug/theseusd` on `9c323191`: **12 of 12 steps** in 1.7 s.
It now fails a silent answer or a silent card. Every bot create in its transcript reads `(flags 0: pings)`: both bind
notices, "ready", the tool line, the card (`buttons ["Approve", "Decline"] (flags 0)`), the footer, and "Done". (On
the first cut the bind notices, tool line, card, footer and "Done" were `flags 4096: silent`.)

### The lifecycle bench

`target/debug/theseus-sim bench lifecycle --runs 10 --check` on `9c323191`: **LIFECYCLE OK** in 51.7 s. Cold start
p95 25.2 ms, from the config copy 27.1, clean shutdown with a job 25.6, a reply's post in flight 77.0 (measured), SIGKILL
restart 22.4, swap 25.2, cancel 12.4 in 2 frames; the continuation driver started "before the Discord binding's token
resolved: no wait for the binding". Nothing is new before the gateway's Ready: the words are read with the bindings
file the binding already reads; the thinking renderer is in memory, bounded by the renderer's 8 held turns (one
string per loop), with no store read; the window's map is gone.

## The live check (the maintainer's, with the test bot)

A scratch daemon of this build on a fresh state dir (never a copy of the owner's store), its own `--config`,
`--socket` and `--state-dir`, `[web]` off, `[discord]` on with the test bot's token reference as the owner's Discord
checks use it, and a bindings file of its own naming **only the test channel** (`private = true`, `users = [the
owner]`, no `[[dm]]` of the owner's): the AGENTS.md trap. Use a profile whose model thinks (its `thinking_display`
`summarized`, the default), so step 1 shows a thinking message.

```sh
D=$(mktemp -d /tmp/lively.XXXX)    # theseus.toml and bindings.toml written as above
target/release-thin/theseusd --config "$D/theseus.toml" --socket "$D/sock" --state-dir "$D/state" &
```

With the owner's phone at hand and the test channel's notifications set to "All messages":

1. **No config, as today.** Type `read README.md and say its first line`. Expect the thinking message
   (`💭 thinking`), the tool line and the reply, and a notification for each create (as on main, plus the new
   thinking message). `theseus --socket "$D/sock" ledger -n 50 -k discord.message.out`: every row `ping: true`.
2. **`tools` silent.** `theseus --socket "$D/sock" shutdown`; add `silent = ["tools"]` under `[discord]` in
   `$D/theseus.toml`; start it again; type the same. Expect no notification for the tool line (it posts; Discord
   shows no bell for it), and one for the thinking and for the reply. The tool line's row: `ping: false`.
3. **`show_tools = false`.** With the daemon running, add `show_tools = false` to the test channel's `[[channel]]`
   in `$D/bindings.toml` (read live within about 2 s; health's Discord block shows the new revision).
   Type the same: no tool line is posted at all; the reply and its thinking are. Add `show_thinking = false` too:
   the next turn posts no thinking message.
4. `theseus --socket "$D/sock" shutdown`, then delete `$D`. Run the owner's daemon again so its slash commands are
   registered again (the AGENTS.md trap).

## What is left, and choices for the owner

- **Thinking is new on Discord, and on by default.** The owner asked for a chat "replete with thinking and tool
  messages", and said it is nice today; the binding never posted thinking, so the owner may have meant the loop's
  own text before a tool call ("Reading the tide chart."), which was and stays shown. If so, the thinking message is
  its own commit (`Commit `9c323191` (`discord: each place shows or hides tool lines and thinking`).`, with the two words) and can be dropped, or `show_thinking` can default to off; say which.
  With no config it notifies like everything else, so a thinking turn now buzzes once more per loop.
- **A thinking message shows the first 1,800 characters**, then `… (N more characters)`: the whole is the session's
  history. One message per loop, no split, so a long thinking never floods the channel.
- **Silence is daemon-wide**, as argued above; a per-place `silent` would be a few lines in the bindings file if the
  owner wants it.
- **No window.** Removed, not kept as an opt-in: nothing asked for it after the redirection.
- **Not tested end to end:** the `woken` and `thinking` categories through a real turn (the table's tests cover
  them, and `thinking`'s create goes the same path as `tools`'); a reply whose stream wrote nothing (Discord away for
  the whole turn) takes `owed` from its post, as before.
- `a_cards_settle_waits_for_its_create_and_edits_it_by_id` keeps its name (batch 10's notes name it), though the
  settle now edits nothing in that scenario; its doc says so.
- Docs to change (not mine to edit): the spec's Part III item and `docs/status.md` for the step; P5 (Discord) in the
  spec could state the rule: every message notifies unless `[discord] silent` names its kind; a place's
  `show_tools`/`show_thinking`; thinking as a message of its own.
- Shared files touched outside the binding: `crates/theseus-core/src/config.rs` (the field and the `mod` line, the
  `Default` impl moved out), the config template, `crates/theseus-core/src/provider.rs` (the fake's thinking arm, 9
  lines), and theseus-sim's proof. No store format change, no protocol type, no new dependency.

## The gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (incremental off: the first session
filled the disk allowance with `target/debug/incremental`), once on each commit's tree:

- `7dcfa4ca`: fmt, shape, features, clippy, the cockpit's lint/test/build, the test build, the reader rule: ok.
  Suite: 3,401 tests, 3,368 passed, **33 failed, all the known L1 root-daemon tests** (theseus-pv6i): 19 of
  theseus-sandbox's contract tests, its bench's `spawn_100`, and 13 of theseusd's `sandbox` tests. Nothing else. The
  phases after it, by hand: protocol types ok, `theseus-sim bench turn --check --runs 5 --burst 0` ok (5 and 9
  frames), `cargo deny --offline check` ok.
- `9c323191`: the same phases ok. Suite: 3,408 tests, 3,375 passed, the same 33 L1 failures and nothing else;
  protocol types ok, the turn bench ok (5 and 9 frames), deny ok.

No test of the preamble's flaky list failed in either run.

`CLOUD_REPORT.md` is this branch's last commit, `cloud report (not for main)`.
