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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail3-threads`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from the integration branch `integ/threads` at `be6ef45f` (2026-10-22: `main` `6918e3c1` plus threads rounds 1-3, round 3 green); that branch, not `main`, is your base. Where the
text below names `bbc65530`, a store format, line ceilings or "work in flight tonight", this section is right and the
text below is stale. Line numbers quoted below may have moved: search by name.

**Store format on your base: 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 49 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 49 at once; the maintainer renumbers at the join, so never
write the number as a literal anywhere but the constant and its tests.

**What landed since `bbc65530`** (77 merges; `git log --first-parent bbc65530..HEAD` lists them). The ones most likely
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
- Since 1d092ded (`87696a7c`, 2026-10-21): task questions and reports never stall a task (`owner_ask`, `turn/join_step.rs`, `task.close`; `afa16ac5`); `theseus chat` answers the router's question and keeps an explicit session (`5f36ef45`); herdr actions take the focused pane (`6f8487f2`); the threads bench scorer (`4abaceb2`); the lifecycle bench's margins (`b87a46c4`); three load flakes waited on as events (`87696a7c`).
- Tests made hermetic on fast hosts (`76237a96`), load-flaky tests ordered by events (`a2f8d276`).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room)
- `crates/theseus/src/render.rs` 3,115 / 3,128
- `crates/theseus-discord/src/render.rs` 2,976 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-protocol/src/lib.rs` 2,556 / 2,733 (split in round 2; still add only what your task names)

Put new code in new files (a module of its own, one `mod` line in the parent). New wire types go in a protocol module
file of their own, re-exported with one line. Never raise a ceiling yourself (the keel guard flags it). If a line you
truly need will not fit under one, leave the file one line over, and say so in your report's first section with the
file and the reason: the maintainer raises it at the join, where the other rows' lines are counted too.

**Work in flight beside you** (other sessions on other branches, all from this same base; stay out of their files where
you can, and where you can't, keep your edit small and additive so the joins merge):
- The rows beside you in this wave, and the files each owns, are named in your task's own "Rules for this row" and
  "Not in this row" sections below. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: tail3-threads, the directory rule's recount, growth's two mistakes, a load-proof Jev-late test, and the chat's placed-turn edges (theseus-ax4dp, theseus-iw50k, theseus-tvyx0, theseus-lh0e4, theseus-dos5f, theseus-axgln, theseus-o2on8 doc line)

Branch: `cloud/20261021-tail3-threads`. Every commit's subject carries the Beads id of what it changes. Deadline for
the report: 4 hours after you start.

**Why.** The follow-ups of integ/threads round 3 (tail2-threads, merge in `be6ef45f`; tail2-chat, merge `9b138bb2`).
- **theseus-ax4dp (review F1+F2 of tail2-threads).** `thread_route::apart_by_dir` (the directory rule, theseus-vw8k7)
  drops an apart thread's file/root hits, stickiness, chain and learned correction. But (1) it recomputes the apart
  thread's entity with `hitting` counted **before** the drop, so a surviving ref/branch/host hit of that thread is still
  multiplied by `SHARED` (0.60) as if another thread were hit too; and (2) `pre_route` already halved every *other*
  thread's stickiness and zeroed its chain because "another thread's entity is named" (`other_named`), and that is not
  undone when the only named entity was the apart thread's dropped file hit (the growth case: `ops` stays at 0
  sticky). Both err conservative (the message falls to Jev or an ask), but both are wrong.
