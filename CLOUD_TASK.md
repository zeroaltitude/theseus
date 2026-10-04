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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-classify-role`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: `classify.v1` and `role.v1` at inbound, step 25a (theseus-0j2.6)

Branch: `cloud/20261004-classify-role`. Every commit's subject carries `theseus-0j2.6`. Deadline for the report: 4
hours after you start.

**Background.** Step 23a wired Jev, an external judging API, into the daemon in shadow (`[judge]`, `JudgeService` in
crates/theseus-core/src/judge/, a sink that records every judgment as a `judge.call` row, the shadow day budget, and
`loop.v1` at a turn's end). Step 25a asks two more packs about every message a person sends: `classify.v1` (CLASSIFY:
a new ask, a follow-up, a correction, a control word, a message for a live task, or small talk; and whether it should
become a task) and `role.v1` (ROLE_GUESS: the working role the reply needs). They share the `inbound` state, so they
ride one request, and both stay in shadow through M5: routing a message to a task is M7's, and the roles table and
role switches are step 26c's. The learning ledger (25c) will compare `classify.v1`'s `should_promote` with the
model's own `task.create` calls.

**Read first:**
- docs/design/m5-judgment.md: §2.2's batching row, §2.4's `classify.v1` and `role.v1` rows, §2.8c, §2.9's label
  sources, and §3's 25a entry; the spec's §3.4 (docs/spec/03-part1-s3.md: the role table's twelve seed rows);
- crates/theseus-judge: packs/classify.v1.toml and role.v1.toml (`options_from = "roles"`, `only_when`), builders.rs
  (`InboundInput`, `TaskInput`, `RoleInput`), batch.rs, fake.rs, and fixtures/inputs/inbound.json;
- crates/theseus-core (AGENTS.md first): turn.rs (`TurnRequest`: `input` is `None` for a continuation; where the
  input node is written), task.rs (a task's first turn is its brief), rpc/driver.rs (wakes' and reports' turns),
  places.rs, judge/ and tests_judge.rs.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- The place rule replaced labels and audiences: a place is private or shared. The state's place kind is the
  session's surface and that rule's answer. Voice is live (`[[channel]] voice = true`): spoken messages start turns
  as typed ones do.
- Without a roles table, `role.v1` asks nothing (its question is `only_when = "roles"`). The table, the session's
  role, and switching are 26c's. So that `role.v1` asks in shadow now, give the inbound state the twelve seed rows'
  ids and stances (spec §3.4) as compiled-in data in the judge module, with `current_role` none; 26c replaces them
  with its versioned table. Say so in the report.
- A plain turn writes 5 frames. The sink's own frames can land inside a turn's window: jev-wire-in's join found one
  inside the turn bench's measured turn (theseus-0j2.3, settled at that join; read how before you add judgments inside
  turns). A turn's own frames never change.
- The store has one format number: judgments are ledger rows, and need no bump.

**One convention, four sessions** (you, 23b's surfaces, and the gate and compile points, each dispatching judgments
inside turns; keep to these names so the merges stay mechanical). The core mints a judgment's id at its dispatch:
`theseus_judge::Ask` gains `pub id: Option<String>` (`Ask::new` sets `None`), and `Judgment::pending` uses it when set,
else mints one as now. Each dispatch marks the turn's trace with a zero-length span: name `judge`, kind `mark`,
attributes `pack` (as `classify.v1`), `point`, `mode`, and `judgment` (the id its row will carry). Put your point in a
module of its own under judge/ (a child module sees the service's private parts): `judge/mod.rs` gains a `mod` line
and its `WIRED` lines, and 23a's loop path stays as it is.

**What to build (25a):**
1. The inbound point: every turn whose input is a person's message (the CLI, the cockpit, a Discord message, a spoken
   one) dispatches one decision point with both packs, in shadow, spawned and marked, once its input node is written.
   Never a continuation (a wake's or a report's turn), a task's first turn, or a slash command.
2. The `inbound` state, built in the spawned task from the session's nodes: the message and its author, the place
   kind, the previous human message and the last reply (trimmed), the session's live tasks (id, the brief's first
   line, state), the minutes since the last message, and the roles; scrubbed and capped as 23a's state is.
3. Both judgments recorded through the sink: one call, one blob, the cost split by question count (theseus-judge's
   batching does the split), each judgment with its own id and mark.
4. A failing Jev changes nothing: down, slow, or rate-limited, the turn's request bytes and result are the ones the
   judge-off daemon makes, and nothing waits.

**Proof, offline**, against the fake Jev: §3's 25a tests: one request per inbound message (the fake counts one call);
two rows that share a blob, with the cost split by question count; no judgment for a slash command, a wake's turn, a
report's turn, or a task's first turn. Also: each fake mode (down, slow, 429, malformed) recorded by its error class
while the turn's request bytes and result stay unchanged; a judged turn keeps its frame budget; under load where a
test times anything. Planted reverts: send the two packs as two decision points, and show the one-call test fail;
judge a wake's turn, and show the exclusion test fail.

**The live check is the maintainer's**, with a real key under `[secrets] jev_api_key`. Write it as exact commands on
a scratch daemon with a fresh state directory, a GLM profile, and `[judge] enabled = true`: three messages in one
session, a new ask, a fragment ("and the tests too"), and "stop" typed as plain text; `theseus judge log` shows three
`classify.v1` judgments with their kinds and three `role.v1` judgments with the roles guessed, each pair from one
call.

**Leave alone:** the roles table, the session's role, and the role line (26c); places.rs and the bindings (another
change brings bindings format 2); Discord's slash commands (another change adds `/prompt`) and voice; the turn's
recall step (another change gives recall its node); 23b's surfaces; and the other judge points.
