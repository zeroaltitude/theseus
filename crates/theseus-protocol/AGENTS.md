# theseus-protocol

The wire types (spec §3.18): JSON-RPC 2.0 over newline-delimited JSON. Types only: no runtime, no I/O, no clock.
Every client and the core read it, and the cockpit's TypeScript is generated from it.

Key modules: `lib.rs` (the `method`, `notify`, `error_code` tables), `events.rs`, `push.rs`, `gate.rs`, `ts.rs`. Read by: every crate on the wire, and the cockpit (generated).

## What's here

- `lib.rs`: the methods' params and results, `VERSION`, and three tables: `method` and `notify`, each one list
  from which its `ALL` is built, and `error_code`.
- `defer.rs` (theseus-7m7w.2): `tool.defer`'s answer (`DeferListResult`: each kind of place, each tool family it loads or
  defers with the reason, its use, searches and misses, and the estimated tokens of a first request whole and as sent) and
  health's `tools:` line (`DeferHealth`).
- `events.rs`: one struct per notification, and the `events!` table, from which `Event::VARIANTS` is built.
- `actions.rs`: `action.list`'s params and result, and `ActionInfo`: the newest `n`, an execution's, or with
  `unsettled` every action not settled however old (theseus-hnof.3), which the cockpit's Ship and watch read beside
  the newest page so a job that runs for hours keeps its place.
