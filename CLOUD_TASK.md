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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-compaction-roots`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: compaction roots, `context_overage`, and the assembled strategy, step 30c (theseus-6fn.4)

Branch: `cloud/20261004-compaction-roots`. Every commit's subject carries `theseus-6fn.4`. Deadline for the report: 5
hours after you start.

**Background.** When a session outgrows its model's window, the compiler's ring drops leading turns (compiler.rs,
`strategy: ring`), and when even the newest exchange does not fit, it takes that last cut anyway (`|| last`) and sends
a request it estimates too big. Step 30c, M6's compaction roots, summarizes the dropped range instead: a cheap profile
writes a `Summary` node, and the prefix becomes the summary, then the kept tail. It names the overage as a turn's
outcome, and adds the assembled strategy, a recompile's prefix with a recall section in it. 30b, merged before you
start, put recall in front of the model. M5's CONTINUE, 33 and 35a build on this step.

**Read first:**
- docs/design/m6-memory.md: §1.5, §2.4's last bullets, §2.5, §2.8 (`Summary`, the compilation's `recall_id`,
  `BudgetReport`), §2.11's testimony headers, §2.12 to §2.14, and 30c's rows in §3.1 and §3.2;
- crates/theseus-core: compiler.rs (`compile_with`, the ring, `Compilation`, `Overflowed`), turn.rs (`WINDOW_CLASS`,
  `window_failure`, how the model call is planned, reserved, dispatched, and settled), tests_overflow.rs, recall.rs,
  turn/recall_step.rs, node.rs (`Body`), tests_layouts.rs; crates/theseus-index/src/extract.rs.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **30b's names.** Its task named the `Recall` node (`recall_id`, `arm`, references), the `recall.ran` row, the
  `BudgetReport` (each drop with its reason, tokens and tier, the ring's cut as a range, an overage), and its split
  read (`recall_begin`, `recall_end`). Its code on main is the truth wherever it differs from these names.
- **One store format number**, `MANIFEST_FORMAT`. The `Summary` body and the compilation's `recall_id` are one bump
  from main's number as you cloned it, with layout samples; the maintainer renumbers at the merge.
- **The provider's overflow is handled already** (theseus-9p88): a refused or cut request rings by the provider's
  numbers, and one the ring can't fit fails with class `context_window`. `context_overage` is the estimate's case,
  before anything is sent. Keep both classes, or argue for one, and say which in the report.
- **Recall's default mode is `off`**, not the design's shadow. The assembled recall section follows the session's arm
  as 30b assigns it: in shadow it writes only the row of what it would have admitted.
- **The place rule** replaced labels. A compaction summarizes the session's own nodes only, and the assembled
  section keeps 30a's place filter.
- **The cockpit replaced the Observatory.** Leave it, and say in the report what its compaction view should show.

**What to build (30c),** each a green commit:
1. `[memory] summary_profile` (default `glm`, as the design says; `off` keeps the ring) and
   `assembled_budget_tokens` (4,000), with defaults and template lines. The template line says that the dropped range
   goes to that profile's provider, which may not be the session's.
2. The `Summary` body (`first`, `last`, `nodes`, `text`, `profile`, `model`, `cost_usd`) and its render: a testimony
   header (`[Summary of 212 earlier messages, 2026-09-20 to 2026-09-27, written by glm]`), first in the prefix
   whatever its position. The index tender indexes its text (`the_extractor_covers_every_body_variant`).
3. Compaction where the ring would cut. The summary call goes through the turn's own provider-call path, so it is
   reserved before it runs and settled at its real cost. Then a compilation with `strategy: compaction`, thinking
   stripped as the ring strips it. A `Recall` node in the range is dropped, never summarized (tier `compaction` in the
   `BudgetReport`), and a second compaction folds the first summary in. If the call fails or would overrun, the ring
   runs as today, and the row says why. It lands as a fact: a row, a span, a metric, and a narrative line
   ("Compaction summarized 212 messages into 380 tokens with glm, for $0.0011").
4. `context_overage`: when the newest exchange alone does not fit, the turn fails before any call, with the window,
   the estimate, and the exchange's tokens, and the `BudgetReport`'s overage. No retry repeats it.
5. The assembled strategy at a task's first compile and at a compaction: the system block, a recall section (30b's
   pipeline at `assembled_budget_tokens`, a `Recall` node whose id the compilation records), the summary, the tail.
   Its read finishes before the compile, under the recall deadline. If time runs out, leave this one and report it.

**Proof, offline:**
- The design's 30c tests: compaction replaces the ring on overflow; the summary's range; the ring as the fallback when
  the summary call fails; `context_overage`; the assembled prefix of a task's first compile; a compaction rebuilt byte
  for byte from its manifest after a restart.
- The next request after a compaction begins with the compacted request's bytes.
- The summary's cost is reserved, then settled, in the ledger.
- A plain turn's frames are unchanged: `a_plain_turn_stays_within_its_frame_budget`, and `theseus-sim bench turn
  --check`, which the gate skips here.
- The old layouts read (tests_layouts).
- Run the overflow and compiler tests 5 times under load.
- Planted reverts: take the ring's last cut again instead of `context_overage`, and show its test fail; render the
  summary at its node's position instead of first, and show the byte-for-byte test fail.

**The live check is the maintainer's**, with a GLM key. Write it in the report as exact commands on a scratch daemon
with a fresh state dir, Discord and the web UI off, and two small-window lines (`[catalog."glm-5.3-flash"]
context_window = 32000`, `[profiles.glm] max_output_tokens = 2000`). Check the numbers against the template's own
system block first:
1. A `glm` session reads one file of about 30 KB a turn (`fs.read`), four turns. Once past the window, `theseus
   --json ledger --kind context.compiled` shows `strategy: compaction`, the ledger shows the summary call's cost, and
   `theseus --json history <session>` shows the `Summary` node with its range.
2. The next turn appends, with no recompile.
3. `theseus ask -s <session> --attach <a 150 KB text file> "read this"` fails at once with `context_overage` and its
   numbers, and makes no provider call.
4. With `summary_profile = "off"`, the same steps ring as before.

**Leave alone:**
- CONTINUE's signals and `continue.v1` (25b): it stays in shadow, and your triggers stay deterministic.
- The memory pass (31a) and the exam's arms (34b), built beside you in recall.rs, config/memory.rs and theseus-memory:
  keep your code in compaction's own modules.
- 32c's `+rerank` arm, and the judge's calls in turn.rs with 23b's trace marks.
- The task arrangement (27), independence (28a) and the TASK record (39a). If 27's `Arrangement` is on main, the
  assembled prefix keeps it where 27 renders it.
- The cockpit, and crates/theseus-core/src/external.rs.
