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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-rerank-arm`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: Jev's rerank of recall's top 20, step 32c (theseus-6fn.3)

Branch: `cloud/20261004-rerank-arm`. Every commit's subject carries `theseus-6fn.3`. Deadline for the report: 4 hours
after you start.

**Background.** Recall (step 30a) asks the index tender on a turn's first loop, drops what the place rule and its
other reasons forbid, ranks the rest in the science's fused order (crates/theseus-memory's `Baseline`), and packs
them; in shadow it records what it would admit in a `recall.shadow` row. The Jev wire-in (step 23a) put Jev's client
in the core: `[judge]`, `JudgeService`, the sink's `judge.call` rows, the shadow day budget, and `loop.v1` in shadow.
Step 32c is the `+rerank` arm: one Jev request per recall asks, of each of the top 20 candidates, whether it holds
information that would help answer the message, and re-sorts them by the answers. The memory exam compares it with
`baseline` (§2.9).

**Read first:**
- docs/design/m6-memory.md §2.7 "Jev rerank (32c)", §2.9's arms table and "Equal total budget", §2.12's turn row,
  §2.15's Jev bullet, and 32c's rows in §3.1 and §3.2; docs/design/m5-judgment.md §2.2, §2.3 (packs as data, the
  loader rules), §2.5 and §2.6;
- crates/theseus-judge: pack.rs, builders.rs, the packs/ files, fake.rs; crates/theseus-core: judge/ (`JudgeService`,
  `WIRED`, the sink, spend.rs), recall.rs, turn/recall_step.rs; crates/theseus-memory: recall.rs, science.rs.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **Arms, canary and live arrive with step 30b**, which another session is building beside you, and live judgments
  as kernel actions with 26b. So build the arm in shadow: a recall that ran is reranked off the turn's path, and its
  `judge.call` row records what `+rerank` would admit beside what `baseline` admitted. Keep the reorder a function
  30b's live path can call before its pack, and say in the report how it should.
- **Who pays.** Shadow spend is the judge's day budget (spend.rs), so a shadow rerank is paid there, with `purpose:
  "recall"` on its row. The session-budget path of a live arm waits for 26b: report it.
- **The place rule replaced labels.** Only candidates that passed every filter, place first, may reach Jev, and the
  core's scrubber cleans the state as it cleans every state.
- **The store has one format number.** A new ledger row needs no bump.

**One convention, six sessions** (you, 23b's surfaces, 24's security pack, 25a's inbound packs, 25b's continue pack and the other memory or category point, each dispatching judgments; keep to these names so the merges stay mechanical). The core mints a judgment's id at its dispatch: `theseus_judge::Ask` gains `pub id: Option<String>` (`Ask::new` sets `None`), and `Judgment::pending` uses it when set, else mints one as now. A dispatch inside a turn marks the turn's trace with a zero-length span: name `judge`, kind `mark`, attributes `pack` (as the pack you call), `point`, `mode`, and `judgment` (the id its row will carry); a dispatch outside a turn needs no mark. Put your point in a module of its own under judge/ (a child module sees the service's private parts): `judge/mod.rs` gains a `mod` line and its `WIRED` lines, and 23a's loop path stays as it is. If another session's identical change is already on main when you clone, use it as it is.

**What to build (32c):**
1. `rerank.v1` in crates/theseus-judge/packs/, with its builder: the new message, trimmed, and up to 20 candidates'
   excerpts, one Noul each ("this note holds information that would help answer the message"), within the loader's
   rules; add a `recall` decision point if the closed set needs one. Loader and builder tests beside the others.
2. A pure reorder in theseus-memory, in a module of its own: the top 20 re-sorted by Jev's probability, the rest after
   them in fused order, and an unanswered item keeping its fused place.
3. The core's rerank, spawned after a recall's pipeline, never on the turn's path: the client's own deadline of 600
   ms; any failure (timeout, breaker open, no key, the day's limit, shed) falls back to the fused order, and the row
   says which. The row carries the recall's id, both orders' admitted ids, Jev's latency against the deadline, and
   the cost. `WIRED` gains the pack in shadow; `[judge.packs."rerank.v1"] mode = "off"` turns it off.
4. `theseus judge log` shows each rerank with its recall's id and whether the order changed what would be admitted.

**Proof, offline**, against the fake Jev:
- the design's 32c tests: a fake Jev reorders; a timeout falls back to the fused order; spend recorded as `purpose:
  recall`;
- a shared place's rerank state holds no candidate the place filter dropped, over the place property test's
  generated stores;
- a slow, failing or rate-limited Jev changes no turn's request bytes, duration or frame count;
- the day's limit skips a rerank; mode off calls nothing.

Run them 5 times under load. Planted reverts: hand Jev the candidates from before the filters, and show the place test
fail; await the rerank in the turn, and show the timing test fail.

**The live check is the maintainer's**, with Jev's real API key (`[secrets] jev_api_key`) and a model key. Write it in
the report as exact commands on a scratch daemon with a fresh, short state dir, `theseus-index` beside `theseusd`,
Discord and the web off, `[memory] mode = "shadow"` and `[judge] enabled = true`:
1. Session A: "Remember: the grey heron nests by the old weir at Millbrook." Session B: "Where does the grey heron
   nest?" `theseus judge log` shows a `rerank.v1 (shadow)` judgment with B's recall id, its cost and latency, and
   `theseus memory recalled B` shows the recall it reranked.
2. Ten more questions across sessions, then `theseus --json ledger --kind judge.call -n 100`, its `rerank.v1` rows:
   their p50 and p95 latency against 600 ms, and the cost per recall.
3. `[judge.packs."rerank.v1"] mode = "off"` and a restart: the next recall writes no rerank row.

**Leave alone:** 30b (the `Recall` node, canary and live, and `[memory]`'s arm keys, being built beside you): keep
your changes to the recall path to one call into your own module; the other packs being wired tonight (23b's
surfaces, 24's `security.v1` at the gate, 25a's inbound packs, 25b's compile signals, 27's arrangement): add one
`WIRED` line, not a reshaped service; `security.v2` and `security.v3`'s packs; `theseus judge prove`'s report
generator in theseus-judge (just joined); the judge sink's frame (a fix joins with it).
