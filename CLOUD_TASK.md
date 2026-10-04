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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-extend-propose`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: a proposed extension, frozen, started in L1, tested, and put to the operator, step 43a (theseus-ext.5)

Branch: `cloud/20261004-extend-propose`. Every commit's subject carries `theseus-ext.5`. Deadline for the report: 5
hours after you start.

**Background.** Self-extension (M7 step 43) lets the model add a tool as a small MCP server of its own: "planks, never
the keel". Step 36b (merged) built the MCP board (crates/theseus-core/src/mcp/: servers from `[mcp.servers.<name>]`,
started after serving, their tools offered in private places). But it runs every server at L0, and refuses
`sandbox = "l1"` with a message naming a follow-up: L1's spawn path served jobs that run to an end, not a long-lived
child with live pipes. An agent-written server never runs at L0, so 43a starts with that follow-up. 43a proposes,
freezes, starts, and tests an extension, and puts it to the operator. Loading it on the ack, its restart, and its
revoke are 43b.

**Read first:**
- docs/design/m7-surface.md §2.7, its 43a entry under "Each step's tests and live check", and §5's questions 21 and
  22;
- crates/theseus-sandbox's AGENTS.md and spawn.rs (`spawn(spec, init, Stdio)`, whose `Stdio` takes a stdin);
  theseus-kernel's job_l1.rs, job_egress.rs, and children.rs;
- crates/theseus-core: mcp/ and config/mcp.rs; rpc/confirms.rs (`judge_act`, `Act`) and the budget question, a
  question the operator answers through the same card; places.rs;
- crates/theseus/src/client.rs (`OPERATORS`, `refuse_in_a_job`).

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **Who may ack.** `judge_act` no longer traces the asking process: an act counts when it is the owner's from a
  private place (the place rule), and the CLI refuses the operator's methods in a job's shell (`THESEUS_SESSION`).
  "A job's process can't ack" means both refusals; L1, whose view has no route to the daemon, is the boundary.
- **The manifest.** The design makes it a node (`kind: extension`, `trust: agent`, `derived_from` the call), but no
  such node body or `trust` field exists. Unless a node is clearly right, keep the manifest as a META record (as
  36b's `mcp.tools.<server>` and the AWS hands' group record are), shown in the proposing call's result, so no record
  kind changes. Say which you chose, and why.
- **One store format number,** `MANIFEST_FORMAT`, 7 on main. A new stored field, value, or record kind bumps it, with
  its layout sample in tests_layouts.rs.
- **Credentials are granted at a job's launch.** An extension gets no secret in this step.
- **The cockpit replaced the web UI.** Leave it, and say in the report what its view of a proposal should show (the
  frozen files, a diff against the version it would replace, the tests).
- **This VM runs as root, and L1 refuses a root operator's job** (theseus-pv6i), so here every L1 start is refused.
  An earlier session on a VM like this one ran the sandbox's whole contract as uid 65534, and it passed. Run your L1 tests as
  that uid (for example `setpriv --reuid=65534 --regid=65534 --clear-groups`, with the binaries and a scratch dir
  that uid can reach), and say exactly how in the report.

**What to build (43a):**
1. **MCP servers in L1.** A stdio server with `sandbox = "l1"` starts in L1 through the sandbox's spawn:
   - its stdin and stdout are live pipes to the board, and its stderr goes to its log;
   - no network unless asked, through the egress proxy's list as a job's is;
   - its view is the workspace's, and a stop ends its whole namespace.

   One shape is a role of the daemon's own binary, as `job-wrapper` and `hand` are, spawned through
   `children::spawn` and holding the init, so that the board sees an ordinary stdio child. Lift 36b's refusal, and
   keep `l0` the default for configured servers.
2. **`extend.propose { name, dir, command, description, tests?, network? }`,** a harness tool. Class `Run`, posture
   `notify` (in the template's `[policy.tools]`, which a test counts), waiting under T1's hold like any `Run` call. `dir` must be inside the workspace roots.
3. **Freeze.** Copy `dir` into `<state>/extensions/<name>/<digest>/`, the digest a SHA-256 over the tree (sorted
   paths, modes, and contents), so a later edit in the workspace changes nothing that runs.
4. **Start and test.** The board starts the frozen copy in L1, in a `proposed` state whose tools no turn is offered.
   Then `initialize`, `tools/list`, and each declared test (`{ tool, arguments, expect: contains | equals }`) as a
   `tools/call`. Then stop it.
5. **The manifest:** the name, the digest, the command, the tools (names, schemas, descriptions), the tests and their
   results, the capabilities asked for, and the proposing session and its principal.
6. **The ack:** an approval card in a private place, answered as other questions are (Discord's buttons,
   `theseus confirm`, the cockpit's card): "Load wordcount 3f2a1c: 1 tool, 3 of 3 tests passed, no network?", with
   Load and Decline. An ack writes `extend.acked`, and a decline `extend.declined`. Nothing loads in this step.
7. **Rows and surfaces:** `extend.proposed`, `extend.tested`, `extend.acked`, and `extend.declined`, each a fact with
   its narrative line; `theseus extend list`; health's count of proposals. If time runs short, leave the CLI and
   health's count, and say so.

**Proof, offline,** with a tiny stdio server in a test fixture, or theseus-sim's `fake-mcp`, as the proposed server.
The design's 43a tests:
- the freeze's digest is stable, and an edit after proposing changes nothing that runs;
- the tests' passes and failures are recorded;
- no tool is offered before the ack, nor after it in this step;
- an ack from a job's shell, and one from a shared place, are refused;
- a decline loads nothing.

Also an MCP server in L1: it starts and answers, sees no network, and its process ends with the daemon's stop and
with the daemon's `kill -9`. Planted reverts: offer a proposed server's tools, and show the test fail; start the
server from the workspace's dir instead of the frozen copy, and show the test fail.

**The live check is the maintainer's,** on the owner's machine, since L1 refuses a root daemon. Write it in the report
as exact commands, on a scratch daemon with a fresh state dir and the fake Discord:
- GLM writes a tiny stdio MCP server (a word counter) in the scratch workspace and proposes it;
- the card shows at the fake Discord, and `theseus confirm` acks it;
- the ledger shows `extend.proposed`, `extend.tested`, and `extend.acked`, and no `mcp__ext-` tool is offered.

It costs only the model's tokens.

**Leave alone:**
- 43b (loading, revoke, `/extensions`);
- 36c's prompts: another session adds them to the same board, so keep your edits to mcp/mod.rs small and your code
  in modules of its own;
- the MCP server side (41b), and the terminal tools (`term.*`);
- `security.v1` at the gate, and bindings format 2's places: other sessions change them;
- the cockpit.
