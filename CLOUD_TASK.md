<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate you touch, scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

**Setup** (about 15 minutes, once):
- First, in the foreground and alone, run `cargo --version` and wait for it: it installs the toolchain that
  rust-toolchain.toml names (about 30 seconds). Start no other cargo or rustup command until it is done: two at once
  collide in rustup's download directory. If it fails, run it again.
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- `npm ci` in cockpit/.
- `cargo build --workspace --all-targets` (about 9 minutes cold).
- `cargo deny fetch`, so the gate's deny phase can run offline. If the fetch fails, skip that phase and say so in the report.
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.
- Never delete anything outside the repository and /tmp (nothing under /root/.rustup or /root/.cargo). This
  environment refuses some commands, and three refusals in a row stop the session until a person looks: when one is
  refused, take another route instead of retrying it.

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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-task-record`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the task record: the `TASK` kind, three layers, versions, and the task tools, step 39a (theseus-ext.6)

Branch: `cloud/20261004-task-record`. Every commit's subject carries `theseus-ext.6`. Deadline for the report: 5 hours
after you start.

**Background.** A task today is a session (DD7, crates/theseus-core/src/task.rs): `task.create` opens a child
execution at depth one with a carved budget, which reports once and sets its own wakes (37b); since step 27 it needs an
authored arrangement (`arrangement`, `pieces`, `fidelity_ack`, an `Arrangement` node in the child: the names 27's
prompt gave; the code on main is the truth wherever it differs). There is no task record: no title but the brief's
first line, no states but the execution's, no plan, acceptance or evidence. Step 39a makes the task graph a persisted
structure (spec §3.5): a `TASK` record kind, three layers so the agent cannot redefine success, versions with
compare-and-swap, the `task.*` tools, and the graph in the model's view. Leases, the board, `/tasks`, the Discord card
and the cockpit's graph are 39b's.

**Read first:**
- docs/design/m7-surface.md §2.4, §5's questions 12, 13 and 15, and 39a's entry under "Each step's tests and live
  check"; the spec's §3.5 (docs/spec/03-part1-s3.md); docs/design/roadmap-v2.md §6, conflict 6;
- crates/theseus-core (AGENTS.md first): task.rs, 27's arrangement module and tests, session.rs, store.rs's
  `with_session` (the lock and its order), rpc/confirms.rs (`judge_act`, `Act`) and the budget question, compiler.rs,
  tests_tasks.rs, tests_layouts.rs; crates/theseus-kernel's `open_task`; crates/theseus-store's `kinds` and
  `MANIFEST_FORMAT`; theseus-protocol's `TaskInfo`; the CLI's `theseus tasks`.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **One store format number.** `TASK` is a new record kind (the next free number), so `MANIFEST_FORMAT` goes up by one
  from main's as you cloned it, with the layout sample; `kinds::SCHEMAS` is gone. The maintainer renumbers at merges.
- **27's arrangement stays.** `task.create` with `brief` keeps 27's rules, and its refusal cases stay in your tests
  (conflict 6). A plan item (no `brief`) delegates nothing, so by default it needs no arrangement; say what you chose.
  A record's objective and acceptance come from the arrangement's `objective` and `acceptance` pieces when given.
- **`judge_act` traces no process.** An accept counts when it is the owner's from a private place, and the CLI refuses
  it in a job's shell (`THESEUS_SESSION`): "a job's process can't accept" means both.
- **"A task with a session follows its execution, in the same frame"** would reach into every kernel transition
  (kernel.rs is at its line ceiling). The default: derive a session task's running states (`in_progress`,
  `waiting_human`, `suspended`) from its execution when read, and write the record at its own changes only, its close
  by the report (in the frame that ends the task) included. Say what you chose.
- **The Observatory is gone.** The surfaces are the protocol, `theseus tasks`, the ledger, the narrative and
  telemetry. Keep the cockpit's task views working: `task.list`'s answer keeps every field it has, and the records
  come with it.

**What to build (39a):**
1. **The record:** `Task { id: tsk_…, version, title, objective, acceptance, state, parent?, deps, owner, session?,
   origin, evidence, proposal? }` (§2.4; `claim` is 39b's), its kind and bump, and a per-task lock in the order
   session, task, execution. Old stores' task sessions, with no record, still list from their executions.
2. **The tools, under CAS:** each edit names the `version` it read; a stale one is refused with the record as it is
   now (`task.stale_refused`). `task.create` (with `brief`, the session and its record in `open_task`'s frame;
   without, a plan item), `task.update { id, version, patch }`, `task.split { id, version, into }`, `task.close { id,
   version, outcome, evidence }`. Layer 2 (title, children, deps, owner) applies; layer 3 (evidence: a node and its
   identity, a commit or a job id) only appends; layer 1 (objective, acceptance, abandoning an accepted task) becomes a
   proposal.
3. **Layer 1's proposal** waits for the task's requester, else the owner, through the existing question path (`theseus
   confirm`, the cockpit's card): accept applies it in one frame (`task.change_accepted`), decline leaves it
   (`task.change_declined`). The gate's path may be simplest (a layer-1 patch plans as a call that always waits, as
   the floor does), or a question of its own, as the budget question is; say which.
4. **A session's task** follows its execution as decided above; its report closes it `done` with the report as its
   evidence, and a failure closes it `failed`.
5. **The view.** A turn whose scope has tasks (a conversation: the tasks it started and their children; a task: its
   subtree and its parent's line) sees a line per open task (id, title, state, owner, deps, a line of acceptance,
   version) and one per closed subtree with a count, about 1,500 tokens at most (then open tasks only, with what was
   left out counted). In the tail, after the cached prefix, and never in a plain turn's request. `compile()` stays
   pure: pass the view in, and record its digest and counts on `context.compiled`.
6. **Surfaces:** `task.list` returns records, `task.get` reads one, a `task.changed` notification, `theseus tasks` as
   a tree with states and versions; §2.4's ledger rows but the lease's; a narrative line per change; a counter per verb
   and a `theseus.tasks.open` gauge.

**Proof, offline:** the design's 39a tests: a stale version refused; a proposal waits, accept applies it, decline
leaves it, and an accept from a job's shell is refused; evidence never removed; a crash between steps (records
survive, and DD7's sessions without one still list); the view bounded, with its count; a plain turn's 5 frames and
token count unchanged. Also 27's refusal cases with and without `brief`, and a store written before your bump opening.
Run the CAS tests 5 times under load. Planted reverts: accept an edit whose version is stale, and show the stale test
fail; let `task.update` apply an objective patch at once, and show the proposal test fail.

**The live check is the maintainer's.** Write it as exact commands on a scratch daemon with a fresh state dir, a GLM
profile, and a scratch git repository as the workspace:
1. Three small changes discussed, then "plan these as three tasks, and split the second into two": `theseus tasks`
   shows the tree with states and versions.
2. After a scratch commit, "close the first, with the commit as its evidence": `task.closed` carries it.
3. "Change the second task's acceptance to …": a proposal waits; `theseus confirm` accepts it, and the next turn's
   view shows the new acceptance.
4. On `theseus-sim fake-model --rules`, a rule that edits a task at a version another rule already moved past: the
   edit is refused, `task.stale_refused`.

**Leave alone:** 28a's `check_of` and basis, built beside you in `task.create` too: keep your edits to its `Input`,
schema and description additive, and your code in modules of its own; 27's resolution and fidelity rules; the task's
wakes and the report's wake; `tasks.parked` (28b); the compiler's recall and M6's compaction roots (other changes); the
kernel's transitions themselves (records ride the closures the core passes them, as `open_task`'s do).
