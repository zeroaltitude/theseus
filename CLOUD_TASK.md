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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-memory-arm`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the memory exam over the real recall pipeline, one daemon per arm, step 34b's wire-in (theseus-6fn.5)

Branch: `cloud/20261004-memory-arm`. Every commit's subject carries `theseus-6fn.5`. Deadline for the report: 5 hours
after you start.

**Background.** crates/theseus-exam is M6's memory exam (34a, then exam-v2), merged ahead of its reader: its
Cargo.toml says `reserved_for = "row 55 (34b's wire-in), M6: the exam's harness over the real pipeline, through the
scratch daemon's [memory] arm config key"`. It has items whose success needs something from an earlier session, a
fixture writer (`write-store`), a check language, paired statistics, a driver (`run`) over a scratch daemon's socket
with the arms `none` and `oracle`, the headroom report, and a probe of a running index tender. Row 55 points it at the
real pipeline: each arm is a scratch daemon whose config names its `[memory] arm`, so the exam measures recall
itself, and its report is M6's first honest one. 30b, merged before you start, gave `[memory]` its canary and live
modes and `arm`.

**Read first:**
- docs/design/m6-memory.md §2.9 (the arms, the exam, the replay, the plan, the decision rule, what the numbers can
  say, the report), §2.14's struck row, and 34b's rows in §3.1 and §3.2, struck words included: they say what was cut;
- crates/theseus-exam: lib.rs, drive.rs, fixture.rs, render.rs, report.rs, stats.rs, tender.rs, main.rs, and
  tests/daemon_reads.rs; crates/theseus-core: config/memory.rs, recall.rs, turn/recall_step.rs, and tender.rs.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **The arm is config, never a turn's field** (the cut-list's 6.2, Part III Item 80): `turn.submit` carries no arm,
  and `allow_arm_override` was never built. One daemon per arm, each on its own copy of the exam's store.
- **30b's names.** Its task named `[memory] mode` (`canary`, `live`), `arm` (`none`, `baseline`), `experiment`, the
  `Recall` node and its render, and the `memory.arm` row. Its code on main is the truth where it differs.
- **Vectors are on main** (29c). A tender without its model files answers BM25 and entities alone. This VM has none,
  so `baseline` equals `bm25` here; the maintainer's run has them.
- **No canary data exists yet.** By the rule's third clause every feature's decision reads "insufficient", and the
  report says so, with its n.
- **The cockpit replaced the Observatory.** The report is a file the exam writes, and the CLI's.

**What to build,** each a green commit:
1. `[memory] arm` gains `bm25` (the tender's `bm25` and `entity` sources, and no vectors), beside `none` and
   `baseline`. A daemon runs the arm its config names, and the recall row names the arm and its sources.
2. **The driver over real arms**, as one command for the maintainer. `none`, `bm25` and `baseline` are each a scratch
   daemon in `live` mode. `oracle` stays the driver's note, sent to the `none` daemon: its bytes equal 30b's render
   of the same items, and it sits where 30b's note sits, after the task's text. The command copies the exam's store
   per arm, writes each config from a base config (`[memory]` set; Discord and the web UI off), starts each daemon,
   and waits until its tender has indexed the whole store (and embedded it, when it has the model). Then it runs
   items × arms × runs interleaved, as `run` does today (pairing, the spend cap, resume), and stops every daemon it
   started.
3. **The report over four arms**: each arm's pass rate with its interval and n; paired differences, clustered by item,
   for `baseline − none`, `bm25 − none`, `oracle − none`, `baseline − bm25` and `oracle − baseline`; cost per pass; the
   held-out half apart; the decision per feature, with the clause that decided it; and what could not be measured,
   and why. It is a frozen file that names the plan's digest.
4. `docs/m6-ablation-plan.md`, as §2.9 lists it: the arms and their versions (digests), the primary metrics, the unit,
   the minimum samples, the decision rule, and the analysis.
5. **The reader rule:** theseus-exam's `reserved_for` becomes its `tool` marker. Keep the crate and its binary (lib.rs's
   old note says the driver moves into theseus-sim here: update it). Give the crate the AGENTS.md every crate has,
   and name it in the root AGENTS.md's map.
6. **The replay, if time allows**, over a copy of a store: every arm recomputed over the recorded turns' `recall.shadow`
   rows with `as_of`; the silver labels (re-supply, reference, re-derivation, should-have); recall and precision at k,
   MRR, and the stale rate. Otherwise report it as left.

**Proof, offline:**
- A daemon runs the arm its config names: `none` asks the index nothing, `bm25` asks for BM25 and entities alone, and
  `baseline` for the fused sources.
- End to end, through real daemons on a stand-in model that answers from a recall note when its request has one: a
  small exam through `none`, `bm25` and `baseline`, where `baseline` passes an item `none` fails, and no daemon is
  left running afterwards.
- The oracle note equals the core's render, byte for byte.
- The report's tables against hand-computed fixtures, with n, and "insufficient" where an interval can't decide.
- If the replay is built: its leakage test (a node written after a turn never appears in that turn's recall).
- Planted reverts: give every daemon the same arm, and show the arms test fail; render the oracle note the old way,
  and show the render test fail.

**The live check is the maintainer's**, with a GLM key and the tender's model files under `[index] weights_dir`, and
spend capped by the driver's limit (about $5: the design puts a headroom run at a few dollars). Write it in the report
as exact commands: write the exam's store; the four-arm run over the held-in half, 3 runs each; the report; then the
held-out half, once. Say what each should show: four arms' rows and their n, where `bm25` and `baseline` fall between
`none` and `oracle` (or what the run found instead), the spend, and no daemon left running.

**Leave alone:**
- 30c (compaction) and 31a (the memory pass), built beside you in recall.rs, config/memory.rs and theseus-memory. 31a
  versions `baseline`, so your report names whatever digest the daemon runs. Keep your core change to the arm's value
  and the sources it asks for.
- 32c's `+rerank` arm, beside yours in the same key: keep both at the merge.
- The index tender's crate, except through its protocol; the cockpit.
