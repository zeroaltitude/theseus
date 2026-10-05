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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-retention`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: FSRS-6 retention, its projection from the memory rows and the `+retention` arm, step 32a's wire-in (theseus-6fn.11)

Branch: `cloud/20261005-retention`. Every commit's subject carries `theseus-6fn.11`. Deadline for the report: 4 hours
after you start.

**Background.** The math is on main: theseus-memory's `fsrs.rs` (FSRS-6 from the published algorithm, checked
against the reference crate, `fsrs6-default`, `Fsrs6::step` and `fold`) and `access.rs` (an `AccessEvent` and the
grade §2.7's table gives it; `Shown` is no review). The memory pass (31a) now writes what it folds:
`memory.labeled` (a node's durability: its first sight), `memory.used` (a recalled item used or not, and its
outcome) and the operator's `memory.label`. Step 32a's wire-in folds them into a retention projection and adds the
`+retention` arm, which ranks recall's candidates by retention too, at the same budget.

**Read first:** design m6-memory.md §2.3, §2.7 (FSRS-6), §2.9's arms, §5 questions 9 and 10, 32a's rows in §3.1
and §3.2; theseus-memory (fsrs.rs, access.rs, science.rs, recall.rs); theseus-core's recall.rs, turn/recall_step.rs
and rerank_step.rs, memory_pass/ (attribution.rs, labels.rs), fact/memory.rs, fact/recall.rs, rpc/memory.rs,
config/memory.rs, store.rs (`ledger_page`); theseus-store's pages.rs; theseus-exam's drive.rs, arms.rs, report.rs;
the AGENTS.md files.

**What changed since the design was written** (the code and AGENTS.md win; report each difference):
- **The rows' shapes are 31a's** (fact/memory.rs): `memory.labeled` (scope `memory:<session>`, `durability`,
  `position`), `memory.used` (scope `recall:<session>`; `used`, and `outcome` only when used), `memory.label`
  (scope `memory`). The store pages a ledger kind through its index (`k:<kind>`), at the cost of its answer.
- **The labels are four**, `useful`, `wrong`, `stale` and `should_have`; there is no "remember". The table grades
  `useful` Easy and `wrong`/`stale` Again; choose `should_have`'s grade (Easy, or none) and say why.
- **Rerank is live** (32d: `turn/rerank_step.rs`, `Memory::manifest_ranked` with the pass's links, `refill`, the
  rerank's `Recalled.science`): a rank change must reach the repack too. The baseline science is version 2
  (`prefers_newer`).
- **The arms are scratch-daemon config** (the exam's `[memory] arm`, one daemon per arm), never a `turn.submit`
  field; the exam judges arms at an equal budget. Memory's canary keeps its own hash (the ladder left it).
- **FAST**: nothing on the start path; the turn bench runs memory in shadow. With the judge on, a message already
  waits up to about 650 ms before its first call (the index 250, rerank 200, route.v1 200, 25e): add nothing
  outside recall's deadline.
- The cockpit replaced the Observatory: leave cockpit/ but regenerated types; report what its Memory view shows.

**What to build (32a's wire-in),** each a green commit:
1. **The events.** Each row to its `AccessEvent` at its time, in position order: first sight by durability, used
   by outcome, not used as `Shown`, labels by the table. Pure where it can be; a table test row by row.
2. **The projection:** a node's `Retention`, folded with `Fsrs6` (`fsrs6-default`) from every arm's events (one
   projection, §5 question 9), built after serving on the blocking pool when the config's arm reads it (or the
   first search that asks, off the turn path), never on the start path; kept current as the pass's frames and
   `memory.label` are written. A recall before it is built ranks without it and says so in its row.
3. **The `+retention` arm:** a science that ranks by the fused score and the node's retrievability at the turn's
   time (`retrievability_at`), its weight and form versioned data in its digest; a node with no retention keeps
   its fused score (or say what is fairer). `RankCtx` carries the candidates' retention, filled by the core: the
   science stays pure. **The arms' seam** (31b and 32b add arms beside you; build it so, and merges are
   keep-both): `MemoryArm` gains yours (serde `"+retention"`, `sources()` as `baseline`'s);
   `Memory::science_for(arm) -> Arc<dyn MemoryScience>`, a match, `baseline` but for yours; the recall's `Scene`
   carries the turn's arm's science (shadow, a canary's control, a search without an arm: `baseline`), used by
   `manifest_ranked`, `refill` and the rerank's `Recalled`; `MemorySearchParams` gains `arm` (`theseus memory
   search --arm`).
4. **Surfaces:** under the arm, each manifest item's retrievability, stability and difficulty (JSON fields in the
   row: no format bump); `memory search --arm +retention` and `memory recalled` show them; health's memory line
   says the projection's state and its nodes; a metric of its size.
5. **The exam:** `Arm` gains `+retention` (its daemon's own `[memory] arm`), `daemons_for` its name, and the report
   the pair `+retention − baseline`. Its stores hold no `memory.used` rows: say what items would show retention
   (the fixture writer writes ledger rows). docs/m6-ablation-plan.md's next version is the maintainer's: say what
   it adds.

**Proof, offline:** the design's 32a tests through the core: the rebuilt projection equals the incremental one,
event for event; exposure without use changes nothing (a `memory.used` with `used: false` leaves a node as it was);
first sight by durability; each label's grade; under `+retention`, two candidates with equal fused scores order by
retention (one used `ok`, one `corrected`), and the repack in Jev's order keeps that science; the place property
test still holds; a shadow turn's request is byte for byte unchanged; `theseus-sim bench turn --check` both kinds.
The recall and projection tests 5 times under load. Planted reverts, each naming the test it breaks: `Shown` graded
as a review; the rank ignoring retention; the events folded out of position order.

**The live check is the maintainer's** (a GLM key, or the stand-in `theseus-sim fake-model --rules`). Exact commands
for a scratch daemon: fresh state dir, `theseus-index` beside `theseusd`, Discord and the web off, `[memory] mode =
"live"`, `arm = "+retention"` (debug: `recall_deadline_ms = 2000`):
1. A: "The Osprey build caches to the blue bucket." B: "The Osprey build runs on the larch runner." C: "Tell me
   about the Osprey build.", then "Thanks.": `theseus --json ledger --kind memory.used` shows C's items, a used
   one `ok`.
2. `theseus memory search --arm +retention "Osprey build"`: each node's retrievability, stability, difficulty and
   last review; a used node's review is at its use (a same-day Good leaves stability as it was: FSRS-6's rule).
3. `theseus memory label <B's node> stale`: B's stability falls (a same-day Again) and recall drops it as
   `labeled_wrong`; `theseus health` names the projection's nodes.

**Leave alone:** 31b's consolidation and 32b's activation (beside you: the seam's lines only), 33's tiering (the
transcript reads, a node cache, `decay_sweep`'s callers), 35a's situations (the compiler's situation input,
recall/render.rs's headers), the memory pass's labeler and gate (read its rows; change none), rerank's step but its
science, 39b's task board, 42b's cockpit tabs.
