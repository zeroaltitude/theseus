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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail7d-contam`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `f9c11594` (2026-10-22: release stage 7a, install #74a, the card hotfix, on top of
stage 6's `ae420a49`); that is your base. Stage 7a changed only the card's files, `docs/status.md` and one core
`AGENTS.md` paragraph (the owner card), so every line number below, quoted at `ae420a49`, still holds within a few
lines: search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). Neither row in this
wave needs a bump: the misroute guard reads the stored working sets as they are, and the thread lens stores nothing. If
you find you need a stored field after all, bump it to 50, add the old layout's literal sample to
`crates/theseus-core/src/tests_layouts.rs` as AGENTS.md says, and never write the number as a literal anywhere but the
constant and its tests.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). An unlisted file stays at 2,500 lines or
fewer. Every file these rows touch is unlisted and under that: `toolrun/misroute.rs` 288, `rpc/thread_flow.rs` 853,
`rpc/tests_thread_route.rs` 1,686, `thread_recall.rs` 370, theseus-memory `threads.rs` 368, `rpc/tests_own_top.rs` 175,
`rpc/tests_thread_mix.rs` 224, `config/threads.rs` 549. Never raise a ceiling yourself (the keel guard flags it).

**Work in flight beside you** (rows on `ae420a49` joining the lane `integ/tail7`): the ontology (`judge/onto/*`), effort
routing, the TUI, `tests_hold_compile.rs`, the recall bench, `scratch.rs`, `turn/data_step.rs`, `rpc/mod.rs`
(`build_telemetry`), external text and holds (`external.rs`, `toolrun/order.rs`), budgets (`turn/defer_step.rs`,
`turn/detour_step.rs`, `config/threads.rs`'s working size, `compiler.rs`, `compiler/cold.rs`, `thread_context.rs`),
injection tiers (`recall.rs`, `turn/recall_step.rs`, `turn/rerank_step.rs`, `config/memory.rs`, `turn.rs`), drafts,
message texts, the config template and Discord. Stay inside the files your task names. If you must edit a file outside
that list, name it and the reason in your report's first section.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail7d-contam, find which change let other streams' notes into a thread's recall on #74, and bring admission back near T12 without losing the 98bz rows' intent (theseus-stwkb)

Branch: `cloud/20261021-tail7d-contam`. Every commit's subject carries `theseus-stwkb`. Deadline for the report: 4 hours
after you start.

**Why (P2, a plan bar the threads lane must meet).** Threadbench T13 at k=3 on install #74 (2026-10-22) measured
cross-stream contamination above the threads plan's bar. The bar is "cross-stream contaminated ≤ one session (A3) + 0.5,
owner-check contamination ≤ A3 + 0.5"; the plan says to stop and report when a lane crosses it. Means over 3 runs:

| arm | cross contaminated /12 | owner-check contamination /26 | request tokens |
|---|---|---|---|
| A1, threads | 5.3 | 3.3 | 30.9k |
| A1o, threads + ontology | 3.3 | 1.0 | 28.6k |
| A3, one session | 0.7 | 0 | 58.8k |
| T12 A1 (before, eb43b3d4) | 2.0 | 1.7 | 26.2k |

The replies stay right, but they name other projects' details from "the recalled notes" while declining them. For
example, A1 r1 msg 28: *"The recalled notes this turn give other ports (8453, 8443, 9443). They belong to the homesvc,
ledgerweb and tarnlib threads, so I didn't use them."* That is what the contamination scorer counts. It is also what grew
request tokens by 18%.

**The evidence you cannot reach** (the bench's saved store WALs live on the owner's machine; these numbers were read
from them for you). Each `recall.ran` row carries `admitted[]` (node, session, fused) and `dropped[]` (with a reason).
Here an admitted note is counted by whose session it came from, against the asking turn's thread:

| run | recall rows in a thread's session | admitted from **another thread** | admitted from home / no thread | admitted from its own thread | lens drops: thread_entity / thread_base / thread_bar / **thread_own** |
|---|---|---|---|---|---|
| #74 A1 r1 | 120 | **278** | 41 | 0 | 746 / 486 / 365 / **0** |
| #74 A1 r2 | 117 | **206** | 44 | 0 | 794 / 574 / 346 / **0** |
| #74 A1o r1 | 127 | **279** | 30 | 0 | 988 / 385 / 461 / **0** |
| #74 A1o r3 | 125 | **257** | 40 | 0 | 942 / 517 / 424 / **0** |
| T12 A1 r1 (eb43b3d4) | 116 | **6** | 9 | 0 | 2,427 (one `thread` cause then) |

More facts from the same runs:
- **No thread's own note is ever admitted.** A thread's own hits are already in its session's context, so they drop
  `in_context` (2,558-2,707 such drops a run). That was true at T12 as well.
- **`thread_own` drops are 0** in every #74 run checked. Not one of about 1,600-1,900 lens drops a run comes from
  `cross_recall_beats_own`.
- **Jev's `recall_cross.v1`** (98bz.10) in A1 r1: 79 calls. Its `same_subject` answers were yes at the confirm band 29
  times, at act 15 and at escalate 10 (no: act 85, confirm 49, escalate 2). So Jev admitted at most about 44-54 of the
  278.
- **Links** (98bz.11): no thread in A1 r1 has `links` set. A link waiver cannot have fired there.
- **Working sets** (98bz.5) now hold relative roots: `tarnlib`, `datatidy`, `ledgerweb`, `quillhn`, and for the homesvc
  thread both `homesvc` and **`notes`** (a generic folder every stream writes to). At T12 every working set was empty.
  homesvc's thread is the largest source of cross admissions in A1 r1: 26-29 notes into each of the other five threads.
  21 of the 143 messages say "notes", and 18 say "homesvc".

**The leading suspect, from reading the code: theseus-98bz.8** (a5838e1a, "own_top counts only the asking thread's notes
that can still be admitted"). `own_top` is now taken over the thread's own candidates that are **not** in context. In a
thread's own session those are all in context, so `own_top` is 0, and `beats_own` (`paid <= own_top`) never drops
anything. The bar that held other streams' same-shape facts out at T12 is gone. That matches thread_own = 0 above.
98bz.8 was meant for one case: msg 113, where a **home** note held the new cutoff and the thread's in-context note kept
it out. On #74, msg 113 still fails in 5 of 6 runs.

The secondary suspects:
- 98bz.10: Jev admits some notes, at most about 50 a run.
- 98bz.5's generic roots: `notes` in one thread's working set. The lens's naming waiver (`waived`: the message names a
  working-set entity as a whole word) may let that thread's notes pass unpaid into any thread whose message says
  "notes".
- 98bz.11: ruled out for A1 r1 by the data. Confirm by reading.

**Risk: medium.** This row changes which notes a thread's recall admits. Every verdict change must be pinned by a test,
and the existing lens tests must pass unchanged unless the report shows why one is wrong.

**Prerequisite: base `origin/main` at `f9c11594`.** Your branch joins the lane `integ/tail7` (stage 7, install #75), after
`tail7d-misroute`. No bench runs in the cloud: the conductor reruns the threadbench on #75.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail7d-contam` | theseus-stwkb | bisect admission by unit-level tests on the lens; fix the cause so admission comes back near T12 while msg 113's home note still gets in; a fixture test pins the admitted count | core `thread_recall.rs` (`ThreadScene::lens`'s `own_top`, near :240-290), theseus-memory `threads.rs` (`ThreadLens`, `judge`, docs), core `rpc/tests_own_top.rs`, `rpc/tests_thread_mix.rs` (and `tests_cross_judge.rs` only to add a case), the `own_top_admissible` doc in core `config/threads.rs` (near :39, doc only), core `AGENTS.md` (the thread-lens paragraph, near :548-560), theseus-memory `AGENTS.md` (only if its lens line becomes untrue) | M |

## What exists (at `f9c11594`; search by name)
- theseus-memory `threads.rs` (368 lines). `ThreadLens` (near :75) has `waived`, `top_fused`, `own_top`, `beats_own`,
  `needs_entity` and `admitted` (Jev's yes, 98bz.10). `judge(session, node, fused, tie)` is near :170: the waiver and the
  distance early return (near :189), then the bar, then `needs_entity && tie == Tie::Apart` → `thread_entity`, then
  `beats_own && paid <= own_top` → `thread_own` (near :203), and `base` for notes no thread claims.
- core `thread_recall.rs` (370 lines). The module doc is near :1-40. `ThreadScene::lens(candidates, in_context)` (near
  :240) builds `of_session`/`of_node`, `top`, and `own_top` with the 98bz.8 filter
  `!(self.own_top_admissible && in_context.contains(node))`. The waiver set (slug, title, working-set entities named
  as whole words) and the linked threads (98bz.11, `Board::linked_with`) are built here too. `TurnRunner` fills the
  scene from `[threads]` near :290.
- core `recall.rs` near :700. `Scene::lens` hands the lens the candidates and `&self.in_context`.
- core `turn/cross_step.rs` (188 lines): the `recall_cross.v1` ask, and `ThreadLens.admitted`, then `Memory::refill`.
- core `config/threads.rs`: `own_top_admissible` (true; doc near :39, field near :152, default near :252),
  `cross_recall_beats_own`, `cross_recall_needs_entity`, `cross_recall_bar`, `base_distance`, `cross_recall_judge`,
  `link_proposals`.
- Tests: `rpc/tests_own_top.rs` (175 lines; the msg-113 shape and the switch-off case), `rpc/tests_thread_mix.rs` (224
  lines), `rpc/tests_cross_judge.rs`, `rpc/tests_thread_link.rs`, and theseus-memory's lens unit tests in `threads.rs`
  (near :240-300).
- The commits to read: a5838e1a (98bz.8), d3365737 (98bz.10), c6a7fadc and 86680a12 (98bz.11), and f59fd379/701d521a
  (the join fixes that gave `Reason::Thread(Cause)`). `git log --oneline eb43b3d4..f9c11594 -- crates/theseus-core/src/thread_recall.rs crates/theseus-memory/src/threads.rs`
  lists anything else that touched the lens.

## What to build
1. **Bisect by test first, then fix.** Build one fixture at the lens level, through `ThreadScene::lens` and `judge`, or
   through `Core` if the rig is cheap (`tests_thread_mix.rs` is the model). Use five live threads, each with a working
   set like the bench's (relative roots, homesvc's including `notes`). Each thread's own notes are in context, and each
   other thread has a same-shape note: a port, a path or a config value, sharing the message's entity where the bench's
   did. Count admitted vs dropped by cause for a handful of messages: one naming its own project, one saying "notes",
   and one asking the msg-113 shape, where a home note holds the answer. Then flip each suspect alone and record the
   admitted count:
   - `own_top_admissible` false;
   - the waiver without working-set roots;
   - the cross judge off (`ThreadLens.admitted` empty);
   - links none.

   Put the table in the report. It is the row's bisect. If the table points somewhere other than 98bz.8, follow it and
   say so.
2. **Fix the cause, keeping 98bz.8's intent.** The default, if the bisect confirms 98bz.8: `own_top` (the in-context
   best included, as before 98bz.8) still holds back a note of **another live thread**. The admissible-only `own_top`
   applies only to a note **no thread claims** (the home/base case, msg 113's), so the home note with the new cutoff is
   still admitted. Write it in the smallest form: for example, two values on `ThreadLens` (`own_top` and
   `own_top_admissible`), with `judge` picking by the candidate's provenance (`base` vs another thread). Keep
   `[threads] own_top_admissible` as the switch (false = the pre-98bz.8 verdicts everywhere), and change its doc to
   match. **No new config key** unless the fix cannot be expressed without one: `config/threads.rs` is edited by another
   row in flight, so a new key is a merge for the integrator. If you add one, name it in the report's first section.
3. **If the bisect shows a second cause** (the `notes` root in the naming waiver, or Jev's admissions), fix it only if
   the fix is small, local to the lens and clearly right. Examples: the waiver ignores a working-set root that another
   live thread's set also names; Jev's yes still has to beat `own_top`. Otherwise report it with numbers from your
   fixture as a follow-up. A change to how a working set grows (`thread_grow.rs`) is not this row's.
4. **If the honest answer is a design trade** that the owner must call (for example, "admit fewer cross notes" against
   "tell the model not to narrate declined notes"; the bench cannot tell them apart), say so plainly in the report's first
   section. Change nothing risky, and still land the fixture and the bisect table as tests that document today's
   behaviour.
5. **Docs.** The lens paragraph in core `AGENTS.md` (near :548-560), `thread_recall.rs`'s module doc, and the
   `ThreadLens.own_top` doc say which `own_top` holds back which notes.

## Tests (red first: write each, see it fail on the base code, then fix; each fix also gets a planted revert)
- **`another_threads_note_is_held_back_when_the_thread_holds_its_own_in_context`** (key). The asking thread's best own
  hit is in context (fused 0.104). Another thread's same-shape note shares the message's entity and clears the bar, but
  pays down to or under 0.104. It drops `thread_own`. On the base code it is admitted: that is the red.
  Plant: take `own_top` over admissible notes for every provenance again. The test fails.
- **`the_home_note_with_the_answer_still_gets_in`** (the msg-113 shape). The existing `tests_own_top.rs` case(s) must
  pass unchanged. If the existing one does not isolate the home/base provenance, add one that does.
  Plant: apply the whole `own_top` to base notes too. It fails.
- **`the_fixture_admits_near_t12`**. Pin the fixture's admitted count from another thread, and its drops by cause, as
  exact numbers. Write in the test's doc what T12-like means here: on the base code the count is high, and after the fix
  it is the pre-98bz.8 count, or near it, with the home note still in. A future change that lets other streams back in
  turns this test red.
- **Switch**: `own_top_admissible = false` gives the pre-98bz.8 verdicts on the fixture (the old behaviour, pinned).
- Every existing lens, cross-judge, link and thread-mix test passes unchanged, or the report says which assertion
  changed and why it was wrong.
- Run `cargo nextest run -p theseus-memory threads`, `-p theseus-core own_top`, `-p theseus-core thread_mix`,
  `-p theseus-core cross_judge`, `-p theseus-core thread_link`, then `-p theseus-core` whole.

## The row beside you
`tail7d-misroute` (theseus-4gb4w) fixes the misroute guard, which compares relative working-set roots with absolute
paths. It is recommended to resolve roots at the guard's arm time and leave stored working sets as they are. If it does,
nothing you read changes: the lens's naming waiver still matches `tarnlib` and `notes` as words. If it stored roots
absolute instead, the waiver's whole-word match would change. Check `origin/cloud/20261021-tail7d-misroute` if it has
pushed. In your report, name whether the two interact.

## Not in this row
- `recall.rs` and `turn/recall_step.rs`: another row in flight (injection tiers) edits them, sizing recall and its
  `admit_ceiling` by tier. Stay out of them. If you must pass something new through `Scene::lens` (near :700 of
  `recall.rs`), keep it to that call, and name the overlap in the report's first section.
- `thread_grow.rs`, the router, the misroute guard, `turn/cross_step.rs`'s request (`recall_cross.v1` pack text and
  golden), the bench (`bench/`), the config template, Discord, and every file of the rows in flight that the wave note
  names.
- No bench run. Measurement happens on #75.

## Rules for this row
- Model roles: this row needs judgment. Read before you change, and let the bisect decide. Keep the diff minimal. Every
  line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49: the lens stores nothing, so there is no bump.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented (the bench's project
  names are invented too, and may be reused).
- Read `crates/theseus-core/AGENTS.md` (recall, the thread lens, a thread's working set, threads linking) and
  `crates/theseus-memory/AGENTS.md` first. Run `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds:
- **first section:** the cause in plain words (which change raised admission, by how much on the fixture), and whether
  any part of it is an owner's call;
- the bisect table (each switch alone → admitted from another thread, drops by cause);
- the fix: which `own_top` holds back which notes, and why msg 113's home note still gets in;
- any second cause, and what you did or filed;
- whether `tail7d-misroute` interacts;
- each test, and its plant with the failing line, the red-first run on the base included;
- the keel guard's output, the store format, and any file outside your list (with the reason);
- a `docs/status.md` sentence for the integrator;
- what the threadbench rerun on #75 should show if the fix is right (cross contaminated and owner-check contamination
  back near T12, thread_own drops above 0, admitted-from-another-thread near T12's tens rather than hundreds);
- follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
