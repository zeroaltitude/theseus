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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-learning-ledger`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the learning ledger: labels, the nightly report, and frozen holdouts, step 25c (theseus-0j2.9)

Branch: `cloud/20261004-learning-ledger`. Every commit's subject carries `theseus-0j2.9`. Deadline for the report: 5 hours
after you start.

**Background.** Steps 23a to 25b put Jev, an external judging API, in the daemon in shadow: `[judge]`, `JudgeService`
(crates/theseus-core/src/judge/), the sink's `judge.call` rows (keyed `jdg_…`, scoped `judge:<pack id>`, with every
answer's probabilities and band, cost, latency and context), and packs judged at their points: `loop.v1` at a turn's
end, `security.v1` and `security.v3` at the gate, `classify.v1` and `role.v1` at inbound, `continue.v1` at compile.
Nothing says yet whether a judgment was right. Step 25c is the learning ledger (spec §3.10): labels from people and the
system, a nightly report per pack and question computed by crates/theseus-judge's `learn.rs`, and frozen holdouts.
The ladder (26a) cites its reports, and 25d's replay builds on it.

**Read first:**
- docs/design/m5-judgment.md: §2.5, §2.9 whole, §2.13, §2.15, §2.16, and 25c's entry under "Each step's tests and
  live check"; the spec's §3.10 (docs/spec/04-part1-s3.10.md);
- crates/theseus-judge: learn.rs (`calibration`, `precision_recall`, `percentile`, `Window`, `holdout_split`,
  `sufficient`), judge.rs (`Judgment`), packs/ (which questions decide, which classes act);
- crates/theseus-core (AGENTS.md first): judge/, fact/judge.rs, rpc/confirms.rs (`judge_act`, `Act`), tighten.rs,
  rpc/mod.rs's `check_store_history` (the `store-verify` thread's 5% duty) and `sweep_spool_after_serving`;
  crates/theseus/src/client.rs (`OPERATORS`, `refuse_in_a_job`) and render/judge.rs.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **Your parents' names**, as their prompts gave them (the code on main is the truth wherever it differs): 24 writes a
  `judge.label` row (`lbl_…`, scoped `judge:security`, source operator, weight 1.0) for each judgment of a call the
  operator presses "should have asked" on, and each security judgment names the call's correlation id, posture, and
  whether the hold made it wait. 28b, if on main, writes `judge.label` rows for accepted and rejected proposals. Keep
  one `judge.label` kind and shape, and read their rows as they are.
- **J1's process trace is gone.** `judge_act` counts an act when it is the owner's from a private place, and the CLI
  refuses an operator's method in a job's shell (`THESEUS_SESSION`): "refused from a job's process" means both.
- **The web UI is the cockpit.** If 23b's Judgment section is on main when you clone, add label buttons to its
  judgment view and the report page beside it; if not, leave cockpit/ and say what both should show.
- **Not built yet:** nudges, canaries and arms (26a, 26b), and audit labels (25d). Leave out the report's canary part
  and the nudge-based system labels; read `source: audit` labels where they exist. 25a judges no slash command, so
  §2.9's `control` label from one has nothing to label: leave it out.
- Labels and reports are ledger rows: no `MANIFEST_FORMAT` bump. A new `[judge]` key needs a default and a template
  line (the config note is sparse).

**What to build (25c):**
1. **Labels everywhere.** An acting `judge.label { judgment, question?, label, note? }`, judged by `judge_act` (an
   `Act` of its own) and in `OPERATORS`; `theseus judge label <id> …`. A row, never an edit: `lbl_<id>`, scoped as its
   judgment, with the question (or all), the label, its source (operator, system, audit), who and through what, a
   weight, a note. Say what a label holds for a Choice, a Noul and a Score.
2. **System labels** (§2.9's table), derived deterministically by the report's run, source system, weight 0.5, keyed
   by judgment, question and rule so a second run writes nothing twice. `loop.v1`: a continuation phrase ("continue",
   "go on", "keep going", "you didn't finish") as the next human message within 10 minutes is "stopped too early"; a
   near-identical task brief within 24 hours is "false completion"; `budget_exhausted` is never "should have
   stopped". `security.*`: a declined waiting call is risky, an approval without complaint is not (both weak).
   `classify.v1`: the model's own `task.create` in that turn (`should_promote`). Say how you read each.
3. **The report**, per pack, version and question: calls, labeled calls, precision and recall per Choice class, Brier,
   ECE and the reliability table for Nouls and top choices, agreement with the baseline (define it per pack; 23b's
   `theseus.judge.disagreements` rule if on main), the share in each band, cost, and latency p50/p95/p99 per workload
   class, every number from `learn.rs`. **Holdouts:** a closed window (the latest 14 days by default) frozen into the
   report with its bounds; below the minimum (200 labeled per deciding question, 30 per acting class) it says
   "insufficient" with its counts. Written as `judge.report` rows and `<state dir>/learning/<date>.json` (derived;
   the rows rebuild it). A read, `learning.report { pack?, date? }`; `theseus judge report` runs it now and prints
   it, `--date` reads a stored one.
4. **The tender** (FAST): after serving, never within 10 minutes of a start, at `[judge] learning_hour` local time (3
   by default; a missed night runs once, 10 minutes after the next start), on a thread of its own at low priority, at
   most 5% of a core, holding the core weakly. It reads the `judge:<pack id>` scopes, never the whole history, and
   nothing runs with `[judge] enabled = false`.
5. The cockpit's report page and label buttons, if 23b's section is on main.

Not in this step: the learning channel's digest (`[judge] learning_channel`).

**Proof, offline**, on seeded stores: §3's 25c tests: a label through the protocol and the CLI, refused from a job's
shell and from a shared place; each system label from a scripted history, and a second run writing nothing; the
tender never within 10 minutes of a start, at low priority (on tokio's paused clock); the report's numbers on a
synthetic store equal `learn.rs`'s on the same pairs; a holdout frozen into the report, unchanged by later judgments.
Planted reverts: let a second run write its system labels again, and show the idempotence test fail; let the holdout
take a judgment after its window, and show the frozen-window test fail.

**The live check is the maintainer's**, with a real key under `[secrets] jev_api_key`. Write it as exact commands on a
scratch daemon with a fresh state dir, a GLM profile and `[judge] enabled = true`: three `ask` turns in one session
(one runs `proc.run echo hi` at `notify`), then "go on" within a minute of a reply; label five judgments from `theseus
judge log` with `theseus judge label`; `theseus policy tighten proc.run --call <id>`; `theseus judge report` shows
counts, operator and system labels, and "insufficient"; `<state dir>/learning/<date>.json` exists; `theseus ledger
--json -k judge.label`; with `THESEUS_SESSION` set, `theseus judge label` is refused.

**Leave alone:** the packs and builders (a change is a new version); the judge points, the memory pass's `memory.v1`
and the `+rerank` arm included (being added beside you); 23b's surfaces beyond the buttons and page; prove.rs; the
ladder (26a); crates/theseus-core/src/external.rs.