- **theseus-iw50k (author's follow-up).** Working-set growth (`thread_grow.rs`, `rpc/thread_grow.rs`) (1) files a
  *moved* message's entities in the thread the turn ran in, not the thread the owner moved it to ("that was for
  <thread>", `rpc/thread_move.rs`), and (2) grows message words that are bare file names (`slow.sh`), which is what made
  dry runs "name" a thread.
- **theseus-tvyx0.** `tests_families.rs` `jev_late_or_unreachable_falls_back_to_the_named_rule` asserts the wait took at
  most `judge_wait_ms + 250 ms`; under four busy loops it is red every time (passes alone, 0.35 s).
- **theseus-lh0e4 (tail2-chat review F1).** `thread.placed`'s docs say only the submitting connection hears it, but
  `tell_placed` sends through `EventSink::new(bus, home, direct)`, which also publishes on the home session's bus: every
  watcher of the home session gets it.
- **theseus-dos5f (F2).** The chat now follows a placed turn's session (`ours_in`). Events that name no turn
  (`ConfirmRequested`, `TaskChanged`) cannot be filtered to that turn, so another turn of the thread (one sent from
  Discord, say) can surface its question or task in this chat.
- **theseus-axgln (F3).** After a reconnect, the chat re-issues `session.watch` and `work.watch` for its own (and main)
  session only: a placed thread's session in `placed.also` is no longer followed.
- **theseus-o2on8 (doc line only).** `TurnSubmitResult.raised` has no doc comment; the text is given below.

**Risk: low-medium.** ax4dp changes routing scores, only toward what the directory rule already meant; iw50k changes what
a set grows from; the chat items narrow what a chat shows.

**Prerequisite: base `origin/integ/threads` at `be6ef45f`** (round 3 green: `main` 6918e3c1 + threads rounds 1-3), not
`main`: the directory rule, growth's callers, `thread.placed` and the chat's `placed` exist only there. Your branch joins
`integ/threads` round 4. Run the keel guard against `origin/integ/threads`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail3-threads` | theseus-ax4dp, iw50k, tvyx0, lh0e4, dos5f, axgln, o2on8 (doc line) | recount after the apart drop; growth follows a move, drops bare names; un-timed Jev-late test; placed docs; turn-scoped questions/tasks in a following chat; re-watch after reconnect; one doc line | core `thread_route.rs` (`pre_route`, `apart_by_dir`, `entity_of`), `rpc/tests_thread_route.rs` (new tests at its end), `thread_grow.rs`, `rpc/thread_grow.rs`, `rpc/thread_move.rs` (only what growth needs), `tests_families.rs` (that one test); protocol `thread_placed.rs` (doc), `lib.rs` (the `THREAD_PLACED` doc line and the `raised` doc lines only), and for dos5f at most one optional field on `ConfirmRequest` and/or `TaskChanged` (`tasks.rs`) with its `protocol.gen` file; core `rpc/thread_flow.rs` (`tell_placed` only, if you change delivery); TUI `chat/thread_ask.rs`, `chat/state.rs` (`connected` only), `chat/events.rs` (two match arms), `chat/tests_thread_ask.rs` | M |

## What exists (lines at `be6ef45f`; search by name, a line may have moved)
- **Scores.** `crates/theseus-core/src/thread_route.rs` (2,103 lines): `SHARED = 0.60` :71; `entity_of(h, hitting)`
  :473 (× `SHARED` when `hitting > 1`); `pre_route(text, threads, last, now_ms, half_life_ms)` :493: `hits` per thread,
  `hitting` = threads with any hit, `other_named = hitting > 0 && h.is_empty()` halves `sticky` and (with `hitting ==
  0` as the chain's filter) zeroes `chain`; `Pre { slug, title, entity, sticky, chain, score, hits, learned }`.
  `apart_by_dir(pre, threads, text, dir, base, home)` :584: counts `hitting` from `pre` before the loop, then per apart
  thread keeps only hits whose entry is named whole (`named_whole`) or not a file/root, sets `entity =
  entity_of(&kept, hitting)`, zeroes `sticky`, `chain`, `learned`, `score = noisy_or([entity])`, returns the dropped
  slugs. Its caller: `rpc/thread_flow.rs` near :343 (`keep_apart_by_dir`, right after `learned_thread`, in
  `place_unaddressed`). Its tests: `rpc/tests_thread_route.rs` section "the directory rule" at the end
  (`a_message_from_another_directory_is_not_placed_by_stickiness`, `a_file_named_from_another_directory_is_not_the_threads`)
  and pure tests in `thread_route.rs`.
- **Growth.** `crates/theseus-core/src/thread_grow.rs` (224 lines, pure: `touched_in`, `grow(cur, touched, named,
  elsewhere)`, `MAX_SET`); `rpc/thread_grow.rs`: `grow_for(routing, session, text)` picks `routing.slug` or the session's
  own thread, `named = threads_import::extract::working_set([text])`; `thread_grow(g, turn_id)` after the turn. The
  extractor `threads_import/extract.rs` :21 doc: "**files**: a token with a `/` whose last part has an extension, or a
  bare …" (read the whole rule). The move: `rpc/thread_move.rs` (and the correction words in `correction/words.rs`).
- **The test.** `crates/theseus-core/src/tests_families.rs` :226: the fake Jev holds the families answer, a task
  releases it after 3 s; asserts one `wait` span with `answered: false` and `duration_us() <= (judge_wait_ms + 250) *
  1000`; `raised.families[0].by == "named"`; then the unreachable half.
- **thread.placed.** `crates/theseus-protocol/src/thread_placed.rs` module doc ("It goes to the connection that sent the
  message"); `lib.rs` :488 the `THREAD_PLACED` doc ("to the connection that sent it"); `rpc/thread_flow.rs` :266
  `tell_placed` (`direct = Some((conn.client, conn.tx))`, `EventSink::new(self.bus.clone(), home, direct)`).
- **The chat.** `crates/theseus-tui/src/chat/thread_ask.rs`: `ours_in(session_id, turn_id: Option<&str>)` near :153
  (own session, or a session in `placed.also` and the turn, when named, in `self.mine`); `thread_placed`, `adopt`
  (pushes onto `placed.also`, re-issues `Self::watch_with(&sid, &self.placed.also)`), `ALSO`. `chat/events.rs` :453
  `Event::ConfirmRequested(c) if self.ours_in(&c.session_id, None)`, :480 `Event::TaskChanged(t) if
  self.ours_in(&t.session_id, None)`. `chat/state.rs` :360 `connected(now_ms)`: on a reconnect it issues
  `SESSION_WATCH` for the session, `Self::watch_work(&main)`, and main's watch; `chat/tasks.rs` :84/:90 `watch_work`,
  `watch_with(sid, also)`. Protocol: `ConfirmRequest` (`lib.rs` :2254) has `correlation_id`, `session_id`,
  `execution_id`, … (check whether `execution_id` already identifies the turn: a turn's execution id is derived from
  its ids in places, `replacen("ses_", "exe_", 1)` in tests); `TaskChanged` is `tasks.rs` :394.
- **raised.** `lib.rs` :2186-2188, `#[serde(default, skip_serializing_if = "Option::is_none")]`,
  `#[cfg_attr(test, ts(optional))]`, `pub raised: Option<route::ToolsRaised>,` with no doc.

## What to build
1. **theseus-ax4dp.** Make the scores after the directory rule the ones `pre_route` would have given had the dropped
   hits never been there: recount `hitting` after the apart threads' drops, recompute every thread's `entity` with it,
   and recompute `other_named`'s effect (stickiness halving, chain) for the threads it touched. The cleanest shape is
   likely one pure function that scores from the (kept) hits and the raw stickiness/chain, used by both `pre_route` and
   `apart_by_dir`, rather than undoing a halving after the fact; `Pre` may carry what it needs. Keep `apart_by_dir`'s
   signature and its returned slugs, and `pre_route`'s results for every case with no apart thread unchanged.
2. **theseus-iw50k.**
   - (1) A turn whose message the owner moved to another thread grows the thread it was moved **to**, not the one it ran
     in; and a move after growth does not leave the entities in the wrong thread (pick: grow at move time, or undo the
     earlier growth; say which and why). Read `thread_move.rs` first and keep the change small.
   - (2) Named entities grown from a message drop bare file names (no `/`): a working set grows a file only from a
     path. Do this in `grow_for`'s `named` (growth's own filter), not in the import extractor, whose other callers keep
     their rule.
