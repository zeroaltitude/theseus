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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-rerank-live`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: Jev's rerank of recall made live, step 32d (theseus-6fn.7)

Branch: `cloud/20261004-rerank-live`. Every commit's subject carries `theseus-6fn.7`. Deadline for the report: 5 hours
after you start.

**Background.** Step 32c's `rerank.v1` (judge/rerank.rs) asks Jev, after a recall's pipeline, which of the top 20 notes
that passed every filter would help answer the message, and records what its order would admit. It reranks shadow
recalls only (memory in shadow, a canary's control); `recall_live` reranks none. 32d makes it live, bounded, and lets
the owner's labels teach it.

**Read first:** docs/design/m6-memory.md §2.7, §2.9's arms, §2.12, §2.14; m5-judgment.md §2.2, §2.9; theseus-core's
judge/, turn/recall_step.rs, recall.rs, config/memory.rs, learning/, rpc/learning.rs, tests_rerank.rs; theseus-judge's
breaker.rs, judge.rs, pack.rs, packs/rerank.v1.toml, fake.rs; theseus-memory's rerank.rs; cockpit's
JudgmentLabels.tsx; the AGENTS.md files.

**What the code says** (it wins over this prompt: report each difference; where it can't settle a question, build the
clear part and report the question):
- One breaker, `JevJudge`'s, serves every pack: five slow reranks in a row stop the others.
- A per-item question is named as its answer names it, `helps.1` to `helps.10` and `helps_more.1` to `helps_more.10`,
  with `about` the note's `<node>#<chunk>`. The learning report reads whole answers only, `labels::check` knows pack
  questions only, and the cockpit's buttons skip per-item answers.
- §2.7 pays a live rerank from the session: that waits for 26b; keep the reservation on the judge's day budget.
- The exam runs the judge off and refuses `arm = "+rerank"`: leave its arms; report how `+rerank` would wire in.
- A new JSON field in a ledger row needs no `MANIFEST_FORMAT` bump.

**What to build (32d):**
1. **The labels test, first** (theseus-mm4a). A note the owner labeled `wrong` or `stale` must never reach Jev's rerank
   request nor come back through the repack. `recall_end` hands the labels over (`Recalled.labeled`); set that field to
   `Default::default()` and no test fails today. Test through a real turn (`FakeJev::seen()`): the note's key and text
   are in neither the request nor the row's admitted lists. Plant that revert; after step 3, test the live path too.
2. **Rerank's own breaker.** Rerank's failures and timeouts move only its own breaker; the other packs keep the shared
   one. One client, so `max_in_flight` and the shed count stay shared. `judge.circuit` rows name their breaker
   (`breaker: "rerank"`, none for the shared one), health's judge line shows both, and `breaker_status()` stays the
   shared one's (route.v1 reads it).
3. **Live, bounded.** In `recall_live`, when rerank.v1 is live, take the candidates as `recall_end` does
   (`manifest_with`) and make one call into judge/rerank.rs. The turn waits at most `[memory] rerank_wait_ms` (new;
   default 200, 1 to 600) from the rerank's start, state building included. In time, the node holds the repacked order.
   On a miss, a timeout, rerank's breaker open or the day's budget spent, recall's own order stands; a late answer is
   still recorded, marked `late`. The call keeps its 600 ms deadline. Why 200: 11 live reranks took p50 115 ms and p95
   149 ms (the most 149), so 200 misses few, and with the index's 250 ms it stays inside §2.12's 600 ms under
   `+rerank`. A `judge` span covers the wait (with `applied` or the reason); the `recall.ran` row and `theseus memory
   recalled` say whether Jev's order was used. Check the mode before cloning candidates.
   **The arms rule**, in the template's comment: memory's mode decides what reaches the model, rerank's whether Jev
   orders it. A recall in front of the model (a canary's treatment; every session under `live`) is reranked live when
   rerank.v1 is `live`, in shadow when `shadow`; one that reaches no model (memory in shadow, a canary's control) is
   reranked in shadow, as now; `off` reranks none.
4. **The ledger grades per-item answers.** `theseus judge label <jdg_id> true --question helps.3`, refused for a
   question the judgment didn't ask; the row keeps the item's key. The cockpit gives each item true and false buttons.
   The report grades per-item answers by definition (`helps`, `helps_more`), as a Noul. A system label (its own rule,
   `SYSTEM_WEIGHT`, keyed with its item): a note the owner labels `useful` or `should_have` (`theseus memory label`)
   after a recall makes that recall's rerank answer about it `true`; `wrong` or `stale`, `false`. The label's `recall`
   names the recall; else take the newest rerank before it that asked about the node, and report that choice. A
   whole-judgment `right` or `wrong` stands for every question: report whether it should for every item.
5. **Live by default.** `WIRED` gives rerank.v1 `PackMode::Live`: live whenever the judge and memory are on. Health's
   pack lists, the template's `[judge]` words, and both AGENTS.md files follow.

**Proof, offline** (as tests_rerank.rs): step 1's test on both paths; five rerank timeouts open rerank's breaker while
a `loop.v1` judgment still goes out; a live recall carries Jev's order into the request; a slow Jev leaves the request
byte for byte a judge-off turn's, and its late row lands; breaker open, budget spent, judge off, rerank in shadow: no
wait; a paused-clock test (tokio's `start_paused`, a channel for Jev): the turn goes on exactly `rerank_wait_ms` after
the rerank starts, an answer 1 ms before is applied, one 50 ms after is late; per-item labels and refusals, the
report's counts, the system label written once and beaten by an owner's 1.0; the turn bench (`theseus-sim bench turn
--check`, judge off) unchanged. Timing tests five times under load. Planted reverts, each naming the test it breaks:
step 1's; rerank's outcomes on the shared breaker; the wait unbounded; per-item answers dropped from the report.

**The live check is the maintainer's**, with `[secrets]` entries `jev_api_key` and a model key. Write exact commands for
a scratch daemon (fresh state dir, `theseus-index` beside `theseusd`, Discord and the web off, `[memory] mode = "live"`,
`[judge] enabled = true`):
1. Session A: "Remember: the grey heron nests by the old weir at Millbrook." Session B: "Where does the grey heron
   nest?" `theseus --json ledger --kind turn.ended -n 1` shows B's `judge` span (rerank.v1, live, applied), and
   `theseus memory recalled B` the recall in Jev's order.
2. `rerank_wait_ms = 1` and a restart: the next recall keeps its own order; `theseus judge log` shows the late answer.
3. `theseus judge label <jdg> true --question helps.1`, then `theseus judge report --pack rerank` shows it graded;
   `theseus memory label <node> useful --recall <rcl>`, and the report shows the system label.

**Leave alone:** route.v1 (being built on `cloud/20261004-route`: judge/inbound.rs, the client's permits, `WIRED`,
pack.rs's list, report.rs's `ACTING`): change one `WIRED` line, and grade per-item answers in functions of your own;
the memory pass (31a, unjoined; `Memory::manifest` gains a `links_of` argument): keep your recall-step change to one
call; compaction's roots (30c, joining now); independence (28a), extensions loading (43b), the hands' network.
