# The Ship of Theseus, chapter 7: Part II, the build plan ([index](README.md))
# Part II — Build Plan

_Tabitha/Claude, 2026-09-25. Part II says in what order the spec gets built, what each stage must prove before the next begins, and what is deliberately left out of the early stages. It follows the reviewer's five-phase shape (kernel → narrow agent → failure boundaries → intelligence as experiments → surface) and Appendix A's constraint: the execution kernel and simulator are the critical path, and everything else is a feature that must earn its place._

## P0. How to read the plan

Each milestone below says what it will **build**, what must be **proved** before the next begins, and what is deliberately **not yet** done. What actually happened is recorded in Part III, one section per milestone, so a plan section is never edited to match reality after the fact; the divergence is written down instead.

There are no duration estimates. Milestones are ordered by what each must prove before the next can begin, and two things will dominate the pace: how much of the kernel the simulator forces us to rewrite (it always forces some), and how much time the Discord and Anthropic integration steals from the kernel if started too early. The plan defends against the second by refusing to start them until M2 is green.

Every milestone has three parts: **build** (what exists at the end), **prove** (the test that gates the next milestone, always executable, never a judgment call), and **not yet** (what a reasonable person would want to add here and must not). Milestones are Beads epics under `openclaw-ph78`; each "prove" line becomes a closing criterion.

**Standing rules for every milestone.** Part III records where any of them slipped.
1. **Visibility** (M0.6). A new kind of work lands with its span, attributes, and metric on the same commit. A new capability lands as a native toollet unless a written reason says it cannot.
2. **Speed** (M3.5, §2 FAST). New startup work lands with its bench row. A new on-disk format lands with the reader for the format it replaces.
3. **Nothing declared without its reader** (the owner, 2026-09-27; theseus-wjy).
   - Every new route by which content reaches another context lands with its edge and a reverse-index entry. Examples are a summary admitted into another session, a task report into a channel, borrowing, gliding, and MCP.
   - Every new crate, protocol method, notification, edge kind, or label lands with at least one reader, on the same commit:
     - a crate is read when `theseusd`, `theseus`, or an installed tool binary reaches it by normal dependencies;
     - a method, by its dispatch arm;
     - a notification, by its `Event` and a sender;
     - an edge kind or a label, by a `match` arm, a pattern, or an `==` that names it. _(Since 2026-10-05, theseus-g7qp hole 4; Part III Item 182: a read of an edge kind, an `Event` or a `LedgerKind` counts only where the type it names is ours: a path that resolves to the type's home, or a bare name in a file that imports the home's type by name and no other type of that name, or in the home's own file; anything else counts for nothing, and a missing reader's message names the file and the fix. Imports are read per file, and `self::` and `super::` paths are not resolved, so both fail closed. Holes 1 and 3, a reader in code that never runs and a target cfg that is never true, stay open.)_
   - Anything declared ahead of its reader carries a marker, `row <n> (<step>), <milestone>: <its reader>`, and Part III lists it.
     - A crate's marker is `reserved_for` under `[package.metadata.theseus]` in its own manifest; a tool binary says `tool` there instead, being a reader of its own. _(Since 2026-10-05, theseus-g7qp hole 2; Part III Item 182: a `tool` is held to the install lists, `scripts/build.sh`'s `shipped` and `scripts/setup.sh`'s `SHIPPED`, read as text: the two must agree, and every root and every tool must be shipped; a tool no install ships says `run_from_tree = "<why, and who runs it>"` beside its `tool`, as theseus-exam does, and a stale claim (a shipped tool that still says it) or an orphan one (no `tool`) fails.)_
     - The protocol's, edge kinds', and labels' markers go in the registry test's `RESERVED` table.
     - A marker on an item that has its reader fails, so the list stays true.

   The gate enforces the second and third with a registry test, `tests_registry` in theseus-core, which `scripts/gate.sh` runs on its own before the suite (built 2026-10-01, Part III Item 32).
   - It enumerates every workspace crate, `method::ALL`, `notify::ALL` with `Event::VARIANTS`, `EdgeKind::VARIANTS`, and `Label::VARIANTS`. Each list is built from the same table as the items it lists, so a new item can't be left out of it.
   - It fails unless each item has its reader or a marker, and it names the item and the fix.
   - The first rule stays the reviewer's: no test can tell a new route.
   - Hook events left with the hooks (theseus-hco). If theseus-bdn's hook point lands, its events join the registry.

## P1. Milestones at a glance

| # | Name | One-line exit test |
|---|---|---|
| M0 | First light | The binary starts with config and secrets from 1Password, all hook events registered with no handlers, and one `turn.submit` over the protocol returns the model's reply from exactly one loop |
| M1 | Keel | `kill -9` at any point during a simulated workload; restart recovers every committed record byte-for-byte |
| M2 | Kernel | All kernel scenarios in spec §8 pass under randomized fault injection, including crash inside each of the five startup steps |
| M3 | First hands | The owner completes a real coding task in a known repo from Discord; the harness is killed mid-job; the job finishes and its result lands in the channel |
| M3.5 | Fast | The lifecycle bench meets every §9 lifecycle budget on the owner's store and on a synthetic 10,000-session store, the gate fails a commit that misses one, and a store in the previous format serves at once under the new binary |
| M3.6 | Daily Driver | The owner carries his daily Discord work on Theseus end to end, on one channel beside OpenClaw, and each item's prove passes |
| M4 | Boundaries | Every row of the durability table (§6) is demonstrated, the measured off-node recovery point is under 60 s, and L1's contract tests pass |
| M5 | Judgment | Jev-driven stopping and classification beat the deterministic baseline on a held-out trajectory set at equal total budget |
| M6 | Memory as experiment | An ablation report over the §5.5a metrics says which of FSRS, spreading activation, reranking, and synthesis stay |
| M7 | Surface | Theseus carries the owner's daily Discord work end to end; OpenClaw is no longer in the loop for that channel |

A usable agent exists at M3; the shape of the whole exists at M0. The back half is where the plan is least certain, and that is fine: by then the measurements exist to re-plan.

## P2. M0 — First light

The very first version, specified by the owner: a vertical slice through every layer, each at its thinnest, so the shape of the whole is real before any part is deep.

