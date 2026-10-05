<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

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
- About 33 L1 tests fail here: theseus-sandbox's contract tests, its bench's `spawn_100`, and theseusd's sandbox tests. The VM runs as root, and L1 refuses a root daemon's job that has no job cgroup (theseus-pv6i).
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, `theseus-store tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs), and `theseus-core term::tests::python3s_repl_computes_on_the_screen` can fail (theseus-1n2y). Neither is on the flaky list: rerun it alone, and name it in the report.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About fifteen other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- route.v1 on the turn path (`[routing]`, detours and switches; store format 15);
- the live rerank (rerank's own breaker, a bounded live wait, per-item grading);
- security.v3's live notices and their brake;
- replay, audit and backfill of judgments;
- the ladder (`pack.mode`, `mode_for`, `ask_mode`, `pack_arm`, `theseus packs`, the cockpit's Ladder panel);
- the tools smalls (categorize on an empty ontology, the LSP's `start_on_edit`, AWS hands' runaway mode);
- the tasks smalls (layer 1 for the owner's tasks only, `task.change_expired`, place warnings, a check's restricted view; store format 16).

These are other cloud sessions like you, each on its own branch:
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- FSRS-6 retention and the `+retention` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop, if it launches beside them.

On the owner's machine:
- gliding with the place rule (`channel.post` and `channel.read`; places.rs, rpc/publish.rs, approval.rs, outbox.rs);
- the cockpit's Ship view (a gentle roll);
- the Linux lanes: spawn without fork and a light job cgroup (the job launch path), one sync per job completion, background work that yields, LSP watching, then socket activation;
- a benchmark run (bench/).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 15 since route, and 16 after the tasks smalls), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-activation`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: spreading activation as a recall source, and the `+activation` arm, step 32b's wire-in (theseus-6fn.12)

Branch: `cloud/20261005-activation`. Every commit's subject carries `theseus-6fn.12`. Deadline for the report: 5
hours after you start.

**Background.** The math is on main: theseus-memory's `activation.rs` (`spread` over an `Adjacency`, §2.7's edge
weights and `SpreadParams` as data: two hops, decay 0.7, threshold 0.1, at most 200 nodes; a recall's
`derived_from` weighs zero by construction) and the trait's `activate`. The memory pass (31a) writes what it walks:
`same_entity` and `supersedes` edges, and each node's entities (`memory.labeled`'s `about`). Step 32b's wire-in
builds the adjacency projection and adds the `+activation` arm: activation as one more ranked source in recall's
fusion, at the same budget.

**Read first:** design m6-memory.md §2.4, §2.7 (spreading activation), §2.9's arms, §2.12, §2.13, 32b's rows in
§3.1 and §3.2; theseus-memory; theseus-core's recall.rs,
turn/recall_step.rs and rerank_step.rs, graph.rs, node.rs, memory_pass/, fact/memory.rs, store.rs, rpc/memory.rs,
config/memory.rs; theseus-index's fuse.rs; theseus-store's pages.rs; theseus-exam; the AGENTS.md files.

**What changed since the design was written** (the code and AGENTS.md win; report each difference):
- **The edges as written** (graph.rs): EDGE `{ kind, from, to, via, at_ms }`, no weight, scoped `in:<to>`.
  `derived_from`'s routes are `report`, `brief`, `publish`, `arrangement`, `claim` and `recall` (zero); 31b, built
  beside you, adds `synthesis`. Map each route you know; one you don't spreads nothing, said. `supersedes` runs
  from the newer node to the older: 1.0 toward the newer, 0.2 back.
