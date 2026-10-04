<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate you touch, scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

**Setup** (about 15 minutes, once):
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- `npm ci` in cockpit/.
- `cargo build --workspace --all-targets` (about 9 minutes cold).
- `cargo deny fetch`, so the gate's deny phase can run offline. If the fetch fails, skip that phase and say so in the report.
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.

**Known on this VM, and not yours to fix** unless your task names them (other changes fix them):
- The sandbox's contract tests that need a non-root user can fail or skip here: the VM runs everything as root, and Linux exempts root from `RLIMIT_NPROC` (theseus-pv6i).
- `theseus-core tests_push::the_snapshot_and_the_events_agree_under_the_position_rule` failed once in 20 suites here (theseus-amr2).
- A reaping or job test whose `turn.submit` is answered `internal`, `No such file or directory (os error 2)`, under load (theseus-46ya: a spool read race).

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About twenty-five other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- the Jev wire-in (`[judge]`, `JudgeService`, the judgment sink, `loop.v1` in shadow, crates/theseus-judge's client);
- the ontology wired in (crates/theseus-ontology's records, its snapshot, the context compiler's guidance walk; `MANIFEST_FORMAT` goes to 7);
- MCP tools in turns (`McpBoard`, `[mcp.servers]`, the `Tool` trait's names as `&str`);
- `theseus judge prove` (a report generator in crates/theseus-judge);
- five gate flakes;
- the user unit's restart limits and install checks (crates/theseusd/src/install/, scripts/user-service.sh);
- the MCP server (`[mcp_server]`, `/mcp` on loopback, `Surface::Mcp`);
- AWS hands on Lambda and Fargate (the `hand` role, `aws.hands.run`, the SQS poller, infra/aws/'s hands files);
- the durability tender (WAL segments and blobs to S3, index rows to DynamoDB);
- a terminal toolset (`term.*`, a pty per session);
- the gate's bench build, features recheck and cockpit build (scripts/gate.sh).

These are other cloud sessions like you, each on its own branch:
- the judgment surfaces (trace marks, `judge` spans, `judge.list`/`get`, the cockpit's Judgment section);
- `security.v1` in shadow at the gate;
- `classify.v1` and `role.v1` at inbound;
- CONTINUE's signals and `continue.v1` in shadow;
- the arrangement on `task.create`;
- the memory `Recall` node, `derived_from` edges and the `BudgetReport`;
- the `+rerank` memory arm;
- the cockpit's Ontology view;
- `categorize.v1` in shadow and `tasks.parked` in health;
- bindings format 2 (many guilds, per-place ceilings);
- MCP prompts (`/prompt`, `theseus prompt`, the cockpit's picker);
- `extend.propose`;
- `theseus restore --from s3://…`;
- AWS hands' cancellation, TTL reaper, reservations and grid;
- the LSP board and tools (crates/theseus-lsp's consumers).

On the owner's machine:
- a benchmark run (bench/, and docs/benchmarks.md at its end).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field to a stored record or a new record kind, bump it by one from main's number as you cloned it, and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. Others bump it too: the maintainer renumbers at the merge.
- **Files near their line ceiling** (scripts/long-files.txt): crates/theseus-protocol/src/lib.rs, crates/theseus-core/src/turn.rs, crates/theseus-core/src/config.rs, crates/theseus-discord/src/runtime.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/tests_m3.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-judgment-surfaces`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- Don't edit the spec, docs/status.md, the README, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
- Don't commit half a step. If time runs out mid-step, leave it out and report what you found.

**The report:** when you are done, or at your task's deadline (by the clock, waits included), whichever comes first, write CLOUD_REPORT.md at the repository root. Commit it as the branch's last commit, subject `cloud report (not for main)`, and push. The maintainer reads it from the branch and drops that commit at the merge. For each step it says:
- what you found;
- what you changed (commit hashes);
- how you proved it: the commands and their results, with test counts, the runs under load, and each planted revert and what failed;
- the live check the maintainer should run, as exact commands, and what each should show;
- what is left or uncertain, and any design choice the owner should hear about.

Then the gate's result, naming each failing test and why. Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line per step, and the gate's result.

**Planted reverts:** to prove a test guards a behaviour, plant the bug, show the test fail, then restore the file and `touch` it, so cargo rebuilds it (a restored file with its old mtime keeps the planted build). Run `git status` after every restore.

**Load:** where your task asks for runs under load, use AGENTS.md's recipe. Priority, not count, makes the load: run the test with `nice -n 19`, and beside it four busy loops at nice 0, each `sh -c 'while :; do :; done' &`. Kill the loops by the pids you started, never by a name pattern.

---
## Your task: the judgment surfaces, step 23b (theseus-0j2.4)

Branch: `cloud/20261004-judgment-surfaces`. Every commit's subject carries `theseus-0j2.4`. Deadline for the report: 5
hours after you start.

**Background.** Step 23a wired Jev, an external judging API, into the daemon in shadow: `[judge]` (off by default),
`JudgeService` in crates/theseus-core/src/judge/ (built at the first judgment, never on the start path), a sink that
writes every judgment as a `judge.call` ledger row (keyed by its `jdg_…` id, scoped `judge:<pack id>`, in the sink's
own frames, its state in a blob first), the shadow day budget, and `loop.v1` judged after each turn the baseline ended
with no tool calls. Today a judgment shows only as a row, health's `judge:` line, and `theseus judge log`. Step 23b
makes judgments visible wherever the turn is: the trace, the narrative, telemetry, the protocol, the CLI, and the
cockpit. Other sessions add packs beside you, and later steps add labels, the ladder, and live judgments: build every
surface for any pack, not for `loop.v1` alone.

**Read first:**
- docs/design/m5-judgment.md: §2.2's latency row, §2.5 (its trace paragraph), §2.6, §2.13 surfaces, §2.16 FAST, and
  §3's 23b entry under "Each step's tests and live check";
- crates/theseus-core (AGENTS.md first): judge/ (mod.rs, sink.rs, spend.rs, loop_end.rs), fact/mod.rs (the `Fact`
  trait: a row, a notification, sentences, a span) and fact/judge.rs, trace.rs (marks), narrative.rs, telemetry/
  (metrics.rs and its tests' fixtures), rpc/memory.rs (`memory.recalls`, a list over scoped rows: the model for
  `judge.list`), tests_judge.rs, and turn.rs where it calls `judge.after_turn`; crates/theseus-judge's judge.rs
  (`Ask`, `Judgment`) and fake.rs; the CLI's render/judge.rs;
- cockpit/AGENTS.md, then cockpit/src/views/SessionDeck.tsx, src/components/Flame.tsx, src/lib/history.ts and
  src/lib/timemachine.ts.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- The cockpit (cockpit/, served at `/`) replaced the Observatory and the old web app; the protocol's TypeScript is in
  cockpit/src/protocol.gen/.
- 23a dispatches `loop.v1` after the turn's last frame, and that frame carries the turn's trace. A mark in the trace
  needs the dispatch decided (mode and sample are pure) and the judgment's id minted before that frame.
- Facts (fact/mod.rs): one type per thing that happened, each channel its projection. 23a gave the judge's facts rows
  only. Their sentences, metrics, and spans are yours, and the sink writes outside any turn.
- A plain turn writes 5 frames. Judgments add none, and no notification method: they stream on `ledger.tail`, which
  the cockpit already follows.
- Agreement, labeled precision, the calibration strip, label buttons, and promote or rollback are later steps' (25c,
  26a). `judge` spans for live judgments come with the first live pack (26b): render the kind, write none.
- The store has one format number: rows and META keys need no bump.

**One convention, four sessions** (you, and the gate, inbound, and compile points beside you, each dispatching
judgments inside turns; keep to these names so the merges stay mechanical). The core mints a judgment's id at its
dispatch: `theseus_judge::Ask` gains `pub id: Option<String>` (`Ask::new` sets `None`), and `Judgment::pending` uses it
when set, else mints one as now. Each dispatch marks the turn's trace with a zero-length span: name `judge`, kind
`mark`, attributes `pack` (as `loop.v1`), `point`, `mode`, and `judgment` (the id its row will carry). Don't reshape
23a's loop path beyond what the mark needs; a pack's point lives in a module of its own under judge/.

**What to build (23b):**
1. The marks: `loop.v1`'s dispatch decided and its id minted before the turn's last frame, marked there, and spawned
   after it as now. No new frame, no network, nothing awaited.
2. Sentences and metrics for the judge's facts, recorded where the sink writes: a judgment landing ("Jev, in shadow,
   judged the stop: progressing (0.81, act band). The baseline ended the turn; recorded, not acted on."), the pause
   and its end (`judge.resumed`, at the first judgment of a new day after a paused one), the breaker's moves.
   Metrics: `theseus.judge.calls` {pack, mode, band, class}, `theseus.judge.duration_ms` {pack, class},
   `theseus.judge.on_path_ms`, `theseus.judge.errors` {class}, `theseus.judge.disagreements` {pack} (say how you
   define a disagreement with the baseline), and `theseus.cost.usd` with `theseus.spend = judge`.
3. The protocol: `judge.list {pack?, session_id?, since?, limit}` (rows, no states) and `judge.get {id}` (the row and
   its state from the blob), types in theseus-protocol/src/judge.rs. Health's judge block gains what §2.13 lists and
   23a left out (shed; the key's state).
4. The CLI: `theseus judge log` on `judge.list`; `theseus judge show <id>` (the state as fields, each answer with its
   probability and band); the marks in `ask --trace`.
5. The cockpit: a Judgment section (per pack: mode, version, calls, cost, p50 and p95 per workload class; a judgment
   log with filters; a judgment's state as fields and its answers as probability bars), and in a session's view each
   judgment beside the loop it judged, with its mark in the flame chart. Follow cockpit/AGENTS.md's invariants: read
   only what is on screen, one copy of the ledger, state in the address, and a step in the time machine's fold for
   any row that changes what a view shows.

**Proof, offline**, against the fake Jev: §3's 23b tests (a mark per shadow dispatch, naming the id its row carries; a
narrative line when a judgment lands; the metrics' names and attributes against the telemetry tests' fixtures;
`judge.list`'s filters; `judge.get`'s state equal to its blob); a judged turn keeps its frame budget and its request
bytes; the lifecycle bench's shape with the judge on (the maintainer measures); the cockpit's lint, tests, and build.
Planted reverts: mint the id after the last frame, and show the mark test fail; let `judge.list` ignore its pack
filter, and show its test fail.

**The live check is the maintainer's**, with a real key under `[secrets] jev_api_key`. Write it as exact commands on
a scratch daemon with a fresh state directory, `[judge] enabled = true`, and `[telemetry] otlp_endpoint` at a local
OTLP receiver: three `ask` turns, one with `--trace` showing its `judge` mark; `theseus judge log`, then `theseus judge
show <id>`; the cockpit's Judgment section shows the calls, cost, and p50/p95, and the session's view shows each
judgment beside its loop; the receiver shows `theseus.judge.calls`.

**Leave alone:** the other judge points beside you (the gate, inbound, compile, an exchange's end) and the memory's
`+rerank` arm, which reaches Jev through M5's client; the prove report (crates/theseus-judge/src/prove.rs and its
`theseus-judge` binary); the packs; the cockpit's Ontology view, MCP prompt picker, and extension card (other changes: add only your
own route and section); and crates/theseus-core/src/external.rs.
