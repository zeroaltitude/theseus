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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-independence`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: independent check tasks: `check_of`, the exclusion set, the overlap flag, and the basis, step 28a (theseus-vug.3)

Branch: `cloud/20261004-independence`. Every commit's subject carries `theseus-vug.3`. Deadline for the report: 4 hours
after you start.

**Background.** A task (DD7, crates/theseus-core/src/task.rs) is a child session that sees only what its parent gives
it: the brief and, since step 27, an authored arrangement. 27 added `arrangement` to `task.create` (pieces `{quote,
role}` or `{node, role}`, roles `objective`, `acceptance`, `design`, `context`; `trust`, `supersedes`,
`fidelity_ack`), each quote resolved to one node of the calling session, and an `Arrangement` node in the child's
session, whose pieces the child's first compilation renders after the brief. Those are the names 27's prompt gave; the
code on main is the truth wherever it differs. Step 28a adds the check task (theseus-vug: "independence as a compiler
property"): `task.create { …, check_of, profile? }` opens a task that checks another task's work by its claim, never
its working, and records why the check is independent.

**Read first:**
- docs/design/m5-judgment.md §2.10, §2.11, §2.14's session row, and 28a's entry under "Each step's tests and live
  check"; docs/design/roadmap-v2.md §6, conflict 6;
- crates/theseus-core (AGENTS.md first): task.rs (`create`, `Report`, `load_report`, `resolve`, `info`), 27's
  arrangement module and its tests, node.rs, graph.rs (`derived_from`, `VIA_REPORT`, `VIA_BRIEF`), session.rs
  (`SessionRecord`, `TaskOf`, `TargetRef`), compiler.rs (a task's first compilation), tests_tasks.rs and
  tests_layouts.rs; the CLI's `theseus tasks`, the Discord renderer's report line, and the cockpit's task view.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **One store format number.** The basis on the check's session record (§2.14's "SESSION 3 → 4") and whatever a node
  carries for the check are one bump of `MANIFEST_FORMAT` from main's number as you cloned it, with the layout
  sample. Others bump it too; the maintainer renumbers at the merge.
- **The place rule replaced labels.** `check_of` resolves only among the tasks the calling session started (as a quote
  resolves only in its own transcript), and a check takes its parent's class.
- **`compile()` is pure** over the child's own nodes, so what the check renders (the checked task's pieces, its
  report as a claim) is carried by a node in the check's session, written in `open_task`'s frame, with `derived_from`
  edges to its sources.
- **What can leak.** A task's compilation reads no other session's nodes, so the exclusion is enforced on what reaches
  the check: its brief, and 27's quoted pieces.
- `task.create` has no `profile` today: a task runs on its parent's target. The Observatory is gone: the basis shows
  on `theseus tasks`, the report's post, and the cockpit's task view.

**What to build (28a):**
1. **`check_of: <task>`** on `task.create` (its id or the end of it), with `profile?`, the check's model (the parent's
   target without it). Say whether `profile` is open to every task or only to checks. A check of a task with no
   report (running, failed, or cancelled) is refused with the reason. Decide whether `check_of` meets 27's arrangement
   rule, the checked task's objective and acceptance pieces standing for the check's own; the default is that it does,
   and a check may add pieces of its own.
2. **The admission.** The check's first compilation renders its brief, the checked task's `objective` and
   `acceptance` pieces (from its `Arrangement` node), and its report as a claim ("claimed by task a1b2c3, as of
   14:02"), and no other node of the checked task's session. A piece of the check's own that resolves to the report's
   node renders as the claim; one whose node derives from any other node of that session is refused, naming the
   exclusion.
3. **The overlap flag.** A brief or piece sharing a span of 12 words or more with a node of the checked task's
   session, other than its report, is flagged; the check still runs, and each flag (with its span) is part of the
   basis. That session's brief and `Arrangement` node are the parent's own words, and its pieces are admitted, so the
   default leaves them out of the comparison; say what you chose, and how you count words.
4. **The basis**, on the check's session record: the checked task, the excluded sessions, the admitted pieces, the
   model and profile, and the overlap flags; a ledger row at the open (name yours). Shown as `🔍 check of task a1b2c3 ·
   independent (excluded ses_…, <model>)`, or with `· overlap: N spans`, beside the check's report (Discord, the CLI),
   on `theseus tasks`, and in the cockpit's task view.

**Proof, offline:** the design's 28a tests: a check's compilation holds no node of the checked task's session but its
report, rendered as a claim; a copied 12-word span raises the flag, and an 11-word one does not; the basis recorded
and shown; a check of a task with no report refused. Also: a session record written before your bump still reads;
27's refusal and fidelity tests, DD7's, and the task wakes' tests pass unchanged (name any you had to change).
Planted reverts: admit the checked task's last tool result beside its report, and show the exclusion test fail; make
the flag need 13 words, and show the 12-word test fail.

**The live check is the maintainer's.** Write it as exact commands on a scratch daemon with a fresh state dir and a
GLM profile:
1. In one session, ask for a task that counts the words in a scratch file and reports the number; wait for its report.
2. Ask for a check of that task (on a second configured profile, if the scratch config has one). `theseus rpc
   compilation.list '{"session_id": "<check session>"}'` shows the brief, the checked task's objective, and the claim,
   and nothing else of its session.
3. `theseus tasks` shows the 🔍 line with the excluded session and the model; the check's report carries it too.
4. A check of a still-running task is refused, with the reason in the call's result.

The overlap flag's live case needs a brief that copies the checked task's working, which its parent never sees by
itself: the offline tests carry it.

**Leave alone:** the `TASK` record (39a), being built beside you, which changes `task.create` too: keep your edits to
its `Input`, schema and description additive, and your code in a module of its own; 27's resolution and fidelity
rules (use them); the task's wakes and the report's wake; `tasks.parked` (28b); the compiler's recall and M6's
compaction roots (other changes); the judge; a `verify.v1` pack (filed).

**Update from the maintainer (before launch):** this step now starts after step 39a, the `TASK` record (three layers, CAS, the tools), has merged into `main`, so your clone has it. Build 28a's fields on that record and on `task.create` as 39a left them, not on the older shape this prompt may describe. Where the two differ, the code on `main` wins.