**Build.**
- Toolchain pinned: `rust-toolchain.toml` at stable (1.98.1 on 2026-09-25), target `x86_64-unknown-linux-musl`, static release profile, `cargo deny` with the permissive allowlist, `cargo nextest`, CI building the static binary on every push.
- Workspace crates: `theseus-protocol` (types only), `theseus-core` (kernel library), `theseus` (the binary: `serve`, `chat`, `--tender`).
- **Config and secrets (§3.19):** TOML config, `op://` references, resolution through the `op` CLI under the service-account token, zeroizing in-memory secrets, fail-closed startup. Starting set: Anthropic, Jev, GitHub, AWS.
- **Hooks (§3.17):** every hook event defined as a typed enum with its kind (Gate, Transform, Claim, Observe), a registry that accepts handlers over the protocol (`hooks.register`) and in code, the run-hooks path wired at each event site, and zero handlers installed. The turn runs through every hook site and nothing fires.
- **Turn runner (§3.3a):** session with a turn lock; a toolchain manager that compiles the context (the user prompt, nothing else), offers the tool list (empty), sends one provider request to the Anthropic Messages API with streaming, and returns the response.
- **Advancer:** the trait, with `stop_after_one_loop` as the only policy, ledgering its decision.
- **Protocol server (§3.18):** JSON-RPC over NDJSON on stdio and a Unix socket; `session.open`, `turn.submit`, streamed `model.delta`, `turn.ended`, `hooks.list`, `hooks.register`, `health`. `theseus chat` as the thin client.
- A first `Store`: the session record and a per-turn ledger row in an embedded store, so even the hello slice persists what it did. No WAL discipline yet; that is M1.

**Prove.** From a clean shell with only the service-account token in the environment: `theseus serve` starts, resolves every referenced secret from the vault, and refuses to start if one is missing. `theseus chat` sends a prompt; the core runs one loop against the Anthropic API and returns the reply; the ledger shows one turn, one loop, `stop_after_one_loop`, and every hook site visited with zero handlers. The same conversation works over stdio and over the socket. The binary is static.

**Not yet.** No tools. No Discord. No Jev. No store durability guarantees. No context beyond the prompt.

## P2b. M0.6 — Exquisite visibility (added 2026-09-25)

Not in the original plan. The owner's principle, adopted as work before Keel because every later milestone is judged through it.

