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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-hands-cancel`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the hands' cancels, budgets, and watching, step 40 part 2 (theseus-mgw.11)

Branch: `cloud/20261004-hands-cancel`. Every commit's subject carries `theseus-mgw.11`. Deadline for the report: 5
hours after you start.

**Background.** Hands are jobs that run in AWS (the AWS design §3.3). Part 1 (merged) built `aws.hands.run` on Lambda
and Fargate:
- the `theseusd hand` role inside the hand image, and its signed completion envelope;
- a group as a META record (`aws.hands.group.<group>`) plus one kernel action per hand (tool `aws.hand`);
- the SQS poller, which settles each hand through `Kernel::accept_completion`.

It left running hands to their TTL, capped a group only by `max_usd` at launch, and drew no surface. Part 2 finishes
step 40.

**Read first:**
- docs/design/aws-toolset.md §3.3 (TTL, budget per hand, watching a hundred hands), §3.7, §3.10, and §5's "Step
  40"; the spec's §0 row on the home AWS account (docs/spec/02-part1-s0.md: the budget's cascade);
- crates/theseus-core: aws/hands/ (mod.rs, launch.rs, group.rs, poller.rs, tool.rs, tests_hands.rs) and
  toolrun/hands.rs;
- crates/theseus-kernel: cancels.rs, `CancelState` and `VerifiedBy`, the reconciler, and how an action reserves
  (`reserve_micros`) and settles;
- infra/aws/theseus-hands.yaml (the TTL reaper's Lambda) and theseus-foundation.yaml (the month's budget); the
  cockpit's Systems view (SystemsCards.tsx's AWS card).

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **The role is `theseusd hand`,** and a group is a META record plus kernel actions, not a new record kind.
- **The budget's cascade** (spec §0). The month is built: an AWS Budget whose stop at 100 % attaches
  `theseus-deny-spend`. The day and the hour come with this part:
  - the day is a second AWS Budget that only alerts;
  - the hour is metered by Theseus, since AWS's billing lags by hours: each AWS action's estimated cost is reserved
    before it runs, and past the hour's line Theseus alerts. Alert only: whether it should also refuse is the owner's
    open question.
- **The AWS-side TTL reaper exists:** the hands stack's Lambda, every 15 minutes, acting only on tasks tagged
  `theseus:owner`. Theseus has yet to read what it did.
- **The hands' network is moving** to a network the account already has, so that Theseus never runs a NAT gateway of
  its own (another change). Leave the network stack and launch.rs's network discovery alone.
- **The cockpit replaced the Observatory.** One store format number, 7 on main: a new value in a stored enum (a
  `VerifiedBy` for a stopped task, say) is a layout older builds can't read, so it bumps the number with its sample.

**What to build (40 part 2),** in this order:
1. **Cancellation per backend.** Fargate: `StopTask`, verified when `DescribeTasks` shows STOPPED. Lambda:
   `cancel_unsupported`, its timeout the bound. A group whose `until` is met cancels its running hands, and a cancel
   or `/stop` of the call cancels the group. A late envelope after a cancel is recorded as late, never settled twice.
2. **Reservations.** Each hand's action reserves its worst case (TTL × the rate for its size) against the session's
   dollar budget, so that a group meets the session's limit and its question as a model loop does, and settles at its
   real cost. `max_usd` stays the group's cap.
3. **The hour and the day.**
   - Theseus's hourly meter over AWS actions' reserved and settled costs, a key under `[aws.accounts.<id>]` with a
     default of $1 an hour, alerting once an hour past it: a row, a notice, and health.
   - A daily budget in the foundation template (`DailyBudgetUsd`) that only alerts, reconciled from the config as
     the month's is.
4. **Overdue and reaped hands.** The reconciler asks ECS about an overdue Fargate hand (`DescribeTasks`) before calling
   it unknown. A hand the reaper stopped settles failed, with the reaper's reason. The reaper's failure records on the
   queue are read and surfaced, not left to the dead-letter queue.
5. **Quotas.** Before a launch, read the Fargate vCPU quota (Service Quotas) and Lambda's concurrency (its account
   settings), cached. A group bigger than the room launches in waves, and never fails for a quota.
6. **Watching.**
   - health's hands line: running by backend, the oldest, and the spend reserved;
   - Discord's one line per group, edited in place ("🖐️ 37/100 done, 2 failed, $1.84 of $5"), never a line per
     hand;
   - a protocol read of the groups, and the cockpit's grid: each group's cells by state, and its cost against its cap.

   If time runs short, the grid is the one to leave: say so.

**Proof, offline,** against part 1's stateful fake of AWS:
- the cancel lifecycle per backend, and `until` met stopping the rest;
- reservations reaching the session's limit and its question, and settling at real cost;
- the hour's alert firing once;
- an overdue task resolved by `DescribeTasks`, a reaped hand's reason, and waves for a quota.

**The kill -9 prove:** a real `theseusd` against the fake endpoint (the rig in crates/theseusd/tests/aws.rs),
`kill -9` mid-group, a restart, and every hand and the group settled exactly once. Run the hands tests under load.
Planted reverts: skip `StopTask` when `until` is met, and show its test fail; drop a hand's reservation, and show the
budget test fail.

**The live check is the maintainer's; it spends real money, and waits for the owner's go.** Write it in the report as
exact commands, on a scratch daemon with the account bound and the hands stacks applied:
- a twenty-hand Lambda group with `until: first_success` and `max_usd: 1` (about $0.40 reserved at worst, likely
  under two cents spent): its running hands show `cancel_unsupported`, and the group settles;
- once Fargate hands run on the account's existing network (the other change; never a NAT gateway of Theseus's
  own), a Fargate hand cancelled mid-run shows `StopTask`, then STOPPED (a few cents);
- the hour's alert, with its line lowered to a cent;
- the daily budget's change set shown, and applied only with the owner's go (AWS Budgets may charge for a budget past
  an account's first two: check its price first, since the stacks' own cost stays under a dollar a month);
- `kill -9` of the daemon mid-group, a restart, and each result settled once.

Name the account's config table, never its id.

**Leave alone:**
- the hands' network (another change);
- step 16's restore: another session works in crates/theseus-core/src/aws/;
- the durability tender;
- `security.v1` at the gate, and the cockpit's Judgment and Ontology views: other sessions change them.
