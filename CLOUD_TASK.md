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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-continue-shadow`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: CONTINUE's candidate signals and `continue.v1` in shadow, step 25b (theseus-0j2.7)

Branch: `cloud/20261004-continue-shadow`. Every commit's subject carries `theseus-0j2.7`. Deadline for the report: 4
hours after you start.

**Background.** The context compiler (crates/theseus-core/src/compiler.rs, a pure `compile()`) recompiles only on
deterministic triggers: a new session, a changed model, system block, or tools, a manual recompile, an overflow, a
refused image. Otherwise it appends to the compiled prefix. CONTINUE (spec §4.4a) is the judgment of whether to
recompile, and how, when no trigger fired. Step 25b builds its cheap candidate signals, deterministic, inside
`compile()`, and asks Jev's `continue.v1` in shadow only when a signal fired and no trigger did: no signal, no call, so
most turns in a live thread cost nothing. Nothing acts on it in M5: the compaction and assembled strategies the pack
can name are M6's. Step 23a wired Jev in, in shadow (`[judge]`, `JudgeService` in crates/theseus-core/src/judge/, a
sink that records every judgment as a `judge.call` row, the shadow day budget, and `loop.v1` at a turn's end).

**Read first:**
- docs/design/m5-judgment.md: §2.4's `continue.v1` row and its "CONTINUE's candidate signals" bullet, §2.15's
  `[judge.signals]`, §2.16 FAST, and §3's 25b entry; the spec's §4.4a (docs/spec/05-part1-s4.md);
- crates/theseus-core (AGENTS.md first): compiler.rs (`CompileInput`, `Compiled`, `trigger`, and its tests), the
  turn's compile call and its `context.compiled` row, config/judge.rs, judge/ and tests_judge.rs;
- crates/theseus-judge: packs/continue.v1.toml, builders.rs (`ContinueInput`, `SignalInput`), and fake.rs.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- The compiler has grown: the place rule's withheld context files, the overflow ring, refused images, memory recall
  in shadow (30a), and the ontology's guidance walk (21b). Keep the signals in a module of their own beside
  compiler.rs, with one field on `CompileInput` and one on `Compiled`.
- The store has one format number. The signals ride on `context.compiled` (a ledger row and its notification), not on
  the stored `Compilation`, so they need no bump; a stored field would.
- The config note is sparse: `[judge.signals]` goes under `[judge]` (config/judge.rs) with defaults and template
  lines, held by the template tests.
- A plain turn writes 5 frames. The sink's own frames can land inside a turn's window: jev-wire-in's join found one
  inside the turn bench's measured turn (theseus-0j2.3, settled at that join; read how before you add judgments inside
  turns). A turn's own frames never change.

**One convention, four sessions** (you, 23b's surfaces, and the gate and inbound points, each dispatching judgments
inside turns; keep to these names so the merges stay mechanical). The core mints a judgment's id at its dispatch:
`theseus_judge::Ask` gains `pub id: Option<String>` (`Ask::new` sets `None`), and `Judgment::pending` uses it when set,
else mints one as now. Each dispatch marks the turn's trace with a zero-length span: name `judge`, kind `mark`,
attributes `pack` (as `continue.v1`), `point`, `mode`, and `judgment` (the id its row will carry). Put your point in a
module of its own under judge/ (a child module sees the service's private parts): `judge/mod.rs` gains a `mod` line
and its `WIRED` line, and 23a's loop path stays as it is.

**What to build (25b):**
1. The signals, pure, each from what `compile()` is given (the clock passed in, never read inside): a dormancy gap
   (over `dormancy_minutes`, 360, between a new input and the node before it), the tail crossing a soft band
   (`tail_band`, 0.5, of the window, then each further quarter), a task report or wake arriving, and a provider cache
   miss (cache reads fall to zero while the prefix is unchanged). Each fired signal, with its value, on
   `context.compiled`.
2. `[judge.signals]` (`dormancy_minutes`, `tail_band`), so tests and live checks can shorten them. The signals are
   computed whether or not the judge is on.
3. The compile point: when a compile fired a signal and no deterministic trigger, `continue.v1` in shadow, spawned
   and marked, its state (`ContinueInput`: the signals, the window, prefix, and tail sizes, the last cache read, the
   compilation's age, strategy and trigger, the budget left, the last human message) built off the turn's path.
4. The compiler decides nothing new: its request bytes are the same with the judge on or off.

**Proof, offline**, against the fake Jev: §3's 25b tests: pure `compile()` tests, one per signal, each beside its near
miss; no signal, no judgment; a deterministic trigger, no judgment. Also: the request bytes the same with the judge on
and off; a judged turn keeps its frame budget; the output golden changes only where a signal fires
(`THESEUS_GOLDEN=write`, and say why). Planted reverts: fire the dormancy signal at any gap, and show its near-miss test
fail; dispatch when a trigger fired, and show the trigger test fail.

**The live check is the maintainer's**, with a real key under `[secrets] jev_api_key`. Write it as exact commands on
a scratch daemon with a fresh state directory, a GLM profile, `[judge] enabled = true`, and `[judge.signals]
dormancy_minutes = 1`: a turn, a two-minute wait, another turn in the same session. The second turn's
`context.compiled` row (`theseus ledger --json -k context.compiled`) shows the dormancy signal, `theseus judge log`
shows a `continue.v1` judgment, and the turn's request digest equals a judge-off run's.

**Leave alone:** the compiler's strategies and triggers (compaction and the assembled strategy are M6's); memory
recall in the compiler and the turn, and the Recall node (another change); the ontology's guidance walk; the
memory's `+rerank` arm; 23b's surfaces; and the other judge points.
