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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-threads-cross`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of `main` for this wave (it wins over anything below about `main`)

This task was written against an older `main`. Your branch starts from `main` at `1d092ded` (2026-10-20). Where the
text below names `bbc65530`, a store format, line ceilings or "work in flight tonight", this section is right and the
text below is stale. Line numbers quoted below may have moved: search by name.

**Store format on your base: 47** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 48 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 48 at once; the maintainer renumbers at the join, so never
write the number as a literal anywhere but the constant and its tests.

**What landed since `bbc65530`** (67 merges; `git log --first-parent bbc65530..HEAD` lists them). The ones most likely
to touch your area:
- Google part 1 (`f897d6b4`, with join fixes `897c3ea7`, `26b3d91b`): the `theseus-google` crate, `[google]`,
  sign-in, `google.call` (it classifies the filled path and refuses a query or an escape in a raw path).
- The data rule (`63596193`): sensitive names go only to models of US origin, failing closed; a routed turn that keeps
  failing asks and moves up a model; Jev warns on fishing for personal details.
- Turn-end facts (`9b4d2a33`, join fixes `dfd3b55e`, `69c8d74a`): a command's result names the new files it left; a
  turn about to end beside them is told once.
- Bench measurement (`5e0bd195`), the first request's persona layer per place (`fa5832c7`), the tool catalog built
  once (`17d6deec`), `blocking` as one function (`6c1332c1`).
- Needs-you cards (`37d1edb2`), the task home `#theseus-tasks` (`cf243efe`), agent answers labelled (`32512fbd`),
  self-improvement fails open (`051fc186`), a tool `tools_find` finds is callable in the same turn (`05655588`).
- Voice: echo cancel (`6bdfe9b1`), the join guard (`5a7bcbb9`), Jev checks an unclear transcript (`a86a4ade`).
- The ontology (`dc2f5f71` core, `c7ee0bb7` Jev files each inbound message, `38745067` pictures, `3a75e094` it
  reaches routing, context and recall, `7895bb45` the nightly sweep, `580c8c37` the labelled replay and the +onto
  bench arm, `1d092ded` the cockpit's Ontology view).
- Threads: a place's home splits into threads (`fd1f2e51`); a thread's recall no longer pulls another stream's facts
  (`eb43b3d4`, the thread lens); a thread adopts its task's running turn (`76237a96`).
- Tests made hermetic on fast hosts (`76237a96`), load-flaky tests ordered by events (`a2f8d276`).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room)
- `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (three lines of room)
- `crates/theseus/src/render.rs` 3,115 / 3,128
- `crates/theseus-discord/src/render.rs` 2,976 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/tests_m3.rs` 7,979 / 8,050
- `crates/theseus-core/src/turn.rs` 3,404 / 3,525
- `crates/theseus-index/src/vectors.rs` 2,561 / 2,650

Put new code in new files (a module of its own, one `mod` line in the parent). New wire types go in a protocol module
file of their own, re-exported with one line. Never raise a ceiling yourself (the keel guard flags it). If a line you
truly need will not fit under one, leave the file one line over, and say so in your report's first section with the
file and the reason: the maintainer raises it at the join, where the other rows' lines are counted too.

