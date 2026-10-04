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
- **The ontology** (row 26, step 21b; M4 §2.8; theseus-8kk.1): `ontology.rs` holds the snapshot (`Board`: built
  after serving by one `onto:` META prefix scan, `Core::warm_ontology`, or by its first reader; kept current on every
  write, which checks, writes its records and their rows in one frame, and swaps it), the given kinds (a bound
  place's `channel:`/`person:` category made at its first bind, `Core::bind_categories`; a session's given
  membership read from its place at compile, never stored), and the turn's `Walk`, fixed with the request's spec.
  `compiler::compile` renders the guidance after the context files: an append composes the memberships its
  manifest recorded, a recompile (or a ring) the current ones, so a membership change waits for the next recompile
  and a guidance edit in play is one `system_changed`. The manifest records `memberships` and `guidance`. A shared
  place composes only its own place's given memberships, and the place rule's admissions never read a membership.
  The methods are `rpc/ontology.rs` (`ontology.list`; the writes are `judge_act(Act::Ontology)`, the owner from a
  private place); the rows `fact/ontology.rs`. Tests: `tests_ontology.rs`.
- **Tool calls**: `toolrun.rs` (every call the model makes becomes a kernel action: the gate and the dispatch), with
  a job's call in `toolrun/job.rs` (its turn waits on the job's wake, `toolrun/waits.rs`, and takes the job's
  completion with its result in one frame; Tier 7.1), the continuation in `toolrun/resume.rs`, and the results
  no call's own run writes (late ones, and a cancel's) in `toolrun/late.rs` (theseus-5gw9). The gate's parts are
  `policy.rs` (postures and the floor), `external.rs` (the hold after external text), `broker.rs` (granted
  secrets), `approval.rs` (who answers, and from where), and `peer.rs` (the web UI's other-uid check at accept).
  Plus the harness's own tools,
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
  CloudFormation with state), and `aws/tests_durable.rs` (a fake S3 and DynamoDB with state, binary bodies, and
  checksums); `config/aws.rs` holds `[aws]`'s types and checks.
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
- **Grants in L1** (theseus-w5op; decided by Eddie 2026-10-03, superseding theseus-7y9y): an L1 job takes its
  program's broker grants at its launch, exactly as an L0 job does. `sandbox::decide` runs the L1 decision through
  `ToolRuntime::brokered` (decision 15: the stricter of the call's posture and the secret's, so any approval comes
  before the launch), and `sandbox::for_job` asks `Broker::for_job`. The value rides in the job's environment over
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
  handle has finished; anything else is unsupported. Each verdict lands on its action, in health's `cancels`, and
  as a fact (`fact/cancel.rs`). An aborted call's result waits for the cancel's verdict (`after_abort`). Its test is
  `tests_cancel.rs`; the daemon's are `tests/job_wrapper.rs`, `tests/sandbox.rs` (L1), and `tests/tasks.rs`.
- **Terminals** (theseus-n88g.4): `term/`. `term.open`, `term.send`, `term.read`, `term.close`: a program on a pty
  (libc's `posix_openpt`; its child through `children::spawn`, `setsid`, the pty its controlling terminal), read as
  a screen by `term/vt.rs`, a small VT model whose module doc says what it leaves out. Async tools whose run needs
  its session, so `toolrun`'s async path runs a `term.*` call through `Terms::run`. `term.open` plans its argv and
  `term.send` its terminal's, so the gate judges both as that program's run; `term.read` and `term.close` are reads.
  A listed external program's screen (or a terminal sent keys naming one: `Listed::in_text`) is outside text. At
  most 4 a session; closed at its execution's end (`turn.rs`), a cancel and a `/stop` (`rpc/driver.rs`), and the
  daemon's stop (`finish_stop`), with a close that hangs up, waits its grace, and kills what lingers, including a
  process that left the tree but holds the pty. In memory only: the rows `term.opened` and `term.closed`
  (`fact/term.rs`) are the record. No broker grant reaches a terminal (`brokered`). Tests: `term/tests.rs` (goldens
  and real `sh`, `python3`, `cat`), `tests_term.rs` (the gate, the hold, each close).
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
  `config/theseus.example.toml`, public, so it never carries a deployment's own values: a deployment's note holds
  only what differs from the defaults, and `config/sparse.rs` cuts a whole note to that (`theseusd config
  --sparse`, theseus-vwar).
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
  `extend/answer.rs`: an ack binds its confirm, a decline or an expiry declines it, neither wakes anything, and
  nothing loads until 43b). Facts in `fact/extend.rs`; `extend.list` and health's `extensions` in
  `extend/list.rs`; tests in `extend/tests.rs` and theseusd's `tests/extend.rs`.
