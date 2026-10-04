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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-lsp-board`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the language-server board and its tools, step L2 (theseus-n88g.8)

Branch: `cloud/20261004-lsp-board`. Every commit's subject carries `theseus-n88g.8`. Deadline for the report: 5 hours
after you start.

**Background.** crates/theseus-lsp (lane L1, merged ahead of its reader) is a hand-written LSP client:
- framing, routing, document sync, diagnostics pushed and pulled, navigation, and a rename's `WorkspaceEdit` (read,
  never applied);
- the stop: `shutdown`, `exit`, then a kill after each preset's grace;
- the position helper, `locate(text, line, symbol, occurrence)`, which gives a UTF-16 column;
- presets for six servers (ty, pyright, basedpyright, TypeScript 7, typescript-language-server, rust-analyzer), and
  a scripted fake server, `theseus-lsp-fake`.

The caller spawns: `Client::start(Server { reader, writer, kill, pid }, Options)`. The crate's Cargo.toml reserves it
for this step (`reserved_for = "row 0 (L2, theseus-n88g.8), …"`). L2 wires it into the core: a board of servers, and
tools the model calls. L3, diagnostics in edit results, comes after.

**Read first:**
- crates/theseus-lsp: AGENTS.md, lib.rs, client.rs, servers.rs, position.rs, fake.rs, and tests/fake.rs;
- crates/theseus-core: the `Tool` trait and the tool catalog; the gate (postures, `Decision::at_least`); places.rs
  (`offered`, `files_tool`); the job environment (`[tools] proc_env`); aws/stack.rs (a write that executes exactly
  what an earlier read showed, by its digest); how health and facts are assembled; and theseus-kernel's children.rs.

**The design** (approved by the owner; it is not in docs/design):
- **Lazy, never on the start path.** One server per (server, workspace root), started by the first call for a file of
  its language. The root is the nearest marker at or above the file, inside the roots: a `Cargo.toml` with
  `[workspace]` (or the topmost), `tsconfig.json`, `jsconfig.json`, `package.json`, `pyproject.toml`, `setup.py`, or
  `requirements.txt`.
- **Spawned through `children::spawn(Kind::Owned)`** in its own process group, with the job environment, and its
  stderr in a capped `<state>/lsp/<server>-<root hash>.log`.
- **The start is a `Run` act.** A language server runs build scripts and proc macros, as `cargo build` would. So the
  first start per root is judged at `proc.run`'s posture for the server's argv (notify under the template), and
  recorded: `lsp.started`, `lsp.ready` (with its time to ready), `lsp.stopped`, `lsp.failed`.
- **The stop.** A server stops when idle for `[lsp] idle_stop_mins` (10), at the daemon's stop (SIGTERM to its group,
  never waited for), or after no answer within `request_timeout_secs` (30); the next use starts it again. A call's
  cancel sends `$/cancelRequest` and answers cancelled, and `/stop` does that for the session's requests in flight.
  The server stays up for the next call.
- **The tools,** addressed by `path`, a 1-based `line`, and the `symbol`'s text (with `occurrence` when it appears
  twice), never a column:
  - `Read`: `lsp.definition` (the location, with a few lines around it), `lsp.references` (grouped by file,
    capped), `lsp.hover`, `lsp.symbols` (`{path}` for an outline, `{query}` for the workspace), and
    `lsp.diagnostics {path?}`;
  - `Write` and `NonRepeatable`: `lsp.rename {path, line, symbol, new_name}`. The gate judges its edit as an
    `fs.patch` of every file it writes (a file outside the roots, or on the approve list, waits); then it applies the
    edit and returns the diff.
- **Config:** `[lsp]` with `enabled` (off by default), `idle_stop_mins`, and `request_timeout_secs`; the built-in
  presets; `[lsp.servers.<name>]` overriding `command`, `extensions`, `roots`, and `settings`. Each key has its
  default and its template entry, and each tool its `[policy.tools]` line (a test counts them).
- **Seen in:** health's `lsp[]` (server, root, pid, state, time to ready, memory) and its line; an `lsp.request` span
  with `lsp.server` and `lsp.method`; the metric `theseus.lsp.request.duration`.

**What changed, and what the client's run found:**
- **The place rule:** a place is private or shared. A language server reads the whole project, not only a shared
  place's public paths. So keep `lsp.*` out of `places::files_tool`: the tools stay private-only, as `offered` already
  makes them. Test it.
- **A plan is synchronous,** and a rename's edit is known only once the server answers. Follow aws/stack.rs (a read
  that returns the edit and keeps it by its digest, then a write that applies exactly that edit, its files the
  plan's resources), or give the plan an async step. Say which you chose in the report.
- **The servers:** ty registers pull diagnostics only after `initialized`; TypeScript renames an imported name at its
  import site, so locate the definition first or say so; rust-analyzer took 4.2 GB and 18 s to quiescence on this
  repository, which is why the start is lazy and the idle stop matters. The client's events channel is unbounded:
  drain it.
- **No gauge kind** exists in the telemetry encoder, so the count of servers up is health's, not a metric.
- **The reader rule:** `reserved_for` goes once the core reads the crate.

**What to build,** each a green commit:
1. `[lsp]` and the board: lazy start, roots, the idle stop, the restart after a failure, the facts, health.
2. The read tools.
3. The start's posture.
4. The rename.
5. Cancel and `/stop`, the span, and the metric.

If time runs out, leave the rename, and report it.

**Proof, offline,** with `theseus-lsp-fake`:
- a definition, references, hover, and symbols through the tools;
- no server at the daemon's start or before the first call, and one server per root;
- the idle stop (tokio's paused clock), and the start again on the next call;
- the first start notified at `proc.run`'s posture, and not again for that root;
- a shared place offered no `lsp.*` tool;
- a rename whose edit touches a file on the approve list waits;
- a cancel sends `$/cancelRequest`, and after the daemon's `kill -9` no server is left.

Planted reverts: start servers at the daemon's start, and show a test fail; skip the gate on the rename's files, and
show its test fail.

**The live check is the maintainer's.** Write it in the report as exact commands, on a scratch daemon with a fresh
state dir, `[lsp] enabled = true` and `idle_stop_mins = 1`, over a small Rust or Python project with its server
installed:
- ask the model where a function is defined and who calls it;
- health's lsp line shows the server and its time to ready;
- a rename returns its diff, its notice names every file it wrote, and a minute idle stops the server.

It costs only the model's tokens.

**Leave alone:**
- L3: diagnostics in `fs.write`, `fs.edit`, and `fs.patch` results;
- the MCP board and its catalog: another change is joining them, and the `Tool` trait's names become `&str` there;
- the terminal tools (`term.*`), and `security.v1` at the gate: other sessions change them;
- the cockpit.
