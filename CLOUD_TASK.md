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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-situations`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: situations, the precedence line, testimony, and volatile values as of a time, step 35a (theseus-3nk.1)

Branch: `cloud/20261005-situations`. Every commit's subject carries `theseus-3nk.1`. Deadline for the report: 5 hours
after you start.

**Background.** A `Recall` node renders testimony after a turn's new message (30b), a compaction's `Summary` renders
first in the prefix (30c), and a core overage fails as `context_overage`. Step 35a makes what a compile admits explicit
and checked: the situation becomes a compiler input, a fixed line says how to weigh sources, and notes and summaries
say when and where they come from, volatile values as of a date. Lessons (35b) admit through your table next.

**Read first:**
- docs/design/m6-memory.md §2.4's render, §2.5, §2.8's COMPILATION line, §2.11, and 35a's rows in §3.1 and §3.2; the
  spec's Part II, P8, the Books bullet (docs/spec/07-part2.md);
- crates/theseus-core: compiler.rs (`compile`, `Compilation`), compiler/compaction.rs (`header`, the floor),
  turn/compile_step.rs, turn/recall_step.rs (`hold_node`), recall/render.rs, turn.rs's `system_blocks`,
  memory_pass/labels.rs (`volatile`), node.rs (`RecalledRef.header`, `Summary.header`), tests_compaction.rs,
  tests_recall_node.rs, tests_recall.rs, tests_layouts.rs; theseus-protocol's `ContextCompiled`.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **Testimony is half built.** An item's header is frozen at recall (`a message from <author> in <session>, <date>
  UTC (as of @<position>)`), a summary's when written (`[Summary of 6 earlier messages, <dates>, written by
  <profile>]`). Missing: the place, a reply's model, a summary's positions and model, volatile. Frozen bytes never
  change: new headers are for new nodes.
- **The assembled strategy** is a task's first compile and a compaction (30c). A model, system or tools change
  recompiles the transcript. Record what the code admits; add no recall to other recompiles (report if one should).
- **route (25e) joined:** a switch recompiles on the routed model; a detour compiles a request of its own
  (`compile_detour`: the persona, the last exchanges, the message; no recall), never the session's compilation.
  Thinking goes back only to the model that wrote it. Store format 15.
- **rerank-live (32d) joined:** a live recall may wait `[memory] rerank_wait_ms` for Jev's order, which its `Recall`
  node holds. Headers stay per item.
- **The memory pass (31a)** labels `volatile` (`memory_pass::labels::volatile`) in `memory.labeled` rows keyed by
  node id: the one rule, never a second copy. LiveFact probes, which would re-derive a value, are filed.
- **The place rule:** a shared place recalls only its own sessions, so its headers name no other place.
- **39a's task view** is the last block of the request's last message, never a node: a class of its own.
- **compiler.rs is at its ceiling** (2,547 lines): new code in a module of its own (`compiler/situation.rs`).
- **The store's format:** `Compilation.situation` is stored: bump by one, with a sample of a compilation before it.
- **The books** (P8) start here: build none, add no node kind. A book's compile rule will be a class situations
  admit: name in the report the book each class feeds (notes and summaries: the diary). Only the operator writes an
  SOP: the precedence line is fixed text, never a model's or a note's.

**What to build (35a),** each a green commit:
1. **The situation:** a closed set (a conversation's first compile, a task's first compile, a continuation, a
   recompile with its trigger, a resume after a restart, a detour), decided in the compile step from what it holds (no
   new read), passed into `compile()`, which stays pure, and recorded on the compilation and `context.compiled`
   (TypeScript regenerated). Say how you tell a resume: a session's first compile this run, nothing new brought.
2. **What each admits:** a table in code after §2.11, built from what today's code admits (no turn that passes today
   fails; lessons reserved for 35b, row 63), checked after the compile by a pure pass over the request's pieces. A
   piece its situation does not admit, or a set that does not close (a result whose call is absent; an assembled
   `recall_id` whose node is gone), fails as a named outcome of its own, as `context_overage` does: nothing sent, the
   piece named. Today's repair stays: a call with no recorded result gets its synthetic one. A resume renders the
   manifest's prefix and recalls nothing until new inbound.
3. **The precedence line,** after the persona in the system header: "When sources disagree, trust them in this order:
   what this turn's tools just returned; the operator's current request; the recent conversation; older conversation
   and summaries; recalled notes, which are dated testimony." Static: one `system_changed` recompile per session.
4. **Testimony headers,** for items and summaries written from now on: as-of (date and position) and origin (place,
   author, a reply's model), as `(1) a reply by glm-5.3-flash in #harbor, 2026-09-30 14:34 UTC (as of @18231)`.
5. **Volatile values:** an item whose shown text holds one, by the labeler's rule, ends its frozen header `volatile:
   as of <date>, unverified`. Say how you read it: the source's row by key, or the rule over the shown text.

**Proof, offline:** the design's 35a tests: each situation admits its classes (table-driven); a set that does not
close fails, naming the piece; the precedence line costs one `system_changed` recompile, then appends; volatile
rendering; a resume rebuilds its prefix byte for byte (a new core on the same store) and recalls nothing. Also: old
`Recall` and `Summary` nodes render their stored bytes; route's detour test, the compaction tests and recall's place
property test pass; an older store opens; `a_plain_turn_stays_within_its_frame_budget` and `theseus-sim bench turn
--check`, both kinds. The compiler and recall tests 5 times under load. Planted reverts, each naming the test it
breaks: the closure check off; the volatile mark dropped; the precedence line moved to the tail.

**The live check is the maintainer's,** with a GLM key and `theseus-index` beside `theseusd`: exact commands for a
scratch daemon (a fresh state dir, Discord and the web off, `[memory] mode = "live"`, `arm = "baseline"`):
1. Session A: "The Pellworth harbor gauge read 3.2 m at 14:05 today, on firmware v2.4.1." Session B: "What did the
   Pellworth gauge read?" `theseus --json history <B>`: the item's header names A's place and ends `volatile: as of
   <date>, unverified`; `theseus --json ledger --kind context.compiled -n 1` names the situation.
2. A restart, then B again: a continuation that appends, and `provider.call`'s cache reads cover B's prefix.
3. On a store the previous build wrote, a turn recompiles `system_changed`, the next appends, and the model quotes
   the precedence line when asked how it weighs sources that disagree.

**Leave alone:** tiering (33, beside you: the transcript read, stubs, the heat cache): keep to the compile's admitted
set; consolidation (31b): its `Synthesis` header arm joins yours at the second merge; retention and activation (32a,
32b: recall's candidates and arms); the task board (39b: the view's lines gain claims); route's and rerank's waits;
gliding (38b, a local lane: places, publish, the outbox); the cockpit (say what its context view should show).
