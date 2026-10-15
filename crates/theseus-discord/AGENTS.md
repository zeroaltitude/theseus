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
- A reply's footer ends with `[inspect](<url>)` (theseus-x9swu): a masked link whose angle brackets keep Discord from drawing an embed,
  from the result's `inspect_url` (`[cockpit] public_url`), on the footer's one line: no part, message or ping is added.
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
  them. Registered after `/orders`; answered in the `revokable_interaction` chain.
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
  The earned-autonomy card (`runtime/earned.rs`, theseus-uttu): the core's weekly `earned_card` post goes to the owner's
  DM alone, as a Jev notice does, a row of Yes with a notice / Not this one / Show the record for each candidate
  (`earned:<yes|no|show>:<id>`); a yes or a no is `policy.earned.answer` as the presser, which the core judges (the
  owner, from a private place), the record is read and shown to the presser alone. Tests: `tests_gateway/earned.rs`.
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
  `work.answer` as the presser, which the core judges; "something else" takes words, so it is answered from the CLI. The core's
  `needs_you` post (an open question past `[notify]`'s line) goes to the owner's DM alone, always pings (`Event::NeedsYou` is in no
  category), has the same buttons for an ask, and is edited by the question's settle. Tests: `tests_gateway/ask.rs`.
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
    that speaks in a Discord place of any class, `rpc/work_board.rs::may_watch`, scrubbed, `may_answer` false), kept by
    `work.changed` under the position rule, asked again after `events.lost` and whenever the places' sessions change
    (`Tasks::rewatch`, once for a burst). A view goes to the place of its root's conversation (`PlaceMsg::Tasks`), which
    draws its own board. Nothing is stored; "since you looked" is one in-memory time a place (`Clear done`).
  - **The board** (`render/board_v2.rs`, `render/v2.rs`, `runtime/live_board.rs`, `courier/v2.rs`): one Components V2
    message a place, pinned, **edited and never posted anew**, so it never pings; a header with the counts, **Needs you**
    (title, short id, the question, `[ Answer ↗ ]`), **Now** (what it does, turn, `$spent of $limit`), **Done, not yet
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
    here acts: 300 ms after the first, at least 2 s apart, so **a busy day's worst minute is 20 reads** (each 3 calls to
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
    notice (`purpose: watch.met`, an answer `done`) pings like a reminder with `[Done]`. A press is `work.answer` as the
    presser; the message becomes the answer, its buttons gone (a refusal is told to the presser alone and the buttons
    stay). Not settled from another surface: the post edits itself on the press that settles it, as a reminder does.
  - **The status line** gains `⏰ 16:52` (the next reminder due before the local midnight, a time and no word of it,
    `Counts.reminder`).
  - Tests: `render/horizon.rs`, `runtime/horizon.rs`, `runtime/horizon_cmds.rs`, `tests_gateway/horizon.rs` (the stand-in
    now types modals and autocomplete: `FakeDiscord::modal`, `autocomplete`, and `Reply.choices`).
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
  while the process message's create is held at the fake, whatever its text (`hold_writes_containing("-# 💭")`):
  `an_answer_the_post_writes_after_the_thinking_is_still_the_answer` (a provider that holds the text back until the
  create is held, so the post, not the stream, writes the answer) and `..._after_the_fold_...` (every write held until
  the reply is queued: the order of a fast host, theseus-wpv2g). Each asserts its scenario happened. `render/process.rs`'s unit tests: the stream and the fold, a tool loop folding at
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
