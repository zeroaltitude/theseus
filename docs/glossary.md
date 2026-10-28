# Glossary

The words Theseus uses, as the code and the docs use them today. Where one word means several things, each meaning is
listed with where it is used. Paths are from the repository root; "spec" is [the Ship of Theseus](spec/README.md);
[cli.md](cli.md) lists the commands.

The owner's six nouns are *conversation*, *thread*, *plan*, *step*, *task* and *job*. Each entry opens with its one line,
which every surface (the cockpit, the CLI, the terminal UI and these docs) says the same way. The entry then says
where the code keeps it.

**"Task" means only the running agent.** A record on the work board is a *plan* or a *step*, never a task. The
names in the code and on the wire keep their old spelling and are not words a person reads: `task.record` (the store's
`TASK` kind), `TaskRecord`, `task.list`'s `records`, `task.changed`, `task.split`, `task_create` and `task_close` (the
model's tools), `WorkKind::Step`, `PlanItem` and every command (`theseus tasks`, `theseus task ...`).

## A

**activity thread**: the Discord crate's name for a Theseus thread (see *thread*, first sense) when it makes a Discord
thread for it: "one Discord thread per activity thread" (`crates/theseus-discord/src/runtime/guild_thread.rs`). The
Discord thread is recorded in the Theseus thread's `surfaces` and run as a place of its own.

**ask**: Anything waiting on your answer: an approval, a question, a budget, a failure to look at, a change to a step's goal. The Needs-you list holds the asks.
In the code it is a confirm request, a question, a budget question or a failed row; `theseus answer` answers one and
`theseus status` counts them.

## B

**binding**: the Discord binding, `theseus-discord`, which runs inside the daemon and acts only through the protocol
(`crates/theseus-discord/AGENTS.md`). Also *the bindings file*: the operator's file in the state dir that says which
guilds, channels and DMs are places, which are `private`, and each place's ceiling.

## C

**card**: four things. (1) A *question card*: a notification with buttons posted to a place (a confirm card; the
outbox delivers it; spec §3.16). (2) A *task card*: a task's post in its place, naming the task. (3) The *owner card*: facts
proposed from imported history that you keep, edit or decline (`theseus card`, `[card]`; `crates/theseus-core/src/card/`).
(4) A *thread's card*: a few lines of what a thread holds (`MAX_CARD` in `crates/theseus-protocol/src/threads.rs`).

**census**: `theseus census`, the next request counted part by part down to each tool, sent nowhere
(`crates/theseus-core/src/census.rs`). `--place` takes `cli`, `dm`, `channel`, `task`, `headless` or `harness`
(`PLACES` in `crates/theseus-protocol/src/census.rs`); `harness` is the census's own sixth name and is not a
*place kind*.

**cockpit**: the daemon's web UI, a Vite app served at `/` by `theseusd` (`cockpit/AGENTS.md`). Its build is not
committed. A *private* place.

**conversation**: Where you talk with Theseus in one place (a DM, the CLI, a channel, the cockpit). It takes one turn at a time.
In the code it is a session of kind `conversation`, one of the two `SessionKind`s, beside `task`
(`crates/theseus-protocol/src/lib.rs`). A person talks in it; a task works for it. It is also a kind of execution.

**conversation mode**: one of `ask`, `edits`, `plan`, `auto` (`SessionMode`, `crates/theseus-protocol/src/modes.rs`),
set with Shift+Tab or `theseus mode`. It changes how a conversation's tool postures act. The only thing the word *mode*
names (see *mode*).

## E

**execution**: the kernel's durable record of a session's work, with its state, turns, outstanding actions and budget
(`crates/theseus-kernel/AGENTS.md`). One per session. `theseus sessions --state` lists them.

## F

**first request**: the first request a fresh session sends for a kind of place, counted at the provider's figures. It
has a token budget per place (cli/task 8,000, dm 15,000; `crates/theseus-core/src/first_request.rs`).

## H

**herdr**: the terminal workspace manager Theseus reports to; `theseus herdr` keeps a pane per task that needs you or
works (`docs/herdr.md`).

## J

**Jev**: the judge Theseus puts typed questions to inside the loop (which model a message needs, whether to recompile, rerank, and others); it never owns a safety guard. Spec §3.7;
`crates/theseus-judge/` holds its client and the *packs*.

