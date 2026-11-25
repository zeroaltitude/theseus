//! The wire names: every method and every notification, each from one table (`names!`), so no constant is left out of
//! its `ALL`. `lib.rs` re-exports `method` and `notify` here, so their paths are unchanged.

/// A module of wire names, from one table: each constant with its docs, and `ALL`, built from the same table, so no
/// constant is left out of it. The reader rule's registry test (theseus-wjy, theseus-core's `tests_registry`)
/// enumerates `ALL`: each method needs its dispatch arm, and each notification its `Event` and a sender.
macro_rules! names {
    ($($(#[$doc:meta])* $name:ident = $value:literal,)*) => {
        $($(#[$doc])* pub const $name: &str = $value;)*

        /// Every name in this module, from the same table as the constants.
        pub const ALL: &[&str] = &[$($name,)*];
    };
}

/// Method names. Requests (client → server). `ALL` lets a server say which only read (theseus-2fo).
pub mod method {
    names! {
        HEALTH = "health",
        SESSION_OPEN = "session.open", SESSION_LIST = "session.list",
        /// A session's retirement by the owner, and its undo (theseus-emqx).
        SESSION_RETIRE = "session.retire", SESSION_REOPEN = "session.reopen",
        /// The owner's words on money (issue 6; `money`): reopen an execution that ended at its limit, settle or forgive a call held as unknown spend.
        EXECUTION_REOPEN = "execution.reopen", ACTION_SETTLE = "action.settle",
        /// Someone began to type into a session: the daemon warms what the message will wait for (theseus-tnky).
        SESSION_TYPING = "session.typing",
        TURN_SUBMIT = "turn.submit",
        LEDGER_TAIL = "ledger.tail", LEDGER_WINDOW = "ledger.window",
        /// The ledger's hash chain, walked (theseus-bmkxk).
        LEDGER_VERIFY = "ledger.verify",
        PROFILE_LIST = "profile.list", PROFILE_USE = "profile.use",
        /// The runtime live profile released, back to the config's `[model] live` (theseus-ervz9).
        PROFILE_RELEASE = "profile.release",
        /// Every session's and task's held profile, counted (theseus-h66v4.2): the read side of `profile.unpin`.
        PROFILE_PINS = "profile.pins",
        /// The live pin released as `profile.release` does, and with `all` every held profile let go (theseus-h66v4.2).
        PROFILE_UNPIN = "profile.unpin",
        EXECUTION_LIST = "execution.list", EXECUTION_CANCEL = "execution.cancel",
        /// `/stop` (W1): halt a conversation's work, and keep the conversation; a failure dismissed, a failed task retried (theseus-7zgsm).
        EXECUTION_STOP = "execution.stop", EXECUTION_DISMISS = "execution.dismiss", TASK_RETRY = "task.retry",
        ACTION_LIST = "action.list",
        ACTION_CONFIRM = "action.confirm",
        /// Every question waiting for the operator, across sessions.
        CONFIRM_LIST = "confirm.list",
        SESSION_HISTORY = "session.history",
        SESSION_WATCH = "session.watch", SESSION_UNWATCH = "session.unwatch",
        SESSION_NUDGE = "session.nudge", SESSION_MODE = "session.mode",
        SESSION_PROFILE = "session.profile",
        /// A new conversation from any moment of another, by reference (theseus-2qzx, `fork::SessionForkParams`).
        SESSION_FORK = "session.fork",
        /// Rewind (theseus-23rk, `rewind`): a session's messages with the
        /// files its edits changed since each, and the restore of them.
        FILES_CHANGES = "files.changes", FILES_RESTORE = "files.restore",
        CATALOG_LIST = "catalog.list", COMPILATION_LIST = "compilation.list", NODE_LIST = "node.list",
        /// Where a node went (theseus-n4m, step 12a): the contexts of its own session that held it, then its copies
        /// in other sessions over `derived_from`, each with theirs. A read, computed when asked.
        NODE_REACH = "node.reach",
        TOOL_LIST = "tool.list",
        /// The tool families each kind of place loads or defers, with their use and misses (theseus-7m7w.2, `defer::DeferListResult`).
        TOOL_DEFER = "tool.defer",
        SHUTDOWN = "shutdown",
        /// The narrative (`narrative = true`): the recent tail, then every new line as a `narrative.line` notification until `narrative.unwatch`.
        NARRATIVE_WATCH = "narrative.watch",
        NARRATIVE_UNWATCH = "narrative.unwatch",
        /// "Should have asked" (theseus-sgh): the tool asks first from now on. Stored, never in the config; it can only make a tool stricter.
        POLICY_TIGHTEN = "policy.tighten",
        /// Undo a tightening: the tool goes back to what the config says. It loosens, so it takes the same trusted answer as an approval.
        POLICY_UNTIGHTEN = "policy.untighten",
        /// Remove a content label from a session (theseus-9ymz0, `content::ContentUnlabelParams`): an `injection` label
        /// stops its calls that act (under `injection = "refuse"`), and a `secret`, `confidential` or `personal` one marks
        /// what the session holds, until the operator removes it. It loosens, so it takes the same trusted answer as an approval.
        CONTENT_UNLABEL = "content.unlabel",
        /// Publish an item into a place's conversation (the place rule, theseus-nbsh): the owner's act, from a private place.
        PLACE_PUBLISH = "place.publish",
        /// The ontology (M4 §2.8, theseus-8kk.1): the kinds, the categories and their guidance, and memberships; then
        /// the operator's writes, judged as an approval is (the owner, from a private place).
        ONTOLOGY_LIST = "ontology.list",
        /// Each hands group, its cells by state, and its cost against its cap (step 40 part 2): the cockpit's grid.
        HANDS_LIST = "hands.list",
        /// Each open execution's money and where its limit comes from, its tasks under it, and the totals (step 42a, `BudgetListResult`). A read.
        BUDGET_LIST = "budget.list",
        /// Each tool's posture layer by layer in the gate's order, for a session or every place (step 42a, `PolicyExplainParams`). A read.
        POLICY_EXPLAIN = "policy.explain",
        ONTOLOGY_CATEGORY_ADD = "ontology.category.add",
        ONTOLOGY_GUIDANCE_SET = "ontology.guidance.set",
        ONTOLOGY_MEMBERSHIP_SET = "ontology.membership.set",
        /// `categorize.v1`'s unanswered proposals (M5 28b), and the operator's answers to one, judged as the ontology's writes are.
        ONTOLOGY_PROPOSALS = "ontology.proposals",
        ONTOLOGY_PROPOSAL_ACCEPT = "ontology.proposal.accept",
        /// The owner's standing orders, preferences and facts (theseus-a7vi, `standing::StandingListParams`): a read, and the owner's two writes.
        THREAD_NEW = "thread.new", THREAD_EDIT = "thread.edit", THREAD_PAUSE = "thread.pause", THREAD_RESUME = "thread.resume", THREAD_DONE = "thread.done", THREAD_REOPEN = "thread.reopen", CARD_PROPOSE = "card.propose", CARD_LIST = "card.list", CARD_ANSWER = "card.answer", CARD_DONE = "card.done", CARD_OBSERVED = "card.observed", THREAD_SEAL = "thread.seal", THREAD_UNSEAL = "thread.unseal", THREAD_SEAL_DISMISS = "thread.seal_dismiss", THREAD_MOVE = "thread.move", THREAD_MERGE = "thread.merge", THREAD_UNMERGE = "thread.unmerge", THREAD_SPLIT = "thread.split", THREAD_SPLIT_PLAN = "thread.split_plan", THREAD_UNSPLIT = "thread.unsplit", THREAD_LINK = "thread.link", THREAD_UNLINK = "thread.unlink", THREAD_UNCLAIM = "thread.unclaim",
        /// The personas and the built-in default (theseus-slvl.2): a read.
        PERSONA_LIST = "persona.list", PERSONA_IMPORT = "persona.import", PERSONA_CARD_CONFIRM = "persona.card_confirm", PERSONA_ADD = "persona.add", PERSONA_SOUL = "persona.soul", PERSONA_SOUL_READ = "persona.soul_read", PERSONA_SET = "persona.set", PERSONA_RETIRE = "persona.retire", PERSONA_DEFAULT = "persona.default", // an agent's workspace carried over as a persona, and its card kept (theseus-a8n7)
        INITIATIVE_RATE = "initiative.rate", INITIATIVE_WEEK = "initiative.week", // a thumbs up or down on what Theseus said first; the week (theseus-slvl.9)
        VOICE_SHOW = "voice.show", VOICE_AUDITION = "voice.audition", STANDING_LIST = "standing.list", STANDING_ADD = "standing.add", STANDING_REVOKE = "standing.revoke", STANDING_PROPOSALS = "standing.proposals", STANDING_KEEP = "standing.keep", STANDING_DISMISS = "standing.dismiss", STANDING_ANSWER = "standing.answer",
        ONTOLOGY_UNDO = "ontology.undo", ONTOLOGY_KIND_ADD = "ontology.kind.add", ONTOLOGY_KIND_RETIRE = "ontology.kind.retire", JUDGE_LATENCY = "judge.latency", // Jev's writes taken back, the kinds table's two writes (theseus-xzxqv), and a point's latency by day
        ONTOLOGY_PROPOSAL_REJECT = "ontology.proposal.reject", ONTOLOGY_PROPOSAL_ACCEPT_ALL = "ontology.proposal.accept_all", ONTOLOGY_PERSON_MERGE = "ontology.person.merge", ONTOLOGY_PROPOSAL_REJECT_ALL = "ontology.proposal.reject_all", ONTOLOGY_PROPOSAL_NEVER = "ontology.proposal.never", ONTOLOGY_NEVER_LIST = "ontology.never.list", ONTOLOGY_NEVER_UNDO = "ontology.never.undo", ONTOLOGY_PERSON_NOT_SAME = "ontology.person.not_same", ONTOLOGY_PERSON_NOT_SAME_LIST = "ontology.person.not_same_list", ONTOLOGY_PERSON_CANDIDATES = "ontology.person.candidates",
        /// How often a task's forecast held (theseus-zz752, `forecast::ForecastCalibrationParams`). A read.
        FORECAST_CALIBRATION = "forecast.calibration",
        /// Tasks (DD7): the child sessions conversations started, with state and spend.
        TASK_LIST = "task.list",
        /// Stop a task and its jobs; the place hears it once.
        TASK_CANCEL = "task.cancel", TASK_PAUSE = "task.pause", TASK_RESUME = "task.resume", // the owner holds a task at its next loop, and lets it go on
        TASK_REHOME = "task.rehome", TASK_NOT_THIS = "task.not_this", // the owner says "not this" to a task: cancel it, one rolling preference, label its offer
        TASK_RESCHEDULE = "task.reschedule",
        /// The owner's second proof (issue 22, `owner_proof`): a code from their authenticator app. Pairing, its first code, the status, a reset, and ending every open window. The owner's own shell alone.
        OWNER_PROOF_ENROLL = "owner.proof.enroll", OWNER_PROOF_CONFIRM = "owner.proof.confirm", OWNER_PROOF_STATUS = "owner.proof.status", OWNER_PROOF_RESET = "owner.proof.reset", OWNER_PROOF_LOCK = "owner.proof.lock",
        /// Keeping a new secret in the vault (issue 26, `vault_keep`): the owner gives the value in a surface's own form for a card a turn's `vault.keep` put to them, or from their own terminal with no card. The value is in `vault.keep.answer` and `vault.keep` and nowhere else; `vault.keep.waiting` (which cards wait, and why none) carries none.
        VAULT_KEEP_ANSWER = "vault.keep.answer", VAULT_KEEP = "vault.keep", VAULT_KEEP_WAITING = "vault.keep.waiting",
        /// The owner starts a task with no model turn (theseus-7oq0w, `task_start::TaskStartParams`).
        TASK_START = "task.start",
        /// Wakes (DD8): the turns conversations asked for at a time, with `wake.at`, that have not run yet.
        WAKE_LIST = "wake.list",
        /// Cancel a pending wake: nothing fires.
        WAKE_CANCEL = "wake.cancel",
        /// The owner's verbs on wakes, with no model turn (theseus-8ux6x): set a reminder, snooze one, hold and resume a series, settle a fired one.
        WAKE_CREATE = "wake.create", WAKE_SNOOZE = "wake.snooze", WAKE_PAUSE = "wake.pause", WAKE_RESUME = "wake.resume", WAKE_DONE = "wake.done",
        /// Routines (theseus-pvm5c, `routines.rs`): recurring work, every run a fresh task.
        ROUTINE_CREATE = "routine.create", ROUTINE_LIST = "routine.list", ROUTINE_RUNS = "routine.runs", ROUTINE_PAUSE = "routine.pause", ROUTINE_RESUME = "routine.resume", ROUTINE_RUN_NOW = "routine.run_now", ROUTINE_SKIP = "routine.skip", ROUTINE_TRY = "routine.try", ROUTINE_UPDATE = "routine.update", ROUTINE_DELETE = "routine.delete",
        /// Watches (theseus-f2r2b): a check with no model until it holds; a cancel is `wake.cancel`'s.
        WATCH_CREATE = "watch.create", WATCH_LIST = "watch.list",
        /// Projects (theseus-n5ui2, `projects.rs`): long-term work on the task graph, with milestones, a weekly
        /// review, and a slice in every session that attaches.
        PROJECT_CREATE = "project.create", PROJECT_LIST = "project.list", PROJECT_SHOW = "project.show", PROJECT_ATTACH = "project.attach", PROJECT_TRUST = "project.trust", PROJECT_UNTRUST = "project.untrust", PROJECT_TRUSTED = "project.trusted",
        /// A project's items (issue 13): a step of the project's own list, added, closed and listed.
        PROJECT_ITEM_ADD = "project.item.add", PROJECT_ITEM_CLOSE = "project.item.close", PROJECT_ITEM_LIST = "project.item.list",
        /// The end of a project (issue 37): its open items, its review and its reminders end with it.
        PROJECT_CLOSE = "project.close",
        /// The owner's own instruction file (`own_file.rs`): what is accepted and what is on disk, with the change (a
        /// read), and the accept of what is on disk, by the digest the owner was shown. The accept is the owner's, at
        /// their own terminal alone.
        OWN_FILE_SHOW = "own_file.show", OWN_FILE_ACCEPT = "own_file.accept",
        /// The last changes the owner approved to Theseus's own settings, as words (issue 22).
        CONFIG_HISTORY = "config.history",
        /// The whole text, before and after, of a change waiting for the owner's yes: a prompt file, or a setting too long for its card (issue 23).
        CONFIG_WHOLE = "config.whole",
        UNDO_LIST = "undo.list", UNDO_ALLOW = "undo.allow", // the owner lets a call undo.v1 held run as it was first proposed (theseus-p5kln)
        /// One answer verb for every question (theseus-ds5os).
        WORK_ANSWER = "work.answer",
        /// The push (theseus-in3): a snapshot of every execution that needs someone or works, then `execution.changed`, `confirm.requested`, and `confirm.resolved` for every session, until `executions.unwatch`. A read.
        EXECUTIONS_WATCH = "executions.watch",
        EXECUTIONS_UNWATCH = "executions.unwatch",
        /// The work board (theseus-hkx30): every root that needs you or works with its open subtree, then
        /// `work.changed` and `work.notice` until `work.unwatch` (`work_watch.rs`). A read.
        WORK_WATCH = "work.watch", WORK_UNWATCH = "work.unwatch", WORK_AGENDA = "work.agenda", WORK_DIGEST = "work.digest",
        /// The owner was shown the work board at a position: the store's look moves there (issue 46). A write.
        WORK_LOOK = "work.look",
        /// A running job's last lines (`job_follow.rs`, theseus-e1hu4), and its output followed until `action.unfollow`.
        WORK_PEEK = "work.peek", JOB_STOP = "job.stop", ACTION_OUTPUT = "action.output", ACTION_UNFOLLOW = "action.unfollow",
        /// The counts behind `theseus status --short` (theseus-8kn12), read from the board's O(1) fields: no
        /// per-execution view, no subscription. `push::StatusShortResult`. A read.
        STATUS_SHORT = "status.short",
        /// Wait until a session needs someone, settles, or ends (theseus-in3). The daemon owns the wait; it answers
        /// at once when it is satisfied already. A read.
        SESSION_WAIT = "session.wait",
        /// The index tender (roadmap row 51), as health's `index` block says it: the tender as the core supervises
        /// it, and its own status. A read; the core asks the tender, bounded.
        INDEX_STATUS = "index.status",
        /// A search of the index, forwarded to the tender as it came (`index::IndexQueryParams`): fused hits as of a position. A read.
        INDEX_QUERY = "index.query",
        /// Drop the index and build it again from the WAL (theseus-jn17m): the owner's act, forwarded to the tender.
        /// The daemon writes the start, the progress and the end as `index.rebuild` rows, so the work board shows it as the daemon's own work.
        INDEX_REBUILD = "index.rebuild",
        /// Recall's pipeline over a query, as a turn in a session's place would run it (M6 step 30a; `memory::MemorySearchParams`): what it would admit, and why each other hit was dropped. A read; it writes nothing.
        MEMORY_SEARCH = "memory.search",
        /// A session's recalls (`memory::MemoryRecallsParams`): each turn's
        /// `recall.shadow` manifest, newest last. A read.
        MEMORY_RECALLS = "memory.recalls",
        /// The sources of the handles a session's replies cite (`cite::MemoryCitesParams`; theseus-ejdk). A read.
        MEMORY_CITES = "memory.cites",
        /// An operator's label on a node recall offered or should have (`memory::MemoryLabelParams`; M6 step 30b):
        /// `wrong` and `stale` keep it out of recall. Acting: the owner, from a private place.
        MEMORY_LABEL = "memory.label",
        /// The history tool for an MCP client (`memory::HistoryReadParams`; issue 46): a place's exchanges, by
        /// words, a thing or a handle, with no model call. A read.
        HISTORY_READ = "history.read",
        /// Mark a node wrong and find where it went (`retract::FactRetractParams`; theseus-2ced): the `wrong` label, a correction note for each live session that used it, its syntheses stale, one card for the tasks to re-run. Acting: the owner, from a private place.
        FACT_RETRACT = "fact.retract",
        /// Your own benchmark (theseus-pw1q.12; `own_bench`): a past turn's first request replayed on another profile (the owner's run), and the report of what the owner's turns need (a read).
        TURN_REPLAY = "turn.replay", ROUTE_REPORT = "route.report",
        /// Jev's judgments (M5 23b; `judge::JudgeListParams`): the newest `judge.call` rows, by pack, session, and
        /// time, without their states. A read.
        JUDGE_LIST = "judge.list",
        /// One judgment by its id (`judge::JudgeGetParams`): its row, and the state Jev was sent, from its blob. A
        /// read.
        JUDGE_GET = "judge.get",
        /// The learning ledger (M5 25c; `learning`): an operator's label on a judgment (acting:
        /// the owner, from a private place), and the report, stored by date or run now.
        JUDGE_LABEL = "judge.label",
        LEARNING_REPORT = "learning.report",
        /// The owner's correction of a turn's routing (theseus-q31l; `route::RouteCorrectParams`), judged as
        /// `judge.label` is.
        ROUTE_CORRECT = "route.correct",
        /// The owner's runs over the ledger (M5 25d; `judge_runs`): a candidate replayed, an audit,
        /// a backfill (only under the owner's consent); and `judge.prove`, the prove's report (L3), a read.
        JUDGE_REPLAY = "judge.replay", JUDGE_AUDIT = "judge.audit", JUDGE_BACKFILL = "judge.backfill", JUDGE_LEARN = "judge.learn", JUDGE_PROVE = "judge.prove",
        /// The ladder (M5 26a; `packs`): each pack's mode, a read; the owner's promote and rollback.
        PACK_LIST = "pack.list", PACK_PROMOTE = "pack.promote", PACK_ROLLBACK = "pack.rollback",
        /// The import (theseus-0lrr.6; `import`): a batch of episodes; a tag's erase, the owner's; the tags, a read;
        /// and a tag's topics (theseus-anh3), the owner's: its sessions' labels as ontology topics and memberships.
        IMPORT_EPISODES = "import.episodes", IMPORT_ERASE = "import.erase", IMPORT_LIST = "import.list",
        /// The imported episodes, filtered and counted by facet, a page at a time (theseus-7n3e;
        /// `import::ImportSessionsParams`): the context explorer's list. A read.
        IMPORT_SESSIONS = "import.sessions",
        /// What a turn sees, part by part with token counts (theseus-7n3e;
        /// `context::ContextExplainParams`): the system block's parts, the guidance, the tools, the
        /// recall, and the conversation. A read; it compiles nothing.
        CONTEXT_EXPLAIN = "context.explain",
        /// The next request, counted part by part down to each tool (theseus-7m7w.1; `census`). A read.
        CONTEXT_CENSUS = "context.census",
        /// The exchange ledger (issue 44, step 1; `exchange`): its measuring rows summed, a read; and the
        /// exchanges and touches of older history written from the nodes, with a dry run that only counts.
        EXCHANGE_REPORT = "exchange.report", EXCHANGE_REBUILD = "exchange.rebuild",
        /// A place's ladder built from its history (issue 44, step 4; `ladder`): its `did` lines and folds,
        /// with a dry run that counts and prices, and a cap.
        EXCHANGE_LADDER = "exchange.ladder",
        /// A reading sheet of `did` lines and first-level fold nodes beside what their writer read (issue 44,
        /// step 4; `exchange::ExchangeGistsParams`), drawn by a seed. A read for the owner in person.
        EXCHANGE_GISTS = "exchange.gists",
        /// The exchanges from before topics were filed per exchange, each opening message asked of the
        /// ontology's judge once and its topics touched (issue 46), with a dry run that counts and prices, and a cap.
        EXCHANGE_TOPICS = "exchange.topics",
        /// One turn, whole (theseus-x9swu; `inspect::TurnInspectParams`): the cockpit's inspector at
        /// `/session/<id>/turn/<turn>`. A read; thinking and texts to a private place only.
        TURN_INSPECT = "turn.inspect",
        /// What a turn was shown and why (issue 44; `view::ViewShowParams`): the view record of each view the
        /// turn built. A read; ids, kinds and counts, text only for the owner at his own terminal.
        VIEW_SHOW = "view.show",
        /// The view a place's next turn would get, assembled now and neither sent nor kept (issue 44;
        /// `view::ViewPreviewParams`). A read: the owner's in person, or an agent's of its own place.
        VIEW_PREVIEW = "view.preview",
        /// The books, first cut (theseus-civ0; `books`): each book's count and span, and a book's
        /// episodes newest first, filtered and paged. Reads.
        BOOKS_LIST = "books.list", BOOKS_PAGE = "books.page",
        IMPORT_TOPICS = "import.topics", IMPORT_PEOPLE = "import.people",
        /// The OpenClaw sync (theseus-pjmj7, theseus-ievo3; `import_sync`): the owner's config and cursor for a
        /// run, a read; the run's end (cursor, counts, the people steps); and a sync asked for NOW, which the
        /// daemon RUNS then and there, one at a time (S29-1). The last two the owner's.
        IMPORT_SYNC_PLAN = "import.sync.plan", IMPORT_SYNC_RECORD = "import.sync.record", IMPORT_SYNC_REQUEST = "import.sync.request",
        /// The gates' bench history on this machine (theseus-1hk), for the
        /// cockpit's speed wall: every recorded run's p50s, p95s, and limits
        /// (`bench::BenchHistoryResult`). A read of the gate's CSV.
        BENCH_HISTORY = "bench.history",
        /// The L1 jobs running now, with their commands (M4 17b, theseus-kpz1), for the cockpit's boundaries board
        /// (`sandbox::SandboxUsage`). A read of the daemon's memory.
        SANDBOX_USAGE = "sandbox.usage",
        /// The AWS bootstrap (AWS design §5, C2; `AwsBootstrapParams`): the
        /// plan of an account's foundation, posture, and relay stacks,
        /// read-only, or that plan applied on the operator's yes. The
        /// operator's alone: refused from a job's process.
        AWS_BOOTSTRAP = "aws.bootstrap",
        /// The alerts subscription confirmed with the token from SNS's email,
        /// authenticated on unsubscribe (theseus-9p40). The operator's alone.
        AWS_CONFIRM_ALERTS = "aws.confirm_alerts",
        /// The MCP servers the config attaches and their tools (M7 36b, `mcp::McpListResult`). A read.
        MCP_LIST = "mcp.list",
        /// Restart one MCP server (`mcp::McpRestartParams`), a failed one included.
        MCP_RESTART = "mcp.restart",
        /// Sign in to an MCP server that signs in (issue 55; `mcp::McpLoginBeginParams`), sign out. The CLI's alone.
        MCP_LOGIN_BEGIN = "mcp.login.begin", MCP_LOGIN_FINISH = "mcp.login.finish", MCP_LOGOUT = "mcp.logout",
        /// The prompts the MCP servers list (M7 36c, `mcp::McpPromptListParams`). A read.
        MCP_PROMPT_LIST = "mcp.prompt.list",
        /// Proposed and loaded extensions (M7 43a, 43b; `extend::ExtendListResult`). A read.
        EXTEND_LIST = "extend.list",
        /// Revoke a loaded extension (43b, `extend::ExtensionRevokeParams`). The operator's.
        EXTENSION_REVOKE = "extension.revoke",
        /// Self-improvement (theseus-pw1q; `rsi`): the kill switch, anyone's halt and the owner's resume, the log, the digest.
        SELF_HALT = "self.halt", SELF_RESUME = "self.resume", SELF_LOG = "self.log", SELF_DIGEST = "self.digest",
        SELF_BACKLOG = "self.backlog", SELF_DROP = "self.drop", SELF_MOVE = "self.move", SELF_RETRY = "self.retry", SELF_FILE = "self.file", SELF_BRANCHES = "self.branches",
        SELF_EXAMS = "self.exams", SELF_REVERT = "self.revert", SELF_PIPELINE = "self.pipeline", SELF_PATTERNS = "self.patterns",
        /// Pause or unpause new self builds (issue 6, theseus-7glm6; `rsi::SelfPauseParams`): a state of the running daemon, cleared by its restart.
        SELF_PAUSE = "self.pause",
        /// Narrower policies for the hand roles (theseus-pw1q.15; `rsi::SelfNarrowParams`). The owner's to apply.
        SELF_NARROW = "self.narrow",
        /// Google Workspace (`google`): the one-time consent, the revoke, and the status. The CLI's alone.
        GOOGLE_AUTH_BEGIN = "google.auth.begin", GOOGLE_AUTH_FINISH = "google.auth.finish", GOOGLE_REVOKE = "google.revoke", GOOGLE_STATUS = "google.status",
        /// The calendars over a window, for the owner's page (`calendar::CalendarWindowResult`); its `calendar.changed` carries no content. The owner, in a private place.
        CALENDAR_WINDOW = "calendar.window",
    }
}

/// Notification names (server → client). Each has its `Event` variant, which
/// senders build and clients match on.
pub mod notify {
    names! {
        TURN_STARTED = "turn.started",
        LOOP_STARTED = "loop.started",
        MODEL_DELTA = "model.delta",
        TOOL_PROPOSED = "tool.proposed",
        LOOP_ENDED = "loop.ended",
        TURN_ENDED = "turn.ended",
        PROFILE_CHANGED = "profile.changed",
        /// Thinking summaries / progress updates as they stream.
        MODEL_THINKING = "model.thinking",
        /// One loop's answer is whole (theseus-ck0n): its stream ended, sent before its
        /// settle's frame is written, so a client shows the whole text at once. `text` is
        /// the loop's whole text, so a client that dropped a delta still ends with it.
        MODEL_ANSWERED = "model.answered",
        /// The context for a loop was compiled (append or recompile, sizes, digest).
        CONTEXT_COMPILED = "context.compiled",
        /// A tool call started (after the gate) and ended (with its result).
        TOOL_STARTED = "tool.started",
        TOOL_ENDED = "tool.ended",
        /// A tool call needs the operator's confirmation; the turn has parked.
        CONFIRM_REQUESTED = "confirm.requested",
        CONFIRM_RESOLVED = "confirm.resolved",
        /// A node was written to a watched session (history stays live).
        NODE_WRITTEN = "node.written",
        /// A turn failed after it was admitted (provider error, store error). The
        /// requester also gets the error response; watchers only get this.
        TURN_FAILED = "turn.failed",
        /// A call ran under a `notify` posture (`[policy].enforcement`, or a
        /// `[policy.tools]` / `[policy.mcp]` line), and the operator is told.
        POLICY_NOTIFIED = "policy.notified",
        /// A tool was tightened, or its tightening undone (theseus-sgh). These go
        /// to every connection watching a session, once each, since a tightening
        /// holds for every session. The params are a `TightenResult`.
        POLICY_TIGHTENED = "policy.tightened",
        POLICY_UNTIGHTENED = "policy.untightened",
        /// The operator removed a content label from a session (theseus-9ymz0), to the session's watchers. The params are
        /// an `UnlabelResult`.
        SESSION_UNLABELLED = "session.unlabelled",
        /// A content label was raised on a session (theseus-9ymz0), to the session's watchers. The params are a
        /// `LabelRaised`.
        SESSION_LABELLED = "session.labelled",
        /// One line of the narrative, to every `narrative.watch` subscriber.
        /// Unlike the others it is not a ledger row: the narrative is never stored.
        NARRATIVE_LINE = "narrative.line",
        /// An execution's view changed (theseus-in3): to the session's watchers and to every `executions.watch`
        /// subscriber, once per committed frame that changed it. Its params are an `ExecutionView`, whose frame
        /// carries the ledger rows.
        EXECUTION_CHANGED = "execution.changed",
        /// The connection fell behind (theseus-in3): its queue passed the backlog cap, so notifications were dropped,
        /// counted, until the queue drained. Re-read each stream named. Transport, like `narrative.line`: counted in
        /// health, never a ledger row.
        EVENTS_LOST = "events.lost",
        /// A notified call's `security.v1` score, after its notice (M5 24).
        JUDGE_SCORED = "judge.scored",
        /// `security.v3`'s live notice on an open call it is sure was risky.
        JUDGE_NOTICED = "judge.noticed",
        /// A task record changed (39a): `tasks::TaskChanged`.
        TASK_CHANGED = "task.changed",
        /// A notice wake fired (theseus-8ux6x), to every connection; a mode or plan changed (theseus-lyxy).
        WAKE_NOTICED = "wake.noticed", SESSION_MODE = "session.mode",
        /// The work board's frame (theseus-hkx30): `work_watch::WorkChanged`, to
        /// every `work.watch` subscriber; and the policy's `notices::Notice`.
        WORK_CHANGED = "work.changed", WORK_NOTICE = "work.notice",
        /// A followed job's output (`job_follow.rs`): a chunk from a byte to the connection that asked, then its end.
        ACTION_OUTPUT_CHUNK = "action.output.chunk", ACTION_OUTPUT_END = "action.output.end",
        /// Self-improvement's switch moved (theseus-pw1q.19): `rsi::SelfChanged`, to every connection that watches
        /// every session.
        SELF_CHANGED = "self.changed",
        /// What capture did with the owner's words (theseus-slvl.5): a record kept, or a question.
        STANDING_NOTICED = "standing.noticed",
        CALENDAR_CHANGED = "calendar.changed",
    }
}
