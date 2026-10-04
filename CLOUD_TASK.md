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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-categorize-shadow`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: `categorize.v1` in shadow, and parked tasks in health, step 28b (theseus-vug.1)

Branch: `cloud/20261004-categorize-shadow`. Every commit's subject carries `theseus-vug.1`. Deadline for the report: 4
hours after you start.

**Background.** Two steps are on main that this one joins. The Jev wire-in (23a) put Jev's client in the core:
`[judge]`, `JudgeService`, the sink's `judge.call` rows scoped `judge:<pack>`, the shadow day budget, and `loop.v1`
in shadow at a turn's end. The ontology (21b) put topics in the core: categories, guidance, and each session's
memberships (crates/theseus-core/src/ontology.rs, rpc/ontology.rs). Step 28b asks Jev which declared topic a
conversation belongs to (`categorize.v1`, its pack and builder already in crates/theseus-judge), shows the operator
its proposals, and records the operator's answer as the judgment's label; Jev writes no membership. It also adds
the parked-task invariant to health.

**Read first:**
- docs/design/m5-judgment.md: §2.4's `categorize.v1` row, §2.5 (`judge.label`'s shape), §2.12, §2.13's health and
  CLI lines, §2.16, and 28b's entry under "Each step's tests and live check";
- crates/theseus-judge: packs/categorize.v1.toml (its header says why the state leaves memberships out), builders.rs
  (`CategorizeInput`, `categorize`), pack.rs, fake.rs;
- crates/theseus-core: judge/ (`JudgeService`, `WIRED`, the spawned judgment at a turn's end, the sink), ontology.rs
  (the snapshot, `given`, the judged writes), rpc/ontology.rs, rpc/confirms.rs (`judge_act`), places.rs, task.rs
  (`parks_on_wake`), and how health is assembled; crates/theseus-kernel's `Wake` and execution states.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **The place rule replaced labels.** A shared place's compile walk reads no interpreted membership (21b), so
  categorize only conversations in a private place, and skip tasks (a task's one human message is its brief).
- **The cockpit replaced the Observatory.** The proposals' surfaces here are the protocol and the CLI; the cockpit's
  Ontology view (21c) is being built beside you. Say in the report what its proposals panel should show.
- **The learning ledger (25c) is not built.** Write each answer as a `judge.label` row in §2.5's shape (`lbl_<id>`,
  scope `judge:categorize`, the judgment, the question, the label, source `operator`, who and through what), so 25c
  can take labels over without a new shape.
- **The store has one format number.** Ledger rows need no bump; a new stored field does.

**One convention, six sessions** (you, 23b's surfaces, 24's security pack, 25a's inbound packs, 25b's continue pack and the other memory or category point, each dispatching judgments; keep to these names so the merges stay mechanical). The core mints a judgment's id at its dispatch: `theseus_judge::Ask` gains `pub id: Option<String>` (`Ask::new` sets `None`), and `Judgment::pending` uses it when set, else mints one as now. A dispatch inside a turn marks the turn's trace with a zero-length span: name `judge`, kind `mark`, attributes `pack` (as the pack you call), `point`, `mode`, and `judgment` (the id its row will carry); a dispatch outside a turn needs no mark. Put your point in a module of its own under judge/ (a child module sees the service's private parts): `judge/mod.rs` gains a `mod` line and its `WIRED` lines, and 23a's loop path stays as it is. If another session's identical change is already on main when you clone, use it as it is.

**What to build (28b):**
1. The `exchange_end` trigger, decided off the turn's path as `loop.v1`'s state is built: at the end of a
   conversation's turn that ended with no tool calls, judge when 10 human messages have arrived since the session's
   last `categorize.v1` judgment, or when this exchange began after 30 minutes' quiet and something new arrived. Say
   in the report how you read "the first exchange end after 30 minutes' quiet".
2. Its input: the session's title and its last ten human messages; up to 50 candidate topics with their
   descriptions, from the ontology's snapshot in memory (with more than 50, say how you chose); up to 5 interpreted
   memberships for the `still_member` Nouls. `WIRED` gains the pack in shadow.
3. Proposals: each answered judgment whose `topic` names a topic the session is not in, or `new_topic`, with its
   confidence and band, newest first and unlabelled. A read method for them, and acting methods (names yours) to
   accept one (the operator-origin membership through the ontology's write path, and its label, in one frame; for
   `new_topic`, the operator names the topic) or reject it (its label). Both go through `judge_act`, and the CLI
   refuses them inside a job. `theseus ontology proposals`, `accept` and `reject`.
4. Health's `tasks.parked`: each task in progress that cannot progress by itself (no running turn, queue place, job,
   wake, or pending question younger than 24 h), with its blocker named, read from the open executions only, never a
   history scan; and its line in `theseus health`. A task waiting on its own pending wake (37b) can progress, so it is
   not listed.

**Proof, offline**, against the fake Jev: the design's 28b tests (the `exchange_end` trigger; candidates from the
kinds table; accept and reject write labels and an operator membership; parked detection on scripted states). Also: a
shared place's session and a task are never judged; a slow or failing Jev changes no turn's bytes, duration or frame
count; a refused accept writes nothing. Run them 5 times under load. Planted reverts: count every message instead of
human ones, and show the trigger test fail; let accept skip `judge_act`, and show the refusal test fail.

**The live check is the maintainer's**, with Jev's real API key (`[secrets] jev_api_key`) and a model key. Write it in
the report as exact commands on a scratch daemon with a fresh state dir, Discord and the web off, and `[judge] enabled
= true`:
1. `theseus ontology topic add harbor --desc "…tides, moorings, the harbour master"` and a second, unrelated topic.
2. Ten messages about moorings in one CLI session: `theseus judge log` shows a `categorize.v1 (shadow)` judgment, and
   `theseus ontology proposals` lists `harbor` with its confidence.
3. Accept it: `theseus ontology member <session>` lists `topic:harbor`, origin `operator`, and the ledger holds its
   `judge.label` row.
4. With 50 topics declared, one judgment's output tokens against its reservation (an open question in theseus-judge's
   price.rs: a 52-option Choice's allowance is an extrapolation).
5. After `theseus policy tighten proc.run`, a task whose `proc.run` waits for approval: `theseus health` does not
   list it as parked, since its question is younger than 24 h; the offline tests carry the 24-hour case.

**Leave alone:** the other packs being wired tonight (23b's surfaces and `judge.list`/`get`, 24's `security.v1`, 25a's
inbound packs, 25b's compile signals, 27's arrangement on `task.create`): add one `WIRED` line and your own decision
point, not a reshaped service; the cockpit (21c); the compile walk and the ontology's write rules (use them as they
are); the judge sink's frame (a fix joins with it); crates/theseus-core/src/external.rs.