3. **theseus-tvyx0.** The test keeps its meaning ("the turn does not wait for Jev's late answer") without a tight
   deadline: release the held answer much later (for example 30 s, well past any loaded wait) and assert the wait
   ended before the release (`answered: false`, duration well under the release), plus `raised.families[0].by ==
   "named"`. Do not change `judge_wait_ms` or production code. Run it 20 times under four busy loops (`yes > /dev/null`
   ×4) and paste the count.
4. **theseus-lh0e4.** Decide: either the docs say what happens ("to the sender's connection and to every watcher of
   the home session"), or delivery becomes sender-only (`EventSink` with no bus publish, if one exists). Pick docs
   unless a home-session watcher acting on it is a real bug you can show; say which. Fix `thread_placed.rs`'s module
   doc and the `THREAD_PLACED` doc line together.
5. **theseus-dos5f.** A chat following a placed thread shows a `ConfirmRequested` or `TaskChanged` of that thread's
   session only when it belongs to one of the chat's own turns (`self.mine`). Use what the events already carry if it
   identifies the turn (`execution_id`, a task's parent/turn field); add one optional field (`turn_id: Option<String>`,
   `#[serde(default, skip_serializing_if = "Option::is_none")]`, ts optional) only if nothing does, filled where the
   event is built, and regenerate its `protocol.gen` file. The chat's own session's events are unaffected.
6. **theseus-axgln.** `connected` on a reconnect re-issues the work watch with `placed.also` (`watch_with(&main,
   &self.placed.also)` or the right equivalent) so a placed thread's turns and tasks stay followed.
7. **theseus-o2on8 (doc line only).** Above `pub raised: Option<route::ToolsRaised>,` in `lib.rs`, add exactly:
   ```rust
       /// The deferred-tool families this turn's first request loaded past its place's budget, each with its tokens and
       /// who admitted it (`named` or `jev`), under `[tools.defer] admit_ceiling` (theseus-xlri5); none when nothing passed.
   ```
   The spec's Part III items of that issue are the maintainer's: not yours.

## Tests (each with a planted revert; the first is the key test)
- **`the_directory_rule_recounts_what_it_dropped`** (pure, `thread_route.rs` tests): threads A (apart, roots elsewhere;
  the message names its bare file `slow.sh` and its branch `feat-x`) and B (near, sticky): after the rule, A's entity is
  `entity_of([feat-x], 1)` (no `SHARED`), and B's stickiness is the full, un-halved value. Plant 1: recount with the old
  `hitting`: A's case fails. Plant 2: leave `other_named`'s halving: B's case fails. Paste both.
- `a_moved_message_grows_the_thread_it_was_moved_to` (rpc, `tests_thread_route.rs` end or a new `tests_thread_grow.rs`):
  the entities land in the destination's set and not the origin's. Plant: grow at the turn's thread only.
- `a_bare_file_name_does_not_grow_a_set` (pure or rpc): `slow.sh` alone adds nothing; `tools/slow.sh` adds the file.
  Plant: drop the filter.
- `a_placed_threads_other_turn_does_not_ask_in_this_chat` (TUI, `tests_thread_ask.rs`): after a placement, a
  `ConfirmRequested` of the thread's session for a turn not in `mine` is not shown; one for the chat's placed turn is.
  Plant: `ours_in(.., None)` again.
- `a_reconnect_keeps_following_the_placed_thread` (TUI): after placement and a reconnect, the effects include a work
  watch naming the placed session. Plant: the old `connected`.
- lh0e4: a test only if you change delivery; o2on8: none.
- `cargo nextest run -p theseus-core thread_route thread_grow tests_thread_route tests_families`, `cargo nextest run -p
  theseus-tui chat`, and, if you touched the wire, `cargo test -p theseus-protocol ts` then `git diff --stat --
  cockpit/src/protocol.gen` (only the files of the types you changed).

## Not in this row
- theseus-2gtyi (the rule is silent from a directory above every root) and theseus-tbqpy (a ladder adoption rule for
  the cross breaker): owner calls, waiting for the owner.
- theseus-m7fwf (walk1-fix's routing LOWs): that code is on `main`, not on this base.
- The row relaunched beside you on this same base, tail2-route (W8c2): `turn/route_step.rs`, `turn/personal_step.rs`,
  `turn.rs`, `turn/detour_step.rs`, `rpc/context.rs`, `tests_detour.rs`, protocol `src/context.rs` and `src/why.rs`,
  cockpit `ContextDetour*.ts`/`ContextPart.ts`/`ContextTurn.tsx`, `crates/theseusd/tests/judge.rs`. Stay out of all of
  them.
- Spec Part III items; `docs/status.md` and core `AGENTS.md` beyond one sentence each for ax4dp and iw50k (put them in
  the growth/directory-rule paragraphs tail2-threads wrote; search `directory rule` and `working_set_grows`).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed. The key test must be shown failing on its
  planted revert.
- No new always-loaded model text and no new tool.
- **Line ceilings** (`scripts/long-files.txt`, at `be6ef45f`): `crates/theseus-protocol/src/lib.rs` 2,556 / 2,733 (add
  only the doc lines named; a new type goes in its module). `rpc/tests_thread_route.rs` is 1,589 lines: if your tests
  would take it past 2,500, put them in a new test file with one `mod` line. Never raise a ceiling.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. tvyx0 changes a timing
  bound into an ordering bound with the same meaning: say so in the report, with the old and new assertion. A cloud row
  cannot carry a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/integ/threads
  --head HEAD --quiet` and paste its output. It must be empty.
- **Dependencies.** None new. **Store format** on your base is 48. A working set is stored in the thread record; this
  row changes what grows into it, not its layout. If you add a stored field, bump to 49 with the old sample in
  `crates/theseus-core/src/tests_layouts.rs`.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/integ/threads`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner".
- Read `crates/theseus-core/AGENTS.md` (threads), `docs/design/threads.md` and `crates/theseus-tui/AGENTS.md` first. Run
  `cargo nextest run -p theseus-core`, `cargo nextest run -p theseus-tui`, `cargo clippy --workspace --all-targets --
  -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what changed per Beads id; the scoring shape you chose for ax4dp and the cases whose numbers changed;
the move choice for iw50k; the 20-run loop count for tvyx0; the docs-or-delivery choice for lh0e4; what dos5f filters
on and any wire field; each test and its plant with the failing line; the keel guard's output; the store format;
follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
