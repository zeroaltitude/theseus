# theseus-discord

The Discord binding (spec P5, M3): text and voice channels in one or more guilds, and direct messages, in the
daemon's process. Read by theseusd.

Key modules: `runtime.rs`, `courier.rs`, `render.rs`. Read by: theseusd.

## What's here

- `src/runtime.rs`: the gateway loop and the places (a text channel or a DM, each backed by one session), with the
  slash commands and the confirm buttons. `Routes::resolve` finds a message's or an interaction's place.
- `src/courier.rs`: durable delivery, the binding's side: one lane per place, and one for the operator's notices.
- `src/render.rs`: a session's events as Discord messages. Pure: events in, messages out. A reply's footer gives
  the session's total beside the reply's own cost (`TurnSubmitResult::cost_words`, theseus-c0bb).
- `src/bindings.rs` (the bindings file; `bindings.example.toml` is its format), `src/files.rs` (attachments: a
  text file as its text, any other file as its bytes up to `[tools] max_attachment_bytes`, 32 MiB, which the core
  keeps and reads, an image as an image and a PDF page by page; a file over its cap is listed; theseus-c9l6),
  `src/viewers.rs` (who can view a channel), and `src/rpc_client.rs` (the in-process protocol connection).
- **Places** (the place rule, theseus-nbsh): a `[[channel]]` with `private = true` is a private place (its session
  gets everything); any other guild channel is shared (the public tools alone). The binding tells the core its places
  as it starts (`Core::bind_places`), before it reads a message, and reads each private channel's viewers once, after
  the gateway connects (`check_private`), so health warns when anyone besides the owner can view it. It reads no
  viewers before a turn or a post, and loops stream everywhere. One walk of a channel's viewers (`runtime/audience.rs`,
  `view`) serves that read and the approval check (theseus-sgh). `/publish` (`runtime/publish.rs`) goes to the core's
  `place.publish` as the presser, which the core judges: only the owner, from a private place.
  `/prompt` (`runtime/prompt.rs`, M7 36c): `name:<server/prompt>` with autocomplete from `mcp.prompt.list` (25 choices
  at most), then a modal with an input per argument (up to 5; more is one `args` input of `name=value` lines), whose
  submit is `turn.submit { prompt }` through the place's line of turns. The core refuses a shared place's prompt.
  `/extensions` (`runtime/extensions.rs`, M7 43b): the loaded extensions from `extend.list`, ephemeral, each with a
  Revoke button (`ext-revoke:<name>`); the text and a press go through the place (`Control::Extensions`,
  `Control::Revoke`), a press as `extension.revoke` with the presser's ids, which the core judges.
  Jev's live notices (`runtime/jev.rs`, theseus-0j2.13): the operator lane posts a `jev_notice` to the owner's DM
  alone (`Lane::owner_dm`: never `to_operator`'s fallback, which may be a shared place) with right / wrong / noise,
  each a `judge.label` with the presser's `DiscordOrigin`, which the core judges (the owner, from a private place).
  A refused press tells the presser alone; a counted one comes back as the core's `jev_labeled` post, which edits
  the notice and clears its buttons. `judge.noticed` draws nothing in a place.
- **A place's session** (`runtime/succession.rs`, theseus-emqx): a place resumes its stored session when it can take
  turns and is not retired; otherwise it starts fresh, and nothing is opened at the bind, at `/new`, or when its
  session can take no more turns: its next message opens one (`Place::ready`) through `Core::bind_place_to`, which
  records the supersession both ways in the place's frame. A session the owner retired is succeeded the same way. A
  place says its bind notice only at its first bind (META `discord.bound.<place>`). Tests that submit to a place's
  session before any message name their core (`runtime::open_at_bind`), or run a debug daemon with the plant
  `THESEUS_TEST_OPEN_AT_BIND` (theseusd's outbox, tasks and wakes tests). Tests: `runtime/tests_succession.rs`.
- **The bindings file, read live** (`runtime/live.rs`, theseus-ocwt): stat'ed every 2 s, parsed only when its mtime,
  size or inode moved, acted on only when its revision did. Places diff by key: a removed one loses its routes and actor
  at once, and its lane is retired (`Shared::retired`), ending between posts and then refusing the rest
  (`refuse_unbound`); an added one starts as at a start; a changed one is updated in place (routes, actor, lane label),
  keeping its turn and its lane's messages. A file that does not load changes nothing, and health's detail says why. A
  stamp is acted on only once it held a tick and is a period old (a torn save is not acted on), a place whose bind
  failed is tried again each tick, and the DMs keep the file's order. A guild's invite check and voice channels wait for
  the next start, and the detail says so, measured from the file the start bound. The watch ends at the daemon's stop,
  as the gateway loop does: each selects against `Outbox::stopped` (core `outbox.rs`, a wait on the stop's `flight`
  watch, no polling), and `event_loop` logs "discord gateway loop ended at the daemon's stop" (theseus-9ggu).
  `Shared::refuse_unbound` holds the lanes' lock across `Outbox::refuse_unbound`, so a place re-added live cannot have
  its new lane's post refused (theseus-yduk); the lock order stays `lanes`, then `retired`, with the outbox's and the
  kernel's locks only inside `lanes`.
- **The task board and `/tasks`** (39b, theseus-ext.14): `runtime/board.rs` routes a `task.changed` to its home's
  place (`theseus_core::task_graph::home`) and sends the tree to the lane as one live upsert under
  `render::BOARD_KEY` (`render/board.rs`); `courier/board.rs` pins it once (a refusal logged once a lane) and, after
  a restart, finds the pinned board by `BOARD_HEAD` before making another. `/tasks` is the records' tree, then the
  task sessions of today. The layer-1 card is `render::board::change_card`, with Accept and Decline on Approve's
  and Decline's ids (`Buttons::Accept`, `runtime::accept_buttons`). The fake Discord pins (`refuse_pins`).
- **Guilds and ceilings** (step 38a, theseus-ext.3): `bindings.rs` reads format 1 (a top-level `guild_id`, its
  `private` beside it) and format 2 (a `[[guild]]` each, with its own `private`, and each `[[channel]]` naming its
  `guild`); a file that mixes them is refused, naming the line. `runtime/guilds.rs` tells the core each guild's word
  (`Core::trust_guilds`) and each place's guild and ceiling (`BoundPlace`), keeps them for health and the
  `discord.bound` row (`PlaceBits`), reads the bot's roles in every guild, and gives each place's session its spend
  limit at each start (`Core::place_spend`). Routing needs no guild: a channel id is unique across guilds. Slash
  commands stay global. A voice channel may be in any bound guild; its call joins in that guild.

- **"halt self"** (theseus-pw1q.2, `runtime/halt.rs`): a message whose first two words are `halt self` (any case;
  the rest is why) is `self.halt` as its author with the message's ids, in any place the binding reads (a halt is
  anyone's). "resume self" is `self.resume` the same way, and the core counts it only from an author holding an
  owner handle (`places::owner_anywhere`); anyone else's is refused and ledgered there. The week's self digest is a `self_digest` post, to the
  owner's DM alone (`jev_post`).

- **A reaction corrects routing** (theseus-q31l, `runtime/route.rs`): ⬆️ or ⬇️ on one of this process's replies
  (found by message in `Shared.replies`, which the courier fills as a turn's key lands, bounded) is `route.correct` on
  that turn, `stronger` or `cheaper`, with the reactor's place, so the core judges it (the owner, from a private
  place); a counted one gets ✅. The reply's footer says `routed: <mode>` (or `correction`). There are no footer buttons
  on Discord: a reaction does the same without a button row under every reply. The gateway asks for both reaction
  intents (not privileged).

## Invariants

- **What a person does goes through the protocol** (a message is `turn.submit`, a press `action.confirm`, `/stop`
  `execution.stop`, `/trust` `policy.trust`), so the core judges Discord as it judges the CLI and the web UI. What
  the binding delivers and reports, it reads and writes in the core directly: the outbox, the cards' questions, its
  ledger rows (Item 30).
- **What must be seen is an outbox post**, written by the core when it happens: a reply, a card, a card's settle, a
  failed turn. Live progress (streamed text, tool lines, typing) is best effort and never replayed (Item 6).
- **The reply shows before its frames are synced** (theseus-ck0n): a loop's first text goes out at once, the rest of
  its stream at the edit tick, and its whole text the moment its stream ends (`model.answered`, which the core sends
  before the settle's frame), all live ops under the post's keys, so the post, durable and exactly once, only seals
  them and adds the footer. A create's `discord.message.out` row goes through `Core::binding_ledger_soon`, so the
  lane's next write never waits on that row's sync.
- **One lane per place is the only writer of its messages**: posts first, in order, then live progress. A create
  carries a nonce from its message's key, with `enforce_nonce`, so a retry after a crash returns the first message.
- **A lane's maps are bounded** (theseus-celu.37, theseus-6809): `msgs`, `sent` and `sealed` keep the task board's
  key always, every key of a turn the place's renderer holds (its last `RECENT_TURNS`, 8; each key begins
  `<turn_id>:`, a call's notice card `<turn>:notice:<tool_use_id>` included), and the `KEYS_KEPT` (256) other keys
  named most recently (`Lane::touch`; a late live state names its key too). The place's actor sends the held turns
  (`LaneMsg::Held`, `courier/held.rs`) at each turn's start and at `/new`'s rebind; a turn that leaves the list is
  forgotten. So a lane holds at most 8 x (`max_loops`, 40 by default, x (a loop's text parts + its process message +
  its notice cards) + a footer) + 256 + 1 keys: 905 when each loop's text is one part and no card posts; a loop's
  thinking is in its process message and adds no key. The renderer's
  `emitted` and `menus` hold only its held turns' keys, and `notices` only their calls' (`render/held.rs`). Every
  insert goes through `touch` (or `seal`), or the bound leaks.
- **Today's pings by default; silence is per category, per place** (theseus-l1y1, `policy.rs`; the owner: a lively
  chat, and nothing said in channel not worth a buzz). Every `Write` says whether its create `ping`s, and a create
  that doesn't carries `flags: 4096` (`SUPPRESS_NOTIFICATIONS`; mentions notify no one either), as does a notice
  embed. With no config every chat message pings, as before. `policy::TABLE` (data) gives each write its chat
  category: `cards`, `failures` (a failed turn or task, disk below the floor), `answer` (the first text part of the
  reply to an owner's own message: `Lane::owed`, set with the anchor when the author is an owner, taken when that
  part is written, live or by the post, ping or not; a reply's post classes its first text part the answer whenever
  `owed` is still set, whatever else of the turn the stream wrote, a loop's process message included), `later_parts`, `woken`, `tool_lines` (and notice embeds),
  `reports` (a task's end, hands), `notices` (Note, Jev, Glide), `ops` (restart, MCP, low disk). A write pings
  unless its place's `silent` list (its `[[channel]]`'s or `[[dm]]`'s, else `[discord] silent`) names its
  category. A closed card (written once, settled, without buttons or mention: `detail.settled`; its settle edits
  nothing), the task board, and a loop's process message that holds its thinking (`render/process.rs::holds_thinking`:
  its top line is `-# 💭`), with its tool lines or before them, have no category and never ping. So a thinking turn
  buzzes for its answer alone, and its last loop (thinking, then the answer) adds one silent create; a loop that does
  not think pings for its tool line as before (the owner's call on the fold, 2026-10-10: no ping owed to a later
  create, which put a tool loop's buzz a loop late under the next loop's thinking). `ping_window_secs` (the place's, else
  `[discord]`'s; 0, off, by default) holds a channel to one ping in that long (`policy::Pings`, in memory, 256
  channels kept); a ping it holds goes out silent. An edit never notifies. The `discord.message.out` row says
  `ping` and `held` (the window took it). The shared policy's urgency (`theseus_protocol::notices`) decides what
  the house shows, not whether the chat buzzes.
- **A place shows its tool lines and its thinking unless its binding says not to** (theseus-l1y1,
  `runtime/show.rs`): `show_tools` and `show_thinking` on a `[[channel]]` or a `[[dm]]`, both on by default, read
  live with the rest of the file. Both live in a loop's process message, `<turn>:L<n>:tools` (`render/process.rs`, through
  `View::fold` on every renderer op): the loop's thinking (`model.thinking`) streams as `-#` lines at its top, made
  at the loop's first thinking or first tool line, whichever comes first, and folds to `-# 💭 thought for N s`
  once the loop's text starts (or the loop ends); the tool lines below are the renderer's, as without thinking.
  Its backticks and backslashes are escaped as it streams, so a code fence in the thinking never opens a block over
  the tool lines below. It shows at most what fits beside the tool lines in 2,000 bytes (1,800 characters), saying
  what it left out; it
  keeps 3,600 characters and counts the rest, for the renderer's held turns. Off hides that half in that place
  alone, and a message with neither half is never made; the turn, its cards and its reply are unchanged. There is
  no thinking message of its own (an earlier cut's `:think`), and the binding showed no thinking before
  theseus-l1y1.
- **A place answers only where its bindings file binds it.** An interaction in an unbound place gets no answer, so
  daemons on one bot token with disjoint bindings each answer their own places (Item 11). A card in a guild channel
  mentions exactly its answerers, and nothing else mentions anyone (Item 15).
- **Slash commands are bare names** (`/new`, `/stop`, `/trust`, …), and each control has one effect (Item 9).

## Tests

- `src/tests_outbox.rs` drives the binding against `theseus_sim::fake_discord` (REST: it honours a nonce as
  Discord does, and can be down, hang creates, or fail). Point a daemon at it with `[discord] rest_proxy` and
  `gateway_proxy`.
- `src/tests_silent.rs` (theseus-l1y1): which creates ping, through the stand-in's gateway (the fake keeps each
  create's `flags`; `Msg::silent`), with no config, with each category silent, with a place's own list, and with a
  30 s window, daemon-wide and a place's; a create's row is written soon after it, off the lane's path, so a test
  waits for the rows it reads as well as the messages.
- `src/tests_show.rs` (theseus-l1y1): a place bound `show_tools = false` and `show_thinking = false` beside one
  that says nothing, through the gateway. The fake provider streams a `thinking` block as `Delta::Thinking`.
  `src/tests_process.rs`: a thinking turn's creates and pings against the same turn without thinking (its answer
  the one buzz, every process message silent), the fold, a thinking answer's silent create, `show_thinking = false`
  in one place, `silent = ["tool_lines"]` in one place against a DM that says nothing
  (`a_place_that_silences_tool_lines_silences_its_process_messages_alone`), and an answer the reply's post writes
  while the thinking's create is held at the fake (`an_answer_the_post_writes_after_the_thinking_is_still_the_answer`,
  `hold_writes_containing`). `render/process.rs`'s unit tests: the stream and the fold, a tool loop folding at
  `model.answered` and not at its end after the tool ran, a fence in the thinking escaped, the cut.
  `src/courier/tests_order.rs`: a loop's thinking queued as a new message goes before the lane's waiting post, and a
  tool line alone after it (the lane run against the fake, everything queued before it starts, so load cannot
  reorder it).
- `src/tests_gateway.rs` drives it through the stand-in's gateway too (theseus-6g62): `FakeDiscord::say` types a
  message as a user, and `press` presses a button the binding posted, each sent as Discord sends it; `replies()`
  is what the binding answered each press, and each message keeps every version (theseus-qifw). A guild set on
  the fake (`set_guild`) answers the viewer check, so a card in a trusted channel is tested end to end
  (theseus-ck0k). Its core starts the continuation driver, as the daemon does, or an approved call never runs.
- `theseus-sim discord proof --theseusd <bin>` runs a typed message, a card, a refused press, and an Approve
  against a real daemon on the stand-ins in about 3 s; theseusd's `tests/discord_proof.rs` runs it in the gate.
  A step that changes the binding uses it as its live check. Slash commands, select menus, attachments, and a
  dropped gateway are not driven through the stand-in yet: tests drive those through `on_interaction` and
  `place_for_tests`.
- `split_text` (`src/render.rs`) has property tests: a message split past Discord's 2,000-character limit must
  never loop or panic.

## Traps

- **Never bind the operator's places from a second daemon.** A scratch daemon on Discord binds only a test channel,
  runs on a fresh state dir (never a copy of the operator's store, theseus-c3e), and carries no copy of the
  operator's bindings file.
- Registering commands is global to the bot: a scratch daemon's command list replaces the installed one's until the
  operator's daemon next starts.
- A test that reads the channel waits for every message it reads: the outbox draining doesn't mean the live tool
  line has landed.
- A ledger row names a Discord id as a string. Clippy's `cmp_owned` turns `row["author_id"] == ID.to_string()`
  into `== ID`, a JSON string against a number that never matches: bind the string first.