**Build.**
- The turn trace (§3.3a): nested spans on every turn, on the result, in the ledger, in failure payloads; waterfall in the web UI; `ask --trace`. *(Done, theseus-8af.)*
- OpenTelemetry as a default projection (§3.20): spans from the trace with exact timestamps, GenAI conventions on provider calls, metrics, OTLP/HTTP with vault-sourced headers, no-op until an endpoint is configured. *(theseus-vng.)*
- A standing rule for every later milestone: a new kind of work (tool call, judgment, completion, compaction, memory pass) lands with its span kind, its attributes, and its metric on the same commit; and a new capability lands as a native toollet unless there is a written reason it cannot (§3.23). And nothing is declared without its reader (P0's rule 3): a new crate, method, notification, edge kind, or label lands with what reads it, or with a marker naming the row and milestone that bring its reader. The gate's registry test (`tests_registry`, theseus-wjy) fails anything else. Part III records where either slipped.
  _(Amended 2026-10-02, theseus-j6qn, Review 2's C2, Part III Item 43.)_ The rule's form: a new kind of work
  lands as facts in `crates/theseus-core/src/fact/`, each one type that says its ledger row (a `LedgerKind` and
  its data), its notification, its narrative sentences, and its span or attributes, recorded once where it
  happens; its metric comes from the trace at the turn's end. A row kind is a new line in
  `theseus-protocol/src/ledger.rs`. M4's labels and M5's judgment rows are facts like any other: no new
  channel, and no hand-written row, notification, or line.

**Prove.** With a collector listening, one turn produces one trace whose spans match the ledger's `turn.trace` row exactly in count, names, nesting, and durations; the metrics for that turn arrive; with no endpoint configured, nothing is sent and the turn is no slower. Tested against an in-memory exporter; verified live against a receiver.

**Not yet.** Logs as OTel log records (the ledger is the log; it can be exported later). Prometheus scrape endpoint (optional, small).

## P3. M1 — Keel

**Build.**
- CI extended to aarch64; the `--tender <role>` entry point that does nothing yet. (Toolchain, `cargo deny`, and the x86_64 static build arrived in M0.)
- The event record types: `Node`, `Edge`, `Execution`, `Session`, `Compilation`, `Action`, `Completion`, `JudgmentRecord`, `LedgerRow`, with schema version stamps and forward-only migration hooks.
- The `Store` trait: append, read-by-id, range-scan-by-position, checkpoint, and a transactional `settle(completion, continuation)` primitive.
- Two `Store` implementations behind a feature flag: `redb` and `fjall`. A benchmark harness with our shape of workload: append-heavy small records with group commit, recent-window scans, id lookups, edge-segment reads, concurrent readers during writes.
- The WAL and spool layout on disk, with checksummed, length-prefixed records and torn-tail truncation.
- The simulator skeleton: virtual clock, deterministic scheduler, scripted fault injection (kill at record boundary, kill mid-fsync, disk full, torn write), and a replay checker that compares the recovered state against the oracle.

**Prove.** Under a randomized simulated workload, `kill -9` at any point followed by restart recovers every committed record exactly and no uncommitted record; the benchmark picks the store, and the number that picked it is recorded in the spec's §1.

**Not yet.** No Discord, no Anthropic, no tokio actors per channel, no arena optimization. The arena at this stage is a `HashMap`.

## P4. M2 — Kernel

**Build.**
- Executions as durable objects with the state machine from §3.15 and an authority context (principal, grant, delegation limits, channel ceiling) that derived work inherits.
- Actions with harness-minted correlation ids, `ActionPlanned → Dispatched → Settled | OutcomeUnknown`, per-adapter **retry class**, and resolvable `OutcomeUnknown`.
- The `Completion` envelope and two transports: in-process channel and Unix-socket spool. SQS and loopback HTTP are stubs with the same interface.
- The job wrapper: detached systemd scope (or plain double-fork on the desktop), spooled result, own deadline, cancellable by id, with the cancellation lifecycle `requested → acknowledged → verified | unsupported`.
- The harness loop parked on `select()`, the one-minute heartbeat reconciler, the five-step startup order, at-least-once delivery with idempotent settlement.
- The turn lock per channel with explicit release points at offload boundaries, and a first durability job that runs in released time (checkpointing), so the model is exercised before it matters.
- Sessions as compiler scopes (§3.2a): one execution per session, per-execution turn locks, the admission scheduler, promotion by fork with inherited authority and carved budget, `reports_to` delivery as ordered channel actions.
- Budgets as **hard limits** with reservations, held reservations on unknown usage, and the reserved control-and-cleanup budget. No estimation yet.
- Deterministic policy gate with the ordering from §3.17 (transform → validate → policy → confirm bound to the final action → revalidate → dispatch), without hooks yet; the ordering is what is being tested.
- Ledger rows for every state transition.
- _Added 2026-09-26, before M2 began (v0.24):_ Session and Execution records in the store with the per-session position table (§4.4b); the `derived_from` edge written on promotion; the deterministic kernel simulator with virtual clock and fault injection, moved here from M1 (Part III A1); group commit, moved here from M1.

**Prove.** All kernel scenarios in §8 pass under randomized fault injection: lost completion, duplicate completion, completion during restart, cancel of a detached job, unknown then success, late completion after cancel, crash after settlement before continuation delivery, crash inside each of the five startup steps, two executions on the same task, wrapper deadline with harness down, a promoted task running concurrently with its conversation with messages routed to each, admission ceiling hit while `/cancel` is honored, graceful upgrade with a hundred sessions mid-turn. Reproducible from a seed.

**Not yet.** No model. The "tool" in M2 is a fake that sleeps and sometimes fails; the "channel" is a simulated mailbox.

## P5. M3 — First hands

The narrow agent. One channel binding, one shell class, no intelligence beyond the model.

**Build.**
- Discord via `twilight`: one application, one guild, DM and one text channel, message send and edit, one component (the confirm button). Bindings as a file.
- Direct Anthropic Messages API with streaming, tool use, prompt-cache layout from §4.5, complete-block-only dispatch, and usage accounting into the ledger. Interrupted-call reservations held as unknown.
- A **model catalog** (`[catalog."<model id>"]`): per model, the serving provider, context window, maximum output tokens, prices per million tokens for input, output, cache read, and cache write, and capabilities (tools, vision, reasoning). A built-in catalog ships in the binary for the Anthropic family (`claude-opus-5-5`, `claude-opus-5`, `claude-sonnet-5`, `claude-fable-5-1`, `claude-haiku-4-5`) and the GLM 5.x models; config entries override or add. Three consumers: a profile that omits `max_output_tokens` defaults to the model's real ceiling; the budgeter uses the context window to decide what fits and when a recompile is forced; the ledger and telemetry turn tokens into dollars. An unknown model id still runs, with cost marked unknown and a startup warning. Prices and limits cannot self-update (the provider's models endpoint lists ids, not limits or prices), so the catalog is a versioned table and every priced ledger row names the catalog version that priced it. Decided with the owner 2026-09-26; it is config the moment code reads it, and not before (§3.19 rule). Until then `max_output_tokens` on a profile is the only token limit in config, and it is an output cap, never an input one.
- The context compiler in its simplest form: one compilation per session then transcript append; recompile only on the deterministic triggers of §4.4a (no Jev yet); a manifest that records the compilation, the tail range, the as-of position, and the request digest.
- **Toollets, native first (§3.23), from the selected set (§3.24):** the `fs` family (read, write, edit, patch, glob, grep, list), `proc.run`, `text.diff`, and `git.diff`/`git.log` on the operator's real checkouts, with `proc.run` as the typed, shell-free escape hatch through the L0 job wrapper; `proc.session.*`, `git.commit`, `text.query`, `http.fetch`, `web.search`, `channel.*`, `memory.*`, `node.read`, `wake.at`, and `extend.*` follow in their milestones. No `bash` tool: a shell is `proc.run { argv: ["bash", "-c", …] }`, visible as such in the ledger. Fast in-process toollets stay synchronous; anything doing I/O past the bound or crossing the process boundary is an action with a completion. `theseus.tool.calls` and the shell-fallback ratio from the first turn.
- The model loop with deterministic control only: `/stop`, `/cancel`, budget exhaustion, confirm.
- The in-binary web UI in its first form: list executions, actions, and ledger rows; tail a channel. Read-only.
- `theseus restore` from a local WAL directory (S3 comes in M4), because the restore path exists from the first release.

**Prove.** The owner completes a real coding task in a known repository from Discord. During a long shell job the harness is killed and restarted; the job finishes, its completion is settled from the spool, the execution continues, and the result lands in the channel. The web UI shows the whole history. A request the owner is not permitted to make is blocked at the gate with a clear message.

**Not yet.** No Jev, so promotion to an autonomous task is by explicit human command (`/task`) only. No roles. No memory beyond transcript. No MCP. No voice. No compaction (long conversations simply get a fresh transcript root by hand). This is the discipline Appendix A demanded and the first place we will be tempted to break it.

## P5b. M3.5 — Fast (added 2026-09-27)

Not in the original plan. This is the owner's principle (§2 FAST), adopted after an OpenClaw restart took most of ten minutes. It comes before Boundaries **[D]** because every later milestone adds startup work, and the gate should refuse regressions before they pile up. Beads: theseus-qa0.

**Build.**
- **A lifecycle bench,** `theseus-sim bench lifecycle`, over five phases:
  - cold start to the first `health` answer;
  - clean shutdown with executions waiting and a job running;
  - SIGKILL, then restart;
  - a binary swap under load;
  - restore.

  Each phase runs N times, with p50 and p95 reported per phase and per startup step. `scripts/gate.sh` compares the result with §9 and fails on a miss, allowing a noise margin that is itself measured. _(Built: cold, shutdown, and kill in F1, 2026-09-29; swap and restore in F4b, 2026-09-30. Restore is measured, with no budget yet.)_
- **Serve first.**
  - Secrets resolve in the background. Today six concurrent `op read` processes take 1.0 s between them (A3, lifecycle timings); one `op inject` for every reference is the first candidate, and measurement picks the winner.
  - The provider, the Discord binding, and the GitHub client each wait for their own secret, and health reports `secrets: resolving | ready | failed <name>`.
  - The GitHub token check (0.2 s) runs after serving.
  - A failure is loud (health, the ledger, the Observatory) but never delays the socket.
- **Startup phases in the Observatory,** from the kernel's existing step timings plus the new background phases, so a slow start names its cause the first time it happens (EXQUISITE VISIBILITY).
- **Versioned readers for WAL record layouts and manifest formats.** The next format change migrates in a tender, and the M2 practice of moving an old-format store aside (A2) is retired. _(Built 2026-09-30, F4a: per-kind schemas and a format-3 manifest. The move-aside had already gone in ea06ff8, batch C. Part III A3c.)_
- **A standing rule for every later milestone:** new startup work lands with its bench row. A new on-disk layout lands on the same commit as the reader for the layout it replaces. It bumps its record kind's schema number (`kinds::SCHEMAS`) and adds a test that reads the old layout; a new record kind is added to the table. A build never writes over a store newer than itself: it refuses to open it and says to install the newer build. _(Adopted in F4a, theseus-qa0. Six kinds went to schema 2 for the fields they had gained since M2, among them the session's hold on external text, and an execution's wakes, report wakes, and stop, all of which an older binary would have dropped.)_ _(Since S2, Review 2's R8, Part III Item 52: theseus-core's `tests_schemas` holds the rule. Each kind's shape is recorded under its schema number, and a changed shape fails until the number moves. It caught 17b's change to a tool-call node at the join, which became NODE schema 4; Item 58.)_ _(At 18a, ACTION 3 and OUTBOX 2: an action's cancel verdict, and a post, which is an action of its own kind (Item 60). At 19a, NODE 5 and COMPILATION 4: a node's label, and the manifest's audience, readers, integrity in play, and withheld nodes (Item 61). Each reads its old layout in place, with a test. Such a test builds the old bytes from a literal, never through the build's own serializer, or a change to how a record serializes moves both sides and the test still passes: 19a's NODE 4 test did until it was rewritten, and theseus-djfj tracks the rest.)_ _(At 19c, NODE 6: a graduated node's warrant; the held post needed no layout (Item 63). At 18d, ACTION 4 and OUTBOX 3: an action's `parent`, a credential request's job, read from 3 and 2 as none (Item 65). The grants step wrote no schema: `Action.parent` stays, written by nothing since, so 18d's stored requests read whole, and its two ledger kinds read as unknown kinds (Item 71). theseus-djfj is done: every old-layout read test takes its old bytes as a literal, its comment naming the build that wrote them (Items 63 and 67).)_ _(Superseded 2026-10-03 by Tier 7.9 as amended (the owner, 14:20: any new stored field bumps it; Part III Item 82): one store format number, `MANIFEST_FORMAT`, with a literal sample of the layout it replaces in theseus-core's `tests_layouts`; `kinds::SCHEMAS`, the manifest's marks and R8's `tests_schemas` are gone. It was 4 at that step, 5 for a wake's repeat (Item 84), and 6 for the WAL's synced mark (Item 92); the cloud's ontology wire-in takes 7 at its join. The index's tables are a rebuildable projection, versioned by their own mark, never by the format (Item 91).)_
- _Added 2026-09-29._ **Parallel tool calls** (theseus-a60). The owner, 2026-09-28, 23:57: "we're writing all our core tools in Rust right? I mean I know that sometimes using shells and other expected binaries like git is good practice, but I do want as many things native and performant as possible. Also, I imagine that you often can run things in parallel. My thought is that the most performant way to run trivially parallelizable tasks is to avoid os threads and use truly async code." Today a response's calls run one after another, and in-process toollets run on tokio's blocking pool, whose default ceiling is 512 threads.
  - Gate every call in the response first. A call under `approve` parks, as today.
  - Run the `open` and `notify` calls of one response concurrently, as async tasks, and answer them in the model's order.
  - Calls on the same path keep their order.
  - CPU work goes to a fixed pool the size of the cores, never a thread per task.
  - `fs.grep` and `fs.glob` use the parallel walker on big trees.
- _Added 2026-09-29._ **Frame batching and one transcript read per turn** (the complexity review's findings 3 and 8; theseus-hco, Part III A3b).
  - A ledger row that is not a state transition rides in the next frame the kernel or the runner already commits, and an action that needs no confirm is planned, authorized, and dispatched in one frame. The review put a plain turn at about 8 frames.
  - A turn reads its session's nodes once, at its start, and keeps the nodes it writes. Today every turn decodes the whole transcript three or more times, plus once per loop.

**Prove.** The lifecycle bench meets every §9 lifecycle budget at p95, both on the owner's store and on a synthetic store of 10,000 parked sessions. With 1Password unreachable, the socket still answers inside its budget and health names the missing secrets. A store written in the previous record layout serves immediately under the new binary _(proved in F4a on a store 460a35b wrote, checked in as a fixture, and on a copy of the owner's store)_, and a tender rewrites it while turns run, with no request failing _(future work: no layout has needed a rewrite)_. The gate refuses a branch that adds a 100 ms sleep to cold start. _Added 2026-09-29:_ a response of five reads and two greps takes the slowest call, not the sum; a plain turn writes fewer frames than 17, and the frame-budget test holds the new count _(8 after F2, 5 after F4b)_.

**Not yet.** Turn-path latency beyond §9's 5 ms. The arena's memory layout (M6–M7). Anything that needs more than one node.

## P5c. The build order after M3 (added 2026-09-27)

These are the owner's decisions from the openrig and herdr walkthrough (Appendix F). The owner approved the order on 2026-09-27; the umbrella issue is theseus-5r9. The items run strictly in sequence on `main`, each through the gate. Each is recorded in Part III as it lands.

_Note, 2026-09-28 (theseus-8az). Item 1 began as steps 2a through 2a.3: consequence detection in the gate, then three rounds of hardening. They were built, then superseded by theseus-8az, and the gate is now notify over block (§3.9; Part III A3b, the reversal). Step 2b, trusted-channel approval (theseus-sgh), still stands. Step 2c's floor ceremony (theseus-qc4) is moot, since the floor now asks instead of refusing; with no refusal left in the gate, the rest of 2c waits on the owner (§3.9, Owner override). The list below is the plan as approved._

_Note, 2026-09-29 (theseus-5r9). The order now runs as the queue in the chain's working file, `theseus-5r9.md`, shows. The owner asked for "a long stream of work" at 23:32 on 2026-09-28 (P5d), and the usage audit (theseus-p3k, reviewed 00:27 on 2026-09-29) set its order. The queue is: the complexity cuts (theseus-hco) and Narration (theseus-5fy), both done; M3.6 items 1 to 4, of which dollar budgets and context files are done; complexity batch C (theseus-0g4); item 1's step 2b (theseus-sgh); item 5, M3.5 Fast, with parallel tool calls (theseus-qa0, theseus-a60); M3.6 items 5 to 8; then the owner's end-to-end testing. After it come item 2's reader-rule test, items 3, 4, 6, and 7, and M4, re-planned. Item 2's hook half went with the hooks: theseus-0dp is moot, and only the reader-rule gate test (theseus-wjy) remains. The list below is still the plan as approved._

_Note, 2026-10-02. The order has since run as the re-cut roadmap's, `docs/design/roadmap-v2.md`: one spine of steps on `main`, with lanes beside it, each step reviewed before the next. `docs/status.md` says where it stands. Items 3, 4, 6, and 7 below are built (Part III Items 33, 42, 38, and 39 with 41), and with them stage B. The week after v1 is planned in `docs/design/roadmap-v1.1.md` (theseus-empf, Part III Item 45). The list below is still the plan as approved._

1. **Irreversible consequences, trusted approval, owner override** (theseus-770, theseus-sgh, theseus-qc4; §3.9). *Prove:*
   - `proc.run git push --force` in a workspace repository waits for approval under `notify` and under `open`.
   - An answer from an untrusted channel or user does not count.
   - An owner override of an against-policy call on declared property runs and is ledgered.
   - A floor override requires the typed ceremony, and a call on undeclared property cannot be overridden.
   - Every detection rule passes its examples.
   - An existing config with no `[approval]` or `[owner]` section still loads.
2. **Hooks and the reader rule** (theseus-0dp, theseus-wjy; §3.17, P0). *Prove:*
   - A blocking in-process handler on each gating event stops its action in a scenario.
   - The registry test fails a branch that declares an event without a site.
   _(Built 2026-10-01 without hooks, which theseus-hco cut. The registry test enumerates crates, methods, notifications, edge kinds, and labels: Part III Item 32.)_
3. **Protocol push** (theseus-in3): `execution.changed`, a watch over all sessions, `session.wait`, and one `attention()` mapping. *Prove:*
   - A client learns every execution transition without polling.
   - `session.wait` returns on `blocked`, `settled`, and `terminal`.
   - A subscriber that falls behind re-snapshots.
   _(Built 2026-10-01: Part III Item 33.)_
4. **The herdr adapter** (theseus-l1l). *Prove:*
   - In a herdr pane, a session waiting on a confirm shows `blocked` within a second.
   - Answering in the pane resumes the session, when the CLI is a trusted channel.
   - `theseus herdr sync` is idempotent.
   _(Built 2026-10-01, steps 11a to 11c: Part III Item 42. The pane read `blocked` 2 ms after the question's
   row, and an answer in the pane counts through the daemon's socket, as the CLI's does.)_
5. **M3.5 Fast** (P5b, theseus-qa0).
6. **Epidemiology, step 1** (theseus-n4m): reverse compilation membership, a reverse `derived_from` column, `node.reach`, and reach shown in the Observatory. *Prove:* `node.reach` returns every compilation and session that included a node, in a core scenario. _(Built 2026-10-01, step 12a: Part III Item 38. The reverse compilation membership and the reverse `derived_from` column are derived, not stored: the column is the scope `in:<node>` on the store's scope index, and `node.reach` reads compilations from their own records (§6.1).)_
7. **`theseus tui`** (theseus-7yx). *Prove:* from the TUI alone, the operator can see every session's state, jump to the next session that needs attention, approve or decline a confirm (as a trusted channel), and submit input. _(Built 2026-10-01 as `theseus-tui`, steps 10b to 10e in the `tui` lane, which `theseus tui` execs since step 10f: Part III Items 39 and 41. It answers as the CLI's channel does, so `[approval] channels` decides whether its answers count.)_

## P5d. M3.6 — Daily Driver (added 2026-09-29)

Not in the original plan. The owner, 2026-09-28, 23:32: "I do want to wait until I have an end to end version before extensively testing. Please prepare a long stream of work". The back half of the plan (M4 to M7) was ordered to prove the kernel before features, and the kernel is proven: M0 to M2 are closed, and M3 is built. What keeps Theseus from carrying a day is a short list spread across M5 to M7. This milestone pulls the minimum of it forward, as simply as possible. Its exit test is M7's, started early. Beads: theseus-5jl.

**Why these items.** The usage audit (theseus-p3k; reviewed 2026-09-29, 00:27) read 30 days of the OpenClaw work the owner does now.
- 13,719 tool calls: 48 % in his Discord DM, and 43 % in delegated sessions.
- The shell made 69 % of the calls, and the file tools 22 %.
- The DM's median turn took 90 s, and its p95 20 min.
- The DM used about 52 M budget units a day, and Theseus's template then gave a session 20 M, so a session would have ended within hours.
- No MCP server is configured anywhere the main agent runs.

Each item answers one need in those numbers, and the order follows the audit.

**Order.** Items 1 to 4 run early, before M3.5, because the owner already uses Theseus from Discord and they fix first-day pain. Items 5 to 8 follow M3.5. Item 9 is optional.

**Build.**
1. **Budgets in dollars** (theseus-0sg): the limit, the question, and the reset of §3.13. The owner's design (2026-09-29, 00:09) replaced the audit's renewing daily allowance. *Prove:* a session at its limit waits and asks, an approved reset sets its spend to $0 and the waiting call goes ahead with the lifetime cost unchanged, a decline keeps it waiting, and executions stored with unit budgets still serve.
2. **The workspace: context files and roots** (theseus-58a): `context_files` on a profile, compiled into the system block with their digests (§4.4), and `roots` shown in the template. *Prove:* a DM turn follows a rule that only a named file states, without a tool call; an edit to the file causes exactly one `system_changed` recompile; and with `~/.openclaw/workspace` and `~/reports` in `roots`, a spec session's reads and edits run without a confirm.
3. **Quiet notices** (theseus-w4f): under `notify`, the notice rides on the loop's tool message, and the separate embed per call becomes a `[discord]` setting that defaults off. *Prove:* a turn with 30 notified `proc.run` calls posts one tool message, edited in place, and no other message, while the ledger has 30 `tool.notified` rows and the web UI shows each notice.
4. **Attachments and images** (theseus-9g2): the binding reads each attachment up to `[tools].max_read_bytes`, text as user content marked external and an image as an image block for a model with vision, and `fs.read` returns images the same way. *Prove:* a 5,000-character paste (Discord sends it as `message.txt`) is answered from its content, a PNG screenshot is described, and a 20 MB archive is listed as not read, with the reason.
5. **`http.fetch` and `web.search`** (theseus-yd6): native toollets that inherit the posture; a loopback or private address waits for approval. The search backend is the owner's choice. *Prove:* a fetch of a docs page quotes a fact from it, a search returns ranked results with URLs, and a fetch of `127.0.0.1:7433` waits for approval.
6. **Durable delivery** (theseus-q4v): every post and edit the binding makes is an outbox action whose completion records the Discord message id, replayed in order and once on reconnect; the driver's startup wait for bindings goes. *Prove:* with Discord unreachable during a long turn, the reply posts exactly once when Discord returns, a `kill -9` between send and settle does not duplicate it, and the M3.5 bench shows no driver wait at startup.
7. **Task sessions** (theseus-qn2): `task.create { brief }` opens a child session that inherits the place's authority, takes a carved budget, and reports to the place through the outbox. *Prove:* from the DM, "run the theseus gate and report" returns at once, a second question is answered while the task runs, and after a `kill -9` and restart the task finishes, its report posts once, the parent's next turn quotes it, and the child stays within its budget.
8. **`wake.at`** (theseus-cff): `wake.at { at | after, note }` sets the kernel's existing `Wake::DueAt` on the current session; one-shot only. *Prove:* "check the build in 10 minutes" gives a turn in the DM ten minutes later, across a daemon restart, and a wake that fell due while the daemon was down runs after startup, marked late.
9. **Optional: secrets in a command's environment** (theseus-dcy): `[tools].proc_secrets` maps an environment variable to a `[secrets]` name, injected at spawn and scrubbed from output. Taken only if the `op` waits prove tiresome in testing. *Prove:* `gh api user` runs under `notify` with `GH_TOKEN` from the vault, with no `op` call and no approval, and the token appears in no node.

**Prove.** The owner carries his daily Discord work on Theseus end to end, on one channel beside OpenClaw, and each item's prove passes. The exit test's config needs no code: the roots, the context files, `allow_argv` for the frequent read-only programs, the DM's profile, and the limit from item 1. Theseus has its own bot, so its DM runs beside Tabitha/Claude's, and OpenClaw leaves the loop only when the owner says so.

**Not yet.** Two items stay in M7, per the audit's "Challenged" list:
- The MCP client. No MCP server is configured anywhere the main agent runs, and vestige comes in through an HTTP plugin.
- Recurring schedules. Every recurring automation posts to Slack, reads Google Workspace, or maintains OpenClaw's or vestige's stores, and the DM's own scheduling was mostly one-shot (6 of 8).

Slack, the automations, and the heartbeat stay in M7 too, and on OpenClaw until then. Voice, AWS shells, multi-guild bindings, Jev packs, memory science, and self-extension stay in their milestones.

## P6. M4 — Boundaries

Make the durability and safety claims true, and measure them.

**Build.**
- The durability tender: WAL segments to S3, index rows to DynamoDB, scheduled in released turn-lock time by staleness; the "oldest unshipped committed record" metric and alarm. _(Built 2026-10-04, step 15; Part III Item 108: woken by each WAL change with a 5 s settle and a 60 s backstop, not scheduled in released turn-lock time; the metric is health's line and a lag histogram, and there is no alarm yet: the OTLP encoder has no gauge kind.)_
- `theseus restore --from s3://…` with reconciliation near the gap and redaction tombstones applied before restored content becomes visible. _(The S3 restore was built 2026-10-04 as `theseusd restore --from s3://…`, step 16, Part III Item 123: the rows decide, every object is checked against its row, and a gap restores the prefix before it and says so; the reconciliation and the tombstones are not part of it.)_ Redaction with receipts (`erased_local`, `pending_backup`, `external_copies`).
- L1 native sandbox with the §7 contract, and **contract tests** that prove each denial: no route to the metadata service, no route to localhost services including Theseus's own UI, capabilities empty, seccomp active, process tree killed on cancel. _(Begun 2026-10-02: `proc.run` runs in L1 since step 17b, Part III Item 58, whose daemon test shows the contract in a real L1 job against L0's contrast. Egress was built in 18c, Item 62, and credentials are grants at launch, Item 71, after 18d's run-time socket, Item 65; cancellation per backend was built in 18a, Item 60.)_ _(Trimmed 2026-10-03, the cut-list's Tier 4, Item 77: no self-test at the start, no delegated cgroup, and a root daemon's L1 job refused.)_
- Confidentiality labels on nodes with inheritance through generated nodes; audience-safe compilation; disclosure tests in the simulator (private material never reaches a public audience's context). _(Labels on nodes, the audience, and audience-safe compilation, with placeholders that keep the request valid: built 2026-10-02 in 19a, Part III Item 61. Graduation and the held post were built in 19c, Item 63, and the disclosure simulator in 19b, Item 66; 19d closed its two findings, Item 68, and its live check passes 40 seeds of 2,000 steps with `--strict`.)_ _Replaced 2026-10-03 by the place rule (Part III Item 76). Every place is private or shared; a shared place's request carries its own conversation, the public tools, the public trees and the public context files; and the owner publishes into it explicitly. Labels on nodes, the compile filter, held posts, quiet loops, graduation, and the disclosure simulator were removed._ _Since 2026-10-03 the operator may declare a whole guild private, `private = true` beside its `guild_id` (the owner, 17:14; Part III Item 89)._
- Control-plane separation as an installer option: dedicated `theseus` user owning store, WAL, spool, and policy; L0 jobs as the operator.
- Cancellation verification per backend (systemd scope, L1 process tree), and `cancel_unsupported` reporting. _(Built 2026-10-02 in 18a, Part III Item 60: an L0 job by its wrapper's process tree, an L1 job by its pid namespace ~~or its cgroup~~ (its cgroup went in Item 77), an async tool by its task, and `unsupported` for an in-process toollet. No systemd scope: an L0 job has no cgroup of its own yet, theseus-yfdj.)_ _(Since 2026-10-04, card 5, theseus-a5nv; Part III Item 152: an L0 job has a threaded cgroup of its own where the daemon's is delegated, with a process cap, and is stopped and verified by it (`verified_by: cgroup`); elsewhere by its wrapper's tree, as before. Still no systemd scope per job: one costs about 48 ms to start.)_
- The ontology (§4.1a, theseus-8kk): the kinds table, declared memberships, guidance, and the compile walk, with topics as the first new kind. _(Built 2026-10-04: 21b wired it in, at store format 7, Part III Item 100; 21c gave it the cockpit's Ontology view, Item 111.)_
- ~~Integrity labels by transmission, and the one-step-stricter rule for exposed contexts (§3.9 Exposure). Also the `external` origin, file hashes, and the `Advisory` with its correction control (theseus-3vu).~~ **Dropped** (the owner, 2026-10-03 11:24, the cut-list's Tier 1.1; Part III Item 74): T1's latch stays per session, and trust clears it; `[policy] external_programs` (`gh`) and a job's session feed it; text laundered through files is Jev's `security.v1` (row 39).
- The consequence boundary under L1 (§7): credential brokering and egress recognition. _(Credential brokering under L1 is a grant at launch, Part III Item 71, after 18d's run-time requests, Item 65, were built and removed. Egress recognition stays filed: it would need TLS interception, dropped for v1; the list of hosts (18c, Item 62) is M4's control.)_
- ~~**Credentials as stand-ins** (theseus-gh7; the owner, 2026-10-01 15:13: build it before the AWS hands' first
  account write). A job granted a secret sees a stand-in, never the value. The L1 egress proxy terminates TLS for
  the hosts the secret is granted to, through a per-job CA the job trusts, and swaps the value in only on those
  connections, re-signing SigV4 for AWS. A stand-in sent anywhere else is a refused connection and a ledger row. It
  lands with egress and credential brokering (18c, 18d), so it is in place before C2, the first account write. It
  shares the TLS termination that egress recognition needs.~~ **Dropped for v1** (the owner, 2026-10-03 00:00, under
  the default-trust principle, §2; Part III Item 69): heavyweight for a default-trusted harness, and its shape is
  a credential-intercepting proxy's. **Credentials as built** instead (Items 69 and 71):
  - the AWS keys and the providers' keys are the harness's alone, read only by Theseus's own tools, and
    `theseusd check` and health name them;
  - a job, at L0 or in L1, gets a secret only by the operator's `[broker]` grant to the program it runs, at its
    launch, at the stricter of the call's posture and the secret's (decision 15), and the wrapper withholds the
    value from the job's output;
  - the egress list (18c, Item 62) bounds where a held value can go.

  C2, the first account write, no longer waits on credentials; it waits on the owner's go-ahead.

**Prove.** Every row of the durability table is demonstrated by a test: process crash, node restart with disk intact, SSD loss with restore from S3, external effect without evidence. The measured off-node recovery point under a synthetic load is under 60 s at p99 and the turn-latency cost of the durability work is reported. L1 contract tests pass. Disclosure tests pass. _(They did until 2026-10-03, when the place rule replaced labels and the disclosure simulator went with them. The proof is now the place rule's tests, each with a planted revert that fails it, and a live check on a copy of the owner's store, Part III Item 76.)_

**Not yet.** No AWS shell classes. No hook handlers beyond tests; the hook points themselves are wired before M4 (P5c). No Jev.

## P7. M5 — Judgment

Jev enters, in shadow first, and hooks arrive because Jev packs are the first real hook handlers. _(Since 2026-10-04, 23a; Part III Item 105: Jev entered in shadow, wired into the core as `JudgeService`. The hooks did not arrive: they were deleted on 2026-09-28 (§3.17), and packs are core calls at fixed decision points. The M5 design's other divergences from this section (`docs/design/m5-judgment.md` §1) hold: a live pack fails to its baseline rather than `waiting` on Jev recovery; the prove is measured in the soak; CLASSIFY and CONTINUE stay in shadow through M5.)_

**Build.**
- Jev client with the confidence gate, state capping, batching by the budgeter, latency measurement per workload class, and every call accounted as spend.
- Question packs: `CLASSIFY` (new ask vs nudge vs control vs addressed-to-task, and when a conversation should promote to an autonomous task), `JUDGE_STOP`, `ROLE_GUESS`, `CONTINUE` (append or recompile, and how). All in **shadow**: recorded, compared with the deterministic baseline, never acting. _(Wired in shadow on 2026-10-04: `CLASSIFY` and `ROLE_GUESS` at inbound, in one request (25a), and `CONTINUE` at compile, behind the candidate signals (25b); Part III Items 121 and 122. `JUDGE_STOP` came first, with 23a (Item 105).)_ _(Since 2026-10-04 three packs act, each by the owner's decision: `route.v1`, `rerank.v1` and `security.v3`'s notices, and every pack's mode is the ladder's (26a). Replay, audit and backfill (25d) are built. Part III Items 139, 141, 142, 144 and 145.)_
- The learning ledger with correct labels (`budget_exhausted` its own class), holdout split, and canary promotion of a pack from shadow to live when it beats the baseline. _(The ledger was built 2026-10-04, step 25c, Part III Item 129: operator and system labels, the nightly report per pack version with its 14-day holdout frozen in, and its tender; a turn the budget ended takes no label, and no rule writes "should have stopped".)_ _(The candidate itself now comes from the owner's labels: the learning loop, step 25f, since 2026-10-05, Part III Item 164.)_
- Hooks: `Gate`, `Transform`, `Claim`, `Observe` kinds with fail-closed gates, typed observer results, ordering before final authorization, ledger rows per invocation. Compiled-in handlers first; remote handlers over the protocol may observe.
- Roles table with the twelve seed rows, announced role changes, roles as hints in the compiler. _(The twelve seed rows are compiled in as data, and `role.v1` guesses among them in shadow, since 25a, 2026-10-04 (Part III Item 121); the versioned table, its announcements and its hints are 26c's.)_
- Executions gain `waiting` on Jev recovery and provider outage (fail closed, say so).
- Task sessions and promotion, with the required arrangement (§3.2a) and pieces admitted by reference.
- Independence as a compiler property, obligation invariants, honest delivery receipts, and `relies_on` in shadow (theseus-vug). _(Built 2026-10-04: independence as check tasks (28a, Part III Item 132), and the parked-task invariant as health's `tasks.parked` (28b, Item 126). Delivery receipts and `relies_on` stay filed.)_
- `categorize.v1` in shadow (§4.1a). _(Built 2026-10-04, step 28b; Part III Item 126.)_
- _`security.v2` and `security.v3`, candidates in shadow (2026-10-03, Part III Items 80 and 95): computed facts, a decoding scrubber, a planted-injection eval set, and `steered` deciding beside `risky` above a provisional 0.75. The bar comes from shadow data through the soak, and file provenance (option B) is decided at v1 (the owner, 23:24; theseus-sh05)._
- _`security.v1` and `security.v3` in shadow at the gate since 2026-10-04 (step 24; Part III Item 120), `security.v2` not wired: the score on notices, and the press's labels._

**Prove.** On a held-out set of recorded trajectories, Jev-driven stopping and classification beat the deterministic-only baseline at equal total budget (judge cost included) on task success, false completion, and unnecessary continuation. Any pack that does not beat baseline stays in shadow and the plan says so.

**Not yet.** No memory science. No MCP.

## P8. M6 — Memory as experiment

**Build.**
- The context graph beyond transcript: typed edges, roots, compaction roots (append-only, with `derived_from`), rotating ring, assembled continuation. `CONTINUE` goes live for recompile-strategy choice if M5 said it could. _(Compaction roots, `context_overage`, and the assembled strategy's first form were built 2026-10-04, step 30c, Part III Item 131; `CONTINUE` stays in shadow.)_
- The index tender: Nomic v1.5 embeddings (768 stored, 256 indexed), usearch, tantivy, reciprocal-rank fusion. Baseline retrieval: transcript tail + task graph + summaries + BM25/embedding + freshness and provenance rules. _(The index tender runs since row 51, 2026-10-02: Part III Item 50. Recall's wire-in is 30a.)_
- `MemoryScience` trait with a **baseline** implementation (no retention model, no activation) and a native FSRS-6 + prediction-error + spreading-activation implementation behind it.
- The memory pass and recall as specified, consolidation as a tender job producing shadow syntheses with citation checks.
- Tiering tender: demote by heat, rehydrate on reference; arena as a bounded cache with the presence filter. _(Its first cut, step 33, is in the core and no tender: payload stubs and a bounded heat cache of decoded nodes, since 2026-10-05, Part III Item 162. Demotion to S3 and the presence filter wait for a store that needs them.)_
- The ablation harness: each feature toggled independently at fixed total budget, scored on §5.5a's metrics over recorded trajectories and a live canary. _(The exam half was wired to the real pipeline on 2026-10-04 (34b's wire-in, Part III Item 124): one scratch daemon per arm, the report and the replay as `theseus-exam` commands, and the first honest report, on GLM. The canary half waits for canary data.)_
- **Books** (the owner, 2026-10-01; theseus-lqo6): recall organizes the graph's context into typed books, each a different organization of knowing, built with the recall wire-in (the "mini RAG", rows 30a to 30c).
  - The five: a **dictionary** (keyed by the exact term), an **encyclopedia** (by topic), a **cookbook** (by goal), an **SOP reference** (by situation), and a **diary** (by time). A casebook (decisions, as precedent) and a register (items with a lifecycle state) are candidates.
  - A book type is five answers: its key, its entry's shape, its write discipline, its authority, and its compile rule. It extends the ontology's closed set of composition rules (`chain`, `intent_line`, `ranked`, `recall_only`), and §5.2's memory-pass kinds say which book an entry belongs to.
  - Books are derived views, never the source of truth. Each entry cites its nodes (`derived_from`) and takes the strictest label of its sources (§5.4) _(labels on nodes were removed on 2026-10-03, Part III Item 76: in a shared place, recall and the books draw only on that place's own sessions)_. A rebuild from the graph must reproduce the book, and that is a test.
  - Only the operator writes an SOP; a recipe is promoted only after repeated success; nothing retractable goes in the shared header (Appendix F). A book type exists only once the compiler reads it (the reader rule), and the exam measures recall per book. _(Since 2026-10-05, 35a; Part III Item 183: of the classes a compile's check knows, notes (`recall_note`, `recall_section`) and summaries are the ones the diary will admit; no book or node kind is built yet.)_
- The ontology's learning half (§4.1a): category embeddings, re-association by sweep and dream, lessons as guidance, and §5.5's namespaces as kinds. Also compilations that are never silently thinner, testimony and precedence, and volatile values rendered as-of (theseus-3nk). _(Built 2026-10-05 as M6's 35a; Part III Item 183: situations as a compiler input, with a check that fails a turn whose request its situation does not admit, enforcing from the first day; the precedence line; testimony headers; volatile values as of a date. Lessons (35b) are not built.)_

**Prove.** An ablation report exists and is honest. Features that do not move task success, false completion, stale recall, or disclosure violations at equal cost are disabled by default and marked experimental in the spec.

**Not yet.** Voice, MCP, AWS shells, multi-channel gliding.

## P9. M7 — Surface

**Build.**
- Multi-guild, multi-channel bindings; per-channel ceilings; gliding with intersected ceilings; coalescing with per-author authority; proactive and scheduled work under derived authority and owner grants; `Wake` nodes. _(2026-10-03: a whole guild may be declared private, Part III Item 89, and wakes repeat, Item 84. ~~38a, the bindings' second format, is startable;~~ 38a, the bindings' second format with a ceiling per place, was built on 2026-10-04 (Part III Item 117); ~~38b, gliding, waits for the owner's call on a redesign, since it was built on the labels the place rule replaced.~~ 38b, gliding, was redesigned on the place rule (the owner, 2026-10-04 15:09) and built that afternoon, Part III Item 140.)_
- Tasks fluid in chat with the three mutability layers, CAS, claim leases, workspace locks.
- MCP client (tools, prompts, elicitation in), then MCP server on localhost with the static key, sampling budgeted and Jev-judged.
- Voice: `songbird` receive and send, STT and TTS as accounted spend, the voice turn as a workload class in the Jev latency budget. _(~~Leaves v1 until the owner chooses speech providers: rows 77 and 78.~~ Its crate is parked outside the workspace since 2026-10-03, Part III Item 72. Rows 77 and 78 return to v1: the owner chose Deepgram at 14:20 that day, §2. Built that afternoon, Part III Item 83.)_
- AWS shell classes A1–A4 with scoped task roles, SQS completion transport live, EventBridge task-state changes into the queue, reconciler API polling only past deadline.
- Web UI grows: in-thread observability, policy mapping administration, budgets, ledger views, learning channel.
- Self-extension (§3.21): `extend.propose`, the operator ack, hot-loading a sandboxed MCP server as a tool, revocation.

**Prove.** Theseus carries the owner's daily Discord work end to end for two weeks with OpenClaw out of the loop for that channel, with the ledger showing budgets, judgments, and hook runs, and no disclosure or authority violation in the record. _(The v1 rule since 2026-10-04 at 22:09, the owner's "A": v1 comes after at least 7 days of his daily use with no serious bug, a serious one restarting the count, with every speed target met and each of B5's losses explained or fixed; about October 13 at the earliest. It replaces the two weeks; §2.)_ _(The count began on 2026-10-05 at 16:48, when install #5 put the v1 build on his daemon (theseus-u8aa; §2), so its seven days end on October 12 at 16:48 at the earliest. Every install since carries fixes on top of it.)_

## P10. What is cut from the first useful agent, on purpose

Jev, roles, memory science, compaction, MCP, voice, AWS shells, hooks, multi-channel. M3 is a Discord front end on a durable execution kernel with a bash tool. If that is not already useful for coding in a known repo, the intelligence features will not rescue it; if it is, every later feature has a baseline to beat.

## P11. Risks the plan is built around

| Risk | Where it bites | Mitigation in the plan |
|---|---|---|
| Kernel rewrite after simulator findings | M2 | Simulator exists before the kernel does (M1); the fake tool and mailbox keep the rewrite cheap |
| Integration work starves the kernel | M3 | Discord and Anthropic are not started until M2 is green |
| Durability work steals turn latency | M4 | Measured explicitly; the turn lock releases only at offload boundaries, and the tender is a separate process |
| Jev does not beat the baseline | M5 | Shadow first; a pack that loses stays in shadow and the spec says so |
| Memory science does not transfer | M6 | Baseline first, ablations gate every feature |
| L0 default proves unsafe in practice | M4 onward | L1 ships with contract tests in M4 so switching the default is a config change, not a project |
| Embedded store becomes the bottleneck | M6–M7 | The `Store` trait and the M1 benchmark harness make the swap a bounded project |

## P12. Immediate next steps

1. Beads epics `theseus-9w9` (M0) through `theseus-ext` (M7) exist in the theseus repo, chained by dependency, each carrying its "prove" line.
2. M0 first steps: workspace crates, `rust-toolchain.toml`, `cargo deny`, the 1Password config loader, the hook registry, the turn runner, the protocol server, `theseus chat`.
3. Repository: `~/projects/theseus`, `github.com/zeroaltitude/theseus` (decided). This document lives there alongside the design notes: since v0.80 in chapters under `docs/spec/`, indexed by `docs/spec/README.md`, with `docs/the-ship-of-theseus.md` pointing to them; until v0.79 as the one file `docs/the-ship-of-theseus.md`.

