# theseus-core

The agent itself: config and secrets, sessions, the turn, the context compiler, tool calls and the gate, the protocol
server, the push, the outbox, and telemetry. Read by theseusd, theseus-discord, and theseus-sim.

Key modules: `turn.rs`, `compiler.rs`, `toolrun.rs`, `rpc/`, `config.rs`, `crash.rs`, `aws/`. Read by: theseusd, discord, sim.

## What's here

- **The turn**: `turn.rs` (the turn runner: one turn under a session's turn lock, its loops, its frames), `advancer.rs`
  (continue or end the turn), `harness.rs` (the harness loop, parked on its events and the heartbeat), and
  `rpc/driver.rs` (what that loop drives: the heartbeat's spool drain and reconcile, continuation turns, due wakes,
  and the cancel path).
- **Context**: `compiler.rs` (manifests, recompiles, the cache layout, the token estimate), `context_files.rs`, and
  `catalog.rs` (each model's window, prices, and caching).
- **Places** (the place rule, theseus-nbsh; it replaced 19a's labels on nodes): `places.rs`. Every place a session
  speaks in is private or shared. Private: the CLI and the web UI (no place), a DM with an owner (`Config::owners_for`),
  and a guild channel the binding bound `private = true`; it gets everything. Shared: every other place; it gets the
  public tools (`places::public_tool`), the file tools only under `[places] public_paths` (canonical roots, so a link
  out of a tree is outside it), and only the context files marked `readers = "public"`
  (`context_files::withhold_shared`). A turn's class is `TurnRunner::class_of`: where its words go, which is its
  session's place (a task's is its parent's, `outbox.target`), or, for a session whose place moved on, where its wakes
  and reports answer (`outbox.wake_target`); shared when that cannot be read. It is fixed in `TurnCtx.class` once the
  turn has taken its wakes and reports. The catalog (`ToolRuntime::definitions_for`) and the tools note
  (`system_note_for`) are per class, and the gate refuses a shared place's other calls (`places::refusal`, answered
  `Not run: …`, reason `place: …`). The binding tells the core its places as it starts (`Core::bind_places`, in
  memory: an unnamed guild place is shared), and reads each private channel's viewers once (`private_place_viewed`,
  `place.viewed`), for health's `places:` line. M6's recall and the books, when built, draw in a shared place only on
  that place's own sessions. Tests: `tests_places.rs`, `places::tests`.
  - **Publish** (`rpc/publish.rs`; graduation's light form): `place.publish` puts one item (a node, a file the owner
    can read, or a message) into a bound place's session as the owner's message, with a `derived_from` edge for a
    node (`publish`), its `place.published` row, and a notice post in the place, in one frame under the place's
    execution lock, never while a turn holds it. Only the owner, from a private place: `judge_act(Act::Publish)`
    (a job's process never may), then `places::may_publish`. `theseus publish`, Discord's `/publish`.
- **Tool calls**: `toolrun.rs` (every call the model makes becomes a kernel action: the gate and the dispatch), with
  a job's call in `toolrun/job.rs`, the continuation in `toolrun/resume.rs`, and the results no call's own run
  writes (late ones, and a cancel's) in `toolrun/late.rs` (theseus-5gw9). The gate's parts are `policy.rs`
  (postures and the floor), `external.rs` (the hold after external text), `broker.rs` (granted secrets),
  `approval.rs`, and `peer.rs` (who is asking: a job's process can't answer). Plus the harness's own tools,
  `task.rs` and `wake.rs`, the web tools in `web/`, and AWS in `aws/`: the bound accounts, each key's check after
  serving (its calls fail closed until STS names the account), and `aws.call`, `aws.describe`, `aws.whoami`, and
  `aws.s3.list` (reads only until 14b; a write is invalid input).
- **L1** (M4 17b): `sandbox.rs`: `[sandbox]`, a job's class (toward L1 alone: the default, `l1_argv`, the model's
  `sandbox: true`), L1's posture (notify), the view an L1 job gets, the probe after serving (`PROBE_AFTER`), the
  delegated cgroup (systemd's own answer), and health's block. Its facts are `fact/sandbox.rs`; its tests
  `tests_sandbox.rs`, and the daemon's `tests/sandbox.rs` with real L1 jobs. What a call's proposal binds about
  its job is `sandbox::Bound`: its class, and (18c) its egress list.
- **Grants in L1** (theseus-w5op; decided by Eddie 2026-10-03, superseding theseus-7y9y): an L1 job takes its
  program's broker grants at its launch, exactly as an L0 job does. `sandbox::decide` runs the L1 decision through
  `ToolRuntime::brokered` (decision 15: the stricter of the call's posture and the secret's, so any approval comes
  before the launch), and `sandbox::for_job` asks `Broker::for_job`. The value rides in the job's environment over
  the spec's pipe, the wrapper withholds it from the job's output (`redact`), and the result's head names what the
  job was given (`given GH_TOKEN`, names only, from the wrapper's `detail.sandbox.granted`). Tests:
  `tests_grants.rs`, and the daemon's `tests/sandbox.rs` with a real L1 job. 18d's run-time socket
  (`theseus-cred get`) is gone; a stored `cred.request` action and its ledger rows still read.
- **Egress** (M4 18c): `egress.rs`. `[sandbox] egress` and a call's `sandbox: { egress }`; the gate's step for
  hosts beyond the list (the call waits, and its approval reaches those hosts alone, since the list is in its
  proposal); and what a completion's `detail.egress` makes of a result: its `sandbox.egress` and
  `sandbox.egress_refused` rows (in the frame that writes the result), its lines, and DD5's `external` marker
  when the job connected out, so T1 holds the session (`via: egress`) and the node is untrusted, the owner's. Its
  tests: `tests_egress.rs`, and the daemon's `tests/sandbox.rs` through a real proxy.
- **Cancellation** (M4 18a): `cancel.rs`, the one stop every path that stops running calls goes through
  (`ToolRuntime::terminate_all`: a cancel, a task's cancel, `/stop`, the disk's floor, a stop at a job's launch).
  A job's wrapper is asked to stop its tree; an async tool's task is aborted (`Stops::track`) and verified once its
  handle has finished; anything else is unsupported. Each verdict lands on its action, in health's `cancels`, and
  as a fact (`fact/cancel.rs`). An aborted call's result waits for the cancel's verdict (`after_abort`). Its test is
  `tests_cancel.rs`; the daemon's are `tests/job_wrapper.rs`, `tests/sandbox.rs` (L1), and `tests/tasks.rs`.
- **The protocol server**: `rpc/` (`server.rs` routes each method by name; `methods.rs`; `confirms.rs`), with
  `bus.rs` and `outbound.rs` (one ordered, capped queue per connection).
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
  `config/theseus.example.toml`; `config_overlay.rs` prints it with an operator's private overlay in place
  (`example-config`, theseus-dxgb), so the public template never carries the deployment's own values.
- **The index tender's supervisor**: `tender.rs` (row 51): it starts `theseus-index` 2 s after serving
  (`START_AFTER`, so a start's aftermath stays quiet), restarts it with backoff, takes over the one an exec kept
  at once, and asks it for health and `index.query`, each call bounded (health asks only a tender that runs, and
  never past 100 ms). Its rows are facts (`fact/index.rs`). Its tests, `tests_tender.rs`, run on tokio's paused
  clock with a stand-in `Os`.
- **`store.rs`** is the kernel's view of storage: `Store::for_turn`, a session's writes, and the turn's transcript.

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
- **The push's board**: `crates/theseus-core/src/push.rs`, fed by `Kernel::observe`; `attention()` is in
  `crates/theseus-protocol/src/push.rs`.
- **The outbox**: `crates/theseus-kernel/src/outbox.rs`, `crates/theseus-core/src/outbox.rs`, and
  `crates/theseus-discord/src/courier.rs`.
- **The config**: `crates/theseus-core/src/config.rs`, and the template `crates/theseus-core/config/theseus.example.toml`
  (`theseusd example-config` prints it, with an operator's private overlay in place: `config_overlay.rs`).
- **L1**: `crates/theseus-core/src/sandbox.rs` (`[sandbox]`, the class, the probe), and the wrapper's L1 path in
  `crates/theseus-kernel/src/job_l1.rs`.
- **The index tender**: the binary in `crates/theseus-index`, its supervisor in `crates/theseus-core/src/tender.rs`
  (started 2 s after serving, restarted with backoff, SIGTERM at a stop), and its child kind in
  `crates/theseus-kernel/src/children.rs`.

## Invariants

- **The frame budget.** A plain one-loop turn writes 5 frames, and each loop with one in-process tool adds 4.
  Observability rows ride in the turn's next frame; the session's write rides in `end_turn`'s
  (`Store::defer_session`). `tests_m3::a_plain_turn_stays_within_its_frame_budget` fails a sixth frame.
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
  stricter wins.
- **An L1 call runs at notify** (Eddie's decision, 2026-10-02): none of the L0 order applies, since the view hides
  the floor, the approve list's paths, and the socket; the external-text hold still does, and so does the
  operator's own word about the tool: a `[policy.tools]` line or a tightening that asks makes it wait
  (theseus-jfs6), and so do hosts its call names beyond `[sandbox] egress` (18c), and a granted secret's posture,
  as at L0 (theseus-w5op). The inherited
  `[policy].enforcement` never does. Its proposal names its class, so a confirm binds it, and a confirmed call runs in the class its
  proposal names. Nothing falls back from L1 to L0.
- **Results tell the truth.** `toolrun::cap` cuts on line edges and says what it left out, with the tool's own way
  to get the rest (`Tool::rest`). A listing names its scope.
- **Thinking goes back only to the provider that wrote it**, and every recompile strips the prefix's thinking.
- **A shared place never receives the owner's material** (the place rule). Its model is offered only the public
  tools, the gate refuses anything else, and its system block carries only public context files. A task takes its
  parent's class. Nothing flows from a private place to a shared one but what a person says there, and the owner's
  publish. A class is fixed for the turn, as the spec is, so a change applies at the next turn's first loop.
- **Nothing retractable goes in the shared header** (Appendix F), so sessions on a profile share one cache entry.
- **The config template is the contract.** Change a default there, not only in code. The loader rejects unknown
  keys, and `example_template_uncommented_still_parses` un-comments every line and parses it.
- **Approval fails closed** (review 2's consideration 2). Without `[approval]`, only the CLI and a Discord DM the
  bindings file binds answer; the web UI and a guild channel answer once the section names them, and health then
  says `approval: open`. A test's bare label answers as the CLI (`From<&str> for Answerer` is test-only).
- **A result that is outside text writes its session's hold in its own frame** (theseus-9bp): `external::with_hold`
  for a result written alone, and `external::under_hold` for a frame that may carry one (a late result, the cancel's
  sweep), which is built first without the session's lock and again under it only when it does. A job of a program
  `[policy] external_programs` lists is outside text too (`external::Listed`, read from the call's input by every
  `job_result`), and a session opened, or sent a turn, with `opened_from` a holding session takes its hold in the
  frame that writes it (`external::from_job`; every job's `THESEUS_SESSION`). Both are light guards under default
  trust, not boundaries: a job can strip its own environment (theseus-b5cl).
- **Secrets**: no value in a log, a row, a node, a result, or an error. `scrub.rs` scrubs tool output: each board
  value verbatim, in base64, and percent-encoded, and the shapes of secrets never resolved here (token prefixes,
  AWS keys, private-key blocks, JWTs). The broker hands a value only to the program it is granted to, run by its
  own argv, and never to one the call could make it run (`broker::launches`: the call's own environment, gh's
  aliases and extensions, git's aliases, `-c`, and the programs its options and URLs name), in L1 as at L0.
- **The AWS keys and the providers' keys are harness-only** (theseus-gh7): Theseus's own tools read them from the
  board (`aws/`, the providers), and no job is handed one unless `[broker]` names it (`Broker::may_hand_out`). `broker::harness_only` is the line `theseusd check` and health print, naming them; the
  template test holds it (`the_templates_harness_only_keys`). Containment is that, the operator's `[broker]` grants,
  and the egress list: credentials as stand-ins (TLS interception at the proxy) were dropped for v1 (Eddie,
  2026-10-03). Do nothing heavier without his say.
- **One fact, recorded once.** A new ledger row, notification, or narrative sentence is a fact's projection in
  `fact/`, not a hand-written channel at its site. Recording writes no frame: its row rides in the turn's next frame
  (or a frame now, outside a turn), and a row that must ride in a frame the site builds is `Rec::row`, with the rest
  announced once that frame is written (`Rec::announce`).

## Tests

- In-process suites in `src/`: `tests_m3.rs` (turns through the whole core), `tests_continuations.rs`,
  `tests_failures.rs`, `tests_tasks.rs`, `tests_wakes.rs`, `tests_external.rs`, `tests_overflow.rs`,
  `tests_push.rs`, `tests_config.rs`, `tests_books.rs` (a fault at each exit after a paid loop still closes the
  books, R1), `tests_refused.rs` (a corrupt record skipped, counted, and the driver still continuing, R4), and
  `rpc/tests.rs`.
- `tests_registry.rs` is the reader rule's test. The gate runs it alone, before the suite.
- `tests_schemas.rs` holds the store's version rule (P5b; Review 2's R8): each record kind's type, filled through
  its own `Deserialize` (every field, every variant), has its shape recorded under its schema number in
  `tests/golden/record_schemas.txt`. A changed shape fails until the kind's number moves. A new record type, or an
  internally tagged enum in one, gets its sample there.
- `tests_output.rs` is the output golden (theseus-j6qn): scripted scenarios through whole cores, and every frame,
  notification, and narrative line they produce, against `tests/golden/core_output.txt`. A refactor of a channel
  leaves it byte-identical; `THESEUS_GOLDEN=write` rewrites it, for a change you mean, and the diff is the review.
  A narrative line that the machine's load decides is left out there (`by_the_load`); a new one joins it.
- `tests_outside_text.rs`: property tests over every reader of outside text (the HTML reader, the wake's time
  parsers, the provider's SSE lines, and the scrubber, which also never lets a planted value through), with a fresh
  seed each run, so the gate keeps looking (Item 13). The release
  profile aborts on a panic, so one panic on outside text takes the daemon down. A new reader joins them.
- `FakeProvider::scripted` scripts a provider's answers. `Parts::for_tests` builds a core around it, and a second
  provider can be inserted into `parts.providers`.
- `Store::fail_turn_frame` (test-only) fails the first turn frame whose records match, once, as a full disk would.
- Debug builds assert, at every read, that a turn's kept transcript equals a fresh read of the store. A node written
  past the turn's handle panics a debug daemon.

## Traps

- Every child process the core starts (an `op`, a job's wrapper) goes through `theseus_kernel::children::spawn`, or
  the daemon's reaper can take its exit status (`ECHILD`).
- A static or a detached thread holds the core or the store by `Weak`. One that held an `Arc<Core>` kept the store
  open past a clean stop, and every start then paid redb's repair.
- A test that needs a process outside every job skips that part when the test runs inside a job, and says so: an
  approval from inside a job is refused, correctly.
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
  input its reservation assumed, the output from the characters streamed), never held unknown, and a `provider.cut`
  row says it is an estimate. A stop that lands after the stream ended keeps the answer and runs none of its calls.
- `cargo test -p theseus-core` builds a second copy of every dependency. Use
  `cargo nextest run --workspace -E 'package(theseus-core)'`.
