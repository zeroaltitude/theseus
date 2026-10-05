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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-consolidation`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: consolidation into cited syntheses, and the `+synthesis` arm, step 31b (theseus-6fn.10)

Branch: `cloud/20261005-consolidation`. Every commit's subject carries `theseus-6fn.10`. Deadline for the report: 5 hours
after you start.

**Background.** Recall, the memory pass (31a) and the live rerank (32d) are on main. Consolidation (design
m6-memory.md §2.7), off every turn, finds nodes recall admits together, has a profile write one short cited
synthesis per cluster, checks each citation, and keeps it as a `Synthesis` node. Never shown in shadow: only the
`+synthesis` arm puts one before a model. In the books (spec P8) a synthesis is an encyclopedia entry (by topic),
never an SOP (the operator's alone) nor a recipe (promoted after repeated success): say so in its doc comment, and
build no book.

**Read first:** design m6 §2.7 to §2.9, §2.14, §2.15, 31b's rows in §3.1 and §3.2; theseus-core's memory_pass/,
recall.rs, turn/recall_step.rs and rerank_step.rs, config/memory.rs, graph.rs, node.rs, learning/tender.rs and
audit.rs, judge/; theseus-memory; theseus-judge's pack.rs, builders/, packs/; the AGENTS.md files.

**What changed since the design was written** (the code and AGENTS.md win; report each difference):
- **The memory pass writes only between turns** (`memory_pass/turns.rs`, theseus-ms5m): no frame inside a turn,
  the turn bench's included. Yours too, through `Turns::between`, which assumes one writer (`writing` is a flag):
  count writers, or write through the pass's writer; say which.
- **The place rule replaced labels.** A shared place draws only on its own sessions; a private place draws on any
  (m6 §2.15, and the rule as decided). main's `Place::may_draw_on` admits only private places' sessions to a private
  place: make it `(Private, _) => true` in your first commit (its subject also names `theseus-1is6`), with the place
  property test asserting both directions. A session with no target reads as private, so a synthesis in its harness
  session is private: a shared place never sees one. Synthesize any cluster with no external source (DD5); count the
  rest with their reason.
- **Rerank is live** (32d: `rerank_step.rs`, `manifest_ranked` with the pass's links, `refill`). **The ladder**
  (26a): a judged point asks `pack_on` and `ask_mode`; a new pack is `WIRED` in shadow, and its `Builder` needs arms
  in theseus-judge's `builder_identity` and the core's `unrebuildable` (25d).
- **A call outside any session**: replay's audit (learning/audit.rs) prices, reserves and settles one under a cap.
- **The owner's call at 30c**: no second provider reads a session's text by default. `synth_profile` defaults to
  `session`: the provider every source's session last used (`SessionRecord.last_target`, which route.v1 may move
  turn by turn, 25e); sources that disagree wait, said. A named profile is the operator's choice.
- **The store format**: a new NODE body bumps `MANIFEST_FORMAT` by one from main's as you clone it. It replaces no
  layout, so say beside the bump that no sample is owed, as format 12's note does.
- The cockpit replaced the Observatory: leave cockpit/ but regenerated types; report what its Memory view lists.

**What to build (31b),** each a green commit:
1. **The node:** `Body::Synthesis { text, sources, check, stage }` (§2.8; add what you need), origin `agent`, in a
   harness session of its own found by a META key (as `ladder.session`), never compiled; `derived_from` edges to
   its sources (`via = "synthesis"`); every exhaustive `Body` match; the bump. The memory pass leaves it unlabeled:
   its gate would mark it `same_entity` with its sources, and baseline v2 would drop them for it.
2. **Clusters** (pure, in theseus-memory): pairs admitted together in at least 3 distinct turns, from
   `recall.shadow` and `recall.ran` rows read by kind through the store's pages; each component of 3 to 8 nodes,
   none a synthesis or a recall; a cluster synthesized before (its sources' digest) is not proposed again.
3. **The run:** learning/tender.rs's pattern (a thread at nice 19, about 5% of a core, at `[memory]
   consolidate_hour` (4), not within 10 minutes of a start; nothing with memory off); `memory.consolidate {
   dry_run? }` (acting, the operator's, through `judge_act`) and `theseus memory consolidate [--dry-run]`. Per
   cluster, at most 120 words, every sentence citing; deterministic checks (each sentence cites, each cited id is
   in the cluster); `synthesis.proposed` and `.checked` rows (scope `memory`); `synth_limit_usd_per_day` (0.50),
   its day's spend read from its rows, so a restart keeps it. A dry run writes nothing.
4. **Jev's check:** a pack `citation.v1` at a point of its own, one Noul per sentence and cited source ("the source
   supports the sentence"); one under 0.5 rejects it. Without Jev it stays `unchecked`, never promoted. Say whether
   a shadow pack's verdict may qualify it for the arm.
5. **Shadow scores:** for recent recalls that admitted two or more of its sources, would the pack have selected it
   (`synthesis.scored { would_select, rank }`)? Rows keep no query: score it as its best admitted source, or
   better; say which.
6. **The `+synthesis` arm**, checked syntheses as candidates. Every other arm leaves the harness session out
   (`exclude_sessions`; check the tender drops it before its top k), and the pipeline drops a synthesis the arm
   does not admit, or an unchecked one, with a reason of its own. **The arms' seam** (32a and 32b add arms beside
   you; build it so, and merges are keep-both): `MemoryArm` gains yours (serde `"+synthesis"`, `sources()` as
   `baseline`'s); `Memory::science_for(arm) -> Arc<dyn MemoryScience>`, a match (yours ranks as `baseline`); the
   `Scene` carries the turn's arm's science (shadow, a control, a search without an arm: `baseline`), used by
   `manifest_ranked`, `refill` and the rerank's `Recalled`; `MemorySearchParams` gains `arm` (`memory search
   --arm`). Report what the exam needs to run it (its store has no recall rows).

If time runs out, leave 6, then 5, and report them.

**Proof, offline:** clusters from a table of rows; the fake Jev rejects an unsupported sentence; a cluster with an
external source is never synthesized; a shared place never admits a synthesis, and a private place admits a shared
place's session; nothing shown in shadow (a turn's request digest equal with and without syntheses stored); spend
stops at the cap; a dry run writes nothing; no frame inside a turn (as `the_pass_writes_only_between_turns`); under
`+synthesis` a checked one admitted, an unchecked one dropped with its reason, the place property test holding. Frame
tests 5 times under load. Planted reverts, each naming the test it breaks: `may_draw_on` back to private-only; a
synthesis admitted in a shared place; an unchecked synthesis admitted; a frame not waiting for turns.

**The live check is the maintainer's**, with a GLM key and optionally Jev's. Exact commands for a scratch daemon:
fresh state dir, `theseus-index` beside `theseusd`, Discord and the web off, `[memory] mode = "live"`, `arm =
"baseline"`, `synth_profile` the GLM profile (debug: `recall_deadline_ms = 2000`):
1. A, B, C each state a fact about an invented Kestrel relay (port 7714, its log path, its nightly restart); D, E, F
   each ask "What do we know about the Kestrel relay?": `theseus memory recalled` shows each admitting all three.
2. `theseus memory consolidate --dry-run` lists one cluster, writing nothing. Without it: `theseus --json ledger
   --kind synthesis.proposed` shows the cited text and its cost; `synthesis.checked` Jev's verdict or `unchecked`;
   `theseus reach <A's node>` lists the synthesis.
3. `arm = "+synthesis"`, restart, G asks again: `memory recalled G` shows it admitted when checked, else dropped
   with its reason. Under `baseline` it never appears.

**Leave alone:** 32a's retention and 32b's activation (the seam's lines only), 33's tiering (transcript reads, a
node cache), 35a's situations (the compiler's situation input, recall/render.rs's headers), the memory pass's
labels and gate, rerank's step, 39b's task board, 42b's cockpit tabs.