**job**: A command run for a turn or a task, such as a build or a test run.
In the code it is a process the daemon runs for a session (`proc.run` and the like), detached, durable and cancellable,
wrapped by `job-wrapper` (`crates/theseus-kernel/src/job.rs`). `theseus peek` and `attach` read one.

**judgment**: one typed question put to Jev, recorded with its probabilities, outcome and cost, in shadow before it
acts. `theseus judge log` lists them.

## L

**label** (a content label): one of four flags on a session, `injection`, `secret`, `confidential` or `personal`, with what
raised it. Jev's content pack raises them, and so do two free rules: a `[privacy] sensitive_names` word and a Google
result. `secret`, `confidential` and `personal` keep the session off a model of restricted origin (`cn_models` reads
them); `injection` refuses the session's calls that act until you remove it. Only you remove one, and a removal sticks
(`crates/theseus-core/src/content.rs`). `theseus content SESSION`. Not the learning ledger's *labels* (your right, wrong
or noise on a judgment), which a removed content label also writes.

## K

**keep-warm**: after a conversation's last turn, one small request every `[cache] keep_warm_every_minutes` that reads the
cached prefix and so keeps it from expiring (`crates/theseus-core/src/keep_warm.rs`).

## M

**mode**: a conversation's mode: `ask`, `edits`, `plan`, `auto`. The word used to name more: a routing *task class*
(`[routing.classes.<name>]`), a pack's *rung* (`[routing] rung`, `[judge.packs.*] rung`), and `on_max_loops` are the new names.
That leaves `[memory] mode` (`off`, `shadow`, `canary`, `live`), `[self] mode` (`off` or `act`) and the spend-limit modes
(`[kernel] spend_limit_mode`, `daily_spend_ceiling_mode`) to rename later.

**task class**: what the route pack decides a message needs (`trivial`, `quick`, `chat`, `sophisticated`, `deep_coding`,
`routine_coding`, `review`, `other`), with the profiles that serve it in `[routing.classes.<name>]`; `[routing] rung` is a
different value, `live` or `shadow`. `theseus why` prints it as "task class".

## N

**node**: one record in the conversation graph (a message, a tool call, a report, a note), with a short id such as
`msg·a1b2c3` (`crates/theseus-core/src/node.rs`). `theseus history`, `reach`, `fork` and `retract` take one.

## P

**pack**: a Jev question as versioned TOML data (`crates/theseus-judge/src/pack.rs`). Each pack has a mode.

**rung** (a pack's mode): what a pack may do: `off` (not called), `shadow` (called and recorded), `canary` (acts for a share of
sessions) or `live` (acts everywhere). The ladder moves a pack between them; the config's `max_mode` is a ceiling; a pack's own `rung` lowers it
(`crates/theseus-core/src/config/judge.rs`, `crates/theseus-core/src/judge/ladder/`). `theseus packs` shows each pack's rung.

**persona**: who Theseus speaks as: a name, voice and files, chosen by session, place or `[context] default_persona`
(`crates/theseus-core/src/persona.rs`). `theseus persona`.

**place**: where Theseus speaks: the CLI, the web UI, a DM, a guild channel, a task's session, or a virtual place
`thread:<slug>`. See *place class* and *place kind*.

**place class**: `private` or `shared` (`PlaceClass`, `crates/theseus-protocol/src/places.rs`). Private (the CLI, the
web UI, a DM with the owner, a channel bound `private = true`) gets everything; shared gets its own conversation and
the public tools only. `theseus places`.

**place kind**: `cli`, `dm`, `channel`, `task` or `headless` (`PlaceKind`,
`crates/theseus-core/src/tools_defer/family.rs`), the kind of place a loaded tool set is kept for. `cli` covers the
CLI, the terminal UI and the web UI. Not the same as the place class, or the *surface*.

**plan**: A piece of work broken into parts. It holds steps and smaller plans, shows *n of m* done, and folds open and shut.
In the code it is a record on the work board (a `TaskRecord`) that has steps under it; `task.list`'s
`records` carry it, and the plan of a conversation in `plan` mode is its steps (`PlanItem`,
`crates/theseus-protocol/src/modes.rs`, `crates/theseus-core/src/task_graph/plan.rs`). *Plan* is also the name of
that mode (see *conversation mode*); the gate's internal `Plan` type (what a call will do) is a different thing and is
never shown.

