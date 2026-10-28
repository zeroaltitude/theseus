# theseus-core

The agent itself: config and secrets, sessions, the turn, the context compiler, tool calls and the gate, the protocol
server, the push, the outbox, and telemetry. Read by theseusd, theseus-discord, and theseus-sim.

Key modules: `turn.rs`, `compiler.rs`, `toolrun.rs`, `rpc/`, `config.rs`, `crash.rs`, `aws/`. Read by: theseusd, discord, sim.

## What's here

- **The turn**: `turn.rs` (the turn runner: one turn under a session's turn lock, its loops, its frames), `advancer.rs`
  (continue or end the turn), `harness.rs` (the harness loop, parked on its events and the heartbeat), and
  `rpc/driver.rs` (what that loop drives: the heartbeat's spool drain and reconcile, continuation turns, due wakes,
  and the cancel path). A turn waits for its slot in `turn/admission.rs`'s queue (theseus-4g0l): three classes (a
  person's typed input, then a conversation's wakes and continuations, then a task session's turns), first come first
  served inside one, a class higher for each `AGE_STEP` (30 s) waited; a slot given back goes to exactly the next turn,
  and the kernel counts no ceiling for a queued turn (`Kernel::admit_queued`, `admit_input_queued`). A foreground job's
  wait lends its slot after `LEND_AFTER` (2 s; `toolrun/job.rs`) and retakes one at its class's head; a wake's or
  task's wait fails when no turn was admitted for `[kernel] admission_stuck_secs` (600), class `admission_stuck`; a person's
  never does (in the queue, or behind their own execution's previous turn): it waits another window. In the queue,
  one notice an episode names the holders (`Admission::set_notice`, ended by the queue's next movement); behind its
  own previous turn it waits silently, ended by that turn ending (`/stop` included). The queue is
  `ToolRuntime::admission`. Tests: `turn/tests_admission.rs`, `tests_m3/admission.rs`. A turn's end and a late
  result's wake are one frame (`turn/end_step.rs`, theseus-6qwr): the
  stop read under the frame's lock, the wake nested so its failure takes back only itself. A failed turn's retry is
  the driver's (theseus-ljr), but not a `--stdio` daemon's: its run parks the turn on input for its one client
  (`turn/stdio_step.rs`, set by theseusd before serving, theseus-zqxv); `[model.retries]` can make a transient
  failure's call again inside its turn instead (`turn/retry_step.rs`, 2 by default; a call cut at its whole-call bound is retried once a turn at most; the bench profile's, since a
  headless run ends with its turn: theseus-7gir.21). Provider timeouts: `total_secs` bounds a call until its stream
  begins; a live stream (events within `stream_idle_secs`) goes on to `Timeouts::whole_call`, `LIVE_STREAM_TOTALS` (3) times
  it, and the model call's kernel action deadline is the same (theseus-5pg8u). Once the stop has begun (`Outbox::stopping`), the driver begins
  no continuation and a call about to be sent is settled failed unsent, class `stopping` (`ProviderError::Stopping`),
  with no in-turn retry, so the next start's driver retries it (`turn/stopping_step.rs`, theseus-jtrc); its run posts no
  notice and health and telemetry count no provider error for it (theseus-36re). A `--stdio` daemon started `--one-shot SECS`
  (`theseus --spawn ask`, theseus-mqxk) serves one run (`one_shot.rs`, on the outbox): each turn's result names what
  it left for later (`TurnSubmitResult.later`, `turn/later_step.rs`, the jobs read before the late results are taken
  and again as it parks), its turn's connection watches the session, and `wake.at` says a wake due past the bound
  will not fire here; no other daemon reads it. The ask's own turn fixes the run's end, and a wake fires in the run
  when it is due by the end the run had when it was set, less `FIRE_MARGIN_MS` for the driver's tick (`OneShot::fires`):
  the words and the result's `fires`, which the CLI follows, are one rule however long the turn ran. A refused request is first made once more on the model that
  refused it (`[model.retries] refusal_retry`, on by default; at most one retry per request and three per turn, a `provider.refusal_retry` row each; theseus-keuap),
  and only a refusal that survives that goes to its model's fallback, the catalog's
  `refusal_fallback_model` (Sonnet 5.5's: Sonnet 5), and the fallback answers that request, the next going back to the routed model unless the turn stays (a
  `[model.retries] hard_decline_categories` refusal, a timeout, or the third routed refusal in a row; theseus-wf482)
  (`turn/fallback_step.rs`,
  theseus-7gir.18; `[model.retries] refusal`, on by default; the provider's own `refusal_fallbacks` wins where it rides
  the request, `compiler::server_fallbacks`). The compilation stays the profile model's: the requests name the fallback
  (`RequestSpec::fallback`), carry both models' thinking, and leave the refused answer out (`replaced_answers`, as a
  cut one); the session's target stays. A `provider.fallback` row per switch and a `provider.routed_back` row per return, and
  the result's `fallback`, whose `line` every surface shows. Tests: `tests_fallback.rs`.
  **The stop nudge** (theseus-fh0u, the owner's D8; `turn/nudge_step.rs`, `judge/stop.rs`): a loop the baseline ends
  with no tool calls asks `loop.v1` at the stop, live (it is wired live while `[judge] stop_nudge`, on by default, is
  on; off, the switch holds it in shadow), and waits at most `judge::stop::WAIT` (1 s, the pack's own on-path bar).
  When `announced_unfinished` acts true, one harness node (`harness:stop_nudge`, the model reads it as the user's) and
  its `turn.nudged` row ride one frame, and the turn calls the model once more; at most once a turn, and the nudged
  turn's end lands `nudge_loop`'s event. Never in a shared place, with the session's own switch off
  (`session.nudge`, META `stop_nudge.<session>`), while a job of the turn runs, at a strict loop cap, or with Jev
  unreachable. A stop judged live is marked with point `stop` and not judged again after the turn; every judgment
  after a turn is recorded in shadow. The result's `nudged` (with `acted`) is on `turn.ended`'s row, which the prove
  counts. Tests: `tests_stop_nudge.rs`, theseusd's `tests/stop_nudge.rs`; the judge's other tests run loop.v1 in
  shadow (`tests_judge::judge_config` turns the switch off).
  **The end note and what a command left** (theseus-53ujg.1; `[model] end_note`, on; `turn/end_note_step.rs`,
  `toolrun/job/beside.rs`, `rewind.rs`, wire `theseus_protocol::files_left`): a `proc.run` job's launch lists the
  entries of its directory and of the directories the turn's file tools wrote to (at most 8; one over 512 entries is
  skipped), through `theseus_store::blocking`, in memory by correlation id; its first result lists them again and
  says the new, ignored-by-no-`.gitignore` entries no newer than the job's end in one line after its header
  (`[new files: …]`, cached so every build says the same). `rewind`'s `settle` notes each file tool write by
  execution. At a `no_tool_calls` stop, after the stop nudge declined and never after it sent, a turn whose commands'
  new files still sit beside its edits, or that edited files after its last finished command, gets one harness node
  (`harness:end_note`, facts only) with a `turn.end_noted` row, and one more loop; once a turn, never in a shared place
  (`nudge_step::may_continue` is the condition both share). The result's `files` (changed, left, the note and whether
  the model acted) is `harness_lines`' `Files:` line and rides `turn.ended`'s row; a turn's maps drop with it
  (`end_note_step::Held`). A command running beside another in one directory may be credited with the other's files.
  Tests: `tests_end_note.rs`, `beside::tests`, `tests_stop_nudge.rs`.
  **Task turns leave a slot free** (theseus-o1h3f.4; `turn/admission.rs`): task-class turns hold at most `ceiling - 1`
  slots (`Admission::task_limit`; a ceiling of 1 keeps its one for tasks), so a person's message always finds one
  — except between a lend and its retake, which the limit never refuses, where the count can briefly reach the
  ceiling; the next slot given back goes to the person.
  A task waiter the limit declines stays at its queue's head, the free slot still counts as the queue moving
  (`hand_on` sets `moved`), and a newcomer that may take the slot passes it. A retake after a lend is never refused
  by the limit (it held a slot). A waiting task turn ages to the wakes' rank and no higher, never to a person's.
  The rule lowers a limit; the ceiling (8), the stuck window and `AGE_STEP` are unchanged. Tests:
  `turn/tests_admission.rs`.
- **The steer** (theseus-qu1lc; `[kernel] steer_queued`, on by default): a person's typed input that finds its session's
  turn running does not wait for the turn to end. The running turn opens a `Steer` for its session
  (`toolrun/waits/steer.rs`, held in `JobWaits::steers`, which the terminals share; `turn.rs` opens it once admitted and
  `finish`es it with the turn's result), and the queued input offers itself to it before it waits in the admission queue
  (`turn/steer_step.rs`, `steered`: only `Class::Person` input, no MCP prompt, and the owner's input to a task
  (`EventSink::owner`, from `Core::owner_input`: the owner in a private place, never a job's or a prompt's; theseus-slmj1,
  `turn/join_step.rs`: a task with the owner's message offered and not taken parks on input instead of reporting, and its
  report ends `(you joined: N messages)`, the count in memory and, from the frame that ends the task, the META record
  `task.joined.<session>`); a wake, a continuation and anyone else's input to a task queue as before). The signal is O(1) and polls nothing: an atomic count per steer and one over all sessions, and a
  `Notify`. Waits yield to it with no stop to the job and a result that says the job still runs and a message arrived,
  its meta `{"interrupted_by": "message"}` with the job's handle: `job_wait` (`jobs_tools.rs`), the turn's own foreground
  wait (`wait_in_turn` and a batch's, which go to the background as when their bound passes, but only once the command
  has run `STEER_AFTER`, 1 s, so a short one still gives its result), and a terminal's wait (`term/tools.rs`). At the
  loop boundary, after the tool results and `take_late`, before the next model call, `steer_in` takes the offered
  messages in arrival order and writes each as the user node its own turn would have (the running turn's id), with the
  `turn.steered` row (`fact/steer.rs`), in one frame: written once or not at all, and when that frame fails the offerers
  queue as they would have. The offerer is answered with the running turn's result and its client's lane is released at
  once (`ordered::applied`), so a second message on the same connection is offered too. The inbound judgments run for
  each (`inbound_point(.., steered)`), but the turn's route stays the running one: neither the route verdict nor the
  owner's words about routing apply until the next turn. A message that comes during a model call waits for that call's
  boundary (the call is never cut); one that comes during the turn's last call (no tool calls) runs as the next turn.
  Nothing matches the words: "cancel it" reaches the model, which calls `job_stop`. The binding sends a message at once
  when a turn is in flight (`theseus-discord`'s `runtime/steer.rs`) instead of holding it. Tests: `tests_steer.rs`,
  `toolrun/waits/steer.rs`, `term/tests_idle.rs`; `bench/async/steer_stand_in.py` measures it.
- **Context**: `compiler.rs` (manifests, recompiles, the cache layout, the token estimate), `context_files.rs`, and
  `catalog.rs` (each model's window, prices, and caching). A model may have a second price tier, `LongPrompt`
  (Haiku 5.5's: 5x past a 100,000-token prompt, theseus-3okf): `terms` prices a usage at the tier its prompt
  (input, cache reads and writes) picks, `reserve_micros` and `reserve_parts` at the tier the input estimate picks;
  price a usage only through them, never by reading a row's base prices. A refusal fallback's window holds its
  model's (`every_refusal_fallbacks_window_holds_its_models_requests`). A call that must answer through one tool asks `Catalog::forcing(model)` (`thinking_off`, and `may_force`) and builds its tool and `tool_choice` with `provider::one_tool::one_tool` (theseus-yym79): forced only where thinking is plainly off (`disabled`, or none sent and not unasked), else `auto` with a strict tool whose every object is closed.
  - **Files given to the model** (theseus-9g2, theseus-c9l6): `attach.rs` and `blobs.rs`. A message's files: text as
    text (`[tools] max_read_bytes`), an image as an image, and any other file kept whole in the store's blobs up to
    `[tools] max_attachment_bytes` (32 MiB), a `File`. A PDF is read once, when it arrives, in theseus-files' capped
    child (`theseus_files::convert`; the turn's `accept_files`, under `theseus_store::blocking`): its pages, its text
    by page (a JSON blob), and a part of its first pages for each page limit it passes (100, 600) or the request's
    byte budget (18 MiB). A `file.read` row each, with its span and `theseus.file.read.duration`. The compiler
    renders a PDF per model (`catalog`'s `pdf`): a `document` block (the whole file, or the largest part the request
    has room for, with a line saying what was left out), else its text by page (GLM, a PDF the provider refused, one
    the request has no room for). PDFs are budgeted in render order (`attach::Spend`), so an earlier one never
    renders differently because of a later one, and the cached prefix holds. `fs.read` and `http.fetch` return a
    PDF's pages (`theseus_tools::Media::Pdf`), kept the same way (`toolrun::keep_media`). Join 2: a Word, Excel,
    PowerPoint, OpenDocument, EPUB, RTF, or notebook file, and an archive's list, is read into sections when it
    arrives (`theseus_files::convert::doc`; a JSON `{"sections": …}` blob, `blobs::Section`), and every model reads
    its text, a vision model its notebook images too; a recording, a video, and any other file are named with how to
    read them. **`file_read.rs`** (`file.read`): the runtime runs it (`ToolRuntime::read_file`), finding the file
    among the session's nodes by name or digest: a PDF's other pages, a document's sections, an archive's member
    saved and read under `<cwd>/.theseus-files/<session's last 8>/`, a recording's transcript through Deepgram
    (`[voice]`'s key and settings; capped at `[tools] transcribe_max_minutes`; kept by digest in
    `blobs/derived/`; booked as `speech.transcribed` by `book_transcript`), a video's transcript and frames
    (ffmpeg), OCR (tesseract), or `save`. In a shared place its result is outside text. Tests: `attach.rs`'s, and
    theseusd's `tests/files.rs`, which run the real converter, a turn per model, and `file.read` with a stand-in
    Deepgram.
  - **One long instruction** (theseus-3ki0): `compiler/clearing.rs` and `compiler/ring.rs`. A pass runs when the results not yet
    cleared pass `clearing::CLEAR_AT_TOKENS` (150K tokens), or the estimate passes `clearing::CLEAR_AT` % of the budget
    (a small window clears as before), and stubs every result over 2,000 characters except the newest ones within
    `KEEP_TOKENS` (40K; and what `CLEAR_TO` % leaves beside the rest of the request), oldest first, the newest
    exchange's always whole (theseus-5lzox; it replaced "the newest 20 answers", whose 270K sawtooth rewrote the cache
    every dozen calls and cleared the first files read, which the model then read again). One recompile, the cleared
    results kept as its report's drops of tier `cleared` (no store format: the report is stored already), which the
    render reads, so appends render the same stubs; a pass that would leave the request above halfway from the line to
    the keep does not run, so passes stay rare, except past the percent line: there a refused pass is retried keeping only
    the newest exchange whole and taken under the 20-exchange rule's own 42.5 % line (results of 2,000 characters or
    fewer are never cleared yet count toward the guard, so without the retry a session of them climbed to the ring,
    larger than the old rule left it). Each pass is one
    `context.cleared` row (`fact::compaction::Cleared`, from the compile step). The stub names the call and how to
    get the result back (a kept output's `full_ref` when there is one); an `fs_read`'s names the lines its result held
    and the file's length (`clearing/remember.rs`, read from the result's own footer, never from the call's input), and
    every stub ends with the first sentence of the model's text in the answer after the result (`remember::notes`, from
    stored nodes at render time, so the bytes never change). `compiler/tests_replay.rs` replays a 207-call hunt
    through the compiler (its `the_replays_grid` prints the trigger/keep grid). The ring's range holding one user message
    cuts inside its turn too: at an answer whose earlier calls all have their results before it (`ring::in_turn`),
    searched by halves; the instruction (the message and what precedes its first answer) is pinned in every
    candidate, and a summary renders after it (`compaction::summaries_first`, by its range's `first`; `hidden` and
    `outside` keep a node before a summary's range visible). The summary call is told the work in progress
    (`turn::compaction::IN_TURN`) when its range holds no user message, and reads a range past its model's window
    shorter, each node's cut cut in proportion down to 400 characters (`shorter_cut`), before it gives up for the ring. `[context] working_window_tokens`
    (`config/context.rs`) lowers the window through `window_override` (`TurnRunner::working_window`). The budget
    and its report are `compiler/budget.rs`. Tests:
    `compiler/tests_ring.rs`, `tests_one_turn.rs`.
  - **CONTINUE's candidate signals** (M5 25b): `signals.rs`, read inside `compile()` from what it is given (the
    clock passed in as `CompileInput.signals`, never read there): a dormancy gap, the tail crossing its soft band,
    a task report or a wake arriving, and a provider cache miss, each against what was written since the model's
    last answer, so an input's signals fire once, at its turn's first compile. Thresholds are `[judge.signals]`;
    they are read whether or not the judge is on, and decide nothing: every one that fired rides on
    `context.compiled` (`signals`). A compile that appended and fired one asks `continue.v1` in shadow
    (`judge/compile.rs`: the dispatch is a spawn, and the turn's trace gets a zero-length `judge` mark naming the
    judgment's id, minted at the dispatch). Tests: `tests_continue.rs`.
  - **The system header** (`TurnRunner::system_blocks`): the persona, the assembly note (`turn::ASSEMBLY`: what the
    harness puts in a request, and that a recalled note is no part of the person's message; theseus-fpm2), the
    precedence line (`compiler::situation::PRECEDENCE`, 35a), the tools note, and the profile's text, all static.
    Harness facts every request needs go there, never into a session's messages.
  - **The layered request and the thread's context** (theseus-h4a7; `compiler/layers.rs`, `compiler/card.rs`,
    `compiler/cold.rs`, `thread_context.rs`): the system is three layers with a breakpoint each, then the messages'
    automatic one, the provider's four: `header` (the header text and the standing block), `persona` (the persona in
    play's files), `project` (the system-level files, then the guidance of the session's categories, a thread's
    `topics` among them: `Walk::with_topics`), and an unmarked `session` block (its directory). Nothing per request
    goes inside the three, so two sessions on a profile and a place class send them byte for byte; `layers.rs` is the
    one place they are rendered (the prefix library, row T13, builds on it), and `context.compiled`'s `cache.layers`
    holds each one's digest (the stored manifest is unchanged: no store format). In a thread's session the thread's
    card heads the first message (`card.rs`), a snapshot kept as the META record `thread.card.<compilation id>` in the
    compilation's own frame, so every append repeats its bytes and a recompile takes the card as it stands.
    `thread.note {line}` (threaded sessions only; the tool is left out of every other spec) rides the call's frame
    with the thread's next record. A thread whose next message finds its cache window passed (`stubs::cold`, the same
    check) is recompiled small at the turn's first loop (`cold.rs`, trigger `cold_resume`): the fresh card, the
    summary, the newest `[threads] resume_keep_turns` turns, results over `clearing::MIN_CHARS` stubbed, a turn dropped
    only when that is not enough (a ring, summarized as any ring is), to the working size: `[threads] working_percent` (40) of the serving model's window, or `working_tokens` when set (`ThreadsConfig::working_size`; resume_keep_turns is 20; theseus-d9je7). Unthreaded
    sessions and warm threads are never touched by it. Tests: `compiler/tests_layers.rs`, `tests_cold.rs`,
    `rpc/tests_thread_context.rs`.
  - **The prefix library** (theseus-rr15, T13; store format 41; `prefix.rs`, `prefix/`, `config/prefix.rs`,
    `fact/prefix.rs`, `rpc/prefix.rs`): layer 3 (the project: the system-level files, then the topics' guidance) is
    kept as a header by digest (META `prefix:<digest>`, its hit log `prefix.hits.<digest>`, a retirement
    `prefix.retired.<digest>`, all in the frame of the compilation that minted or reused it) and reused byte for
    byte. The pick runs only at a compile point (`prefix/turn.rs`: no compilation, a forced recompile, a cold
    resume, or a system that changed under the session; never under a warm session): the nearest headers by sketch
    (`prefix/sketch.rs`, a hashed bag of words, computed in the daemon) on the same lower layers and place class,
    each scored by `fit` (the share of the fresh layer's bytes it covers; a changed or a foreign section never
    fits), then reuse at `[prefix] confirm`, below it mint only when `p(reuse) × reads × delta > warm coarser`, else
    the coarser header (`prefix/fit.rs`'s module doc is the rule). What the header lacks heads the session's block
    (`prefix::applied`, in `compile_with`), and the manifest's `prefix` names the header, so an append replays the
    same bytes from memory (`Library::replay`). Each pick is a `prefix.picked` row; `prefix.report` (`theseus model prefix
    report`) joins them with the turn's cache read and what went wrong after (a correction, a re-ask): a reuse below
    `act` that went wrong is a false fit. `[prefix] enabled = false` renders each session's own layers. A session's
    and a place's standing records are the session's block's, not the header's (`standing::Block::local`), so layer
    1 is the same for every session of a place class. Warmth is per header and model, in memory (each compile
    touches its header): routing's cold estimate takes off what a warm header that fits holds on the switch's
    target model, under that target's own lower layers (`warm_prefix_tokens`, asked only for a cache hold). The
    keep-warm read compiles with the replayed header and the standing block, as the turn does. Not yet: keep-warm
    refreshing the most-read headers alone instead of one read a session, and Jev's `prefix` pack (the fit is
    code's). Tests: `prefix/tests.rs`, `rpc/tests_prefix.rs`, `compiler/tests_layers.rs`, `tests_route_warm.rs`,
    the layouts' format-34 samples.
  - **Situations** (M6 35a, theseus-3nk.1): `compiler/situation.rs`. What a compile is for: the step tells it from
    what it holds (`TurnRunner::situation_of`, `turn/situation_step.rs`: no compilation yet is a conversation's or a
    task's start; a session's first compile in this run, `RunCompiles` in memory, with nothing its turn brought is a
    resume; else a continuation; a detour its own), and `compile()` settles it (`settle`: a new compilation of its
    own triggers is a recompile with its trigger). It rides `Compilation.situation` (store format 22) and
    `context.compiled`. `admits` is the table of what each admits, from the code (lessons reserved for 35b): a
    detour admits its window's messages, replies and results, a task's arrangement among them, and a quick detour's
    own recall note, never a summary or an earlier note (`compile_detour` leaves those out; theseus-pa1xx). `check`, after the compile, fails a piece not admitted, or a
    set that does not close (a result without its call, a call without its result), as `context_unadmitted`, with
    nothing sent and no retry; it enforces from the first day, with no shadow mode (the owner, 2026-10-05). An assembled
    `recall_id` whose node is not in the session is a section never written (its call never dispatched; nodes are
    never deleted): the render leaves it out, and so does the check (theseus-783a). The
    precedence line (`PRECEDENCE`) follows the persona and the assembly note in the header. Headers written from 35a
    on (§2.11's testimony): an item's names its origin and place (`TurnRunner::place_name`,
    `recall::render::item_header`) and ends `volatile: as of <date>, unverified` when its shown text holds one by the
    labeler's rule; a summary's names its positions and model; a synthesis's sources name their places too. A frozen
    header never changes. Tests: `tests_situation.rs`, `compiler::situation::tests`, `recall::render::tests`.
- **Places** (the place rule, theseus-nbsh; it replaced 19a's labels on nodes): `places.rs`. Every place a session
  speaks in is private or shared. Private: the CLI and the web UI (no place), a DM with an owner (`Config::owners_for`),
  and a guild channel the binding bound private (its own `private = true`, or, saying nothing, in a guild the bindings
  file trusts whole with its guild's `private = true`, theseus-rdqg); it gets everything. Shared: every other
  place; it gets the public tools (`places::public_tool`), the file tools only under `[places] public_paths`
  (canonical roots, so a link out of a tree is outside it), and only the context files marked `readers = "public"`
  (`context_files::withhold_shared`). A turn's class is `TurnRunner::class_of`: where its words go, which is its
  session's place (a task's is its parent's, `outbox.target`), or, for a session whose place moved on, where its wakes
  and reports answer (`outbox.wake_target`); shared when that cannot be read. It is fixed in `TurnCtx.class` once the
  turn has taken its wakes and reports. The catalog (`ToolRuntime::definitions_for`) and the tools note
  (`system_note_for`) are per class, and the gate refuses a shared place's other calls (`places::refusal`, answered
  `Not run: …`, reason `place: …`). The binding tells the core its places as it starts (`Core::bind_places`, in
  memory: an unnamed guild place is shared), and reads each private channel's viewers once (`private_place_viewed`,
  `place.viewed`), for health's `places:` line. In a trusted guild it reads none, and health names each private
  channel there `(in a trusted guild)` (`Core::trust_guilds`, `PlaceInfo.trusted_guild`). M6's recall draws in a
  shared place only on that place's own sessions (and the books, when built, will too). Tests: `tests_places.rs`,
  `places::tests`.
  - **Ceilings** (step 38a, theseus-ext.3; `ceiling.rs`): the bindings file may give a place a ceiling, which
    narrows what its class allows and never widens it. It is read with the class (`TurnRunner::view_of`, so a task
    has its parent's), and rides in `TurnCtx.ceiling`: its `tools` filter the catalog (`PlaceView::offered`) and the
    gate refuses the rest (`Ceiling::refusal`, reason `place: …`), its floor is applied after the policy's decision
    and before T1's hold (`Ceiling::floor`), its spend limit is the kernel's `place_limit` (the lower of it and the
    config's, pinned while a place caps it, told at each binding start), and its profile is a `turn.submit`'s when
    the turn names none (`Core::place_profile`). Each guild has its own trust word (`PlaceRule::trust_guilds`).
    Tests: `tests_ceilings.rs`, `ceiling::tests`, and the kernel's `tests_place_limit.rs`.
  - **Publish** (`rpc/publish.rs`; graduation's light form): `place.publish` puts one item (a node, a file the owner
    can read, or a message) into a bound place's session as the owner's message, with a `derived_from` edge for a
    node (`publish`), its `place.published` row, and a notice post in the place, in one frame under the place's
    execution lock, never while a turn holds it. Only the owner, from a private place: `judge_act(Act::Publish)`
    which asks `places::owner_in_private`. `theseus publish`, Discord's `/publish`.
- **The request census** (theseus-7m7w.1; `census.rs`, `provider/count.rs`): `context.census` compiles the *next* request of
  a session (`TurnRunner::census_request`: the spec a turn builds, its standing block, `compiler::compile` over the
  transcript, as `keep_warm_request` does), or a fresh session's first request for a kind of place (`cli`, `dm`, `task`,
  `harness`), and sends nothing. `census_parts` apportions the compile's own estimate to the parts with `Ledger` (a part's
  tokens are the rounded-up running total through it less the one before it), so the system's sections, each tool
  (description, schema, the rest) and the messages add up to the total exactly. A tool's use is the ledger's
  `action.planned` rows over 30 days (`tool_use`, an operator's read, never a turn's path; none while the index's shape is
  built). `exact` asks `Provider::count_tokens` (Anthropic's `/v1/messages/count_tokens`, no model run) four times in
  parallel, the request and its smallest form with the system and the tools added alone. A surface that reads no private
  text gets the sizes without the context files' names. Tests: `tests_census.rs`, `provider/tests_count.rs`; the bench
  is `theseus-sim bench prompt`.
- **The ontology** (row 26, step 21b; M4 §2.8; theseus-8kk.1): `ontology.rs` holds the snapshot (`Board`: built
  after serving by one `onto:` META prefix scan, `Core::warm_ontology`, or by its first reader; kept current on every
  write, which checks, writes its records and their rows in one frame, and swaps it), the given kinds (a bound
  place's `channel:`/`person:` category made at its first bind, `Core::bind_categories`, with a guild channel's
  `guild:<id>` category its parent, named by the id since the binding carries no guild name; a session's given
  memberships read from its place at compile, the guild through the channel category's parent, never stored; the
  `thread` and `project` kinds are seeded and their reader, from the thread and project records, is not built
  yet), and the turn's `Walk`, fixed with the request's spec.
  `compiler::compile` renders the guidance after the context files: an append composes the memberships its
  manifest recorded, a recompile (or a ring) the current ones, so a membership change waits for the next recompile
  and a guidance edit in play is one `system_changed`. The manifest records `memberships` and `guidance`. A shared
  place composes only its own place's given memberships, and the place rule's admissions never read a membership.
  The methods are `rpc/ontology.rs` (`ontology.list`; the writes are `judge_act(Act::Ontology)`, the owner from a
  private place); the rows `fact/ontology.rs`. **Machine writes** (theseus-e3mjc; `rpc/ontology_machine.rs`, no
  method calls them yet: R3's RPC and CLI): `Core::ontology_machine_write(by, write, records)` writes Jev's, the
  sweep's or the dream's categories and lists in one frame, each row (scoped `ontology.write:<write>`) carrying the
  record as written and as it was (`fact::ontology::Undoable`), and `Core::ontology_undo(write, who, via)` reads those
  rows back and writes the inverse records and an `ontology.undo` row (label `wrong`) in one frame, as the operator:
  the earlier list byte for byte, a minted category retired unless another session's list uses it (`InUse`, nothing
  written). The rules of who may write what are theseus-ontology's (`machine.rs`). A machine write also carries
  **kinds and guidance** (theseus-63tfk): the sweep's kind (`KindSet`) and the dream's first guidance, at most 400
  characters, only where none exists (`GuidanceSet`), each row with its `undo` (a guidance written where none was
  is undone to an empty one, one version on; a kind or guidance put back is one version on). **The ontology's
  nightly sweep** (theseus-63tfk, R5 slice one; `judge/onto/sweep.rs`, `[ontology] sweep`, default on, run by
  `Core::onto_sweep_after_serving` at `[judge] learning_hour` and once after a start): the deterministic plan uses no model, no spend;
  `Ontology::sweep_plan(now_ms)` (theseus-ontology's `sweep.rs`) gives the steps, each written as its own machine
  write of the sweep's (id `sweep-<ms>-<n>-decay|merge`, undone by `Core::ontology_undo`): decay of machine
  memberships older than 30 days (one write), and a merge of each duplicate category (same kind and parent, names
  folded to lowercase letters and digits; only a category a machine wrote, with no guidance, no children, and no
  operator or import membership) into the operator's, else the better held, else the earlier-id twin. The
  sweep's row is its writes' own rows (`ontology.category` with `merged_into`, `ontology.membership`); no
  `ontology.swept` row: the morning note builds its lines from those rows (below). **Beyond the deterministic plan**
  (theseus-81zx7, theseus-nzzz4, theseus-olt3h, theseus-0umqz; no store format change: META keys and ledger rows
  that exist): (1) `kind_of_thing = other` at the act band is `Step::Other` in `decide.rs` and a step of the
  `onto.filed` row (names, confidence; no record, no card); (2) `judge/onto/mint.rs`: three distinct sessions' `other`
  rows in the last 7 days that share a folded mined name mint a kind, one category and the memberships as one write
  (`sweep-<ms>-<n>-kind`), a name no kind can take or the table holds counted as `refused`, a name a kind lists in
  `examples` (an undone one too) not clustered again; (3) `judge/onto/judged.rs` (`[ontology] sweep_usd_per_day`, 1.0,
  0 turns this part off and the deterministic sweep still runs): `same_thing.v1` asks of machine pairs that look alike
  (theseus-ontology's `judged.rs`: a shared word of 4 letters or more, at most 20 a run) and merges at the act band by
  the one survivor rule (`Ontology::plan_pair_merge`); `two_things.v1` asks of a machine category 12 sessions or more
  hold (at most 3 a run) and splits it into two named children, moving the sampled sessions it placed at the act band
  (`plan_split`); the day's spend and what was asked (not again for 30 days) are META `onto.sweep.judged`, and every
  call also passes the judge's day budget; (3b) `judge/onto/renamed.rs` (theseus-63tfk.1; three more asks under the same
  money, mark and not-again rule, each a Choice among mined names so Jev writes none): `name_it.v1` renames a machine
  category with 5 titled sessions or more whose name is in none of their titles while one mined name is in half of them
  (`stale_names`; at most 3 a run), writing `Ontology::plan_rename` (the old name in the description as `was: <old>`;
  `Step::Rename`, `sweep-<ms>-<n>-rename`, undone like a merge); `same_kind.v1` asks, before the plan mints, of pairs of
  mined `other` names whose sessions add to each other's (singleton chains `a~b`, `b~c` included, and two names that are
  each a cluster) while the pair's two groups plus the names no kept yes has linked reach 3 sessions (at most 20 a run,
  largest union first) and a yes joins them (`mint::Hints::same`; a yes is kept in the mark's `same` for as long as its
  `kind <a> <b>` ask, so a chain asked over two nights still clusters, and a kept pick follows its cluster to a new key); `name_it.v1` names a kind about to be minted from a cluster of two names or more, and `none`, a
  pick below the act band or no answer leaves the most shared name (`Hints::named`, `Swept.named_kinds`; a pick is kept in the mark's `named` for as long as its ask is not asked again, so a cluster that mints a later night uses it; a Jev-named kind's description ends with `mint::NAMED_NOTE`, which the morning note and the cockpit read). The night runs
  hints, then the plan, then the pair, split and rename asks (`onto_sweep_night`); `onto_sweep` alone is the plan with
  no model; (4) `consolidate/guidance.rs`, run by the nightly consolidation tender after
  its own run (never `memory.consolidate`): one call a category of an interpreted kind a machine wrote, no guidance,
  held by 5 private sessions or more (3 a night, most held first) on `synth_profile`'s rule (`Core::session_profile`),
  under `[memory] synth_limit_usd_per_day` counting consolidation's spend (META `onto.dream.last`, written after each
  call, so a stop keeps what was spent), an answer over 400 characters refused, never cut, a session whose first or last
  message cannot be read left out (the category waits only below 5 left), a category that waits says why in `Dreamed.waits` (`Wait`: different profiles, a profile or provider that is missing, a model that thinks unasked, too few readable sessions, no price), which the tender logs, and `[ontology] dream_guidance` off (default
  on) reading, calling and writing nothing; (5) `judge/onto/digest.rs`: `morning_note` appends one line a sweep or dream
  write of the last day with its `theseus ontology undo <id>`, built from the writes' own rows (`undo.write`), verbatim
  after the voiced words. **No card goes out at night** for any of these writes: the pacer is a gap and not quiet hours,
  so a night post would wake the owner; the morning note is the one place they are told. Tests: `tests_ontology.rs`,
  `tests_ontology_bind.rs`, `tests_onto_sweep.rs`, `tests_onto_other.rs`, `tests_onto_mint.rs`, `tests_onto_judged.rs`,
  `tests_onto_names.rs`, `consolidate/tests_guidance.rs`, `tests_onto_digest.rs`.
- **Standing** (theseus-a7vi; `standing.rs`, `rpc/standing.rs`, `turn/standing_step.rs`, `fact/standing.rs`): the owner's
  standing orders, preferences and facts, one `protocol::StandingRecord` each as a META key `standing:<id>` (store
  format 29), append-only (a revoke, with empty text, or an edit is a new record whose `ends` names the one it ends).
  `standing::Board` holds them in memory (one META scan after serving, `Core::warm_standing`, or by the first turn),
  so a compile reads memory and a plain turn writes no frame for them. `TurnRunner::stand` renders the turn's scope
  as `standing::Block`: the records for everywhere at the end of the header layer, and this session's and this
  place's at the head of the session's block (theseus-rr15; no block of its own, so no new cache breakpoint), beside
  the walk in `turn.rs` and `route_step.rs`. A change of the records is a `system_changed` recompile; otherwise the prefix is
  byte-identical. It is system, outside the ring, so no compaction drops it. A shared place gets the orders' text and
  one line (`standing::WITHHELD`) for preferences and facts, never their text; `standing.list` gives their text to
  the owner in a private place alone. The compile's manifest lists each record (`Manifest.standing`: id, shown), and
  `standing::missing`, run by the situation step after `situation::check`, fails a compile whose system lacks one it
  should show as `context_unadmitted` (`standing <id> missing`, why `standing_missing`).
  **A surface scope** (theseus-slvl.4, store format 34; `StandingScope::Surface(name)`, `terminal` for the CLI and the TUI,
  `cockpit`, `discord`, `voice`, `mcp`): the record is in no system block, so the system and tools stay byte-identical
  across surfaces. `Block::here` renders the turn's surface's records as a "Standing here:" block with the same kinds
  and the same privacy rule, and `standing_step::standing_here` attaches it to the request's last message
  (`task_graph::view::attach_text`, after the cached prefix, beside the task view and the commitments), counted in the
  estimate; `standing::missing_here` refuses a request that dropped it, and the compile's `context.compiled` row names
  them in `standing_here`. The surface is `TurnRequest.surface` (`standing::surface_name` of the connection; a
  continuation and a caller that names none have none, so no surface record holds), kept in the turn's `Held`
  beside the blocks. `standing.list` takes the asker's `surface` with a `session_id`; its tally counts only the
  system's records (no manifest lists the tail). `standing.repeated {record, node}` is a row of ids
  (`Core::standing_repeated`, for the capture row to call), counted per record without a format change.
  The compaction summary's instructions keep a style request ("for now, be brief") in the operator's words
  (`turn/compaction.rs`, `tests_compaction`). `standing.add` and `.revoke`
  are `judge_act(Act::Standing)`: the owner, from a private place; the CLI refuses them inside a job
  (`client::OPERATORS`). `standing.list` tallies the manifests (compilations of each live record's scope since it
  began, and those lacking it), a scan of the sessions' compilations: a read for a person, never a turn's path. Tests:
  `tests_standing.rs`, `tests_standing_ring.rs` (three compactions), `standing::tests`, and the layouts sample and
  format-27 store test in `tests_layouts.rs`. Not yet: the context explorer's totals (`context.explain`) do not count
  the block.
- **The owner card** (theseus-slvl.6; no store format change then: META keys and two ledger kinds; live learning, theseus-6f8t0, is store format 51; `card/`, `rpc/card.rs`,
  `fact/card.rs`, `config/card.rs`, wire `theseus_protocol::card`): standing's facts and preferences, filled from the
  imported history one question at a time. `card::evidence` reads the archive's evidence by labels (a USER-style file, a
  feedback note, a `sop` book hint, a `decision_or_preference` triage) and **never a partner-confidential episode: the
  label is checked from the session record before any node is read** (counted as `withheld`); only operator text in a
  conversation, operator and agent text in a note, never outside text. `card::cluster` (pure) splits lines, joins
  near-duplicates by content-word overlap through an inverted index, ranks by distinct sources then recency, and says a
  cluster is fresh while most of its episodes are not in the seen set (every proposal's sources and every episode sent
  in a call, `card.sent.<call>`). `card::ask` is the request (`[card] profile`, Haiku; one tool, `state_cards`; scrubbed)
  and the parse (a statement must cite an episode its cluster showed, must not copy a line, one a cluster). `card::run`
  (`Core::card_run`, behind `card.propose` and the nightly tender): at most `MAX_CALLS` calls of `BATCH` clusters, each
  reserved on `[card] day_usd` and the day ceiling first, one frame a call (proposals, the sent episodes, the job's
  state, the `card.proposed` spend row); a statement repeating a live standing record or any proposal's words
  (`cluster::similar`) is dropped; `[card] max_open` waiting at most (a real run that finds it full is an attempt: it
  records `last_run_ms`, scans nothing, and the next comes on the normal schedule; the tender also keeps a `ran` mark like
  its twins, set by each real attempt but not by the daily done look, theseus-15kn3). A proposal is the META record
  `card.proposal.<id>`
  (`CardProposal`: the question, `Keep/Edit/Not this`, its sources, expiry); `card.answer` keeps or edits it through
  `standing_add` (`source` `card:<id>`) then settles the proposal and writes `card.answered` with the episodes; Not this
  and expiry (`card_expire`, applied when the card is read or the job runs, no timer) make its sources seen for good.
  The proposals are queue rows with owner-ask's words and options, not parked executions: thirty of those would hold
  thirty executions for a fortnight. The nightly tender (`card/tender.rs`) is the learning tender's `due` held to
  `[card] every_days` for the archive pass, which is off once `card.done`; the live pass runs every night at the same
  hour for as long as `[card] observe` is on, done or not. **Drafting** (theseus-ly75u): before clustering, `evidence::kept` drops a
  note's line addressed to an assistant about running itself (`addressed`: "you" as its reader, or a session's
  machinery, matched on word boundaries by `has_phrase` as `one_task` is; a bare "heartbeat" or "bootstrap" counts only in a line that also says "you"), keeps only the owner's lines (`about_owner`: a pronoun, "the owner", a third-person verb, or a name then one, "Mara prefers", "Mara is", unless the first word is a setup-file order or determiner from `NOT_A_NAME`) of an assistant's setup file (`setup_file`: soul, identity,
  bootstrap, agents, memory, heartbeat, tools, a dated daily log), and drops any line of one task, repository, branch or
  run (`one_task`); the result counts them (`setup`, `one_task`). The tool's `scope` takes `one_task`, which `parse`
  drops and counts (no scope is that narrow); `parse` also drops a thin statement (under two content words) and one
  that lost the "on whose behalf" of the lines of the episodes it cites (`turned`). The tool's `conflicts` names a call's clusters a statement contradicts, and
  `run::settle_conflicts` keeps the better supported of each pair and notes the other (`CardProposeResult.conflicts`). A later call of one run lists the run's earlier proposals after its clusters as `P1`, `P2`, … (`Job.earlier`, in the user message only: the system text and the tool stay the same bytes); a statement the model says conflicts with one is not offered and is noted the same way, and the call's privacy routing covers those proposals' clusters' sessions. `card.answer` refuses a decline that carries a `scope`.
  **Scope**: `card.answer {scope}` (`StandingScope::from_words`, the one parser of `words()`'s forms) sets a keep's or
  an edit's scope; option `scope` on a kept proposal writes its record again there (`standing.add` with `replaces`,
  same kind, words and source). No coding scope: the route mode is decided after the standing block is fixed.
  Tests: `card/tests_pure.rs`, `card/tests_job.rs` (the invented archive in `card/fixture.rs`, and its `setup_export`),
  `config/tests_card.rs`.
  **The observer** (theseus-6f8t0; `card/observe.rs`, `card/observe/extract.rs`, hooked in `judge/categorize.rs`'s
  `at_exchange_end` beside people.v1's): in a Private session the owner is alone in (`capture::owner_alone`; never a
  task), at `categorize::due` on its own META mark `card.observe.<session>`, one forced-tool call on `[card] profile`
  (through `google::private::for_session`, so the data gate; scrubbed; `audit_reserve`, `[card] day_usd` and `day_hold`)
  reads the owner's own lines since the mark: only the nodes `turn.submit` noted as the owner's (`capture::expect`'s
  owner mark, in memory, so lost at a restart: earlier lines are then not read). Code keeps an item only if it cites a
  line, is lasting, is not one task's or a paste, and is not a restatement (that counts as said again, as capture's
  `standing.repeated`). The observations, the moved mark and the `card.observed` row are one frame; a due point with none
  of the owner's lines moves only the in-memory mark and writes no frame. Spawned after the turn, so the census and the
  turn path are unchanged. `observe::sweep` (called first by the live pass) reads each owner-alone session active in the
  last 48 h past its mark. Read with `theseus card observed`. Tests: `card/observe/tests.rs`.
  **The live pass** (theseus-6f8t0; `card/live.rs`, `rpc/card_stale.rs`): the observer's `card.obs.<id>` notes (META, never a node:
  not indexed, recalled or compiled; `CardObservation`, read through `state::observations` alone) become questions, nightly and after
  the archive's run in `card.propose`: the observer's sweep first, then expiry, then **Stale** (an observation that contradicts a live
  record: "still true?", Keep, Edit, End it; at most `[card] stale_per_night`, one open per record), **Add** (a fact or a correction
  alone, at support 1, no call; or two or more clustered preferences and decisions, stated by the card's call) and **Pattern**
  (preferences and decisions of at least `[card] pattern_sessions` distinct conversations), at most `[card] live_per_night` made since
  this night's learning hour (`live::night_of`) and `max_open` open. An observation a question rests on is `used` in its frame; Not
  this and expiry move it to `dropped`. `end` (`theseus card end`) revokes through `standing_revoke`; Edit on a Stale question is one
  `standing.add` with `replaces`. Nothing keeps, ends or edits a record but the owner's answer. Tests: `card/tests_live.rs`.
  **The DM poster** (theseus-6f8t0; `card/dm.rs`, started with the card tender by `card_tenders_after_serving`): every 10 minutes, `due` picks
  the open proposals with `observations` and no `posted_ms`, oldest first, at most `[card] dm_per_day` a local day, from `[initiative]
  morning_hour` on, with `[card] observe` on, a DM bound and the pacer's window open (a closed window waits; never silent); each is one
  `card_question` outbox post, and the posts and the proposals' `posted_ms` are one frame, read again under the writer handshake. Tests:
  `card/tests_dm.rs`.
- **Personas are the ontology's** (theseus-utfnl; store format 50; `personas.rs`, `personas/{migrate,soul,write,note}.rs`,
  `rpc/persona_write.rs`, theseus-ontology's `persona.rs`). A persona is a `persona:<key>` category of the seeded `persona`
  kind (interpreted, assigned by the operator and the import only, one per session, precedence 100; the row is pinned for
  every writer, `Kind::check_basis`), whose `Category.persona` card holds the soul file's path (`soul`, `soul_public`,
  `soul_digest`) and the short fields (`emoji`, `card`, `tts_voice`, `avatar`, `initiative`, `shareable`, `default`). It takes
  no membership list, no guidance record and no merge from any origin (the ontology refuses them), so Jev, the sweep and the
  dream can never write one (`tests_persona::no_machine_origin_writes_a_persona`). **`personas::Book`** is what every reader
  reads (`Core::personas()`, `TurnRunner::personas()`): built from the ontology snapshot and rebuilt when a write swaps it
  (`ontology::Board::personas`); a persona with a soul is *live*, one without *awaiting* (listed, in play when named, but
  no text, never offered to Jev, not counted by `engaging()`). `Config::context_paths(&book, persona, engaging)` builds the
  persona layer from the live ones in key order, each its soul with the same header as before, so the cached prefix did not
  move at the migration (`tests_personas::migration_changes_no_prefix_byte`). **The migration** (`migrate.rs`, in
  `Core::build`): a config with `[personas]` and no META `personas.migrated` writes a category each (origin operator,
  `added_by` `config-migration`, the first file the soul, any other named in the `persona.migrated` row and the warning as
  not carried) with the mark and the row in one frame, and one FYI note; then `[personas]` and `[context] default_persona` are
  ignored (`Config::persona_warnings` says so at each start). **Jev's `none`**: the persona question gets one more item,
  `none`, the default persona's name and card (else the general assistant's); the default is not listed beside it
  (`engage::inputs`, `Choice::is_default`), and `none` engages the default persona, else the built-in. No pack changed.
  **The cap**: `[context] persona_max_tokens` (2,000; 200 to 8,000), bytes over `BYTES_PER_TOKEN`; refused at every soul write
  with the count (`soul::within`), never cut; a hand-edited or migrated file over it is carried whole, said once a digest
  (`turn/persona_step.rs` `alert_over_cap`), flagged in `theseus census` and health. **Only the owner writes a soul**:
  `persona.add|soul|set|retire|default` are `Act::Ontology` acts (`judge_persona`: the owner, from a private place; the CLI
  refuses them in a job), and `soul::Plan::write` (checked first by `plan_soul` and the ontology, then atomic, 0600 in a 0700 directory, the replaced file kept as
  `.<ms>.bak`, a `persona.soul` row) is reached only from them. **Reading a soul** (theseus-lky63; `personas/soul_read.rs`, `persona.soul_read`, judged as the writes are; no store format): the file's text, digest and its `<file>.<ms>.bak` backups (newest first, 20), or one listed backup by `bak_ms`; `persona.soul`'s optional `expect_digest` refuses a write when the file changed since it was read. The cockpit's editor loads the file and writes it whole, as the CLI does. Tests: `tests_persona_soul_read.rs`, `cockpit/test/personas.test.ts`. `persona.import` writes a category (origin import). A
  persona added without a soul draws one `persona_soul` post to the owner's DM (`personas/note.rs`; Discord's
  `runtime/persona_soul.rs`: Write its soul is a modal, Decline retires it, each `persona.soul` or `persona.retire` as the
  presser). Nothing binds a persona to a place but the bindings file's `persona =`. Tests: `tests_personas.rs`,
  `tests_persona_write.rs` (under `tests_ontology`), `engage::tests`, the ontology's `tests_persona.rs`.
- **The owner's agents as personas** (theseus-a8n7, T10b; store format 41; `persona_import/`, `persona_recall.rs`,
  `standing_proposals.rs`, `rpc/persona_import.rs`, `rpc/standing_proposals.rs`, `config/persona_settings.rs`, `[persona_import]`
  because `[personas]` is a map of personas). `persona.import {from, name, agent?, dry_run}` (the import's act:
  `judge_act(Act::Import)`, refused in a job by the CLI; `theseus persona import`) reads an agent's workspace on the
  daemon's machine and carries `[persona_import] import_files` (`SOUL.md`, `IDENTITY.md`, `USER.md`, `AGENTS.md`, `MEMORY.md`,
  and the folder `memory`) **byte for byte** into `<state>/personas/<name>/`; the top-level files are the persona's context
  files (the CLI writes `[personas.<name>] files` into the config), the folder's notes are carried and read for feedback
  but not compiled. A file over the 64 KB context limit is cut at its headings into `NAME.partN.md`, every part verbatim
  (`split.rs`: the parts joined are the file). A later import whose bytes differ is a new version: the replaced files move
  to `.versions/<n>/`, `import.json` (`Manifest`: agent, version, digests, the drafted card) is rewritten, and the next
  compile that sees the digests change writes the usual `persona.changed` row. Nothing is edited, summarized or
  reformatted (`tests.rs`: a CRLF file with curly quotes and no trailing newline comes back identical). **The card** is
  drafted without a model (`card.rs`: the identity file's role line, the agents file's level 1 and 2 headings) and is a
  *draft* while the config's card equals the manifest's draft and the owner has not kept it (`persona edit --confirm-card`
  = `persona.card_confirm`, or edited the text): `persona.list` says `card_draft`, and `offered` (a card and no draft) is
  what route.v5's persona question will read. **Feedback** (`feedback.rs`: a note whose frontmatter says `type:
  feedback` gives its first paragraph, a `## Feedback` heading gives its bullets; `order` when the words command, else
  `preference`) becomes *proposals* (`StandingProposal`, META `standing.proposal:<id>`, ids stable per persona, kind
  and words), filed with the import's `persona.imported` row in one frame and never as a record. `standing.proposals`
  lists them, `standing.keep` is `standing.add` of the words scoped `StandingScope::Persona(name)` and then closes the
  proposal (`standing.proposal_answered`), `standing.dismiss` closes it for good; a closed proposal is never proposed
  again. A persona scope holds for turns whose `Target.persona` is that persona (`standing::Scope.persona`).
  **Recall** (`persona_recall.rs`): while a persona is engaged (the turn's `Target.persona`; route.v5's verdict will
  engage one the same way) `[persona_import] own_boost` (1.2; 1.0 off) multiplies the rank score of an imported episode of its
  own agent (`ImportedFrom.agent`, matched lower case against `Manifest.agent`, else the persona's key) in the pure
  pipeline (`theseus_memory::PersonaBoost` in `Params`; filters see the index's score, so it admits nothing), and that
  item's frozen header begins `[from Wren, 2026-09-14, @4210]` (`render::persona_mark`). A model's own lookups and
  `memory.search` do not boost. **Threads**: `thread.new { place }` takes the place's persona (the bindings file's, when
  the config has it) as the thread's `persona`; the router's new-thread tap and Discord's `/thread new` pass their
  place. Tests: `persona_import/tests.rs`, `rpc/tests_persona_import.rs`, `rpc/tests_standing_proposals.rs`,
  `rpc/tests_persona_threads.rs`, `tests_persona_recall.rs`, theseus-memory's `tests_persona.rs`, `tests_layouts.rs`
  (a format-40 standing record, the persona scope), and the fixtures `tests/fixtures/agents/{wren,finch}` (invented
  workspaces; `.gitattributes` keeps their bytes).
- **The surface tag** (theseus-slvl.3; store format 34; `surface.rs`, `turn/surface_step.rs`): a person's message node
  records the surface it was written from (`Node.surface`: `terminal:ask`, `terminal:tui`, `terminal`, `cockpit`, `mcp`,
  `discord:dm`, `discord:channel:<shared|private>:#name`, `voice`), settled by `Core::surface_of` in `turn.submit` from
  the connection's listener (`approval::Surface`), the client's own word (`TurnSubmitParams.via`: `ask`, `tui`, `voice`;
  anything else reads as none) and, on Discord alone, the session's place and its class. It rides the turn's
  `EventSink.surface` into `Turn::user_node` and a steered message's `Steered.surface`; a connection no listener named
  gets none. The compiler (`user_blocks` -> `surface::lead`) renders `[Terminal · TUI]` and the like at the head of the
  person's text, in the user turn only: layers 1 and 2 are byte-identical across surfaces, the node's text stays as
  typed, and a node's tag is fixed at its write, so a place that moves changes only later messages. The words are one
  table (`surface::tag`); a voice turn keeps its framing line and gets no tag. The header's one sentence is the tail of
  `turn::ASSEMBLY`. A reply is labelled `Theseus (model)` by `theseus_protocol::surface::reply_label` (history, the TUI)
  and the cockpit's `lib/speaker.ts`; until persona data joins, the literal. Tests: `rpc/tests_surface.rs`,
  `surface::tests`, the layouts' format-33 node sample.
- **Initiative with a character** (theseus-slvl.9; no store format change: four ledger kinds and one META key; `initiative/`,
  `turn/promise_step.rs`, `rpc/initiative.rs`, `config/initiative.rs`). What Theseus says first is in its persona. A wake's result
  and a task's report are turns of a session and speak as its persona already (`persona_of`; a task takes its parent's); a
  turn that took a wake or a report writes an `initiative.spoke` row (kind `wake` or `report`, its id the turn's) so a verdict can
  name it. A message the daemon starts itself in words is one short call (`initiative::speak`): the persona's file (the built-in
  default when it has none) and the standing block as its system, one sentence of task, the facts as the message, no tools, on
  `[initiative] profile`, reserved on the day ceiling like any background call and held to `[initiative] day_usd` (0.50, read back
  from today's `initiative.spoke` rows, which `day_ceiling::SPEND_KINDS` counts); past it, or with the model off or unreachable,
  it is its template. **A cut-off answer is never posted** (theseus-050g2: at 200 tokens Sonnet 5.5's default thinking spent the
  cap and a morning note went out as half a sentence): the call asks with thinking off where the model lets it
  (`catalog::thinking_off`: Sonnet 5.5's `between_tools`, the other adaptive models' `disabled`, none for one that always
  thinks) at `speak::MAX_TOKENS` (1,024); a `max_tokens` stop is asked once more at `RETRY_MAX_TOKENS` (2,048) within the line,
  then it is its template, which is also what any stop but `end_turn` or `stop_sequence` gets; the row carries the newest answering
  call's `stop_reason` and `output_tokens` (the cut first call's when the retry was refused by the line or failed). Every capped call tests its stop with `provider::hit_cap` and names the cut with `provider::cap_phrase(cap)`: one place for "this answer hit its cap" (theseus-2c6iz), never an inline `== "max_tokens"`. The template is `initiative/lines.rs`, one table that reads the persona's `name` and
  `emoji` and nothing else of it; `initiative_post` sends it to the owner's DM (courier kind `initiative`, key
  `ini:<id>`) with its row in one frame. Producers:
  the morning note (`initiative/morning.rs`: the driver's tick asks `post_morning_if_due`, one lock and a comparison until the
  day's first ask after `[initiative] morning_hour`; the work is a spawned task, only on a day a task ended, a wake fired, a
  promise went unkept or notes were proposed, once a day by META `initiative.morning`) and the nightly consolidation's finding
  (`initiative/consolidation.rs`, `Proactive::Observation`: chatty only). **The level**: `Initiative::lets` (theseus-protocol) is
  the one table; `Rules::initiative` feeds `notices::policy`, and the daemon's own proactive posts ask the same `lets`.
  **Promises**: `initiative/promise.rs`'s table of looking-ahead phrases; at a turn's end a reply that matches writes `promise.kept`
  when the turn scheduled something (`Pending::scheduled`, bumped by `wake.at`, `watch` and `task.create`) and `promise.unkept`
  when it did not; no row for a reply that promises nothing, so a plain turn writes nothing here. **Verdicts**: `initiative.rate`
  (`initiative/rate.rs`, `judge_act(Act::Session)`: the owner from a private place; the CLI refuses it in a job) writes
  `initiative.rated`, the last verdict on an id standing; Discord's 👍/👎 on a reply or a note calls it. `initiative.week`
  (`initiative/week.rs`) is the measure `theseus persona` shows. **The posted digest** (theseus-ss36w; `initiative/digest.rs`): at each `[discord] digest_at` slot (`"08:00"`, `"18:00"` by
  default; `[]` off) the driver's tick (`post_digest_if_due`, one lock and a comparison until a slot's first ask) begins one
  `work.digest` read since the last digest (META `discord.digest.last`) as the owner's surface and posts it as kind
  `digest` to the owner's DM, only when it has lines; the slot is claimed in memory, marked in META
  `discord.digest.<date>.<HHMM>` in the post's frame (or alone when nothing happened), so a restart posts none twice. `digest`
  is in `pacer::UNPROMPTED`: the slot waits, unclaimed, while the window is closed, within its local day (`due` is pure:
  the latest slot today that has come and is later than the last claimed). Tests: `tests_work_digest.rs`, `digest::tests`.
  Tests: `tests_initiative.rs`, `initiative::promise::tests`,
  `initiative::lines::tests`, the protocol's `tests_notices.rs`.
- **Capture** (theseus-slvl.5; `capture/`, `rpc/capture.rs`, `fact/capture.rs`, `config/standing.rs`; pack `directive.v1`; no store
  format: new META key `standing.capture` and new ledger kinds): the owner's words become standing records without a
  command. `directive.v1` (live, at `inbound`) rides the batch with `classify.v1` and `role.v1` (no call of its own) and is
  asked only for a message `turn.submit` noted as the owner's own (`capture::expect::Expected`, taken at the inbound point;
  never a job's `opened_from`, an MCP client's, a prompt's, or one with files) in a place only the owner writes in
  (`capture::owner_alone`: no place, or an owner's DM) and never a task's. `judge_inbound` hands its judgment to
  `Core::capture_directive` once the batch settles, off the turn's path. The bands are the pack's own (`act` 0.80, `confirm`
  0.50): `capture::decide` (pure) keeps at the act band, asks from the confirm band, and below it only notes a correction
  (`standing.candidate`) to propose it when its words overlap one noted in the last week (`capture::words::same`); an order
  is always asked; a message of the same kind as a live record of its scope and sharing its words is `standing.repeated`
  and no record. The words kept are the message's own on one line (Jev answers choices, not text; over 240 characters
  nothing is captured). A kept record's source is `capture:<node>`, its scope Jev's when its `scope` answer reached the act
  band, else where it was said (a Discord place, else the session). One frame each (`capture/act.rs`): the record, its
  `standing.added` and `standing.captured` rows, the state (`capture/state.rs`: candidates, open asks, repeats, which
  judgment kept which record), and, for Discord-origin words, the `standing_card` post to the owner's DM. The CLI and web
  UI hear the fact's narrative line. `standing.answer` (`rpc/capture.rs`) keeps an ask (`keep`, `everywhere`) or drops it
  (`not`); an undo is `standing.revoke`, which labels the judgment `wrong capture` (`capture_undone`; three in a day roll
  the pack back, and a label finds its pack in the `standing.captured` row while the sink has not written the judgment).
  `[standing] capture = false` stops all of it. Tests: `tests_capture.rs`, `capture::tests`.
- **The task offer** (theseus-bh3t.5; no store change: a field on the reply's outbox post): `classify.v3` is `classify.v2` word for
  word and one question, `offer_task` ("work to be done later, in more than one step, that the person has not asked to start
  now"), which decides nothing and rides the one inbound request. When it answers yes at the act bar in a judgment that acts
  (`inbound::offer_answer`), the batch notes the turn and the first 1,000 characters of the message (`JudgeService::note_offer`,
  64 kept in memory); the reply's post takes them (`turn/reply_step.rs::offer_for`) as `"offer": {"brief"}` when the turn was no
  trivial or quick detour, no task's, in the owner's DM (`capture::owner_alone`), and made no `task_create`. It never waits for
  the judge: a batch that has not landed by the reply costs the reply nothing, and the turn is left waiting
  (`judge/offer.rs`, theseus-ypnxt.5): when the batch lands within two minutes of the reply, the core posts an `offer`
  (`{turn_id, brief}`, one frame, only then) to the reply's place, and the Discord binding adds the button by one edit of
  the reply (`courier/offer.rs`); a later one is dropped, as is one for a turn that made a task. The binding keeps the
  briefs in META (`discord.offers`, the newest 64). Discord turns it into a button (`runtime/start_task.rs`). Judging the reply's own text is a point of its own, not built. Tests: `tests_offer.rs`.
- **Threads** (theseus-nhew; store format 33; `threads.rs`, `rpc/threads.rs`, `fact/thread.rs`, `config/threads.rs`): a thread is
  a durable record bound to a virtual place `thread:<slug>`, so it reuses what a place has. The record is
  `theseus_protocol::threads::ThreadInfo` as a META key `thread:<slug>` (an edit writes the key again, in one frame with
  its `thread.created|edited|paused|resumed|done|reopened` row); the session it runs on is the place's own record
  (`discord.session.thread:<slug>`, written by `Core::bind_place_to`, so supersession, retirement and the states are the
  place's), and its last activity is its session's, so a turn writes nothing for its thread. `threads::Board` is an
  immutable snapshot read once (two META scans, `Core::warm_threads` after serving, or the first reader) and swapped on
  a write: `Core::threads()` gives it, `Board::of_session` the thread a session serves and `Board::live(PlaceClass)` the
  active ones for the router (none for a shared place). A thread is private always: `places::PlaceRule::class` classes
  `thread:*` and `discord:thread:*` private, every method is `judge_act(Act::Thread)` (the owner, from a private place),
  and the outbox gives a virtual place's session no target (`outbox::virtual_place`), so it posts nowhere, as a CLI
  session's. `turn.submit { thread }` runs in `Core::thread_session`: the place's session, or one opened and bound on
  first use and after the session was retired (the owner's `/new` is a retire), under the board's lock so two first
  messages open one; only an active thread takes a message. `thread.done` retires the session and leaves the card in it
  as a harness note (and in its row). Two threads are two executions and run at once, under the one daily ceiling.
  `[threads] enabled|max_live`. The rows built on it (`thread-context`, `thread-recall`, `thread-route`) add their keys
  to `config/threads.rs` and call the board. Tests: `rpc/tests_threads.rs`, `threads::tests`, the layouts' format-33
  sample and format-32 store test in `tests_layouts.rs`.
- **What a question and a finished edit show** (theseus-z6au; `tool_words.rs`): `ToolRuntime::preview` makes a
  `confirm.requested`'s `preview` (an edit's, write's or patch's diff from theseus-tools' `change`, a `proc.run` as a
  shell line with its directory), scrubbed as the model's copy is; the asking call (`toolrun::ask`) and `confirm.list`
  (`rpc/confirms.rs`, which reads the files as they are then) both make it. `scrub_diff` scrubs a result's `meta["diff"]`
  as `result_node_in` writes it, and `fact::tool::ToolEnded` pushes it as `tool.ended`'s `diff`. Tests:
  `tests_tool_words.rs` (a preview equals the run's diff; a planted secret is in neither).
  **On Discord** (theseus-sjzm; `rpc/thread_surface.rs`, `turn/reply_step.rs`): `ThreadInfo.surfaces` is edited whole by
  `thread.edit { surfaces }` (a person, judged) and one at a time by `Core::thread_surfaces` (the Discord binding's own act: a
  Discord thread it made, a channel the config binds; an edit row whose `who` is `discord`/`config`). Every thread fact
  tells `thread.changed` (`created|edited|paused|resumed|done|reopened`, and `opened` when the thread's first message
  opens its session: `ThreadOpened`, no row) to every connection that hears the wide notifications. `Core::thread_place_session(slug,
  target)` is `thread_session` judged by the place (private only) instead of a person, for a channel the binding runs as
  the thread. `turn.submit { thread, speaking { discord, answer_at } }` judges the speaker (a Discord thread turn needs the
  origin) and posts the reply at `answer_at` (the speaker's own DM or channel; any other is refused) instead of the
  session's place: `TurnRequest.answer_at`, read first by `stage_reply`; a retried or continued turn answers at the
  session's place. `[threads] discord_channel` and `[[discord.thread]]` (`config/discord.rs`, `ThreadBind`);
  `PlaceRule::bind_more` keeps a place the binding made while it runs across a new bindings file. Tests:
  `rpc/tests_thread_surface.rs`.
  **On the work board** (theseus-5i8i; `push/work/threads.rs`, `rpc/work_board.rs`, `rpc/tests_thread_board.rs`): every view
  carries `thread`, the thread whose session is its root conversation (`WorkBoard::thread_for_root`, stamped in `put`), and
  the board holds the thread groups (`ThreadGroup`) with the session each serves. Both come from the threads' board in memory
  only: `Threads::on_change` tells `Push::set_threads` after each write or bind (installed by `warm_threads` and by the first
  `work.watch`), which diffs, restamps the views in place (and marks them stale, so the next frame sends them again) and
  publishes the changed groups as `work.changed { threads }`; the seed reads the held board, never the store. A watch's
  snapshot reads each thread's stats once (`Core::thread_groups`: context tokens, today's dollars) and a connection that
  reads no private text is given and sent no group. A group's update leaves the stats it did not read.
- **Threads from the owner's earlier harness** (theseus-71ib, T11; no format bump: a ledger kind and two META keys):
  `thread.import` (`rpc/thread_import.rs`, `threads_import/`), `judge_act(Act::Thread)`, the owner from a private place.
  With no `apply` it is the dry run: `source::scan` reads the imported sessions of every tag holding a source that
  starts with `from` (`openclaw` takes `openclaw-store`, `openclaw-sync`) that ended in the window, their owner
  messages (`Integrity::Operator`) and summary; `candidates::candidates` (pure) makes one candidate of each channel
  conversation (a Slack or Discord channel, by its id) with an owner message in the last `since_days` (30) and, for the
  DMs, one of each topic cluster (an episode's first topic label cut to two path parts) with at least `min_cluster`
  (8) owner messages. The title is the channel's label, the persona the agent's only if `[personas]` has it, the
  topics the episodes' labels (3), the working set what `extract.rs` finds by rules in the owner's lines and
  summaries (repos, `#N` and `owner/name#N` and Beads ids, branches, files, hosts; no model). **A partner-confidential
  episode counts and gives no text**: its messages are in the numbers and the sensitivity counts, and in no title,
  topic, working set or model request; a stream of nothing else is titled `confidential <kind> conversation`.
  With `apply` (slugs, looked up in a fresh scan) each thread is made one at a time: the card's call
  (`card.rs`: Haiku by `[threads] import_profile`, one tool, `write_card`, scrubbed input, the newest 30 episodes within
  24,000 characters), then `thread.new` and `thread.edit` (card, working set, persona), then one `thread.imported` row
  (cost, tokens, the citations as handle, node and session; a day-ceiling spend kind) with the day's spend
  (META `threads.import.day`) in one frame. A call whose worst case passes `[threads] import_day_usd` (2.0) or that the
  day ceiling refuses is not made: the thread is made without a card, the result says why. A card line must cite
  episodes the call offered (`E1`…) and ends in their recall handles (`[m:a1b2c3]`, `cite::handles_for` over the
  episodes' anchor nodes); a line citing none, or repeating a message of 60 characters or more, is dropped and counted
  (`dropped`). **Imported history stays testimony**: no node is written into any session and the thread has no session
  of its own until its first message. The Vestige dump (`--vestige`) is the CLI's: each memory an episode of source
  `vestige` (`episode::VESTIGE_SOURCE`, scrubbed as the harnesses' are), its id the digest of its text. Not here: the
  Discord channel's bind (T6; `ThreadCandidate.channel` and a note say it can be the thread's place), and the
  card's reach into a turn (T2). Tests: `threads_import/tests.rs`, `rpc/tests_thread_import.rs`, theseusd's
  `tests/thread_import.rs`; the invented export is `threads_import/fixture.rs` and
  `tests/fixtures/threads-import/openclaw.jsonl` (`THESEUS_GOLDEN=write` rewrites it).
  **Thread recall and seals** (theseus-f2g0; no store change; owner D12: Jev recommends, the owner seals).
  `thread_recall.rs` maps a candidate's session to its thread (`Board::of_session`, else the record's supersession
  link naming `thread:<slug>`, else its task parent, kept in `Memory::thread_memo`) and builds the pure
  `theseus_memory::ThreadLens` over a recall's candidates (`Scene.threads`, `TurnRunner::thread_scene`: none while no
  thread exists, so a daemon without threads recalls as before). In a threaded session a candidate of *another*
  thread pays `[threads] cross_recall_penalty` (0.35 of its score, in the pack's rank) and is dropped as `thread`
  unless its fused score is above `cross_recall_bar` (0.75) of the query's best, unless the message names its thread
  (slug, title, or a working-set entity: `thread_recall::named`). **Streams do not mix** (theseus-wncdv; the
  threadbench's threads arms answered about another stream's facts on 4 to 7 of 12 cross-stream questions, one
  session on 0: streams share the shape of their facts, so a question about this thread's port scores another's
  nearly as high, and the place's own conversation, where each stream's first messages ran before its thread
  existed, passed undistanced): (1) provenance: `ThreadLens.judge(session, node, fused, tie)` takes a node's
  own thread (`of_node`: `moved.of.<node>`, what a split moved) over its session's, and `[threads] base_distance`
  (true) makes a node no thread claims another stream's to a threaded asker (the same penalty and bar, never
  waived by a name); `cross_recall_beats_own` (true) makes another stream's hit, once it has paid, still outrank the
  asking thread's own best hit (`own_top`, from the candidates it owns, in context or not; a note no thread claims
  weighs only its own not yet in context, `own_top_admissible`, so msg 113's home note gets in: theseus-98bz.8,
  theseus-stwkb); (3) when another thread's notes are pulled: `cross_recall_needs_entity` (true) drops a hit whose
  chunk holds entities (`Tie::Apart`: index entities,
  none of the message's) of another stream, so a question's shape alone pulls none. Each key off restores the
  earlier rule. (2) A prefix carries only the asking stream's own sections: `prefix::fit` refuses a header with a
  section the fresh layer lacks, so no reuse can hold another stream's guidance (`prefix/tests.rs`,
  `a_prefix_carries_only_the_asking_streams_own_sections`); the A1 vs A1n gap was a router decision (a new
  stream's first message held in the sticky thread), so `[prefix] enabled` stays true. Tests:
  `theseus-memory/src/tests_threads.rs`, `rpc/tests_thread_mix.rs`. **The cross judge** (theseus-98bz.10;
  `judge/cross.rs`, `turn/cross_step.rs`, `[threads] cross_recall_judge`, on): the `thread_*` drops whose share of the
  query's best is above `cross_recall_bar` (not sealed, not held), at most `cross_recall_judge_k` (3) best first, go to
  `recall_cross.v1` in one request the turn waits for at most `route_wait_ms`; a Noul yes at the confirm bar in a
  judgment that acts admits the note with the penalty, anything else leaves the drop. Its failures move only its own
  breaker (`cross::BREAKER`, `recall_cross`, theseus-pt7lt; the `judge.circuit` row names it), and while it is open
  `cross_live` sends nothing and waits for nothing. **The place digest** (theseus-98bz.9; `[threads] place_digest`, on):
  `classify.v4`'s `scope` question (asked when the place has live threads, or in a thread's own session with
  `Inbound.in_thread`, which asks it without the list) answered `place` at its act bar gives that turn's request a
  digest of the place's other live threads (`place_digest_line_chars` a line), and a turn with the digest asks the cross
  judge nothing (`tests_cross_judge::no_call_when_the_digest_fired`, which also asserts a drop above the bar, so it
  cannot pass with nothing to guard). Tests: `rpc/tests_cross_judge.rs`. A sealed thread's notes, or one with a seal
  proposed (`seal_pending`), are dropped as `sealed` for every session but its own, whatever the message names, the
  rerank and `thread.read`-less lookups included (`memory.lookup`'s words ask with the seal and no distance). The
  `Recall` item of another thread's note begins its frozen header `[from thread «title», day, @pos]`
  (`render::thread_mark`). `rpc/thread_seal.rs`: `thread.seal` (+`redirect`: a harness note in each session the
  notes had crossed into, found from the `derived_from` edges, via `recall`, of `Recall` nodes), `thread.unseal`,
  `thread.seal_dismiss` ("Not needed": a `thread.seal_dismissed` row, a label for the judge's report, and META
  `thread.seal_dismissed.<slug>` holding the content digest, so one proposal per thread until its card, goal, title or
  working set changes), `Core::thread_recommend_seal` (the entry point a later Jev pack calls, row theseus-c1uz: sets
  `seal_pending`, writes `thread.seal_proposed` and posts a `seal_card` to the owner's DM, Discord's Seal this thread /
  Not needed), and `seal_by_word` (the owner's `/seal` or "seal this thread" in a thread's session, before the turn).
  `thread.new { sealed, split_from }`: a split from a sealed thread starts sealed; a task's session is its parent's
  thread; `threads::merged_seal` is the rule for a merge. `thread_read.rs`: the model tool `thread.read {thread,
  query?}` (private places; the card and the top hits, cited by thread and position; `gate` raises it to `approve`
  when the thread is sealed or proposed). Tests: `rpc/tests_thread_recall.rs`, theseus-memory's `tests_threads.rs`.

- **The thread router** (theseus-rha2; no store change): an unaddressed message in a place with live threads finds its
  thread (never one `theseus ask -s` named: `via` is `ask`, `may_route`, theseus-0rfjx). `rpc/thread_flow.rs`
  (`route_to_thread`, called from `turn_submit` with `route` true) runs in this order: addressing by place, an
  explicit word (`#slug`, `/t slug`, "in the … thread:") and a reply to a routed turn
  (`thread_route::address`, certain, no judge); then the in-memory pre-router (`thread_route.rs`, pure: entity matches
  against each live thread's working set, stickiness `[threads] sticky_half_life_secs`, the reply chain, fused as a
  noisy-or, a thread whose title the message names in every content word adding 0.35 unshared (`thread_title.rs`,
  theseus-4qdh8: two titles named contend and are asked; a title names its thread as an entity does, so it halves
  another thread's stickiness and takes the replier's chain, and an apart thread keeps it, theseus-7nx13; with Jev down
  a single title named routes there as a guess, `GUESS` 0.30 under the title's 0.35); `thread_flow.rs` holds its
  `State`: the last thread per
  home session, routed turns, pending asks, correction memory); then `classify.v2`'s `thread` question
  (`Source::Threads`, at most 50 options plus `new_thread`
  and `home`), which rides **the same inbound request** as the rest of the batch: `JudgeService::inbound_early` begins
  the point before the turn and the turn takes the handed-off `Early` (keyed by session, with a pre-minted turn id), so
  routing adds no model call (`tests_thread_route::jevs_thread_answer_rides_the_one_inbound_request…` holds
  `jev.seen().len() == 1`). `thread_route::decide` is the table: bar 0.90 quiet (a tag in the reply), 0.60 to 0.90 a
  visible tag with a move handle, below that one question (`thread.asked`, answered by `thread.answer`, with `[1] [2]
  [n]ew [h]ome` in the CLI and buttons on Discord), a new-thread offer on one tap, a one-off at home, and only two or
  more live threads wait. `[threads] route_wait_ms` (400) bounds the wait: a timeout keeps the pre-router's pick as a
  `guess` and the `thread.routed` row counts it (`timed_out`). **The misroute guard** (`toolrun/misroute.rs`, armed by
  `arm_guard` for a placement below the act bar only): the session's first write or `proc.run` outside the thread's
  working-set roots (a relative one read under the session's directory, `cwd_for`, the one the working set grew it in)
  goes on once at the notify posture with a one-line notice (path, thread, the next likeliest thread, `Undo: files.restore <path>`; a stricter decision is kept; theseus-g85jy); a `proc.run` from a directory above the roots (the session's directory)
  decides nothing, and the next write or program does. **Recovery** (`rpc/thread_move.rs`): `/move`, `theseus thread
  move`, or "that was for …" writes a redirect note in the wrong thread, re-submits the message once to the right
  one, and writes a correction (a `wrong thread` label on the `thread` question, and `Steer::Thread` in the correction
  layer, so the next close message follows). A thread session's replies go to the place the message came from through
  the outbox overlay (`Outbox::show_at`). Known limits: pending asks, router memory and thread corrections are in memory
  (lost at a restart); a thread session's live stream does not reach the Discord place actor (its durable post does); the
  early judgment's rows carry the home session. Tests: `rpc/tests_thread_route.rs`, `thread_route::tests`,
  theseus-judge's `tests_threads.rs`; the 40-message fixture `fixtures/thread_route_40.json` is also
  `scripts/thread-route-check.py`'s input.
  **The ask and the sticky rule** (theseus-98bz.2, theseus-eqp1v, theseus-98bz.4, theseus-q3ggn): an ask's options are
  its threads (`[threads] ask_every_thread`: every live, unsealed one up to four, else three and `other`, which an
  answer of a slug fills; off, the best two), `new` when raised, and `home`. With `sticky_alone_asks` (on) a top thread
  that stands on stickiness and the chain alone (no entity, no learned correction) and that Jev did not pick is held
  just under the confirm bar, so the message is asked about; the `new` option rides that ask alone when the message
  names two or more entities no working set holds (`new_entities`: the import's extractor, a ticket shape `OPS-4214`
  read as a ref), and any ask Jev raised `new` in. **The directory rule** (theseus-vw8k7; `thread_route::apart_by_dir`,
  called by `Core::keep_apart_by_dir` in `rpc/home_claim.rs` right after `learned_thread`): with the turn's directory
  known (the submit's `dir`, else the session's), a thread whose roots are set (absolute or `~/`; a relative one read
  under the thread's own session's directory) and none of which holds the directory or lies under it keeps only its
  entity score: stickiness, the chain and a learned correction are dropped; Jev's pick still fuses. A thread with no
  readable root, or a turn with no directory, is scored as before. What the rule drops is then scored as if it had never been there (`thread_route::settle`, shared with `pre_route`: another thread's entity counts less, and stickiness halves, only while another thread is still named; theseus-ax4dp). The roots predicate is `thread_route::holds_path`,
  which `home_claim::thread_of` reads too. Tests: `rpc/tests_thread_route.rs`
  (`a_message_from_another_directory_is_not_placed_by_stickiness`), `thread_route::tests`.
  **A thread's working set grows from its turns** (theseus-98bz.5; `thread_grow.rs`, `rpc/thread_grow.rs`, `[threads] working_set_grows`): after a turn in a thread (`routing.slug`, else `Board::of_session`), one page of the session's nodes after the turn's start gives its calls' paths (relative to the session's directory: the file and its first component as a root) and the message's entities (`extract::working_set`) join the set, but for what another live thread holds; one frame with a `thread.edited` row by the harness, only when the set changed. A list takes nothing past `thread_join::MAX_SET` (64), and nothing is evicted yet. A message's bare file name (`slow.sh`) grows nothing, only a path does (`thread_grow::paths_only`); a message the owner moves takes back what its words grew into the thread it ran in (`thread_ungrow`, from `State::take_grew`), and the thread it moves to grows from the message's resubmitted turn (theseus-iw50k). Tests: `rpc/tests_thread_grow.rs`, `thread_grow::tests`.
  **The bench's clock** (theseus-98bz.3; `[threads] bench_clock`, false, a test hook): a `turn.submit` carrying `route_at_ms` (`TurnSubmitParams.route_at_ms`) is routed at that clock, `ask -s` included, and refused when the key is off; the threadbench sends each message's plan time. Tests: `rpc/tests_thread_bench.rs`.
  **A lone thread offers a new one** (theseus-98bz.7; `thread_route::lone_offer`, `[threads] single_offers_new`): with one live thread below the confirm bar and two or more entities no working set holds, the message is asked about (the thread, new, home); a spoken turn runs at home. Tests: `rpc/tests_thread_lone.rs`.
- **Threads kept warm** (theseus-ytt8, T9; `keep_warm_threads.rs`): a thread's session is a private conversation, so the
  keep-warm reads it as it reads any; what differs is who is kept. An active thread whose owner wrote within `[threads]
  keep_warm_hours` (4; 0 keeps none), the shorter of that and the profile's window, never a paused or done one:
  `thread.pause` and `thread.done` forget the session at once (`KeepWarm::forget`), a read re-checks the thread's state
  (`TurnRunner::thread_warm`), a restart leaves out what is not active, and a resume holds nothing until its next owner
  message. One request per thread: its own prefix includes the layers every thread on the profile shares, so its read
  restarts those entries too, and a tail-only request would be a different prefix. The tender reads the most recently
  written first (`KeepWarm::due`) and, when the day's line refuses a read, leaves the rest of the round unread, counts
  them (`skipped_today`) and warns; they stay held and are due an interval later. The numbers (`ThreadCacheStats`:
  cold resumes and their rewrite tokens and dollars, warm hits, rewrites avoided and the modeled saving, keep-warm reads
  and dollars) are `stats_of` over the day's `provider.call` (a turn's first call has loop 0) and `keep_warm` rows;
  `thread show`/`list` carry a thread's, health's `threads.cache` the total and each thread's, from a copy a minute old
  (`CacheNumbers`) so health reads no ledger, and nothing for a daemon with no thread. Tests:
  `rpc/tests_thread_keep_warm.rs`, `keep_warm_threads::tests`.
  **Merge and split** (theseus-vt9k; `thread_join.rs`, `thread_split.rs`, `rpc/thread_join.rs`; no store format change:
  the fields were T1's, the one new key is a META record). `thread.merge {from, into}` is one frame: B's record (A's card
  quoted at its end under `merged from «a», <date>:`, at most six lines while B has lines of its own, A's working set
  and topics joined, three topics at most with the rest named in the result for the owner to pick, a seal on either
  kept), A's record (`state: done`, `merged_into: B`, its surfaces cleared), the META `thread.merge.<a>` (what the undo
  needs: A's state, surfaces, and the session the merge retired) and the `thread.merged` row (A's whole card and the
  surfaces it archived). A's session then gets a note at its tail and is retired, a frame each; nothing of A is
  deleted and B's session is untouched (its card snapshot refreshes at its next recompile, as T2's does).
  `Board::live_of(a)` follows `merged_into` to the thread that is live now (the distance rule's reader), `links_of`
  the other ends of `merged_into` and `split_from` (`ThreadRow.merged_from`, `split_to`). `thread.unmerge {slug}` is one
  frame that puts A back and gives B a card line, then reopens the session the merge retired. `thread.split {from, slug,
  range | positions}` (WAL positions, the `@` numbers of `theseus history`) opens the new thread's session first
  (`opened_from` the source, so it holds what the source holds), writes its brief and one `Body::Arrangement` of the
  chosen messages (role `context`, verbatim, an edge to each quoted node), then writes the thread, its place record and
  the `thread.split` row in one frame with the session already bound (`Threads::write_bound`), then one
  `Body::Moved` node at the source's tail (see **Home split** below): the source keeps every node in the store, but its
  compile leaves them out. The new thread inherits topics, persona and seal; its working set is read from what the chosen turns did
  (paths, hosts, `#` refs: `thread_split::working_set_of`). The model's `thread.split {slug, from, to?}` (a thread
  session's tool, async, the owner's tap at every posture by `Plan::authority`) quotes the first message and
  optionally the last, resolved as an arrangement's quotes are; its result is the source's note (it quotes only: the
  running turn's own compile is not rewritten mid-turn, so it writes no `Moved` node and takes no whole-turn check). A
  conversation's first compile admits an arrangement (`compiler::situation`) for it. Tests: `rpc/tests_thread_join.rs`,
  `thread_split::tests`, `thread_join::tests`.
  **Home split** (theseus-q9b85; `rpc/thread_home.rs`, `moved.rs`, `split_turns.rs`, `compiler/situation.rs`; store format
  47: `Body::Moved`, the META keys `thread.split.<id>` and `moved.of.<node>`, the ledger kind `thread.unsplit`).
  `thread.split` (the owner's act) takes a thread's slug, a place's own session by its whole id (`ses_…`), or
  `home@<place>` as `from`; `range`, `ranges` or `positions` name the turns, which must be whole (`split_turns::close`:
  every node of a `turn_id` and every call, its reply and its result move together, a turn's recall notes with their
  turn; a pick that cuts one is refused with the positions to type). The target is a new thread `slug`, or the existing
  thread `into`, which takes the brief and the arrangement at its tail (its prefix unchanged), never across a seal.
  Over `MAX_PIECES` or `MAX_CHARS` is refused with "split it in parts": a large stream moves as several splits into the
  same thread. The source keeps every node, and one `Body::Moved` node (split id, thread, the moved ids, the first and
  last position, the gist: `--gist`, else the first human message cut to 80 characters) takes them out of its compile:
  `compiler::compile_with` reads the stubs' `Moved` records (`moved::read`, decoding only those), selects a recompile's
  nodes without the hidden ones (`Moves::shows`), recompiles once, trigger `moved`, when its frozen compilation holds one
  of them or a record since undone (`Moves::stale`), places the note `moved to thread «X»: <gist>` before the first node
  after the range began (`moved::in_place`, called by `situation::selected`), and keeps the hidden nodes out of a tail
  (`selected`'s tail filter) where the note renders at its own place, so a range that was in the tail changes no prefix.
  The nodes stay in the store: `session.history` marks them (`detail.moved_to`), and recall indexes them as before: the
  turn's `in_context` leaves them out of the source's, so it may bring them back, and a recalled node's header names the
  thread it moved to (`moved.of.<node>`, `ThreadScene::other_title_of`). `thread.unsplit {split}` writes a second `Moved`
  node that `undoes` the first (the source reads the range again in its own place), a quiet `Moved` node in an existing
  target that hides the quoted turns without a note, and `thread.done` on a thread the split opened; a ledger row
  (`thread.unsplit`) records it and nothing is deleted. `thread.split_plan` is the dry run: it plans as a split does,
  writes nothing, and compiles the source once more with a hypothetical `Moved` node (`census_request_with`) for its
  before and after tokens. The cache: the source's prefix changes once, at the next turn after a split; no other
  turn's does. Tests: `rpc/tests_thread_home.rs` (the key test, into, unsplit, recall, dry run, refusals),
  `moved::tests`, `split_turns::tests`, `tests_layouts` (format 46 opens and takes a `Moved` node).
  **Linked threads** (theseus-98bz.11; store format 48: `ThreadInfo.links`, the ledger kinds `thread.linked`, `thread.unlinked` and, for part B, `thread.link_proposed`;
  `rpc/thread_link.rs`, `fact/thread.rs`). `thread.link {slug, other}` and `thread.unlink` are the owner's act (`judge_thread_pair`, from a
  private place; no model tool reaches them): one frame holds both records (each holds the other's slug, sorted, at most 8) and the row; a
  link that holds and an unlink that is absent write nothing; a sealed, seal-pending, merged or unknown end, or one linked with itself, is
  refused with the reason, and an unlink is allowed whatever the seal. `ThreadScene` takes `Board::linked_with(here)` (the ends of its
  `links` and every thread whose `links` hold it, each with the live thread a merge left it in) into `waived`, so a linked thread's notes
  pass unpaid and ask Jev nothing; `ThreadLens::judge` checks the seal before `waived`, so a seal set after the link still holds them out.
  **Jev offers a link once** (`thread_link_seen.rs`, `[threads] link_proposals`, on): in `cross_judged`, once Jev's yes is admitted,
  each other thread it drew on (live, unsealed, not merged, not linked, never the place's own conversation) is counted once a
  recall, in a spawned blocking task after the recall (META `thread.link_seen.<a>.<b>`, the slugs sorted: `{recalls, asked}`; no
  frame of any turn's). At a pair's second recall, with the owner's DM bound, one frame holds the record with `asked`, a
  `thread.link_proposed` row and one outbox post to the owner's DM: `{kind: "link_card", text, here, other}` (theseus-98bz.12),
  which Discord draws with **Link these threads** (`thread.link` as the presser, judged as `theseus thread link` is) and **Not
  needed** (writes nothing); its words end with the CLI form, `theseus thread link a b`. A pair is asked once, whatever the owner
  did. Off, nothing is counted, written or posted. Discord's `/thread link` and `/thread unlink` are the same calls.
  Tests: `rpc/tests_thread_link.rs`, the layouts' format-47 thread sample.
  **A home turn's claim** (theseus-98bz.6; no store format change: the META key `thread.claim.<id>`, `claim_` values of
  `moved.of.<node>`, the ledger kinds `thread.claimed` and `thread.unclaimed`; `rpc/home_claim.rs`, `[threads] claim_home_turns`, on):
  a routed turn that ran at home (`thread_routed_turn`, no slug) whose result's files, all of them (`more` zero), fall in exactly one
  live, unsealed thread's working set (`home_claim::thread_of`, pure: a `files` entry, or under a `roots` entry, a relative one under
  the turn's directory) has its own nodes (paged back from the session's newest, at most 1,024) keyed to that thread by a `MovedTo`
  whose `split` is `claim_…` (`MovedTo::is_claim`), one frame with the claim's record and its row; a node a split moved keeps its
  value. The lens and recall read it as a split's; the home's compile, which reads `Body::Moved` nodes, is unchanged. `history`
  marks it `claimed_by` (never `moved_to`), and a split may take a claimed node (`refuse_moved`). `thread.unclaim` (`theseus thread
  unclaim ID`) writes null where the key still holds the claim's value; its `kept` counts only keys holding another claim's
  or a split's value (a null one is neither, theseus-l4n39). The claim and its undo read the keys and write in one frame
  under the home session's execution lock, the lock a split's and an unsplit's frames hold, so a split lands wholly
  before or after them. Tests: `rpc/tests_home_claim.rs`, `home_claim::tests`.
- **Tool calls**: `toolrun.rs` (every call the model makes becomes a kernel action: the gate and the dispatch), with
  a job's call in `toolrun/job.rs` (its turn waits on the job's wake, `toolrun/waits.rs`, and takes the job's
  completion with its result in one frame; Tier 7.1; a command that never started says why, `toolrun/not_started.rs`,
  never "(no output)": theseus-f7tz), a `proc.run` batch's steps in `toolrun/steps.rs` (each a job
  under the call's one correlation id, the next launched once one exits 0, the turn's wait held across them so
  the drain never settles the call between two; theseus-7gir.3) and its gate in `toolrun/batch.rs` (`judge`: each
  step as the call alone, the strictest taken), which calls of one response run together in `toolrun/parallel.rs`
  (theseus-d1hi: reads with reads; writes with writes to other paths; programs with programs; at most `[tools]
  parallel_runs`, `parallel_reads` or `parallel_writes` of a group at once; two programs to one terminal or MCP server,
  naming one file, or sharing an operand (a service, container, host, port, directory: `toolrun/operands.rs`, the rule
  in parallel.rs's header, theseus-0hufe) in order, the later's result saying `[started after call N, which names `x`
  too]`; a class change a barrier; each result of a group of writes or programs ends saying how many ran beside it, theseus-da46; tests
  `tests_m3/parallel/runs.rs` and `order.rs`), the continuation in `toolrun/resume.rs`, and the results
  no call's own run writes (late ones, and a cancel's) in `toolrun/late.rs` (theseus-5gw9). A call's span is
  built in the turn that answers it (`turn/calls.rs`, theseus-8pei): `run_tools`'s calls in their loop, and the
  calls `resume` answered (`ResumeOutcome.ran`) and the late results `absorb` took (`LateCall`, a point with the
  job's `run_ms`) under the continuation's span; the tool metrics count each call once, at its answer. A result the
  cap cuts keeps its whole output (theseus-v73m; `toolrun/kept.rs`, `outputs.rs`): a job's (`proc.run`, a batch's
  step: its raw file scrubbed again in a streaming pass, to its 64 MiB, never the tail alone) and a `term.read`'s,
  0600 at `<state>/outputs/<session>/<call>.out`, named in the node's `full_ref` and in the cut line with the lines
  left out, counted in the whole output, and the `fs_read` that reads them; that read is judged as one inside the
  roots (`Outputs::own_read`, in `order.rs`, the session's own folder alone). The raw file is still deleted. The
  sweep (`Core::sweep_outputs`, with the spool's hourly sweep) deletes a retired session's copies, any past
  `[tools] outputs_keep_days`, and the oldest past `outputs_max_bytes`. `fs.read` has its own cap
  (`Tool::result_max_chars`, 100,000), cut as a contiguous head, never head and tail. Tests: `tests_kept.rs`. The gate's parts are
  `policy.rs` (postures and the floor), `external.rs` (the hold after external text: `[policy] external_text` is optional and, unset, follows `enforcement` (`PolicyConfig::external_mode`; `open` notifies too); under notify a hold tells once, its first call that acts, marked by the META key `external.told.<session>` holding the hold's `since_ms`, written in that call's frame, so a restart does not tell twice and a new hold tells again; ask waits on every call; no expiry, an old hold still holds), `broker.rs` (granted
  secrets), `approval.rs` (who answers, and from where), `peer.rs` (the web UI's other-uid check at accept), and `peer_proc.rs` (who is behind a CLI answer).
  A URL whose host is a private address waits for approval (`listed`) and only an approved fetch reaches it,
  unless `[policy] private_addresses = "open"` (theseus-7gir.20, the bench profile's), which lifts both
  (`web::net::PrivateAddresses`, `Web.private`). Plus the harness's own tools,
  `task.rs` and `wake.rs`, the web tools in `web/`, and AWS in `aws/`: the bound accounts, each key's check after
  serving (its calls fail closed until STS names the account), who signs (`session.rs`: the key until the config
  names `owner_role`, then work, job, floor, and tender sessions, and the key signs only STS), `aws.call` (reads,
  writes, runs; its plan reads `theseus-aws-guard`: a guardrail is the floor, IaC-only and stack writes are invalid
  input, deletions of what holds state wait), `aws.describe`, `aws.whoami`, `aws.s3.list`, the stack tools
  (`stack.rs`: plan, apply bound to the change set's digest, status, delete), `aws.cost` (`cost.rs`), the bootstrap
  (`bootstrap.rs`, `rpc/aws.rs`: a read-only plan, the apply on the operator's yes), and the tenders after serving
  (`tend.rs`: the budget's reconcile and line, GuardDuty's weekly usage), and the durability tender (`durable.rs`,
  step 15: with `durability = true`, the WAL's sealed segments, the open one's tails, and blobs to S3, and index rows
  to DynamoDB, in the session `theseus-durability`, from a cursor in `<state>/durability/` saved after every object,
  so a restart asks S3 instead of sending again). Its tests are `aws/tests.rs` (C1), `aws/tests_c2.rs` (a fake
  CloudFormation with state), `aws/tests_durable.rs` (a fake S3 and DynamoDB with state, binary bodies, and
  checksums, and a missing key answered 404 or 403 by the session's policy, GET and HEAD alike, judged as real S3
  judges it: the implied `s3:ListBucket` carries the key as `s3:prefix`), `aws/tests_list_prefix.rs` (`ListItsPrefix`
  under `StringLike`: each session lists only its own prefix, theseus-bfk9), `aws/tests_synced.rs`
  (only frames the writer synced ship: `Hooks::synced_to`, `Store::synced_to`, theseus-mgw.12), and
  `aws/tests_restore.rs`; `config/aws.rs` holds `[aws]`'s types and checks.
- **A job's handle** (theseus-n8gk): a job that goes on in the background, past `proc_sync_secs` or at once with
  `proc.run {background: true}`, gets a short id in its session (`j1`, `j2`, …; `toolrun/handles.rs`), which its
  placeholder's meta names (`job`), so a restart reads the map again from the transcript; the long id still works.
  A job a restart found alive gets its short id too (`toolrun/resume.rs`, `check_dispatched`: the same meta, and words naming
  its command and the job tools).
  `job.read`, `job.wait` and `job.stop` (schemas in theseus-tools' `jobs.rs`) run in `toolrun/jobs_tools.rs`, through
  the core (`Board`, attached as `memory.lookup`'s is): a read is the one tail reader, `toolrun/peek.rs`
  (`crate::toolrun::peek`, bounded: 64 KiB by seek, lines counted only up to 8 MiB), which the work board's
  `work.peek` is to call too; a wait wakes on `JobWaits::settled` (the drain's take, a stop's and the reconciler's
  wakes) and takes nothing; a stop is `Kernel::stop_call` and `terminate_all`. The gate finds the job in the calling
  session (`job_planned`: another session's is refused by name) and judges `job.stop` as a run of the job's own
  argv and directory. A wait's or a stop's result that gives the job's end names it (`delivers`), and the late result
  the end still writes says only that, and asks for no turn of its own (`LateCall.delivered`). A job that ends
  while its turn runs reaches the model at the turn's next loop: `take_late` runs at the top of each loop after the
  first, writing nothing when nothing settled. Tests: `tests_job_handles.rs`, and theseusd's `tests/job_handles.rs`
  (real wrappers: a wait, and a stop that leaves no process).
  **A wait lasts as long as it asked** (theseus-7m7w.3): `job.wait`'s `timeout_secs` (default `proc_sync_secs`) is clamped to
  `[tools] job_wait_max_secs` (1800; at least `proc_sync_secs`, at most the tool's deadline, `WAIT_SECS_MAX`), not to
  `proc_sync_secs`: the job's end, a person's message (steer's signal) and a stop still end it at once, so the cap only
  bounds a wait nothing ends. The result's "Still running after N s" names the real wait, and what was asked when the
  cap cut it; the tool's description gets the cap appended (`jobs_tools::say_wait_cap`). The wait measures with
  `tokio::time::Instant`, so a paused clock drives it. **A wait that outlasts the cache keeps it warm**
  (`keep_warm_wait.rs`): a response that asks for a `job_wait` has `TurnRunner::stash_for_wait` keep the request it
  answers (one clone, only for such a response, only for a request that caches and a prefix of at least `[cache]
  keep_warm_min_tokens`); the wait sends it again at `MAX_TOKENS` through `keep_warm_send` (the between-turns read's own
  send: reserved, held on the day ceiling, booked as a `keep_warm` row whose data says `during_wait`) every 4 minutes
  of a 5-minute entry, or `[cache] keep_warm_every_minutes` of an hour's, beside its other events, so a settle, a message or a
  stop ends it and a read in flight is dropped unbooked. The stash dies with the turn (`KeepWarm::turn_ended`). Tests:
  `tests_wait_cost.rs` (paused clock, `Gated` launcher holding the job; the measure prints with `--nocapture`).
  **`task.wait` is the turn's wait on its own tasks** (theseus-z9mze.2; `task_wait.rs`, an async tool run through `run_job_tool`'s
  dispatch, family `task`): it holds the turn until the tasks it names have ended, then writes each ended task's report as
  the relayed harness node `read_reports` writes (`task:<short>`, never a `Kind::ToolResult`, so clearing cannot stub a
  delivered finding), taken with `Kernel::take_reports_in_turn` so the next turn writes none again, and returns a short
  index (at most `INDEX_MAX_CHARS`). Ended by a task's end (`JobWaits::task_settled`, fired by `settled()` and at a session's
  end, with the one-second look as the backstop), a person's message, a stop, or the timeout (`clamp_timeout`: 30 s to `[tools]
  fan_out_max_wait_secs`, and less `max(60 s, 15 %)` of the turn's deadline); a task still running is told in a note and stays
  on the parent's `reports`. It lends its admission slot after `LEND_AFTER` (helper turns are `Class::Task`), keeps the cache
  warm as `job_wait` does (`stash_for_wait` also accepts the breakpoint the task view moves into the last block), and bounds the
  fan-in: `report_chars` each, `[tools] fan_in_chars` and half the room before the clearing line in all (`KeepWarm::room`,
  recorded by `stash_for_wait`), the rest of a cut report kept in the session's outputs. `task_fan_out` calls `wait_on_tasks`.
  Tests: `tests_task_wait.rs`. `task_wait` and `task_fan_out` are the family `helpers` (`tools_defer/family.rs`), which no place
  loads and no cue names yet, so they add nothing to the owner's DM task flow's request, and a shared place is not offered them
  (`places::public_tool`).
  **`task.fan_out` starts helpers that read one area each** (theseus-z9mze.3; `task_fan_out.rs`, `turn/helper_step.rs`): each is a
  task opened through `task::create` (the path `task.create` runs, given the helper's id and its group) as `exe_Xh<i>`, grouped by `Opening::helper`, its objective the
  turn's own operator message (quoted by id), its brief wrapped with one report-format line. **No count cap, by the owner's word**
  ("very very lightweight on any limits to theseus tasking ... notify, work, and allow feedback"; "All recs!" on the cap question):
  helpers past the free slots queue; a call is refused only for the floor (`[tools] helper_floor_usd`, never under twice the
  worst case one call reserves at `helper_max_output_tokens`), depth, or a bad input. The total is `budget_usd` or half of what the
  session has left (half of the greater of that and its limit under a notify limit), split equally. A helper's tools are the
  reader set (`reads`: class `Read`, less the task, job, channel, term and plan tools; `helper_spec` narrows the request,
  `helper_refusal` refuses a call outside it); its calls' output is capped at `helper_max_output_tokens` (`helper_target`); at 75 %
  of its carve it is told once to report (`helper_step::helper_note`); at its carve under a limit that asks it ends with the
  reason `budget` and no question (`helper_step::at_carve`, `join_step::task_end`). Under the default, a limit that notifies,
  a carve is a notice and nothing ends a helper until the kernel's hard stop. Tests: `tests_task_fan_out.rs`.
  **A silent job's wait returns at a beat** (theseus-53ujg.6; `[tools] job_wait_beat_secs`, 600, 0 off, else at
  most 3600 and tied to neither `proc_sync_secs` nor the cap, since the bench profile's 900 s foreground wait keeps it; `toolrun/jobs_tools/wait_facts.rs`): a job whose spool output file has not
  grown for that long in this wait (each look stats it; growth restarts the window) returns the wait early, with
  meta `beat` and `silent_secs`, and every still-running result carries the same facts: the job's run time and this
  wait's, its CPU time and share of one CPU where its cgroup gives one (`theseus_kernel::cgroup::job_cpu_usec`; none
  for an L1 job or a daemon with no delegated cgroup, and then no CPU words), and its output through `peek` (no output
  since it started, or no new output for N s and the last 10 lines). Facts only, no advice. The beat is off for a job
  past its file's head. With the beat 0 the result is byte for byte the old one. Tests: `tests_wait_beat.rs` (a child
  of `tests_wait_cost.rs`, whose config turns the beat off), `wait_facts::tests`, the kernel's cgroup test.
  At each beat the session's place also gets one status line, a `notice` post written from the facts the beat holds
  (the job, its run and quiet time, its CPU share where a cgroup reads, its last line, scrubbed; `[tools]
  job_wait_status`, on; no model call, `jobs_tools/wait_status.rs`), and `waits::LOOK` stays at 60 s or less (a test
  pins it).
- **Hands** (step 40, theseus-mgw.6 and .11): `aws/hands/`. `aws.hands.run` (`tool.rs`, `launch.rs`: the request, the
  backend Lambda or Fargate as §3.3 chooses, the stacks' outputs read once per account, each launch and its tags) runs
  through `toolrun/hands.rs`, not `run_inproc`: its call answers `background`, and the group's aggregate is its late
  result (`job_result` hands it to `hands_result`). A group (`group.rs`) is the call's action, a META record
  `aws.hands.group.<group>`, and one kernel action per hand (tool `aws.hand`, its `resource` the group's key), all
  written in one frame before anything launches; a dispatch is a hand's claim. The poller (`poller.rs`,
  `Core::poll_hands_after_serving`) long-polls the completion queue only while a group or a hand is open, checks each
  envelope's HMAC (`envelope.rs`: HKDF of the account's secret and the correlation id, nothing stored), quarantines a
  failure (`completion.quarantined` with its `why`), and settles through `Kernel::accept_completion`. The `hand` role
  (`hand.rs`) is `theseusd hand`. Part 2 (theseus-mgw.11): a running hand stops by its backend (`cancel.rs`:
  Fargate's `StopTask`, verified `ecs` once `DescribeTasks` or ECS's own event shows STOPPED and booked at the time
  it ran; Lambda's `unsupported`, its timeout the bound, its late envelope booking its cost), when `until` is met and
  when a cancel or `/stop` reaches the group's call through `ToolRuntime::terminate_all`. Each hand reserves its
  worst case in the group's frame, and a group over the session's budget asks its question as a model call does
  (`Hands::take_over_budget`, read at the top of the turn's next loop). The heartbeat leaves hands to their own
  reconciler (`overdue.rs`: asked about with `DescribeTasks` before unknown; one the TTL reaper stopped fails with
  its reason). `quota.rs` caps a group's running hands by the account's quota, read once an hour, so a big group
  launches in waves. `watch.rs` is health's hands block, the hour's meter (`hourly_alert_usd`, alert only, its mark
  a META record), and the reaper's failures off the queue; `grid.rs` is `hands.list` and Discord's one line per
  group (a `hands` post under the group's key). Tests: `aws/hands/tests_hand.rs` (the role), `tests_hands.rs`
  (through the core, a fake AWS with a queue), `tests_part2.rs` (part 2 on the same fake), and theseusd's
  `tests/hands.rs` (a real daemon `kill -9`'d mid-group).
- **Language servers** (L2, theseus-n88g.8): `lsp/` over theseus-lsp's client. `Board` starts one server per
  (server, root) at the first call for a file of its language, never before (the root: the nearest marker inside the
  workspace roots; for Rust the nearest `Cargo.toml` with `[workspace]`, else the topmost), through
  `children::spawn(Kind::Owned)` in its own process group with the job environment, its stderr in a capped
  `<state>/lsp/<server>-<hash>.log`. It stops one idle for `[lsp] idle_stop_mins`, one whose request timed out, and
  all at the daemon's stop (SIGTERM, never waited for); a crash is `lsp.failed`; the next call starts it again. The
  tools (`lsp/tools.rs`) are async reads addressed by path, 1-based line, and symbol text; the rename is two calls
  (`lsp/rename.rs`, as the stack tools: `lsp.rename.plan` keeps the edit by digest, `lsp.rename` writes exactly
  it, its files the plan's resources). `lsp::gate`, after the call's own order in `toolrun`'s gate, judges a call
  that would start a server at `proc.run`'s posture for its argv (once per root in a daemon's life) and makes a
  rename's write outside the roots wait. A cancel or `/stop` aborts the call's task, so the client sends
  `$/cancelRequest`. Each request is an `lsp.request` span under its call (`Board::bind`, `spans`) and the metric
  `theseus.lsp.request.duration`; the facts are `fact/lsp.rs`; health's `lsp` block is `Board::health`. Never
  offered in a shared place (`places::offered`). Config: `config/lsp.rs`. Tests: `tests_lsp.rs` (the fake in
  process), and the daemon's `tests/lsp.rs` (the real fake binary, and a `kill -9`).
- **L1** (M4 17b): `sandbox.rs`: `[sandbox]`, a job's class (toward L1 alone: the default, `l1_argv`, the model's
  `sandbox: true`), L1's posture (notify), the view an L1 job gets, and health's block: the last L1 launch, read from
  its job's completion (`Sandbox::launched`), and why L1 refuses every job of a root daemon (theseus-pv6i).
  `theseusd check` runs the self-test on demand (`Sandbox::self_test`, over `toolrun::sandbox_for`'s view); nothing
  probes at the start, and an L1 job has no cgroup (theseus-gyin). Its facts are `fact/sandbox.rs`; its tests
  `tests_sandbox.rs`, and the daemon's `tests/sandbox.rs` with real L1 jobs. What a call's proposal binds about
  its job is `sandbox::Bound`: its class, and (18c) its egress list.
- **An L0 job's cgroup** (theseus-a5nv): `cgroup.rs` asks systemd once, 2 s after serving (`systemctl show -p
  Delegate`), whether the daemon's own cgroup is delegated, and readies it (`theseus_kernel::cgroup::ready`); health's
  `cgroup` startup phase holds the answer, and `run_job` hands it to each wrapper (`WrapperArgs.cgroup`), whose
  command is born in a cgroup of its own with `[tools] job_pids_max`. A result whose cap refused processes says so
  (`toolrun/job.rs`). The daemon's `tests/cgroup.rs` runs them in delegated scopes and units.
- **Grants in L1** (theseus-w5op; decided by the owner 2026-10-03, superseding theseus-7y9y): an L1 job takes its
  program's broker grants at its launch, exactly as an L0 job does. The gate's order (`toolrun/order.rs`) runs
  the L1 decision (`sandbox::unbrokered`) through `ToolRuntime::brokered` (decision 15: the stricter of the call's
  posture and the secret's, so any approval comes before the launch), and `sandbox::for_job` asks
  `Broker::for_job`. The value rides in the job's environment over
  the spec's pipe, the wrapper withholds it from the job's output (`redact`), and the result's head names what the
  job was given (`given GH_TOKEN`, names only, from the wrapper's `detail.sandbox.granted`). Tests:
  `tests_grants.rs`, and the daemon's `tests/sandbox.rs` with a real L1 job. 18d's run-time socket
  (`theseus-cred get`) is gone; a stored `cred.request` action and its ledger rows still read.
  An `aws` grant (18e) is the same: an L1 job gets its job session at launch, named by its correlation id,
  and it is the only AWS credential it holds (`~/.aws` is in `sandbox::CREDENTIALS`, hidden in every
  view whatever the approve list says; no daemon `AWS_*` variable passes `forbidden_env`; L1 has no route
  to the metadata service). It reaches AWS through its egress list, as any host: `[sandbox] egress`, or
  the hosts its call names. Tests: `aws/tests_l1.rs`, and the daemon's `tests/sandbox.rs`.
- **Egress** (M4 18c): `egress.rs`. `[sandbox] egress` and a call's `sandbox: { egress }`; the gate's step for
  hosts beyond the list (the call waits, and its approval reaches those hosts alone, since the list is in its
  proposal); and what a completion's `detail.egress` makes of a result: its `sandbox.egress` and
  `sandbox.egress_refused` rows (in the frame that writes the result), its lines, and DD5's `external` marker
  when the job reached a host beyond `[sandbox] egress`, so T1 holds the session (`via: egress`) and the node is
  untrusted, the owner's. A job that reached only listed hosts holds nothing (theseus-gyin). Its tests:
  `tests_egress.rs`, and the daemon's `tests/sandbox.rs` through a real proxy.
- **Cancellation** (M4 18a): `cancel.rs`, the one stop every path that stops running calls goes through
  (`ToolRuntime::terminate_all`: a cancel, a task's cancel, `/stop`, the disk's floor, a stop at a job's launch).
  A job's wrapper is asked to stop its tree; an async tool's task is aborted (`Stops::track`) and verified once its
  handle has finished; anything else is unsupported. Each verdict lands on its action, in health's `cancels` and
  the metric `theseus.cancel` (both from `Stops::count`, theseus-qdk5), and as a fact (`fact/cancel.rs`). An aborted call's result waits for the cancel's verdict (`after_abort`). Its test is
  `tests_cancel.rs`; the daemon's are `tests/job_wrapper.rs`, `tests/sandbox.rs` (L1), and `tests/tasks.rs`.
  A job's last steps wait for its stop's end (theseus-dwoj): `stop_backends` stops, then `Stopped::write_verdicts`
  writes each job's acknowledgement, verdict and fact's row in one frame, and a cancel's sweep of its unanswered calls
  joins that frame (`answer_after_cancel_with`), so a cancel of a running job is two frames, the cancel's and that one
  (theseusd's `tests/cancel_frames.rs`). A crash between them leaves the action `requested`, which every reader takes
  as `acknowledged`.
- **Stop all** (theseus-cdcwr; `rpc/stop_all.rs`): `execution.stop { tasks: true }` is `/stop` and then the cancel of every task
  under the conversation, children before the task they belong to (`Kernel::task_tree`, one task-index read per task, an
  ended task walked through), each with `task.cancel`'s effect in its own frame, then one `execution.stopped_all` row
  (`fact::driver::ExecutionStoppedAll`). The conversation's wakes stay; a sleeping deferred task is cancelled. It is an
  owner's act (`judge_act(Act::Session)`, so a shared place is refused and ledgered); the CLI refuses `stop --all` in a job
  (`refuse_stop_all_in_a_job`). A plain `/stop` is unchanged: its tasks run on. Depth is one today (`TaskDepth`), so the
  nested walk is held by the kernel's `a_task_tree_lists_each_task_after_its_own_tasks`. Readers: `theseus stop --all`, the
  TUI's `S`, Discord's `/stop all:true`. Tests: `rpc/tests_stop_all.rs`, theseusd's `tests/tasks.rs`.
- **Terminals** (theseus-n88g.4): `term/`. `term.open`, `term.send`, `term.read`, `term.close`: a program on a pty
  (libc's `posix_openpt`; its child through `children::spawn`, `setsid`, the pty its controlling terminal), read as
  a screen by `term/vt.rs`, a small VT model whose module doc says what it leaves out. Async tools whose run needs
  its session, so `toolrun`'s async path runs a `term.*` call through `Terms::run`. `term.open` plans its argv and
  `term.send` its terminal's, so the gate judges both as that program's run; `term.read` and `term.close` are reads.
  A listed external program's screen (or a terminal sent keys naming one: `Listed::in_text`) is outside text. At
  most 4 a session whose programs run: an ended one frees its slot, its last screen readable until `term.close` or
  until a full session's open reclaims the oldest (theseus-ggqf). Closed at its execution's end (`turn.rs`), a cancel
  and a `/stop` (`rpc/driver.rs`), and the daemon's stop (`finish_stop`). `term.close`, a cancel and a `/stop` hang
  up, wait the grace, and kill what lingers, a process that left the tree but holds the pty included. At the
  session's end and the daemon's stop, under `[tools.term] keep_background` (default on, `config/term.rs`), the
  close ends only the program and the pty's foreground group (by group signals, so a fork mid-close is not spared)
  and leaves the background running, as `proc.run` does (`term/left.rs`: booked, in health's `terminals_left`, a
  `term.left` row, ended by a later cancel or `/stop` of the session's execution or its parent's,
  `Core::end_terminals_left`). `term.read`'s `until_idle` waits for the program back in front (`tcgetpgrp`) and
  waiting for input (`/proc/<pid>/syscall`: a read of fd 0, or a select or poll), looked at every 40 ms and never
  within 100 ms of a send. In memory only: the rows `term.opened`, `term.closed` and
  `term.left` (`fact/term.rs`) are the record. No broker grant reaches a terminal (`brokered`). Tests: `term/tests.rs` (goldens
  and real `sh`, `python3`, `cat`; a `/proc` scan for a test's own processes looks for a marker holding the run's
  pid, so another tree's run beside it is never taken for its own, theseus-d006, theseus-fps6), `term/tests_keep.rs`,
  `tests_idle.rs`, `tests_slots.rs` (theseus-ggqf; `term/bench.rs` its ignored measures), `tests_term.rs` (the
  gate, the hold, each close, a task's job left and a `/stop`'s or a cancel's end of it, the config reaching the
  terminals), and theseusd's `tests/headless.rs` and
  `tests/terminals.rs`.
- **The protocol server**: `rpc/` (`server.rs` routes each method by name; `methods.rs`; `confirms.rs`), with
  `bus.rs` and `outbound.rs` (one ordered, capped queue per connection; a test may lower the cap a new connection
  takes, `Push::backlog_cap`, so the lag prove overflows it with a few hundred events, theseus-0u6g).
  `rpc/ordered.rs` (theseus-klo2): one lane per connection, keyed per session, for the methods that change a
  session (`ORDERED`): each waits for the earlier ones on its session to take effect (`applied()`, at the first
  frame; a stop at `reachable()`), and reads never wait. The wait, from the line's arrival, is the turn root's
  `lane_us` (`ordered::lane_us()`) and `theseus.rpc.lane.wait` by method. Tests: `rpc/tests_ordered.rs`,
  `telemetry/tests_lane.rs`.
- **Dismiss and retry** (theseus-7zgsm; `rpc/retry.rs`, `fact/retry.rs`; store format 34): a failed task stays flagged
  until the owner dismisses it, and the daemon records it so every surface agrees. `execution.dismiss` is
  `Kernel::dismiss_execution` (one frame: the execution's own record carries `dismissed`, so a restart and the push's
  seed keep it); the push reads it (`ExecutionView.dismissed`: attention is idle, `failed: <reason> · dismissed`; the
  work view keeps its `report`, `dismissed: true`, and leaves its parent's `failed` and `needs_you` counts, and may be
  pruned like any ended task). `task.retry` reads the failed task's brief and quoted pieces and opens a new task from
  them in `Kernel::open_retry`'s one frame (session, brief, arrangement and its edges, task record, `task.retried`, the
  carve no larger than the failed one's, the failed one dismissed); the brief keeps the first words and ends with a
  line saying it is a retry, why the last try ended and the owner's `note`; a retry of a retry names its predecessor
  (`retry_of`, the work view's too). Both are `judge_act(Act::Session)` (the owner, from a private place) and the CLI
  refuses them inside a job (`client::OPERATORS`). Core entry points `Core::dismiss_failure` and `Core::retry_failed`
  take an `Answerer`. Not done: the new brief's `derived_from` edge to the parent's reply. Tests: `tests_retry.rs`, the
  kernel's `tests_retry.rs`.
- **`work.answer`** (theseus-ds5os; `rpc/answer.rs`): one verb for every question. `question_id` is a whole id or a unique
  end of at least four characters among the open questions and pending wakes (`pick`; an end naming two is refused,
  naming both); `option` is one of `answer_options` for the question's kind (`kind_of_action`: budget, extension, a
  layer-1 change, else a call; a reminder's are `reminder_options`), and one the question lacks is refused with the valid
  ones. A call, a budget question, a change and an extension's ack go to `confirm_action_with`, so the place rule is
  `judge_act`'s exactly as `action.confirm`'s (a refusal is ledgered `approval.refused` and the question keeps waiting);
  a reminder's Done and snoozes go to the wake owner's verbs, refused only on a surface that is never an approval
  surface. `QuestionKind::Ask` is `owner_ask`'s (below): its option is a number from 1, `choice:<i>` or `other` (`answer_ask`). Every
  answer, by any verb, is one `confirm.resolved` carrying its `option` (`CallAnswered.option`, `BudgetAnswered.option`)
  and the push's change. A `ConfirmRequest` is built through `rpc::offer`, which words its `options`; `confirm.list`
  adds `may_answer` for the asking connection (`Core::may_answer`: the place rule asked without making an act; a push
  says nothing, since every connection hears it alike). Tests: `tests_m3/answer.rs`, and one each in
  `tests_task_graph.rs`, `extend/tests.rs` and `tests_reminders.rs`.
- **`owner_ask`** (theseus-j7cje; `ask.rs`, `needs_you.rs`, `rpc/answer.rs`'s `answer_ask`, `rpc/confirms.rs`; the pure parts are
  `theseus_protocol::work::ask`): the model asks the owner a question with 2 to 6 options (`question` up to 500
  characters, `label` up to 60, `default` an option's number from 1, `expires_in` minutes, 30 by default and 1,440 at
  most). The tool plans with `Plan::authority`, so the gate waits at every posture (`authority_for` leaves it alone); it
  is a harness tool in private places only (`places::public_tool` does not list it), so the execution parks as on an
  approval and the question is a `QuestionKind::Ask` with `choices`. An answer is `work.answer` with the option: it binds
  the confirm in one frame with the answer's row and a META record, `ask.answer.<correlation id>`
  (`AskAnswer`; no store format), and the continuation "runs" the call, which only reads that record back as its result
  (`The owner chose 2, "Ship it behind the flag"; note: …`; `other` takes a note; a plain `action.confirm` yes is refused,
  since it chooses nothing). Its life is `ask::expiry_of` (plan time plus its own life), read by the same driver tick that
  expires approvals (`expire_questions`, `question_due`); at expiry `expire_ask` writes the same record with `expired`, and
  the result says the default applied or that no one answered. The brief of a task says when to ask (`task::brief_text`).
  **The always-on channel** (`needs_you.rs`, `[notify]` in `config/notify.rs`): the driver's tick reads the open questions from the
  kernel (not the push, which is seeded by the first watcher), and one still open after `escalate_after_secs` (60) with no
  screen open (`Bus::interactive_watchers`), or `escalate_with_screen_secs` (300) with one, gets one `needs_you` outbox post to
  the owner's DM on that tick, whatever the pacer's window: the window paces the buzz, never the card (theseus-27p6w), so a post made while it is closed is `silent: true` and does not take it (the courier pings it otherwise). With `[notify] dm_cards` (on; off, the one-line posts of before byte for byte) each such question is a card of its own (theseus-suvpe): `needs_you/words.rs` words it in three lines from the request's structured fields (what, why, what yes does; the asker of a self task is "Theseus's self-repair: <backlog title>" from its task record), several due together are a header post (`header: true`, which pings) and a card post each (`silent: true`), and the Discord binding gives each its buttons, Details and a reply that answers it. The outbox indexes it as the question's card
  **Plain words and fail open** (theseus-cudar; the owner, 2026-10-19 20:08 and 2026-10-20 07:26): an approval's request carries `plain` (`ConfirmRequest::plain`, worded once by `needs_you/plain.rs`, from `Core::plain_of` in `rpc/confirms.rs` and at ask time in `toolrun.rs`): who asks (the asker's name, never a raw id but one short tag), what (a verb and a friendly object: `object_of` names an earlier run's report "its own report from the 05:12 run", the store "Theseus's own store (conversation content)", a path keeps its end), the model's one-line reason (the last line of the assistant message that made the call, `reason_line`), a risk badge (`risk_of`: read-only, writes, runs a program, network, spends, touches secrets) and what Yes and No do. The Discord card, its settle line (`card.line`, so an Expired card keeps the whole question), the Details press, `theseus answer` and both TUI panes read it; a request with none keeps the gate's reason. **Where the reason comes from** (theseus-zcn80): `needs_you::rule_clause` reads the clause of the reason's last `— approve (…)` (the owner's own posture has none), and `plain` words it once for every surface: `held` is the floor, outside text, the thread guard's doubt (`toolrun::misroute::is_question`, "it may belong in another thread"), the clause, else "`tool` is set to ask"; `why` is the guard's question (both threads and the move) or the model's line, else the clause. Discord's `why it asks` reads the same clause, and `words.rs` the same `held`. A thread's session is asked by its thread's title (`Core::asker_name`). **Self-improvement fails open** (`toolrun/order.rs`, `ToolPolicy::fails_open`, `own_tree::fail_open`): a self session's call that the policy layer holds for approval runs with a notice (`tool.notified`, "self-improvement fails open") unless it is the floor (store, spool, bindings, the 1Password token, sealed sets), a task's layer 1, an AWS call, a private address, an approve-listed program or argument, an approve-listed path other than the state dir's own, a write or a run in the state dir, or an L1 call; every later layer (web text, a place's floor, a language server's start) still holds it. `self.evidence` (`rsi/evidence.rs`, registered with `[self] mode = "act"`, self sessions only) reads ledger rows of a cause key in a time range, an item's earlier reports and the gate files, plain names inside `self/reports` and `self/gates`, so a self session never searches the state dir or its store; a started item's brief names its earlier reports (`evidence::briefing`).
  (`Outbox::pinged`), so the answer's `card_closed` writes the settle that edits it; a question that has a card in a Discord place
  (`Outbox::has_card`) gets no second post; with no DM bound nothing is written. Half-way to a question's expiry the work board
  reminds (the policy's rule 2): `WorkBoard::remind`, from `Push::remind_questions` on the same tick, reading nothing until
  `remind_at`. `task.create { notify_done }` is stored on `TaskOf` (store format 34) and flags the task's work view, so the
  policy's report row interrupts. Tests: `tests_m3/ask.rs`, `tests_ask.rs` (a task asks, the CLI answers, it resumes), the
  Discord crate's `tests_gateway/ask.rs`, `notices::tests` for the flag.
- **`task.list`'s filters** (theseus-ypnxt.1; `rpc/task_history.rs`): `ended`, `since_ms`, `query` (all words, any case, over
  the title, the objective or else the brief, and the report's first line), `id` (one task by its id or the end of it, with its
  children and whole report) and `limit` (the newest N) narrow the answer; with none set `task_list` answers exactly what it
  did. `TaskListResult.details` carries, by task, the report's first line (for `ended`, `query` and `id`) and, for `id`, the
  whole report and whether the task is held. The report is the node its record's `report:` evidence names (one point read),
  else why it ended; it is read only when a filter asks. Tests: `rpc/tests_task_history.rs`.
- **`work.digest`** (theseus-qkoi3; `rpc/digest.rs`, `push/work/digest.rs`): "since you last looked", in a few lines. Params
  `{since: position | since_ms, roots?: [session_id]}`, answer `{from, to, lines, from_ledger, now, questions}`; the
  protocol (`work_digest.rs`) words a `DigestLine` once (`digest_words`), folds bursts (three or more of one kind under
  one root within ten minutes of each other become one line, `fold_digest`) and words the summary line every surface prints
  (`digest_summary`). From the work board's notice ring when it reaches back to `since` (`notices_since` complete), plus
  the board's cancelled tasks (the policy gives a cancel no notice); else from the ledger by kind (`task.ended`,
  `action.confirm_answered`, `execution.failed`, `wake.fired`, `tool.late_result`) over the whole window, a page of each kind
  at a time from its start, to `HARD_ROWS` (20,000; past it the lines are whole up to the time the read reached and the result's
  `more` says so, with the command that lists the rest) on the blocking pool, and the answer says so. A task that reported is
  told by title with the first line of its report (`DigestLine.report`, `Core::report_text`, the one reader `task.list` uses
  too): up to five under one root, then `📋 and N more: A, B`, never `N tasks reported`. A time is turned into a position by the ledger index's first row at or after it. Both
  sources make the same `Raw` lines, so a window told from either has the same kind, work and count (the test
  `the_ring_and_the_ledger_agree_on_the_same_window`); a job that ended well is no news in either. Seen stays the
  client's: the daemon keeps no position per surface, and writes no frame. The place rule is `work.watch`'s: a
  connection that reads no private text names its roots and is told of a shared place's alone, scrubbed and unable to
  answer; the in-process Discord binding may also name a private place (its DM), and only what lies under the roots it
  names. `roots: [task]` is that task's own timeline. A question an agent answered says so (theseus-5rvss): `DigestLine.by`, from the ledger row's `by` (`agent:<name>`) or the ring's `Notice.by`, words it "✓ answered by an agent (claude): …" and counts it apart (`1 answered by you, 3 answered by an agent`), never folded with the owner's; a line with no `by` is the owner's. Tests: `tests_work_digest.rs` (a child of `tests_work.rs`: the
  scenario told from both sources, a window past the ring's start, a task's timeline, the place rule).
- **Session states** (theseus-emqx): live, quiet or retired, derived when read (`theseus_protocol::sessions::derive`,
  `SessionRecord::state_at`; `session_info` gives every list its state, busy reading live whatever its age), never
  stored as a state: the record keeps only `retired` (superseded or by hand; empty is derived past `[sessions]
  empty_grace_minutes`), the `superseded_by`/`supersedes` links, `reopened_ms` and `title_was` (store format 25).
  `succession.rs`: a place's move (`Core::bind_place_to`: the place's record, both links and `session.superseded` in
  one frame, the old session's lock then the new one's), and the owner's `session.retire`/`session.reopen`
  (`judge_act(Act::Session)`, `rpc/sessions.rs`). `execution.list` pages the same way (`rpc/exec_list.rs`: `{n, before}` newest-born first, `{ids}`; none is every one as before); `session.list` with no params is every session as before; `state`
  filters, and the answer names its windows. The re-title is `turn/title_step.rs` (in the turn's own session write,
  once, never a task's). Tests: `rpc/tests_session_states.rs`.
  `session.list {dir, with_text}` (theseus-6yka, `rpc/session_find.rs`): one directory's sessions, where a session's directory and the asked one have the same root (the nearest ancestor holding `.git`, else itself); the index's births are scanned a step at a time and the answer holds whole steps (it can hold more than `n`; its `older` goes back from the last), newest active first; `with_text` (alone, every directory's) adds `first_text`, `last_text` and `prompts`, read from the store's node pages, no model. `SessionInfo.started_on` is `discord` (the outbox has a target for it) or `cli` (a client gave it a directory). Tests: `rpc/tests_session_find.rs`.
- **Surfaces of the record**: `push.rs` (the board), `outbox.rs`, `narrative.rs`, `ledger.rs`, `trace.rs`, and
  `telemetry/` (OTLP, metrics, spans).
- **Facts**: `fact/` (theseus-j6qn, Review 2's C2). One type per thing that happened, which says in one place what
  each channel gets: its ledger row (`KIND`, `row`), its notification (`METHOD`, `event`), its sentences
  (`narrate`), and its span (`span`). A site builds the fact and records it once (`TurnCtx::record`, a turn's
  `record`, or `Core::rec`/`session_rec` for the core's own); `FACTS` lists every one. A row's kind is a
  `theseus_protocol::LedgerKind`; `LedgerRow::new` takes nothing else (tests write an old or unknown name with
  `LedgerRow::named`).
- **Start and stop**: `config.rs`, `config_copy.rs`, `config_gate.rs`, `secrets.rs`, `startup.rs`, `restore.rs`
  (`restore`, and `repair`: a corrupt frame taken whole from a copy, theseus-15g), `sweep.rs`, `disk.rs`,
  `binary.rs` (whether jobs can write the daemon's own binary, read when health asks), and `crash.rs` (the panic
  hook's crash file beside the store, which the next start takes and health reports). The config template is
  `config/theseus.example.toml`, public, so it never carries a deployment's own values: a deployment's note holds
  only what differs from the defaults, and `config/sparse.rs` cuts a whole note to that (`theseusd config
  --sparse`, theseus-vwar). `config/renamed.rs` is the one table of renamed keys, (old, new) paths, and `explain`, which turns the loader's unknown-field error for an old key into "`[model] max_loops_mode` was renamed to `[model] on_max_loops`" (naming the file); there is no alias, so a rename adds a row and a literal case in its tests (`CASES`, one per row), and the operator edits the file. `config/lookup.rs` finds the config when nothing names one (theseus-5aqz):
  `~/.theseus/theseus.toml` if it exists, else `/etc/theseus/theseus.toml`, which `scripts/setup.sh` writes.
- **MCP servers** (M7 36b): `mcp/`. `McpBoard` tends each `[mcp.servers]` server (`config/mcp.rs`) after serving,
  never before: a stdio server through `children::spawn(Kind::Owned)` in its own process group (stderr to
  `<state>/mcp/<name>.log`, capped), with the job's environment and its `env` secrets; an HTTP one over reqwest.
  States, the crash backoff (1 s, 5 s, 30 s; a third crash in 10 minutes is `failed` until `mcp.restart`), and a
  stop that sends SIGTERM and never waits. Each server's last list is a META record, `mcp.tools.<server>`, read at
  the start (one key each), so a start offers it at once and a call waits for its own server alone. `McpTool`
  (`mcp/tool.rs`): canonical `mcp:<server>/<tool>` for the gate, wire `mcp__<server>__<tool>`, `Run` and
  `NonRepeatable` unless the server's `read` lists it (a server's hints never loosen), results outside text
  unless `external = false` (an error result too), `mcp_unavailable` and `outcome_unknown` in the failure's meta,
  which `toolrun`'s async path reads. `ToolRuntime::mcp` is the catalog the board fills: offered after the
  built-ins, in private places only, and a turn's request spec fixes its tools, so a `list_changed` applies at the
  next turn (`mcp.tools_changed`, and an operator notice). Facts in `fact/mcp.rs`; tests in `mcp/tests.rs` (an
  in-process fake through a `Connect` stand-in, the backoff on the paused clock) and theseusd's `tests/mcp.rs`.
  **Prompts** (36c, `mcp/prompts.rs`): each server's `prompts/list` is kept with a digest per definition and stored
  as META `mcp.prompts.<server>` (no format bump: a new key), listed again on `prompts/list_changed`; `mcp.prompt.list`
  reads it, and `mcp.list` carries it. `turn.submit { prompt }` resolves the prompt first (`McpBoard::resolve_prompt`:
  arguments checked against the definition, that server waited for alone, `prompts/get`), so a refusal leaves no
  session or node; a shared place's session is refused (`REFUSED`). The messages are user-role nodes of origin `mcp`
  (store format 8), author `prompt:<server>/<name>`, written with T1's hold (`mcp.prompt`, from `external = true`) in
  one frame (`turn/prompt_input.rs`). A use of a prompt whose definition differs from its last use (`mcp.prompt_used.<server>`)
  is `mcp.prompt_changed` and an operator notice (`mcp_prompt_changed`), once; the use goes ahead.
  A server with `sandbox = "l1"` (43a, `mcp/l1.rs`) is the daemon's own binary in its `mcp-sandbox` role
  (`theseus_kernel::mcp_l1`), spawned as any stdio server is, holding the server's init: the view a job gets
  (`Sandbox::job_view`), its own `egress` list (none: no network), and its pipes the board's own. It never falls
  back to L0: a root daemon's start fails with L1's refusal. `l0` stays a configured server's default. Its
  test is theseusd's `tests/mcp_l1.rs`, which as root checks the refusal alone.
- **Proposed extensions** (M7 43a, theseus-ext.5): `extend/`. `extend.propose`, a harness tool the turn awaits
  (`ToolRuntime::run_extend`, never on a core): `freeze.rs` copies the directory into
  `<state>/extensions/<name>/<digest>/`, read-only, the digest a SHA-256 over the sorted tree (paths, 755/644
  modes, bytes; a link is refused); the board starts the frozen copy as `ext-<name>` in L1 on trial
  (`mcp/trial.rs`, state `proposed`, never in `servers`, so `rebuild` never offers its tools), lists, runs each
  test, and stops it; the manifest is a META record, `extend.manifest.<name>.<digest>`; and the ack is a planned
  `extend.ack` action with its card, answered by `action.confirm` (`judge_act`'s place rule, then
  `extend/answer.rs`: an ack binds its confirm, a decline or an expiry declines it, neither wakes anything). Facts in
  `fact/extend.rs`; `extend.list` and health's `extensions` in `extend/list.rs`; tests in `extend/tests.rs` and
  theseusd's `tests/extend.rs`.
  **Loaded** (43b, theseus-ext.8; `extend/load.rs`, `mcp/ext.rs`): the ack's frame also writes the META record
  `extensions` (one key: name to digest, command, tools, capabilities, who acked and when, the proposing session, and
  its place and ceiling at the ack), `extend.loaded`, the server's stored list `mcp.tools.ext-<name>` (the trial's
  tools, so a new version never offers an old one's), and a replaced version's manifest (`replaced`). Then the board
  loads `ext-<name>` beside the configured servers (`McpBoard::load_extension`): the frozen copy in L1, the acked
  network only, no secret, results outside text only with a network (Q22); its tools are in the catalog at once and
  offered from the next turn's start (the turn's spec is fixed). Posture `notify` unless `[policy.mcp]
  "ext-<name>"` says otherwise (never looser than the enforcement), class `Run`; the load's ceiling's floor holds
  every call (`Floors`, in `toolrun`'s gate after the place's own). A start reads the record once
  (`Core::seed_extensions`, in `build`) and offers the stored lists; each starts after serving with the rest. The
  `ext-` prefix is refused for a configured server. `extension.revoke` (`extend/revoke.rs`, `judge_act(Act::Revoke)`,
  in the CLI's `OPERATORS`): one frame (the record without it, its manifest `revoked`, `extend.revoked`), then the
  board drops it and SIGTERMs its group; the frozen copy stays. Tests: `extend/tests_load.rs`, theseusd's
  `tests/extend.rs`.
  **Hands** (theseus-pw1q.14; store format 34; `extend/hands.rs`, `extend/broker.rs`, `aws/hands/start.rs`,
  `fact/extend_hands.rs`): `extend.propose { hands: { usd_cap, backends, max_group } }` (all required; `HandsCap`)
  is frozen into the digest (`freeze::with_caps`: the tree's digest, then the ask's canonical JSON; no ask, the tree's
  own) and said on the Load card ("This extension may run up to 20 hands at a time on Lambda, at most $2.00 a day.",
  buttons Load and Don't load). The ack's load gives that server's connection, and no other, a channel
  (`Extensions::channel`, `theseus_mcp::client::Serve`): its own stdio pipe answers `hands.run`, `hands.status` and
  `hands.cancel` (`broker.rs`'s module doc is the threat model). `hands.run` is checked against the loaded record
  (backend, `max_group` over its open groups' hands; `image`, `profile`, `account`, `region` and a relayed `hand`
  refused), then runs on the extension's day session (META `extend.hands.<name>`, principal `ext:<name>`, an
  execution pinned at `usd_cap`: the kernel refuses the reservation past it in the group's frame) through
  `aws.hands.run`'s own start (`start::plan`, which the turn's `run_hands` shares, then `launch_first`), runaway
  mode's lines included; one launch at a time (`Extensions::launches`). The day session never wakes for a turn: its
  results queue on an execution waiting on input. A group's record names its `extension`, and its settle (or
  cancel) writes `extend.hands.settled` beside `aws.hands.settled`; `extend.hands.launched` and `.refused` (with its
  reason; at most `REFUSALS_A_MINUTE` rows an extension a minute) are the broker's. A revoke spawns
  `hands_revoked`, which stops every open group by its backend's means. `extend.list`'s loaded entry carries
  `hands` (`ExtendHandsInfo::line`, the CLI's and the cockpit's words). Tests: `aws/hands/tests_broker.rs` (the hands
  rig's fake AWS), `extend/hands.rs`'s, `extend/tests.rs`' card test, `freeze.rs`'s digest test, the layouts'
  format-33 samples.
- **Flows** (theseus-pw1q.21.4; RSI loop 1's cheapest minted tool; off until one is loaded; no store format change: the META key
  `flows` and three ledger kinds, `flow.loaded`, `flow.retired`, `flow.ran`): `flow/`. A flow is a frozen JSON spec
  (`flow::spec`: v1, validated, SHA-256 over a canonical form) of steps over `proc.run`, file read, list and search, and the
  git and text reading tools (`ALLOWED_TOOLS`; no `sandbox`, `env`, `background`, `command` or `steps` argument), at most 24
  steps and 10 minutes, a cycle only with `max` on every step in it. It is frozen read-only at
  `<state>/self/mint/<name>/<digest>/spec.json` and read back, digest checked, on each start (`flow::load`). Each loaded flow is one
  tool, `flow:<name>` (wire `flow__<name>`), a deferred family of its own (`family::of_wire`), added to the catalog by
  `definitions_for` and kept in a request only where its scope holds (`TurnRunner::flows_in_scope`: a repository id `r:<hex>`
  that holds the session's directory, and a kind of place); with none loaded `ToolRuntime::flows.any()` is one read lock and
  nothing changes. **`flow_step`** (`toolrun/flow_step.rs`) is `admit` and `start` for one step, the path of any call of a
  response: plan, gate at the model's posture, place rule, L0 or L1, its own `ToolCall`, result and rows. A step that would wait
  is not asked: the flow stops and hands back the exact call (`needs-approval`), which the model makes and so waits as any call
  does (the flow holds no state across a turn's park); a step policy refuses stops the flow with the refusal as its result. The
  flow's own call is open at the gate (`flow::quiet`, in `modes::gate`) and class `Run`, so the hold still reaches it.
  `flow::class` is the one reading of a result's class (`ok`, `err`, `conflict`, `test-fail`, `rerere-left`, `rerere-resolved`).
  `Core::flow_load` (a test, the owner's CLI when it comes, R8's self-load) scrubs the spec and refuses one the scrubber would
  change; a load or retire by the `"self"` principal asks `rsi::gate` first. `theseus self revert flow:<name>` is the owner's undo
  (`flow_retire`: out of the record, `flow.retired`, the frozen copy kept). Not in the keep-warm, census or `context.explain`
  specs, which are built without a session's directory. Tests: `flow/tests*.rs` (a real temp git repository with an upstream).
- **Self-improvement's spine** (theseus-pw1q.2, theseus-pw1q.4; off by default): `rsi/`. `rsi::gate(&ctx)` is the
  one question every self step asks before it starts and between its phases: `Off` while `[self] mode = "off"`
  (`config/self_improve.rs`, read at the start), `Halted` while the kill switch is on (whatever the mode; a store never
  resumed is halted, so `"act"` alone starts nothing), else `Allowed`. One read of a cached state: the META record
  `self.switch`, read at the first ask and kept by the moves (`rsi::Switch`, `Core.rsi`); unreadable is halted.
  `self.halt` is anyone's and idempotent; `self.resume` is `judge_act(Act::SelfResume)` (the owner from any place:
  the CLI, the web UI, or an author holding an owner handle, `places::owner_anywhere`; a `from_job`, the `THESEUS_SESSION` the CLI sends, never counts), a refusal an `approval.refused` row. Each
  move is one frame: the record and `self.halted`/`self.resumed` (`fact/rsi.rs`), then told once as `self.changed` to the connections that watch every session (`Core::self_changed`, `Bus::publish_watchers`; no frame, no row: the TUI's self view reads the log again, theseus-pw1q.19). `self.log` (`rsi/log.rs`) reads
  every `theseus_protocol::rsi::SELF_ROWS` name and today's self-change kinds (`rsi::TODAY`) through the ledger's
  pages by kind, newest first, each with what, why, numbers and undo; a later self step's row carries those four in
  its data and shows with no change here, and its kind becomes a `LedgerKind` on the commit that writes it.
  `rsi/digest.rs`: the week's text (`self.digest`), and the driver's tick posts it to the owner's DM once a week
  (`post_self_digest_if_due`, kind `self_digest`, META `self.digest.posted`) only while `posts_digest()` (mode act,
  digest weekly): with the mode off it reads and writes nothing. Methods in `rpc/rsi.rs`, routed through
  `rpc_prefixed`. Tests: `rpc/tests_self.rs`.
  **The self budget** (theseus-pw1q.3; `rsi/budget.rs`): one account for all self-improvement, `[self.budget] branch_usd`
  ($10) and `day_usd` ($30, the local day), the owner's hard keel. `Budget::reserve(kind, branch, worst_usd)` before
  spending, `settle(reservation, actual)` after; a refusal (`Stopped`: line, spent, left) writes `self.budget.stopped`
  once per branch and line. The account is one META record (`self.budget`) written with each row's frame; the first use
  after a start settles a hold the old process left open at its worst case. A task session whose record has
  `origin.principal == rsi::SELF_PRINCIPAL` reserves each model call in `turn/self_budget_step.rs` (its branch is the
  root task's id; a failure of class `self_budget` ends the turn with the budget's words); other sessions pay one
  `Option` test. Mode off: every call refuses, reads and writes nothing. Tests: `rsi/tests_budget.rs`, `tests_self_budget.rs`.
  **Sealed holdout sets** (theseus-pw1q.5; the hard keel: Theseus gets a score, never an item): `[self.sealed] dir`
  (unset: none; checked at load to sit outside every root, `cwd`, the state dir, the repository and `[sandbox]
  ro_paths`, neither inside nor holding one: `config/self_improve.rs`). `rsi/sealed.rs`: `Sealed::put(set, family, bytes) ->
  ItemId` (a keyed hash; write-only, no call returns an item) and `score(set, &dyn Runner) -> Scores` (per-family
  counts and rates, cost, an opaque keyed id per failure), reached by `Core::sealed_put` (which writes the
  `self.exam.counted` row, `sealed: true`, with the id alone). The item is `{"v":1,"task","check",
  "canary"?}` with the exam's check language, the part that reads a reply (`sealed/check.rs`); every output passes
  `sealed/filter.rs` and an error is a fixed sentence. Out of reach: `ToolRuntime::refusal` asks `sealed::Guard`
  first, for every tool in every place (a path the plan resolves into it, an argument that reaches it from the
  call's directory, or its path written anywhere in the call's input: `judge`), and the path is a floor path, so
  no L1 view (a job's, an MCP server's, a proposed extension's) shows it. An L0 job can still build the path from
  pieces: L1 is the boundary, as for the socket. Health's `self` line gives counts per set (`SelfState.sealed`).
  The paths the keel guard should hold are `sealed::KEEL_PATHS`. Tests: `rsi/sealed/tests.rs`, `tests_reach.rs`.
  **The scorer's Runner and the model relay** (theseus-pw1q.21.5; no store format change: one ledger kind; `rsi/sealed/runner.rs`,
  `relay.rs`, `config/self_sealed.rs`): `[self.sealed]` holds `dir` (it was `[self] sealed_dir`, now refused with the rename
  message), `every_branch`, `item_usd` (0.25) and `drift_every_days` (7). `Core::sealed_score_branch(set, Candidate { branch,
  bins, profile }, head)` asks `rsi::gate` first (off or halted: nothing is read, run or written), then runs `Sealed::score`
  over a `ScratchRunner` on the blocking pool and writes one `self.sealed.scored` row (counts, rates per family, the relay's
  metered cost, opaque failure ids). `ScratchRunner::new` copies the candidate's `theseusd` and `theseus` read-only into
  `<state>/self/runner/<branch>/bin/` with their SHA-256 digests, writes a generated config beside them (memory, judge,
  channels, web, index and MCP server off; no AWS, no hands, no extension; `model.api_base` the relay; a placeholder key), and
  runs each item as one L1 job, `theseus --spawn theseusd ask --json`, over `runner_view`: the system and that directory
  read-only, no workspace, no production state or sealed dir, and the relay's name as the whole egress list. The daemon's
  state lives in the job's tmpfs and goes with its pid namespace; the directory is removed when the runner drops. The job's
  environment is bare (`DAEMON_ENV` only; `SCRATCH_UNSET` is the list theseus-exam's `start` strips too). `Relay` is served in
  process on the job's egress listener (`theseus_kernel::job_egress::register`), speaks `/v1/messages`, injects the
  production key, serves only the profile's model, is refused while `rsi::gate` says off or halted, reserves each call
  (worst case from `max_tokens` and an input estimate) on the item's line and on `[self.budget]` before forwarding, settles to
  the provider's usage (a stream that never ended at its worst case), and logs usage lines only (`UsageLine`; never a body).
  An item's cost is the relay's meter, a call refused at a line makes the item `RunFailed`. The live run in L1 needs a
  non-root daemon with a job cgroup; the tests run the item through a stand-in `Launch`. Tests: `tests_relay.rs`,
  `tests_runner.rs`, the config's `the_sealed_table_has_its_defaults_and_the_old_key_is_renamed`.
  **The sealed hold** (theseus-pw1q.21.7; no store format change: two ledger kinds and META keys; `rsi/sealed/hold.rs`,
  `rsi/join/sealing.rs`, `fact/rsi_sealed.rs`): the self-join's `sealed` phase. On each try's merge, after main's gate has
  built it in the merge's tree and before the signature, a branch whose diff touches a behaviour path
  (`hold::BEHAVIOUR`, patterns with `*`: prompt and system-text assembly, tool descriptions and schemas, routing and the
  profiles' defaults, Jev's pack code, recall/rerank/memory ranking, context assembly and the compiler, the turn loop's
  stop rules; a test file is none; a test holds that each pattern matches a file) or any branch with `[self.sealed]
  every_branch` is scored on every sealed set (`Seams.sealed`, a `Scorer`: `Scratch` runs `Core::sealed_score`, the scratch
  runner over each set, on the merge's `target/debug`) beside main's score (`Core::sealed_main`: the installed binaries,
  one META record for each (main commit, `Sealed::digest`), the first ever kept also as the drift baseline). `hold::judge`
  holds when a family of 5 or more items loses more than 1 item or a set's total falls at all; smaller families and
  one-item losses are reported. A hold is `self.join.stopped {phase: "sealed"}` (`numbers.held`: that head is not tried
  again) with the numbers and the opaque ids of the items that newly fail, plus the item back to `proposed` with the same
  evidence (`Run::reopened`, shared with a review's reject); a check that could not run is a stop tried again. Every pass
  writes one `self.sealed.checked` row (`passed`, `held`, or `skipped` with its reason when there is no `dir` or no item),
  its text checked by `Sealed::allows` first, and its `what` is `rsi::log::sealed_line`'s `sealed 38/40 → 39/40`, which
  `theseus self log` and the cockpit's self rows show as written. **The weekly drift check** (`Core::sealed_drift`, from the
  join tender every `[self.sealed] drift_every_days`): the installed main against the baseline; a family (or a set's
  total) that fell is one `self.sealed.drift` row, which the backlog night files as one `sealed.drift:<family>` cause
  (`backlog/read.rs`); nothing is reverted, and a changed set is a new baseline. Tests: `rsi/sealed/tests_hold.rs`,
  `rsi/join/tests_sealed.rs`.
- **The self backlog** (theseus-pw1q.7; `rsi/backlog.rs` and `rsi/backlog/`): the nightly read (`backlog_night`; its
  tender, `backlog_after_serving`, runs an hour after consolidation, only with `[self] mode = "act"`) asks `rsi::gate`
  first and between its phases, reads the last day's ledger by kind (failed calls and turns, provider errors, invalid
  tool input, routing corrections, wrong labels, turns past their budgets, refused acts, spend lines), health's
  warnings and the learning report (`read.rs`), and files each cause (a stable key) as a task under one parent,
  "Self-improvement backlog", `origin.principal = "self"`, `proposed` with no session (or `waiting_human` for a keel
  class, `keel.rs`, which never starts). A cause open already gets the night's evidence; a dropped cause is never
  filed again (`self.backlog` META, `state.rs`, no format change). `rank.rs` documents the score; `[self]
  backlog_max_open` trims the lowest; `backlog_starts` (unset: `[self] pipeline_builds` less the builds in flight, `SelfImproveConfig::night_starts`) top unmarked items start in a session under the reserved
  `"self"` principal (`start.rs`: `open_session_as`), and the first turn runs from the tender. A fix item's tree is
  opened first (`self_tree_open`: the one open already, else `tree::open`, off the runtime's worker), the session's `dir`
  is that tree, and its first turn's input ends "Your tree is <path> on branch <branch>; read and edit there, by
  absolute path." (theseus-qh2tt); a tree that cannot be opened leaves the item proposed (`NoTree`, `Night.held`) and the
  pipeline's next start held as `tree`, with the reason. A mint's start is its own and opens no tree. `word.rs`:
  `self.backlog`, `self.drop`, `self.move` and `self.retry` (a failed item is proposed again, its session cleared, an
  evidence row "retried by the owner"; any other state is refused with words), the owner's by `Act::SelfBacklog`
  (`owner_anywhere`). A cause whose newest closed item's build never ran (`infra::unjudged_items`: the gate never started)
  is not filed again either: it is kept out like a dropped one until the owner's `self retry`, and a night that only
  kept candidates out still tells the owner, with `theseus self backlog --all` and `theseus self retry <item>`. The index dates a
  ledger row by its frame's write time: a test seeds rows, then runs the night at a clock read after the seeding.
  Tests: `rpc/tests_self_backlog.rs`.
  **The branch builder** (theseus-pw1q.8, seam S6; `rsi/tree.rs`, `rsi/pr.rs`, `toolrun/self_pr.rs`, `fact/rsi_pr.rs`):
  `rsi::tree` is the API self-join and auto-revert reuse: `open` (a worktree off `[self] repo`'s `origin/main` at
  `<state>/self/trees/<slug>`, branch `self/<date>-<slug>`, build tree `<state>/self/target/<slug>`), `gate` (main's
  `scripts/gate.sh` blob copied to `<state>/self/gates/` and run with `THESEUS_GATE_ROOT`, so the tree's own copy judges
  nothing; nice 19 and `ionice -c 3`, `THESEUS_GATE_NO_BENCH=1`, no compiler wrapper, no `THESEUS_*`; main's
  `scripts/may-build.sh` asked first when main has one, and waited on for as long as it says no, one quiet FYI notice
  to the owner every 30 minutes carrying the rule's last line — `MAY_BUILD_WAIT` is that notice's interval, not a
  deadline, and neither a stop nor a halt lands until the wait ends, `theseus-rexxp`; a gate stopped at `GATE_SILENCE`
  names the phase it was in, which at L0 is its log's last `gate: ` line), `push` (`refs/heads/self/*` only, never forced, after the
  daemon's scrubber over every added line and message, and main's `scripts/scrub.sh` when there is one), `close`.
  `self.propose_change { item, issue, why }` is registered only under `[self] mode = "act"`, runs only for an
  execution whose principal is `"self"`, and asks `rsi::gate` between phases: the first call opens the tree (which
  joins the workspace roots while `[self]` builds: `toolrun/guarded.rs`; the state dir's approve-list hold does not reach it for
  the self session that owns it, theseus-pw1q.21.9: `rsi/own_tree.rs` names the one tree of the session's task, `toolrun/order.rs` passes it to
  `ToolPolicy::decide_in` for the `fs.*` tools that write, and for the read-only `fs.*` and `git.*` tools of any self session
  it passes the trees root instead (`own_tree::open_to`, theseus-qh2tt: reading every tree, writing only its own), and every other path, the floor and the sealed sets ask as before; the edit hook's server start is the same case, theseus-kq4z4: a self session's `fs.write|edit|patch` in its own tree starts no language server, so `lsp::edits::gate` judges no start as `proc.run` and the result's `meta.lsp.not_started` says `self tree`, since rust-analyzer runs a tree's build scripts and proc macros outside L1; a server already up still answers, and an `lsp.*` call's own start is judged as before), the next commits, gates, writes the frozen
  report (`<state>/self/reports/<branch, / as ->.md`, 0444, ending `<!-- REPORT COMPLETE -->`), pushes a green branch
  when `[self] remote` is set and closes the tree. Each attempt is one `self.branch.built` row; a keel finding posts
  a `self_keel` card to the owner's DM and marks the item (`<trees>/<slug>.keel`) so it is never retried around.
  **A gate that never started halts the builds** (`rsi/infra.rs`, theseus-otkwu): the second infrastructure failure in a
  row (`gate: failed`, a `red` at exit 127 or at 0 seconds) halts self builds and tells the owner in the DM, who releases it
  with `theseus self resume` or "resume self"; the code of such an attempt was never tested, so it is marked neither red
  nor filed as a cause. A gate cut off by a signal or the OOM killer after it ran for a while still reads as **red**, not
  infrastructure: it errs on the safe side.
  `self.branches` lists them (`theseus self branches`). Tests: `rsi/tests_tree.rs` (a throwaway repo and a stand-in
  gate), `rsi/tests_pr.rs` (through the core). **Every self-code run is in L1** (theseus-pw1q.21.1; `rsi/jail.rs`,
  `sandbox/self_build.rs`): the branch's gate, the planted revert's test runs, the merge gate with its benches,
  auto-revert's gate and the install's build go through `Jail`, over `Sandbox::self_build_view`: the tree and its build
  tree the only read-write binds (`L1.rw`, set nowhere else, so no model input reaches it), the repo, `~/.cargo`,
  `~/.npm` and `~/.cache/theseus` overlays whose writes vanish, `~/.rustup` and main's gate copy read-only, no network
  (cargo runs `--offline`, so a branch that adds a crate the cache lacks fails its gate with the supply-chain reason,
  `sandbox::supply_chain`), 8,192 processes (`BUILD_PIDS`; the job default of 512 stays), and the floor and the sealed
  sets hidden as in every view. A `Place` carries its `Jail`: a daemon's always (`Board::place`), a unit test's none
  (its stand-in gate runs at L0 in this crate's tests only; `Place::jail` is an error anywhere else). Main's own scripts
  (`may-build.sh`, the keel guard, `scrub.sh`) still run at L0: they are main's, not the branch's. The install builds in
  L1, then its L0 half (`install::script`) only copies the binaries and restarts the daemon. `self.propose_change`
  refuses only when L1 cannot run on this host (`Board::l1_unavailable`: health's refusal, else `Sandbox::self_test`),
  sealed set or not. Tests: `rsi/tests_jail.rs`, `tests_pr.rs`, `tests_sandbox.rs`'s `no_model_input_reaches_rw`, and
  theseusd's `tests/self_build.rs` (real L1; an ordinary user runs them).
  **Graduation** (theseus-pw1q.13; `extend/usage.rs`, `rsi/backlog/graduate.rs`, `fact/rsi_graduate.rs`): `Core::extension_usage`
  counts an extension's calls, failures (an error result, a timeout, a crash, an unknown outcome) and p50/p95 from the
  ledger's `action.*` rows by kind (producer `async:mcp:ext-<name>/<tool>`); no counter on the call path. `extend.list
  {usage: true}` carries it (cached 60 s; `theseus extend list`, the cockpit card). The night's backlog reads it too: a
  loaded extension with `[self] graduate_min_calls` (20) calls in `graduate_window_days` (14) since it was loaded, no
  failure and a whole read, once per digest, is the item `graduate ext-<name>` with a brief (source path, schemas,
  declared tests, what done is) and a `self.graduate.proposed` row; a network extension's key (`graduate.ext.net:`) is
  keel-marked. `extension_revoke_graduated` is the join's revoke (`extend.revoked` with reason `graduated`), for self-join.
  **Self-exams** (theseus-pw1q.11; `rsi/exams.rs` and `rsi/exams/`): the nightly pass, after the learning report and
  `rsi::gate` (`Core::self_exams_nightly`; nothing with the mode off), turns the owner's corrections
  (`route.corrected`, `fact.corrected`), the judge's wrong labels on the train split (a judgment older than the
  holdout window), failed tasks (`task.ended`) and over-budget turns (`budget.reached`) into items of its own
  versioned format (`self-exam-v1`; theseus-exam's check language), scrubbed (the daemon's scrubber, handle shapes,
  held people's names). A keyed hash (META `self.exams.key`) splits each family into an open half (written to
  `<state>/self/exams/open/<family>/<id>.json`, `Shelf`) and a sealed half (a `SealedSink`: the sealed store when
  `[self.sealed] dir` is set, which refuses a regex check; with none, sealed items are dropped with a count,
  unreviewed and unwritten). An item counts only after a reviewer on another model (`CoreReviewer`, profile `fable`,
  held on the self budget's day line) calls it fair; then `self.exam.counted`, else
  `self.exam.rejected` with the reason (a sealed item's reason names the failed questions only). `[self]
  exams_day_max` (10) caps reviews a local day. The proposer rule: `Writer::SelfBranch` is refused by every
  `Shelf` write, and `exams::touches_exams` names the paths a self branch's push refuses (`rsi::tree::push`: `self/exams/`,
  `crates/theseus-exam/exam/`). `self.exams` / `theseus self exams` show counts by family and half and today's
  reviews, never a sealed item's text. Tests: `rsi/exams/tests.rs`, `rpc/tests_self_exams.rs`.
  **The self-join** (theseus-pw1q.9; `rsi/join.rs` and `rsi/join/`, `fact/rsi_join.rs`): `Core::self_join_pass(&Seams)`,
  from its tender (`join/tender.rs`, only with `[self] mode = "act"`: a pass 10 minutes after serving, then every 15),
  asks `rsi::gate` before every phase and writes `self.join.stopped { phase }` on a refusal. Each green
  `self.branch.built` row is reviewed once per head: the reviewer's model (`[self] reviewer_profile`, `"fable"`) must
  differ from the builder's session's last target (else `refused`); the planted revert (`join/planted.rs`, code, not
  the reviewer: the branch's new and changed tests pass at its head, then fail with every non-test path put back as
  the merge-base has it) runs before any reviewer is paid; the reviewer is a check turn in a new `"self"` session
  (`join/review.rs`, the claim through `check::render`: the diff, the frozen report, the item's evidence, never the
  builder's transcript), its verdict the reply's last `VERDICT:` line. One `self.review.verdict` row; a reject puts
  the backlog item back to `proposed` with the reasons in its evidence, in the row's frame. An accepted head is
  classified (`join/keel.rs`: `CLASSES`, one test a class, plus main's keel guard, a `MANIFEST_FORMAT` line and a
  spend key) and a hit posts a `self_join` card to the owner's DM and holds the head (`numbers.held`). Then, one a
  pass, under the mkdir lock (`join/lock.rs`, `[self] join_lock`, its `owner` file ending `done:`): fetch, the dry
  run (`merge-tree`), a `--no-ff` merge in a detached tree, main's gate through the door with the benches
  (`tree::gate_with_benches`), the signature with `[self] repo`'s `user.signingkey` (none: a card, nothing pushed),
  the push of main never forced, main moved redoing the three (`TRIES`), `self.joined`. `[self] install` (false)
  hands the build and the restart to a `systemd-run --user` unit (`join/install.rs`); the new daemon's
  `self_installed_after_serving` writes `self.installed` and the notice once. Tests: `rsi/join/tests.rs` (a
  throwaway origin, a stand-in gpg, stand-in tests and reviewer), and each module's own.
  **Self-narrow** (theseus-pw1q.15; `aws/narrow/`, `rsi/narrow.rs`, `fact/rsi_narrow.rs`): Theseus proposes a narrower
  policy for a hand role (`theseus-hand-owner`, `-read`) from what CloudTrail showed it calling, checks it, and sends it
  to the owner's card; an IAM change is the hard keel, so nothing applies without the owner's yes. The weekly pass
  (`Core::self_narrow_nightly`, after the exams, nothing with the mode off or the switch thrown; `[self]
  narrow_every_days` 7, `narrow_days` 14, `narrow_read_pages` 400) reads the role's events from CloudTrail's history (a
  page of 50 at its 2-a-second rate; `usage.rs`: distinct `service:Action` pairs and ARNs for the role, an access error
  is not a use, the data events the history cannot show, `DATA_EVENTS`, are kept as the policy has them and named "not
  visible"; a window cut by the page cap proposes nothing), reads the role's policies (`live.rs`: everything attached but
  `theseus-hand-basics` is wide and replaced), and builds the candidate (`build.rs`: observed actions on the ARNs they
  named when those sit inside the statement that allowed them, else that statement's resources; the `KEEP` list from the
  hand's code; the denies copied). `policy::wider` proves it no wider than today on every statement (also a property
  test over random policies). `verify.rs`: Access Analyzer's `ValidatePolicy` (an ERROR or SECURITY_WARNING blocks;
  suggestions are listed) and `SimulateCustomPolicy` of every observed call against the candidate and what stays
  (anything but `allowed` blocks). `apply.rs` patches the deployed hands template as text (anchors checked, read back),
  and the guard crate's scan of it must find nothing new. The card (`self_narrow`, the owner's DM) carries the diff in
  words (`proposal.rs`), `theseus self narrow show ID` the JSON. `theseus self narrow apply|decline|revert ID`
  (`judge_act(Act::SelfNarrow)`: the owner, a private place, never a job's shell): apply makes a change set of the hands
  stack from its deployed template, drops it unrun unless it touches only the role and its new policy and the guards'
  floor is empty, runs it, and keeps the template from before in `<state>/self/narrow/<id>.before.yaml` for the revert.
  For a week after an apply the nightly watch reads the role's AccessDenied events since it; each new denied action is
  a `self.narrow.red` row and a `self_narrow_red` card with the revert. State is the META record `self.narrow`. The
  pass makes AWS reads and no model call. Tests: `aws/narrow/*` (pure, with a fake account) and `rsi/tests_narrow.rs`.
  **Auto-revert and the veto** (theseus-pw1q.10; `rsi/revert.rs` and `rsi/revert/`, `fact/rsi_revert.rs`):
  `Core::self_revert_pass(&Seams, &mut known)`, from its tender (`revert/tender.rs`, only `[self] mode = "act"`: a pass 20
  minutes after serving, then every 10), asks `rsi::gate` first and reads nothing with the mode off, the switch halted or
  `[self] revert_window_hours` (24; 0 watches nothing) at zero. For each `self.joined` row inside the window and not
  reverted, vetoed or held, `revert/signals.rs` (`Signals`; `Files` reads the gate's `gate-times.csv`, the bench
  history and the ledger's error rows against the hour before the first join) reports a `Red`; `revert/attribute.rs`
  says whose it is by bisecting main's first-parent commits with main's gate on a tree at each (`Prober`, `MainsGate`):
  red at the parent of the earliest join is `Before`, the first red commit being no self-join's is `Outer`, else the
  join; a health red is the newest join before it unless an outer commit came between. A red judged is remembered (a
  `known` set) and not tested again. `revert/doing.rs` reverts under the self-join's lock (`git revert -m 1` in a
  detached tree off main, `tree::gate_with_benches` through the door, `git::sign`, `git::push_main`, never forced, the
  install by `join::install`), writes `self.reverted` or `self.vetoed` and the owner's `self_join` notice in one frame;
  a conflict, a red or keel gate, no key or a lost race posts the owner's card and writes `self.join.stopped` (branch
  `revert <merge>`, `numbers.held`), and the watch leaves a held merge alone. `Core::self_revert` is the owner's veto
  (`self.revert`, `theseus self revert`, the cockpit's Revert button): `judge_act(Act::SelfRevert)` is the resume's
  rule (`places::owner_anywhere`; a job's `THESEUS_SESSION` is refused with an `approval.refused` row), it works with
  the mode off, finds the merge among the `self.joined` rows only (an outer-loop join is never reverted; a name of 7
  characters or more), drops the join's backlog item for good (`Core::self_drop_vetoed`, before the revert, so a held
  revert still keeps the item dropped), and an already-reverted merge answers `changed: false`. Tests:
  `rsi/revert/tests.rs` (a throwaway origin, a stand-in gate and gpg, and a "world" clone that lands merges),
  `signals.rs`'s own.
  **The pattern miner** (theseus-pw1q.21.3; `rsi/mine/`, `config/self_mint.rs`, `fact/rsi_mine.rs`, wire `theseus_protocol::patterns`;
  no model call, no store format change: META keys and one ledger kind): the nightly read of recurring tool-call work, off by
  default (`[self.mint] mine = false`; the night also asks `rsi::gate`). `read.rs` folds the NODE tool calls and results
  after a cursor (META `self.mine.cursor`, up to ten minutes ago, at most `MAX_CALLS`) into runs (a turn's calls) of
  `shape.rs`'s tokens: tool, program, subcommand words, flags as a sorted set, typed slots (`$PATH:crates/**.rs`, `$REF`,
  `$NUM`, `$URLHOST`, `$CRATE`, `$STR`), the result's class and features; raw values live in locals for one fold and leave as
  a keyed digest per slot, with a constant's text kept only when it recurs, passes the scrubber and the shape check
  (`constant_ok`). Days are META `self.mine.shapes.<date>`, names `self.mine.vocab`. `motif.rs` splits a run at authored and
  never-mined calls (`shape::NEVER`), counts 3-to-16-grams once per run, keeps the closed ones that reach the thresholds,
  `align.rs` aligns them to the medoid (core at 60%, optional at 25%), `slots.rs` classifies slots, `cost.rs` attributes
  loops and scores (§2.6). `bench.rs` reads Harbor's ATIF into `bench:<suite>` runs. The types R7 reads (`Motif`, `Shape`,
  `SlotKind`, `Occurrence`, `Motif::spec`) are public and stable. The backlog night files a motif as a cause
  `pattern:<scope>:<digest>` (`backlog/read.rs::of_motif`, `Meta.pattern`, `rank::pattern_score`) and never starts it;
  `theseus self patterns` (`Core::self_patterns`, the owner's, works with the mode off) runs the same pass in memory and
  writes nothing. Tests: `rsi/mine/*/tests.rs`, `rpc/tests_self_patterns.rs`.
  **From a motif to a checked tool** (theseus-pw1q.21.6; no store format change: one ledger kind, `self.mint.validated`;
  `rsi/mint/`, `fact/rsi_mint.rs`; nothing is loaded by validating; deploying is below). `spec.rs`: a
  motif's skeleton as R6's flow spec (`flow_spec`, through `flow::spec`'s types), a constant kept only past the shape check
  and the scrubber, else an input; where arms differ by what rerere did it adds a synthetic `git rerere status` step (the
  runtime makes those classes only from that program). `kind.rs`: the rule, in order (flow when every step is an allowed
  tool; an owner card, never a mint, for a tool that would need the network or a secret; Rust; MCP), under `[self.mint]
  kinds`. `brief.rs`: the occurrences split by a keyed hash (META `self.mint.key`) into a build half the writer sees and a
  check half at `<state>/self/mint/<name>/check/`; `brief` is given the build half alone and `isolated` asserts on its
  bytes. The data rule (`data_rule`): a private motif is written and checked only on a profile whose **origin** is not
  restricted (`Config::profile_origin`), never by name. `validate/`: `walk.rs` (a flow's engine over any runner of steps),
  `fixtures.rs` (one fixture per arm, `GitFixtures`: a temp repository with an upstream, clean, known and unknown
  conflicts, red tests; an arm no fixture builds fails by name), `dry.rs` (the dry replay: 3 instances, 90% reproduce,
  0 deviations; remote steps stubbed with recorded results), `ab.rs` (arm A the recorded tool set, arm B plus the flow,
  each arm reserved on `[self.budget]` and held to `[self.mint] usd`; 15% of tokens or time, 2 decisions removed, no loss of
  success; a tool never called goes back to the backlog with that evidence). `run.rs` and `source.rs` are the daemon's
  side: the writer's turn (its reply ends `MINT {json}`), the instances read from sessions (`own_bench/whole`'s
  `scratch` and `undo`), `Hands` as the runner (`Steps for Hands`; arm B's `Minted` flow is listed in the replay's
  request and run through the same hands). The start is `backlog/start.rs`'s `backlog_mint_start`, beside the fix start
  with its own cap (`[self.mint] per_night`); a motif whose pending record exists is not started again. Tests:
  `rsi/mint/tests.rs`, `validate/tests_*.rs`, `rpc/tests_self_mint.rs`, `own_bench/whole/tests_minted.rs`.
  **Deploy, keep or retire, and the morning notice** (theseus-pw1q.21.8; no store format change: two ledger kinds,
  `self.mint.deployed` and `self.mint.retired`, a `mint` field on `extend.acked`, and the META key `self.notice.posted`;
  `rsi/mint/{deploy,retire,rows}.rs`, `rsi/digest/notice.rs`, `backlog/graduate.rs`; every step asks `rsi::gate` and reads
  and writes nothing while the mode is off or the switch halted). `deploy.rs`: a flow that passed (`mint_keep_report`
  keeps its `ValidateReport` at `<state>/self/mint/<placeholder>/report.json` beside `pending.json`) is loaded through
  `Core::flow_load` from its frozen copy, by `"self"`, in its scope (`mint_deploy_flow`); a proposed MCP extension is
  *self-loaded* (`mint_self_load`: the extension's own `extend.ack` card answered by `"self"` through `answer_extension`,
  `extend.acked {by: "self", mint}`) only when `self_load_refusal` finds no network, no hands (a credential), no failed
  test and at least one declared test, under `extend::load::minted_ceiling` (the proposing place's ceiling, its floor at
  least `notify`: "read-only, no network", held by `Floors` on every call); no surface answers as `"self"`
  (`Answerer` of the unnamed surface is refused by the place rule), and the one path takes a manifest's own card, never a
  call's. A Rust mint is no deploy here: its item carries the `mint:<id>` evidence tag, its brief tells the writer to
  call `self.propose_change` with that item, and the self-join closes it. At most `[self.mint] per_night` deploys a day
  (the last 24 hours' `self.mint.deployed` rows) and `max_live` live minted tools (flows by `"self"`, extensions acked
  by `"self"`); a flow with no room waits, validated, and `mint_deploy_waiting` (in the tender) tries it again.
  `retire.rs` (`Core::mint_tend`, a tender every 6 hours after serving, `mint_tender_after_serving`): per live minted tool,
  `decide` over `Evidence` counted from `action.*` rows (`tool_calls`, `extend/usage.rs`, a flow's producer is
  `harness:flow:<name>`): a red health signal attributed to it (`attributed`: the only minted tool with a call within an
  hour before the red, from the revert watch's `Signals`), more than 20% failed over at least 5 calls, fewer than 3 calls
  in `retire_after_days` once that old, its motif's by-hand runs (the miner, `mine_run`, once a pass) not falling once
  that old; retiring is `flow_retire` or `revoke_loaded` by `"self"` and a `self.mint.retired` row, and the frozen copy
  is never deleted. Not built, said in the module's doc: a write outside the declared scope (nothing in the ledger can
  show one: a flow has no authored write and an extension's are discarded) and a live measure of the saving.
  `theseus self revert flow:<name>` and `ext:<name>` (a minted one) are the owner's one verb. **Graduation** extends to
  flows: `graduate_flow_obs` files `graduate flow:<name>` (key `graduate.flow:<name>:<digest>`) at the extensions'
  rule, and a tool retired for a motif that did not fall is a cause, `mint.retired:<tool>:<digest>`. **The notice**
  (`digest/notice.rs`): `notice_text` is pure over the night's ledger rows (joins with their review, sealed score and
  install, deploys, retires, the backlog's waits and counts); `Core::post_self_notice_if_due`, from the harness tick,
  posts one `self_digest` post to the owner's DM from `[initiative] morning_hour`, once a local day, only with
  `[self] mode = "act"`, never halted, scrubbed, and a night with nothing to say writes only its mark. Tests:
  `rsi/mint/tests_retire.rs`, `rsi/digest/tests_notice.rs` (the golden), `extend/tests_load/self_load.rs`,
  `rpc/tests_self_deploy.rs`.
- **The index tender's supervisor**: `tender.rs` (row 51): it starts `theseus-index` 2 s after serving
  (`START_AFTER`, so a start's aftermath stays quiet), restarts it with backoff, takes over the one an exec kept
  at once, and asks it for health and `index.query`, each call bounded (health asks only a tender that runs, and
  never past 100 ms; one status read a second serves every caller, `tender/status_cache.rs`, and a status older than
  3 min says `stale`, theseus-id8d). Its rows are facts (`fact/index.rs`). Its tests, `tests_tender.rs`, run on tokio's paused
  clock with a stand-in `Os`. Its gauges (`theseus.index.*`: lag, documents, RSS, and restarts by their rise) are
  sampled from health's block each metrics interval after serving, only while telemetry has an endpoint
  (`tender/sample.rs`, theseus-gfi4; tests `telemetry/tests_index.rs`).
- **The judge** (M5, steps 23a and 23b): `judge/` (`JudgeService`, built at the first judgment; `sink.rs`, the
  batched frames; `spend.rs`, the shadow day budget; `loop_end.rs`, loop.v1's input; `mark.rs`, a dispatch's mark).
  A judgment a turn dispatches is decided (mode and sample, pure) and its id minted before the turn's last frame,
  and marked in the trace there (a zero-length `judge` span of kind `mark`: pack, point, mode, judgment); the call is
  spawned after the frame with that id (`theseus_judge::Ask::id`). Every point that dispatches inside a turn marks
  the same way. The facts (`fact/judge.rs`) say their sentences, and `Telemetry::record_judgment` counts each
  judgment, once the sink's frame is written; nothing of a judgment rides in a turn's frames but its mark. The
  sink's frames are written only between turns (theseus-0j2.8), through the memory pass's writer handshake
  (`memory_pass::turns`, `JudgeService::write_between`), as consolidation's are: a row, its sentences and its
  metric wait while turns run (the pass's bounds end the wait), and a press finds it in `pending` meanwhile. A
  backlog keeps one clock from its pass's start until the queue is empty (`sink::Queue`, theseus-s1am), so past the
  quiet bound it drains in the next gaps; a clean stop writes the queue before its last checkpoint (`finish_stop`,
  `JudgeService::flush_sink`, theseus-ych4), and a SIGKILL loses it. Tests: `tests_sink_backlog.rs`,
  `tests_sink_flush.rs`. A frame's staged blobs are written before its wait, as one batch (`Blobs::put_many`), a
  stop's frames hold 512, categorize's mark rides in its row's frame, and a shadow judgment no turn waits on writes
  its budget block between turns (`reserve_between`; theseus-ehkp, theseus-xkbs; `tests_sink_blobs.rs`,
  `tests_sink_off_turn.rs`, `tests_sink_busy.rs`, `judge/tests_ahead.rs`, and categorize.rs's own, theseus-5o3d).
  A crash inside a batch leaves its temporary files, and the next start sweeps a dead writer's once it serves, on a
  thread of its own (`Blobs::sweep`, `Core::sweep_blobs_after_serving`; theseus-vipg; theseusd's `tests/spool_sweep.rs`).
  `theseus-sim bench turn --judge` measures where the judge's frames land.
  `judge.list` and `judge.get` are `rpc/judge.rs`; `judge.list` pages back from the newest `judge.call` row (its
  kind's tag, or its kind-and-session tag) and stops one match past its limit, so `matched` is a floor when `more`
  (theseus-wse2; `tests_judge_reads.rs`). Tests: `tests_judge.rs`, `tests_judge_surfaces.rs`,
  `telemetry/tests_judge.rs`.
  - **The judgments a turn waits on write nothing before their call** (theseus-otny): route.v1's inbound batch and
    a live rerank stage their state's blob (`stage_blob`; the sink writes it just before the first row naming it,
    `write_staged_blobs`) and write the shadow budget's block beside the call (`reserve_beside`, `Beside`), so their
    syncs (1.4 s each under a neighbour's IO, 2026-10-04: the 3.2 s late verdict) never delay a verdict. A person's
    message warms Jev's client as it arrives (`warm_on_message`, from `TurnRunner::run`: built, and two `HEAD`s of
    the judge's path, nothing billed, unless it answered within `theseus_judge::client::POOL_IDLE`, 180 s, under the
    edge's 200 to 400 s). From serving on (`Core::warm_judge`, theseusd's `after_serving`, never on the start
    path), `judge/warm.rs` opens two and keeps them: a keeper on tokio's timer sends the `HEAD`s again after each
    `theseus_judge::client::KEEP_WARM` (150 s) of Jev's silence, so a fresh daemon's first message pays no
    connection setup (theseus-ddbi; `tests_route_wait.rs`). A try that fails to connect, with nothing answered since, is `jev_unreachable`, and then
    no turn waits on route or rerank. The fake Jev answers a `HEAD` 405 and counts it as `warmups()`, apart from
    `connections()`, which stays the calls'.
  - **At the gate** (step 24, `gate.rs`): `security.v3` at every call that acts, and at a fetch or a search in a
    holding session (Q12). `security.v1`, asked beside it until theseus-rwpbg.6, is unwired (still embedded in
    `theseus-judge`'s packs, for replay and its stored judgments). `ToolRuntime::start` plans the call and sends its
    notice, then `judge_at_gate` makes the choice and the mark, nothing else; the gate's decision never waits on Jev or
    changes with it. A gate judgment's id is its pack and the call's correlation id hashed (`gate::judgment_id`), so
    the turn's trace marks it (under the call's span) and a "should have asked" press labels it (`judge.label`, in the
    press's frame) before its row is written. A notified call's score, v3's `risky` and the pack's mode, follows its
    notice as `judge.scored`. Tests: `tests_security.rs`.
  - **v3's live notices** (step 24's notices, theseus-0j2.13; the owner's decision of 2026-10-04): `notice.rs`.
    `WIRED` gives `security.v3` `live` while `[judge.packs."security.v3"] notices` is on (the default;
    `JudgeService::given`/`mode`), so its judgments are marked and recorded `live`. When v3, live, answers `risky`
    act-true for an open call the hold did not make wait (`notice::flagged`), the gate's task posts once the
    judgments land, never on the call's path: one frame with the post to the owner (`Outbox::stage_to_operator`,
    kind `jev_notice`, which the Discord courier posts to the owner's DM alone) and its `tool.notified` row
    (`by: judge`, keyed `notice_<judgment>`, scoped `judge:security`), then `judge.noticed` to the turn's clients.
    The brake is the `security` rollback rules (`notice::rules`: v3's, as the ladder reads them with its adoption
    table's; `learn::check_all` over today's notices and the owner's `noise`
    labels on v3's judgments, at each notice and each such label, `JudgeService::after_label`): a fired rule
    writes one `judge.paused` row (`what: "notices"`), a `jev_paused` post and a META mark (`judge.notices`,
    health's read), and holds notices to the next local day. The first use in a run reads today's rows in
    `judge:security`: the pause by its key, and the day's `tool.notified` and `judge.label` rows by a page from the
    local midnight (`read_day`, theseus-b8e2), never the scope whole but while the index's shape is built. A label on a noticed judgment edits its post (`jev_labeled`), and `judge.label` takes a
    judgment whose `judge.call` row the sink has not written yet from its notice's row. Health's `notices`.
    Tests: `tests_notices.rs`.
  - **At an exchange's end** (step 28b, `categorize.rs`): at a private conversation's exchange end (never a shared
    place's, never a task's), `categorize.v1` judges in shadow when 10 human messages arrived since the session's
    last judgment, or the exchange began after 30 minutes' quiet; the decision, off the turn's path, reads the
    session's records after its mark (META `judge.categorize.<session>`, moved as a judgment is dispatched). It
    runs outside every turn, so it marks no trace. The service reaches the core by `Weak`
    (`JudgeService::attach`). Jev writes no membership: `rpc/proposals.rs` lists its proposals
    (`ontology.proposals`, one scan of `judge:categorize`) and takes the operator's answer
    (`ontology.proposal.accept`/`reject`, through `judge_act`): an accept's membership, its topic for
    `new_topic`, and its `judge.label` row (`fact::judge::ProposalLabel`) in one frame. Tests:
    `tests_categorize.rs`.
  - **Jev files the session into the ontology** (theseus-xzxqv; no store format change: two ledger kinds, `onto.filed` and
    `onto.named`, and a META key `onto.named.<session>.<kind>`; `judge/onto/`, `config/ontology.rs`, `rpc/ontology_kinds.rs`,
    `rpc/judge_latency.rs`, `fact/onto.rs`; pack `onto.v1`): `onto.v1` is live (`WIRED`) and rides the inbound batch beside
    `classify.v3` and `role.v1` (one state, one request, cost split by question count), in a private place, for a person's
    message, never a task's, while `[ontology] judge` is on (off: the batch is byte for byte what it was and nothing is written).
    Its state is the inbound state plus `thread`, `project`, `memberships` and `mined_names` (`theseus_judge::builders::onto`);
    `route.v6`, which the turn waits on, never carries them (`prepare_inbound` builds its state from the plain input), and
    `request = "own"` moves `onto.v1` to a third request no turn waits on. The kinds asked are those whose row lists `jev` in
    `assigned_by` among the six the pack asks about (the seeded `project` is given by the harness, so it is not asked until a row
    lets Jev assign it); each kind's options are the picker's (at most 20: the session's, the thread's, a word match, the most
    recent), `new` and `none`, and a mined name's `<kind>_name`. **Three steps, none on the turn's path.** (1) Asked
    (`inbound.rs`: `onto_wanted`, `onto::fill`, `onto::context` puts the category ids and names behind `c1`, `n1` into the judgment's
    context). (2) Landed (`JudgeService::onto_landed`, from `hand_thread_answer`): `decide::decide` (pure) reads the answers in the
    bands (`[ontology] act` 0.80, `confirm` 0.55), and its words are all that is used: the reply's footer (`take_filed` in
    `reply_step`, `TurnSubmitResult.filed`, `filed: topic › a › b (new)`, the same early-or-late rule the task offer has, the late
    one a `notice` post) and the naming call. (3) Written (`sink.rs`): the sink decides again under the ontology's writer lock
    (`Board::stage`, `Staged`) and adds the records, their rows (`Machine`, `rpc/ontology_machine.rs`, scope
    `ontology.write:<judgment>`, `added_by jev:<judgment>`), an `onto.filed` row and the owner's cards to the judgment's own frame;
    the snapshot is held only after that frame is written. A mint (`new` at the act band with a `<kind>_name` at the act band,
    which files under a same-named category instead of minting twice) sends one `onto_card` post to the owner's DM with its undo;
    a confirm-band answer sends one asking and writes nothing; a plain filing sends none; below the confirm band nothing
    but the judgment's row. The operator's entries are never changed: a list full of them refuses Jev's, a refusal is a step
    in the `onto.filed` row (`disagrees`), never a panic. **The naming call** (`onto/name.rs`): `new` at the act band and no mined
    name fits, one call to `[ontology] name_profile` for three names and a line, reserved on the day ceiling, an `onto.named` row
    (a spend, `day_ceiling::SPEND_KINDS`), at most once a session, kind and local day (the META marker); the session's next
    `onto.v1` offers the names first. **Undo**: `ontology.undo` (`rpc/ontology_kinds.rs`; `theseus ontology undo <judgment>`,
    refused in a job) is `Core::ontology_undo`; `ontology.kind.add|retire` write the kinds table (`ontology.kind` rows).
    `judge.latency` (`theseus judge latency --since <date>`) reads the batch's p50 and p95 by local day from the `judge.call`
    rows. Known limits: a message the thread router judges early files the home session, not the thread's; the cards carry
    their undo as a command until the binding's button lands. Tests: `tests_onto.rs`, `judge/onto/decide.rs`' own,
    `rpc/judge_latency.rs`'s, theseus-judge's `tests_onto.rs`.
  - **The ontology reaches routing, context and recall** (theseus-jrf4c; no store format change; `category_recall.rs`,
    `judge/onto/{decide,mod,stage}.rs`): (1) **the project answer attaches**: while a session has no project, `onto.v1`
    asks `project` over the open projects of the task graph (`project_kind`; id = the task id, `new` files nothing, a project
    is the owner's to make); at the act band the plan's `attach` sets `SessionRecord.project` once the judgment's frame is
    written (`stage::attach`, only while it is still none: Jev never moves one the owner or the model attached), so the
    slice reaches the next compile; at the confirm band a card says what to run (`theseus project attach SESSION ID`). The
    row's step is `attached`, the footer `project › Title`. (2) **route.v6's persona question** gets the session's
    interpreted categories by path as the state's `session_categories` (`TurnRunner::category_names`; at most 8; a person is
    not one; none in a shared place), as evidence, no new question and no pack text change. (3) **The compile walk** needs no
    code for the new kinds (a membership of any interpreted kind composes by precedence, its origin and confidence in the
    manifest); `theseus-ontology`'s `tests_manifest.rs` is the golden for a session in six kinds. The harness-given
    `thread` and `project` categories are not read at compile (the slice and the thread's topics already carry them).
    (4) **Recall's category term**: `CategoryScene` (private places only, the held snapshot, `[ontology] recall_boost`
    0.25, 0 off) reads the stored memberships, the thread (`ThreadScene::of`) and the project of the asker and of each
    candidate's session, and hands the pure pipeline a `theseus_memory::CategoryBoost` (project and thread weight 1, topic
    0.6, the rest 0.3). Ambient turn recall only: the model's lookups, `thread.read` and a shared place have none. Tests:
    `tests_onto.rs`, `tests_category.rs` (memory), `tests_manifest.rs`.
  - **People from text** (theseus-wy7y, `judge/people/`): `people.v1`, live (`WIRED`), at the same exchange end as
    `categorize.v1` (`live.rs`: private places only, never a task's, `categorize::due` over the records after its own
    mark, META `judge.people.<session>`, moved in the extraction's frame), and the owner's backfill
    (`backfill.rs`, `import.people { propose }`: a tag's sessions in id order, the machine's quiet between them,
    under a spend cap for the run, resumable from META `import.people.propose.<tag>`, a dry run pricing it with no
    call). A pass (`run.rs`): `[people] extract_profile` (`config/people.rs`) answers through one tool
    (`extract.rs`: name, handles, role line, evidence; scrubbed lines), a `people.extracted` row (cost, tokens,
    model; a day-ceiling spend kind); the owner, personas, agents, `[people] not_people` and a session's known names
    excluded before any call (`NotPeople`, built by `Core::not_people` in `house.rs`, theseus-0p1r: the place rule's
    owner handles, not only `[places] owner`; the held person they hold, by its name and each word, never a
    candidate, and a `match` to it (it stays an option, so Jev can name a form of his name the store cannot know)
    never listed; every imported episode's agent and each agent's `Name:` in its imported
    `IDENTITY.md`, kept per catalog version; the house's names, the profiles and models; a bot's or a UI's name;
    `fold` drops a leading "@"); one `people.v1` judgment a kept candidate (real, involved, which of the 50 nearest
    held people or new, whether its role line judges the person, whether it carries their pay, money, health,
    leave or HR matters: either, or unanswered, drops the line), the candidate in its context. Code
    decides as the proposals are read (`people::decide`, `[people] act`/`confirm`; a person the exclusions exclude
    is not listed nor bulk-accepted, counted in `ontology.proposals`' `hidden`): `rpc/proposals.rs` lists them
    beside the topics' (`OntologyProposal.person`), and an accept joins the held person, or declares the new one
    (unless an exact handle or name finds it by then: any `name:` handle of a live person counts, not only the display name, and two such people refuse the accept with both ids, `name_holders`, theseus-7mow). Tests: `judge/people/tests.rs`, `tests_handles.rs`.
    **Live, gated by Jev** (theseus-u5n8, the owner's "combine, gated by Jev"; `seen.rs`, pack `people_seen.v1`,
    scope `judge:people_seen`): at each due point one Jev call first, over the exchange's lines and the held people
    it may involve (`seen::listed`: the session's own, then those its lines name; never the owner's or one
    `NotPeople` excludes; at most 20, two per-item Nouls of ten). A listed person's Noul at `[people] confirm` is a
    proposal of that person (named `<judgment>/<person>`, once a session, not again where the owner rejected it);
    only its `unlisted` Noul at `[people] gate` (0.6) runs the extraction and `people.v1`. Under it, or unanswered,
    no model call; the mark moves either way. The backfill has no gate. Tests: `judge/people/tests_seen.rs`.
    **The nightly sweep** (theseus-j8qb, `sweep.rs`): what the gate let pass. At `[judge] learning_hour` (the
    learning tender's `due` and `tend`: a missed day once, never within 10 minutes of a start), one pass with the
    purpose `sweep` for each private session, never a task's, with human text since the last sweep (a day, the
    first time) after its newest extraction; the live mark moves to what it read, never back. Under `[people]
    sweep_usd_per_day` (2.0 a local day; 0, or `live = false`, turns it off), the backfill's worst-case rule; a
    `people.swept` row and META `people.sweep.last` (when, the day, its spend) in one frame. Tests:
    `judge/people/tests_sweep.rs`.
    **One row per person** (theseus-fvyx, `rpc/proposal_groups.rs`): `ontology.proposals` with `by_person` groups
    the people's proposals by the held person (an exact handle or the folded name finds) or the folded name, a
    bare first name inside the one other person, proposed or held, whose name holds that word (`first_names`;
    with two or more it is its own row, `ambiguous`); `min_confidence` and `limit` over rows. A row's answer is
    `ontology.proposal.accept_all` with its `judgments` and `as_person` (an accept's `as_person`: a new person's
    proposal joins that person, or declares it by that name) or `ontology.proposal.reject_all`; both judge once,
    one scan. `accept_all` plans all its accepts and writes one frame (`finish_bulk`, `rpc/proposals/accept.rs`);
    `reject_all` still answers each proposal through `reject_open`. The bulk yes takes
    people a row at a time (a row's best at the confidence), leaving an ambiguous first name with why. Tests:
    `judge/people/tests_groups.rs` (the owner's store's shape: 1,550-odd proposals under 298 names).
  - **The learning ledger** (step 25c, `learning/`): labels (`labels.rs`: what a label says of a Noul, a Choice,
    a Score; the heaviest counts, then the newest), system labels derived by each run (`system.rs`, weight 0.5,
    keyed by judgment, question and rule so a second run writes none), the report per pack version and question
    from the `judge:<pack>` scopes alone, every number from `theseus_judge::learn` (`report.rs`), with its holdout
    frozen into it, and the tender (`tender.rs`: never within 10 minutes of a start, at `[judge] learning_hour`, a
    `learning` thread at nice 19, in `SCHED_IDLE`, and about 5% of a core, each next pack waiting while the
    machine is busy: theseus-tood). `judge.label` (`judge_act(Act::JudgeLabel)`) and
    `learning.report` are `rpc/learning.rs`; the run writes its labels, `judge.report` rows and the run's META mark
    in one frame, then `<state>/learning/<date>.json`. The mark keeps the last position the run read (`through`),
    and the next run's rules read no judgment whose windows closed before it (`system::Cut`, `open_ms`: its
    session's nodes, its call's action, the task briefs; theseus-cf5c), and log how many sessions they read. The
    mark's `cut_ms` is where the next run cuts: the run's clock, never past the newest judgment it read, so a clock
    that read ahead closes no open window (theseus-gf8j); a mark without it walks everything once.
    Tests: `tests_learning.rs`, `learning::*::tests`.
  - **Routing** (step 25e, theseus-0j2.11): the route pack (`judge::inbound::ROUTE_PACK`: `route.v5` since
    theseus-c1uz, route.v4's `mode` (with its `review`) and `reply_effort` word for word and two questions that decide
    nothing for its promotion, `persona` and `seal` (below), live at once in place of `route.v4`, `route.v3`, `route.v2` and `route.v1`, which stay embedded) asks at the inbound point in a request of its own, beside
    the batch of `classify.v1` and `role.v1` (theseus-ddbi: one question answers sooner than the batch), live while
    `[judge]` is on (`[routing]`, `config/routing.rs`, lowers it). Its verdict comes back over a oneshot
    (`RouteWait`, an `Answered` with the time it came) the moment its request answers, and the call waits for a
    permit rather than being shed. The turn waits for it beside its first compile, at most
    `max_wait_ms` after it (`turn/route_step.rs`, `beside`; a late verdict applies to the next message alone, and a
    late detour's (`trivial`, `quick`) to none: `routing::carries`, theseus-6n5j), and not at all while Jev is known unreachable
    (`JudgeService::jev_unreachable`, reason `unreachable`, theseus-otny); `routing.rs` decides, purely, at the
    mode's own bar (`[routing.classes.<class>] switch_confidence`, else trivial's 0.4, else the section's 0.6:
    `RoutingConfig::confidence_for`): the mode's first usable profile under a place's cap, a detour (the modes of
    `config::routing::DETOURS`, `trivial` and `quick`: that turn alone, compiled outside the session's compilation,
    which it never writes; never test a mode by name, ask `is_detour`), or a switch of the session's
    `routed` profile (stored: format 15), held above `cold_switch_tokens` until a second turn agrees. A turn whose
    profile the owner chose (`Target.chosen`; the pane's `carried` profile is none) is recorded in shadow. Only the
    compilation the call uses is persisted (`RouteState.defer_persist`), and only its `context.compiled` and
    `loop.started` recorded (`RouteState.deferred`, theseus-d13v); a detour's loop records `loop.started` and
    `context.detoured` (`ContextDetour` carries `recall`, whether the detour's request held the turn's own recall, and `kind` is `steered` for a steer that names no mode, theseus-nfrlj; the names and token counts of its request's parts and the families it loaded, `fact/detour.rs`, `turn/detour_step.rs`,
    in the one frame with `loop.started`) and no `context.compiled`, since its compilation is never stored (`tests_route_rows.rs`); `context.explain`
    explains a turn with no compile from that row (`ContextExplainResult.detour`, the turn list's `detoured`; theseus-r5ydi), and the
    conversation's project slice rides a detour's request as any (theseus-v4u66; `tests_detour.rs`). The row is `route.decided`
    (`fact/route.rs`: `wait_ms`, `late` when the verdict missed the wait, `answered_ms` when the request came back
    after the turn's start, if it had; each live turn's wait is `theseus.route.wait` by `theseus.route.late`,
    `tests_route_late.rs`); thinking goes back only to the model that wrote it (`tests_thinking_writer.rs`), and a place's
    profile caps it (`tests_route_cap.rs`). `routed` holds only while route.v1
    acts live for the session (theseus-9yyr, `route_base`): routing off or in shadow, the ladder's rollback, the
    judge off, or Jev unreachable (`JudgeService::reachable`) clear it at the next turn, in the turn's own session
    write, and so does a `profile.use` after the session's last turn began (META `live_profile.switched_ms`
    against the turn id's time, `crate::id_ms`). A move keeps its base, `Routed.from` (the profile's name it was
    first moved from, format 21, theseus-0j2.17): once route.v1 stops acting the turn runs there, and a turn whose
    base is another (the live profile or a place's changed) clears the move (`same_base`). The pane's carried
    profile, when it is the routed one, stands for `from` (`turn_submit`), never a new base. Tests:
    `tests_route.rs`, `tests_route_base.rs`, `routing::tests`, `turn::route_step::tests`.
    **China-origin profiles** (theseus-a1wvq.4; `config/routing_cn.rs`, `routing/tests_cn.rs`): a restricted profile (origin `cn`
    or `unknown`) enters a class only through `trivial`'s `cn_profiles` (`cn_after` places them; `RoutingClasses::effective`)
    and runs only for a cleared ask (`Ask.cleared`, false until the open-request row), `cheapest` never picks one, and a
    class's `profiles` naming one fails the load, as does `cn_profiles` on any other class.
    Routing refuses a restricted profile only in these mode lists and `cn_profiles`: a restricted session base, `-P` pin,
    place profile or `[model]` default is not refused by routing, and the floor moves or fails that turn (theseus-1v7fl).
    **Only Jev elevates, and it steps down** (theseus-p8dy): a session Jev moved above its base (`Routed.from`)
    goes back to it after two verdicts in a row whose mode's first profile is the base or cheaper (`chat`,
    `routine_coding`, `other`): the first sets a `Hold` of mode `step_down` toward the base, the second makes the
    switch (`Reason::StepDown`), at any context size; a dearer verdict or one under its bar breaks the row, a detour
    leaves it, and never below the base (`routing::step_down`). The owner's correction passes no base, so this path
    never steps it down; a session an owner's correction raised is not told apart from Jev's (no stored field).
    **A task's base from its brief** (theseus-ypnxt.7; `turn/brief_step.rs`, `routing/brief.rs`): a task the owner starts
    with no profile (`task.start`; META `task.route.<session>`, written in the open's frame) asks the route pack alone about its
    brief on its first turn (`JudgeService::at_brief`), waits for it as a message does, and `brief_base` makes the class's
    first usable profile, under the cap, the task's base (`last_target`, no `routed`), a detour's class included; no verdict
    in time, under the bar, or no usable profile leaves the parent's. A profile asked for pins. The row says `source: task brief`.
    The dear tier's guard (`[routing] dear_tier_max_tokens`, 200,000): over it, a `sophisticated` or `review`
    verdict skips a profile whose catalog input price is above the next usable one's (`Reason::ContextGuard`).
    Tests: `tests_route_down.rs`, `routing::tests`.
    **A running task's checkpoints** (theseus-ypnxt.9; `turn/checkpoint_step.rs`, `routing/checkpoint.rs`, `[routing.task_checkpoints]`
    in `config/routing_checkpoints.rs`; on by default): a task started with no profile (the brief's mark, `brief_step::marked`; a
    pinned one has no state and asks nothing), and since theseus-a1wvq a conversation's turn the route placed (not the owner's
    pin, not a detour: `placed_by_route`; its base is where the route put it, its text the message's first characters, its rows'
    source `turn checkpoint`; a turn the owner's correction placed asks nothing; a move persists in the session's `routed` as
    a route switch does, so the next message starts on the moved profile until the step-down), asks the route pack about its
    recent state at a loop's end (`checkpoint_ended`, after
    `advance`) when it fails `failing_streak` loops in a row (a result `ResultStatus::is_error` marks), makes no progress for
    `quiet_loops`, changes its current plan step, or marks the last open one done (`Tally`, pure), at most one verdict per
    `min_loops_between` loops and one in flight, never waited on: the answer is read with `try_recv` at the first later loop
    boundary (`checkpoint_apply`, beside `steer_in`), and a flight a window old is dropped. The text is the brief's first 600
    characters, the plan (12 steps), the last 6 loops' lines and the trigger, within 1,500 tokens (`checkpoint::text`); no
    pack's wording changes. `checkpoint::decide` is the rule beside `decide` and `step_down`: only Jev elevates, the base
    (`Routed.from`, else the brief's pick) is the floor, two agreeing verdicts step back down to it, a cold switch up waits for
    a second, a detour's class moves nothing, `max_switches` bounds the moves up (`Reason::SwitchCap`; a step down past it
    is allowed). A move is a refusal's fallback's shape: one slot of `checkpoint_step::Slots` a move, `route_to`, the
    next compile on the new model; each verdict is a `route.decided` row (`source: task checkpoint`, `trigger`, `loop`,
    and on a switch `cold_tokens` and `cache_usd`). The judgment's settled cost is charged to the task's execution by the
    judge (`Kernel::charge_execution`: no row, no word to the day ceiling, since `judge.budget` already holds it), a task
    that cannot cover `RESERVE_MICROS` skips the ask (reason `budget`), and its `loop.ended` rows say `on` while it is off
    its base (the work board's `moved_to`, the progress line's `on sonnet`). **Effort without a move** (theseus-53ujg.3): a
    verdict that keeps the profile and asks more effort raises it by `route_effort`'s rule, with no slot; the sticky rule
    never holds that raise, a model that takes per-message effort records it as not applied, and a checkpoint never lowers
    effort. An ineligible turn's checkpoint state stays `Off` (`checkpoint_apply` no longer puts it back as `Unread`).
    Tests: `tests_task_checkpoint.rs`, `tests_checkpoint_effort.rs`,
    `routing::checkpoint::tests`. Not covered: a task a model made with `task.create` (it has no brief mark).
    **A routed profile's effort floor** (theseus-53ujg.3; `routing/effort.rs`): on a profile a routing class chose (not the
    session's base), the turn runs at no less than the profile's own `effort`. It applies after the xhigh/max gate and before
    `effort_bounds`, is not gated, and `effort_reason` is `floored` only when it raised the level; on the base profile Jev
    may still go below.
    **The effort** (route.v3, theseus-qe3v; xhigh and max only above `[routing] high_effort_confidence`, 0.40, else
    `high` with `effort_reason` `held_high`, theseus-p8dy; xhigh also only on a sure answer, so an unsure xhigh runs
    `high` as `held_high_unsure` at any confidence, theseus-goq94, while max keeps the bar alone): `reply_effort` rides the route pack's one request beside `mode`;
    `routing/effort.rs` decides, purely: Jev's level over the profile's `effort` unless `effort_fixed`, clamped to
    `[routing] effort_bounds`, only for a model whose catalog row takes effort; an answer below the question's confirm
    band still applies at `[routing] effort_confidence` (0.35) or more, as `applied_unsure` (theseus-7pm8t: an effort
    is not held to the model switch's bar); under it, `unclear`, a pin, shadow, or a late (carried) verdict leave the
    profile's own. `apply_effort` sets it on the spec
    and the first compile's request (`compiler::output_config`), and takes that request's digest again, which the
    turn records and the overflow retry compares. `route.decided`'s `effort_*` fields and `TurnRoute`
    say which. Tests: `tests_route_effort.rs`, `routing::effort::tests`.
    **A turn's effort keeps the cache** (theseus-o719, `compiler/effort.rs`): a change of the top-level
    `output_config.effort` restarts the provider's cache of the messages (only tools and system stay: a 625k DM on
    Fable 5.1 wrote 612k again for one `low` turn). On a model that takes per-message effort (`effort::default_of`:
    Fable 5.1 and Sonnet 5.5; the 400s of 2026-10-11 and 2026-10-21 were requests sent without the beta, theseus-blzt6;
    Anthropic's API only) the top-level effort stays the profile's own, and
    a turn at another level gets an effort-only system message before its user message
    (`mid-conversation-output-config-2026-07-01`). Its first answer records the level (`effort`, store format 27);
    the render puts each record's message back before the user message its answer follows, so later requests repeat
    the bytes; the beta rides every request that carries one, at every loop (`place`, `retarget`: `sync_beta`). Never
    derive these messages from the spec: a history edit breaks the cache and preserved thinking. The 2026-10-11 Sonnet 5.5
    400 (`messages.190.output_config: Extra inputs are not permitted`) was a request sent without the beta, not a refusal
    of the message. **The server-side fallback** (`BETA_FALLBACKS`, `compiler::server_fallbacks`): the request is compiled
    for the profile's model alone, and `effort::takes` reads that model's row, never the fallback the provider may serve
    it on, so the messages and the beta ride it either way. A fallback model without per-message effort that refuses
    them answers the 400 below, and the hold lands on the request's model, not on the fallback's.
    A 400 naming `output_config` on a request that carries one is asked again once without them, the level at the
    top, and the model takes none for an hour, `REFUSAL_HOLDS_MS` (`turn/effort_step.rs`, `provider.effort_refused`).
    A one-step change held on a model without per-message effort (`stick`) says `sticky`, or `sticky_unsure` when the
    held answer was `applied_unsure`, so the row keeps the bar that admitted it (theseus-8zgji). An `xhigh` or `max` held at `high` says `held_high` for
    a sure answer and `held_high_unsure` for one under the confirm bar; a held one then held again by `stick` says
    `sticky`, or `sticky_unsure` for a `held_high_unsure` one (theseus-t7b96).
    **A sticky hold has a way out** (theseus-u5a37): `keep_warm.sent` keeps, beside the level a request carried, the
    kind of hold that carried it (`effort::Hold`: `Sticky`, or `Unsure` for `sticky_unsure`), in memory. On a later turn
    a held change whose answer is sure enough ends the hold: `effort::reverts` wants a `sure` answer in a `sticky`
    window and `confidence >= [routing] effort_confidence` in a `sticky_unsure` one (the lower bar the owner asked
    for); the request then carries Jev's level and the row says `sticky_reverted`. The turn that opens a hold never
    reverts it, and a checkpoint's raise is never held or reverted. A revert rewrites the messages' cache once, as a
    two-step change does.
    **The checkpoint says what effort the work ran at** (theseus-ujzuh): `checkpoint::text_with` adds two lines before
    `Now:`, kept whole when the text is cut: `Effort: <level>[, raised from <level> at loop <n>]` (the top-level effort
    the last loop's request carried) and `Checkpoints this turn: <n>, failing <k> in a row`, from `Active.evidence`.
    No new call and no stored state.
    Tests: `tests_turn_effort.rs`, `tests_effort_beta.rs`, `tests_effort_sticky.rs`, `tests_checkpoint_effort.rs`.
  - **Which persona answers a message** (route.v5, theseus-c1uz, T10a; no store format: nothing new is stored; `engage.rs`,
    `turn/persona_engage.rs`, `judge/seal.rs`, `fact/persona.rs`). With two or more `[personas]` (`Config::persona_engaging`) **in a place where a message can engage another persona**
    (`TurnRunner::engages`, `Target.engages`: a DM, a channel, a thread; a cli, task or headless session's persona is fixed, so
    it carries only the persona in play and a change of it is a `system_changed` recompile: theseus-nsp9l.1, which took the
    owner's cli from 10,588 to 7,555 tokens), **every persona's files are in the persona layer and the header is the
    built-in default** whoever answers (`context_paths(persona, engaging)` loads all, `owned` is false), so a persona is not in the cached prefix at all: the engaged one is
    **one tail line**, `<Name> answers this turn`, attached after the cached blocks of the request's last message
    (`persona_line`, like the surface's standing records), so a switch changes no prefix byte. The decision
    (`engage::decide`, pure) is, in order: an explicit address (`engage::address`: a name then a comma or colon at the
    message's start, `@name`, `/as name`; confidence 1; Jev still asked), the owner's persona correction (`Steer::Persona`,
    how `corrected`) or the layer's entry close to the message (`learned`), Jev's `persona` answer when
    `[threads] persona_engage = "act"` and the judgment acts (at the pack's act bar silent, from confirm to act with the
    why shown, below that or late the prior), else the prior: who answered the session's last turn
    (`persona::Board::engaged`, in memory), else the session's own. A late verdict, or one carried from the last message,
    never changes a persona. The inbound point decides an address, a correction or a turn routing does not wait on
    (`engage_start`); otherwise `compile_first` decides beside the route decision (`engage_verdict`), so the wait is the
    route wait (`[routing] max_wait_ms`) the turn already makes, with no timer of its own. The persona and seal
    questions ride the one route request (the state adds the personas' cards, the speaker, and in a thread its title,
    goal and last three turns with who answered each; `PersonaCtx` in `judge/inbound.rs`): `tests_engage.rs` holds the
    call count. In a shared place only the place's own persona and those marked `shareable = true` are options. Each
    engaged turn is a `persona.engaged` row (how, p, runner-up, matched card terms, Jev's pick when it differs) and
    `TurnRoute.persona`, which every surface words as `RoutePersona::words` (CLI and TUI `render.rs`, Discord's footer,
    the cockpit's `routefooter.ts`). **Corrections teach it**: `route.correct { to: "persona:<key>" }` (`theseus judge
    correct persona:birch`, the cockpit footer's "answered by…", a reaction of the persona's `emoji` on Discord, "that
    was for Birch", `/as birch` alone) labels the turn's route judgment on the `persona` question, steers the next
    message at once and a close one through the layer (`nearest_persona`), retired with the pack version. "Birch, answer
    that" is an address: the turn runs in the same session with only the tail line changed. **Seal**: a `seal` yes at
    the confirm bar or above, in a thread, calls `Core::thread_recommend_seal` when `[threads] seal_recommend` is on
    (`judge/seal.rs`); Jev never seals. Known limits: the prior and the thread's answered-by history are in memory
    (lost at a restart; the prior then falls back to the session's own persona); the router's early inbound call
    (`rpc/thread_flow.rs`) asks the persona question without the thread's context.
  - **Sensitive names** (theseus-oejh3; `config/privacy.rs`, `data_gate.rs`, `turn/data_step.rs`, `pack_names.rs`): `[privacy]
    sensitive_names` is the one list, matched by `theseus_judge::names::hits` (case-insensitive, whole words, a name of
    several words as a run of them). It is the route pack's `data_names.private` (`PrivacyConfig::data_names`; `[routing.data]` is gone,
    theseus-9ymz0, and the old `private_topics` key is refused), a message that hits one makes the route skip every
    profile not of US origin for that turn (`route_profiles(true)`; none
    left: the `[model]` default, else the turn ends before any model call, class `data_gate`), and the provider chokepoint
    refuses a request to a model not of US origin that holds one in any string of its system, messages or tools, before a byte is
    sent (`data_gate::guard`, applied to every provider in `Core::build`, always). At start the
    embedded packs' text is checked against the list (`Core::check_pack_text_after_serving`, a `privacy.packs` startup phase
    naming pack and count, never a name). The origin rule is an allowlist (theseus-xs5ah, the owner: such data "must be routed
    to us-based models"): `ModelOrigin::cleared_for_sensitive` admits only `us`, and never a `restricted` origin (`cn`,
    `unknown`), so the China-origin block stays the floor beneath it; the route, the chokepoint and a private motif's
    model (`rsi::mint::data_rule`) all read it. Tests: `tests_data_gate.rs`, `data_gate.rs`, `tests_route_data.rs`.
    **route.v8** (theseus-f9qa8) is the acting route pack; its `data` question is asked and read by nothing since
    theseus-9ymz0 (the labels on a session replaced it; route.v9 drops it, theseus-gezlj).
  - **Content flags and the restricted-origin floor** (theseus-9ymz0, replacing the open view of theseus-a1wvq.3 to .17;
    `content.rs`, `content_wait.rs`, `data_gate.rs`, `turn/data_step.rs`, `config/content_based_security.rs`, `fact/data.rs`): every item that
    enters a session (an operator's message, a tool result, a brief, a report) is classified once, by itself, and what is
    found is a durable label on `SessionRecord.data` (`ContentFlags`; store format 52, which reads an older record's open-view
    state as no labels and nothing judged; format 53, theseus-gyjyo.3, adds `Label.judgment`, the raising judgment's id, so the owner's taps
    teach the classifier: `unlabel_session` writes a `judge.label` row (source `operator`, the flag's question `no`) in the removal's frame,
    the cockpit chip's "wrong" and `theseus judge label` write the same row without removing the label, and `learning/system.rs`'s
    `content` arm labels a label left standing a week, or present when its session retires, `yes` at weight 0.5). `secret`, `confidential` and `personal` raise at the confirm band and disqualify
    a model of restricted origin (`cn`, `unknown`) for the session; `injection` raises only at the act band, is never asked of the
    operator's own words, and freezes the session: `ToolRuntime::refusal_at` refuses every call that is not a local read
    of files and code (`content::local_read`) until the operator removes the label (`[content_based_security] injection = "notify"` only tells,
    the bench daemon's). Nothing is proved per request: the session carries a label or it does not.
    **The chokepoint** (`data_gate.rs`, keel): a request to a model of restricted origin is refused (class `data_gate`,
    nothing sent, one `data_gate.blocked` row with a `why` word) unless the session in `CALLER` (`for_session`) has no
    disqualifying label and every item in it was judged (`content_wait::unjudged`: an item after `judged_through`); a request
    for no session, or for one with no readable record, is refused (`no_session`, `no_record`). The session is read only for a
    restricted model; `[privacy] sensitive_names` is checked after it, unchanged and independent of every label.
    **The turn's floor** (`turn/data_step.rs`): `Floor { sensitive, cn_ok }`, `cn_ok` from `RouteState.cn` (`Cn`: allowed,
    pending, or no with the flag's word), read from the store, not the turn's copy. A turn bound for a restricted profile whose
    session has unjudged items waits on `TurnRunner::verdicts` for at most `[content_based_security] wait_ms` and then goes to the `[model]`
    default with `Reason::DataGate`: it never blocks and never fails for want of a verdict; a Claude turn never waits
    (`decide_floored` decides as if allowed, and waits only when that names a restricted profile). A later loop on a restricted
    profile reads the session again before it compiles (`cn_move`), so a label raised since moves it; a compaction's restricted
    summary profile waits the same way (`compaction/gate.rs`). `data_floor` moves any turn still bound for one, or fails it
    before any call ("No model cleared for this request's data is usable: …"); the driver parks that class, never retries it.
    `verdicts` is a `content_wait::Handle`: `Ready` (answers at once) until the judge that lands verdicts stands behind it;
    `content_wait::judging::Judging` is the tests' judge. The config refuses a restricted profile for the owner-material keys
    (`[people] extract_profile`, `[card] profile`, `[threads] import_profile`, `[judge.learn] writer_profile`, `[self]
    reviewer_profile`, `[initiative] profile`, `[ontology] name_profile`, `[memory] synth_profile`) and a China-origin `[judge]
    api_base`. Tests: `tests_content_gate.rs`, `tests_content_floor.rs`, `tests_content_freeze.rs`, `tests_data_gate.rs`,
    `data_gate.rs`, `config/data_floor.rs`.
    **The table is `[content_based_security]`** (theseus-gyjyo.2; it was `[content]`, whose keys are each a hard error naming their
    new home in `config/renamed.rs`): `session_flags` (was `enabled`), `injection`, `wait_ms`, `max_pieces`, and three keys **parsed and exposed
    only**, read by later rows of the principles migration: `cn_models` (`always | follow_flags | never`), `context` (handed to the pack
    verbatim), and `[privacy] open_source_paths` / `redact_outbound`. `session_flags = false` with `cn_models = "follow_flags"` refuses to start
    (`ContentBasedSecurityConfig::validate`): with nothing judged there are no labels to follow. Tests: `config/tests_content_based_security.rs`.
    **The data rule is on the record** (theseus-a1wvq.11; `tests_observability.rs`): the route row (`route.decided`) carries `cn`
    (`allowed`, `pending`, a flag's word, `unjudged`), `data_gate` only when the rule kept the turn off a restricted-origin model,
    and, always, `provider`, `model` and `origin` of the profile it runs on; `provider.call` carries `origin`. `data_gate` is an
    open word (a flag's word, `unjudged`, `sensitive`, or a later row's own); each one also counts on `theseus.route.data_gate`
    (`Telemetry::record_data_gate(what, why)`), with the chokepoint (`chokepoint`) and the four background refusals
    (`compaction`, `consolidation`, `keep_warm`, `replay`).
    Forks copy the labels their nodes raised and are judged as far as their source, through the fork point at most
    (`fork::data_through`). `turn.inspect`'s `data` line is `ContentFlags::line`.
  - **The Google private mark** (`google/private.rs`): the META mark a Google result writes; `scoped`/`bound` carry it to
    the floor; it follows recalled nodes and a task's parent; and text a marked session authored for others (a kept
    standing record, by its `capture:` source) is held back, listed `shown: false`, from a request to a restricted
    profile (`standing_step.rs`, `authored`), milestone reminders with it (`commitments.rs`), while a draft's view carries neither the task view
    nor the project slice. The mark never spreads to the readers. Tests:
    `tests_private_authored.rs`.
  - **Google calendars, the cache and the sensitive level** (theseus-jid7t.1; no store format change: a new META key
    and no new record kind; `config/google_calendars.rs`, `google/calendar_cache.rs`, `rpc/calendar.rs`, wire
    `theseus_protocol::calendar`). `[google] calendars` lists ids or `{ id, label, color, sensitive }`, validated at
    load (ids unique, `#rrggbb`, labels unique); unset or empty (the default, the owner's change of 2026-10-23), the
    cache reads every calendar of the account's `calendarList` (`Cache::settle`; the primary as `primary`, read again
    when older than 5 minutes, a newly listed calendar told as `calendar.changed`, `primary` alone until the list is
    read or when it cannot be). A label defaults to the calendar's own summary, a colour to Google's `backgroundColor`
    (then the palette). **Sensitive by default:** a calendar whose id or `dataOwner` is in a Google Workspace domain
    (`google_calendars::sensitive_by_domain`: any domain but gmail.com, googlemail.com and `*.calendar.google.com`)
    is sensitive; `[google] sensitive_domains`, when set, replaces that default. `google.call`'s Calendar reads are
    sensitive while any calendar read is, or while the list was never read with none configured
    (`Cache::any_sensitive`, fail closed). The cache
    (`Google::calendar_window`) keeps each calendar's events over the window asked for, widened a week each side, for
    5 minutes (at most 4 windows a calendar, 1,500 events a window; memory only, nothing about an event reaches the
    store, the ledger, a log or telemetry), then refreshes with `syncToken` (a 410 re-reads the window; a 403 asks
    `freebusy.query` for blocks with no title). Every call is `Google::call`, and its `google.called` row (method,
    status, the calendar's label) is handed back to the caller to write. `calendar.window {from_ms, to_ms}` answers
    the owner on a private connection alone (`owner_in_private`; a span over 62 days is refused) and is no model tool;
    `calendar.changed {labels}` (no content) goes to the connections that watch every session when a refresh finds a
    change, and the refresh tender (`Google::calendar_tend`: one 5-minute wait a round, not before the first window
    read; each round drops a window nobody read for 30 minutes, see **Idle and unreachable** below) ends when no window
    is kept, so a closed page ends it. `theseusd check` prints one line a calendar (`calendar_check`). **The sensitive
    level** (`private::Level`, `private::level`): a read of a calendar with `sensitive = true` puts `"sensitive": true`
    beside `"private": true` in the result's meta, and `private::record` adds the META record
    `google.sensitive.<session>` to the same frame; the level follows what the private mark follows (`followed`,
    `inherited`, a task's lineage) and never lowers. The task-local the chokepoint reads carries the level
    (`scoped`/`within`/`bound` take a bool as private); both floors (the route step and `data_gate`) allow a US-origin
    model alone for any private level, and `place.publish` refuses a node of a sensitive session to a shared place
    (`sensitive_stays_private`); a shared place is offered no calendar tool. A morning note over a sensitive calendar
    is asked of a US model alone and its `initiative.spoke` row holds no text (`initiative_post_as`). **Idle and
    unreachable** (theseus-b09do): the tender drops a window nobody read for `IDLE` (30 minutes) before it refreshes,
    so a closed page costs Google nothing and the tender ends; a sync delta is merged against its window (`overlaps`,
    the test `serve` answers by), so an event outside it is never kept or told, and a window cut at `MAX_EVENTS`
    drops its token; a 5xx, a rate limit or the network is `CalendarStatus::Unreachable`, a real 403 `Forbidden`. Tests:
    `google/tests_calendar_cache.rs`, `tests_sensitive.rs`, `tests_morning.rs`, `tests_calendar.rs`.
  - **The draft view** (theseus-a1wvq.7, narrowed to drafts by theseus-9ymz0; `draft_view.rs`): a draft is a model of restricted
    origin reading a session, so its requests keep the stripped prefix and a bounded tool reach that the open view gave every
    open request, and only drafts get them. `PlaceView.draft` / `TurnCtx.draft` (`DraftReach`: the draft's own tree as its
    roots, `draft_view::draft_roots`, never the owner's directory) makes `content_class()` the shared class (context files marked
    public, standing orders only, the walk, the session block only within the tree, no task view or project slice, the prefix
    key), while approvals, cards, asks and glides keep `class`. Its tools are `DraftReach::offers` (the public tools that start
    no later work, `open_public`: never `task.create`, `task.split`, `task.claim` or `wake.at`; the file tools under the tree;
    never a program), and the gate refuses the rest (`DraftReach::refusal`, never a path through `.git`); `task.update` and
    `task.close` stay and reach only the draft's own record (`toolrun::batch::own_record_only`). Recall draws on no session
    (`drawn_place`). A turn is a draft's when its session's META record says so (`draft::is_draft`, `TurnRunner::reach_of`),
    which a task's session alone can be. The open task (`task.create { open }`) and the open detour are gone: `open` is refused
    in words (`task::OPEN_RETIRED`). Guards: `tests_private_view.rs` (the private view's golden, byte for byte; a draft's request
    carries none of the owner's material; a draft's recall), `tests_draft_reach.rs`.
    A task's turn is private while its parent holds the Google mark, however late the parent took it
    (`private_session` reads every task above it on a task's turn alone, `google::private::lineage`; theseus-y8k3u); the learning writer and a thread
    import's card are scoped over every session they hold (`over_sessions`). Tests: `tests_private_background.rs`,
    `tests_private_lineage.rs`.
    Consolidation never runs on a restricted model (`restricted_origin`), keep-warm skips a restricted target, and
    `turn.replay` refuses one. Tests: `tests_bg_gate.rs`.
    **Drafts** (theseus-a1wvq.13; `draft.rs`, `turn/draft_step.rs`, `fact/draft.rs`; no store format change: a META record
    `draft.<session>` and the ledger kind `task.draft`; that record is what marks a draft's session, `draft::is_draft`): a
    China-origin model only drafts code, in its own tree, with no programs, and a Claude check always reviews it; nothing is
    applied or posted before that review. A coding class's
    `[routing.classes.<class>] cn_draft` (a restricted profile; `routine_coding` and `deep_coding` only) and `final` (its
    Claude reviewers, first usable wins; required with it) make an ask of a conversation whose labels do not keep a restricted
    model off (no task, no wake, its `dir` in a git repository: the tree is the whole repository, and it is applied
    at that top; the ask is the turn's own message) open a draft instead of a model call: a task session on the `cn_draft` profile in a
    worktree at `<drafts_dir>/<short>` on branch `draft/<short>` (`[server] drafts_dir`, default `~/.cache/theseus/drafts`: outside the state dir and every approve-list entry, refused at load otherwise, and a root of the workspace while a class drafts, so the review's runs there take the tool's posture; a draft made before keeps its tree under `<state>/drafts` where `tree_of` finds it; theseus-iilre), which is its roots alone (never the owner's directory,
    and never a path through `.git`), briefed by the core with the turn's own message. Each failed condition is the route row's
    `draft_skip` (`labelled`, `in_task`, `wake`, `not_git`, `draft_unusable`, `no_reviewer`). `DraftReach::offers` withholds programs from a draft, and where the floor would move it its turn ends
    (`draft_stopped`: `draft_not_admitted` for a brief whose session the labels did not allow a restricted model, `draft_stopped` for a label that landed after). Its report is never posted (`TaskOf::target_for`); its end opens the review (`draft_ended`: a
    check on a `final` profile in the tree, the draft's claim, a correlation id from the draft's), whose end commits,
    applies `git diff <base>..<final>` with `git apply --check` first and never `--index` or `--3way`, removes the
    tree, and writes the line and the `task.draft` row. `first_pass` (a restricted profile, needs `cn_draft`;
    theseus-a1wvq.17) opens a second draft-kind task on the draft's diff before the review (`draft/first_pass.rs`): its
    report is in the review's brief and nowhere else, it is bounded at 10 minutes, and one that cannot open lets the
    review open without it, its tree removed. The core's git in a draft's tree is pinned to the git dir recorded at open,
    with fsmonitor, external diff and textconv off; `sweep_drafts` settles drafts a crash or cancel left (at start and
    after each cancel); one step of a draft runs at a time (`draft/flight.rs`; theseus-b0ozi). Tests: `tests_draft.rs`,
    `tests_draft_first_pass.rs` (a `first_pass` equal to the `cn_draft` is refused: the drafting model would review its own draft; the bound's timer path, theseus-ms3hd), `tests_draft_hardening.rs`, `tests_draft_home.rs`.
  - **Privacy warnings** (theseus-gasov; `routing/personal.rs`, `turn/personal_step.rs`, `fact/privacy.rs`): route.v7 asks
    `personal` of every message (`none`, `elicit`: it tries to draw out the operator's personal details; `expose`: it asks to
    open a conversation with others where they could come out). The owner's personal details live in the context by design
    (the owner, 2026-10-18); the guard is a warning at the exit. This turn's own answer at the act band, in a shared place from
    the confirm band (`PersonalAnswer::warns`) posts one `privacy_warning` to the owner's DM alone (`stage_to_operator(None, …)`; the Discord
    courier sends it with Jev's notices, never to the place) quoting the message's first 200 characters, scrubbed, each line quoted and each link fenced, at most one post per
    place in 10 minutes (`may_post`; the rest `folded`), and writes `privacy.warned` (choice, confidence, place class,
    judgment, posted, folded, held). A late verdict, kept for the next message, warns nothing and holds nothing (a known gap).
    In a place only the owner writes in, nothing is held: the turn and its reply go on. **In a shared place the reply is held**
    (theseus-i1z13, the owner's 2026-10-18 20:46 spec): `warn` sets `RouteState::held`, and the turn loop, once the first compile
    returns, ends the turn with `hold_reply` and no model call: `I don't share that here.` (`HELD_REPLY`) is the turn's assistant
    node and output (so the next turn's history reads it), the stop reason is `held` (theseus-2rvwv: the footers say the reply was held and name no model),
    and the owner's DM carries the case: who asked (`RouteState::asker`, the inbound author), where (the place's bound name),
    the message and why. The hold is not folded by the one-post window, only the DM is: a second reach in it is held and not
    posted again. A task's turn is warned about, never held. Read beside `engage_verdict` in the route step, so a message
    that steered a running turn is not read. A compile that fails (`context_overage`) in a shared place still reads the verdict and is held, not failed (theseus-cljlz); in a private place or for a task it fails as before, and a private place's warning still posts. Tests: `tests_personal_hold.rs`, `tests_personal_warn.rs`, `turn/personal_step.rs`.
  - **A home past its size** (theseus-jg1eg, the follow-up of theseus-vydhq; `turn/home_size_step.rs`): a place's own session (`Outbox::place_session`,
    never a task's or a thread's) whose first-loop request (`Compiled::est_tokens`) passes `[threads] home_warn_percent` (50) of the
    serving model's context window (`Turn.fill.window`, the catalog's or `[context] working_window_tokens`'s; 1,000,000 when none is
    known; theseus-d9je7, the owner, 2026-10-21: long sessions are fine, so no smaller figure warns, and `working_tokens` is a
    thread's cold-resume bound, not this) posts one `home_size` to the owner's DM alone (`stage_to_operator(None, …)`; the Discord
    courier sends it with Jev's notices): the place's name as the owner sees it, the size and its share of the window, and a dry-run
    `theseus thread split home@<stored place> earlier-<date> --first @A --last @B --dry-run` over the home's oldest settled stretch
    (its first unmoved turn through the turn before the newest `[threads] resume_keep_turns`; none when no turn is left, then
    `theseus history` only), with `thread unsplit` and the id the split prints. Nothing splits itself: a split moves context. Once is
    the META row `home.size.warned.<session>` (`warned`, written in the post's frame, so a restart keeps it and a successor home
    starts unwarned; `threshold` the computed bound); a request back at or under the bound writes it `warned: false`, so the home is
    told again if it grows back. With no owner DM bound nothing is posted or marked. Tests: `tests_home_size.rs`,
    `turn/home_size_step.rs`, `config/threads.rs`.
  - **The owner's corrections** (theseus-q31l; `correction/`, `turn/route_step/correct.rs`,
    `rpc/route_correct.rs`): words (`correction::words`, deterministic and narrow, read by `turn.submit`
    unless a job sent it, and taken at the inbound point only where the owner alone writes, the CLI, the web
    UI or an owner's DM, never a guild channel bound private and never a task's turn; after a bare verb a lone
    adjective is a remark, not a name: `named_plainly`), or `route.correct` (a reaction, a press, `theseus judge
    correct`: `judge_act(Act::RouteCorrect)`).
    Each writes the owner's label (`not:<mode>` on `mode`, only when the profile named is not one the
    verdict's mode leads to) and a `route.corrected` row (scoped `route.corrections`, its provenance by
    id) in one frame: the turn's own for words. The turn steers ahead of the verdict (`Steering`,
    `routing::steer_to`, `Reason::Correction`, `route.decided`'s `source`/`follows`) and waits for none.
    The layer (`correction::layer`) is in memory, bounded, rebuilt after serving (`warm_corrections`),
    and retires when another route pack version acts; the route pack is in `lineage::LEFT_ALONE`, so
    no nightly rewrite folds it today. Tests: `tests_route_correct.rs`, `correction::*::tests`.
  - **Replay, audit, and backfill** (step 25d, theseus-0j2.14; the owner's runs, each `judge_act(Act::JudgeRun)`
    and in the CLI's `OPERATORS`, each on a `learning` thread at nice 19, routed together by `rpc/judge_runs.rs`).
    `learning/replay.rs` (`Core::judge_replay`, public for the learning loop): a candidate the build does not wire
    over a report's frozen holdout, its train split, the incumbent's errors, or ids; stored states when builder,
    version and cap match, else `learning/rebuild.rs` (only `loop.v1`'s input: the turn's `turn.ended` row and its
    nodes through `loop_end::input`; `unrebuildable` says why for the rest); thresholds-only re-bands, no call. Its
    calls and run are scoped `judge.replay:<pack id>`, which the report never reads; `[judge] replay_limit_usd`.
    `learning/audit.rs`: a profile's answers as audit labels (weight 0.5), capped by `audit_limit_usd`.
    `learning/backfill.rs`: under `backfill_consent` only; one judgment per event (keyed by it), `event_at_ms`, which
    `read_scope` takes as the judgment's time. Tests: `tests_replay.rs`, `tests_audit.rs`, `tests_backfill.rs`.
  - **The prove** (L3, roadmap row 50, theseus-0j2.18): `learning/prove.rs` builds `theseus_judge::prove`'s records,
    one per `task.ended`, from rows the ledger holds (`build` is pure; `Core::prove_input` reads pages by kind and
    session), every outcome from the labels as `labels::resolve` settles them, never a rule derived again;
    `classify_quality` is classification's half. `judge.prove` (`rpc/judge_prove.rs`, a read: its window reads the
    `pack.mode` rows from their scope, since the ladder's first load writes) runs the generator over them; `theseus
    judge prove` prints its Markdown byte for byte as `theseus-judge prove` does. Tests: `tests_prove.rs`.
  - **The ladder** (step 26a, `judge/ladder/`): each pack version's mode as `pack.mode` rows scoped per pack id
    (`pack:<id>`, a few rows), read once after serving (`Core::warm_ladder`, `JudgeService::read_ladder`), or by an
    RPC (`pack.*`, `judge.label`) or the nightly check, then kept (`Ladder`; with no row, `WIRED`'s line). No point
    reads it (theseus-289c): before the warm read, `Ladder::given` answers `Ladder::unread`, the wired line under
    the config with a pack that would act in shadow (a row may have rolled it back; never act on an unread ladder),
    `placed` the root, health's lines say so (`… shadow (until the ladder is read; wired live)`), and route_base
    keeps a session's move for the read ladder. The warm read writes the adoptions in one frame, a quiet stretch
    after serving and between turns (`memory_pass::turns`). A new local day starts empty, reading nothing: every
    event since midnight landed through `land`, and the notices' brake reloads the ladder. Core rigs whose turns are
    judged do the warm read at build (`tests_judge::warm`: `tests_judge`'s rigs, `tests_route`, `tests_rerank`,
    `tests_notices`, `tests_security`); `tests_ladder_unread.rs` holds the rule. Every point asks
    `JudgeService::mode_for(pack, session)` (or `ask_mode`, which records `pack_arm` in the judgment's context): the
    ladder's rung under `JudgeConfig::mode_of`'s ceiling, a canary acting in its `learn::arm` and the control in
    shadow, `rolled_back` as shadow. A ceiling at shadow needs no read. Rollback (`rules.rs`): each event a rule
    counts is a `pack.event` row scoped `pack.event:<id>:<day>`, so a restart reads the day back; `learn::check_all`
    runs as each lands (`JudgeService::land`; `judge.label` lands a label's) and again after the nightly report; a
    rule from the adoption table (`adopt.rs`: `route.v1`, `rerank.v1`, `security.v3`, live before the ladder,
    adopted once as the owner's, and `route.v2` and `route.v3`, each adopted with its own reason, theseus-3okf and
    theseus-qe3v) is a day's brake (`until` the next local midnight, folded away after it), any other
    stands until a promotion. `rpc/packs.rs` is `pack.list`, `pack.promote` and `pack.rollback`
    (`judge_act(Act::Ladder)`; short of `promote::bar` the owner's row is `forced`, the system's refused); a
    `security.*` promotion is a card, its question planned on the ladder's own session (META `ladder.session`) and
    answered by `action.confirm` like an extension's ack (`answer_promotion`: the bind or decline and the row in one
    frame, nothing wakes). A promotion's `said` names a learned version standing ahead of the one moved, where the
    move judges nowhere or only in a canary's control arm, and its rollback (`rpc/packs_ahead.rs`, theseus-nwa5):
    said, never refused. Tests: `tests_ladder.rs`, `judge::ladder::tests`.
  - **The learning loop** (step 25f, theseus-0j2.12; design §2.17): `learning/propose.rs` (the run: nightly after
    the report, and `judge.learn`, the owner's act), with its pure parts in `theseus_judge::propose` (names from
    v101, the interleaved split below 200 labeled in the window, the text-only check, the threshold re-fit, the
    decision, the writer's prompt). Learned versions are `pack.version` rows scoped `judge.learn:<id>` beside the
    `judge.proposal` rows, read by `judge/lineage.rs` (`JudgeService::pack`, `root_of`, `placed`) by the warm read
    after serving, or by `placed_read` (the learning loop, `pack.list`), never by a point, their files
    `<state>/packs/` derived. Every point asks `placed(root, session)` for the version standing in its root's place,
    then `mode_for` that version (capped by the root's config line). A move is `Core::promote_learned` (26a's act,
    citing the proposal). The writer's output is capped (`WRITER_MAX_TOKENS`): the profile's own cap reserves past
    the day's limit. Tests: `tests_learn_loop.rs`, `theseus_judge::propose::tests`.
- **A fresh install's defaults** (theseus-is8t): `[memory] mode` defaults to `live` on the `baseline` arm (the memory
  is what sets Theseus apart, and the arm needs no Jev; with no index a recall answers nothing, `unavailable`, and costs
  nothing; a shared place still draws on its own sessions alone), and `[judge] enabled`, left out, follows the Jev key
  (`JudgeConfig::resolve`, run as the config loads: `enabled_setting` is the config's word, `enabled` what acts,
  `auto_why` the reason; neither of the last two is in a note, so a config that says nothing stays sparse). Auto is on
  when `key_secret` names an entry that can resolve: an `env:` variable that is set, a `file:` that reads, or an
  `op://` reference that is not a placeholder (the vault is never asked at the start: a reference it cannot answer
  leaves the judge on with its key unresolved, as `enabled = true` does); off with no entry, an unset variable, an
  unreadable file or a placeholder. `enabled = false` is off whatever the key, `true` with no entry still refuses to
  load. `features.rs` words health's first line (`HealthResult.features`: memory, Jev with routing live or shadow or
  off and why, the scrub, the daily ceiling), which `theseusd check` prints too. Memory on also means the memory pass
  after each turn and the nightly consolidation (`synth_limit_usd_per_day`, read only by `+synthesis`). Tests:
  `config/tests_fresh.rs`, `tests_fresh_recall.rs`, `features::tests`. A test that counts Jev's calls, frames or
  transcript reads sets `memory.mode = Off` (the judge rigs do), and a daemon test's `safe_note` sets
  `[judge] enabled = false`, so no test daemon turns the judge on from a stand-in key.
- **Recall** (M6 step 30a, in shadow): `recall.rs` (`Memory`: `[memory]`, the science, and who answers the index's
  query, the tender or a test's stand-in, `Memory::set_ask`; the manifest; `TurnRunner::place_of`, the place rule
  read as `class_of` reads it), `turn/recall_step.rs` (begun as the first loop's model call goes out, read once it
  answers, never past `[memory] recall_deadline_ms`: a stalled index costs a turn at most the deadline past its
  call), `fact/recall.rs` (the `recall.shadow` row, scoped `recall:<session>`, with references and the query's
  digest, never copies; the `recall` span; the narrative line), `rpc/memory.rs` (`memory.search`, which writes
  nothing, and `memory.recalls`), and `config/memory.rs`. In shadow the model's request is the one compiled without
  recall, and the row rides in the turn's next frame. The filters and pack are `theseus_memory::recall`. Tests:
  `tests_recall.rs`.
  - **Words in time** (theseus-w9qv): a query that asks for vectors goes out twice at once (`recall::race`): as
    asked, and for its word sources alone, which answer in a few ms while the query's embedding may take hundreds.
    The whole answer wins whenever it arrives within the deadline; past it (or when the vector search fails), the
    words' hits rank and pack as usual, the row's `outcome` is `words_only` (`recall::WORDS_ONLY`) with `why` and
    `skipped.vector`. Never past the deadline. Health's memory block counts the last 50 recalls by outcome and the
    last whole answer's time (`recall/outcomes.rs`, in memory since the start; `MemoryHealth.recalls`), and again
    by what the answer held (theseus-y9a3, `RecallManifest::answered`, read from the row: `vectors`, `words`
    alone, or `empty`; a `ran` whose vector source the model's load skipped held words alone), and
    `theseus.recall.index_ms` times each recall's wait by outcome. Tests: `tests_recall_words.rs`,
    `tests_recall_deadline.rs`. The words answer in time only if the tender answers queries at the daemon's
    priority, not its background one (`theseus-index`'s `priority.rs`).
    Since theseus-zo1y the vector source embeds the turn's new text alone (`RecallQuery.vector`, from `query_of`),
    a short turn's (`SHORT_WORDS`, 3 words or fewer: "yes") with the previous reply's last lines after it
    (`short_context`, 120 characters), so it is not near noise;
    cut by the tender at `[memory] recall_vector_tokens` (32) word pieces, while the words read the longer query; the
    two queries go on one connection, the words' first (`tender/pair.rs`, `IndexTender::query_two`), which closes
    once no one waits (the deadline passed, or the turn dropped its `Begun`, which aborts its task), so the tender
    stops embedding. Tests: `tests_recall_deadline.rs` (a stand-in socket that counts connections).
  - **Questions about time** (theseus-q1tbn; `recall/temporal.rs`, `recall/stamp.rs`): a turn whose new words ask
    "when did / was / is", "what day / date / time did", "how long ago / since", "the first / last time", "the most
    recent / earliest", or "before / after <event>" is read by a small deterministic reader (`temporal::read`, a
    `Kind`: `When`, `Order`, `Duration`, `Latest`, `Earliest`; no model call; a time that is only the subject, "what
    time zone do I use", is left alone, the table is `recall/tests_time.rs`). Its words go out as the event alone
    (`temporal::subject`: the temporal words match every dated note), and the pack is ordered by the event's time
    (`temporal::order`, applied in `Memory::manifest_ranked` through `rerank::eligible`/`repack`, so the filters and
    the place rule run first): of the first 12 matches, notes that state the event (a time expression, or a change
    verb) go before notes that only mention it, each tier oldest first for `When` and `Earliest`, newest first for
    `Latest`, both ends first for `Order` and `Duration`. The manifest's `temporal` names the kind. A live rerank
    (32d) orders by Jev, over this. A header's date is the note's own, in the daemon's zone with the weekday and zone
    (`said Tue 2026-03-17 19:40 MST`), and where the shown text names a relative or partial time, the date it resolves
    to against the note's own day (`; "yesterday" = Mon 2026-03-16`, `stamp::header_time_in`, which never reads the
    clock); headers written before this one keep their UTC bytes. Tests: `recall/tests_time.rs`,
    `tests_recall_time.rs`, `recall::render::tests`; the exam's `probe --temporal` applies the same reader and order.
  - **The subject beside a same-kind value** (theseus-96cyh; `recall/neighbour.rs`): a recalled chunk about another
    subject of the type the question asked after (another host, path or crate than the one it names) says whose it is,
    `about host db-stage.example.test, not the asked host db-prod.example.test`, so the model cannot read the
    neighbour's value as the asked one. The rule is the subject bar's (`theseus_memory::Floor::other_subject`), but
    computed in core from the candidates alone, so it holds with the floor and the bar off (both default 0.0): the
    asked entities are those the question named that some candidate holds (`marks`), and a chunk holding none of them
    and one of an asked one's type gets the note (`note`). `apply` puts it on `RecallItem.neighbour` at both of
    recall's fill sites; `turn/recall_step.rs::hold_node` appends it to the end of the item's rendered header
    (`with_note`) *before* the handle, thread and persona marks, so it is frozen in the Recall node's header and every
    later request, a compaction's recall section included, reads the same bytes. An asked subject's chunk, and any
    chunk of a question that named no entity, keeps its header byte for byte. The field is `serde(skip)` and `ts(skip)`:
    in memory only, never on the wire or in the store (no format change; `memory.search` clients never see it). No
    config key switches it off. Tests: `recall::neighbour::tests` (the rule), `tests_recall_neighbour.rs` (the header
    on the node, whole-core).
  - **Current-value questions** (theseus-mo1vm; `recall/current.rs`, `recall/tests_current.rs`): a "now / currently /
    still" question that asks for a value ("how many days of X do we keep now?", "which port is Y on now?", "do we
    still keep 30 days?") is `temporal::Kind::Current`; `temporal::read` hands a question it reads no event in to
    `current::read`, and a "now" question that is an event, or asks for no value ("what are we doing now?", "what day
    is it now?", "what time does the backup run now?"), is read as before. Notes about such a subject repeat every
    value it had, so the pack goes by `current::order`: the hits that state the subject's value (half the
    question's content words, rounded up, and a number) first, newest first (a `supersedes` link among them wins over time; the
    exam's probe has none), the rest after in relevance order. An older statement whose values do not include the
    newest's carries `, superseded: older than the <day> statement` at the end of its frozen header
    (`RecallItem.superseded`, `#[serde(skip)]`, set by `current::mark` after `fill` and read by `hold_node`'s
    `current::with_mark`); nothing else changes, so a pack with no such question renders byte for byte as before. A
    live rerank (`refill`) keeps the marks: it reads them off the manifest's items before the repack
    (`current::marked`) and sets them again on the same keys after the fill (`current::apply`), as it does the
    neighbour note; a statement first admitted by Jev's order goes unmarked. A date or a timestamp in a note's text
    (`2026-03-02`, `2026-03-02T08:00:00Z`) is not a value of it, nor any part of one (`current::values` skips the
    whole run). Tests: `recall/tests_current.rs`, `tests_recall_current.rs` (the marks through a live rerank).
  - **`memory.lookup`, the model's own read of memory** (theseus-w9qv; `memory_lookup.rs`, `memory_lookup/read.rs`):
    words through recall's pipeline, a span or a book or a topic through the import catalog, and since theseus-8c8s
    `session` (one session's messages in order, paged, tool calls and outside text left out and counted; with words, a
    search of that session alone) and a span's listing of the harness's own sessions before the imported episodes.
    Read, Async, SafeToRepeat; private places only, checked by the gate and again by the tool; personal and
    partner-confidential text veiled. Its description names the CLI commands it replaces and when to use it: the
    owner's DM once answered a question about a past month with four `proc_run`s of `theseus memory index search` while
    this tool was offered (the context held the CLI from before the tool existed). A `proc_run` of the daemon's own
    CLI for what it answers ends with a `[native: memory_lookup {…} …]` line, after the cap (`native.rs`,
    `ResultNode.hint`); the CLI still runs, since running it may be the work. Tests: `tests_memory_lookup.rs`,
    `native::tests`.
  - **The MCP server's memory tools** (theseus-02i1h; `memory_lookup/mcp.rs`, `rpc/memory_read.rs`): `memory_search` and
    `memory_get` are one read-only method, `memory.lookup`, in `mcp_server::ALLOWED`, served by `memory_lookup`'s own
    `by_words` (no second pipeline), with `private_only` dropping each hit whose session `place_of` does not read as
    private (a private asker may draw on a shared place's sessions; these tools never show one). Only `Surface::Mcp`,
    and only while `[mcp_server] memory` is on (default off). A hit is typed (`MemoryHit`: citation = node id, WAL
    position, session, as-of minute; origin, harness, sensitivity; veiled text is never in it). Each read is a
    `memory.read` row (`fact::memory::MemoryRead`: principal, tool, client, a digest of the words, hits). Tests:
    `tests_mcp_memory.rs`; the tools' golden is theseus-mcp's `tests/golden/memory_tools.json`.
  - **In front of the model** (step 30b, theseus-6fn.2): `[memory] mode = "canary"` (a sticky share,
    `canary_fraction`, by a hash of session and `experiment`: `MemoryConfig::assign`, recorded once as a `memory.arm`
    row; the control runs `none` live with `baseline` in shadow) or `"live"`. The read finishes before the first
    loop's compile, under the same deadline (`recall_first`), and what it admits is a `Recall` node
    (`node::Body::Recall`: references, never copies) held in the turn (`Turn.recall`) and rendered by the compile as
    though written (`recall_view`, at the position after every node); the new compilation's prefix leaves it out
    (`recall_compiled`), so it renders in the tail as it will once written. It rides the provider call's plan frame
    with a `derived_from` edge to each source (`via = "recall"`, so `node.reach` counts the copy), and its
    `recall.ran` row the turn's next frame: no frame of its own. Its render (`recall/render.rs`) is §2.4's testimony,
    under a preamble that names the harness as its source and no part of the person's message (it renders in their
    user turn, after their words: theseus-fpm2), each item's frozen header and its source's text over the frozen byte
    range, read by position (cached: sources never change), so the next request begins with the previous one's bytes.
    Past `session_recall_cap_tokens` (64,000) of notes in the tail, recall pauses (`paused`) until the next recompile.
    **The pack is sized to the prompt** (theseus-d9je7; `turn/tier.rs`): the route pack's mode folds into a tier,
    `trivial` small, `sophisticated`/`deep_coding`/`review` deep, the rest ordinary, and so is a verdict late, missing,
    under its bar or not acting, and a turn routing does not decide. Ordinary packs `recall_budget_tokens`/
    `recall_max_items` (3,000 / 8), deep `deep_recall_tokens`/`deep_recall_items` (8,000 / 16); small packs as
    ordinary and its detour drops it. The pack never waits: `recall_live` reads the verdict once the index has
    answered (`TurnRunner::injection`, a peek that puts a landed answer back for the compile's own wait), else
    packs ordinary, and the budget it took is fixed on `RouteState.injection` and the `route.decided` row's
    `injection` (`tier`, `fallback`: `verdict late` and the like, the sizes, the admit multiple), which `theseus why`
    says in one line. The sizes ride `Scene.budget_tokens`/`max_items`, so a live rerank's refill packs the same.
    The tier touches only the tail: no cached prefix byte moves (`tests_injection_tiers.rs`). Deep's keys at
    ordinary's numbers are the rollback.
    **The narrow-read hint** (theseus-tdkcn; `turn/tier.rs`, `toolrun/session_dir/read_window.rs`): an acting `deep_coding`
    verdict (`InjectionBudget.coding`; not `review`, `sophisticated`, an unsure or late verdict, or a turn routing does not
    decide) puts `CODING_HINT` in the request's tail (`coding_hint`, beside the persona line: map with `fs_grep`, read
    100-300 line slices, write down findings), and a bare `fs_read` returns 400 lines (`ToolCtx.default_read_lines`, set
    per session at the turn's budget and cleared by any other turn's); the schema and the footer are unchanged.
    Tests: `tests_coding_hint.rs`, `tests_narrow_read.rs` (theseus-tools).
    A trivial detour's request carries no recall (theseus-n7nc): while routing has the route to decide
    (`RouteState::deciding`) the row waits for it, and once it is known `recall_routed` drops a detour's node, its
    rides and its footer count, and the row says `detoured`. A quick detour's request carries it (theseus-pa1xx,
    `config::routing::detour_recalls`): a quick question is often about what was said or decided before, so
    `compile_detour` reads through `recall_view` and keeps the turn's own node on its later loops, and the node, its
    footer and its row are a kept compile's. Tests: `tests_route.rs`.
    `memory.label` (`rpc/memory.rs`, `judge_act(Act::Label)`, refused by the CLI inside a job) writes a `memory.label`
    row scoped `memory`; `wrong` and `stale` drop a node as `labeled_wrong` from a set built after serving
    (`Core::warm_labels`, `recall/labels.rs`).
    Every compilation carries a `BudgetReport` (`Compiled.budget`, stored on a new `Compilation`; the ring's cut as a
    range, a recall's budget drops, an overage); the reply's `TurnSubmitResult.recalled` feeds Discord's
    `🧠 N recalled` footer. Tests: `tests_recall_node.rs`, `recall::render::tests`.
  - **The `+rerank` arm in shadow** (step 32c): `judge/rerank.rs`. After the shadow recall's pipeline
    (`recall_end`: `[memory] mode = "shadow"`, or a canary's control), the recall step's one call,
    `JudgeService::at_recall`, picks the candidates that passed every filter (`theseus_memory::rerank::eligible`,
    the place rule first, and the operator's labels: only they may reach Jev), marks the turn's trace (`judge`,
    kind `mark`, with the judgment's id), and spawns `rerank.v1` under its own 600 ms deadline (a live-urgency
    call): the reorder, the repack, and a `judge.call` row (scoped `judge:rerank`, context `purpose: "recall"`,
    and `rerank`: the recall's id, both orders' admitted keys, `changed`, `fallback`, the latency against the
    deadline, the cost). The day's limit sends nothing and writes no rerank row. Tests: `tests_rerank.rs` (the
    place test over generated stores reads what the fake Jev was sent).
  - **Rerank live, bounded** (step 32d, theseus-6fn.7): `WIRED` gives `rerank.v1` `Live`. The arms rule: memory's
    mode decides what reaches the model, rerank's whether Jev orders it. A recall in front of the model goes through
    one call, `TurnRunner::recall_reranked` (`turn/rerank_step.rs`; the mode is read before the candidates are
    cloned): live, `JudgeService::at_recall_live` dispatches as `at_recall` does and the turn waits at most
    `[memory] rerank_wait_ms` (200; 1 to 600) from the rerank's start, on a oneshot the turn closes when the wait
    ends (`try_recv` takes what was sent before, so `applied` and `late` never both hold); in time and answered,
    `Memory::refill` packs again in Jev's order. Rerank's breaker open, the day's budget paused, or Jev known
    unreachable (theseus-otny) is read before the dispatch and not waited on. A `judge` span of kind `wait`, the manifest's `rerank`, and the row's
    `live`/`applied`/`late` say what happened. Rerank has a breaker of its own (`rerank::BREAKER`,
    `JevJudge::with_breaker`): its outcomes move only it; the client, its permits and its shed count stay shared,
    and `breaker_status()` is the shared one's. Per-item answers (`helps.3`, `about` the note's key) take labels of
    their own (`learning/items.rs`: the check, the report's per-definition questions), and the owner's memory labels
    write rerank's system labels (`learning/rerank.rs`). Tests: `tests_rerank_live.rs` (one on tokio's paused clock,
    with Jev a channel: `JudgeService::rerank_with`), `tests_rerank_labels.rs`.
  - **FSRS-6 retention and the `+retention` arm** (step 32a's wire-in, theseus-6fn.11): `recall/retention.rs`. Each
    memory row is the `AccessEvent` it is (`event_of`, pure: `memory.labeled` a first sight by durability,
    `memory.used` a use by outcome or, not used, `Shown`, which is no review; `memory.label` by §2.7's table), at
    its row's time. The `Projection` folds them with `Fsrs6` (`fsrs6-default`) per node from every arm's rows (one
    projection, §5 question 9), each node's events kept by position, so a row brought twice counts once and one
    arriving out of order refolds the node: it always equals a rebuild. Built after serving only when read
    (`Core::warm_retention` under `[memory] arm = "+retention"`, the first `+retention` search, or a turn of the
    arm), by one walk of the three kinds through the ledger's index (`k:<kind>` tags; waits on tokio's timer while
    the index's shape is built), on the blocking pool; kept current by `Memory::retention_written`, which the memory
    pass's frames and `memory.label` call with their positions. The arms' seam is `Memory::science_for(arm)` (a
    match: an arm that ranks its own way adds its line) and `Scene.science`, which `manifest_ranked`, `refill` and
    the rerank's `Recalled` (with its `retention`) read; shadow, a canary's control and a search without an arm use
    `baseline`'s. Under a science that reads retention the manifest's `retention` says the projection's state
    (`ready`, or why it ranked without), and each admitted and dropped item its `RecallRetention`. `memory.search`
    takes `arm`; health's `memory` block and the gauge `theseus.memory.retention.nodes` give its size. Tests:
    `tests_retention.rs`, `recall::retention::tests`.
  - **The adjacency projection** (step 32b, theseus-6fn.12): `recall/adjacency.rs`, what spreading activation walks,
    folded from the record and never stored: neighbours by position in a session (a `Recall` node is no one's
    neighbour), a tool call and its result by `tool_use_id`, the EDGEs by kind and route (`mapped`: `derived_from`'s
    copying routes, `recall`'s at weight zero by construction, a route it does not know, such as 31b's `synthesis`,
    spreads nothing and is counted `unmapped`; `supersedes` 1.0 toward the newer node, 0.2 back), and each node's
    entities from its `memory.labeled` row, expanded at a spread from each entity's list, never stored per pair
    (`1/ln(1+df)`; an entity in more nodes than `cap` is not expanded: at the defaults, 1,095, past which one such
    edge cannot carry a seed over the threshold alone). A node the memory pass never labeled has no entities.
    `Projection::refresh` folds what was written since; the result does not depend on where a fold stopped. Tests:
    `tests_activation.rs`.
  - **The `+activation` arm** (step 32b): `recall/activation.rs`. `[memory] arm = "+activation"` (canary or live;
    shadow and a canary's control run `baseline`) asks the index for `baseline`'s sources, then, before the pipeline,
    spreads (`Memory::activated`, theseus-memory's `Activated` science, which is `baseline` but for `activate`): the
    turn's new node at 1.0 (`Begun.new_node`; its projection edges and the query's entities the hits matched, the
    tender asked nothing more) and the top 10 fused hits over the best one's score. The reached nodes are one more
    ranked source: a reached hit gains `weight / (60 + rank)` and `sources.activation`, and at most 20 of the
    strongest the index did not return (none the turn holds, none at or after `as_of`, none the index never indexes)
    join the candidates read from the store, before every filter (the place rule reads them as any candidate). The
    spread runs on the blocking pool for what is left of the index's deadline (`deadline`); the projection is built
    after serving (`Core::warm_activation`), and a turn that finds it unbuilt starts the build and goes on
    (`building`); a search builds it itself, unpaced (`Adjacent::paces` counts every build's paces), unless the warm
    build is running: then it answers `building` at once, never queued on the projection's lock behind the warm
    build's paces (theseus-e21m, theseus-6fn.14; `tests_activation_search.rs`). The seam other arms share: `MemoryArm`, `Memory::science_for(arm)`, the
    `Scene`'s `science` (read by `manifest_ranked`, `refill` and the rerank's `Recalled`), and `memory.search`'s
    `arm` (`theseus memory search --arm`). The manifest's `activation` says what it did and its share of what was
    admitted (each item's rank and score are its `sources.activation`); the `recall` span holds a `recall.activate`
    span, timed in `theseus.recall.activate_ms` by outcome, with its additions in `theseus.recall.activated`
    (`telemetry/tests_recall.rs`); health's `memory` block (`Core::memory_health`) names the projection's nodes,
    edges, entities and bytes. Tests: `tests_activation_arm.rs`.
  - **Consolidation and the `+synthesis` arm** (step 31b, theseus-6fn.10): `consolidate/`. Off every turn
    (`memory.consolidate`, routed with the owner's runs in `rpc/judge_runs.rs`, `judge_act(Act::JudgeRun)`; the
    nightly tender `consolidate/tender.rs` at `[memory] consolidate_hour`, never within 10 minutes of a start;
    the plan on a `learning` thread at nice 19, the calls and the frames' waits as tasks on the runtime, never a
    thread blocked on it: a stop mid-wait must not panic it, and the release profile aborts on a panic), the newest `recall.shadow`/`recall.ran` rows (by kind through the
    store's pages) become clusters (`theseus_memory::consolidate`), each written by `synth_profile` (`session`:
    the profile every source's session last used; disagreeing sources wait), checked deterministically and by
    `citation.v1` (`judge/citation.rs`, a point of its own, `WIRED` in shadow) on the answer's entry
    (`theseus_memory::consolidate::entry`, a leading heading set aside: the node keeps the entry,
    `synthesis.proposed` the answer whole, `synthesis.checked` the `heading`; a cluster whose one answer was rejected
    for its form, `rejected` with no `judgment`, is proposed once more on a later run, `FORM_TRIES`), and kept as a `Body::Synthesis`
    node (store format 18) in the harness session (META `memory.session`, never compiled) with `derived_from`
    edges (`via = "synthesis"`) and `synthesis.proposed`/`.checked`/`.scored` rows (`fact/synthesis.rs`, scope
    `memory`; the day's spend reads back from them). A cluster with external text is never synthesized. Each
    frame goes through the memory pass's writer handshake (`MemoryPass::writing`; `memory_pass/turns.rs` counts
    its writers). The arms' seam: `Memory::science_for(arm)` and `Scene.science`; every arm but `+synthesis`
    leaves the harness session out before the index's top k (`Memory::begin`'s `exclude_sessions`), and the
    pipeline drops a synthesis as `arm` or `unchecked` (`MemoryScience::synthesis`). Tests: `tests_consolidate.rs`.
- **A first keystroke warms what the message will wait for** (theseus-tnky): `warm.rs` and `rpc/typing.rs`. Every
  surface sends `session.typing` (the TUI's input line, `watch --interactive`, Discord's typing event) once per idle
  spell (`theseus_protocol::warm::SPELL_SECS`, 120 s; the client's `Typist` and the core's `Warmth::admit` keep the same
  rule), and the core answers at once and spawns two warm-ups beside it, writing nothing (no frame, no row): the
  session's provider connection (`Provider::warm`: a keyless `HEAD` of `/v1/messages`, never a model call and
  nothing billed, sent only when no answer came within the pool's idle time; the provider is the session's last
  target's, else the live profile's) and the tender's model (`IndexTender::warm`: `index.warm`, to a tender that
  runs). The client's pool keeps an idle connection `provider::POOL_IDLE` (300 s; reqwest's default is 90). Only an
  owner's typing counts on Discord (`Core::typist_refused`: another person in a shared place warms nothing), and the
  MCP surface is refused by its allow-list. Health's `warm` block (`WarmHealth`) says when each was last warmed; the
  metric is `theseus.warm` by target and outcome. Nothing on a turn's path waits for it. Tests: `warm::tests`,
  `rpc/tests_typing.rs` (a stand-in provider that counts warm-ups apart from requests, a stand-in tender socket,
  a provider whose warm-up never ends), `provider/tests_warm.rs` (a loopback server that counts connections).
- **Retraction** (theseus-2ced; the killer-features plan's R13): `retract.rs`, `fact.retract { node, why }`
  (`rpc/memory.rs`, `judge_act(Act::Retract)`: the owner, from a private place; `theseus retract`, in the CLI's
  `OPERATORS`). It writes the `wrong` label through `memory.label`'s own row, then walks `node.reach` to its caps and
  sorts each exposure (`theseus_protocol::retract::exposure_kind`): a **session** that goes on (a recall copy whose
  `memory.used` row says used, or says nothing yet: treated as used; a report's relay; the node's own session) gets
  a correction note, queued in the META record `retract.notes` (`Notes`, on `Memory::corrections`) and written at
  its next turn by `turn/retract_step.rs` as the harness's own user-role node (`Correction: <gist> was marked wrong
  on <date>: <why>`), in a frame of its own that also takes it off the queue, so once; a **synthesis** that cites it
  is labeled `stale` (consolidation leaves a wrong or stale node out of its clusters, so the cluster is proposed
  again without it); a **task** that used it is named on one `retraction` post to the owner's DM (`jev_post`, never
  a card per task); a **published copy** is listed and labeled `wrong`; **seen, not used** is listed. All in one
  frame with the `fact.retracted` row (scope `retract`) that holds the table, so a second retraction answers from
  it and writes nothing. The answer is shaped for the place the request is run for (`session_id`): a shared place's
  never shows a private exposure's words. A new ledger row kind and META key, no format bump. Tests:
  `tests_retract.rs`.
- **Replies that cite what they remember** (theseus-ejdk, provable-memory-a): `recall/cite.rs`, `turn/cite_step.rs`,
  `rpc/cites.rs`; the pure parts are `theseus_protocol::cite`. Each recalled item's frozen header begins with a
  handle, `[m:a1b2c3]`: the last six characters of its node's id (what `theseus history` prints after the prefix),
  grown by the fewest characters that make every id of the pack distinct (`cite::handles_for`; an imported id ends
  `_<index>`, so a handle's characters are letters, digits and `_`). A header recalled before handles has none and is
  never cited. The system header's assembly note ends "When you state something from recalled testimony, cite its
  handle." (one sentence, no new breakpoint). At the turn's end `cite_reply` reads the handles in the reply
  (`handles_in`) against the turn's own `Recall` items (`Recalled.items`, kept while `count` is above zero: a detour
  or an unassembled section clears it): a handle the turn was not given is text, and each one it was given is a
  `memory.cited` row (scope `recall:<session>`, beside `memory.used`: recall id, handle, node, source session) in the
  turn's next frame, and a `Citation` in the result's `cites` (the reply's post carries it in its footer's `result`).
  `TurnRunner::citation` builds one: the frozen header, the source's excerpt over the range recall showed (600
  characters), and, for a person's Discord message, where it was said: the turn's `reply_to` rides in the input's
  own frame as the META record `msg.origin.<node id>` (no store format change; no row of its own). **A shared place
  never cites a private or imported source**: `citation(item, shared)` returns none for one, and a shared session's
  `memory.cites` and `session.history` leave it out; the handle stays text. `memory.cites` (`rpc/cites.rs`) resolves
  handles in a session's own `Recall` nodes, newest first; `session.history`'s `cites` does the same for the
  replies on its page. Citations only show: no Jev call, no frame, no ping. Tests: `tests_cite.rs`.
- **The import** (theseus-0lrr.6, the soul migration): `import/`. An outside pipeline's episode files (JSON Lines,
  format 1: `import/episode.rs`, every field from the format's lists and the hash over Python's canonical JSON, both
  string forms, `py_float`) become **imported sessions**: a `SessionRecord` with `imported` (`ImportedFrom`: tag,
  episode, hash, source, agent, place and labels as recorded facts, triage, as-of; store format 23), its id the
  episode's (`ses_ep<64 hex>`, `session_id_of`), its record scoped `import:<tag>`. It has no execution, and
  `turn.submit` refuses it (`import::refusal`), so it never takes a turn or reaches a compile but as recall's
  testimony; `session.list` leaves it out unread: the whole list, `confirm.list` and `compilation.list` read
  `Store::live_sessions` (theseus-store's `latest_of_kind_except`: two reads of the key table either side of the
  `ses_ep` run, so no imported row is visited, theseus-26jo; one session's `compilation.list` reads its record
  alone), and a page and the learning tender's task-brief walk (`sessions_from`) step over the import's births by key in
  their walk (`newest_keys_where`, theseus-7087); health takes the tags' counts (one META record a
  tag) off the projection's keys, and its fallback reads every session, to count the owner's own, the imported and the
  erased apart (theseus-revl). Its place is private whatever place
  the episode names (`TurnRunner::place_of` reads the id), so a shared place never recalls it. Each message is a
  node of origin `import` (`Body::Imported`: source, unit, sha256, integrity; `created_at_ms` the message's own
  time), the summary a `Body::ImportedSummary` citing its messages' ids; outside integrity is external to the index,
  so recall keeps it out (`untrusted`) unless `[memory] include_external`, and its header names it outside text.
  `import.episodes` (`import/write.rs`) takes one batch of a file's lines in one frame (cut past 4,000 records or
  8 MiB), with the tags' counts (META `import.tag.<tag>`) and an `import.batch` row, on the blocking pool, one import
  or erase at a time; an episode imported before is skipped, the same id with another hash appended to when its stored
  messages are an exact prefix of the new episode's (below) and else rejected and named by the first differing idx, an
  erased one not imported again, and a line that does not read named by its number. `import.erase` tombstones a tag (each node
  written again under its id, origin and time with `Body::Erased`, the session's `erased` receipt, an
  `import.erased` row), then asks a running tender's `index.forget`. The tag's counts ride in every frame of a batch
  or an erase, and an erase's count of the tag's erased sessions is absolute, counted from the tag's records as it
  reads them: a reader lags a frame at most, a kill between frames leaves the counts whole, and the next erase counts
  a cut run's tombstones too (theseus-mce3). The follower drops a tombstoned node as it
  reads it, at a rebuild too; `session.history` and `node.list` show an imported node by its newest record
  (`import::shown`), so an erased one is its tombstone. The WAL's earlier frames still hold the payloads: §5.6's
  erasure in place is not built. `import.list` reads the tags' META records. The owner's acts (`judge_act(Act::Import)`; the CLI's
  `OPERATORS`). `theseus import openclaw|erase|list|topics`. Tests: `import/tests.rs`, theseus-index's `tests_import.rs`.
  **Harness history** (theseus-o73y): `theseus import claude-code|codex` (the CLI's `harness/`) makes the episodes of
  those tools' session files, sources `claude-code` and `codex` (`episode::HARNESS_SOURCES`, apart from the
  pipeline's `SOURCES`), a `cli` place named by the project directory and its id the git branch, the tool results
  outside text. `write::import_batch_scrubbed` scrubs every message, summary and place name of a harness episode
  with the daemon's own scrubber as it writes (`write::scrubbed`; the CLI holds no vault value), after the episode's
  hash was checked over what the CLI sent, so the hash stays the idempotency key and the stored `sha256` is the
  stored text's. Tests: `import/tests_harness.rs`.
  **A session that grew** (theseus-pjmj7; `import/append.rs`, no store format change): the same episode id with another
  hash whose messages begin with the stored ones appends only the new nodes (`node_id_of(session, idx)`, each at its own
  time), with the session's record (hash, message count, `as_of.end_ms`, `last_active_ms`; tag, place, labels and `file`
  stay as first imported) and the tag's counts in the one frame; a summary is written again only when its text differs.
  The prefix is proven by reading the stored message nodes in the order they were written (`Store::session_nodes`: the
  same idx and sha256, position by position), not by a digest kept on the session, and after the scrub (a scrubbed
  source's stored digest is the scrubbed text's). A changed, removed or reordered message is refused naming its idx, an
  episode that adds nothing is refused as such, an erased session is never appended to; the batch's `touched` lists the
  sessions created or appended to. Tests: `import/tests_append.rs`, the grown file in `tests_harness.rs`.
  **The OpenClaw sync** (theseus-pjmj7; `import/sync.rs`, `rpc/import_sync.rs`, `judge/people/sync.rs`;
  `[import.openclaw]` in `config/import.rs`: `state_dir`, `owner_ids`, `agents`, off while `state_dir` is unset):
  `theseus import sync` runs `scripts/openclaw-export.py` on OpenClaw's own stores and sends the episodes (source
  `openclaw-sync`, scrubbed like a harness source: `episode::SCRUBBED_SOURCES`) through `import.episodes`; the daemon's
  half is `import.sync.plan` (config, cursor, the first run's start: the newest `as_of` end of the `openclaw-store`
  sessions) and `import.sync.record`, which writes META `import.sync.state` (the exporter's cursor, the run's counts,
  the people list) with the `import.synced` row in one frame, then makes the touched tags' people (`import::people`),
  reads each touched session's nodes after its mark (META `judge.people.<session>`, the live point's key; a WAL position)
  for people under the sweep's cap, shared with the sweep (`people.sync.day` beside `people.sweep.last`), and writes the
  state again with the spend and the sessions left. Health's `imported.sync` (`HealthSync`) and its `warn` for a failed
  or a missed night (`SyncState::health`). Tests: `import/tests_sync.rs`, `judge/people/tests_sync.rs`, theseusd's
  `tests/import_sync.rs`.
  **Its topics** (theseus-anh3): `import.topics` (`import/topics.rs`, the owner's act, never at a start) reads a tag's
  sessions' stored `labels.topic` (slash paths) and writes, through the ontology's `Board::write` a frame at a time
  (cut past 4,000 records, quiet waited for between, the stop looked for there): a topic for each label and each
  prefix (a part found by its name under its parent, so the operator's own topic is used; a new one's id the path's
  slug, `added_by` `import <tag>`), and each live imported session's topic list, origin `import` (the topic kind's
  seed row names it), at most the kind's 3: an ancestor of another label dropped first, then the pipeline's first
  three; the operator's own memberships kept and counted first; a list that holds the same topics from the same
  origins is not written again (idempotent), and an `import.topics` row per frame. `import.erase`'s second half
  (`topics::unassign`) empties the erased sessions' lists, then retires each topic an import made that nothing uses
  (`Category.retired_ms`: no child, membership, or guidance; the snapshot holds none for it, a load skips it), the
  deepest first. `ontology.list` counts each category's sessions (`members`) and leaves the memberships out on
  `memberships: false` (the cockpit's page and the CLI's tree). Tests: `import/tests_topics.rs`.
  **Its people** (theseus-wy7y): `import.people` (`import/people.rs`, the owner's act, `--dry-run` counts) reads a tag's
  live sessions' `person:<name>` authors (from their nodes) and each DM's other party (its place's id: digits are
  `discord:`, else `slack:`), one person each (a name that spoke alone in one party's DMs is that party), found by an
  exact handle first (`Ontology::person_by_handle`; a DM's person holds its `discord:<id>`), never by a display name
  across makers; an author or party the proposals' exclusions exclude (`Core::not_people`, theseus-0p1r) is never a
  person (counted in `excluded`); each session's person list, origin `import`, at most `PER_SESSION` (12). The erase empties every
  stored kind's lists (`Kind::stores`) and retires the import's people nothing uses. The person kind is given with a
  stored side (`theseus_ontology::person`): the operator and the import declare people and keep stored lists, never
  the transport; `Walk::of` takes a stored person the place also gives once; handles, merges and their undo are
  `rpc/people.rs` (`ontology.person.merge`, the `ontology.merged` row holds what moved; a DM's first bind whose id
  another person holds merges that one into it), with `ontology.proposal.accept_all`, the bulk yes. Tests:
  `import/tests_people.rs`, `tests_people_proposals.rs`, theseus-ontology's `tests_people.rs`.
- **The books, first cut** (theseus-civ0): `books/`, `rpc/books.rs`. `books.list` (each of the seven books and
  `unsorted` with its count and its episodes' first and last start) and `books.page` (a book's episodes newest first,
  by cursor, filtered by topic, source and place, its facets on a first page), read only, from the import's labels
  (`labels.book_hint`; none, or one not of the seven, is `unsorted`). The index keeps an imported session's book
  terms in the store's projection (`books::session_terms`, in `store::PROJECTION`, renamed `projection.core.2` for
  them): `bk:<book>` (the count), `bt:<book>␁<start hex>` (the order), `ft:`/`fs:`/`fp:` the same per topic, source
  and place, and `ct:`/`cs:`/`cp:` the facets; an erased session has none. So the list reads no record, and a page
  a range of terms (theseus-store's `terms_range`, `term_counts`, `terms_of`) and a session and a summary per
  episode shown; a filtered page checks its other filters by the key's own terms and stops past `SCAN_MAX` (5,000)
  looked at, with a cursor. Until the terms are whole (`build_terms`, after serving) both read every imported record
  (`books::Walked`) and say `indexed: false`. Asked on a surface that reads no private text (only the CLI and the web UI read it, as for
  `import.sessions`) or for a shared place (`session_id`), an episode's text (summary,
  topics, place, partner) is withheld and no filter or facet reads it, as recall keeps the import from a shared
  place. An episode's messages are `session.history`. Tests: `books/tests.rs` (the measure over 20,000 is ignored:
  `--ignored --nocapture`).
- **The daemon's resident memory** (theseus-9lxe): `resident.rs`. glibc keeps an arena per thread that allocated at
  once, and a blocking-pool thread that built something large (`import.sessions`' catalog, a full `ontology.list`, the
  books' terms rebuilt, an import's batch) leaves its freed pages held: over 21,779 imported sessions a daemon held
  202 MiB from the system with 60 in use. Every method's answer and each background build's end marks work
  (`Resident::mark`); after `QUIET` (10 s) with no other mark, and never later than `MAX_DEFER` (60 s) after the first
  (the cockpit reads `health` every 2 s while in sight), the tender trims (`malloc_trim(0)`) when the free heap passes
  `TRIM_FLOOR` (8 MiB). `import.sessions`' catalog (`import/catalog.rs`) is built a page of records at a time, under a lock of
  its own (health and the tender read its state on a runtime worker, never waiting for a build),
  holds each episode compactly (its repeated values interned; `Catalog::episode` makes a page's rows whole), and is dropped after `CATALOG_IDLE` (10 min) with no read; the next read builds it again (the Context page reads it
  every 30 s while open). The tender starts after serving (`tend_memory_after_serving`), waits on a `Notify` and
  tokio's timer, holds the core by `Weak`, and trims and drops on the blocking pool. Health's `resident` block: the
  resident set, the heap in use and held (`mallinfo2`, as the tender last read it: it walks every free chunk under each arena's lock, so health never calls it), the trims, and the caches by size. A musl build reads no heap
  and trims nothing. Tests: `import/tests_catalog.rs`, `resident::tests`.
- **The arrangement** (M5 step 27, theseus-vug.2): `arrangement.rs`. `task.create` needs an `arrangement` of quoted
  pieces (`{quote | node, role}`, `trust`, `supersedes`), resolved in the calling session's own transcript (exact,
  whitespace runs as one space, at least 20 characters, exactly one node; the reply holding the call and earlier
  `task.create` results are no source), refused without a standing `objective` or `design` piece, and held by the
  fidelity check (a brief under 200 characters, more than ten operator messages since the last task, one piece)
  unless `fidelity_ack`. The pieces go into the child as one `Body::Arrangement` node after the brief, in
  `open_task`'s frame, with a `derived_from` edge (`arrangement`) to each piece's node, and carry the text the
  compiler renders after the brief (a superseded piece by reference only). The session's `task.arrangement` names
  the node, which `task.list` reads; the rows are `task.arranged` and `task.arrangement_refused`
  (`fact/arrangement.rs`). A scripted `task.create` in a test needs an arrangement whose quote its parent's
  transcript holds once. Tests: `tests_arrangement.rs`, `arrangement::tests`.
- **Check tasks** (M5 step 28a, theseus-vug.3): `check.rs`. `task.create { check_of, profile? }` opens a task that
  checks another by its claim: `check_of` resolves only among the tasks the calling session started, and a task
  with no report is refused (`task.check_refused`). Its `Arrangement` node carries the checked task's standing
  `objective` and `acceptance` pieces (which stand for its own under 27's rule; the fidelity check does not apply),
  any pieces of its own, and the report as a `claim` (`Body::Arrangement.claim`, rendered by `check::render`), with
  a `derived_from` edge to the report (`VIA_CLAIM`). The exclusion is enforced on what the parent writes: an own
  piece that copies the report reads as the claim, one deriving from any other node of the checked session is
  refused. The overlap flag (`OVERLAP_WORDS`, 12, words as runs of letters and digits without case) compares the
  brief and own pieces with that session's nodes but its report, brief and arrangement. The basis is
  `TaskOf.check` (`theseus_protocol::TaskCheck`, store format 14), the row `task.check_opened`, and its line on
  `task.list`, the report's post and node. `profile` is a check's alone (`ToolRuntime::profiles`). Tests:
  `tests_check.rs`, `check::tests`.
- **The task graph** (M7 step 39a, theseus-ext.6): `task_graph/`. A task is a record of the store's `TASK` kind
  (`theseus_protocol::tasks::TaskRecord`, stored as shown; `tsk_…`, a task session's id sharing its session's tail).
  `task.create` without `brief` records a plan item (no arrangement needed); with it, 27's rules hold and the record
  rides `open_task`'s frame. `task.update`, `task.split`, `task.close` (`task_graph/tools.rs`) run in the harness under
  `Store::lock_task` (the order: a session's lock, a task's, an execution's), compare the `version` the call names
  (stale: refused with the record as it is now, `task.stale_refused`), and write the record in the frame that settles
  the call. Layer 1 (objective, acceptance, abandoning) plans with `Plan::authority`, so the gate asks at every posture
  as the floor does: its proposal is written on the record in the asking frame, the yes applies it, a no clears it in
  the answer's frame (`confirms.rs`). A task session's running state is read from its execution (`state_now`), and its
  report closes its record in the frame that ends it (`tools::Closing`, `turn.rs`). The view (`task_graph/view.rs`)
  is the last block of a request's last message, the conversation's breakpoint on the block before it; a test's
  stand-in that reads the last user text skips it (`view::is_view`, `view::HEAD`). Facts: `fact/task_graph.rs`.
  Tests: `tests_task_graph.rs`, `task_graph::tests`.
  - **Claim leases** (39b, theseus-ext.14; `task_graph/lease.rs`, store format 19): `task.claim { id, version }` sets
    `claim { by, session, until_ms }`, `[kernel] task_lease_minutes` ahead, and moves the version; the holder's
    edits and its claim again renew it without a version of their own, and a close ends it. A held task refuses
    another's claim before its version is compared (`blocked: claimed by session … until HH:MM`); another's edit is
    not held back. The due pass (`Core::free_expired_leases_if_due`, the driver's tick) frees a lapsed claim from
    `ToolRuntime::leases`, the claims kept in memory (built after serving by `Core::warm_leases`, noted after each
    edit's frame), never a scan a tick: the record and `task.lease_expired` in one frame, the version kept. A claim
    past its `until_ms` reads free before that (`claim_at`, `shown`). A change's board is its home's place
    (`task_graph::home`). Tests: `tests_task_claims.rs`.
  - **The layer-1 question's change** (39b): `ConfirmRequest.change` (`tools::change_of`: the task, its title, the
    field, before and after), worded once by `TaskChange::question` for the Discord card, `theseus answer`, and
    the cockpit.
- **Earned autonomy** (theseus-uttu, the killer-features plan's row R18): the loosening that comes from the record,
  proposed and never silent. `policy/earned/find.rs` is a pure function over ledger rows (`tool.confirm_requested`,
  `action.confirmed`, `action.declined`, `policy.tightened`): `proc.run` approvals go into a trie of their argv's words
  (the program by base name), and a node is a candidate when the approvals reaching it and no narrower candidate hold at
  least `[policy.earned] min_approvals` (20) in `window_days` (7), with no decline, no "should have asked", no floor call
  under it, and each approval asked by the tool's posture alone (`by_posture`: a reason ending `approve (enforcement =`
  or a `[policy.*]` line; the approve lists, the roots, the hold and a tightening are no evidence); file tools group by
  tool and workspace root. A prefix is never shorter than the program and one argument, never a shell or a program that
  runs what it is given, never an interpreter with an inline-code option (`prefix_ok`, asked again of every stored grant).
  `policy/earned.rs` holds the grants, the groups passed on (90 days; a "should have asked" revoke is for good) and the
  open card in one META record, `policy.earned`, written with its ledger row in one frame (`policy.earned_proposed`,
  `_granted`, `_passed`, `_revoked`) as tightenings are, read when the core starts. The gate's step is `earned::gate`
  in `toolrun/order.rs`, after the call's own policy and before the broker's: a call the tool's posture alone makes wait
  (`ToolPolicy::posture_only`: nothing the floor, a layer-1 change, the lists or an AWS rule asks), in a private place,
  with no `env`, for a tool no one tightened, that a grant covers, runs at `notify` (never `open`), its notice's setting
  `earned grant <id>`; every later layer still raises it. `rpc/earned.rs`: `policy.earned` (the number, the grants, the
  card; `propose_now` and the answers are `judge_act(Act::Earned)`, the owner from a private place), `policy.earned.answer`,
  `policy.revoke` (anyone's, as a press is), and the card's tender after serving (`earned_after_serving`, once a day at
  `[judge] learning_hour`, a card at most every `every_days`, its work on a thread at nice 19). "Should have asked" on a
  grant's notice (`tighten_by_earned`: the notice's row names the grant) ends the grant and tightens nothing.
  `policy/earned/measure.rs` is the number: questions per active hour over four weeks, and what the grants would have
  saved. Not built: M5's Jev decision on the approval ladder (a judgment before the card, beside `earned::gate`).
  Tests: `policy/earned/tests_find.rs`, `tests_measure.rs`, `tests_m3/earned.rs`, the daemon's `tests/earned.rs`.
- **Scoped grants** (theseus-drjs; `policy/grants.rs`, `rpc/grants.rs`, wire `theseus_protocol::grants`): one answer's
  "yes, and don't ask again", for its conversation alone. The offer (`ConfirmRequest.grant_offer`, and the option
  `approve_grant` beside the plain yes) is computed when the question is asked (`grants::offered`, from the call's
  gate) and again from the stored gate record by `confirm.list` and a card (`grant_offer_of`), one rule (`offer`):
  only where the posture alone asked (no later layer changed the policy's reason), in a private place, outside L1, no
  `env`, no steps, no tightening; `proc.run` by `prefix_of` (the program and its subcommand, `python3 -m <module>`, a
  launcher's program too; never a shell, a wrapper, inline code, a relative program path or an option first) or the
  edit tools under the session's directory (`edits`; never `/` or the home). An answer takes it by `action.confirm
  { grant }` (its pattern must equal the daemon's own offer) or `work.answer`'s `approve_grant` (no pattern at all);
  the grant is the daemon's offer, its `policy.granted` row in the answer's frame (`confirm_action_full`'s `extra`).
  The gate's step is `grants::gate` in `toolrun/order.rs`, right after `earned::gate` and with its conditions: a
  covered call (`grants::covers`, the one rule modes calls: the normalised prefix and the program as written, no URL
  after it; or every resource canonical under the root) runs at `notify`, its notice's setting `ran under your grant: <what>, this conversation, sgr_…`. In memory
  per session (`Earned::scoped`, one lookup a call); `policy.grants` lists them, `policy.revoke sgr_…` ends one
  (`policy.grant_ended`), a retire or a supersession ends a session's in its frame, and `load_grants` reads them
  back after serving (`warm_grants`). No store format change: new ledger kinds only. Tests: `policy/grants/tests.rs`,
  `tests_m3/grants.rs`.
- **Modes** (theseus-lyxy; no store format change; `policy/modes.rs`, `rpc/modes.rs`, `fact/modes.rs`, `task_graph/plan.rs`, wire
  `theseus_protocol::modes`): `ask`, `edits`, `plan` and `auto`, an overlay on the config's postures for one conversation,
  never written into the config. `Earned::modes` (`Modes`) is the board in memory, one atomic while no session has a
  mode and one read-lock lookup after; a session no one moved has no entry, so a new conversation (a new session) is
  `ask`. `session.mode {session_id, mode?}` sets or reads (`rpc/modes.rs`): `ask` and `plan` tighten, so are anyone's;
  `edits` and `auto` loosen, so are `judge_act(Act::Session)` (the owner, from a private place; the CLI refuses it in a
  job); a shared place has none but `ask`; it is in `ORDERED`. Each change is one `session.mode` ledger row carrying the
  mode alone (`ModeChanged`, which is also the `session.mode` notification), read back once after serving
  (`Core::warm_modes`, `load_modes`: each session's newest row, a retired session's none; an older row's `steps` is
  ignored). The gate's step is
  `modes::gate`, run inside `Layer::Scoped` right after `grants::gate` and under its conditions (a call the tool's posture
  alone made wait, private place, no L1, no floor, no `env`, no tightening, `args_within_roots`): `edits` runs `fs.edit`,
  `fs.write` and `fs.patch` whose every canonical resource is under the session's directory at `notify` (it asks
  `grants::covers` with an edits grant it holds for the call), `auto` runs any such call at `notify`; the notice says
  `ran in edits mode: edits under <dir>, this conversation`. `plan` is a refusal, not a posture: `ToolRuntime::refusal_at`
  (the place's `refusal`, then `plan_refusal`) refuses a call whose class is not a read (a writer, a program, an MCP
  tool, a post, a batch's every step) with `plan mode: read-only; present the plan with plan_present…`, reason `mode: …`
  (`refusal_reason`), before the order runs, so no layer of the order can pass it. `plan_present {steps}` always asks (the
  gate raises it to approve whatever `[policy]` says; the question's preview is the numbered plan, its other yes
  `GrantOffer::mode_edits`, `approve_grant` / `e`, which `confirm_granting` turns into a pick, not a grant); once approved
  its run records each step as a **plan item of the task graph** and moves the conversation to `ask`, or `edits` for the pick.
  **One plan** (the owner's decision 2, theseus-lyxy round 2): a plan item is a `TaskRecord` with no session and no parent
  (`task_create` without a brief makes the same), so the work board's `progress` (`plan 2/5`, its `current`) counts the
  steps of a plan as it counts any; `task_graph/plan.rs` holds the whole of it (`create`, `step`, `view`, `items_of`): the
  approval's records ride the call's frame, a millisecond apart so they keep their order, ids `tsk_<call>p<n>`, a call
  made again records nothing twice. `plan_step {index, state}` (1-based by the order the items were listed; `doing` is
  `in_progress`, `done` closes the record `done` with the evidence `plan_step:<n>`, `skipped` closes it `abandoned`; a closed
  step stays) is a Write-class harness tool that runs open (it only moves a record), so two marks in one response keep their
  order and plan mode, which refuses a write, has no step to mark; `task_close` moves the same items. The checklist every
  surface shows and `plan 2/5` are `session.mode`'s `plan: Vec<PlanItem>` (`{id, title, state}`, a view of the records,
  never stored): the result carries it (`task_graph::plan::view`, a scan of the task records, only when asked), and
  `PlanMoved` (`fact/modes.rs`, notification only) sends it again after any frame that changed a plan item
  (`ToolRuntime::plan_moved`, in `harness_done`: a `task_create` or `task_close` of the model's is the same plan).
  `plan_present` and `plan_step` are in the deferred family **`plan`** (`tools_defer/family.rs`; in no place's defaults, so
  no first request carries them), loaded while the conversation is in plan mode: `deferral_in` adds the family to the
  fresh choice, and entering `plan` (`session.mode`) asks for a recompile on the next turn (`request_recompile`,
  `Transcript`), so the request that follows takes the fresh set, whatever the last compilation recorded. Both are
  harness tools in `run_harness` (`ToolRuntime::run_plan`), offered in private places only; neither is in the template's
  `[policy.tools]` (as `thread.note` is not). The mode is not offered in the tools note or the system prompt (a
  refusal's words and the tools' descriptions carry it). Tests: `tests_m3/modes.rs`, `task_graph/plan.rs`'s,
  the TUI's `tests_mode.rs` and `chat/tests_mode.rs`, the CLI's `tests/mode.rs`.
- **The model's commitments** (theseus-lherl; horizon row HZ1): `commitments.rs`, `wake_tools.rs`. The conversation's
  pending wakes ride in the last block of every request, beside the task view (`view::attach_text`, no breakpoint of
  its own; a stand-in model that reads the last user text skips both: `view::is_head`): at most `MAX_LINES` (10) lines
  and `MAX_TOKENS` (300), then `and N more`, nothing at all while none is pending, `⏰ a8cebb Fri 09:00 note` and a
  series `🔁 a41d04 every 1d 21:00 (next Fri) note`. Built from `Kernel::take_wakes_listing`'s one read, held in the
  turn (`Frames.commitments`, lent as `TurnCtx::commitments`) and replaced by what each wake tool's frame returns,
  never read from the store again; an operator's cancel mid-turn shows at the next turn. `wake.cancel {id}` and
  `wake.move {id, at | after, series?}` act on this conversation's own wakes (a name is resolved among them, another
  conversation's is not found), only in a private place (not `public_tool`, so a shared place is not offered them
  and the gate refuses them; each tool asks `places::refusal` again), one frame each with its row
  (`wake.cancelled { by: "model" }`, `wake.moved { from, to }`); a move keeps the id, a series' next occurrence moves
  alone unless `series: true` moves its time (`every`, `days`, `until` kept). A routine, a watch or a deferred task
  joins as another line with its own icon. Tests: `tests_commitments.rs` (the stand-in model reads the block's ids).
- **Reminders** (theseus-8ux6x; horizon HZ2; store format 30): every future thing is a wake with a purpose
  (`theseus_kernel::Purpose`: `Check` the model's own, `Remind`, and `Routine` and `Watch` declared for their rows) and a
  run (`WakeRun`: `Turn`, `Notice`, and `Task` and `Probe` refused with words until routines and watches land).
  `wake_home.rs`: **Home** is the conversation a reminder lives in when no place would hear it, the session the owner's
  DM runs on (`PlaceRule::owner_dm_places`), else a "Reminders" conversation made on first use (META `home.session`);
  `Core::fire_notices`, in the harness's 500 ms tick (no new timer), takes every due `Notice` wake in any state of its
  conversation, a turn running included (`Kernel::fire_notices`: one frame, no turn, no model call, no admission slot),
  and posts its note as a `reminder` outbox post where its conversation posts, else where it posted when set, else Home's
  DM; with no place at all the surfaces still hear `wake.noticed` (`fact/wake.rs`, `bus::wide`, with its four answers
  `theseus_protocol::reminder_answers`, which `notices::reminder_notice` makes the Interrupt of), so none is silent. The
  owner's verbs, no model turn (`rpc/wake_owner.rs`): `wake.create { when, note, every?, days?, until?, session? }` (no
  session: Home), `wake.snooze` (a pending wake moves; a fired one, by its whole id with `fired`, is set again from its
  META record), `wake.pause { until? }`, `wake.resume`, `wake.done`; anyone's, as `wake.cancel` is, the daemon-wide
  `[kernel] max_reminders` bounding a flood. The answers are direct calls until `work.answer` routes them. `wake_when.rs`
  reads `when` (`10m`, `in 1h`, `tomorrow 9:00`, `fri 9:00`, a date, RFC 3339; the kernel's `when.rs`). The model's side:
  `wake.at { for: "owner" | "self", say_only }` (`for: owner` is a `Remind`, `say_only` a `Notice`; a placeless
  `Remind` takes the DM's target when set). Limits: `[kernel] max_wake_ahead_days` (365, replaces `MAX_AHEAD_MS`;
  `task_start` reads it too), `max_wakes` (check-ins a conversation), `max_reminders` (100), `max_routines`, `max_watches`
  (`config/wake_limits.rs`). The commitments block says `remind:` and `(paused)`. Tests: `tests_reminders.rs` (a real
  core, the driver, the real clock), the kernel's `tests_wake_owner.rs`, theseusd's `tests/wakes.rs` (a stop across the
  due time), theseus-discord's `tests_gateway/remind.rs`.
- **Routines** (theseus-pvm5c; horizon HZ4; store format 34): recurring work done right. A routine is a conversation's execution of
  its own (limit = its month, `[routines] default_month_usd`, pinned) with one pending wake (`Purpose::Routine`, `WakeRun::Task`) and
  a record, `theseus_protocol::routines::RoutineSpec`, as the META key `routine:<session id>` (no stored field of the session's
  own); `routine.rs` holds the pure and shared parts (`Book`: the records in memory, read once on the driver's first tick;
  `prepare`/`create`, which the owner's verbs and the model's tool both call, needing no `Core`; `plan`, the firing decision;
  `info_of`, `run_row`), `rpc/routines.rs` the eight verbs (`routine.create|list|runs|pause|resume|run_now|update|delete`, the
  owner's acts: `judge_act(Act::Routine)`, refused inside a job by the CLI) and the due scan's pass (`fire_routines_in`, called
  at the end of `Core::fire_notices`, the harness's 500 ms tick: no new timer). `plan` decides in order: after downtime
  `catch_up` (`one`: a run saying "N missed while the daemon was down"; `none`: skipped as `catch_up`; `auto`: by the schedule's
  period, an hour or more runs once); a run still going (`overlap`: skipped, or a refusal for run now); the month's money (a
  run is carved whole from what the month has left; short of it the occurrence is skipped as `budget` and the owner told once a
  month, `RoutineSpec.month_told`); else the run: a task session whose first node is headed `Run 22 of Morning digest, due
  08:00, started 08:00:00.3 (2 missed while the daemon was down); the last run reported: <first line>`, and the usual task
  brief after it, its report posted to `deliver_to` (`dm` by default, `none`, or a place) like any task's. The month turns at
  the first run of the local month (`budget.reset { why: "month" }`). `speak: when_notable`: the brief asks for a report ending
  `NOTHING NEW` when there is nothing to say, and such a run only logs (`TaskOf::target_for`, `routine::quiet`, the run's mark
  `routine.run:<task session>`). A run of a routine notifies at its limit instead of asking (`Kernel::overdraws`); A routine's **grant** (theseus-52tut, below) is its wake's `TaskRun.tools`. The model's `routine.create` (`routine_tool.rs`) is private-place only
  and always waits for the owner's yes: its plan names an authority, which `task_graph::tools::authority_for` keeps for it, and
  its card reads `Create routine "Morning digest": weekdays at 08:00 (next: Fri 08:00, Mon 08:00, Tue 08:00) · up to $0.50 a
  run, $10 a month · posts to your DM`. A conversation's commitments block lists the routines it made (`🔁 a41d04 weekdays 08:00
  Morning digest (next Fri)`, from `Book::made_in`, memory only). `[kernel] max_routines` moved to `[routines] max_routines`.
  Tests: `tests_routines.rs` (a minute routine runs three times as three fresh tasks reporting to the DM; overlap; pause and run
  now; a spent month and its reset; the catch-up's words; the model's card; `when_notable`; update and delete), the kernel's
  `tests_routines.rs` and `tests_cron.rs`, and `tests_layouts.rs`' format-33 wake.
- **A routine's trust** (theseus-52tut; horizon §5.4, D-H6 "a routine's named tools run without asking, even after the run reads
  outside text; everything else still waits"): `routine/grant.rs`, `routine/watch.rs`, `rpc/routines_watch.rs`. The **grant** is the
  routine's wake's `TaskRun.tools` (no new stored field; `RoutineInfo.grant`): tool names, and for `channel.post` its place
  (`channel.post #digest`), set by `routine.create`/`update` (`--grant`, `--add-grant`) and the model's `routine.create` (shown on its
  card), checked at creation (`grant::check`: a tool the daemon has, none of `NEVER` (`proc.run`, `term.*`, `aws.call`, …), a post with
  its place) and read by the gate for the routine's own run only: `Book::grant_for(task)` is one map lookup for a task whose parent
  session and execution are the routine's (a task a run starts has the run as its parent and gets none; any other session pays an
  `Option` test), cached from the wake and dropped by `Book::set`. `grant::gate` is the `Layer::Routine` step of `toolrun/order.rs`
  after the glide's layer: a granted call that only the tool's posture (`posture_only`) or the hold would make wait runs at `notify`
  (`ran under the routine's grant: <entry>`) and T1's hold does not apply; the floor, a layer-1 change, the approve lists, a granted
  secret, a place's ceiling, a tightened tool and an L1 call's own asks still hold. **Bounded asks**: a routine run's question waits
  `[routines] run_deadline_secs` (900) (`ask_ttl_ms`: its card, `confirm.list`, the expiry scan); past it `expire_questions` queues it
  (`Book.blocked`) and the harness's tick (`Core::due_and_routines`: `due_without_turns`, then `routines_tick`) writes META `routine.blocked:<task>`
  `{tool, reason}` and cancels the run, so it ends `blocked` (`RunRow.state`, `blocked`) and never expires silently. **The ping rule**
  (`notices.rs`'s `ROUTINE_PURPOSES`, `routine_pings`, `routine_line`, `routine_answers`): a failed or blocked run is one Interrupt a
  routine a local day with the streak, posted to the routine's target as a `reminder`-shaped post and sent to every surface as the
  `wake.noticed` of `fact::wake::RoutineNoticed` (purposes `routine_failed|blocked|skipped|missed`; the notice's id is the routine's
  wake); the ping book is META `routine.ping:<routine>` (last run settled, last day told), so a restart tells nothing twice, and the
  run's own failure report is not posted (`watch::replaced`). Skipped and missed occurrences inform. `work.answer` on a routine's wake
  takes `run_now|pause|details` (+ `add_to_grant` after a block) (`answer_routine`). **Try it now**: `routine.try` runs
  occurrence 0 (`plan_trial`: run 0 in the log, not counted in `runs`). Tests: `tests_routine_trust.rs`, `routine/grant/tests.rs`,
  protocol `tests_notices.rs`. Not built: the creation card's `[Try it now]` button (the verb and the CLI exist).
- **Projects** (theseus-n5ui2; horizon HZ10; store format 45): long-term work on the task graph, no second list. A project is a
  TASK record of `kind: project` (`TaskRecord.kind`, `due_at_ms`, `review`; an item is what a record without a kind is),
  its milestones `kind: milestone` with a date, its steps the plan items under it. `project/` holds the parts: `progress.rs`
  (steps and milestones done of total, the next due, the overdue, from the subtree), `slice.rs` (an attached conversation's
  slice: the goal, the open milestones with dates, the next steps, within `MAX_TOKENS` (300) of the task view's 1,500,
  built from the records in memory every loop and attached after the commitments' block, so the request ends with it and
  no compaction can take it; a conversation with no project adds nothing and reads nothing), `remind.rs` (each open dated
  milestone holds two notice wakes in Home, or in the conversation whose call set them: `⚑ due today: <project>, milestone
  N: <title>` at the date and `⚑ overdue: …` a grace later, `[projects] overdue_after_minutes`; their ids are made from the
  milestone's, so `sync_milestone` finds them again, moves them with the date, and cancels them when the milestone or its
  project closes; a date already past sets none), `create.rs` (the project and its milestones as records in one frame,
  deterministic ids from the call's), `review.rs` (the weekly review, a routine made with the project, `[projects]
  review_schedule` `mon 08:00` by default; its brief names the project by id and asks for a report with next week's steps
  as plan items and `Decisions for you`, because `owner_ask` is not on main yet), `info.rs` (a list row). The owner's verbs
  are `rpc/projects.rs` (`project.create|list|show|attach`; the first is `judge_act(Act::Routine)`, the last `Act::Session`);
  the model's are `task_create { kind: project|milestone, due, milestones, review }` (a project with a review waits on one
  card, as `routine.create` does: `task_create`'s plan names the authority, `authority_for` keeps it) and `project.attach`
  (`project_tool.rs`; it writes `SessionRecord.project`, which no turn's write-back touches). `task_update`'s patch takes
  `due`. The goal is the owner's layer when the owner sets it (a project made by `project.create` has `by_model` false, so
  an agent's edit of its objective or acceptance waits for a yes); a milestone's date is the agent's plan. Reminders are
  synced after the call's frame (`remind::after`, in `harness_done`) and after the owner's verb; a plain item costs a kind
  test. `context.compiled.project` names the slice (`theseus why` prints it). Tests: `project/tests.rs` (pure, and the
  slice's bench), `tests_projects.rs` (the owner's create, a moved date and a close, a date firing, an overdue notice with no
  sound, the review's run in its log, the layer-1 guard, the model's card, and three compactions), `tests_layouts.rs`.
- **Deferred tasks** (theseus-rcgph): `task.create { start_at | start_after }` opens a task asleep (`task_start.rs`):
  `Kernel::open_task_at` writes it `Waiting{DueAt}` in the open's one frame with its carve held (the parent's
  remaining shows it), the due scan queues it, and its first turn's catch-up writes the node `Deferred task, set <when>
  for <when>; started <now>` (late, and whether the daemon was down, as a wake's; `turn/deferred_step.rs`), once, by
  its META record `task.deferred.<session>` (no store format change). The time is `wake.at`'s parse and bounds
  (`MIN_AHEAD_MS`, `MAX_AHEAD_MS`). `TaskInfo.starts_at_ms` is derived from the execution while it sleeps
  (`task_start::starts_at`), and the board, `theseus tasks` and `/tasks` say `▷ starts Fri 09:00`. A cancel before
  the start ends it and releases the carve. Tests: `tests_tasks_deferred.rs`, the kernel's `tests_deferred.rs`.
- **Watches** (theseus-f2r2b; horizon HZ6): waiting on the world with no model. A watch is a pending wake of purpose
  `Watch` and run `Probe(WakeProbe)` (`watch/spec.rs` says where each field rides: `kind` run|fetch|file, `subject` a
  JSON object of the check's fields and `then`, `until` the condition as typed, the note the title, `repeat.every` the
  interval and `repeat.until_ms` the expiry; no store field, no format bump), so `wake.list`, `wake.cancel`,
  `[kernel] max_watches` (20) and a conversation's end already apply. `watch/cond.rs` (pure: `exit N`, `equals`,
  `contains`, `matches` through `theseus_tools::rx`, `changes`, `above`, `below`, `exists`, and a JSON path),
  `watch/step.rs` (pure: an `Entry` takes a check's result and says whether it is news; the first value, a different
  one, a met condition, an expiry and a third failure in a row are news, an unchanged value is not, failures back off
  x2 to an hour), `watch/check.rs` (a file read, a fetch through the web tool's client (`Web::get_text`), a command in
  L1 through `theseus_kernel::probe_run`, each bounded to 30 s and 64 KiB and scrubbed before it is a value),
  `watch/board.rs` (the open watches in memory, next check times and last values; `Core.watches`, shared with
  `ToolRuntime.watches`), `watch/run.rs` (`Core::watch_tick`, which the driver's 500 ms tick calls: the first tick reads
  the open watches in with `Kernel::open_watches`, each then checks once at once; at most four checks run at once, off
  every turn's thread, the core held by `Weak`; `watch_finish` applies a result and writes news through
  `Kernel::probe_result` (a watch that ends, or a reply that meets it, leaves the board before its frame is written, so a
  reader that finds `watch.met` never finds the watch waiting: theseus-1b2zm): one frame, with `watch.checked|met|expired|failing`, the state in META `watch.state.<wake id>`,
  and the post; `fact/watch.rs` announces `wake.noticed` with purpose `watch.met` (an Interrupt with sound),
  `watch.expired` or `watch.failing` (Inform)). A `then: turn` or `task` turns the met wake into a due turn wake of the
  same conversation whose node reads `👁 watch met: <title> → <value>, after 2 d 4 h and 611 checks` (`wake::fired_text`);
  a task's brief rides that turn's text (a task needs an arrangement quoted from a transcript, which only a turn can
  write). The model's tool is `watch` (`watch_tool.rs`): a command's watch plans with an authority, so the gate asks
  once at every posture with the command shown (`task_graph::tools::authority_for` leaves the watch's alone), and
  nothing edits a watch afterwards (`wake.move`, `wake.snooze` and `wake.pause` refuse one); not in a task. The owner's
  `watch.create` and `watch.list` (`rpc/watch.rs`; `Act::Watch`, the owner from a private place, refused inside a job)
  and `theseus watch-for add|list|rm`. The commitments block lists a watch as `👁 c3d4e5 PR 123 merged? (OPEN at 14:05)`
  from its last frame's state. `[watch_for] default_every|min_every|max_days` (`config/watches.rs`). A `run` check needs
  L1, so a root daemon's refuses it (theseus-pv6i) and the watch fails with that reason. Tests: `watch/tests_watch.rs`
  (a real core: a file met within an interval, twenty unchanged checks write no frame, a planted secret scrubbed,
  three failures inform once, expiry, a restart, the cap), `watch/tests_watch_tool.rs` (the approval and the command
  that never changes, `then: turn`), and each pure module's own.
  **The `reply` check** (theseus-rlnbf; no store change): `{kind: "reply", place, from?, words?}` waits for a message,
  never on a timer. `watch/reply.rs` (the pure matchers, `Core::watch_reply_seen` and `watch_reply_met`, `Entry::replied`)
  and the board's place index (`Board::reply_matches`, kept by `insert`/`insert_new`/`remove`, so a restart rebuilds it with
  `watch_load`; one atomic load when no reply watch is open, one map lookup when none is for the place). The Discord
  binding calls `watch_reply_seen` for each message, before routing, reads only (`runtime/reply.rs`); a match is one
  frame (`watch.met` with the message id, author and `external: true`, the value the first 200 scrubbed characters) and
  a notice in a private place (`reply_target`: the watch's own place when private, else the owner's Home, never the
  shared place the reply came from). A reply watch only notices (`then: turn|task` is refused: a stranger's words are
  outside text and would drive a turn). `place` is a bound place's name, its target, or a channel's id (`reply::resolve_place`);
  `from` a user id, `discord:<id>` or a name (any case, contained); `words` text in any case or a `/pattern/`. Its seed is due
  at its expiry, which is its one look. Set by the owner's `watch.create` (the place rule is `Act::Watch`'s) and the model's
  `watch` tool (asks the owner once). Tests: `watch/tests_reply.rs`.
- **The Operations page's verbs** (theseus-jid7t.2; no store format change: META `session.profile.<session>`, three ledger
  kinds, `theseus_protocol::ops`; `rpc/session_profile.rs`, `rpc/task_rehome.rs`, `rpc/task_reschedule.rs`, `data_mark.rs`,
  `fact/ops.rs`, the kernel's `task_edit.rs`), each the owner's act (`session.profile`: `Act::Session`; the two task verbs:
  `Act::Task`) and none a Discord message (a card follows the kernel's records, as it does for any surface's change).
  **`session.profile { session_id, profile | null }`** holds a session to a profile: the META key (`{profile, was}`, `null`
  once let go) and its `session.profile_set` row are one frame. A turn's model: its own `profile` (not a pane's `carried`
  one), then the hold (`TurnRunner::sticky_named`, in `turn.submit`; `target_as_ran`, so a continuation, a wake, a task's
  next turn, keep-warm and the census read it too), then the last turn's, the place's and the live one; routing may move a
  turn that was not pinned, and the floor moves a refused base as it moves any (a judge-less core's private session fails at
  the provider's floor instead, as before). A turn fixes its model once (`route_to` routes once), so a running task takes it at
  its next turn, never mid-call. `null` puts back the target the session ran on when the hold began (`was`) and answers
  `previous`, which is the undo. **The refusal** is `data_mark::profile_refusal(cfg, catalog, mark, profile)`, pure over
  `refusal_of(mark, profile, known, origin, cleared)` (the origin picks the words only: a `us` profile that is not cleared is told the catalog does not list its model as of US origin), built on `uncleared` which `TurnRunner::uncleared_profile` reads too, so the menu
  and the route's floor cannot disagree (`turn/tests_sticky.rs` pins every mark x origin): a private or sensitive session runs
  only on a profile of US origin, an unknown profile runs nowhere, and a restricted profile on an open session is set with a note
  (the floor decides per turn). Nothing is written when refused. **The data mark** (`DataMark`: `open`, `private`,
  `sensitive`) is `session_mark`, read from `google::private::level`: the Google level of the session or any task above it
  (sensitive after a sensitive calendar's read, private after other Google content), else open. It rides `SessionInfo.data`
  and `WorkView.data` (stamped as a view leaves the board, `Core::stamp_data`; the board reads no store) and
  `ProfileInfo.origin` names each profile's origin word.
  **`task.rehome { task, home }`** moves where the report is posted (the task record's `target`, the execution's
  `reports_to`, META `task.place.<session>`; the frame that ends the task reads the target again,
  `TaskOf::target_for_now`): `home` is a conversation's session id or `thread:<slug>`; the budget stays carved from the
  original parent and the parent's next turn still reads the report. A home with no place leaves the report on the board.
  Refused: an ended task; a missing, retired, task or shared-place home; a home whose mark is weaker than the task's,
  or **whose mark cannot be read** (`data_mark::try_session_mark` over `private::try_level`, the one walk `level` uses
  too: a failed read is an error for the home, `Sensitive` for the task). A home with no place writes `task.place.<session>`
  as JSON `null` (not `""`: a build before this one reads a string and binds nothing from null; this build's rebuild and
  the index tender read null and `""` alike as no place). `session.profile` refuses a retired session (a let-go goes).
  **`task.reschedule { task, start_at_ms | start_after_ms | start_when }`** moves a sleeping deferred task's due wake
  (`Kernel::edit_task`) and its `task.deferred.<session>` times, parsed by `Core::start_time`, which `task.start` shares;
  `previous` is the undo. Tests: `rpc/tests_session_profile.rs`, `tests_task_rehome.rs`, `tests_task_reschedule.rs`,
  `tests_data_mark.rs`, `turn/tests_sticky.rs`, the kernel's `tests_task_edit.rs`. **The floor's table is shared with the cockpit**
  (theseus-jid7t.6): `cockpit/test/fixtures/profile_floor.json` lists every mark × origin × known case and whether
  `refusal_of` refuses it; `tests_profile_floor.rs` holds the daemon to it and the cockpit's `test/ops-floor.test.ts` holds the model
  menu's greying to it, so a change to the floor edits that file and both tests say where the other side must follow.
- **Starting a task with no model turn** (theseus-7oq0w): `task.start { brief, title?, home?, budget_usd?, profile?,
  start_at_ms? | start_after_ms? | start_when?, quotes?, report_to? }` (`rpc/task_start.rs`, `theseus_protocol::task_start`),
  the owner's verb for every surface (`theseus task start`, the TUI's `N`; Discord's "New task" and the horizon's deferred start
  call it). It is `Kernel::open_task_owned`, the opening `task.create` shares (`open_task_at`, both on `open_task_in`), minus the
  parent's turn: the parent is the conversation `home` names (not a task, not retired), else Home (`wake_home.rs`, made on first
  use). The task's execution, session, brief, graph record, quotes' arrangement, the carve and the `task.started_by_owner` row
  (`fact/task_start.rs`) are one frame; the model is not called, and the report waits on the parent (`wake_parent` is false, so
  it never costs a turn). The carve is `task.create`'s: `budget_usd`, else a quarter of what the parent has left; an explicit
  budget over what it has left is refused with the numbers (not capped silently), nothing left carves nothing. `start_*` opens
  it asleep (`task.create`'s bounds, `task_start::bounded`; `start_when` is `wake.create`'s `when`, read in the daemon's zone).
  `quotes` become one `Body::Arrangement` (role `context`) after the brief and hold the task's session as external text
  (`external::VIA_QUOTE`, the web text's hold and policy; a parent that already holds passes its own). Authority is
  `judge_act(Act::Task)`: the owner, from a private place; a shared place and an MCP client (`mcp_server::JUDGED`) are refused with
  where to start it instead, a job's shell by the CLI (`client::OPERATORS`). Tests: `tests_task_start.rs`, theseusd's
  `tests/task_start.rs`, the CLI's `tests/task_start.rs`.
- **`task.not_this`: the owner's "not this" on a task** (theseus-o1h3f.2, the feedback leg of notify-work-feedback; no store
  format change: a standing record and a judgment label that exist, no new kind; `rpc/task_not_this.rs`,
  `theseus_protocol::task_not_this`; routed with `task.cancel` by `task_stop`, `rpc/server.rs`). It is **not** the card proposal's
  `Keep/Edit/Not this` (`card.answer`, above) nor the capture ask's `standing.answer` `not`: it is a call about a *task*.
  `task.not_this { task, reason?, author?, discord? }` answers `{ task, preference, replaced?, labeled? }`: `task` is an id, an
  execution's id or the end of either, as `task.cancel` takes it; `reason` is the owner's words and rides the preference; `author`
  and `discord` are the surface's, as `task.pause`'s. Three things, in this order:
  (1) **it is judged first**, `judge_act(Act::Standing { method: "task.not_this", .. })`, which takes the verdict every act
  without an arm of its own takes: the owner, from a private place (`places::owner_in_private`), as `task.rehome` and
  `task.reschedule` (`Act::Task`) do; a refusal is ledgered `approval.refused` and leaves the task running and nothing written;
  the CLI refuses it inside a job (`client::OPERATORS`). (2) **It cancels the task with `task_cancel_by`**, the function `task.cancel`
  itself calls, so it ends and releases exactly what `task.cancel` does (Discord's Stop and the other `task.cancel` callers), with nothing of
  its own added; a task that has ended is left as it was and the rest goes on (the answer is not a refusal), and an unknown task is
  `NOT_FOUND` with nothing written. (3) **It writes the one rolling preference**: a `StandingKind::Preference` for
  `StandingScope::Everywhere` whose `source` is `task.not_this` (`theseus_protocol::task_not_this::SOURCE`), made with
  `standing.add`'s `replaces` naming the live record of that source, so there is one and never a pile (the answer's `replaced` is
  the id it ended). Its text is one line of prose: the task's shape, which is what the model said it started the task for
  (`TaskOrigin.because`), else the task's title, cut to 300 characters, and the owner's reason, cut to 400: `Not this: the owner
  turned down a task you started on your own for "<shape>". Do not start another like it. Their reason: <reason>`. **It is a
  preference, and only that**: words in the standing block that every turn's header layer carries (`standing::Block`, the records
  for everywhere), which the model reads and can follow or not. Nothing gates, counts, caps or refuses a start by it: the
  helper slots and the spend gates are as they were. The same words again leave the one record (`replaced` is none); two writers
  that race for the live one retry up to three times. (4) **It labels the offer**, best
  effort, because a task carries no link to the offer that produced it: the newest acting `offer_task` judgment in the task's
  origin session, found in the last `OFFER_SCAN` (2,000) ledger rows, is labelled false (`judge.label`, the note `not this` or
  `not this: <reason>`) and its id is the answer's `labeled`; none found, or a label that fails (warned), leaves `labeled` empty
  and the cancel and the preference stand. The core does not ask whether the task started itself: any task may be told "not
  this", and the Discord reaction alone is limited to the ones that did. Readers, one tap each: `theseus task not-this ID [REASON]`
  (the CLI's `task_start.rs`), the cockpit's Ops page (`u` pressed twice on a task, `cockpit/src/ops/keys.ts` and `verbs.ts`,
  the `not_this` verb), and a 👎 reaction on a task that started itself (theseus-discord's `runtime/task_react.rs`). Tests:
  `rpc/tests_task_not_this.rs` (the cancel and the preference; two calls leave one, the second's, naming the first as `replaced`;
  an ended task answered, and the same words again leave the one record; a stranger cancels and writes nothing; an unknown task is
  not found; the text is one line), `cockpit/test/ops-verbs.test.ts`'s verb row, the Discord crate's `tests_task_react.rs`.
- **Forks** (theseus-2qzx, the killer-features plan's R19; store format 32): `session.fork { session, at, title?,
  profile?, budget_usd? }` (`rpc/fork.rs`; the stored parts and reads are `fork.rs`) makes a conversation from any
  moment of another, by reference, in one frame: the new execution (the source's authority, a pinned limit of
  `budget_usd` else the daemon's), its session record (`forked_from`: the source and the last node taken), its first
  compilation (`inherits`: a run of the source's nodes through the fork point, `includes` empty, `trigger = "fork"`,
  the source's manifest with the thinking stripped, `derived_from` the source's compilation in effect at the node,
  found by walking `derived_from` back from the session's current one), the fork edge (`derived_from`, `via =
  "fork"`, into that compilation), the source's reverse entry (an `includes` edge from the fork's compilation into
  the source's *session*, one for the run: `EdgeKind::Includes`; `node.reach` reads it, `reach::forks_of`, as
  `forks`, generation by generation, a fork of a fork too) and a `session.forked` row. The source is read and never
  written. `at` is a node (as `reach::resolve` names one) or `@<WAL position>` (the last node at or before it); it
  must close its calls: a message of the person, or an answer that made no tool call. `Store::transcript` of a fork
  is the source's nodes through the point (`fork::prefix_records`, a fork's own prefix first) and then its own, so
  the compiler reads one list and `compiler::situation::selected` takes the run from `inherits`; a recompile, the
  ring or a compaction lists ids as any compilation does. `session.history` of a fork pages the same list (nodes keep
  their source's `session_id`: the CLI marks them). Authority: a caller on a surface that reads no private text may
  fork only a shared place's session, an MCP client only a conversation it opened (`Core::fork_allowed`), an imported
  session never; the fork keeps the source's external-text hold and speaks in no place (the CLI's: private), so
  nothing of it reaches a shared place. Cost: the fork reads the compilation chain back from the current one
  (`CHAIN` steps at most), never the source's nodes. Not here: files and `--worktree` (R20, theseus-23rk, which reads
  `forked_from` and the result's `not_restored`), and R21's surfaces (the TUI's `f`, Discord's "Fork from here", the
  Ship's mark), which call `session.fork`. Tests: `tests_fork.rs`, the layouts' format-29 samples, the CLI's
  `golden_fork.rs`.
- **Deferred tools** (theseus-7m7w.2; store format 33): `tools_defer.rs` and `tools_defer/`. Every tool belongs to a
  family by its wire name (`family.rs`: files, commands, web, memory, tasks, routines, threads, terminals, code,
  channels, aws, ask, projects, extensions, self, and `mcp:<server>`; an unknown tool is `other` and always loads, and a test fails a registered tool in it), and a
  request loads whole families. `RequestSpec.defer` is the turn's fresh choice (`turn/defer_step.rs`, once a turn:
  `Board::select` over the in-memory counts: a place kind's committed defaults (`PlaceKind::defaults`, never a
  benchmark's or a harness's name), `[tools.defer] always_loaded`, a family used in `load_share` of the kind's turns over
  `window_days`, a default unused `hide_after_days` hidden; `select.rs` is pure). `compile` resolves it through
  `tools_defer::forks`, the way it already forks the ontology's memberships: an **append takes the loaded set its current
  compilation recorded** (`Manifest.loaded`), so the request's tool bytes are identical turn after turn; a recompile, a
  clearing pass or a ring (all rendered from `recompiled`) takes the fresh set. A compilation with no `loaded` (before
  format 33) appends with every tool. On a model whose catalog entry has `tool_search` the hidden tools go out with
  `defer_loading: true` and the BM25 tool search tool is first in `tools`; elsewhere they are left out and `tools_find
  {query}` (`find.rs`, a read of the conversation's hidden tools; its answer names the family and says it is loaded now) is offered, and what it found loads
  **in the same turn** (theseus-342qy): `Board::wants` notes the families in `Convo.found`, the turn's next loop takes
  them (`defer_found`, after `checkpoint_apply`) into `Deferral.loaded` and `Deferral.added`, and since an append's
  `forks` takes `Deferral::over(recorded)` (recorded plus `added`), its tools differ from the compilation's and the
  compile recompiles (`tools_changed`; thinking before it is stripped as for any recompile). Tools lead the cached
  prefix, so that one request misses the cache either way; the new compilation records the families and the turns after
  append cache-warm. Where the provider has tool search the search already loads in the turn. `tools.find` is never in the
  catalog (`tools_defer::listed`) and joins a request only here. The tool search's blocks (`server_tool_use`,
  `tool_search_tool_result`) ride in the assistant node as the provider returned them (`scan.rs` reads them). **Misses**:
  `defer_unloaded` (before `run_calls`) answers a call to a tool the request hid, when no search of the conversation
  surfaced it, with `Not run: … search for it first` and a `tools.missed` row; a search is a `tools.searched` row (and
  whether a call followed in its turn); a turn's row `tools.used` carries the place kind, the families loaded, used,
  searched and loaded-but-unused, and is what the next selection counts. The counts (`board.rs`) are read from those
  rows once after serving (`Core::warm_tools_use`) and kept by each turn; the conversation's notes (surfaced tools,
  families asked for) are memory only (the oldest of 256 go). A server tool's streamed input (the search's query) is assembled
  into its block like a call's (`provider.rs`). The metric is `theseus.tools.events` by event (used, searched, missed,
  unused), place and family. `theseus tools` (over `tool.defer`), health's `tools:` line and the cockpit's
  Actions panel show it all. Per-message additions are the `extra` argument of `Board::select` (Jev's branches, later).
  **A model without tool search** (theseus-jpjcd) does not get `tools_find` alone: a small model never called it, so
  `tools_defer::want` loads, with the fresh choice, the hidden families the turn's message names (a tool's whole wire
  name, `aws_whoami`, or a family's name unless it is an everyday word), best first, while the whole first request stays
  within the place kind's budget (`tools_defer::fit::Fit`: the system, what deferring costs, the families already loaded and this
  one; theseus-3bs4e; 8,000 for cli, task and headless, 15,000 for dm and channel); a detour's is counted on the request it sends
  (`turn/detour_step.rs`'s `detour_spec`, which its compile takes too). The text is `RouteState.ask`, set at the inbound point; a model with tool search never reads it. It takes
  effect where a compile takes the fresh set (a first compile, a detour, a boundary): an append keeps its recorded set.
  **The AWS tools are six families by use** (theseus-3msy0; `family::aws_family`): `aws` (call, describe, whoami),
  `aws-s3`, `aws-logs`, `aws-review` (inventory, cost, trail), `aws-stacks`, `aws-hands`. The whole set is 6,869 tokens,
  more than any place can add, so a message that named `aws_whoami` loaded nothing; each family is at most 1,300
  tokens (`aws::tests::every_aws_tool_is_in_a_deferred_family_small_enough_to_load_alone` holds the cap: a tool added
  to a full family goes to a new one). A hyphenated family name is said when each of its words is, and its name's
  point is scored once per family. Evidence from before the split names `aws` for every AWS tool; a place that
  learned to load `aws` loads the generic three now, and the others learn from use again. The detour's test is
  `tests_route_defer.rs::a_detour_without_tool_search_reaches_an_aws_tool_within_the_budget`, run in the owner's DM since the whole-request
  rule: at the cli a detour's request is about 7,300 tokens before a named family, so no AWS family (900 to 1,300) fits beside it there and the
  model reaches one with `tools_find` (`tests_detour::a_detour_naming_a_family_stays_within_its_place_budget`).
  **A trivial detour carries `tools_find` only** (theseus-d9je7, the owner, 2026-10-22; `Deferral.bare`, `defer_step::Said`):
  its `loaded` is empty save a family the message names or Jev judged relevant (`want::extra`), and `resolve` keeps the
  find tool instead of falling back to every tool; its `context.detoured` tools part reads `trivial: tools_find only`.
  A quick detour keeps its place's set. On a model with tool search the set is unchanged (the API refuses a request whose
  tools are all deferred). Tests: `tests_trivial_tools.rs`; the budget-raise tests run on a quick detour, since a bare
  request leaves a named family room under the budget.
  Tests: `tests_defer.rs` (whole turns), `tools_defer/*`'s own.
  **A message that asks for a task loads `tasks`** (theseus-gg78p; `want::asks_for_task`): the DM defers the family (the
  owner's rule of 2026-10-14), and a model that never searched took `proc_run`'s `background` for "start two background
  tasks". A verb that makes one (`start`, `create`, `make`, `launch`, `spawn`, `begin`, `kick`, `new`) within four words
  before "task" or "tasks", or "background task(s)", admits the family for that turn, whichever way the model reaches
  the rest: with tool search `defer_step` adds it to the fresh choice, without it `want::extra` admits it first
  (`By::Asked`, held to `Fit`). "My tasks today" and "what tasks are open" admit nothing, and `PlaceKind::defaults` is
  unchanged. The family's catalog line points background work with a budget at `task_create`. Tests:
  `tests_dispatch.rs`, `want`'s `a_message_asks_for_a_task_only_with_a_verb_that_makes_one`.
  **Admitted families raise the cap, under a ceiling** (theseus-xlri5; `[tools.defer] admit_ceiling` 2.0, `judge_families`,
  `judge_wait_ms` 200): for a model without tool search, a family the message names, then one Jev judges relevant
  (`families.v1`, `judge/families.rs`, started at the inbound point when the session's model lacks tool search or routing
  is live, taken bounded from that start before each `defer_tools` by `turn/families_step.rs`, once a prompt, the
  conversation's last set kept on the defer board by the prompt's digest), loads past the place's budget by exactly its
  `Fit` cost while the whole stays under `floor(budget × admit_ceiling)` (`want::extra` returns `Admitted { family,
  tokens, by }`); learned families stay held to `budget + Fit.raised`. What passed the budget rides `Deferral.raised`
  (not stored), `tools.used`'s `raised` and `TurnSubmitResult.raised`, whose `line` closes `harness_lines`.
  `admit_ceiling = 1.0` and `judge_families = false` are the rollbacks. **By tier** (theseus-d9je7): `admit_ceiling` is an
  ordinary prompt's, `admit_ceiling_small` (1.0) a trivial one's and `admit_ceiling_deep` (3.0) a deep one's, from the
  turn's `RouteState.injection` when it is fixed before `defer_tools` (a verdict landed by then; else ordinary; a routed
  recompile takes the fixed one); the three alike are one ceiling for all. The census's fresh request is ordinary.
  The `families.v1` call is made on every routed
  prompt, a model with tool search included unless the route pack is below canary (the owner's decision of 2026-10-21:
  always try to categorize each toolset), so `families_start` is not narrowed. Tests: `tests_families.rs` (the two that
  read Jev's answer set `judge_wait_ms` to 2,000 in their rig, so the answer is waited for, not raced).
- **Your own benchmark** (own-benchmark, theseus-pw1q.12; store format 34; `own_bench/`, `compiler/cache.rs`,
  `judge/compare.rs`, `config/route.rs`, `fact/own_bench.rs`; a yardstick, not a self-change: it is not behind `[self]
  mode`, and nothing it finds moves routing): "which model you actually need, measured on your own past work".
  - **System blocks by digest.** A new compilation stamps each system block's SHA-256 into its cache layout
    (`BlockBreakpoint.digest`, `compiler::cache::stamp`) and writes each distinct text once to the blobs (`cache::keep`,
    one `put_many` on the blocking pool; `TurnRunner::keep_blocks`, called beside `persist_compilation` in
    `compile_step.rs` and `route_step.rs`). The request never waits on the blobs' syncs (theseus-nsp9l.4: two files and
    the directory, ~21 ms of the send path on the owner's disk): the batch starts once the call's plan frame is durable
    (`start_keeping` in `call_model`; started beside the frames, it slowed their syncs), and the turn waits for it once
    its body is done (`blocks_kept`), so a crash in between costs that compilation its replay (the missing blob is
    named), never the turn (`own_bench::tests::a_new_compilations_request_never_waits_on_its_blocks_syncs`). The digests
    are of the request's own system blocks, so a guidance edit or a changed context file is a new blob. A compilation
    from before format 34 has none, and a replay says so. Blobs follow the blobs' own rules (nothing sweeps them but a
    person).
  - **`turn.replay { session, turn, profile, effort?, scope?, compare? }`** (`own_bench/replay.rs`, the owner's run:
    `judge_act(Act::JudgeRun)`, in the CLI's `OPERATORS`; `theseus model replay`). `rebuild_first` (`rebuild.rs`) renders
    the turn's first request again from its compilation, the blobs, the tools (their digest against the manifest's) and
    the nodes before its first answer, trying the profile's effort and then each level, and accepts it only when its
    digest is the one the answer recorded (`request_digest`, taken before the recall, standing, task and wake views
    ride the last block); otherwise `NotRebuildable { part, why }` names the part (`system`, `tools`, `messages`,
    `compilation`, `profile`, `answer`). The replay is that request for the other profile's model (its thinking, its
    effort, `max_tokens` at most `REPLAY_MAX_TOKENS`), no tool run, reserved and held on the day ceiling as a
    consolidation call is, then judged by `compare.v1` (unless `compare: false`) and written as a `turn.replayed` row:
    no node, no session, no execution. Never a shared place's turn; and when the other profile's provider is not the
    original's (`Core::crossing`), never one whose session is in a sealed thread, whose recall carries a `personal` or
    `partner-confidential` imported source, or whose rebuilt messages carry an image or a document block.
  - **`compare.v1`** (`theseus-judge/packs/compare.v1.toml`, builder `compare`, point `replay`): the request's last
    user message, reply A (the original) and reply B (the replay), each with the calls it asked for named; one Choice,
    `same`, `better`, `weaker`, `different`, or `other`. `WIRED` in shadow, no action, awaited by the replay that asks
    it (`JudgeService::compare_replies`). Its holdout baseline is the owner's labels and route corrections; a version
    that moved routing would climb the ladder on it.
  - **The sampler** (`own_bench/sampler.rs`, `route_replay_after_serving`): two hours after the learning hour, a missed
    night once, never within 10 minutes of a start; before each replay it waits for the machine to be quiet
    (`pressure`). Per (model, effort) cell of the last seven days' turns (`turn.ended` rows and `route.decided`'s
    `effort_ran`), the newest `PER_CELL` (3) not yet judged, replayed on the rung down the routing table
    (`own_bench/rung.rs`: `[routing.classes.*]`'s profiles, a model once, by catalog price). The day's spend is read back
    from today's `route.replay.judged` rows (replay plus judgment, a failed call at its reservation) against `[route_bench]
    replay_day_usd` (1.00; 0 is off), and a replay whose reservation passes what is left is not made: the check is
    before the call (`CapReached`). Each result is a `route.replay.judged` row.
  - **`route.report { week? }`** (`own_bench/report.rs`, a read, any surface): per cell, the turns and spend, the
    replays and the ones `compare.v1` called same or better at `MATCH_BAR` (0.8), the turns whose routing the owner
    corrected (`route.corrected`), the one-sided Wilson floor at 90% of the match rate, the replays' cost against the
    originals', and `movable = spend × floor × (1 − cost ratio)`; the replays' cost today against the cap. The
    cockpit's Benchmarks view has a "Your work" panel over it; `theseus model bench [--week]`.
  The report's draft columns (theseus-a1wvq.12; `own_bench/drafts.rs`, read only): one `DraftCell` per (draft model,
  review model) from the `task.draft` rows (drafts, applied, reviewed, both costs, measured `lines_changed`, `total_usd`;
  the reviewer's reported "tests fixed" is never summed), and `draft_skips`, the turns per `draft_skip` word of
  `route.decided`; `theseus model bench` prints a line per cell and one `draft skips:` line, and nothing without them.
  Tests: `own_bench/tests.rs` (blocks, the rebuild's golden `first_request.golden.json`, refusals, the replay's row,
  `compare.v1` through the fake Jev), `tests_sampler.rs` (the cap, the rung, shared places, attachments),
  `report::tests`, `rung::tests`.
  - **Whole turns** (part (c), theseus-pw1q.20; no store format change: a ledger row's fields and a label's source;
    `own_bench/whole/`): `turn.replay { scope: "whole_turn" }` runs the turn's tools too, on a scratch copy of its
    directory, and judges what it did. `scratch.rs` copies the session's directory (`cwd_for(session.dir)`) into
    an owner-only directory under the temp dir, without `target/`, `node_modules/` or `.theseus-files/` (named in the
    row's caveats), refusing past `[route_bench] whole_tree_mb` or 200,000 entries, and removes it when its guard drops.
    `undo.rs` puts back the directory *as it was at the turn's start* by taking back, newest first, every recorded write
    from the turn on: `fs.edit` by its input's `old_string`/`new_string` (the new text must still be there exactly once),
    `fs.write` by `created` or by the whole diff its result carries; a write with no whole diff, an `fs.patch`, or any
    other writing tool refuses the replay with `NotRebuildable { part: "directory" }` naming the file (a program's
    changes are unrecorded: counted in the caveats, not refused). `hands.rs` answers each call: `fs.read|glob|grep|list|
    write|edit|patch`, `git.diff|log` and `text.diff` run in process on the copy (roots and cwd set to it, a plan naming a
    path outside it refused; the model's directory paths mapped into the copy and the copy's back out of results);
    `proc.run` runs in L1 over the copy as the only workspace (`theseus_kernel::probe_run::run_in`: a view from an empty
    root, writes discarded with L1's scratch, egress `[sandbox] egress`, environment `PATH` and `HOME`; a root daemon's
    L1 refusal fails the call, never L0); every other tool returns a stub: the original turn's own recorded result for
    that tool, else a sentence. `runloop.rs` is the loop (`Core::replay_whole`) under `[route_bench] whole_loops|whole_seconds|
    whole_usd`, each call reserved first as a first-loop replay's is (`call_model`, shared with `call_replay`), the cap
    named in the row. `compare.rs` is the deterministic half (the last test run's pass/fail by the argv's words, the
    set of paths changed, the set of exit codes) beside `compare.v1` on the final replies. `label.rs`: a judged match at
    `[route_bench] whole_label_bar` (0.9) with no cap, the same last test result and the same paths becomes a `judge.label` row
    on the turn's route judgment (question `mode`, the class whose profile list leads with the rung, source `replay`,
    weight `learning::REPLAY_WEIGHT` 0.4, keyed once per judgment and turn). The learning loop drops `replay` labels from
    every holdout (`learning::without_machine` in the report, `propose::gather`'s owner-labeled holdout), counts them
    apart (`LabelCounts.replay`) and uses them on the train side only; the route pack stays in `LEFT_ALONE` and a
    version trained on them still has to beat its baseline on the owner's labels. `route.report` has the columns
    (`whole_replayed`, `whole_matched`, `tests_same`) and `labels_fed`. Only the owner's `turn.replay` runs it; the
    sampler still replays first requests. Tests: `own_bench/whole/tests.rs`, `compare::tests`, `report::tests`,
    `tests_learning.rs`'s holdout and `tests_learn_loop.rs`'s train-only tests; a test sets `runloop::TEST_PROGRAMS`
    (a stand-in for L1, which refuses a root test runner) under a lock.
- **Rewind** (theseus-23rk, R20; store format 40): `rewind.rs`, `rewind/{restore,points}.rs`, `rpc/rewind.rs`. Before `fs.edit`,
  `fs.write` or `fs.patch` runs, the files its plan writes are kept (`rewind::hold`, in the tool's own closure on the CPU
  pool, `toolrun::run_inproc`): a regular file up to `MAX_KEPT` (1 MiB) is put in its own blob directory
  (`<store>/rewind/blobs/`, apart from attachments' so an erase deletes only its own), synced before the edit runs; a
  bigger one, a missing one and a non-file are noted (`Before::{Kept, Absent, TooBig, Unread}`). After the run one
  `PRE_IMAGE` record (kind 13, scope `rewind:<session>`) for each file that changed rides the call's completion frame
  (`ToolRuntime::complete`'s `extra`): the prior state, the digest of the bytes the edit left (`after`), the lines
  added and removed. So a plain turn writes the frames it did, an edit adds one blob sync before the edit and no frame.
  A crash between the blob and the edit leaves a blob nothing names; a crash after the edit and before the frame leaves
  an edit with no record, which a restore reads as a file changed since (listed, never touched). `files.restore
  {session_id, since_node, dry_run}` (node or `@<position>`) restores, newest change first, the first change's prior
  bytes of each file the session's edits changed after that position, **only where the file's bytes still equal
  `after` of its last row**; else it is listed ("changed since"), as are a copy that was not kept and a file over
  1 MiB. A file the session made is removed when it is still what the edit wrote. Files a program changed are in no
  row, so they are never touched (the result counts the `proc.run` calls since, in words). The restore's own changes
  are pre-images too (tool `files.restore`), so `files.restore {since_node: "@<undo_since>"}` undoes it; one
  `files.restored` row with the frame (owner's act: `judge_act(Act::Session)`, refused for a surface that reads no
  private text, which a file path is). `files.changes {session_id, node?}` lists the person's messages newest first
  with the files changed since each (a fork's list includes its source's edits through the fork point:
  `rewind::rows`) and `fork_at`, the nearest earlier node a fork can end at (none for the first message). The
  conversation's half is `session.fork` at `fork_at`; `theseus_client::rewind::run` and the chat call the two in turn.
  `rewind::forget` deletes a session's blobs that no other session's record names (called by `import.erase`; an
  imported session has none). Tests: `rpc/tests_rewind.rs`, `tests_layouts.rs` (the format-33 store and the first
  layout).
- **The speaking voice** (theseus-slvl.7; `rpc/voice_pick.rs`, wire `theseus_protocol::voice_pick`): `Core::voice_of(session)` is
  the persona in play (`TurnRunner::persona_of`, memory and one session read) and the Aura voice it speaks in (`tts_voice`, else
  `[voice] tts_voice`); `greeting_for` is `[voice] greeting` with `{name}`. `voice.show` reads both. `voice.audition` is the
  owner's run (`Act::JudgeRun`, in the CLI's `OPERATORS`): one WAV a voice (the default six, or at most 8 named; at most 500
  characters), made with the daemon's Deepgram key and sent back base64 for the CLI to write; it books to no session, so it
  is bounded instead, and no answer carries the key. Tests: `rpc/tests_voice_pick.rs` (a stand-in Deepgram).
- **Parked tasks** (28b; theseus-vug): `parked.rs`, health's `tasks.parked`: each task that cannot progress by itself,
  with its blocker, from the open executions alone (a question counts as progress for 24 hours).
- **A turn's time, its progress, and its commands' CPUs** (theseus-2uy56, theseus-yct32). **The deadline**: a
  `turn.submit` may carry `deadline_secs` (`theseus ask --deadline SECS`), kept in memory by session and the request's
  arrival (`turn/clock_step.rs`, `set_deadline` in `turn_submit`; the map's key is `(session, arrival)`, so a later submit of the session, a steer or a queued submit, never takes or clears an earlier turn's): the turn that submit started takes it at its first
  loop (`take_deadline`, which drops it), and no other turn of the session has one (a nudge, a wake, a resume, a
  continuation, a submit that steers a running turn; a restart forgets it, and the turn has none). **The time line**: at a loop's start, when the whole minutes left changed (and at the first loop), a harness
  node `[The harness: 9 minutes left of this turn's 15.]` (author `harness:clock`): appended history, never the prefix,
  at most one a minute. **The progress check** (`turn/progress_step.rs`, `decide`, pure): at each loop's end, before the
  checkpoint takes the loop's failure, a loop whose `proc.run` calls answered in the loop with no error result passed, one
  with a command's error result failed (another tool's error counts for the checkpoint, not here), a loop with no
  command or with a command sent to the background is no check (`progress_step::Commands`, set as calls answer by
  `Checkpoints::answered`). Nothing fires while a check passed in the last `MIN_LOOPS_WITHOUT_PASS`
  (3) loops; at `STEER_SHARE` (0.5) of the deadline, or of the execution's spend limit spent by this turn when the turn
  has a deadline and the limit only notifies (`Signals.guarded` is `spend_limit_mode = "ask"`: a guarded turn ends at its
  limit anyway, so spend alone steers nothing; the `turn.progress` row still reports the spend), one steer note that says
  it is not a stop (with a failing check it also asks for the smallest step that can make it pass; theseus-ikr75). Reading
  is progress until there is something to check (`State::end_loop`): until the turn's first answered `fs.write`, `fs.edit`
  or `fs.patch`, a loop with no command adds nothing to `since_pass`; after it, a loop whose reads (`fs.read`, `fs.list`,
  `fs.glob`, `fs.grep`) all answered and opened a key not seen before resets it like a passing
  command (the seen keys are on `State`, bounded by `SEEN_MAX`; past the cap nothing is new, so a full set cannot reset the
  count on every read). The key is tool, path and pattern, and for `fs.read` the slice's first line (a bare read is
  `offset=1`, as it returns from line 1) and its pages: the narrow-read hint's slices of a long file are new ground at
  each new start, and the same slice again is a re-read (theseus-tdkcn with theseus-ikr75). At `STOP_SHARE` (0.85) of the
  deadline only (spend never stops a turn: the owner's notify rule), a stop note asking for a report with no tool calls,
  and the next loop is the last (stop reason `progress_stop`, which the nudge and the end note leave alone). **The stop
  counts apart from the steer** (`since_check`): it moves as it did before reading counted, one more for every loop that
  was not a passing command, reset only by one, so a turn that only reads, or opens new files after its edit, is still
  stopped at its deadline (the deadline's stop is the only thing that enforces it). Each is a harness node
  (`harness:progress`) and a `turn.progress` row (the shares, `since_pass`, `since_check`, `failing`, the times and spend;
  `fact/progress.rs`). The turn's state is a field of its `Checkpoints` (turn.rs
  is at its ceiling), begun at its first loop and gone with the turn. **The CPUs**: the session block names them after
  `Directory:` (`cpu/commands.rs`, `Cpus::of_commands`, read once per process): the tightest of the daemon's cgroup v2
  `cpu.max` (every ancestor's, rounded up), its `cpuset.cpus.effective`, and `available_parallelism`, with the host's
  online count when a limit applies (`CPUs available to commands: 2 (cgroup quota; the host shows 32).`); an unreadable
  file is no limit. Commands run as the daemon's children, so its cgroup is theirs. Tests: `tests_turn_progress.rs`,
  `turn::progress_step::tests`, `turn::clock_step::tests`, `cpu::commands::tests`.
- **A running build is progress; the deliverable hint; the doubt** (theseus-duzha, theseus-eb4o4, theseus-rqu7j; the
  first on `turn/progress_step.rs`, the others on `turn/close_out_step.rs`). **Held loops** (duzha): a loop that is no
  check and sent a command to the background, or waited on or read a job (`job.wait`, `job.read`, answered without
  error) while a job of the execution ran (`TurnRunner::running_jobs`, the kernel's outstanding calls, read only for such
  a loop) or had ended since the last such loop (`State::running`), leaves **both** counts, `since_pass` and
  `since_check`, where they were: a 40-minute build that the turn waits on must not be steered at half the deadline or
  stopped at 0.85 (the stop reads `since_check`). A passing or failing command, a read of new ground, and the pre-edit rule
  keep their precedence; a loop with no job call counts as before, so the steer and the stop still fire on a turn that
  makes no progress. The second loop that reads a job that ended counts again. **The deliverable hint** (eb4o4):
  from the loop after the turn's first answered file edit, one tail block (`close_out_step::DELIVERABLE_HINT`, through
  `Self::attach_hint`, which `coding_hint` also uses) says to call what was built once, the way the task spells it; a turn
  that wrote no file, or only ran commands, carries none (the DM task flow's golden and the keep-warm tests read such
  tails). **The doubt** (rqu7j): at a would-end whose closing message `names_a_gap` (a phrase list over its lowercased
  text, cut on 764 earlier closing messages: it matches 21% of the solved ones and 49% of the failed), in a turn that edited or ran a command, with under `CLOSE_OUT_SHARE` (0.4) of its time and of its spend used,
  once, outside a shared place and a strict loop cap, with no job of the turn running, one harness node
  (`DOUBT_TEXT`) and one more loop, after the stop nudge and the end note (it does not yield to them); a `turn.progress`
  row with decision `doubt`. It builds and installs nothing. Tests: `tests_turn_progress.rs`, `tests_end_note.rs`,
  `turn::progress_step::tests`, `turn::close_out_step::tests`.
- **Limits that notify** (theseus-usei; the owner, 2026-10-07: for tasks and tokens, notify, not restrict):
  `turn/budget_step.rs`. With `[kernel] spend_limit_mode = "notify"` (the default) a provider call past the
  session's limit goes out (`Kernel::overdraws`, theseus-kernel's `overdraw.rs`), and the loop whose call took the
  spend past the limit, or a multiple of it (`crossed`), posts one `notice` to the session's place, a
  `budget.reached` row and a narrative line, all in the turn's next frame; `[profiles.*] on_max_loops = "notify"`
  does the same at `max_loops` and each multiple (`loop.cap_reached`), and the turn goes on. `"ask"` and `"end"`
  are the old behaviour. A multiple is noticed at the check after the settle that crossed it (each loop's start and
  its call's settle, whatever the call's outcome, against `SpendMarks`, the spend each execution's last check saw, in
  memory), never skipped to the next. At `[kernel] spend_limit_hard_multiple` (10; 0 never) times the limit the
  kernel refuses a provider call whose reservation would pass it (`KernelError::HardStop`, `overdraw.rs`), and the
  turn fails `spend_hard_stop` with the stop's words (theseus-jse4; tests `tests_money.rs`). Still asking: a pinned limit (a place's ceiling, an MCP client's), an AWS hands group, and
  the background day caps. Under notify a task's carve is what it asked for up to the parent's whole limit, and
  speech and transcripts are not refused for the limit. The budget question's words are `turn/budget_words.rs`.
  Tests: `tests_limits_notify.rs`, and the kernel's `tests_overdraw.rs`.
- **The day ceiling** (theseus-kp20, softened by theseus-9e02; the kernel's `day_ceiling.rs` is the counter):
  `day_ceiling.rs` reads today's model spend back before serving (`read_today`: the ledger's pages by kind from local
  midnight, provider settles, booked reservations, speech, synthesis, the learning runs, the judge's day record, and
  today's `spend.ceiling` and `spend.day_warning` rows; theseus-vqcgq: the start's read is `read_today_total`, which reads
  only the rows past a stored total, the META record `day_ceiling.total`, that `Core::keep_day_total` moves forward every
  minute, so a start costs the rows written since, not the day's; no usable total is one full read, once, and `read_today`
  is that full read, which the total must equal: `tests_day_total.rs` counts records read, never time), and says a refusal (`TurnRunner::day_refused`: the day's first
  writes one `spend.ceiling` row and, with a DM bound, one `spend_ceiling` post to the owner's DM, which the courier
  sends nowhere else; the start reads the row back). `[kernel] daily_spend_ceiling_mode` says what reaching the ceiling
  does: **`notify`** (the default; the owner, 2026-10-11: tokens less cautious, AWS more) refuses nothing: the check
  after each loop's settle (`spend_checked`, one comparison) takes the day's warning (`DayCeiling::take_warning`, once at
  the ceiling and once at each `daily_notice_step_usd` after it, a level a day) and writes one `spend.day_warning` row, one
  `spend_ceiling` post to the DM and one narrative line, all in the turn's next frame (`TurnRunner::day_warned`,
  `turn/ceiling_step.rs`); health's first line and the cockpit's Money view show the day past its ceiling
  (`HealthResult.day_spend`, `DayCeilingBudget.mode/past/runaway_usd`). The day stops only at `daily_runaway_usd`
  ($2,000; 0 never): the refusal and its sticky rule below are then the runaway line's (`Reached.runaway`, its own words
  and class `daily_runaway`). **`stop`** is the money row's: the ceiling refuses. A turn's refused call fails it, class
  `daily_ceiling` (or `daily_runaway`), and leaves a harness note the next context reads (`turn/ceiling_step.rs`); the
  judge's day budget holds on it (`ShadowBudget::set_ceiling`), and consolidation, the audit, replay, backfill and the
  learning writer hold through `TurnRunner::day_hold`; compaction's summary is refused through the kernel and the ring
  runs; speech asks before its call. The stop is sticky (theseus-l9on): after a refusal every call that day is refused,
  however small, until local midnight or a higher bar at the next start; the words say a refused turn or task ends (it
  does not wait). **The burn-rate brake** (`[kernel] runaway_usd_per_10min`, $200; 0 never): a settled provider call
  fills a small in-memory ring per execution and per ancestor (`DayCeiling::burn_record`, at the settle); a call planned while the last
  ten minutes' spend has reached the line is refused (`KernelError::BurnRate`, `Kernel::burn_stopped`, nothing written),
  and the turn asks the owner to resume as the budget question does (`Called::Braked`; the question's `args.call.brake`
  makes its words, `budget_words.rs`); approving it (`reset_budget`) keeps the spend and empties the window. The bench
  drivers set the ceiling and `daily_spend_ceiling_mode = "stop"` explicitly (bench/harbor's `theseus_bench.day_ceiling`,
  bench/recall's `--day-ceiling`), so a run never spends past its own budget. AWS keeps its stops: only model tokens went
  soft, and `[aws.accounts.<id>] monthly_warn_usd` (default half the month's budget) adds one warning once a month
  (`aws/month_warn.rs`, from the budget's six-hourly read). Tests: `tests_day_ceiling.rs` (the stop mode, which names
  it), `tests_day_total.rs`, `tests_day_notify.rs`, and one each in `tests_compaction.rs` and `tests_consolidate.rs`.
- **`turn.inspect`** (theseus-x9swu, `rpc/inspect.rs`, wire `theseus_protocol::inspect`): one turn whole, for the cockpit's
  inspector at `/session/<id>/turn/<turn>`. A read, on the blocking pool: no frame, no record of its own (store format
  unchanged), nothing sent to the model. It finds the turn's `turn.started` row by paging the session's `ks:turn.started`
  rows back (it also names the previous and next turn), then reads the session's ledger rows and nodes through the tag
  index from `LEAD` (400) positions before it, filtered by `turn_id`, and the answers to the turn's floor questions
  (`action.confirm_answered` and kin, written with no turn, found by the question's `correlation_id`; `answers_of`).
  `assemble` is pure: loops from the assistant nodes (`loop_index`; a call is tied to its loop by its `assistant_node`),
  each loop's `context.compiled` row (digest, sizes, cache breakpoints and layer digests), the calls with their gate
  records and results, `added` (the nodes a loop's request carries beyond the one before's), the totals, the `route.decided`
  row (else the target `turn.started` names) and the `turn.trace` span tree. **What is stored and what is not**: the
  assistant node holds each loop's content blocks verbatim (thinking in full), usage, cost and request digest; the tool
  nodes hold the input, the gate record and the result as the model saw it; a request's system text is in no record (a
  compilation keeps its digest, and the compile row its layers' digests), so the cockpit rebuilds the parts through
  `context.explain`, which says whether they are still the bytes the turn saw. Thinking, a prompt, a tool's input and
  result, gate records, rows' data and the spans' attributes go to a private place only (`Surface::reads_private`): any
  other surface gets shapes, sizes and times and `withheld` says why. The link in a footer is
  `TurnSubmitResult.inspect_url`, set where the result is built from `[cockpit] public_url` (`config/cockpit.rs`; left
  out, `http://localhost:<[web] port>`, settled once at load by `CockpitConfig::resolve`, theseus-ypnxt.5; no link at
  all when the web UI is off and no `public_url` names one, `CockpitConfig::turn_link`, so `turn.inspect`'s `url` is
  empty, theseus-40cuo); every renderer shows that string. Tests: `rpc/tests_inspect.rs` (a multi-loop turn with
  parallel calls, thinking, a compaction and a floor question; the privacy; the 20-loop, 40-call read, p95 under 150 ms
  in a debug build; no frame written), `config/cockpit.rs`'s.
- **`ledger.window`** (theseus-m2tiu, `rpc/window.rs`): the postmortem's read. One page by the kinds' own postings
  (`POSTMORTEM_KINDS`, or a session's tag when narrowed to it and the tasks below it, found from `Kernel::tasks`),
  oldest first, `next` the cursor; each row's strings pass the scrubber. A surface that reads no private text
  (`Surface::reads_private`) must name a session and gets that session's rows and the daemon's, never another's. It
  writes no frame and takes no session or execution lock. The index counts a row at the time it was written, so the
  window's own times then decide by each row's `at_unix_ms`. Tests: `rpc/tests_window.rs` (a busy day's window, 6,000
  report rows in 13 pages, read in 0.14 s in a debug build).
- **The ledger's hash chain** (theseus-bmkxk, store format 31): the store's writer chains every ledger row
  (`theseus-store`'s guide); core reads it. `ledger.verify` (`rpc/ledger_verify.rs`, a read: no frame, no lock, on the
  blocking pool; `theseus ledger verify|head`) walks it with `theseus_store::chain_walk::verify` and answers the head and
  the first broken link, no row data. Health's `ledger_head` is the writer's own head, short. `ledger_pin.rs`: a store
  from before the chain has its old rows pinned once, after serving, on a `ledger-pin` thread that paces itself and holds
  the core by `Weak` and begins `START_AFTER` (2 s) after serving (a thread holding the store at a stop right after serving keeps its close from checkpointing: `early_signal` fails; `ledger.history_pinned` and the META key `ledger.history_pinned` in one frame; the chain's start
  row counts them, so the start path reads no history). The durability tender puts the head (`SEQ:DIGEST`) on each
  segment's and tail's index row and in `durability.shipped` (`Hooks::ledger_head`), the anchor for `verify --anchor`.
  Tests: `ledger_pin::tests`, `tests_layouts.rs`, `rpc/tests_ledger_verify.rs`, `aws/tests_durable.rs`.
- **The operator's two reads** (step 42a, theseus-ext.7): `rpc/budgets.rs` is `budget.list`, each open execution's
  money from its record (`Kernel::open_executions`), where its limit comes from (`config`, `place`, `carve`, or
  `pinned`), its session's lifetime cost, and its last reset from one small ledger page by kind and session tag,
  never a scan (`last_reset_unread` while the index's shape is built); tasks under their parents, and totals of the
  top rows, since a task's spend is its parent's too. Tests: `tests_budgets.rs`. `rpc/explain.rs` is
  `policy.explain`: for a session's place (or the CLI and every bound place), each tool's layers in the gate's
  order, built by running the gate's own order (`toolrun/order.rs`: `ToolRuntime::refusal`, then
  `ToolRuntime::order`, which hands its watcher the decision after each layer) on a call inside the roots; the
  call-dependent layers (the floor, the lists, the roots, a private address, AWS, L1, grants) are conditions with
  their entries. Never copy the order into it: change `order.rs`, and both follow. Tests: `tests_explain.rs` (every
  tool against the order on a real plan, and against whole turns' recorded gates).
- **`store.rs`** is the kernel's view of storage: `Store::for_turn`, a session's writes, and the turn's transcript.
- **Tiering** (M6 step 33, theseus-6fn.13): a turn decodes only what it renders.
  - `node_cache.rs`: the heat cache, `Arc<Node>` by WAL position, one per store and shared by every handle, so every
    reader of nodes (a turn's transcript, `session_nodes`, `get_node`) decodes a
    node once. Bounded by `[memory] node_cache_mb` (64; 0 off) in record bytes; past it, `decay_sweep`'s hints go
    first, then the coldest by last touch and count. It is the store's read path: it serves with memory off. Health's
    `store.node_cache`, the metrics `theseus.node_cache.*`.
  - `stub.rs`: `Transcript` is `Vec<(u64, Stub)>`. A stub has its record's id, kind, origin, turn, and a summary's
    range's end from a peek that skips the payload; it derefs to its `Node`, decoded at the first touch (or taken from
    the cache). So **a walk over a whole transcript reads stub fields** (`n.kind`, `n.id`, `n.origin`, `n.turn_id`,
    `n.summary_last`, `compiler::renderable`, `compaction::is_summary`) **and touches `n.body` only for the nodes it
    needs**, else it decodes the session. A closure typed `|n: &Node|` over stubs derefs each one: type it `&Stub`.
    `context.compiled`'s `decoded` and `stubs` say what a compile read and what it left.
- **A long session's turn** (theseus-0l3g): its cost follows what it renders, not the session's length.
  - `transcripts.rs`: each session's transcript as last read, kept across turns as cold stubs (fields and position,
    never bytes nor node; at most 256 sessions and 400,000 stubs), so a turn's first read scans only the session's
    records after the last one it saw (`Store::scan_kept`). Debug builds still compare it with the store's whole read.
  - `compiler/ring/sums.rs`: the ring's cut before a user message is guessed from the segments' census sums, walked
    from the last cut back (each segment rendered once), then settled by rendering the guess and its neighbour: the
    earliest cut whose own render is under the target, as the walk over every cut picked it while a later cut never
    estimates more. After a start the ring decodes what lies past the stored cut. Tests: `compiler/tests_ring_sums.rs`.
  - `compiler/render.rs` and `compiler/kept.rs`: the render is a state nodes are rendered into, kept per session
    across loops (`[context] kept_mb`, 64; 0 none) and taken up only when its key (compilation, strip, provider, model,
    media, refused images, the cold rewrite's view) and its nodes' ids match and it was closed (no repair, no cut answer
    open, nothing retried). Every request is byte for byte a whole render's: `compiler/tests_kept.rs` holds it over
    generated sessions. Change `render::State::item`, never a copy of it.
  - `provider/digest.rs`: the digest and the body sent are written from the request, with no copy of it.
  - `compiler/phases.rs`: `THESEUS_LOG=info,theseus_core::compiler::phases=debug` logs each compile's phases.

## Where the big things live

- **Kernel transitions and frames**: `crates/theseus-kernel/src/kernel.rs`, and `tx.rs`. Every mutating method writes
  one frame.
- **The store's WAL and index**: `crates/theseus-store/src/wal.rs`, `index.rs`, and `store.rs`.
- **The turn loop**: `crates/theseus-core/src/turn.rs`, with `advancer.rs`; the harness loop in `harness.rs`, and
  what it drives (continuations, the heartbeat) in `rpc/driver.rs`.
- **The compiler**: `crates/theseus-core/src/compiler.rs` and `context_files.rs`.
- **Tool calls**: `crates/theseus-core/src/toolrun.rs`; the gate's postures in `policy.rs`, `external.rs`, and
  `broker.rs`.
- **The RPC layer**: `crates/theseus-core/src/rpc/` (`server.rs`, `methods.rs`, `driver.rs`, `confirms.rs`).
- **Facts**: `crates/theseus-core/src/fact/`: every ledger row, notification, narrative line, and span of the turn,
  its tool calls, answers, and the driver, one type each, recorded once (theseus-j6qn). The ledger's kinds:
  `crates/theseus-protocol/src/ledger.rs`.
- **A running job's output, on demand and live** (theseus-e1hu4; `rpc/peek.rs`, `push/last_line.rs`,
  `toolrun/peek.rs`'s `newest` and `last_line`; `last_line` returns the last *complete* line, ended by a newline or a carriage return, so a sample never shows a line the job is still writing, while `peek` shows the live tail as it is). All read the job's output file, `results/<id>.out`, which its wrapper
  writes while it runs; nothing is timed on a turn's path and no frame is written. `work.peek` (a read, on the blocking
  pool): a stat and at most 64 KiB from the end, scrubbed, at most 20 lines, each cut to 1,000 characters.
  `content.unlabel {session_id, flag}` (theseus-9ymz0, `rpc/content.rs`): removes one content label from a session as `policy.trust` clears a hold: `judge_act(Act::Unlabel)` (the owner, from a private
  place, never a job's process; its refusal is the session's `approval.refused` row naming the method), then the record and the `content.removed` row in one frame under the session's lock,
  the removal kept in `ContentFlags::removed`, a narrative line and `session.unlabelled` to the session's watchers; a flag the session does not carry is an error and writes nothing.
  `Core::tell_label` is the one `content_label` outbox post when a label is raised (the Discord message and its Remove button). `fact/content.rs` holds the four rows (`content.judged`,
  `content.labelled`, `content.removed`, `content.unjudged`). `session.list` carries each session's `labels` (`label_view`).
  `job.stop {id}` (p2f-ops-act, theseus-xzf71): the model's `job_stop` body, `toolrun::jobs_tools::stop_job`, for a surface, under `job_for`'s place rule; it answers the verdict's words only (`ended`, `verified`, `words`), and a job that had ended says so. Only from a trusted channel (theseus-bglzl): `judge_act(Act::Stop)` asks `places::owner_anywhere` first, before `job_for`, so the CLI, the web UI and the owner's own Discord account stop a job in any place and an untrusted caller is refused (`REFUSED`, `approval.refused`) whether or not the job exists; `JobStopParams` carries `author` and `discord` as `PolicyTrustParams` does. Not in `client::OPERATORS`: a stop is protective, like `task.cancel`.
  `action.output {id, from?}`: a follower task for this connection (`SessionBus::follow`, aborted by `action.unfollow`
  and by the connection's end) that holds the file open, so a job that ends and is swept still reads, looks every
  100 ms, and sends whole lines (a longer one at 32 KiB) as `action.output.chunk` with their byte offset, then
  `action.output.end` once the job has settled. It never lags more than 4 MiB (it skips, and the offset shows it), and
  stops queueing for a client whose queue holds 64 messages: it counts the stream (`output:<id>`) lost
  (`Outbound::lose`), so the connection hears `events.lost` as the queue drains, and skips to the end; the client
  peeks and follows `from` the peek's `bytes`; the old follower's chunks still in flight come before that follow's
  answer and begin before the peek's end, so a client drops them (the CLI and the TUI trim by offset). A connection that reads no private text is told "no job" for a private
  session's job (the answer of an id that does not exist). `doing.last_line` of a running job: a task on the runtime
  (`Core::sample_last_lines_after_serving`, held by `Weak`) walks the board's running jobs once a second while there
  are any (it waits on a `Notify` the push's applier signals when a frame leaves one running), reads the last 4 KiB of
  each job's file on the blocking pool, scrubs it, cuts it to 120 characters and says it only when it changed. Jobs of
  a shared place's session are never sampled, and a guarded `work.watch` scope drops the field. A sample is no frame:
  its view keeps its position and takes the sample's time, so a client applies a view by (position, time). Past the
  file's head (`[tools] job_output_max_bytes`, 64 MiB less 4 MiB) the newest output waits in the wrapper's ring and
  the file stops growing: the last line, a peek and a follow stop at the head until the job ends. Tests:
  `rpc/tests_peek.rs`, `toolrun/peek.rs`'s.
- **The work board** (theseus-hkx30; `push/work.rs` and `push/work/`): one `WorkView` per conversation, task, plan step,
  job, open question and pending wake, in one tree, kept as a projection of the frames the push already sees and
  written nowhere. It is fed by the push's one applier pass, after the execution board (`push/board.rs`, the old
  `push.rs` board moved whole) has taken the frame: its changed `ExecutionView`s, and the frame's ACTION, TASK, SESSION
  and allow-listed LEDGER records (`work::wanted` finds a row's kind a few bytes in, on the committing thread; the rows
  are `turn.started`, `loop.ended`, `tool.job_started`, `tool.confirm_requested`, `action.*`, `wake.*`, `task.*` and
  `execution.queued`). The observer also sees what the core writes beside the kernel's frames: `Store::observe` calls it
  for the core's own writes (a turn's rows, a session's and a task's record), and for the rows a turn defers that ride a
  kernel frame (the kernel's observer has its records, the store's the waiting rows in front). `views.rs` builds a view
  from its entity's inputs; `settle.rs` compares it with the one held (`same_work`), prunes the ended (the 50 newest
  per root stay, older ones leave in `gone` and stay counted in the root's `below`), rolls the ancestors up (`below`,
  `progress` from the plan steps), and asks `notices::policy` for a notice per changed view, at most one a frame, kept in
  a ring of 4,096 (`notices_since`). One entity per action: a question's view has the correlation id as its id and, once
  approved, the same id goes on as the job. A question is told by its own view only (`quiet_here`), a wake by the
  conversation's `woke`. `seed.rs` seeds from what the execution board's seed read (no settled action), and `warm`
  fills jobs' commands and usual times (`eta.rs`: the last 16 durations per program key, median) after serving, in place:
  a client applies by the position rule, so the next frame sends those views again (`stale`). `place.rs` is the place
  rule: a connection that reads no private text names its roots, hears only a shared place's (or, for the Discord
  binding alone, a conversation that speaks in a Discord place of any class: `rpc/work_board.rs::may_watch`, theseus-2j2ka;
  it draws that place's board in that place), scrubbed, and cannot answer. The methods are `rpc/work_board.rs` (`work.watch`, `work.unwatch`, routed through `rpc_prefixed`), the bus
  counts the watchers (`work_watchers`, `interactive_watchers`), and `theseus work` reads them. There is no timer: a
  question's reminder (rule 2 of the policy) is not made. **The daemon's own work** (theseus-jn17m; `push/work/system.rs`):
  one root, `system` (`Theseus`, kind `System`), whose children are the system jobs of the last hour, made from rows the
  daemon already writes, in the frames it already writes: `import.batch` (a client's `batch` and `batches` ride the
  params and the row, and the tag's source; the last batch, `batch == batches`, ends the job; the sync's batches carry no
  total and its `import.synced` ends them, with its error when it failed), `import.erasing` (an erase's frames, the
  progress; `import.erased` stays the receipt, which what counts erases still counts), `import.topics` and `import.people`
  (`done` of `of` records), `synthesis.proposed` (`run`, `n`, `of`: a cluster's place in its consolidation) and
  `index.rebuild` (`rpc/index_rebuild.rs`: the owner's `index.rebuild` through the core, `requested`, `progress` at each
  fifth of the WAL's backfill, `finished` or `failed`, by a task on tokio's timer that reads the tender's status every
  3 s while one is outstanding). A job is a title (`Importing Claude Code history`), a count (`Doing.label`: `batch 41
  of 120`, `14 clusters`, `41%`; `progress` carries the numbers and the unit) and a time; `protocol::work_system::line`
  writes them as `title · count · 3:12`. The board has no timer, so a running job whose rows stopped for `QUIET_MS` (5
  min; a rebuild 30) is cancelled at the next frame, and an ended one leaves an hour after it ended (`sweep`); after a
  start `warm_system` reads the last hour's rows again (the warm-up, after serving, no notice). The policy's system arms
  (rule 8): a failed job is an Inform with its reason, one that ends after `long_run_ms` an Inform, progress nothing.
  A failed job is at rest (`Idle`), never needs-you. **System work is the owner's**: `Scope::sees("system")` is false
  for a guarded scope whatever it names, and a private scope has it on the whole board or by naming `system` in `roots`
  (the TUI and `theseus status` ask for it alone). Tests: `push/work/tests_system.rs`, `import/tests_system.rs` (frames
  per import unchanged), `tests_push/work.rs` (over `work.changed`). Tests: `tests_work.rs` (the scenario and its golden,
  counts the watchers (`work_watchers`, `interactive_watchers`), and `theseus work` reads them. The question's reminder (rule 2 of the policy) is
  made on the driver's tick (`WorkBoard::remind`, theseus-j7cje); there is no timer. Tests: `tests_work.rs` (the scenario and its golden,
  `tests/golden/work_board.txt`), `tests_push/work.rs`, `push/work/tests.rs`, `push/tests/work.rs` (the seed equals the
  replay).
- **A task's forecast** (theseus-zz752; `forecast.rs`, `forecast/`, `push/work/forecast.rs`, `rpc/forecast.rs`): what
  tasks like it cost and took, before it runs ("about $0.60 and 8 minutes, at most $1.50"), one notice past the p90, and
  a calibration. `forecast::Book` (`ToolRuntime::forecasts`) holds the newest 2,000 finished tasks in memory (class,
  the hashed words of title and objective, dollars, minutes, loops): the work board adds a task as it ends
  (`note_ended`), and a thread after serving reads the rest from the task executions and the task graph
  (`read_history`, from `Push::warm_work`; nothing is stored, no frame). A forecast is a memory read, never a model
  call: the task's class (`class_of`: `open`, `checked` or `specified` by its acceptance lines, which an arrangement's
  pieces fill), then the 25 nearest briefs by shared words (Jaccard; the newest on a tie), p50 and p90 by
  interpolation; fewer than five like it is `Outcome::Few(n)` ("no forecast yet: 2 like it"), and a table not yet read
  is `Loading`. A forecast is said once per task record (`Book::for_task` memoizes), so `task.create`'s first line
  (`forecast::first_line`, ahead of "Started task …") and the task's `WorkView.forecast` are the same numbers. The view
  carries numbers, `past` and Jev's latest `loop.v1` read (`work_state` and its confidence, from `judge.call` rows that
  `work::wanted` lets through for the loop pack alone); no brief, title or id of the tasks it is made from, so the place
  rule has nothing to hold back. `past` is the board's sticky bit (`mark_past`, in `settle::put`): dollars over the p90
  or minutes since the task began to run over it (a deferred task's sleep does not count), whichever first; the policy's
  rule 11 fires on the change to true, an Inform with Keep going (no call) and Stop (`task.cancel`), and a seed after a
  restart reads it already true (`forecast_open`), so it is told once per task and not again. The board has no timer: a
  task that passes its p90 between two frames is told at the second. "Route down" is not offered: `profile.use` is the
  daemon's live profile, not a session's. The forecast never stops a task. `forecast.calibration` (`theseus tasks
  forecast`) backtests the table: each task that finished in the last 7 and 30 days against the forecast the tasks
  finished before it opened would have given, by class, on the blocking pool. Tests: `forecast/tests.rs` (synthetic
  histories), `push/work/tests_forecast.rs` (whole cores: the line, the view, the notice once, a restart, the place),
  and the protocol's `tests_notices.rs` row 11.
- **The future on the board** (theseus-nuhsz, horizon HZ8; no store change; `push/work/horizon.rs`, `rpc/agenda.rs`): a wake is a
  `Wake` view with `purpose`, `next_at_ms`, `series` and `paused`; the wake of purpose `Routine` is a `Routine` view and `Watch` a
  `Watch` view (id: the wake's; parent: the execution; the routine's runs are its children, so a run hangs from the routine, not
  from its execution). A routine is `Ready` between runs, `Running` while a run's task is open, `NeedsYou` after a failed or blocked
  last run until it runs well, is paused or deleted (a dismissal needs a record, so none is built), `Waiting` while paused; its
  `RoutineView` reads the routine's META record (a frame's payload that begins `{"session_id":"` and holds `"wake_id":`),
  `wake.fired`'s `missed`, `wake.skipped` rows, and the runs the board still holds (30-day counts count the ended runs kept, the 50
  newest per root). A watch's view reads `watch.checked|met|expired|failing` rows (the seed: `watch_state`), and ends `Done` or
  `Expired` by them; its `next_at_ms` is the time it gives up (the next check is in the checker's memory). A deferred task's
  `next_at_ms` is its `DueAt` wait. The board builds executions before wakes in a frame (`settle_built`), since a routine reads its
  runs' views; a routine's failed run speaks through the routine (`run_speaks_through_its_routine`: one notice, with
  `answers_for`'s run now, pause, details). `work.watch`'s snapshot appends every future view (`futures`, a small set kept as views
  are installed) whatever its root does, so a client keeps the future beside the tree. `work.agenda` (`WorkBoard::agenda`) merges
  the wakes' occurrences lazily by time, to the cap, expanding a series with the kernel's `Repeat::next_after` in the daemon's
  zone (a DST change keeps the wall time), skipping what is paused; a window ahead is jumped to with its numbers. `status.short`
  carries `next_due_ms` and `routines_failed` from the `futures` set. Tests: `tests_horizon.rs` (whole cores: the views, a failed run
  and its answers, a met watch, the agenda's weekdays and its pause and its cap, and the golden `tests/golden/horizon_board.txt`),
  `push/work/tests_horizon.rs` (a weekday and a cron series across a DST change, a far window, `until`, the cap, a watch).
- **The push's board**: `crates/theseus-core/src/push/board.rs` (and `push.rs`), fed by `Kernel::observe`; `attention()` is in
  `crates/theseus-protocol/src/push.rs`. A view's `outstanding` counts tool calls only, never the model's own
  (`tool_calls`, as `park` counts them), and `same()` compares it; a queued view keeps its `why` until it leaves
  `queued`; `confirm.list` lists a question from its plan on (theseus-q5af).
  Its seed reads by the store's terms, never every action or execution (`push/hot.rs`, theseus-id8d): the actions
  not settled, the executions that need you or work, their parents, and the 1,000 written last. The rest stay cold
  until a frame touches one or `session.wait` asks (`Push::view_or_load`); the board keeps its counts as entries
  change, so health counts nothing. `push/tests/seed.rs` holds the seed to the full read over mixed stores.
- **The outbox**: `crates/theseus-kernel/src/outbox.rs`, `crates/theseus-core/src/outbox.rs`, and
  `crates/theseus-discord/src/courier.rs`.
- **The config**: `crates/theseus-core/src/config.rs`, and the template `crates/theseus-core/config/theseus.example.toml`
  (`theseusd example-config` prints it); `config/sparse.rs` cuts a note to what differs from the defaults.
- **L1**: `crates/theseus-core/src/sandbox.rs` (`[sandbox]`, the class, the last launch), and the wrapper's L1 path in
  `crates/theseus-kernel/src/job_l1.rs`.
- **The index tender**: the binary in `crates/theseus-index`, its supervisor in `crates/theseus-core/src/tender.rs`
  (started 2 s after serving, restarted with backoff, SIGTERM at a stop), and its child kind in
  `crates/theseus-kernel/src/children.rs`.

## Invariants

- **Messages to the owner** (theseus-x8b9k; `owner_note.rs`, the catalog `docs/messages.md`): every message sent to the
  owner unasked opens with `owner_note::head` (or `note`): `<tag> · <title>` (`ℹ️ FYI`, `✋ Action`, `❓ Decision`,
  `⚠️ Alert`, `⏰ Reminder` with the owner's own words as the title), then one line of what triggered it, then the body
  with the action last. The tag is the one icon: `head` drops a glyph a title starts with, and `is_headed` refuses one.
  A new unasked message uses it and joins `tests_owner_note::every_unasked_message_starts_with_a_kind_and_a_title`. The
  Discord DM task flow (the task reply, its started and thread lines, the board, the forum, `needs_you`'s cards, the
  budget question and notice, the reports) is locked by the owner and does not.
  Self-repair backlog titles are plain words (`rsi/backlog/read.rs`; `plain_title` maps an item filed under the old
  labels, in every backlog read and the needs-you card's asker line, theseus-jsdj5).
- **The frame budget.** A plain one-loop turn writes 4 frames, and each loop with one in-process tool adds 4.
  An input's turn holds its execution (`Kernel::hold_turn`, nothing written) and is admitted in its input's frame
  (`turn/admit_step.rs`, theseus-2uby), so it pays 2 syncs before its model's first byte (`syncs_before_call` on its
  trace); a turn that writes before its input (`catch_up` with anything to take) is admitted alone first.
  Observability rows ride in the turn's next frame; the session's write rides in `end_turn`'s
  (`Store::defer_session`). `tests_m3::a_plain_turn_stays_within_its_frame_budget` fails a sixth frame.
  Each turn counts its own (theseus-wz4y), as `frames` on its trace's root span: its admission's frames, read from
  this thread's count around calls with no `.await` (`theseus_store::frames_written_here`, since the store's writer
  thread writes them), its handle's (`Store::turn_frames`), and its last, which carries the trace. A frame written for
  the turn by another path is counted to it (`Store::count_frames`): a job's completion the spool's drain accepted.
  `tests_m3::frames_counted` and the turn bench (`theseus-sim bench turn`, against the WAL) hold it.
- **A turn has one exit after it begins** (R1). `run_inner`'s body is `turn_body`, and its error goes to `fault`,
  which closes the books as `fail` does (class `internal`). A new `?` in the body or in `finish` lands there; never
  return an error from a turn by another path.
- **Lock order**: a session's lock, then an execution's (`Store::with_session`, `update_session`). Both belong
  to their OS thread, so `SessionLock`, `SessionHold`, and the kernel's `ExecLock` are `!Send` (Review 2's R7):
  holding one across an `.await` in a spawned future is a compile error, and a build-time check beside each fails
  the build if one becomes `Send`.
- **Kernel writes made together are one `Kernel::frame`**, with the core's own records added by `stage`: an answer
  (the bind or the decline, an approval's trust, its row, and the wake) is one (theseus-jj9f). A transaction holds
  its executions' locks, which belong to an OS thread, so it never spans an `.await`; a session's lock goes around
  it, never inside.
- **Serve first.** Nothing new on the start path waits on the network or writes; it goes after serving. Secrets
  resolve behind the socket, and each consumer waits for its own.
- **Where work runs.** An in-process toollet computes on the CPU pool (`cpu.rs`, a permit per core). A tool that
  waits on the network is `Backend::Async`, a future on the runtime. A verb over Theseus's own state is
  `Backend::Harness`. Never block a runtime worker on a sleep or a blocking wait.
- **The gate never refuses** and never parses what a command does. The order is the floor, the approve lists, the
  allow list, then the posture; a granted secret's posture and the external-text hold apply after it, and the
  stricter wins. The whole order after the place's refusal is written once, in `toolrun/order.rs`, which the gate
  and `policy.explain` both run.
- **An L1 call runs at notify** (the owner's decision, 2026-10-02): none of the L0 order applies, since the view hides
  the floor, the approve list's paths, and the socket; the external-text hold still does, and so does the
  operator's own word about the tool: a `[policy.tools]` line or a tightening that asks makes it wait
  (theseus-jfs6), and so do hosts its call names beyond `[sandbox] egress` (18c), and a granted secret's posture,
  as at L0 (theseus-w5op). The inherited
  `[policy].enforcement` never does. Its proposal names its class, so a confirm binds it, and a confirmed call runs in the class its
  proposal names. Nothing falls back from L1 to L0.
- **Results tell the truth.** `toolrun::cap` cuts on line edges and says what it left out, with the tool's own way
  to get the rest (`Tool::rest`), or the kept file and its lines (`toolrun/kept.rs`). A listing names its scope.
- **A kept output is scrubbed, never raw.** What a job printed before the scrubber saw it stays in the spool only
  until its result is written; a copy anywhere else goes through the scrubber first.
- **Thinking goes back only to the provider that wrote it**, and every recompile strips the prefix's thinking.
- **A shared place never receives the owner's material** (the place rule). Its model is offered only the public
  tools, the gate refuses anything else, and its system block carries only public context files. A task takes its
  parent's class. Nothing flows from a private place to a shared one but what a person says there, and the owner's
  publish. A class is fixed for the turn, as the spec is, so a change applies at the next turn's first loop.
- **Nothing retractable goes in the shared header** (Appendix F), so sessions on a profile share one cache entry.
- **The config template is the contract.** Change a default there, not only in code. The loader rejects unknown
  keys, and `example_template_uncommented_still_parses` un-comments every line and parses it.
- **Approvals come from private places** (the place rule, theseus-zmgb). An answer, the undo of a tightening, a
  trust, and a publish count only from the CLI, the web UI, a DM with the owner, or a channel bound `private = true`,
  and only from the owner (`[places] owner`, else the person of each DM the bindings file binds):
  `places::owner_in_private`. A shared place's cards go to the owner's DM. `[approval]` is retired: a config that
  has it loads with a warning, and nothing reads it. A test's bare label answers as the CLI (`From<&str> for
  Answerer` is test-only).
  **An agent's answer is its own, and money and AWS are the owner's** (theseus-5rvss; `peer_proc.rs`, `[policy]
  agent_processes`, default `claude`, `codex`, `gemini`, `aider`, `opencode`, `openclaw`): the socket's mode 0600 makes
  every process of the owner's user the CLI, an agent the owner runs included, so the daemon reads the peer from `/proc`
  when an act asks (`SO_PEERCRED`'s pid, or `--stdio`'s parent; `Conn::answerer`, once a connection, `Peer`): a process
  with a controlling terminal and no listed program above it is the owner's own; no terminal, or a listed program on its
  parent chain (the nearest names it), is an agent's, as `agent:<name>` or `agent:no terminal`. That is `Answerer.agent`
  and its `label`, so every `by` (the `action.confirm_answered` row, the settle's `by agent:<name>`, the narrative) says
  it, and `Notice.by` and `DigestLine.by` carry the name: "you answered" is the owner's own, "answered by an agent
  (claude)" an agent's, and a row with no `agent:` reads as the owner's (no stored field, no format bump). A question
  whose answer spends money or changes AWS (`approval::owners_alone`: the budget question and its reset, and every `aws.*`
  tool but the free reads) refuses an agent's answer, either way, in `judge_act` as `approval.refused` ("this spends money
  or changes AWS, so only the owner answers it: press it in the DM, or answer from your own terminal"), and stays open.
  The owner's terminal, the web UI and the owner's Discord press answer it as before. A light guard, like the job
  marker: an agent that hides its ancestry behind a pty reads as the owner's; L1 and the DM's card are the boundary. A
  test's connection says who is behind it with `Client::behind`. Tests: `tests_m3/agent_answers.rs`, `peer_proc::tests`,
  theseusd's `tests/agent_answers.rs`.
- **A result that is outside text writes its session's hold in its own frame** (theseus-9bp): `external::with_hold`
  for a result written alone, and `external::under_hold` for a frame that may carry one (a late result, the cancel's
  sweep), which is built first without the session's lock and again under it only when it does. A job of a program
  `[policy] external_programs` lists is outside text too (`external::Listed`, read from the call's input by every
  `job_result`), and a session opened, or sent a turn, with `opened_from` a holding session takes its hold in the
  frame that writes it (`external::from_job`; every job's `THESEUS_SESSION`). Both are light guards under default
  trust, not boundaries: a job can strip its own environment (theseus-b5cl).
- **Secrets**: no value in a log, a row, a node, a result, or an error. `scrub.rs` scrubs tool output: each board
  value verbatim, in base64, percent-encoded, and with any of its characters JSON-escaped (`scrub/escaped.rs`:
  the text decoded once where it holds a backslash, a match mapped back to the escaped span; theseus-ubp7), twice
  for JSON inside a JSON string, YAML's and Python repr's escapes among them (a run of `\xNN` that is one character's
  UTF-8 read as that character; theseus-nlvx), and base64 in a decoded text where an escape joins it (`\n` every 60
  characters in a JSON string; theseus-cjyt), and
  the shapes of secrets never resolved here (`scrub/shapes.rs`: token prefixes, OpenAI, Stripe and Google keys among
  them, AWS keys, private-key blocks, JWTs, and a URL's password, the password alone), as written and inside base64,
  percent-escapes, and the escapes (`scrub/decoded.rs`, theseus-oyrt); YAML's folded line break is an escape that decodes
  to nothing (theseus-d80h). A new shape lands with its row in `scrub/corpus.rs`, the planted-secret corpus that
  `scrub/tests_corpus.rs` and theseusd's `tests/planted_secrets.rs` (a daemon, every sink grepped) hold to zero leaks,
  and the clean corpus to zero false positives. Build test secrets from parts: GitHub refuses a push holding one
  whole. The broker hands a value only to the program it is granted to, run by its
  own argv, and never to one the call could make it run (`broker::launches`: the call's own environment, gh's
  aliases and extensions, git's aliases, `-c`, and the programs its options and URLs name), in L1 as at L0.
- **The AWS keys and the providers' keys are harness-only** (theseus-gh7): Theseus's own tools read them from the
  board (`aws/`, the providers), and no job is handed one unless `[broker]` names it (`Broker::may_hand_out`). `broker::harness_only` is the line `theseusd check` and health print, naming them; the
  template test holds it (`the_templates_harness_only_keys`). A program granted `aws_account` gets a short-lived
  job session at launch (`Broker::for_job_of`, `aws::Account::job_session`), never the key, and none before the
  owner role exists. Containment is that, the operator's `[broker]` grants,
  and the egress list: credentials as stand-ins (TLS interception at the proxy) were dropped for v1 (the owner,
  2026-10-03). Do nothing heavier without his say.
- **One fact, recorded once.** A new ledger row, notification, or narrative sentence is a fact's projection in
  `fact/`, not a hand-written channel at its site. Recording writes no frame: its row rides in the turn's next frame
  (or a frame now, outside a turn), and a row that must ride in a frame the site builds is `Rec::row`, with the rest
  announced once that frame is written (`Rec::announce`).

- **The data rule.** A model of restricted origin (`cn` or `unknown`) is sent a request only for a session with no
  cn-disqualifying content label, every item of which was judged; the provider refuses a request to one for any other, and for
  none; origin is judged by endpoint and family as well as by catalog row; `[privacy] sensitive_names` go only to models of
  US origin, whatever the labels say; a China-origin model only drafts code, in its own tree, with no programs, and nothing
  it drafts is applied or reported before a Claude review (`final`). The passages: "Content flags and the restricted-origin
  floor", "The draft view", "Sensitive names" and "Drafts" under the judge.

## Tests

- In-process suites in `src/`: `tests_m3.rs` (turns through the whole core), `tests_continuations.rs`,
  `tests_failures.rs`, `tests_tasks.rs`, `tests_wakes.rs`, `tests_external.rs`, `tests_overflow.rs`,
  `tests_push.rs`, `tests_config.rs`, `tests_books.rs` (a fault at each exit after a paid loop still closes the
  books, R1), `tests_refused.rs` (a corrupt record skipped, counted, and the driver still continuing, R4), and
  `rpc/tests.rs`.
- `tests_registry.rs` is the reader rule's test. The gate runs it alone, before the suite. A `tool` marker is held
  to the install list, read as text (`scripts/build.sh`'s `shipped`, and `scripts/setup.sh`'s `SHIPPED`, its copy);
  a tool no install ships says why beside it (`run_from_tree`, theseus-exam's). A use of `EdgeKind`, `Event`, or
  `LedgerKind` counts only where the type named is its home's (`Home`): a path that resolves there
  (`graph::EdgeKind::X` after `use crate::graph;`), or a bare name in a file that imports the home's type by name
  and no other of that name; theseus-memory's own `EdgeKind` reads nothing of ours (theseus-g7qp).
- `tests_layouts.rs` holds the store's version rule (P5b; theseus-ptx1): one table of every old record layout on
  disk somewhere, each a literal its build wrote (the 460a35b fixture's records, and each layout since). Each must
  decode, keep every field but the ones it names, and survive a round trip. A step that adds a field to a stored
  record bumps the store's format and adds the layout it replaces there.
- `tests_output.rs` is the output golden (theseus-j6qn): scripted scenarios through whole cores, and every frame,
  notification, and narrative line they produce, against `tests/golden/core_output.txt`. It pins shapes, not
  numbers (theseus-ptx1): every number and digit run is `#` (an id's alias keeps its own), so a prompt, a token
  count, or a cost moves none of it; the money, telemetry, and frame-budget tests hold the numbers.
  `THESEUS_GOLDEN=write` rewrites it, for a change you mean, and the diff is the review. A narrative line whose
  presence the machine's load decides is left out there (`by_the_load`); a new one joins it.
- `tests_outside_text.rs`: property tests over every reader of outside text (the HTML reader, the wake's time
  parsers, the provider's SSE lines, and the scrubber, which also never lets a planted value through), with a fresh
  seed each run, so the gate keeps looking (Item 13). The release
  profile aborts on a panic, so one panic on outside text takes the daemon down. A new reader joins them.
- `FakeProvider::scripted` scripts a provider's answers. `Parts::for_tests` builds a core around it, and a second
  provider can be inserted into `parts.providers`.
- `Core::build_with_clock` (test-only) builds a core whose kernel runs on a given clock, so a test drives an hour
  with a `VirtualClock` (`tests_effort_lapse.rs`, theseus-2ix1k). `test_support::until_event(what, f)` waits with no
  deadline of its own and prints `waiting for <what>` every 30 s, so a hang names itself in nextest's output.
- A test's temp directory or file comes from `crate::scratch` (`dir`, `file`, `tempdir`; theseus-5hr6i) and is
  removed when the process exits, leaked into a task or not; a killed process's are pruned once six hours old. Prefer it
  over a bare `tempfile::tempdir()` whenever the fixture can be leaked into a task.
- `Store::fail_turn_frame` (test-only) fails the first turn frame whose records match, once, as a full disk would.
- Debug builds assert, at every read, that a turn's kept transcript equals a fresh read of the store, by positions and
  record keys, decoding nothing. A node written past the turn's handle panics a debug daemon.
- `tests_tiering.rs`: the heat cache and stubs through whole turns (decodes fall, every request the same with the cache
  off, the debug check through a ring and a recompile, a recall source from before a floor); `node_cache::tests`
  holds eviction by heat under the bound (a property test).

## Traps

- Every child process the core starts (an `op`, a job's wrapper) goes through `theseus_kernel::children::spawn`, or
  the daemon's reaper can take its exit status (`ECHILD`).
- A static or a detached thread holds the core or the store by `Weak`. One that held an `Arc<Core>` kept the store
  open past a clean stop, and every start then paid redb's repair.
- A recompile inside a tool loop strips thinking between a call and its result, so context files change on the
  next turn's first loop, never mid-turn.
- **Every `tool_use` gets a result, whoever ends its execution.** A cancelled execution takes no more turns, so
  `answer_after_cancel` writes what its calls left unanswered (the cancel's handler, and the end of the turn that held
  the execution, whichever runs later; each call is answered once). A call that may have run is `unknown`, never
  "not run". A late completion after a cancel drops the node that rode with it (`accept_completion_with` writes
  `extra` only for a settle): the turn that holds the result writes it.
- **A reset never frees held money.** `Budget::available_after_reset` is what a reset leaves; a call over it cannot be
  answered by a question, so its question says so and the retry's refusal fails the turn (`over_limit`), never asks again.
- **The take frame keeps what its retry needs.** A turn that takes a wake or a report keeps where its reply goes in a
  META record written in the taking frame (`wake.target.<session>`); a retry frames its reply from the unanswered
  wake and report nodes. No in-memory field carries it.
- **A `/stop` cuts the model's stream** (theseus-yey): `Core::stop_execution` signals `TurnRunner::stops`, which only
  wakes `call_model`; the kernel's record of the stop decides. The cut call settles as failed at an estimate (the
  input its reservation assumed, split as the request was sent: the turn's previous call's cache at the read price,
  the rest a write when the request carried the conversation's breakpoint, `turn/cut_step.rs`, theseus-3p36; the
  output from the characters streamed), capped at its reservation, never held unknown, and a `provider.cut` row says
  it is an estimate and its `split`. A stop that lands after the stream ended keeps the answer and runs none of its calls.
- **A spec's tools are shared, never copied** (theseus-nsp9l.2): `RequestSpec.tools` is a `tool_set::Tools` (an `Arc` of the
  definitions, immutable; its JSON length, digest and hash state, and the sets a deferral loads, are kept with it), and
  `ToolRuntime::definitions_for` keeps each place's catalog (`toolrun/catalogs.rs`). A compile clones specs and digests
  their tools a dozen times, so a `Vec<Value>` of 100 KB there cost 25 ms of a person's wait; change a set by making a new
  `Tools`, never in place. Tests: `tests_send_path.rs` (counts catalogs made and tool JSON walked; no timing), `tool_set::tests`.
- `cargo test -p theseus-core` builds a second copy of every dependency. Use
  `cargo nextest run --workspace -E 'package(theseus-core)'`.
- **The data rule's traps** (theseus-a1wvq.12). DeepSeek's Anthropic endpoint serves a `claude*` model id with a DeepSeek
  model, so origin comes from the endpoint as well as the id (`catalog::origin_of`). A config row cannot loosen a
  built-in China-origin row's origin (`Config::validate`). `deepseek-flash` is a floating alias: its served model can
  change under the same id. A later loop on a restricted model whose session was labelled meanwhile moves to the `[model]` default (`cn_move`).
  `final` without `cn_draft` fails the load, so deep_coding's pair goes on together. `[routing.data] private_topics` was
  renamed `[privacy] sensitive_names`.