- `exec_list.rs` (theseus-0jet): `execution.list`'s params and result: no params is every execution as before; `{n,
  before}` the newest `n` by birth with the result's `older` cursor; `{ids}` only those.
- `cancel.rs` (M4 18a): a cancel's verdict on the wire (`CancelVerdict`), health's `CancelCount`, and `words`, the
  one wording of a verdict every surface shows.
- `gate.rs`: the gate's record of a tool call (`GateRecord`), written through `canonical` (sorted keys), so stored
  records keep their bytes.
- `push.rs`: what every surface shows of an execution: `ExecutionView` and `attention()`, the design's rules,
  first match wins. The caller passes how to write a time of day, since this crate reads no clock.
- `work.rs` (theseus-753z): the work view, one task model for every surface (`WorkView`: conversations, tasks,
  steps, jobs, questions and wakes in one tree), `attention_of()` (attention's levels and labels for every kind),
  `answer_options()` (a question's answers, worded once), and `WorkView::from_execution()`, so a reader adopts the
  policy before the daemon's work board fills views itself. `tests_work_join.rs` holds `attention_of()` to
  `attention()` over every view the push carries, but for what a work view does not carry (`· +1 more`, a ready
  session's wake time, an unknown state's name, a failed one's pending question).
- `forecast.rs` (theseus-zz752): `WorkForecast` (a task view's `forecast`: p50 and p90 of dollars and minutes, `n`, the
  sticky `past` bit, and Jev's latest read), its words (`words`, `spent_of`, `read_words`, `none_words`), and
  `forecast.calibration`'s types (`ForecastCalibrationResult::lines`, what `theseus tasks forecast` prints). Numbers only.
  `notices.rs` row 11 (`Why::PastForecast`) and `forecast_answers()` (Keep going, Stop).
- `work/horizon.rs` (theseus-nuhsz; horizon HZ8; a submodule of `work.rs`, since lib.rs has no room): the future on the work board.
  `WorkKind` gained `Routine`, `Watch` and `Project`, and `WorkView` gained `purpose`, `next_at_ms`, `series`, `paused`, `routine`,
  `watch` and `project` (every one absent from the bytes at its default, so older peers and every old fixture are unchanged).
  `Horizon` (Now, Today, Week, Later, Recurring, World, Projects), `LocalDay` (the *client's* two local midnights: this crate reads
  no clock and no zone) and the pure `horizon_of(view, now_ms, day)` / `bucket_at` (the table is its doc). `work.agenda`'s params and
  answer (`AgendaParams`, `AgendaLine`, `AgendaResult`, at most `AGENDA_MAX` 200 lines), and the answers a new kind offers
  (`routine_failed_answers`, `watch_met_answers`, `milestone_answers`; `notices::answers_for` and `reminder_notice` attach them).
  `ProjectView` is declared and empty until the `projects` row fills it. `StatusShortResult` gained `next_due_ms` and
  `routines_failed`. Tests: `work/tests_horizon.rs`, `tests/work.rs`' `the_future_shapes` (fixtures `view_routine`, `view_watch`,
  `view_project`, `agenda_*`, `status_short`).
- `answer.rs` (theseus-ds5os): `work.answer` (`WorkAnswerParams`/`WorkAnswerResult`), `confirm.list`'s optional params,
  `reminder_options()` (a reminder's four answers as `AnswerOption`s, made from `wakes::reminder_answers`, the one
  wording), `approves()` (which option ids approve), and `ConfirmRequest::question(kind, may_answer)`, the pure
  `Question` a request is. A `ConfirmRequest` carries its `options` (worded once by the daemon) and, in a read made for
  one connection (`confirm.list`), `may_answer`; `confirm.resolved` carries the `option` that answered.
- `job_follow.rs` (theseus-e1hu4): a running job's output on demand and live: `work.peek` (`WorkPeekParams`/
  `WorkPeekResult`: size, at most 20 lines, when, whether it ended, and optionally its dispatch time and its program's
  usual), `action.output` (`ActionOutputParams`/`Result`) with `action.output.chunk` (`id`, byte `offset`, `text`) and
  `action.output.end` (`outcome`) to the asking connection alone, `action.unfollow`, and `job_words` (`job 0:05 of
  usual 0:31`), the one wording of a job's time. `Doing.last_line` (work.rs) is a running job's last output line.
  Fixtures: `tests/wire/action_output_*.json`, `tests/wire/work/view_job_running.json`.
- `work_system.rs` (theseus-jn17m): the daemon's own work on the board: `ROOT` (`system`), the hour jobs stay
  (`KEEP_MS`), the quiet time that ends a silent one (`QUIET_MS`), and the pure words every surface shares: `line` (`title
  · count · 3:12`), `count_words`, `status_words` (`⚙ importing 41/120`, the first line of `theseus status`) and `collapsed`
  (the root closed, for the TUI). `ImportEpisodesParams` carries `batch`/`batches`, which only the work board reads.
- `work_watch.rs` (theseus-hkx30): `work.watch`'s params and snapshot (`WorkWatchParams`: `limit`, `roots` (session
  ids), `interactive`; `WorkWatchResult`), `work.unwatch`'s answer, and `work.changed` (`WorkChanged`: one per committed
  frame, every view it changed with its ancestors' rollups, `gone` for ids that left). `work.notice` carries
  `notices::Notice` as it is. Fixtures in `tests/wire/work/`.
- `retry.rs` (theseus-7zgsm): a failed task stays flagged until the owner dismisses it, and a retry opens a linked new
  task. `execution.dismiss { execution_id }` and `task.retry { execution_id, note? }` (params and results), `is_failure`
  and `needs_dismissal` (a work view's and an execution view's reading of "a failure that still asks"), and
  `failure_options()`, the three answers a failure draws (Retry for a task, Dismiss, Details), worded once for every
  surface: verbs, not `work.answer`'s answers. `ExecutionView` and `WorkView` carry `dismissed` (the attention rules
  read it: a dismissed failure is idle, `failed: <reason> · dismissed`, on the push and on `attention_of`) and
  `retry_of`. Both fields are left out of the bytes at their defaults.
- `work_threads.rs` (theseus-5i8i): threads on the work board, pure. `WorkView.thread` (a wire field: the thread whose
  session is the view's root conversation) and `ThreadGroup` (in `work_watch.rs`: a thread's words, its session and its
  stats; `work.watch`'s `threads` and `work.changed`'s `threads`, absent when empty and never sent to a connection that
  reads no private text) are grouped by `group` into each live thread with its open tasks, questions and wakes (needs you
  first) and the work in none (`Grouped::loose`). The counting rules are written once in its header: a question counts
  once, a pending seal counts one, a paused thread keeps its count and leaves the board's (`Grouped::needs_you`), a done
  thread shows only while open, warm is within `WARM_MS`. `GroupedThread::header(now, width)` is the one wording of a
  thread's line (the CLI and the TUI share it; `cockpit/src/lib/threadboard.ts` mirrors the rules). Tests:
  `tests_work_threads.rs`, `tests/work.rs` with the `watch_result_threads` fixture.
- `work/ask.rs` (theseus-j7cje): `owner_ask`'s input (`AskInput::parse`: 2 to 6 options, a question up to 500 characters, a label up to 60, a default as an option's number from 1, a life of 1 to 1,440 minutes), the answer ids (`choice_of`: `choice:<i>` from 0, `choice:other`, or a number from 1) and the words the model reads back (`chose`, `said_instead`, `expired`). A `ConfirmRequest` for the tool is an `Ask` question with its `choices`.
- `notices.rs` (theseus-753z; the method table owns the name `notify`): the notification policy, pure. `policy()`
  is the owner's table, one function per row, first match wins; `Rules` holds its numbers as data; `deliver()` says
  how a notice reaches one viewer; `notices/burst.rs`'s `Burst` (a client's memory, not a wire type) folds a root's
  Interrupts within 10 s into one. No clock: a reminder is a view against itself later (`next_reminder`), and the
  caller's `hm` writes a time of day. Its tests are one per row (`tests_notices.rs`); the wire shapes are
  `tests/work.rs` against `tests/wire/work/`, a directory `tests/wire.rs`'s notification walk skips.
- `ledger.rs`: the ledger's kinds, `LedgerKind` (theseus-j6qn): every kind a row is written under, by any crate,
  one line each. Writers take a variant, so a new kind is a new line here, and core's `tests_registry` fails a
  variant nothing writes. A row stores the kind's name, so old kinds (`LedgerKind::RENAMED`) and unknown ones still
  read; renaming a kind is a store version change (P5b).
- `ledger_verify.rs` (theseus-bmkxk): `ledger.verify`'s params and answer (`LedgerVerifyParams`, `LedgerVerifyResult`,
  `LedgerBreak`, `LedgerHead`): the hash chain's walk, read only; no row's data rides it. Health's `ledger_head` is its
  short form (`LedgerHead::short`).
- `places.rs` (the place rule, theseus-nbsh): a place's `PlaceClass` (private or shared), and health's
  `PlacesHealth`, each place with its class. The rule itself is the core's (`places.rs`). 19a's labels went with it:
  a stored node's `label` and a manifest's audience fields read and are left unread (NODE 7, COMPILATION 5), and a
  row of the five `label.*` kinds reads as an unknown kind.
- `sandbox.rs`: health's `sandbox` block (17b, with 18c's egress counts), and `reach` and `egress_in`, the one
  wording of an L1 job's reach (`no network`, `egress: github.com:443`) and the list a proposal binds.
- `cred.rs` (theseus-gh7): `HarnessOnly`, health's and `theseusd check`'s line of the secrets a job may be handed
  and the keys that stay the harness's own. 18d's credential requests, `secret.requested` among them, went in
  theseus-w5op: a stored row of their kinds still reads, as an unknown kind.
- `arrangement.rs` (M5 27): a task's arrangement as `TaskInfo` carries it, its pieces by reference.
- `check.rs` (M5 28a): a check task's basis (`TaskCheck`) as `TaskInfo` carries it, and `TaskCheck::line`, the one
  wording every surface shows (`cockpit/src/lib/check.ts` mirrors it).
- `route.rs`: how routing placed a turn (`TurnRoute`), and a refusal's fallback (`TurnFallback`, theseus-7gir.18) with
  `TurnFallback::line`, the one wording every surface shows (`cockpit/src/lib/fallback.ts` mirrors it).
- `route.rs` also holds `ToolsRaised` and `RaisedFamily` (theseus-xlri5): the families a turn's first request admitted
  past its place's budget, with `line`, the one wording `harness_lines` chains.
- `nudge.rs` (theseus-fh0u): the stop nudge on the wire: the result's `TurnNudge` with `line`, the one wording every
  surface shows (`cockpit/src/lib/nudge.ts` mirrors it), `TurnSubmitResult::harness_lines` (a fallback's line, then a
  nudge's), and `session.nudge`'s params and result.
- `cost.rs` (theseus-c0bb): `TurnSubmitResult::cost_words`, the one wording of a turn's cost that the Discord
  footer and the CLI's status line show: the session's total with this reply's cost, `$47.52 total ($7.86 this
  reply)`, four decimals under a dollar; the turn's cost alone from a daemon that sends no `session_cost_usd`.
- `warm.rs` (theseus-tnky): `session.typing`'s params and result, health's `warm` block (`WarmHealth`, `WarmStamp`), and
  `Typist` (a client's memory of whom it told: one notice per session per `SPELL_SECS`, the caller passing the time).
- `books.rs` (theseus-civ0): `books.list` and `books.page`'s params and results, `BOOKS` and `UNSORTED`.
- `rsi.rs` (theseus-pw1q): self-improvement's wire types (`self.halt`, `self.resume`, `self.log`, `self.digest`),
  and `SELF_ROWS`, the `self.*` rows later self steps write, declared with their fields before their writers: a
  name becomes a `LedgerKind` on the commit that first writes it (the reader rule), and every such row's data
  carries `what`, `why`, `numbers` and `undo`. `SelfState` is also health's `self_improve` (health's `self` line; its `sealed` is `SealedState`, counts of items per sealed set, present only while `[self.sealed] dir` is set);
  `BinaryStatus` moved to `health.rs` to make room in lib.rs. `SelfChanged` is the `self.changed` notification (the switch moved; to the overview watchers, `bus::wide`).
- `earned.rs` (theseus-uttu): earned autonomy's wire types (`policy.earned`, `policy.earned.answer`, `policy.revoke`):
  a group of approvals with its record, a grant, a card, and the number's weeks. Each name carries `Earned`.
- `grants.rs` (theseus-drjs): scoped grants on the wire: a question's `GrantOffer` (scope, kind `prefix` or `edits`,
  pattern, words, its key `a`/`e`, its button label and its `approve_grant` option), an answer's `GrantAnswer` (the
  pattern echoed), a live `SessionGrant`, and `policy.grants`' params and result.
- `modes.rs` (theseus-lyxy): a conversation's mode (`SessionMode`: ask, edits, plan, auto; `next()` is Shift+Tab's order,
  `status()` the status line's `edits (shift+tab)`), a plan item as a surface shows it (`PlanItem {id, title, state}`: a view
  of the task graph's plan item, `PlanState::of_task`; the plan is the work board's, never a second list) with `plan_line`
  (`plan 2/5`, done over the items not skipped, the board's own count), `checklist` (`✔ 1. add the function`, `◼` under way,
  `☐` not begun, `⊘` skipped: the marks F10's S12c reads) and `plan_text`, `session.mode`'s params and result (with `plan`)
  and the `session.mode` notification (`SessionModeChanged`: mode, plan, by). `grants.rs` also has `KIND_MODE` and
  `GrantOffer::mode_edits` (a plan question's other yes, key `e`).
- `window.rs` (theseus-m2tiu): `ledger.window`'s params and result and `POSTMORTEM_KINDS`, the kinds `theseus postmortem`
  reads: one page of a time window, read only, the daemon's answer scrubbed and (for a surface that reads no private
  text) narrowed to the session named.
- `why.rs` (theseus-ighf): `theseus why`'s and Discord `/why`'s words, pure: `why_lines` (all of it) and `why_short`
  (each section's top items and a count of the rest, cut to a budget) from a `ContextExplainResult`, the turn's
  `route.decided` row, `ToolCalled`s (`tools_called` over `node.list`) and `policy.explain`. A shared place's class
  hides a private or imported source's words (`hide_sources`). No wire type; its goldens are `tests/why/`.
- `context.rs` (theseus-7n3e, theseus-r5ydi): `context.explain`'s params and result. A turn the router detoured compiled nothing that was
  stored, so its answer is read from the `context.detoured` row (`ContextDetoured`, `DetourPart`: names and token counts): the result's `detour`
  (`ContextDetour`: kind, profile, model) is absent for any other turn, `ContextTurn.detoured` marks it in the turn list, and `why.rs`'s
  `detour_words` and `fill.rs`'s `context_short` say it, with what a detour leaves out. Goldens: `tests/why/detour*.txt`.
- `census.rs` (theseus-7m7w.1): `context.census`'s params and result (`ContextCensusParams`, `ContextCensusResult` with a
  `CensusBlock` of `CensusPart`s for the system and a `CensusTool` for each tool, `CensusExact`), and `PLACES`, the kinds of
  place a fresh session's first request is counted for. Counts are tokens at the model's figures, apportioned to add up.
- `retract.rs` (theseus-2ced): `fact.retract`'s params and result, one `RetractExposure` for each place a node went, and
  `exposure_kind`, the five kinds in table order.
- `wakes.rs` (theseus-lherl): `WakeInfo`, moved out of lib.rs and re-exported (the bytes are unchanged; `days`,
  `until_ms` and `title` are optional). `tests/wakes.rs` holds its bytes. Since theseus-8ux6x it also has `purpose`, `run` and
  `paused` on `WakeInfo` (each left out of the bytes at its default), the owner's verbs' params (`wake.create`, `.snooze`,
  `.pause`, `.resume`, `.done`), `WakeAnswer` and `reminder_answers()` (Done, Snooze 10 min, Snooze 1 h, Tomorrow 9:00,
  each a direct call), and the `wake.noticed` notification (`WakeNoticed`). `notices::reminder_notice` makes its Interrupt,
  `wake_urgency` is rule 4 by purpose (a reminder interrupts; a check-in, a routine and a watch inform; a view that says none
  is read as before). `tests/wake_notice.rs` holds the bytes.
- `watches.rs` (theseus-f2r2b): `watch.create` and `watch.list` (`WatchCreateParams` with its `WatchCheck` run|fetch|file and `WatchThen`, `WatchInfo`). A watch ending is the existing `wake.noticed` with purpose `watch.met` (`wake_urgency`: an Interrupt), `watch.expired` or `watch.failing` (Inform); the ledger kinds are `watch.checked|met|expired|failing`.
- `cite.rs` (theseus-ejdk): a reply's citations. The handle a recalled item's header carries (`[m:a1b2c3]`:
  `handles_for` makes them unique in a pack, `header_with` and `split_header` write and read one), the handles a text
  holds (`handles_in`), the marks every surface draws (`mark_handles`, `mark_into`: `[1]`, numbered by first
  mention, a handle with no source left as text), and the wire types `Citation`, `DiscordSource`, `memory.cites`'s
  params and result. `TurnSubmitResult.cites` and `SessionHistoryResult.cites` carry the resolved sources; each is
  absent when empty.
- `import_sync.rs` (theseus-pjmj7): the OpenClaw sync's wire types (`import.sync.plan`, `import.sync.record`), `HealthSync`
  (health's `imported.sync`, with the one `warn` sentence every surface shows), `ImportTouched` (an `import.episodes` batch's
  created or appended sessions), and the timer's minute and the missed-night grace.
- `fork.rs` (theseus-2qzx): `session.fork`'s params and result, and the stored `ForkOrigin` (a session record's
  `forked_from`), `Inherits` (a compilation's inherited run) and `ReachFork` (`node.reach`'s `forks`).
- `rewind.rs` (theseus-23rk): `files.changes` (a session's messages, each with the files its edits changed since:
  `RewindPoint`, `FileChange`, `fork_at`) and `files.restore` (`FilesRestoreParams`, `RestoredFile`,
  `FilesRestoreResult`; `since_node` is a node or `@<WAL position>`), and `NOT_RESTORED`, the words every surface says
  of what a rewind never restores.
- `ops.rs` (theseus-jid7t.2): the Operations page's verbs: `session.profile`, `task.rehome` and `task.reschedule`'s params and
  results, and `DataMark` (`open`, `private`, `sensitive`, weakest first), which `SessionInfo.data` and `WorkView.data` carry.
- `task_start.rs` (theseus-7oq0w): `task.start`'s `TaskStartParams` (`brief`, `title`, `home`, `budget_usd`, `profile`, the start time as
  `start_at_ms`, `start_after_ms` or `start_when`, `quotes`, `report_to`), `TaskStartResult` and `ReportTo`. The method is the
  owner's, judged by `Act::Task`.
- `threads.rs` (theseus-nhew): threads on virtual places `thread:<slug>`: `ThreadInfo` (the seam the context, recall and
  routing rows read: extend it only by adding), `WorkingSet`, `Seal`, `ThreadState`, `ThreadRow` (a thread with its
  session's context tokens and today's dollars), the eight `thread.*` methods' params, and `slug_ok`/`place_of`/
  `slug_of_place`. `turn.submit`'s `thread` is in `submit.rs`.
- `diff.rs` and `term/words.rs` (theseus-z6au): a tool call in words and an edit's diff, for every surface.
  `term::words::summarize(tool, input)` is the line a person reads (`Run `cargo test``, `Edit tides.py`, `Search for
  `retry` in src/`), built on `argument`, the one thing a call is about, which Discord's tool lines have always shown
  (its bytes unchanged). `ToolDiff` is `tool.ended`'s optional `diff` (`fs.edit`, `fs.write`, `fs.patch`: path, lines
  added and removed, the unified diff scrubbed and capped at `DIFF_CAP`, 16 KB, `truncated`); `ConfirmPreview` is
  `confirm.requested`'s optional `preview` (`diff`, `command` or `text`). `diff::rows` reads a unified diff into
  numbered `Row`s and marks the changed words of each paired removed/added line (`words`, a token LCS); no style.
- `routines.rs` (theseus-pvm5c): routines on the wire: `RoutineSpec` (the META record `routine:<session id>`, no store
  format of its own), `RoutineInfo`, `RunRow`, the eight `routine.*` methods' params, `CatchUp` and `Speak`, and `NOTHING_NEW`
  (the marker a `when_notable` routine's report ends with when there is nothing to say). Since theseus-52tut: a routine's `grant`
  (`RoutineInfo`, `RoutineCreateParams`, `RoutineUpdateParams.grant|add_grant`), its `streak`, `RoutineBlocked` (a run that ended blocked),
  `RoutinePing`, and `routine.try`; `notices.rs` holds the ping rule's entries (`ROUTINE_PURPOSES`, `routine_urgency`, `routine_pings`,
  `routine_line`, `routine_answers`), sent as `wake.noticed` with a routine purpose.
  `slug_of_place`; since theseus-sjzm also `Speaking` (`turn.submit`'s `speaking`: the Discord origin of a thread turn, and `answer_at`), `ThreadChanged` (the `thread.changed` notification) and `channel_surface`/`surface_channel` (`discord:channel:<id>`). `turn.submit`'s `thread` is in `submit.rs`.
- `threads_import.rs` (theseus-71ib): `thread.import`'s params (`from`, `since_days`, `min_cluster`, `apply`: none is the dry
  run) and result (`ThreadCandidate` rows, `ThreadApplied` with the card and its cited handles, `ThreadSkipped`, the spend).
  `slug_of_place`. `turn.submit`'s `thread` is in `submit.rs`. Since theseus-f2g0: `thread.seal` (`ThreadSealParams`, `ThreadSealResult`,
  `ThreadCrossed`, `crossed_words`), `thread.unseal`, `thread.seal_dismiss`, and `ThreadNewParams.sealed`/`split_from`.
- `own_bench.rs` (own-benchmark, theseus-pw1q.12): `turn.replay`'s params and result (`TurnReplayParams`, `TurnReplayResult`,
  `ReplayReply`, `ReplayVerdict`) and `route.report`'s (`RouteReportParams`, `RouteReportResult`, `RouteCell`).
- `card.rs` (theseus-slvl.6): the owner card's wire types: `CardProposal` (a META record `card.proposal.<id>`: text, kind, scope, the question and its options `Keep/Edit/Not this`, the episodes it rests on, expiry, state), `card.propose|list|answer|done`'s params and results, and `card_question`, the one place the question's words are made. Exported under `CARD_`/`card_` names at the crate root; the lib.rs line budget allowed two lines.
- `standing.rs` (theseus-a7vi, theseus-slvl.4): the standing records' wire types. `StandingScope` has a `Surface(name)` variant
  (store format 34), `SURFACES` names the five (`terminal`, `discord`, `voice`, `cockpit`, `mcp`); `StandingListParams.surface`
  is the asker's surface with a `session_id`. `turn.submit`'s optional `surface` (in `submit.rs`) lets the Discord binding say
  `voice`; `context.compiled` names the tail's records in `standing_here`.
- `plain.rs` (theseus-cudar): `PlainWords` (who, what, the model's why, the rule that holds it, a risk badge, what Yes and No do), `ConfirmRequest::plain`: an approval worded once by the core for every surface; absent from a daemon that does not say.
- `initiative.rs` (theseus-slvl.9): `Initiative` (the persona's `quiet|normal|chatty`, re-exported by core's config),
  `Proactive` (the kinds of message sent first) and `Initiative::lets`, the one table of what each level lets through;
  `notices::Rules::initiative` carries it into `policy()` (`Rules::shaped`: at `quiet` a finished root and a no-purpose or
  check-in wake inform instead of interrupting, a reminder and a met watch never; at `chatty` a promised check-in
  interrupts; `normal` is the table as written). The wire types of `initiative.rate` (a 👍 or 👎) and `initiative.week`.
- `projects.rs` (theseus-n5ui2): projects on the wire: `ProjectInfo` and `ProjectProgress` (`bar`, `▰▰▰▱▱▱▱ 3/7`), the four `project.*`
  methods' params and results, `ProjectSliceSummary` (`context.compiled`'s `project`), and `DEFAULT_REVIEW`. `TaskRecord` gained `kind`
  (`TaskKind`), `due_at_ms` and `review` (store format 45, each left out of the bytes at its default). `notices.rs` reads a reminder
  whose note begins `⚑` as a milestone's (`MILESTONE_MARK`, no clock icon), and `⚑ overdue` as `Why::Overdue`: an Interrupt with no sound.
- `flow.rs` (theseus-pw1q.21.4): `FlowInfo` (a loaded flow, as `Core::flow_list` tells it) and `FlowHandback` (the state a flow
  hands the model). Not in the TypeScript: no cockpit reader yet.
- `tasks.rs` also holds `task.list`'s params and result (moved out of lib.rs, theseus-ypnxt.1): the filters `ended`, `since_ms`,
  `query`, `id` and `limit`, and `TaskListResult.details` (`TaskDetail`: the report's first line, whole report and hold),
  absent from the bytes when no filter is set. `work_digest.rs`: a reported task's `DigestLine` carries its `report` and is
  never folded without its title (`NAMED_MAX` told, the rest named together); `WorkDigestResult.more` (`DigestMore`) says where a
  ledger read stopped. `DigestLine.by` and `Notice.by` (theseus-5rvss) name the agent that answered a question (`claude`, `no terminal`; absent is the owner's own, and every older line): `agent_by`/`agent_of` make and read the ledger's `by` (`agent:<name>`), `by_words` says it in a sentence, `digest_words` words "✓ answered by an agent (claude): …" and `fold_digest` and `digest_summary` keep an agent's answers apart from the owner's.
- `ontology.rs` also holds `ontology.undo` and `ontology.kind.add|retire` (theseus-xzxqv), and `judge.rs` `judge.latency`'s params and
  result (the batch's p50 and p95 by day); `TurnSubmitResult.filed` is Jev's `filed:` footer line.
- `ts.rs`: the TypeScript export.
- `content.rs` (theseus-9ymz0): `LabelView` (a session's label as `SessionInfo.labels` carries it), `ContentUnlabelParams` and `UnlabelResult`
  (`content.unlabel`, and the params of `session.unlabelled`) and `LabelRaised` (`session.labelled`). The words are the core's: the flags `injection`,
  `secret`, `confidential`, `personal`.

## The CLI's spellings and the wire (theseus-rwpbg.10)

The CLI's names changed and the wire's did not: a folded command (`answer ID snooze`, `wake pause`, `tasks --watch`,
`persona rate`, `memory index`, `model bench`, `why --short`, `watch-for`) calls the method it always called. New clients use
`work.answer`, `work.watch`, `session.*` and one cancel; the other answer, watch and cancel methods (`action.confirm`,
`executions.watch`, `execution.cancel`, `task.cancel`, `wake.cancel`) are kept for the cockpit and the TUI and get no new
siblings. A rename of a wire field or a stored value is a store-format step, never a trim.

## Invariants

- **One definition per wire shape** (Item 30). Senders build the typed struct; nothing sends `json!` for a shape
  this crate defines, and nothing reads one back by string keys.
- **Old rows and older peers still decode.** A new field is optional, with serde's default, and absent from the
  bytes when unset, so every fixture keeps its bytes. On the TypeScript side it is `#[ts(optional)]`, which writes
  `field?: T`.