- **Entities:** the memory pass asks the tender (`index.entities`) and writes a node's terms in its
  `memory.labeled` row. A node it never labeled (a store from before 31a, an exam store's past) has none: ask the
  tender for them after serving, or leave them out; say which.
- **The fusion is the tender's** (fuse.rs: weighted reciprocal rank, k = 60): each hit carries its sources' ranks
  and its fused score. Add activation's term as the tender would, its weight in the science's data.
- **Rerank is live** (32d: `rerank_step.rs`, `Memory::manifest_ranked` with the pass's links, `refill`): a candidate
  added before the pipeline reaches the rerank too.
- **The place rule:** a shared place draws only on its own sessions; a private place draws on any. main's
  `Place::may_draw_on` still admits only private places' sessions to a private place; consolidation's session fixes
  that (leave the function to it). A node activation adds is a candidate like any other: its place read by
  `place_of`, and dropped first when the turn may not draw on it.
- **The arms are scratch-daemon config** (the exam's `[memory] arm`), never a `turn.submit` field; the exam
  judges arms at an equal budget.
- **FAST:** the projection is built after serving, never on the start path; its RSS counts toward §9's. The spread
  runs inside recall's deadline: with the judge on, a message already waits up to about 650 ms before its first
  call (the index 250, rerank 200, route.v1 200, 25e). The turn bench runs memory in shadow.
- The cockpit replaced the Observatory: leave cockpit/ but regenerated types; report what its Memory view shows.

**What to build (32b's wire-in),** each a green commit:
1. **The projection** (an `Adjacency<String>`): neighbouring nodes in a session by position (0.3), a tool call and
   its result by `tool_use_id` (0.8), the EDGEs by kind and route, and shared entities (each entity's nodes and
   `df`, weighing `1/ln(1+df)`), expanded at a spread, never stored node by node; bound a common entity's expansion
   (at the defaults one with df over 1,095 cannot carry a seed past the threshold alone) and say what that changes.
   Built after serving on the blocking pool when the config's arm reads it (or a search asks), kept current from
   what the core writes.
2. **The `+activation` arm.** After the index answers, the seeds: the turn's new node at 1.0 (its neighbour, and
   its entities if you ask the tender inside the deadline; say which) and the top 10 fused hits at their
   normalized scores. `activate` spreads (`baseline`'s spreads nothing). The reached nodes are a ranked source:
   each candidate's fused score gains activation's term, and the strongest reached nodes the index did not return
   join the candidates (at most 20; their text, place and trust read from the store) before every filter.
   **The arms' seam** (31b and 32a add arms beside you; build it so, and merges are keep-both): `MemoryArm` gains
   yours (serde `"+activation"`, `sources()` as `baseline`'s); `Memory::science_for(arm) -> Arc<dyn
   MemoryScience>`, a match, `baseline` but for yours; the recall's `Scene` carries the turn's arm's science
   (shadow, a canary's control, a search without an arm: `baseline`), used by `manifest_ranked`, `refill` and the
   rerank's `Recalled`; `MemorySearchParams` gains `arm` (`theseus memory search --arm`).
3. **Surfaces:** the manifest's `sources` count `activation`, and each item its activation rank and score (JSON
   fields in the row: no format bump); `memory search --arm +activation` shows its share of what was admitted; an
   `activate` span inside `recall` (§2.13); health's memory line: the projection's nodes, edges and bytes; metrics.
4. **The exam:** `Arm` gains `+activation` (its daemon's own `[memory] arm`), `daemons_for` its name, and the
   report the pair `+activation − baseline`. Say what items would show activation. docs/m6-ablation-plan.md's next
   version is the maintainer's: say what it adds.

**Proof, offline:** the design's 32b tests through the core: the projection's edges on a small store (positions,
tool pairs, each route, `supersedes` both ways, shared entities with their df); zero weight through a recall's
edges (a node reachable only through a `Recall` node is never activated); determinism (the same store gives the
same projection and spread, built whole or kept current); a node activation adds from another place is dropped for
`place` (the place property test over generated stores, the arm on); a recall under the arm inside its deadline; a
shadow turn's request byte for byte unchanged; `theseus-sim bench turn --check`, both kinds. Timing tests 5 times
under load. Planted reverts, each naming the test it breaks: a recall's edge given weight; activated nodes let past
the filters; `supersedes` walked toward the older node at 1.0.

**The live check is the maintainer's** (a GLM key, or the stand-in `theseus-sim fake-model --rules`). Exact commands
for a scratch daemon: fresh state dir, `theseus-index` beside `theseusd` with `[index] weights_dir` an empty
directory (BM25 and entities alone), Discord and the web off, `[memory] mode = "live"`, `arm = "+activation"`:
1. A: "The Kestrel relay was fixed by commit 3f9a2c1." B: "Commit 3f9a2c1 also raised the retry limit to 9." A few
   seconds on (the pass labels them), C: "What fixed the Kestrel relay?": `theseus memory recalled C` shows B
   admitted with an `activation` rank and no index rank, reached through the shared commit.
2. `theseus memory search --arm baseline "What fixed the Kestrel relay?"` lacks B; `--arm +activation` has it.
3. `theseus health` names the projection's nodes and edges.

**Leave alone:** 31b's consolidation and 32a's retention (beside you: the seam's lines only), 33's tiering (the
transcript reads, a node cache), 35a's situations (the compiler's situation input, recall/render.rs's headers), the
memory pass's labeler and gate (read its rows; change none), rerank's step but its science, 39b's task board, 42b's
cockpit tabs.
