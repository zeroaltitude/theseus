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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-task-arrangement`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the arrangement on `task.create`, step 27 (theseus-vug.2)

Branch: `cloud/20261004-task-arrangement`. Every commit's subject carries `theseus-vug.2`. Deadline for the report: 5
hours after you start.

**Background.** `task.create` (crates/theseus-core/src/task.rs) opens a child session that works on its own and
reports back: depth one, a budget carved from the parent's, the brief as its first message, `wake_parent`, and, since
this week, its own one-shot wakes. The owner decided that promoting work to a task needs an authored arrangement: the
model quotes the messages that define the work, rather than paraphrasing them into the brief, and the quoted pieces go
to the child verbatim. Step 27 adds it: references resolved to nodes, the refusal without one, the fidelity check for
a one-line brief drawn from a long discussion, and the pieces admitted into the child's first compilation. Check tasks
(28a) and M7's task record (39a) build on it, and 39a keeps its refusal cases in its tests.

**Read first:**
- docs/design/m5-judgment.md: §2.10 whole, §2.14's node row, §5's Q6 and Q7, §6's "exact quotes" risk, and §3's 27
  entry under "Each step's tests and live check"; docs/design/roadmap-v2.md §6, conflict 6;
- crates/theseus-core (AGENTS.md first): task.rs whole (`create`, the brief, `is_brief`, the report), node.rs (`Body`,
  `Node::relayed`), graph.rs (`derived_from`, `VIA_BRIEF`), compiler.rs (how a task's first compilation renders its
  brief), tests_tasks.rs, and tests_layouts.rs; crates/theseus-kernel's `open_task`; the CLI's `theseus tasks`, and
  the cockpit's task views.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- The store has one format number, `MANIFEST_FORMAT`. The `Arrangement` body is a new `Body` variant, so it bumps the
  number by one from main's as you cloned it (the preamble's "Shared files"), with the layout sample the rule asks
  for, and a store holding it is refused by an older build. Another change tonight adds a node body too (the Recall
  node); the maintainer renumbers at the merge.
- `task.create` has grown since the design: `wake_parent`, a task's own wakes, a parent's external-text hold passed
  to its task, and a brief that is a relayed `UserMessage` with a `derived_from` edge to the reply that holds the call
  (12a's convention). Keep all of it.
- The place rule replaced labels and audiences: a task takes its parent's class, and a quote resolves only in the
  calling session's own transcript, so the child sees nothing its parent could not.
- The Observatory is gone: the pieces show in `theseus tasks` and the cockpit's task view (cockpit/, served at `/`).
  There is no `theseus tasks show`; add it only if the list can't carry the pieces.
- Jev plays no part here: the learning ledger (25c) compares `classify.v1`'s `should_promote` with the model's own
  `task.create` calls.

**What to build (27):**
1. `arrangement` on `task.create`'s input: `pieces` (each `{quote, role}` or `{node, role}`; role `objective`,
   `acceptance`, `design`, or `context`), `trust` and `supersedes` by piece index, and `fidelity_ack`, in the tool's
   schema and its description, which teaches the model to quote.
2. Resolution: a quote of at least 20 characters must match exactly one node of the calling session's transcript;
   otherwise the call fails and says why (no match; or ambiguous, with the candidates' times and authors), so the
   model tries again. Say how you normalize whitespace, if at all. The result lists the resolved nodes (id, author,
   time, first line).
3. Refusal: with no arrangement, or none with an `objective` or `design` piece, the call fails: "Promotion needs an
   arrangement: quote the messages that define this work."
4. The fidelity check: a brief under 200 characters, from a session with more than 10 human messages since its last
   task, with a single piece, fails, asking for the design to be attached or for `fidelity_ack: true`. The ack is
   ledgered and shown on the task's surfaces.
5. The `Arrangement` node in the child's session, written in the frame `open_task` already writes, with a
   `derived_from` edge to each piece's node. The child's first compilation renders the brief, then each admitted
   piece's full text with its author, time, and origin session (a `trust` piece marked as trusted testimony), and a
   superseded piece by reference only, never its text. `compile()` stays pure over the child's own nodes, so the node
   carries what it renders.
6. Surfaces: `📎 N pieces` on the task's lines (its start, `theseus tasks`, the cockpit), the pieces in the cockpit's
   task view, and the ledger rows.

**Proof, offline:** §3's 27 tests: quote resolution (exact, unique, at least 20 characters; no match; ambiguous); the
refusal without an arrangement; the fidelity flag, and its ack; the child's first compilation rendering each piece
verbatim after the brief with its origin and time, and superseded pieces by reference only; the prefix the same
across the task's later turns; a node written before the bump still read. Also: DD7's and the task wakes' tests
(core and daemon) pass, each `task.create` they script given an arrangement; name every test you changed. Planted
reverts: accept an ambiguous quote's first match, and show the ambiguous test fail; render a
superseded piece's text, and show its test fail.

**The live check is the maintainer's.** Write it as exact commands on a scratch daemon with a fresh state directory
and a GLM profile: a 12-message scratch discussion of a small change, then "do it as a task". GLM quotes the pieces,
and the task's first compilation (`theseus rpc compilation.list '{"session_id": "<task session>"}'`) shows them after
the brief; a one-line brief with a single piece is flagged; the report arrives once.

**Leave alone:** `check_of` and independence (28a), M7's task record (39a), the task's wakes and the report's wake,
`tasks.parked` in health (another change), the Recall node and the compiler's recall (another change), the ontology's
guidance walk, and the judge.
