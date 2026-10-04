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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-recall-node`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: recall in front of the model, step 30b (theseus-6fn.2)

Branch: `cloud/20261004-recall-node`. Every commit's subject carries `theseus-6fn.2`. Deadline for the report: 5
hours after you start.

**Background.** Step 30a put recall in shadow: crates/theseus-memory (`MemoryScience`, `Baseline`, the filters and
the pack) and crates/theseus-core's `recall.rs`, `turn/recall_step.rs`, `fact/recall.rs`, `config/memory.rs` and
`rpc/memory.rs`. On a turn's first loop it asks the index tender, drops what the place rule and its other reasons
forbid, packs the rest, and writes a `recall.shadow` row; the model's request never changes. Step 30b puts recall in
front of the model for the sessions canary or live recall picks: a `Recall` node in the transcript, its edges, and a
budget report on every compilation. 30c (compaction), 31a (the memory pass), the exam's harness and lessons build on
it.

**Read first:**
- docs/design/m6-memory.md: §2.4 (the pipeline, the render, the session's cap), §2.8, §2.9's "Instrument 3: the
  canary", §2.11's first bullet (never silently thinner), §2.13 to §2.15, and 30b's rows in §3.1 and §3.2;
- crates/theseus-memory (AGENTS.md, recall.rs, science.rs); crates/theseus-core's AGENTS.md ("Recall", "The
  ontology"), recall.rs, turn/recall_step.rs and its call in turn.rs, compiler.rs (the ring, `Compilation`, the
  ontology's two-spec compile), node.rs (`Body`), graph.rs (`Edge`, `via`), reach.rs, rpc/confirms.rs (`judge_act`),
  places.rs; theseus-discord's render.rs (the reply's footer).

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **The place rule replaced labels and audiences** (places.rs: a place is private or shared; a trusted guild makes
  every channel bound there private). 30a's filter applies it: a shared place recalls only its own sessions. Keep
  its property test passing through canary and live.
- **One store format number**, `MANIFEST_FORMAT` (7 on main). The `Recall` body and the compilation's `budget` are one
  bump, with the layout sample; the maintainer renumbers at the merge. Declare the `Recall` body alone: `Summary`,
  `Synthesis` and `Lesson` come with 30c, 31b and 35b, each with its own bump.
- **12a's EDGE convention is on main** (graph.rs): write `derived_from` from the `Recall` node to each source with
  `via = "recall"` and no new field, so `node.reach` counts the copy as it counts a report's.
- **M5's ladder (26a) is not built.** Use the design's minimal sticky assignment, a hash of session and experiment,
  recorded once per session as a `memory.arm` row, so 26a can replace it.
- **30a reads the index while the first loop's call is out.** In canary and live the read finishes before the
  compile, under `recall_deadline_ms`; `recall_begin` and `recall_end` are split for that.
- **The cockpit replaced the Observatory.** Leave it, and say in the report what its per-turn recall view should show.

**What to build (30b):**
1. `[memory] mode` gains `canary` and `live`, with `arm` (`none`, `baseline`), `canary_fraction`, `experiment` and
   `session_recall_cap_tokens` (12,000), each with a default and a template line. A canary session's arm is sticky; a
   control session runs `none` live with `baseline` in shadow.
2. The `Recall` node (`recall_id`, `arm`, references to its items, never copies) and its render: §2.4's testimony
   block after the new message, in the same user turn, read from each source by position over the frozen range, so
   every later request repeats the same bytes. It rides in the provider call's plan frame with its edges and a
   `recall.ran` row: no new frame. Past the session's cap, recall pauses until the next recompile.
3. The `BudgetReport` on every compilation (limit, used, each drop with its reason, tokens and tier, the ring's cut
   as a range, an overage) and on the recall manifest.
4. `memory.label { node_id, label, recall_id?, note? }` (useful, wrong, stale, should_have), acting through
   `judge_act` and refused by the CLI inside a job; `wrong` and `stale` drop a node as `labeled_wrong`, from a set
   kept in memory and built after serving. `theseus memory label`.
5. The Discord footer `🧠 N recalled` on a reply recall fed; nothing in shadow.

**Proof, offline:** the design's 30b tests: the render's golden bytes; the next request begins with the previous
request's bytes; the node rides the plan frame (`a_plain_turn_stays_within_its_frame_budget` unchanged, `bench turn`
5 frames); edges scoped `in:<source>`, and `node.reach` counts the copy; the old layouts read (tests_layouts) and an
older build refuses the store; `wrong` excludes; the arm is sticky and recorded; the ring's cut in the report. Also: a
shared place's canary turn renders nothing from another session (the place property test, through canary); a
stalled index holds a canary turn no longer than the deadline; shadow still changes no request byte. Run the recall
and memory tests 5 times under load. Planted reverts: render from the source's whole current text instead of the
frozen range, and show the prefix test fail; skip the place filter in canary, and show the property test fail.

**The live check is the maintainer's**, with a model key (GLM or Anthropic). Write it in the report as exact commands
on a scratch daemon with a fresh, short state dir (the tender's socket path must fit), `theseus-index` beside
`theseusd`, Discord and the web off, and `[memory] mode = "canary"`, `canary_fraction = 1.0` (on a debug build also
`recall_deadline_ms = 2000`: a debug index can answer near 250 ms while it embeds):
1. Session A: "Remember: the grey heron nests by the old weir at Millbrook." Session B: "Where does the grey heron
   nest?" The reply names the weir; `theseus memory recalled B` shows `canary`, `baseline`, A's message admitted.
2. `theseus reach <A's message node>` lists B's `Recall` node as a copy.
3. `theseus memory label <A's node> wrong`; the same question in session C is answered without it, and C's recall
   drops it `labeled_wrong`.
4. On `theseus-sim discord rig` (its `#lab` is bound private), with the same `[memory]` table added to its
   config.toml: step 1's note in a CLI session, then the question said in `#lab` (`theseus-sim discord say`):
   `theseus-sim discord read` shows the reply's `🧠 1 recalled` footer.

**Leave alone:** 32c (the rerank arm, built beside you, in theseus-memory and the core's judge/): keep your edits in
recall's own modules; the compiler's work tonight, CONTINUE's signals (25b) and the task arrangement (27, which adds
an `Arrangement` body too): keep your `Body` change to the variant and its arms; the judge's calls in turn.rs and 23b's
spans; the Discord binding's bindings and runtime (38a); the cockpit; crates/theseus-core/src/external.rs.
