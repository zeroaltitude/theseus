# theseus-discord

The Discord binding (spec P5, M3): text and voice channels in one or more guilds, and direct messages, in the
daemon's process. Read by theseusd.

Key modules: `runtime.rs`, `courier.rs`, `render.rs`. Read by: theseusd.

## What's here

- `src/runtime.rs`: the gateway loop and the places (a text channel or a DM, each backed by one session), with the
  slash commands and the confirm buttons. `Routes::resolve` finds a message's or an interaction's place.
- `src/courier.rs`: durable delivery, the binding's side: one lane per place, and one for the operator's notices.
- `src/render/wakes.rs` (theseus-lherl): `/wakes`, each wake's title, a series' weekdays and end.
- `src/render/preview.rs` (theseus-z6au): an edit's approval card carries its hunk, the question's `preview`, as a
  ```` ```diff ```` block of at most 800 characters with a marker where it was cut. The tool lines are unchanged:
  `summarize` is `theseus_protocol::term::words::argument`.
- A task's report (`render::report`) names the task's own thread, channel or forum post, an index card included, on its first line as `<#id>` when it has one (`Shared::place_of_task`: `TaskThreads::place_of_task`, then `TaskPosts::post_of`; theseus-ypnxt.5, theseus-90qqy), so a tap opens it on the phone. A reply's footer ends with `[inspect](<url>)` (theseus-x9swu): a masked link whose angle brackets keep Discord from drawing an embed,
  from the result's `inspect_url` (`[cockpit] public_url`, else `http://localhost:<[web] port>`), on the footer's one line: no part, message or ping is added.
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
  `/orders` (`runtime/orders.rs`, theseus-a7vi; `add:<text> [kind:] [surface:]`, theseus-slvl.4, is `standing.add` as the presser): the owner's standing records from `standing.list` as the presser
  (the core gives the text of a preference or a fact only to the owner in a private place), ephemeral, each with a
  Revoke button (`orders-revoke:<id>`); a press goes through the place (`Control::Unorder`) as `standing.revoke` with
  the presser's ids, which the core judges. The command is registered after `/extensions`, before the voice ones.
  `/thread new slug: [title:]` and `/thread list` (`runtime/thread.rs`, theseus-nhew): `thread.new` and `thread.list` as the presser
  (ephemeral), which the core judges: threads are private always, so only the owner, from a private place, makes or lists
  them. Registered after `/orders`; answered in the `revokable_interaction` chain. `/thread link a: b:` and `/thread unlink a: b:`
  (theseus-98bz.12; `thread_link_said`) are `thread.link` and `thread.unlink` as the presser, which the core judges the same way.
  **The link card** (`runtime/seal/link.rs`, `Buttons::Link`): the core's `link_card` post (a thread-link proposal, to the owner's
  DM only) gets **Link these threads** and **Not needed** (`link:<yes|no>:<here>:<other>`, at most 90 chars); a press is
  `thread.link` as the presser with its place, so a copy pressed in a shared channel is refused and writes nothing; **Not needed**
  writes nothing (the core marked the pair asked when it posted). Tests: `tests_gateway/link.rs`, `seal/link.rs`'s.
  **Threads on Discord** (theseus-sjzm; `runtime/thread_bind.rs`, `runtime/guild_thread.rs`, `courier/persona.rs`,
  `courier/thread.rs`). `/thread bind slug:` and `/thread unbind` (`runtime/thread.rs`): a channel the bindings file names
  private, or a Discord thread under one (or under `[threads] discord_channel`), runs the thread as its place (the core
  judges the presser: the owner, from a private place; for a Discord thread that is not a place yet it judges from the
  parent). The thread's `surfaces` records it (`thread.edit { surfaces }`; one Discord surface to a thread), a shared
  place refuses, and `[[discord.thread]]` (config) binds a named channel when its thread exists. A bound place takes the
  thread's session (`Core::thread_place_session`, in `Place::ready`) and its posts (`Outbox::place_bound`, in memory,
  asserted again at each start by `revive_thread_lanes` and `start_threads`); its own plain session is left as it was
  (never superseded), so `unbind` gives it back, and `/new` there says so. `State` (`Shared.threads`) maps channel to
  slug. **The DM**: a message that begins `thread:<slug> ` (provisional until `thread-route`'s explicit form joins) or
  replies to a message carrying a thread's tag (`Shared::thread_of_reply`, the one lookup `thread-route`'s rule (c)
  calls: message, turn, thread, in `Replies`) is `Inbound.thread`, run by `Place::thread_turn` as `turn.submit {
  thread, speaking { discord, answer_at: the DM } }`, answered in the DM by the outbox post (no live stream), and
  pointed at from the thread's surface by a quiet notice (`-# from the DM: <first 80 chars> · <link>`, body `quiet`,
  never a ping; kept in `State` until the thread has a surface). A thread's reply in the DM opens with `-# → <title>`
  (`render::thread_tag`); in a bound place it carries none. **One Discord thread per activity thread**: with `[threads]
  discord_channel`, `thread.changed { opened }` (the first message, from anywhere) makes one (a text channel's public
  thread, a forum's post), run as a dynamic place (`dynamic_lane`, `PlaceRule::bind_more`, private) and recorded in
  `surfaces` (`Core::thread_surfaces`); pause and done archive it, resume and reopen unarchive, an edit renames
  (🔒 first when sealed). The syncer applies changes in the order heard (`guild_thread::run`), and `guild_thread.rs` holds
  the one helper for making, archiving and naming a Discord thread: a row that wants a Discord thread uses it. **Persona**:
  a text part in a place that runs a thread with a persona is signed (`courier/persona.rs`): a channel webhook sent under
  the persona's name where the bot may manage webhooks (found or made once a lane), else a bold name at the head of the
  loop's text; a DM's is always the bold name, under the tag. A reply to a message this binding posted counts as a reply
  to the bot even when a webhook wrote it. Tests: `tests_gateway/threads.rs`; the stand-in's threads, webhooks and
  `command` are `theseus_sim::fake_discord`.
  Jev's live notices (`runtime/jev.rs`, theseus-0j2.13): the operator lane posts a `jev_notice` to the owner's DM
  alone (`Lane::owner_dm`: never `to_operator`'s fallback, which may be a shared place) with right / wrong / noise,
  each a `judge.label` with the presser's `DiscordOrigin`, which the core judges (the owner, from a private place).
  A refused press tells the presser alone; a counted one comes back as the core's `jev_labeled` post, which edits
  the notice and clears its buttons. `judge.noticed` draws nothing in a place.
- **A proposed seal's card** (theseus-f2g0, `runtime/seal.rs`): the core's `seal_card` post ("🔒 This looks embargoed: <reason>")
  goes to the owner's DM alone (`jev_post`) with **Seal this thread** and **Not needed** (`seal:<yes|no>:<slug>`,
  `Buttons::Seal`); a press is `thread.seal` or `thread.seal_dismiss` as the presser, with the place it came from, so the
  core judges it (the owner, from a private place); a seal's reply to the presser says what had already crossed into other
  threads. `/thread list` marks a sealed thread 🔒 (🔒? while a seal is proposed).
- **The thread router's question and `/move`** (theseus-rha2, `runtime/thread_ask.rs`): the core's `thread.asked` is posted
  by the place's actor as one line with a button for each choice (`thr:<ask id>:<key>`); a press is `thread.answer` as
  the presser (the core judges it: the owner, from a private place) and the card becomes the answer (an update-message
  callback). The card is best effort: with none, the message runs at home when `[threads] ask_wait_secs` ends. `/move
  thread:` is `thread.move` for the last message the core placed in this place. Tests: `tests_gateway/thread_ask.rs`.
- **A question with options** (theseus-j7cje; `render/ask.rs`, `runtime/ask.rs`, `courier/ask.rs`): the card of an `owner_ask` question
  draws its options numbered with one button each (`Buttons::Ask`, `ask:<n>:<question id>`, five to a row), and a press is
  `work.answer` as the presser, which the core judges; "something else" takes words, so it is answered from the CLI.
  **The needs-you cards** (theseus-suvpe; `[notify] dm_cards`, on; off, the DM gets the one post of before byte for byte):
  the core's `needs_you` post (an open question past `[notify]`'s line) goes to the owner's DM alone and is **one card per
  question**, in three plain lines from the request's structured fields (what it wants, why it asks, what yes does), with
  `Buttons::Card`: Approve and Decline, or an ask's options,
  and **Details** (`need:details:<question id>`, `runtime/ask.rs`), which shows the presser alone the whole request from
  `confirm.list` as the presser (`render::question_details`). Approve and Decline are the classic card's own
  `confirm:` presses (`action.confirm` as the presser), an option is `ask:<n>:<id>` (`work.answer`); the core judges each.
  Several due in one pacer window are a header (`🔔 3 questions need you`, which pings) and a card each (posted silently,
  "2 of 3"); the cards after the first never ping, and the pacer's one post a window is the header. Every card is settled
  by the question's settle, whoever answered (a press, a reply, the CLI, the web UI). **A reply answers** (`runtime/reply/
  answer.rs`): a Discord reply to a card or a header whose whole text is a short form (`yes y approve ok`, `no n decline`,
  `trust`, an ask's number or its choice's text; to a header `approve 2`, `no 1`, `yes all`, `no all`, and a bare `yes` when
  one is open) is `work.answer` as the replier, with their `DiscordOrigin`, so the core judges it as a press; a counted one
  gets ✅, a refused one a reply saying why, and a bare `yes` to a header with several open lists them. Any other text is a
  turn as ever. The message-to-question map is in memory (`Replies::asked`, planned by the lane, landed by the courier's
  write hook), so after a restart a reply to an older card is an ordinary message. Tests: `tests_gateway/ask.rs`,
  `tests_gateway/cards.rs`, `runtime/reply/answer.rs`.
  Capture's cards (`runtime/capture_card.rs`, theseus-slvl.5): the core's `standing_card` post goes to the owner's DM
  alone, as the seal card does (`Buttons::Standing`): a record `directive.v1` kept, with **Undo**
  (`standing:undo:<std id>`, `standing.revoke`), or an ask with **Keep**, **Keep everywhere** (when it would hold
  narrower) and **Not this** (`standing:<keep|everywhere|not>:<ask id>`, `standing.answer`), each as the presser with
  their `DiscordOrigin`, which the core judges. Only a Discord-origin message makes a card; the CLI and the web UI hear
  the narrative line. Tests: `tests_gateway/standing.rs`.
  **Live owner-card questions** (theseus-6f8t0, `runtime/card_question.rs`, `Buttons::CardQuestion`): the core's `card_question` post
  (kind in `pacer::UNPROMPTED`; `jev_post`, the owner's DM alone) has **Keep**, **Edit**, **Not this** (add, pattern) or **End it** (stale)
  (`card:<keep|edit|not|end>:<cdp id>`); a press is `card.answer` as the presser with their `DiscordOrigin` (`not` is option `decline`),
  **Edit** opens a modal of the proposal's words (read by `card.list` as the presser, at most 160 characters; `card:m:<id>`), and a counted
  answer removes the buttons (an update-message callback) and is told to the presser alone. Tests: `tests_gateway/card_question.rs`.
- **Reminders** (theseus-8ux6x, `runtime/remind.rs`): the core's `reminder` post (a wake's note as it is, `Event::Reminder`, in no
  ping category, so it always pings) goes to the place the core chose, with Done, Snooze 10 min, Snooze 1 h and Tomorrow 9:00
  (`wake:<answer>:<wake id>`, `Buttons::Reminder`); a press is `wake.done` or `wake.snooze { fired }` as the presser, answered in the
  message (its buttons go, a line says what happened). `/remind when: what:` is `wake.create` with no session, `when` with
  autocomplete ("in 1h", "tomorrow 9:00", "fri 9:00", and what was typed). Tests: `runtime/remind.rs`, `tests_gateway/remind.rs`.
- **`/why`** (`runtime/why.rs`, theseus-ighf): what this place's session's last turn was shown and why, ephemeral
  (the deferred acknowledgement carries the flag), the short form of `theseus_protocol::why` in a code fence within
  2,000 characters (each section's top three and a count of the rest, never a second message). A place whose class is
  not private is told that an imported or another session's note was there, and shown none of its words. A read of
  the core: no frame, no ledger row (not even `discord.command`'s). Tests: `runtime/tests_why.rs`.
- **`/digest`** (theseus-qkoi3, `runtime/digest.rs`): `work.digest` as this binding, naming the place's own session,
  ephemeral, the protocol's summary and a line each within 2,000 characters (the oldest are counted, not shown). Discord's
  last read is this process's (the digest's end, by session); after a restart the first one reaches back a day. A DM
  whose owner starts typing after four quiet hours (`WELCOME_AFTER_MS`, from this process's last sight of the DM) is
  welcomed back with one notice line when the digest is not empty: it answers the owner's own typing, so it is no
  unprompted post and uses no pacer. `runtime.rs` is at its ceiling: the typing event goes through `Place::typed`, the
  command is registered beside `/why`'s, and `/digest`'s interaction follows `/why`'s in the chain (`why.rs`).
  Tests: `runtime/tests_digest.rs`.
- **App emojis, the posted digest and the catch-up** (theseus-ss36w; the owner's target: always in the loop on status, with
  the chat as lively as before and the digest and the catch-up single messages).
  - **The bot's own emojis** (`runtime/emoji.rs`; art in `assets/emoji/`, made by `make.py`, standard library only, committed):
    a spinning `theseus_working` and a pulsing `theseus_needs_you` (GIF), `theseus_done`, `theseus_failed`, `theseus_sleeping`
    and `theseus_job` (PNG), 128x128 and far under Discord's 256 KB. After READY (`emoji::on_ready`, one call in
    `runtime.rs`, the work in a task) the application's emojis are listed (`GET /applications/{id}/emojis`), the missing ones
    added (so one deleted is added again at the next READY), and the table and META `discord.app_emojis` are what the list
    says; the kept ids draw from READY until the list answers; a list that fails clears the table. **Every site that draws a
    state asks one function**, `emoji::state(WorkState)` or `emoji::job()`, which gives the application's emoji or the Unicode
    glyph it always drew (`⚙️ 🔴 ✅ ❌ 💤 🔧`; `⛔ ⌛` have no art), so a render with the emojis off, or before they are
    known, is byte for byte the old one. Select-option and button labels stay Unicode (Discord draws no custom emoji in
    them). The table is the process's: run tests under nextest, as the gate does, so a test that fills it never shares a
    process with a golden. `[discord] app_emojis = false` never starts it. The stand-in's routes are
    `theseus_sim::fake_discord::emojis` (`app_emojis()`, `refuse_app_emojis`, `drop_app_emoji`, a 256 KB check).
  - **The posted digest** is the core's (`theseus-core`'s `initiative/digest.rs`, kind `digest`, in `pacer::UNPROMPTED`); the
    courier posts it to the owner's DM as it posts a Jev notice (`jev_post`), in `/digest`'s words (`theseus_protocol::digest_text`).
  - **The catch-up** (`courier/catchup.rs`, `Lane::catch_up`): a lane that comes back from `away()` with three or more
    waiting posts that would be silent (a report, a turn's failure or a plain notice whose category is silent in the
    place) and were written since the first post that met the outage, sends one silent message ("While Discord was
    unreachable (14:02–14:19): 2 tasks reported, 1 failed: …"), each folded post settled with its id (`folded_into`).
    Never folded: a card, a needs-you question, a settle, a reply (so the owner's answer, owed or not), a reminder, a
    Jev notice, a post that may have landed (`unsure`). The fold's key is its oldest post's, the posts are dispatched
    before it and settled after it, newest first. Tests: `courier/tests_catchup.rs`.
- **A message sent while a turn runs** (theseus-qu1lc, `runtime/steer.rs`): with the core's `[kernel] steer_queued` on
  (the default), a place whose turn is in flight sends the new message as a `turn.submit` at once instead of holding it
  in `queued`. The core's running turn takes it in at its next loop (or, if that turn ends first, runs it as the next
  turn), and the place's renderer shows the turn's events as it does; the place's `inflight` stays the turn that was
  running, and the steered request's end (`PlaceMsg::Steered`) says only a failure. Off, the message waits in `queued`
  and goes with the rest after the turn, as before. Tests: `runtime/tests_steer.rs`.
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
  and Decline's ids (`Buttons::Accept`, `runtime::accept_buttons`). The fake Discord pins (`refuse_pins`). This text
  board is the fallback: the live board below replaces it while `[discord] live_board` is on and the bot can send V2.
- **The live board, the needs-you card and the status line** (theseus-2j2ka; the owner's target: lots of tasks going,
  always in the loop on status and on when feedback is needed). The chat stays as lively as before (`policy.rs`'s
  table is unchanged: thinking and tool lines on, every chat message may ping); this is **task status only**.
  - **The binding hears the engine** (`runtime/tasks.rs`): `work.watch` from the binding's own connection
    (`interactive: false`), roots named (the Discord surface reads no private text; the core lets it watch a session
    that speaks in a Discord place of any class, and the Reminders Home, `rpc/work_board.rs::may_watch`, scrubbed, `may_answer` false; the owner's DM place adopts Home's tasks once it has a session (`Tasks::set_home`, asked again with every route change), since a CLI task started before the DM had a session lives there and still gets its post and its row, theseus-31pgz), kept by
    `work.changed` under the position rule, asked again after `events.lost` and whenever the places' sessions change
    (`Tasks::rewatch`, once for a burst). A view goes to the place of its root's conversation (`PlaceMsg::Tasks`), which
    draws its own board. Nothing is stored; "since you looked" is one in-memory time a place (`Clear done`).
  - **The board** (`render/board_v2.rs`, `render/v2.rs`, `runtime/live_board.rs`, `courier/v2.rs`): one Components V2
    message a place, pinned, **edited and never posted anew**, so it never pings; a header with the counts (a task that ended is counted by how: `✅ 2 done · ❌ 1 failed since you looked`, `board_v2::headline`; theseus-90qqy), **Needs you**
    (title, short id, the question, `[ Answer ↗ ]`), **Now** (what it does, then the one progress line: `step 3 of 5 · $0.42 of ~$1 (limit $2) · ~6 min left`; `turn N` only with no plan; a paused task shows ⏸️ and `paused` first; theseus-ypnxt.2), **Done, not yet
    seen** (24 h, until Clear), `[ ✓ Clear done ] [ 🔕 Quiet 1 h ]` and a select "Open a task…". Times are Discord's
    `<t:…:R>`, which tick by themselves, so a quiet board is never edited for the clock. At most 40 components and 4,000
    characters (`v2::validate`): a part folds its tail into `+N more · /tasks` (needs you keeps the longest waiting
    first, now and done their newest). The lane writes it at most once in `BOARD_EVERY` (2 s: the latest state
    replaces those waiting; the worst minute is 30 edits and one create a place), finds its pin after a restart
    (`find_board_v2`: a bot message with the V2 flag and the Clear button), and writes a place's *idle* board (nothing
    to show: `LaneMsg::BoardIdle`) only where one is pinned, so no place gets a board for nothing and a restart cannot
    leave "3 running" behind. A V2 write's `content` is its components as JSON with `Buttons::V2` (the lane's
    `sent` dedup and the nonce follow). Discord refusing V2 (a 400) flips the process to the text board and the
    classic card (`Tasks::refuse_v2`), said once.
  - **The task commands** (theseus-a5uds; `runtime/task_cmds.rs`, `answer_cmd.rs`, `follow.rs`, `start_task.rs`, `render/snapshot.rs`,
    `render/answer_v2.rs`). Every one answers ephemerally or edits, so it adds no noise, and its callback goes out on `Shared.interact`,
    a second HTTP client with no rate limiter, so a busy lane never delays a three-second deadline.
    `/tasks [show: open|needs_you|working|done|all]`: the place's snapshot (`snapshot::render`) with `↻` (`tb:refresh:<show>`, edits it in place). `open` (the default) is the board from memory; `done` and `all` take the finished tasks from the store (`task.list` for the place's target, `tasks::finished`, `render/history.rs`: the ten newest with how they ended, when, `$`, the report's first line and the post or channel, then a count), whatever the board cleared or a restart forgot (theseus-ypnxt.5).
    `/task name:` (autocomplete from `Tasks`, in memory: the place's tasks, needs-you first, then working, then ended, at most 25, `new` first
    when it matches): Details (`task_view::detail`, which now also has **Follow** and, for a failure, **Retry**/**Dismiss**); Stop asks once more
    (`[Stop d4e5f6] [Keep it]`) then `task.cancel`. `task`, `cancel` and `answer` autocompletes are answered from
    `task_cmd_interaction` (reached from `prompt_interaction`'s last arm, since `on_interaction` is at clippy's complexity limit); a command is
    handed to its place (`PlaceMsg::Cmd`). `/cancel id:` autocompletes the place's open tasks and wakes by short id. `/quiet [for: 30m|1h|morning|off]`
    sets the place's pings off (`Tasks::set_quiet`, the board's 🔕 does the same) and keeps it in one META key, `discord.quiet:<target>`, read back
    when the place starts.
    **Follow** (`tb:follow:<task>`): one silent message in the place that mirrors the task's tool lines and stream (`session.watch` on a
    connection of the follow's own, private places only), edited in place, at most 3 a place, all of a place's follows together edit at most once in
    2.4 s (so one never edits more than once in 2 s, and a busy place costs 25 edits a minute beside the board's 30), sealed with the report line
    when the task ends. Its footer says `reply here to talk to the task`: a message that replies to the mirror (its id is kept in
    `Follows`, in memory, 64 of them) is the owner's word to that task (`Place::task_reply`, `join_task`'s rule: steered while it runs, the
    conversation with the shared line once it is over); a message that is not a reply is the conversation's.
    **`/answer [question:]`**: autocomplete over the open questions the presser may answer (`confirm.list` as the presser, kept for 3 s per
    presser and channel, so a keystroke asks the daemon nothing: `Questions::fetches`), then the card's controls ephemerally. More than five
    answers are one string select (`ws:<q>`; an ask with six choices has seven answers); "Decline with a note…" (`wn:<q>:decline`) and an ask's
    "Something else" (`choice:other`, from a button or the select) open a modal (`wm:<q>:<option>`): a paragraph input, and for an ask's own
    words a file upload. The submit is `work.answer` as the presser with the words as the note and a text file's text after them (read as any file
    sent here is: `files::plan`, the same caps; a file that is not text is named as not read); it is not stored as an attachment, since an answer
    has none. A refusal is told to the presser alone and the card keeps waiting.
    **Starting tasks** (`tb:new` on the board, `/task name:new`, and the message command "Make this a task"): one modal (brief, budget $1/$2/$5/$10,
    report here or silently to the board; a message made a task brings its text as an editable `quote` field) whose submit is `task.start`
    as the presser, homed in this place's session; `quotes` hold the new session as outside text **unless they are the owner's own words** — since
    theseus-nfhaf the owner's own message marks nothing, and a mark the session inherits names the source it came from. The modal's quote field is labelled
    "The message" for the owner's own message and "The message (read as outside text)" for someone else's (`start_task::modal` takes
    the quote with its `own` flag and sets the `nt:` / `nt:!` id from it; the description line is the same in both); the owner approved the
    change to this locked flow on 2026-10-25, "Yes to all" (theseus-tbm1q), and the string is in no golden. A shared place is told where to start it.
    **A reply that offers a task** (theseus-bh3t.5): the core's reply post carries `offer {brief}` when Jev found the owner's message to be
    work for later; in a private place the reply's last message gets `➕ Make this a task` (`tb:offer:<turn>`, on the sources' row,
    `Buttons::Sources`' third field), and the brief waits in `Tasks.offers` (the newest 64, kept in META `discord.offers`, so a restart keeps them). When the judge was slower than the reply, the core's `offer` post makes the lane edit the reply once to add the button (`courier/offer.rs`; nothing when the lane no longer holds the reply). The owner's press opens the same
    modal with the brief filled (`start_task::modal(quote, brief)`, custom id `nt:<turn>`); a guest's is
    told "Only the owner can start a task here." A submit that started a task edits the button off the reply. Tests: `tests_board/offer.rs`.
    Tests: `tests_board/commands.rs` (the fake gateway's `slash`, `autocomplete`, `message_command`, `submit_modal` and `pick_in_reply`, in
    `theseus_sim::fake_discord::commands`).
  - **The needs-you card** (`render/card_v2.rs`): today's approval card, its words kept (`render::card`), as one V2 box
    with a button for each of the question's `answer_options` on `wa:<last 6>:<option>`; a press is `work.answer` as the
    presser (`runtime/live_board.rs::answer_press`), so the core judges it as it judges the CLI's: a refusal is told to
    the presser alone (a follow-up) and the card keeps its buttons; a counted answer is settled by the outbox's settle
    post on every surface that shows the card (the settle edits a V2 card as a V2 box, `detail.v2`), so an answer
    from the CLI settles it too. The card is still the outbox post it was (durable, exactly once); a card posted
    before this build settles as the classic card it is. **One ping per 30 s a place** for these cards, counted only
    while the card that pinged still waits (`State::card_may_ping`: a question answered and a next one asked buzzes
    again): a burst's later cards are created silent, and when the window ends with any still open one message says
    `🔴 N questions need you` (edited to "answered" when none is). Quiet 1 h makes the place's cards silent.
  - **The status line** (`runtime/presence.rs`): the presence reads counts only, `🔴 1 needs you · ⚙️ 3 running`
    (everyone who can see the bot sees it), cleared when nothing works or waits, never twice inside 10 s (`Pacer`, a pure
    state machine; the latest wins), sent again after READY. `[discord] status_line`. The bot's About Me (the developer
    portal's text, not set by the binding): "I run your tasks. The 📋 board pinned in our DM shows what's going; I ping
    when I need you."
  - **A task's view** (`render/task_view.rs`): the select and `[ Answer ↗ ]` open an ephemeral V2 view of the task for
    the presser: its title, what it does, plan, last words (`progress.said`), the commands of its last jobs, its
    question with the answer buttons, and `Stop`, which asks once more (`tb:yes:`), then the `/cancel` call.
  - Tests: `render/v2.rs`, `render/board_v2.rs` (a golden for 0, 1, 8 and 30 tasks, a needs-you board and an all-done
    one, `render/golden/board_v2.txt`, `THESEUS_GOLDEN=write` rewrites it; and the property: any board is at most 40
    components and 4,000 characters with unique ids), `render/card_v2.rs`,
    `render/task_view.rs`, `runtime/tasks.rs` (the model), `runtime/presence.rs` (the pacer), `courier/tests_v2.rs` (the
    lane: made once, pinned, silent, then edited; the interval; 200 transitions; the pin found after a restart; an idle
    board; a refused V2), and `tests_board.rs` (end to end through the real work engine and the stand-ins: three
    tasks, one asking; an answer pressed; an answer from the CLI; a refused press; a burst; the status line; a
    task opened from the Answer button and the select, an answer pressed in its view, and Stop asking twice). The fake
    Discord holds V2 messages to Discord's rules (40 components, 4,000 characters, no `content` beside the flag,
    unique ids) in a validator of its own.
- **The future in Discord** (theseus-j90w2, horizon HZ9; owner, 2026-10-08: Theseus speaks first in the chat, boards only show).
  Private places only (the place rule: `Shared::is_private`, the core's `place_rule.class`); a shared place is told
  "That is yours alone", offered no autocomplete choice, and its board draws none of it.
  - **The snapshot** (`runtime/horizon.rs`, `Tasks.future`): one loop reads `routine.list`, `watch.list` and
    `work.agenda` (once, naming the places' sessions as the Discord surface must; a routine's own conversation is not a
    place, so a routine is read from `routine.list`, not from the agenda) and the 30-day record of the first 8 routines
    (`routine.runs`, again only when a routine's run count, last run or running changed). It runs when the work engine
    changes (`work.changed`), a `wake.noticed` is heard, the courier posts a routine's or a watch's words, or a command
    here acts, or a minute passes with none (a routine made from the CLI or a wake the model sets moves it with no event this binding hears): 300 ms after the first, at least 2 s apart, so **a busy day's worst minute is 20 reads** (each 3 calls to
    memory-only core reads) **and the board's 30 edits** (`BOARD_EVERY`, unchanged). A snapshot that changed nothing
    redraws nothing. Autocomplete (`/snooze wake:`, `/cancel id:`, `/routine pause|resume|run name:`) answers from this
    snapshot: no daemon call per keystroke, reminders before other wakes.
  - **The board** gets three sections after Done (`render/horizon.rs`, `render/board_v2.rs`): **⏰ Coming up** (the next
    5 of the agenda: Discord's `<t:…>` time, kind emoji, title), **🔁 Routines** (a line each: next run, last ✓ or ✗, 30-day
    record, `$0.86 of $10`, and `[▶ Run now] [⏸ Pause]`, ids `hr:run|pause|resume:<routine id>`), **◎ Projects** (one line each:
    a bar of steps, `3/7`, the next milestone). They fold with the rest (`render` lowers the part with the most
    components until 40 components and 4,000 characters; a routine is four components) as `+N more · /routine list`.
    A place with only a future is not idle, so a DM with a reminder gets its board.
  - **Commands** (`runtime/horizon_cmds.rs`): `/agenda [when: today|week|all]` (ephemeral, the sections of `theseus
    agenda`, a live read), `/routine add|list|pause|resume|run` (`add` is a modal: name, schedule, brief, `0.50 / 10`,
    where it posts; `list` is ephemeral with Run now and Pause or Resume per routine, drawn again after a press),
    `/snooze wake: when:` (`wake.snooze`), `/watch add|list` (`add` is a modal: question, target, condition, every, for;
    **a command check is the owner's, from the DM only, shown with its command and approved once** by `hw:ok:<token>`,
    kept in memory 10 minutes), and `/cancel id:` gains autocomplete. Each act is the core's to judge as the presser.
  - **Speaking** (all normal pinging posts through the outbox, in the routine's or watch's place, the DM by default):
    a routine run's report is headed `🔁 **Morning digest** · <t> · run 22 · 52 s · $0.04` (`Shared::routine_report`,
    from the run's `routine.run:` mark; `speak: when_notable` is the core's: a quiet run posts nothing); a failed or
    blocked run's notice (the core's `routine_failed` post, once a routine a local day, its line and `answers`) is the
    reminder post with `[▶ Run now] [⏸ Pause] [Details]` (`hz:<answer>:<wake id>`, `Buttons::Horizon`); a met watch's
    notice (`purpose: watch.met`) pings like a reminder and has no button: its wake is gone, so `work.answer` has no Done to find, and "keep watching" has no verb. A press is `work.answer` as the
    presser; the message becomes the answer, its buttons gone (a refusal is told to the presser alone and the buttons
    stay). Not settled from another surface: the post edits itself on the press that settles it, as a reminder does.
  - **The status line** gains `⏰ 16:52` (the next reminder due before the local midnight, a time and no word of it,
    `Counts.reminder`).
  - Tests: `render/horizon.rs`, `runtime/horizon.rs`, `runtime/horizon_cmds.rs`, `tests_gateway/horizon.rs` (the stand-in
    now types modals and autocomplete: `FakeDiscord::modal`, `autocomplete`, and `Reply.choices`).
- **A thread per task, and the task forum** (theseus-i3do6; the plan's §1.8, 8: guild tasks get threads, the forum is offered and
  off by default). `runtime/threads.rs`, `runtime/forum.rs`; the bindings' `[[channel]] task_threads` (default on; off in a voice
  channel) and `kind = "forum"` (`Bindings.forums`, apart from `channel`, so no place loop sees a forum).
  - **A thread.** A guild channel's place reads its tasks in the work model each time they change (`Place::sync_task_threads`,
    from `task_msg`; `Tasks::all_of`, whatever their state or a Clear). A task seen working with no thread gets one through the
    activity threads' one helper (`guild_thread_create`, with the archive duration as its argument): `type: 11` given and
    `auto_archive_duration: 1440`, named `⚙️ a1b2c3 · <title>` (`task_name`, at most 100 characters), and the channel gets one
    quiet line (`<#thread>`, `say_pointer`). The task's session then has a place of its own, started by hand (`start_task_place`:
    no `place_limit`, which would overwrite the task's carve; no bind notice; no session opened): a lane (its own rate bucket;
    the parent's ping words), its events (`route` reads `TaskThreads::place_of`, so it is in no `by_session`, which the board's
    watch roots, the broadcasts and `/new` read), and its posts (`Outbox::bind_place`, so the task's replies, tool lines and approval
    cards go there; the channel keeps the conversation's own). **The class is the parent's** (`bound_place`: privacy, guild,
    ceiling and persona, read again at each sync), so a task in a shared channel stays shared in its thread. One rename at the end
    (`✅` done, `❌` failed, cancelled or expired); the thread archives itself a day after its last word; its place and lane go
    `GRACE` (10 min) after the rename (`retire_task_thread`). A create that fails is said on the board and tried again after
    5 minutes; the task goes on in the channel meanwhile. The open threads are one META list, `discord.task_threads` (written at
    create, at the rename and when the place goes; no store format), whose lanes and bound places return at `start_lanes`
    (`revive_task_threads`) and whose place starts at the first sync (`Next::Revive`). **What lands where.** The task's events are
    heard from the moment the task is first seen (a mailbox made and `session.watch` called before the thread is created, so they
    wait in it), but anything the task did before that is not replayed (live progress is best effort, Item 6): the first tool
    line, and a loop's first words when `turn.started` came first (a delta names only its turn), may be in neither place. A
    post staged before the thread exists (an approval card the task asks in its first moments) goes where the task's records
    send it, the channel, as it did before threads. The task's **report** (`📋 Task … finished`) stays in the channel: it is the
    conversation's news, posted to the place the task reports to. The board's rows link the thread (`Parts.links`,
    `TaskThreads::url_of`, kept in memory after the place goes). A task born and over since this binding began that no sync saw running is owed its one thread or post, made in its final state (`TaskThreads::owed`, `next`'s `(None, true)`, the forum worker's index card; theseus-hzdlf); a task from before the binding began is never given one, and a task that had one this run is not given a second (`Inner.born`, task -> birth ms; an entry older than `DONE_SHOWN_MS`
    plus the row's grace with no `made` row is dropped at the next add, theseus-stpqg). `Tasks::snapshot` drops an ended view older
    than `DONE_SHOWN_MS` and applies a `work.watch` answer only if its ticket (taken before the call) is newer than the last applied
    (theseus-0mq02, theseus-x4s7g). Tests: `tests_task_threads.rs`, `runtime/threads.rs`'s, `runtime/threads/tests_owed.rs`, `tests_task_owed.rs`.
  - **A message in the thread** is an operator message to the task (`Place::task_message`, then `Place::join_task`, which a
    reply to a Follow mirror shares; theseus-esxyb): `turn.submit` on its session. **The owner's goes to the task whatever it is
    doing** (the core steers it into a running task at its next step); a guest's goes there only while the kernel's rule takes it
    (a waiting or blocked task is woken), and while the task runs, or once it is over (`execution_*`), it goes to the conversation
    that started the task with the shared redirect line (`theseus_protocol::work::task_redirect_line`), and the thread says so: a
    message is never lost. The owner is `[places] owner` (else the DMs' users). The thread's first line links back to the channel
    (`↩ back to <#channel>`). `/new` in a task's thread is refused: the thread is the task's.
  - **The forum.** The worker (`forum::run`) is the only thing that talks to a forum. Each task of an eligible place is a post
    (`takes`: a private forum shows every task, a shared one only the shared places' of its own guild): the first message is the
    task's card (`task_view::post_card`, no button that acts; a link to its thread), edited on change at most once in 2 s; the six
    tags `needs you`, `running`, `done`, `failed`, `paused` (a paused task, `running` again on resume) and `stopped` (cancelled or expired) are made once if the forum lacks them, so a forum made before them gains the last two at its first use; a tag edit is made at most once in 5 minutes
    (`Pace`/`wait`) and the final state at once. The DM's board row, and a channel's, link to the task's post or thread
    (`Parts.links`). Open posts: META `discord.task_posts`. Tests: `forum.rs`'s and `tests_task_threads.rs`.
  - **The busy day** (10 tasks, 200 transitions): a thread costs one `GET` of its channel, one `POST`, one quiet notice and one
    `PATCH` rename, so 40 requests a day; the worst minute (ten tasks started in it) is ten of each create and notice, and the
    transitions cost no request on the thread side. On the forum side a post is written at most 30 times a minute (its card), so
    200 transitions are at most 200 card edits in that minute and 10 tag edits over 5 minutes.
- **A channel per task** (theseus-bh3t.1; the owner, 2026-10-17; `runtime/task_channels.rs`, `tests_task_channels.rs`, the fake's
  `fake_discord/channels.rs`). A task whose place is private (the DM, or a channel bound private) gets a private text channel
  under the bot's `⚙️ Theseus tasks` category in the one guild the bindings file trusts (`private = true`; none or several:
  nothing changes) when `qualifies` says so: a plan of two steps or more, `expect = long` (`task.create`'s and the new-task
  modal's "Expected"; a META key `task.expect.<session>`, no store format), or still working `task_channel_after_minutes`
  after it began (one `sleep` a task from `Place::promote_later`, which sends the place its next sync and marks the task due; no
  periodic timer). A task in a shared channel keeps its public thread, because its guests are there. The channel is a row of
  `TaskThreads` like a thread (`Made.kind` is `Channel`; `Made.thread` holds the channel's id), so `task_message`, `join_task`
  and the board's links work unchanged: the owner's message there steers the task. The parent gets one quiet line
  (`📺 <#channel> · title`), the Follow mirror `-# in <#channel>`; a task that had a thread is moved (the thread says `moved
  to`, is archived and its place goes after `GRACE`, the session's posts rebound to the channel). At the end the report is
  said in the channel and one PATCH moves it under `⚙️ Theseus tasks · done`, names it `✅`/`❌` and replaces the whole
  overwrite set (theseus-oejh3; the owner: access is removed for everyone but the operator): every overwrite but
  `@everyone`'s View Channel deny, the operator's read (no Send Messages) and the bot's goes, a war-room guest's and
  a hand-added role's included, the guest list is cleared and a later `share:` refused. A member with Administrator,
  and the guild's owner, see every channel whatever the overwrites say: Discord allows nothing narrower; a sweep at the start and once a day deletes channels that ended `task_channel_keep_days` ago (a full done
  category loses its oldest). A 403 turns channels off until the next start (the owner's DM gets one line naming Manage
  Channels and Manage Roles, with the invite link of `INVITE_PERMISSIONS`); the cap and any other failure leave the task as it
  was. Tests: `tests_task_channels.rs`, `runtime/task_channels.rs`'s timer test on the paused clock.
- **The war room** (theseus-bh3t.4, `runtime/war_room.rs`): `/task name:<task> share:<@user>` or `unshare:<@user>` (two optional USER
  options beside `name`; the owner's alone, from a private place, answered ephemerally) brings a guest into a private task's place to watch
  and comment. A task with a text channel of its own gets a member overwrite (view, send, read history; unshare deletes it); a task thread
  adds the guest as a thread member when they can already see its parent (`Shared::view`), else the share is refused, and an unshare says a
  public thread stays visible to whoever sees its channel. The guests are META `discord.task_guests.<task>`. **A non-owner's message in a
  private task's place is a comment** (`Place::comment_on`, from `task_message`, before `join_task`; `Made.private`): never a `turn.submit`,
  a steer or a redirect, shared or not. It is kept in META `discord.task_comments.<task>` (a total and the last 50, text clipped to 500),
  relayed once to the owner as a quiet notice in the parent place (`-# 💬 <name> on “<title>”: <80 chars> · <jump link>`), the guest's
  first one is answered by one quiet line in the task's place, Details lists the last five (`task_view::detail`), and the report's facts
  line counts them (`render::report`'s `guests`). Guest text reaches no session, board line, digest or ledger row (the row has `chars`).
  A shared place's thread keeps `join_task`'s guest rule (redirect to the conversation). At the task's end the guests are out (theseus-oejh3): a channel's close replaces its overwrites, a thread's guests are removed
  from it while it is still open (Discord keeps an archived thread's members), `discord.task_guests.<task>` is cleared, and
  `share:` answers "That task has ended; its place is yours alone." Tests: `tests_war_room.rs`; the stand-in's
  `PUT/DELETE` overwrites and thread members are `fake_discord/overwrites.rs`, and `slash_with` sends USER options.
- **The default forum** (theseus-bh3t.6, `runtime/default_forum.rs`; `[discord] forum`, on, and `forum_channel`, `theseus`, an id or a
  name): one private forum in the owner's guild, the complete index of tasks. It acts only when the bindings trust exactly one guild.
  `Shared::ensure_forum`, at the start and at each READY, finds it (the META record `discord.default_forum`, kept while `forum_channel` is
  what it was resolved from, so a renamed forum is still the forum; else a configured id, which must be a forum of the guild; else
  the first forum of that name) or makes it: type 15, `@everyone` denied View Channel, each owner and the bot allowed (`OWNER_BITS`,
  `BOT_BITS`), the six state tags. A 403, or an id that is no forum, turns it off for the run with one quiet line to the owner
  (`to_operator`); a network failure is tried again. The state is a `watch` (`Stand`): a task that would be homed waits at most 30 s for it.
  **Every task gets one post**, made only by the forum worker (`forum.rs`). `home_of(forum_homed, is_channel_place)` says where a new task
  works: `Post` (the DM's and a private channel's tasks: the post is a row of `TaskThreads` as a thread is, so steering, `/new`'s refusal,
  the end's rename and a restart work unchanged; `TaskPosts::make` asks the worker for it), `Thread` (a shared channel's) or `None`.
  Anything but `Post` gets an **index card** from the worker (its card links `TaskThreads::url_of`, its tag follows the state to the
  end, its channel is routed to the place that told of the task so that `index_of` can answer a message there with one quiet line and
  start nothing). `TaskPosts::moved` is the seam for a task promoted to a channel of its own. Activity threads are untouched
  (`[threads] discord_channel` alone). A tag edit carries `archived: false`. Tests: `tests_default_forum.rs`, the fake's
  `fake_discord/forums.rs` (`refuse_forum`, `forums_made`, `rename_forum`, `archive_thread`), and `forum.rs`'s.
  **A read post leaves within the hour after its day** (theseus-wy8pt, `runtime/forum/archive.rs`): the archive has its own tick
  (`ARCHIVE_TICK`, one hour, spawned by `sweep_task_channels`; `sweep_once` still archives at the start and daily), and a post
  also counts as read when a board cleared since its task's own end (`Tasks::cleared_since_end`, kept in `read_ms`), so a Clear
  done pressed before the forum worker wrote the end is not lost. A guild watched from a file (`watch_guild_file`, as the scratch runs
  use) is read again at every request and keeps the forums the bot made (`keep_forums_set_guild`; theseus-p5g63: the stand-in once
  dropped them, so a recorded forum 404ed and a second was made). The approval card's plain words are `render/approval.rs`
  (theseus-ono8g): the owner-approved layout, locked with a golden in `render/approval/tests_approval.rs`; a request with no `plain`
  keeps `approval::classic`.
- **The task home** (theseus-xlxp4, `runtime/task_home.rs`, `tests_task_home.rs`; `[discord] task_home`, `theseus-tasks`, a name or an id, `""`
  is off, the rollback: the DM's board, reports and pointers are then byte for byte as they were): one private text channel (type 0, the forum's
  `OWNER_BITS` and `BOT_BITS`, `@everyone` denied View Channel) in the one trusted guild, the dashboard of the owner's tasks. Found as the forum is
  (`Shared::ensure_task_home`, at the start and each READY: META `discord.task_home` while `task_home` is what it was resolved from, else a
  configured id, else the first text channel of that name, else made; a 403 or an id that is no text channel is one quiet line to the owner and the
  home is off for the run). Its lane and bound place (private, the DM's ceiling) return at each start from that record whatever the switch says
  (`revive_task_home`), so a report staged for it is delivered. **The seam is the core's** (`Outbox::home_tasks`, set in memory by
  `register_task_homes`): a task opened from the owner's DM reports to the home, so its `reports_to`, the `task.place.<session>` record and the
  report's lane all name it; the core does not know Discord, the binding only says where tasks started from a place report and how to tell a model
  (`task_create`'s result: "reports to #theseus-tasks and its forum post when it finishes"). The DM place's live board goes to the home's lane
  (`Place::board_lane`: one pinned board there; the home's channel is routed to the DM place so its presses and a message typed there are heard,
  and a message there starts nothing: one quiet line), and each task's forum post is pointed at by one quiet line (`🧵 <#post> · title`):
  the DM task's from `make_task_thread` (instead of the DM), an index card's from the worker (`Worker::card_made`). A text channel with task
  threads keeps its own board, reports and pointers.
  **A voice place is never a task's home** (theseus-lxfz0; always, whatever `task_home` says): a task started from a voice binding reports to
  the home, else the owner's DM; its board is never drawn in the voice chat (`board_lane` is none for a voice channel,
  `TaskHome::set_voice`, re-read with the bindings file); its forum post is its index card. **An index card is the place its session's cards go
  to** when the task has none of its own (theseus-90qqy): `bind_card_place` starts a lane for the post, binds it with the parent's class and
  ceiling and `place_bound`s the session, so the approval card the core stages for the task lands in its post, never in the parent channel
  with a mention; `Post.bound` (session, parent place) lets a restart bind it again (`revive_card_places`). A card staged before the card
  exists goes to where the task reports. The stand-in lists the text channels the bot made outside a category (`guild_channels`), so a restart
  finds the home by name.
  **A board the DM drew before the home answered** (theseus-ysp2k): when the home first stands, `ensure_task_home` sends the DM's lane
  `LaneMsg::BoardMoved`, which drops that lane's board message (`Lane::board_moved`, `drop_stale_board`) and any state of it waiting. A place that
  drew to the DM in the instant between the state and this message may leave one more there (not handled). A lane that has no board of its own (a restart: its id went with the process) looks once among the DM's pins for the bot's board, V2 or text, and removes it (theseus-bs8dl; `Lane::earlier_board`); the rig reaches the binding's own state through `task_home::daemon_for_tests`.
- **The DM task flow is locked** (the owner, 2026-10-22; theseus-hd8l2): `tests_dm_flow_golden.rs` drives one DM task end to end and compares
  every message with `src/golden/dm_flow.txt`. Change the golden only with the owner's OK (`THESEUS_GOLDEN=write cargo nextest run -p theseus-discord dm_flow`).
- **The one message a content label adds** (theseus-9ymz0, the only approved addition to that flow): when the core raises a label it calls `Core::tell_label`, a
  `content_label` post in the session's place; the courier renders it with one **Remove** button, `content:remove:<session>:<flag>` (`runtime/content.rs`:
  `parse`, `buttons`, `content_press`). A press is `content.unlabel` as the presser, judged by `judge_act`, which `/trust` also goes through (it is this press for `injection`, typed); a counted press settles the
  message, a guest's is refused and told alone. The DM golden gets one section at its end for it (recorded apart, so every locked line is as it was);
  `tests_gateway/content.rs` drives the press.
- **The mode on the board line** (theseus-lyxy): the task board's first line ends `· mode: plan` when the place's session is
  not in `ask` (`render::board`'s `mode`; no key, no command: Discord only shows it), and a `session.mode` notification
  redraws that board (`runtime/board.rs::routed`). A place with no task has no board, so shows none.
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

- **"self digest" and "self log"** (theseus-pw1q.19, `runtime/self_word.rs`): a message of exactly those two words (any
  case) is `self.digest` (the text the weekly `self_digest` post carries, `rsi::digest_capped`, for the week so far)
  or `self.log` (the last ten changes, newest first, one line each) in a place the place rule calls private (the
  owner's DM, a channel bound private); any other place is told "The self log is private." and nothing more. With
  `[self] mode = "off"` the answer begins "Self-improvement is off. Nothing has run." and still lists the rows so far.
  A read of the core: no frame, no ledger row. `runtime.rs` is at its ceiling, so there is no `/self` slash command
  yet. Tests: `tests_gateway/self_word.rs`.

- **A reply's cited sources** (theseus-ejdk, `render/cite.rs`, `render/reply.rs`, `runtime/source.rs`): the reply's
  `result.cites` (the core leaves out what a shared place may not show) become numbered marks in its text, `-#`
  footnotes under its last text part (above the footer), and one row of buttons on its last message: `Open [n]`, a
  link to the original message when the source was a Discord message in a bound channel (or this lane's own DM), else
  `Show source [n]` (`src:<session>:<handle>`), whose press asks `memory.cites` and answers the presser alone,
  ephemerally, with the excerpt. This binding uses classic components, not Components V2, so `-#` text and an action
  row are the closest equivalent. A reply with no sources is byte-identical to before. Tests: `tests_cite.rs`.
- **A reaction corrects routing** (theseus-q31l, `runtime/route.rs`): ⬆️ or ⬇️ on one of this process's replies
  (found by message in `Shared.replies`, which the courier fills as a turn's key lands, bounded) is `route.correct` on
  that turn, `stronger` or `cheaper`, with the reactor's place, so the core judges it (the owner, from a private
  place); a counted one gets ✅. The reply's footer says `routed: <mode>` (or `correction`). There are no footer buttons
  on Discord: a reaction does the same without a button row under every reply. The gateway asks for both reaction
  intents (not privileged).
- **👎 on a task that started itself is `task.not_this`** (theseus-o1h3f.2, `runtime/task_react.rs`, `Verb::NotThis`; the
  reaction-side of notify-work-feedback). It is the one control of that module's reactions that is a verdict and not a
  command, and it is **not** the card or standing "Not this" above (those are buttons answering a proposal or an ask,
  `card:not:`, `standing:not:`): it is `task.not_this` on a *task*, which cancels it, keeps the one rolling
  preference against work like it and labels the offer (the core's `task.not_this` entry). `on_reaction` (`runtime/route.rs`)
  asks `task_reaction` first. A 👎 (U+1F44E; not ⬇️, which is `route.correct`) counts only on a message that carries a task
  (`task_of_message`: a message in a task's thread or channel, a Follow mirror, a task's first message or its report line)
  **and** only when that task started itself, which `started_itself` reads from the task's graph record
  (`TaskOrigin.self_started`, which `task_self.rs` sets when the turn that made the `task.create` call held no message of the owner's or an MCP prompt: a wake's, a report's or a routine's turn; the self backlog's tasks carry it too). On any other task, or on a message
  that carries none, `task_reaction` answers false and the 👎 goes on to the next reader, the 👍/👎 verdict on a reply
  (`initiative.rate`); the tests hold that on a task the owner asked for nothing happens to it. It is never pre-added: the bot
  adds ⏸ 🛑 👀 📌 (▶️ for ⏸ while paused) and no 👎, so the owner taps the reaction by hand. From a guest, or in a shared place
  (`is_private`), the tap is a debug line and nothing else, and it is still taken (no other reader sees it); the core judges the
  owner too, since the call carries the reaction's `DiscordOrigin` and `discord:<user id>` as its `author`. The call has no
  `reason`. What the presser sees: ✅ on the message they reacted to when it went through (a task that had already ended is
  answered, and gets the ✅ too), and on a refusal or an error one quiet reply to that message (`⚠️ <the core's words>`, no ping),
  with the task unchanged. The binding posts nothing else for it. The
  core, the CLI (`theseus task not-this`) and the cockpit (Ops, `u` twice) take it on any task; only this reaction is limited to
  self-started ones, so that a thumbs-down on a task the owner asked for is never a cancel. Tests: `tests_task_react.rs`
  (`a_thumbs_down_on_a_task_that_started_itself_says_not_this`, `a_thumbs_down_on_a_task_the_owner_asked_for_does_nothing`).

- **Reply watches** (theseus-rlnbf, `runtime/reply.rs`): each message the bot hears, a place of ours or not, is told to the
  core's `watch_reply_seen` first in `on_message`, before routing; it reads only (one atomic load with no reply watch
  open). The place is `discord:channel:<id>`, or `discord:dm:<author id>` in a DM. The core writes the match and the
  notice (to a private place). Tests: the core's `watch/tests_reply.rs`, and `tests_gateway/reply.rs` through the stand-in (ben's reply in a shared #lab, the notice in ana's DM).
- **Typing** (theseus-tnky, `runtime/typing.rs`): the gateway asks for both typing intents (not privileged). A typing
  event of a person the place lets drive it (never in a channel that wants an @mention, never the bot) is told to the
  place's actor once per person per channel per idle spell, which sends `session.typing` with its session's id and
  the typist's `DiscordOrigin`; the core warms only for an owner's typing (another person's in a shared place warms
  nothing). Tests: `tests_typing.rs`, through the stand-in's `TYPING_START` (`FakeDiscord::typing`).

- **The voice channel's status and the board's line** (theseus-arp9l, `runtime/voice_status.rs`): while Theseus is in a call
  the channel's status reads `🎧 Theseus · ⚙️ 3 tasks · 🔴 1 needs you` (`PUT /channels/{id}/voice-status`, a route twilight
  does not type, so a plain `reqwest` write as the bot; `🎧 Theseus` alone when nothing works). **Counts only**, since
  everyone in the channel reads it: never a title, a path or a dollar figure. The counts are the work board's open tasks
  (`WorkKind::Task` that works) and open questions (`WorkKind::Question`), kept in a small map by one in-process watch
  (`Bus::watch_work` with the whole board's scope, interactive false: the binding's guarded `work.watch` sees shared
  places alone, and the call's tasks are the owner's). A change waits 2 s (measured from the first change since the last
  write), at most one write a minute, the last state always written, nothing written while the counts are unchanged;
  cleared at `/leave`, when the call drops and at the daemon's stop; a 403 is said once in health (`last_error`) and
  that call never writes again. `[voice] status` (default on) and a channel's `voice_status` set it. The same map gives
  `Tally::spoken`, one line of at most 200 characters put before a voice turn's words (`[The board: 3 tasks working; 1
  needs you: Beta summary asks fs.write …]`) in a private place only, never a shared one. Tests:
  `runtime/voice/tests_status.rs` (the pace on tokio's paused clock, the fake Discord's REST route and its 403), and
  `voice_status.rs`'s own.

- **Flux as the call's listener** (theseus-v774k; `[voice] stt_model = "flux-general-en"`, the default; nova-3 by config; `runtime/voice.rs`'s
  `flux_listen`, `runtime/voice/tests_flux.rs`): the call's engine gets a `FluxListen` (`Engine::with_listen`), whose sockets open
  and whose turns start only while `speech_fits` has room (`FLUX_ESTIMATE`, half a minute of Flux audio); the REST client keeps
  `prerecorded_model()` (nova-3) for the fallback, and `file.read`'s voice notes do too. Flux audio is booked like any
  transcription, at the `flux-general` price (`Event::Utterance`'s usage: the audio streamed since the speaker's last turn). The
  `voice.barge_in` row gains `stop_latency_ms` and `ended_latency_ms` when a listener's start of turn stopped the clip (from
  `Event::StopLatency`), and `voice.joined` names the `listener`. `pause_ms` and `merge_ms` apply only on the pause rules. The pump books
  `Event::Streamed` (streamed audio no turn carried: the silence fill, the keep-alive, a dropped session's tail) as a
  `speech.transcribed` row with `detail.trailing = true`.
- **The join guard** (theseus-yf9ct; `[voice] join_guard`, on): the engine's `Config::join_guard` is set from it at the join; see
  theseus-voice's AGENTS.md. Off restores the old reading of a sound at the join and of a one-word echo.
- **A call's judge of unclear transcripts** (theseus-s3dwj, `runtime/voice/judged.rs`; `[voice] judge_unclear`, on, and `judge_ms`, 250): the
  engine's `Judge`, answered by the core's `heard.v1` (`JudgeService::judge_heard`, live, one `judge.call` row a call, scoped `judge:heard`).
  It maps the engine's case onto the pack's input and Jev's answer back; the core decides nothing here. Off, no judge is given to the
  engine and its events and times are byte for byte the rules'. The `voice.utterance` row's `judged` holds the judgment id, answer,
  probability, whether it acted and its time (null when the rules decided alone).
- **A task's question and report in the call** (theseus-bh3t.3, `runtime/voice/tasks.rs`; `[voice] speak_tasks`, on): fed by the voice
  status's one work watch (`voice_status::Handle.tasks`; the watch runs when the status or `speak_tasks` is on, and `Handle.board`
  says whether the status is written and the board's line told). A new question of a task and a task's end are one `Command::Report`
  each (title and a sentence or two, no code, links, paths or tool lines, 220 characters), said once (a set of 256), never what was
  there at the join, nothing in a shared voice place or about a task of a shared place, and no end of a task that reports to the voice
  place's own session or was started from it (`Tasks::started_here`; `voice_heard` speaks that turn; a voice place is never a task's home, theseus-lxfz0). The last question said is pending 5 minutes: `voice_turn` first asks
  `voice_answered`, and an owner's whole utterance of "pause it" or "go on" (`held_words`) is `task.pause` / `task.resume` as the speaker for the task the question named while it is pending, else the private place's only running (or only paused) task, with two to choose from an ordinary turn (theseus-ypnxt.5), and one of "approve", "deny", "the second one" or "stop it" (`matched`) is `work.answer` /
  `task.cancel` as the speaker with the voice channel's ids (the core judges; a refusal is "I can't take that here." and the core's
  words in text), posts the transcript line, and makes no turn. A bare "yes" is the engine's backchannel, so the question asks for
  "approve". Tests: `runtime/voice/tests_tasks.rs`.
- **The call's voice and greeting** (theseus-slvl.7, `runtime/voice.rs`): each sentence is made in the voice of the persona in
  play for the voice place's session (`Core::voice_of`, asked at every `Metered::synthesize`, so a persona change mid-call
  is heard at the next sentence with no reconnect; `[voice] tts_voice` when the persona has none), and the sentence's
  `speech.synthesized` row names it (`voice`), as `voice.joined` does for the call's first. After the join succeeds,
  `begin` queues `[voice] greeting` (`{name}` the persona's name; empty is none) as the engine's first command, spoken
  before anything heard is answered; a join that fails speaks nothing. Tests: `runtime/voice/tests_persona.rs` (a stand-in
  Deepgram that logs each request's voice).
- **The deaf watch goes on after the call hears** (theseus-sro04, `runtime/voice/deaf.rs`): before it hears, `deaf()`
  (theseus-d93y); once it heard, `lost()` between two checks: a listed speaker present whose RTCP sender reports say they
  sent `LOST_SENT` packets (5 s of speech), none of which arrived, is lost audio, rejoined once a call as a deaf join is,
  with the `LOST` notice. Sender reports alone are no evidence: a silent speaker sends no packets and keeps reporting.
  The counts (`SsrcCount::sent`) are songbird_io's, which believes a report's count only when it grew no faster than a
  sender sends. Tests: `deaf.rs`'s own and `runtime/voice/tests_deaf.rs`.
- **"Typing…" waits for no one** (theseus-sro04, `runtime/thread_ask.rs::typing`): a place types while its turn runs, or
  while its submit is outstanding, but not while that submit waits on the router's question to the owner (a question
  in the core's `threads.route.waiting()` for the place's session). A spoken turn is never asked about at all: the
  core runs it at home (`rpc/thread_flow.rs`).

## Invariants

- **What a person does goes through the protocol** (a message is `turn.submit`, a press `action.confirm`, `/stop`
  `execution.stop`, `/trust` a `content.unlabel`), so the core judges Discord as it judges the CLI and the web UI. What
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
  category: `cards` (a needs-you card also pays the 30 s rule above), `failures` (a failed turn or task, disk below the floor), `answer` (the first text part of the
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
  channels kept); a ping it holds goes out silent. A question that waits on the owner (a card with an answer's buttons, the needs-you post, a V2 question card, the burst summary) is never held: the window is for chat, so a reply that used it first cannot leave a question unheard (`courier::asks`, theseus-31pgz). An edit never notifies. The `discord.message.out` row says
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
  while the process message's create is held at the fake, whatever its text (`hold_writes_containing("-# 💭")`):
  `an_answer_the_post_writes_after_the_thinking_is_still_the_answer` (a provider that holds the text back until the
  create is held, so the post, not the stream, writes the answer) and `..._after_the_fold_...` (every write held until
  the reply is queued: the order of a fast host, theseus-wpv2g). Each asserts its scenario happened. `render/process.rs`'s unit tests: the stream and the fold, a tool loop folding at
  `model.answered` and not at its end after the tool ran, a fence in the thinking escaped, the cut.
  `src/courier/tests_order.rs`: a loop's thinking queued as a new message goes before the lane's waiting post, and a
  tool line alone after it, and a turn whose post goes before its stream replies to the owner's message once
  (theseus-nxnqc; a newer anchor survives an older post) (the lane run against the fake, everything queued before it
  starts, so load cannot reorder it).
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