- **The index tender's supervisor**: `tender.rs` (row 51): it starts `theseus-index` 2 s after serving
  (`START_AFTER`, so a start's aftermath stays quiet), restarts it with backoff, takes over the one an exec kept
  at once, and asks it for health and `index.query`, each call bounded (health asks only a tender that runs, and
  never past 100 ms). Its rows are facts (`fact/index.rs`). Its tests, `tests_tender.rs`, run on tokio's paused
  clock with a stand-in `Os`.
- **The judge** (M5, steps 23a and 23b): `judge/` (`JudgeService`, built at the first judgment; `sink.rs`, the
  batched frames; `spend.rs`, the shadow day budget; `loop_end.rs`, loop.v1's input; `mark.rs`, a dispatch's mark).
  A judgment a turn dispatches is decided (mode and sample, pure) and its id minted before the turn's last frame,
  and marked in the trace there (a zero-length `judge` span of kind `mark`: pack, point, mode, judgment); the call is
  spawned after the frame with that id (`theseus_judge::Ask::id`). Every point that dispatches inside a turn marks
  the same way. The facts (`fact/judge.rs`) say their sentences, and `Telemetry::record_judgment` counts each
  judgment, once the sink's frame is written; nothing of a judgment rides in a turn's frames but its mark.
  `judge.list` and `judge.get` are `rpc/judge.rs`. Tests: `tests_judge.rs`, `tests_judge_surfaces.rs`,
  `telemetry/tests_judge.rs`.
  - **At the gate** (step 24, `gate.rs`): `security.v1` and `security.v3` in shadow at every call that acts, and at
    a fetch or a search in a holding session (Q12). `ToolRuntime::start` plans the call and sends its notice, then
    `judge_at_gate` makes the choice and the marks, nothing else; the gate's decision never waits on Jev or changes
    with it. A gate judgment's id is its pack and the call's correlation id hashed (`gate::judgment_id`), so the
    turn's trace marks it (under the call's span) and a "should have asked" press labels it (`judge.label`, in the
    press's frame) before its row is written. A notified call's score follows its notice as `judge.scored`.
    Tests: `tests_security.rs`.
- **Recall** (M6 step 30a, in shadow): `recall.rs` (`Memory`: `[memory]`, the science, and who answers the index's
  query, the tender or a test's stand-in, `Memory::set_ask`; the manifest; `TurnRunner::place_of`, the place rule
  read as `class_of` reads it), `turn/recall_step.rs` (begun as the first loop's model call goes out, read once it
  answers, never past `[memory] recall_deadline_ms`: a stalled index costs a turn at most the deadline past its
  call), `fact/recall.rs` (the `recall.shadow` row, scoped `recall:<session>`, with references and the query's
  digest, never copies; the `recall` span; the narrative line), `rpc/memory.rs` (`memory.search`, which writes
  nothing, and `memory.recalls`), and `config/memory.rs`. In shadow the model's request is the one compiled without
  recall, and the row rides in the turn's next frame. The filters and pack are `theseus_memory::recall`. Tests:
  `tests_recall.rs`.
  - **In front of the model** (step 30b, theseus-6fn.2): `[memory] mode = "canary"` (a sticky share,
    `canary_fraction`, by a hash of session and `experiment`: `MemoryConfig::assign`, recorded once as a `memory.arm`
    row; the control runs `none` live with `baseline` in shadow) or `"live"`. The read finishes before the first
    loop's compile, under the same deadline (`recall_first`), and what it admits is a `Recall` node
    (`node::Body::Recall`: references, never copies) held in the turn (`Turn.recall`) and rendered by the compile as
    though written (`recall_view`, at the position after every node); the new compilation's prefix leaves it out
    (`recall_compiled`), so it renders in the tail as it will once written. It rides the provider call's plan frame
    with a `derived_from` edge to each source (`via = "recall"`, so `node.reach` counts the copy), and its
    `recall.ran` row the turn's next frame: no frame of its own. Its render (`recall/render.rs`) is §2.4's testimony,
    each item's frozen header and its source's text over the frozen byte range, read by position (cached: sources
    never change), so the next request begins with the previous one's bytes. Past `session_recall_cap_tokens` of
    notes in the tail, recall pauses (`paused`) until the next recompile. `memory.label` (`rpc/memory.rs`,
    `judge_act(Act::Label)`, refused by the CLI inside a job) writes a `memory.label` row scoped `memory`; `wrong`
    and `stale` drop a node as `labeled_wrong` from a set built after serving (`Core::warm_labels`, `recall/labels.rs`).
    Every compilation carries a `BudgetReport` (`Compiled.budget`, stored on a new `Compilation`; the ring's cut as a
    range, a recall's budget drops, an overage); the reply's `TurnSubmitResult.recalled` feeds Discord's
    `🧠 N recalled` footer. Tests: `tests_recall_node.rs`, `recall::render::tests`.
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
  (`theseusd example-config` prints it); `config/sparse.rs` cuts a note to what differs from the defaults.
- **L1**: `crates/theseus-core/src/sandbox.rs` (`[sandbox]`, the class, the last launch), and the wrapper's L1 path in
  `crates/theseus-kernel/src/job_l1.rs`.
- **The index tender**: the binary in `crates/theseus-index`, its supervisor in `crates/theseus-core/src/tender.rs`
  (started 2 s after serving, restarted with backoff, SIGTERM at a stop), and its child kind in
  `crates/theseus-kernel/src/children.rs`.

## Invariants

- **The frame budget.** A plain one-loop turn writes 5 frames, and each loop with one in-process tool adds 4.
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
- **Approvals come from private places** (the place rule, theseus-zmgb). An answer, the undo of a tightening, a
  trust, and a publish count only from the CLI, the web UI, a DM with the owner, or a channel bound `private = true`,
  and only from the owner (`[places] owner`, else the person of each DM the bindings file binds):
  `places::owner_in_private`. A shared place's cards go to the owner's DM. `[approval]` is retired: a config that
  has it loads with a warning, and nothing reads it. A test's bare label answers as the CLI (`From<&str> for
  Answerer` is test-only).
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
  template test holds it (`the_templates_harness_only_keys`). A program granted `aws_account` gets a short-lived
  job session at launch (`Broker::for_job_of`, `aws::Account::job_session`), never the key, and none before the
  owner role exists. Containment is that, the operator's `[broker]` grants,
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
- `Store::fail_turn_frame` (test-only) fails the first turn frame whose records match, once, as a full disk would.
- Debug builds assert, at every read, that a turn's kept transcript equals a fresh read of the store. A node written
  past the turn's handle panics a debug daemon.

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
  input its reservation assumed, the output from the characters streamed), never held unknown, and a `provider.cut`
  row says it is an estimate. A stop that lands after the stream ended keeps the answer and runs none of its calls.
- `cargo test -p theseus-core` builds a second copy of every dependency. Use
  `cargo nextest run --workspace -E 'package(theseus-core)'`.