**Work in flight beside you** (other sessions on other branches, all from this same base; stay out of their files where
you can, and where you can't, keep your edit small and additive so the joins merge):
- Google part 2 rows: gw-mail, gw-calendar, gw-drive, gw-sheets-slides (`crates/theseus-google/`, each its own
  service module, `fake/<service>.rs`, one registration line each).
- Harness rows: job-truths (job command lines, `theseus-kernel/src/job.rs`, fs/file read results), effort-on-evidence
  (turn checkpoints' effort), turn-end follow-ups and long blind waits, recall abstention and the `service` orphan.
- Threads rows: routing (`crates/theseus-core/src/thread_route.rs`, working sets, claims) and cross-stream reach
  (`theseus-memory/src/threads.rs`, `thread_recall.rs`, a place digest, Jev's near-miss check, thread links).
- Ontology follow-ups (`crates/theseus-ontology/`, the sweep, the dream's guidance, the replay), test-infrastructure
  rows (Discord task-thread tests, leaking shell loops), the China-origin model rows (`data_gate.rs`, session data),
  rough edges in progress words, detours and `theseus why`.
- On the maintainer's machine: task questions and reports (`owner_ask`, `turn/join_step.rs`, `task.close`), the TUI
  chat (`crates/theseus-tui`), herdr (`contrib/herdr`).

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

---
## Your task: threads-cross, a thread reaches the other streams on purpose, labelled, and never by accident (theseus-98bz.9, theseus-98bz.8, theseus-98bz.10, theseus-98bz.11)

Branch: `cloud/20261021-threads-cross`. Every commit's subject carries the `theseus-<id>` of the plan row it
implements (each row below names its id). Deadline for the report: 6 hours after you start.

**Why.** Activity threads keep separable streams of work (a PR round, a security fix, a data pipeline, a home-server
chore) apart, and the thread lens (theseus-wncdv) stopped one stream's facts from leaking into another's recall: a
question about one stream's staging port no longer gets another stream's port. The synthetic threadbench (plan T12,
seed 11, k=3) shows the cost of how it does that, each a thing a person hits:
- **"What else is going on this week" sees nothing.** Four probes ask for an "also this week" section ("Last touch on
  PR_DESCRIPTION.md: add an 'also this week' section covering anything else we ..."). One session (arms A3, A4) names
  the three other non-sealed streams in 12 of 12 sections; the threads arms do it in 1 of 18, and mostly write no
  section at all ("This session only covers the four review comments"), though they often know the other work. There
  is no deliberate cross-stream path: the only way another stream's note reaches a thread is to pay the lens's
  distance or to be named by slug or title.
- **A note the thread already holds keeps out the one fact it lacks.** "The cutoff for flagging changed: use 0.55
  from now on." ran at home; the next message, "Regenerate the report with the new cutoff.", ran in the pipeline
  thread and answered "you haven't given me the new cutoff" in 6 of 6 runs. In its recall the home note holding
  "0.55" (fused 0.097, the #2 of 40 candidates) was dropped by the lens: its share 0.097/0.104 = 0.93 clears the 0.75
  bar, but paid = 0.097 x 0.65 = 0.063 is at or under `own_top` 0.104, and 0.104 is the thread's own top candidate,
  **a note already in context** (dropped `in_context`). Fused scores are reciprocal-rank scale and close together
  (0.08-0.10), so a penalised cross candidate almost never beats `own_top`.
- **Real near misses and same-shape facts are dropped alike.** The lens cannot tell "this other-stream note answers
  the question" from "this other-stream note is the same kind of fact" (another stream's port), which is exactly the
  contamination it was built to stop. Jev, already in the loop on every message, can.
- **Related streams cannot be declared related.** Two streams the owner knows belong together still pay the distance
  on every recall.

**Risk: medium-high.** Row 8 loosens the lens's main guard and rows 9 and 10 add new ways for another stream's facts to
reach a thread. Isolation is the reason threads exist: a sealed thread's title, goal, card, entities and notes never
reach another thread by any path this row adds, and every cross-stream line the model sees is labelled with the thread
it came from. Each row has its own switch, and each switch off restores today's behaviour byte for byte.

**Prerequisite: `main`.**

## Order and size
**Must do, in this order: row 9, then row 8, then row 10. Stretch: row 11.** Row 8 ships only together with row 9 or
row 10 (the plan's rule: it loosens the guard, so it lands beside a deliberate path): commit it only after row 9 is
committed. Commit each row separately (its own commit or commits, its own id in the subject), with its tests and
plants green before you start the next. A row you do not reach is named in the report as not started; a row half done
is reverted from the branch, never committed half done.

## Your rows (names as on `origin/main`)

### Row 9: a cross-scope request gets a place digest, classified by Jev in the loop (theseus-98bz.9)
- **The classification.** Jev already answers `classify.v3` for every inbound message in a place with live threads
  (`crates/theseus-judge/packs/classify.v3.toml`; `CLASSIFY_PACK` in `crates/theseus-core/src/judge/inbound.rs`; the
  `thread` question is asked `only_when = "threads"`). Add a new pack version `classify.v4` (copy v3, keep v3
  embedded for its history as v2 is; follow `crates/theseus-judge/AGENTS.md` for a pack bump: the golden request,
  the state, the eval fixtures) with one more question, `scope`, a choice of `thread` (the message is about this one
  line of work) or `place` (it asks across everything going on: "anything else we are shipping this week", "what else
  is going on", "status across everything"), asked only when the place has live threads. This rides the request that
  is already made: no new call. The owner's design thesis for Theseus is that fast in-loop classification by Jev
  makes the harness stronger; this is one such use.
- **Also in `classify.v4`**, from the routing plan's row 5: the `thread` question's instructions state that a persona
  cue at the start of a message ("Security view:", "Plain words:") picks a voice, not a stream (a PR-round comment
  that began "Security view:" was picked as the security-fix thread at p 0.75). One plain sentence, a fact, no
  example list. The routing row on the other branch changes no pack: this is yours.
- **Reading it.** The inbound point is begun before the session is known and the turn takes it over (`Handoff`;
  header of `crates/theseus-core/src/rpc/thread_flow.rs`; the turn side is in `crates/theseus-core/src/turn/
  inbound_step.rs`). Read `scope` where the turn already reads its inbound judgment. If the answer is not in by the
  time the turn compiles, wait for it at most `[threads] route_wait_ms` and only in a threaded session; on a timeout
  there is no digest. Find out whether a turn addressed to a thread explicitly (`ask --thread`) gets a `classify`
  answer at all, and say in the report.
- **The digest.** On `place`, the turn's context gains a **place digest** for that turn only: one line per other
  live, unsealed thread of the place, labelled `from thread «title»`, with its goal, its card's last line and its
  dated card items, each line capped (`[threads] place_digest_line_chars`); a sealed or seal-pending thread appears
  only in a count, "1 embargoed thread (not shown)". A new compiler module (e.g.
  `crates/theseus-core/src/compiler/place_digest.rs`, one `pub mod` line in `crates/theseus-core/src/compiler.rs`).
  Put it in the turn's own part of the request, after the last cache mark (where the turn's recalled notes go), never
  in the cached layers: the thread card (`compiler/card.rs`) is a snapshot kept with its compilation and the prefix
  must be the same bytes with and without a digest. If the compilation's stored manifest must record it, the
  store-format rule above applies.
- Switch: `[threads] place_digest = true`.

### Row 8: `own_top` counts only what can still be admitted (theseus-98bz.8)
- `crates/theseus-core/src/thread_recall.rs` `ThreadScene::lens` (near :202) computes `own_top` over every candidate
  the asking thread owns (near :224-234), including ones already in context. Compute it over the thread's own
  candidates that are **not** in context. The in-context set is on the recall scene
  (`crates/theseus-core/src/recall.rs` `Scene.in_context`, near :675); `Scene::lens` (near :699) is the one caller
  that builds the lens from candidates: pass the set through there.
- `beats_own`, the bar, `needs_entity` and `base` stay as they are. Update the `own_top` field's doc comment in
  `crates/theseus-memory/src/threads.rs` (near :88).
- Switch: `[threads] own_top_admissible = true`.

### Row 10: Jev judges the near-miss cross candidates (theseus-98bz.10)
- After the lens, take up to K (`[threads] cross_recall_judge_k = 3`) of the recall's drops that the lens made
  (reason `thread` on your base) whose share of `top_fused` is above `bar`, best first, and ask Jev in one call with
  the message: is this note about the same subject the message asks about, not just the same kind of fact? yes or no.
  A new pack `recall.cross.v1` (`crates/theseus-judge/packs/`, a builder in `crates/theseus-judge/src/builders/`,
  its golden request), and a step of its own beside `crates/theseus-core/src/turn/rerank_step.rs` (the existing Jev
  call over a recall: copy its shape; `Memory::refill` packs again with the same filters and budget).
- A yes is admitted **with the penalty**, its provenance header labelled `from thread «title»` (the lens's
  `other_title` already names it). Wait at most `[threads] route_wait_ms`; on a timeout or an error the drops stand as
  today. Skip the step when row 9's digest fired this turn. The call's cost is in the existing `judge.call` ledger
  rows.
- A sealed or seal-pending thread's candidate is never sent (it is dropped `sealed` before this step). Never put such a
  node in the admitted set.
- The lens hook: add one field at the end of `ThreadLens` (`crates/theseus-memory/src/threads.rs`) holding the
  node ids Jev admitted, and one early return at the very top of `judge` that gives those nodes
  `Verdict::Costs(1 - penalty)`. Nothing else in `judge`: the routing row on the other branch owns its rules (it
  splits `Verdict::Distant` and `Reason::Thread` by cause).
- Select the thread drops through one helper of your own (e.g. `fn is_thread_drop(r: Reason) -> bool`, matching
  `Reason::Thread` today): at the join `Reason::Thread` becomes `Reason::Thread(cause)` and only that line changes.
- Switch: `[threads] cross_recall_judge = true`.

### Row 11 (stretch): owner-confirmed thread links (theseus-98bz.11)
- `crates/theseus-protocol/src/threads.rs` `ThreadInfo` gains `links: Vec<String>` (serde default, skipped when
  empty). `ThreadInfo` is a stored record (META `thread:<slug>`, a format-33 literal in
  `crates/theseus-core/src/tests_layouts.rs`): follow the store-format rule above.
- `theseus thread link A B` and `theseus thread unlink A B` (`crates/theseus/src/thread.rs`), through
  `thread.edit`'s params (`ThreadEditParams` in `crates/theseus-protocol/src/threads.rs`; the RPC in
  `crates/theseus-core/src/rpc/threads.rs` `thread_edit`, near :479) so no method name is added to
  `crates/theseus-protocol/src/lib.rs`. A link is written on both threads. Linking a sealed or seal-pending thread
  is refused with a message that says so. Ledger rows `thread.linked`, `thread.unlinked` (new kinds right after
  `ThreadUnsealed` in `crates/theseus-protocol/src/ledger.rs`).
- In `ThreadScene` (`thread_recall.rs` `thread_scene`, near :257), the asking thread's linked slugs join `waived`:
  their candidates pass with no penalty.
- Jev may propose a link once per pair, as an ask, after row 10 admitted notes between the same pair twice; the
  "asked once" mark persists (a META key). Leave the proposal to a follow-up if time is short: the link command is
  the row.

## What already exists (do not redo)
- The lens and its rules (`eb43b3d4`), `named` waivers by slug, title or working-set entity
  (`thread_recall.rs` near :116), the ontology category boost (theseus-jrf4c; it multiplies kept candidates only,
  after the lens), Jev's rerank of a recall (`rerank.v1`), `classify.v3` and `route.v7`.
- Tests: `crates/theseus-core/src/rpc/tests_thread_mix.rs` (the contamination set: every case must pass unchanged),
  `rpc/tests_thread_recall.rs`, `crates/theseus-memory/src/tests_threads.rs`, `crates/theseus-judge/src/
  tests_threads.rs`.

## Tests (each with a planted revert; the first is the key test)
- **`a_place_digest_never_shows_a_sealed_thread`** (row 9, a compiler or rpc test): a place with four live threads,
  one of them sealed and one seal-pending, and a turn whose scope is `place`: the digest holds one labelled line for
  each of the two open other threads and the count line, and no byte of the sealed or seal-pending thread's title,
  goal, card lines or entities appears anywhere in the request. Plant: list every thread. This test must be shown
  failing on that plant.
- Row 9 also: a `thread`-scope turn and a timed-out answer get no digest; with `place_digest = false` the request is
  byte-identical to today's; the digest for five threads is at most 400 tokens; the cached prefix is the same bytes
  with and without a digest; the pack's golden request for `classify.v4` and the eval fixture cases for the
  maintainer's Jev eval (the four "also this week" probe texts from `bench/threads/th_beats.py` beats `pipe.11`,
  `pr.11`, `qh.9`, `ops.8` → `place`; "what's left on the tarnlib PR?" → `thread`; a "Security view:" PR comment
  keeps its stream). Plant: ignore `scope`.
- Row 8: the msg-113 numbers (a cross candidate at 0.097 against an in-context own note at 0.104, top 0.104, no other
  own candidate) now give `Costs`; an own candidate **not** in context above the paid score still drops it; every
  `tests_thread_mix.rs` case passes unchanged; with `own_top_admissible = false` the old verdicts are back. Plant:
  count in-context notes again.
- Row 10 (with the fake judge, `crates/theseus-judge/src/fake.rs`): the "0.55" home note answers "Regenerate the
  report with the new cutoff." → yes, admitted with the penalty and labelled; another stream's port for a "which port
  is the staging copy on?" question → no, stays dropped; a timeout drops as today; K caps the call at three notes; a
  sealed thread's candidate is never in the request; no call when the digest fired. Plant: admit without asking.
- Row 11: a linked pair's cross candidate passes with no penalty; unlinked, as before; linking a sealed thread is
  refused; unlink restores the distance. Plant: drop the waiver.

## Not in this row
- The threadbench run. **Bench measurement (threadbench, any arm, any k) is run by the maintainer after the join,
  never by this session: the VM has no API keys.** Jev's eval fixtures are written here and run by the maintainer.
- The scorer (`bench/threads/th_oracle.py`, `th_score.py`, `report.py`): fixed separately, in plan row 1.
- The routing row's files, which run beside you on another branch from the same base: `crates/theseus-core/src/
  thread_route.rs`, the ask and its answer (`thread_flow.rs` `answer`, `rpc/thread_flow.rs`, the CLI's
  `thread/ask.rs`, Discord's `runtime/thread_ask.rs`), working sets growing from turns, claims of home turns
  (`moved.of.<node>` written for a claim, which your lens reads unchanged), the turn-end hook
  (`thread_routed_turn`), `TurnSubmitParams.route_at_ms`, and the drop causes (`Verdict::Distant`, `Reason::Thread`
  and `RecallDrop`'s new numbers). Shared files, kept mergeable: in `crates/theseus-core/src/config/threads.rs` and
  the template `crates/theseus-core/config/theseus.example.toml` put your keys right after `base_distance` (that row
  puts its keys after `ask_wait_secs`); in `ledger.rs` add your kinds after `ThreadUnsealed` (that row adds after
  `ThreadUnmerged`); in `crates/theseus-memory/src/threads.rs` touch only the `own_top` doc comment, the one new field
  at the end of `ThreadLens` and the one early return at the top of `judge`; the regenerated
  `cockpit/src/protocol.gen/` is regenerated again at the join.
- A recall-abstention row is in flight in the recall step: keep your edits to `crates/theseus-core/src/recall.rs` to
  `Scene::lens` and to `turn/recall_step.rs` to the one call of your new step.
- The cockpit's "why" view of a digest, and the TUI: name them as follow-ups.
- `docs/`, the spec, `docs/status.md`, the README: say in the report what should change.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** Each behaviour ships on by default. Each row's switch is its rollback: a
  test shows that with it off the old behaviour is byte-for-byte back.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). The digest is per turn, only on a
  `place` answer. This row adds no model tool. Model-facing text states facts (what exists, where it came from), never
  instructions, never "must", never examples. FAST: nothing new on a plain turn
  (`tests_m3::a_plain_turn_stays_within_its_frame_budget` passes unchanged, and the gate's turn bench must not move);
  an unthreaded session waits for nothing new; nothing on the start path grows with history.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty unless this task names keel findings.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. A store format change only
  as the section above says.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name anywhere in the repo,
  in commit messages or in the report: write "the owner". Fixture paths and names are invented (the bench's
  `tarnlib`, `datatidy`, `quillhn`, `ledgerweb`, `homesvc` are fine).
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (`theseus-core`, `theseus-memory`, `theseus-judge`,
  `theseus-protocol`, `theseus`). Keep CLOUD_REPORT.md honest: what you ran, what passed, what you could not run on
  the VM (L1 tests need a non-root daemon with a job cgroup; this row should need none; Jev evals need keys) and the
  exact commands for the maintainer.

## Your report
Commit `CLOUD_REPORT.md` at the repository root, written with your editor tools: what changed, per row (done, not
started, or reverted); each test and each plant with the test that failed; what could not run on the VM and the exact
commands for the maintainer (the `classify.v4` and `recall.cross.v1` evals included); the keel guard's output; the
store format if bumped; the follow-ups; and the threadbench numbers the maintainer should look at after the join
(plan targets, row-1 scorer: reach on the "also this week" sections at or above 5 of 6 for A1 on the PR-round and
pipeline streams, sealed leaks 0, embargo probes 4 of 4; cross-stream contamination at or under A3 + 0.5 per 12 and
owner-check contamination at or under A3 + 0.5 per 26; cross cites at or above 11 of 12; request tokens at or under
45% of A3's, so the digest averages about 1k tokens at most; Jev's cents at or under 10% of the run's dollars; p50
turn latency up at most 1 s; the pipeline summary right in 3 of 3; and Jev's yes rate on drops the scorer marks as
contaminating). The plan's stop rule: if contamination moves above its bar, row 8 is the first thing turned off.
Push the branch, check `CLOUD_REPORT.md` is not empty in the pushed commit, and end with a final message that starts
`CLOUD REPORT COMPLETE`.