**posture**: what the gate does with a tool: `open`, `notify` or `approve`; the floor always asks. Spec §3.9;
`theseus policy` shows each tool's.

**profile**: a model profile (`[profiles.*]`); one is live, `theseus profile use NAME` switches it.

**project**: a long-running plan with a goal, dated milestones and a weekly review, a kind of plan, not a fourth
container; its milestones and steps are records under it, and a *slice* of it rides every attached conversation
(`crates/theseus-core/src/project/`). `theseus project`.

## R

**recall**: the pipeline that picks notes from memory for a turn, by BM25, entities and vectors, then ranks them
(`crates/theseus-core/src/recall.rs`, `theseus-index`). `theseus memory search` runs it, writing nothing.

**reminder**: a wake with purpose `Remind` that posts a note when due, with Done and Snooze (`theseus remind`; kernel
`wake_kinds.rs`).

**routine**: recurring work: a conversation's execution with one pending wake, each occurrence a fresh task with its
own budget (`crates/theseus-core/src/routine.rs`). `theseus routine`.

## S

**session**: the unit that holds a turn at a time; a session is a `conversation` or a `task` (spec §3.2a). Its
execution is its durable record.

**standing order**: a record of the owner's, kept in every prompt of its scope until revoked: a kind (order,
preference or fact) and a scope (everywhere, a place, a session, or a surface). `theseus standing`
(`crates/theseus-core/src/standing.rs`).

**step**: One piece of work, checked off when done. When something is listed under it, it becomes a plan: same card, same history.
In the code it is a record on the work board with no session (`TaskRecord`; `WorkKind::Step`), made by `task_create`
without a brief or by approving a plan. `theseus task show` says of one that it is a step with no task, because no run
works on it. A project's milestones and steps are steps too.

**store format**: the one number, `MANIFEST_FORMAT` (`crates/theseus-store/src/store.rs`), that a step bumps when it
changes a stored record. A build refuses a store newer than it knows.

**surface**: where a person wrote from. A person reads one spelling: `terminal` (the CLI and the TUI), `cockpit` (the web
UI), `discord`, `voice` (Discord's voice) and `mcp` (`standing::surface_name`, which `standing add --surface` takes).
`approval::Surface` is the listener that accepted a connection and stores `cli`, `web`, `discord`, `mcp` and `unnamed`
(`crates/theseus-core/src/approval.rs`); `approval::display_name` is the one function that turns a stored `cli` or `web`
into `terminal` or `cockpit`. A message node records a short code for it (`surface.rs`).

## T

**task**: An agent Theseus starts to carry out a step or a plan on its own, with its own budget and a fresh context. It reports back to where it came from.
In the code it is a session of kind `task`: a child that works on its own and reports back (`task.create`; spec
§3.2a, `crates/theseus-core/src/task.rs`). Its budget is carved from its parent's. `theseus tasks`,
`theseus task start`.

**task thread**: in Discord, the thread (or private channel) a task's place lives in, so the task's posts and steers go
there (`crates/theseus-discord/src/runtime/task_channels.rs`). It is a task's place. It is not a Theseus thread.

**thread**: A named line of work with its own conversation. On the Graph it is a swimlane, and everything it started hangs in it.
In the code, three things. (1) A *Theseus thread*: a durable line of work bound to a virtual place `thread:<slug>`, with a
session of its own (`theseus thread`, `ask --thread SLUG`, `[threads]`;
`crates/theseus-protocol/src/threads.rs`). (2) The Discord thread made for one: the Discord crate's *activity thread*.
(3) A *task thread*: a task's Discord place (see above).

**turn**: One run of the model in a conversation or a task, from a message to its reply. ("working" counts turns.)
In the code it is one run of the loop for a session: it ends when the model has no more tool calls to make. A turn is
made of loops, each one provider request (spec §3.3a).

## W

**wake**: a pending turn or note scheduled for a time (`wake.at`), with a purpose (`Check`, `Remind`, `Routine`, `Watch`)
and a run. `theseus wakes` lists them; `pause`, `resume`, `snooze` and `cancel` act on them
(`crates/theseus-kernel/AGENTS.md`).

**watch** / **watches**: `theseus watch SESSION` follows a session live. `theseus watches` are something else: a wake of
purpose `Watch` that checks a command, URL or file on a timer, with no model, until a condition holds
(`crates/theseus-core/src/watch/`).