- **The reader rule** (P0, rule 3). A new method goes into the `method` table and gets its dispatch arm in
  `crates/theseus-core/src/rpc/server.rs` on the same commit. A new notification goes into `notify`, gets its
  `Event` in `events!`, and a sender in theseus-core. Otherwise core's `tests_registry` fails, naming the fix, or
  it takes a marker in that test's `RESERVED` table naming the row that brings its reader.
- **The generated TypeScript is part of the change.** Any test run of this crate rewrites
  `cockpit/src/protocol.gen/` (`the_cockpits_types_are_generated_from_the_rust_ones`). `git add` it, or the gate
  fails. A `serde_json::Value` field names its TypeScript type with `#[ts(type = "...")]`. Large integers are
  `number`.

## Tests

- `tests/wire.rs` compares each shape with its fixture in `tests/wire/`, byte for byte, both ways.
- `THESEUS_GOLDEN=write` rewrites the fixtures. Use it only for a change you mean to make to the wire, and read the
  diff: a fixture rewritten to pass hides exactly the change these tests exist to catch.

## Traps

- ts-rs can't flatten an optional struct: give the outer struct optional fields instead (as `ToolStarted` does).
- Stored floats re-parse one ULP off without serde_json's `float_roundtrip` (theseus-k52m), so a re-encoded node
  can differ from its stored bytes.
