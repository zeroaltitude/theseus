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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-memory-pass`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the memory pass, attribution, and `memory.v1` in shadow, step 31a (theseus-6fn.6)

Branch: `cloud/20261004-memory-pass`. Every commit's subject carries `theseus-6fn.6`. Deadline for the report: 5 hours
after you start.

**Background.** Recall is built: 30a's pipeline (crates/theseus-memory, the core's recall.rs) and 30b's `Recall` node,
edges, and canary and live arms, merged before you start. Step 31a is M6's memory pass. After a turn ends, off the turn
path, it labels the turn's nodes, finds duplicates and corrections among their neighbours, and records whether each
recalled item was used. The baseline arm then prefers the newer node, and `memory.v1` and `attribution.v1` run in
shadow beside the deterministic labels. 31b, 32a and 32b read what it writes.

**Read first:**
- docs/design/m6-memory.md: §2.2 (`index.neighbours`, the entity field), §2.3, §2.6, §2.8 (EDGE, and the
  `memory.*` rows), §2.15, and 31a's rows in §3.1 and §3.2; docs/design/m5-judgment.md §2.3 to §2.5;
- crates/theseus-core: judge/ (`JudgeService`, `WIRED`, the sink, spend.rs, loop_end.rs), recall.rs, tender.rs (the
  tender's client), graph.rs (`EdgeKind`, `Edge`), fact/; crates/theseus-memory (science.rs, recall.rs);
  crates/theseus-judge (pack.rs's `Point` and `Builder`, builders.rs, packs/, src/fake.rs); crates/theseus-index
  (tender.rs's `neighbours`, entity.rs).

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **29c is on main.** The tender embeds (Nomic v1.5, candle) and answers `index.neighbours`. Without its model files
  it answers BM25 alone, and neighbours fail: the gate then records why and writes no edge.
- **30b's names.** Its task named the `Recall` node (`recall_id`, `arm`, references to its items), the `recall.ran`
  row, `memory.label`, and the `memory.arm` row. Its code on main is the truth where it differs from these names.
- **23a is on main**: `[judge]`, `JudgeService`, the sink, the shadow budget, `WIRED`. The packs `memory.v1` and
  `attribution.v1` don't exist yet: write them as versioned TOML beside the others. `Point` and `Builder` are closed
  sets in code, so add their builders, choose their point, and say which. The owner has consented to sending session
  content to Jev, and the scrubber runs on every state.
- **12a's EDGE** is `{ kind, from, to, via, at_ms }`, with no weight: weights are each kind's data (32b's table). Write
  `same_entity` and `supersedes` with `via = "memory"`. A new field would bump `MANIFEST_FORMAT`, with a sample.
- **The place rule** replaced labels: `trust` is DD5's `external` flag, and recall's place filter stays first.
- **Frames.** A batched frame of the judge sink's landed inside a measured turn of the turn bench, whose config keeps
  `[memory]` in shadow, and failed a join (theseus-0j2.3). Keep the pass's frames out of a measured turn.
- **The cockpit replaced the Observatory.** Say in the report what its Memory view should show.

**What to build (31a),** each a green commit:
1. **The pass,** after each turn ends: one call beside the judge's `after_turn`, its work off the turn path, and one
   frame per 32 nodes or 2 s, whichever comes first. Eligible: human and agent messages, and tool results; never
   `Recall` nodes, harness lines, judgments, manifests, or rows. With `[memory] mode = "off"` it does nothing. A node
   a crash left unlabeled waits for its session's next pass, never for a scan on the start path.
2. **The labeler,** table-driven as §2.6 says (kind, durability, about, volatile, trust), one `memory.labeled` row per
   node. Entities come from one extractor, the tender's: ask the tender (a method of its own), or move the rules
   where both crates can read them with no new dependency. Never keep a second copy; say which.
3. **The gate,** over `index.neighbours`: cosine 0.92 or more makes a `same_entity` edge (the duplicate stays), and a
   correction (a rule in the labeler's table) whose top neighbour reaches 0.75 makes `supersedes`, from the newer node
   to the older. A `memory.gated` row holds the decision and the neighbours seen. Thresholds are the science's data.
   The tender embeds a node a little after its frame: wait for it within a bound, or leave the node for the next
   pass, and say which.
4. **Attribution,** in canary and live, of each item a `Recall` node admitted: `used` when one of its entities is in
   the reply or a tool call's input, or an 8-word run of its excerpt is in the reply; later, its outcome: `corrected`
   when the operator's next message corrects what it overlaps, `ok` when the exchange goes on, else `unknown`.
   `memory.used` rows, scoped `recall:<session>`.
5. **`baseline`'s next version,** the edges' reader: it keeps only the newest of a `same_entity` group and prefers the
   newer side of a `supersedes`. Its digest changes, and the recall rows name it.
6. **`memory.v1` and `attribution.v1` in shadow** through `JudgeService`: the packs, scrubbed states (nothing the
   session's own model could not see), `WIRED` lines, sampling, and the shadow budget. They run off the turn path, so
   23b's trace marks don't apply. With `[judge]` off, the deterministic half runs alone. If time runs out, leave this
   one and report it.

**Proof, offline:**
- The design's 31a tests: eligibility and recursion exclusion; the labeler's rules, table-driven; the thresholds make
  the right edges (a stand-in tender with fixed neighbours); attribution on fixtures; at most one frame per 32 nodes
  or 2 s (tokio's paused clock); shadow judgments with the fake Jev.
- `baseline` admits the newer side of a `supersedes`, and recall's place property test still passes.
- `a_plain_turn_stays_within_its_frame_budget`, and `theseus-sim bench turn --check`, both kinds (the gate skips
  benches here). Run the memory and recall tests 5 times under load.
- Planted reverts: let a `Recall` node into the pass, and show the recursion test fail; drop `baseline`'s preference
  for the newer side, and show its test fail.

**The live check is the maintainer's**, with a GLM key, the tender's model files under `[index] weights_dir`, and,
optionally, Jev's API key. Write it in the report as exact commands on a scratch daemon with a fresh state dir,
Discord and the web UI off, and `[memory] mode = "live"`, `arm = "baseline"` (on a debug build also
`recall_deadline_ms = 2000`):
1. Session A: "The staging port of the Larkspur service is 8081." Session B: "Correction: Larkspur's staging port is
   8082, not 8081." `theseus --json ledger --kind memory.gated` shows a `supersedes` from B's node to A's.
2. Session C: "What is Larkspur's staging port?" The reply says 8082; `theseus memory recalled <C>` shows B admitted
   and A dropped or below it. After C's next message, a `memory.used` row shows B's item used, outcome `ok`.
3. With `[judge] enabled` and a key, `theseus judge log` shows `memory.v1` and `attribution.v1` with their costs.

**Leave alone:** 30c (compaction), built beside you: its `Summary` joins the eligible bodies at whichever merge is
second; 34b's exam arms; 32c's `+rerank` (its pack, builder and `WIRED` line beside yours: keep both at the merge);
25c's nightly report, which compares the labels (write the rows only); 31b, synthesis, which is not this row; the
judge's other points (24, 25a, 25b, 28b); the cockpit; and crates/theseus-core/src/external.rs.
