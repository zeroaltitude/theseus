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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-cn-open-view`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from `main` at `87696a7c` (2026-10-21), unless your task names another base (an integration branch). Where the
text below names `bbc65530`, a store format, line ceilings or "work in flight tonight", this section is right and the
text below is stale. Line numbers quoted below may have moved: search by name.

**Store format: your base is integ/cn, where it is already 48 (C14's SessionRecord.data); on main it is 47** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 49 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 48 at once; the maintainer renumbers at the join, so never
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
- `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (three lines of room)
- `crates/theseus/src/render.rs` 3,115 / 3,128
- `crates/theseus-discord/src/render.rs` 2,976 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/tests_m3.rs` 7,979 / 8,050
- `crates/theseus-core/src/turn.rs` 3,405 / 3,525
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

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

---
## Your task: cn-open-view, the open view: a request compiled for open data carries none of the owner's material, whatever model it goes to (theseus-a1wvq.7)

Branch: `cloud/20261021-cn-open-view`. Every commit's subject carries `theseus-a1wvq.7`. Deadline for the report: 5
hours after you start.

## Your base: `integ/cn` with the floor row in (read this before anything else)
Your branch starts from the integration branch `integ/cn`, **not** from `main`. `integ/cn` is `main` (`87696a7c` or
later) plus the China-origin floor row, theseus-a1wvq.3 and theseus-a1wvq.5 (built on branch
`cloud/20261021-cn-floor-session`). Where the note above says your base is `main`, or that the store format is 47, this
section is right:
- **Check first** that the floor row is in: `git log --oneline | grep -E 'theseus-a1wvq\.(3|5)'` shows a `.3` commit
  (the floor) and a `.5` commit (the session's data state, `SessionRecord.data`). If `.5` is missing, stop, write
  that in `CLOUD_REPORT.md`, push it, and end.
- **Store format:** read `const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs` (48 with `.5` in). **This row
  bumps nothing**: the open paths are config and a derived value, never a stored field.

## Why
The owner's words (2026-10-17 09:26 and 09:57, verbatim; one name redacted because this repository is public): "[The
owner's company] and any private or personal or confidential information never goes to Chinese models." "I think that
Chinese models in dms can be evaluated by content rather than by the nature of the channel." So a request to a model
of restricted origin (`cn` or `unknown`) may carry only content judged open, the DM included. The judging is a later
row (theseus-a1wvq.8). This row builds the half that needs no judging: **the open view**, the prefix such a request is
compiled under. Today every request in a private place carries the owner's material, whatever the message:
- the persona and project context files (`context_parts.rs` `built_parts`: only a **shared** place withholds them,
  `context_files::withhold_shared`);
- the standing facts and preferences (`turn/standing_step.rs` `standing_block`, `standing.rs` `Block::of`);
- the ontology's people guidance (`ontology.rs` `walk(session, class)`);
- the session block (`compiler/session_block.rs` `session_block`, empty only for a shared place);
- live recall of personal episodes (`recall.rs` `place_of`: a private place draws on every session,
  `theseus-memory/src/recall.rs` `Place::may_draw_on`);
- the tool offer (`places.rs` `offered`: a private place is offered everything, `memory.lookup` included).

A message about an open-source project, judged open, would still go out wrapped in all of that. The shared view
already defines "the owner's material" and withholds it from a shared place (the place rule, theseus-nbsh). The design
reuses it: **a request compiled in the open view gets the shared view's content rules, while its session keeps its own
place class for approvals, cards and gliding.**

**Risk: medium.** It touches every reader of a place's class on the compile path. The guard is a golden, written
first: a private session's request must stay byte-for-byte as today. Nothing opens a session yet except the parallel
open-task row, and nothing asks for the open view for one compile until theseus-a1wvq.8, so on its own this row changes
no live request.

**Prerequisite: `integ/cn` with theseus-a1wvq.3 and .5 (above).**

## What exists on your base (search by name; lines move)
- **The view.** `ceiling.rs`: `PlaceView { class, ceiling, voice }` (`Copy`), `PlaceView::offered(name, public)`,
  and `TurnRunner::view_of(session_id)` (where the session's words go, from the outbox). `toolrun.rs`: `TurnCtx`
  carries `class` and `ceiling`, and `TurnCtx::place()` builds a `PlaceView` from them. `turn.rs`, after catch-up:
  `let here = self.view_of(sid); (t.tc.class, t.tc.ceiling) = (here.class, here.ceiling);`. `turn.rs` `class_of`.
- **Readers of the class on the compile path** (each is content or authority; classify every one):
  `context_parts.rs` `built_parts` (context files, `spec_of`, `session_block`, `header_parts`, `walk`);
  `prefix/turn.rs` (the prefix key `class_name(place.class)`, `walk`, `standing_block`); `turn/compile_step.rs`
  (`class: t.tc.class` in the prefix query and the compile's record); `turn/route_step.rs` `route_to` (its
  `session_block(t.tc.dir, t.tc.place())`, `walk(t.tc.session_id, t.tc.class)`, `stand`) and `compile_detour`;
  `compiler/session_block.rs`; `compiler/situation.rs` (`admits(situation, p.class, ..)`); `standing.rs` `Block::of`;
  `ontology.rs` `walk` and `Walk::compose`; `category_recall.rs` (withholds for a shared class); `self_card.rs`
  (the web address line for a private class); `recall.rs` `place_of` / `in_private`; `places.rs` `offered`,
  `refusal`, `private_fetch`, `public_roots` (`[places] public_paths`); `toolrun.rs` (the offer, `public_roots`);
  `toolrun/order.rs` (`places::refusal(place.class, tool, plan, &self.public_roots)`, `private_fetch`).
- **Authority, which keeps `class`:** approvals and asks, `owner_in_private`, glide (`glide.rs` `own_class` /
  `other_class`), cards, `rpc/budgets.rs`, `rpc/context.rs`'s reported class, `surface.rs`, `keep_warm.rs` (it skips
  tasks and shared places; leave it), `census.rs`.
- **From the floor row:** `SessionRecord.data: Option<Box<DataState>>` with `opened: Option<Opened>`, `taint:
  Option<Taint { why, node, at_ms }>`; absent `data` reads as the private view. `crate::data::DataWhy`. The floor
  keeps every turn uncleared (`RouteState.clearance`), and `TurnRunner::data_pass` is the seam theseus-a1wvq.8 fills.
- **Config:** `config/routing_data.rs` `RoutingData { open_topics }` (`deny_unknown_fields`), under `[routing.data]`.

## Your row
| # | Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|---|
| 7 | `cn-open-view` | theseus-a1wvq.7 | `PlaceView.open` and `content_class()`; every content reader on the compile path reads it; the open view chosen per session (`data.opened`) and askable for one compile; open recall; the open tool offer under `[routing.data] open_paths`; a private session's request byte-identical | `ceiling.rs`, `toolrun.rs` (one field, one line in `place()`), `turn.rs` (the line that sets the turn's view), `context_parts.rs`, `prefix/turn.rs`, `turn/compile_step.rs`, `turn/route_step.rs` (`route_to`, `compile_detour`), `compiler/session_block.rs`, `compiler/situation.rs`, `category_recall.rs`, `self_card.rs`, `recall.rs`, `theseus-memory/src/recall.rs`, `places.rs`, `toolrun/order.rs`, `config/routing_data.rs`, a new `open_view.rs` | L |

Do the items in this order; commit after each.

1. **The golden first, before any change.** A test that compiles the first request of (a) a CLI private session with a
   context file, a standing fact, a preference, a person in the ontology, a directory and a recalled episode, and (b)
   a DM-shaped session with the same, and compares each request body to a checked-in golden
   (`a_private_sessions_request_is_byte_identical`). Commit it on its own, green, before item 2. It must stay green,
   unchanged, through the row.
2. **The view.** `PlaceView.open: bool` and, in `ceiling.rs`:
   ```rust
   pub fn content_class(&self) -> PlaceClass { if self.open { PlaceClass::Shared } else { self.class } }
   ```
   `TurnCtx` gains `open: bool` (one field and its doc in `toolrun.rs`; `place()` copies it; `toolrun.rs` is 2,479
   lines and unlisted in `scripts/long-files.txt`, so it may not pass 2,500: add only those lines). The turn sets it
   where it sets `class` (`turn.rs`, after catch-up): `open` is true when the session's `data.opened` is set. Every
   other `PlaceView` construction sets `open: false`. Put new logic in a new `crates/theseus-core/src/open_view.rs`
   (one `mod` line), not in the long files.
3. **One compile in the open view.** A named seam that compiles one request in the open view for a session that is
   not open (the open-request row will call it for a `trivial` or `quick` detour in the owner's DM, after Jev judged
   the message open). Its shape is yours; it must (a) leave the session and the turn's own view untouched, so the next
   compile is the private view again, (b) use the open view for every content reader and for the tool offer of that
   one request, and (c) have no caller in this row but its test. Name it, with its signature, in the report's first
   section: theseus-a1wvq.8's task will be written against it.
4. **Every content reader reads `content_class()`** (or the view's `open`): context files withheld unless
   `readers = "public"`; standing **orders kept**, standing facts and preferences withheld (the owner's answer: orders
   stay in the open view); ontology guidance down to transport memberships (what a shared class gets); the session
   block kept **only** when the session's directory is under the open roots (item 6), else empty; the prefix library
   keyed by `content_class()`, so an open prefix never shares a key with a private one; `situation.rs`'s admits;
   `category_recall.rs`; the self card's private-only line; the detour compile (`compile_detour` and `route_to`'s
   `session_block` / `walk` / `stand`). Approvals, glide, cards and asks keep `class`. List every reader you changed
   and every one you kept on `class`, with one line why, in the report.
5. **Open recall.** `theseus-memory/src/recall.rs` `Place` gains `Open`: a turn compiled in the open view recalls as
   `Place::Open`, which draws **only** on nodes of open sessions (`data.opened` set) written before their taint's
   `at_ms` (all of them when untainted), and on nothing else: not a private session's nodes, not a private-view
   session's open runs, not imported episodes (there is no `public` label on imports today; fail closed). A private
   place's `may_draw_on` is unchanged (it still draws on everything, open sessions included). The core's `place_of`
   has other callers (`memory_lookup.rs`, `memory_lookup/mcp.rs`, `push/last_line.rs`, `retract.rs`,
   `recall/cite.rs`): keep what each gets for every session that is not open; for an open session, say in the report
   what each now gets and why it is right. If the index cannot tell a node's time against the taint cheaply, draw on
   no node of a tainted open session at all (fail closed) and name the follow-up.
6. **The open tool offer.** `[routing.data] open_paths: Vec<String>` (default empty; expanded and canonical as
   `places::public_roots` does for `[places] public_paths`), and one function, `open_roots(cfg, session) ->
   Vec<PathBuf>`, that returns them. It is the value a session can extend for itself: theseus-a1wvq.13 will add a
   draft task's own tree there, derived from the task's id, so keep it a function of the session, not a stored field.
   In the open view:
   - the public tools and `task.*`, `wake.*`, `web.search`, `http.fetch` are offered, as in a shared place;
   - `fs.*`, `git.*`, `text.*` are offered only under the open roots (the shared place's `public` rule, with `public`
     = the open roots, never `[places] public_paths`);
   - `proc.run` (and any other tool that runs a command: search `places.rs`'s families and list them) is offered only
     when the session's directory is under the open roots, and each call's `cwd` must be too, else it is refused in
     words at `toolrun/order.rs`'s gate;
   - `memory.lookup`, glide (`channel.*`), MCP tools and `aws.*` are not offered.
   The offer and the refusal read the same view, so a tool the model is not offered is also refused if called.
7. **The token keel.** The open view only removes blocks: an open task's first request is at most its private
   view's, and stays within 8,000 tokens (`first_request.rs`).

## Tests (each with a planted revert; the first is the key test)
Build an open session in tests by writing `SessionRecord.data` with `opened` set directly (the open-task row, beside
you, builds the real opening; do not wait for it). Put new tests in a new `crates/theseus-core/src/tests_open_view.rs`.
- **`open_view_request_carries_no_owner_material`** (T22): an open task session in a private place, with a private
  context file (a marker string), a public one (`readers = "public"`), a standing order, a standing fact and a
  preference (markers), a person in the ontology (a marker name), a personal episode recall would surface (a marker),
  and a directory outside the open roots. Its compiled first request holds the public file, the standing order and no
  marker; the session block is empty; its offered tools hold no `memory.lookup`, `channel.*`, `aws.*`, MCP or
  `proc.run`. With the directory under `open_paths` the session block is kept and `fs.read` is offered. Its first
  request is at most 8,000 tokens. Plant: make `content_class()` return `self.class`; the markers must reach the
  request and the test must fail. Show it.
- `a_private_sessions_request_is_byte_identical` (item 1's golden): green before and after every commit. Plant: none
  (it is the guard); say so.
- `the_open_view_can_be_asked_for_one_compile`: a private session (no `data`); one compile through item 3's seam
  carries none of the markers above; the next ordinary compile equals the golden. Plant: make the seam compile in the
  session's own view.
- `proc_run_needs_its_cwd_under_open_paths`: in the open view, a session whose directory is outside the open roots is
  not offered `proc.run` and a call is refused; inside them it is offered, and a call with a `cwd` outside them is
  refused in words. Plant: drop the `cwd` check.
- `open_recall_draws_only_on_open_runs`: an open request recalls a node of an untainted open session, not a node
  written after another open session's taint, not a private session's node, not an imported episode; a private
  session still recalls all four. Plant: let `Place::Open` draw as `Private`.
- `open_paths_parse`: `[routing.data] open_paths` parses, expands `~`, defaults empty; an unknown key under
  `[routing.data]` is still refused.
- The first-request tests and `tests_m3::a_plain_turn_stays_within_its_frame_budget` pass unchanged.

## Beside you (other sessions on other branches; stay out of their files)
- **cn-open-tasks (theseus-a1wvq.6)**, branch `cloud/20261021-cn-open-tasks`, also from `integ/cn`: `task.create` /
  `task.start` `open`, which sets `SessionRecord.data.opened` in the task's opening frame. It owns `task.rs`,
  `rpc/task_start.rs`, `crates/theseus-protocol/src/task_start.rs` and the cockpit's generated `TaskStartParams.ts`.
  **The contract between you is the field alone:** the view is open when `data.opened` is set. Do not touch task
  creation.
- **cn-routing-compose (theseus-a1wvq.4)**, branch `cloud/20261021-cn-routing-compose`, from `main`: `routing.rs`,
  `config/routing.rs` (`ClassProfiles`), a new `config/routing_cn.rs`, one call line in `Config::validate`, the
  routing passage of `crates/theseus-core/AGENTS.md`, and **exactly two lines in `turn/route_step.rs`**: the
  `restricted` field as the last field of `route_profiles`' `routing::Profile` literal and `cleared: false` as the last
  field of `decide_route`'s `routing::Ask` literal (plus one `cleared: false` line each in
  `turn/route_step/correct.rs` and `turn/brief_step.rs`). Your `route_step.rs` edits are in `route_to` and
  `compile_detour` only; never those two literals or the `route_profiles` callers. Your config field goes in
  `config/routing_data.rs`, never `config/routing.rs`. In `crates/theseus-core/AGENTS.md`, add one short passage on
  the open view beside the data rule's passage, never in the routing passage.
- If a change you want sits in one of those files, leave it and name it in the report.

## Not in this row
Deciding that a request is open, the open run, minting a pass and calling item 3's seam (theseus-a1wvq.8); the tool
result screen (theseus-a1wvq.9); background calls (theseus-a1wvq.10); the footer (theseus-a1wvq.11); a draft task's tree
(theseus-a1wvq.13: it extends `open_roots`). A `public` label on imported episodes is a follow-up: name it.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias. Stored data stays readable.
- **Fail closed, always.** Anything unknown, missing or unparsable is the private view for the owner's own place and
  withheld for an open request; a reader you are unsure of withholds in the open view. Never loosen a check, a
  threshold or a ceiling to make a test pass.
- **On, measured, with a named rollback.** The view is on for every open session; the rollback is reverting this row's
  commits (name them). The golden shows a private session is untouched.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). No new model tool. FAST: nothing new on
  a plain turn (`tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's turn bench must not move): the
  view's `open` is read from the record the turn already holds, never from a new store read.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/integ/cn --head HEAD --quiet` (your base, not `main`) and paste its
  output in the report. It must be empty.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change.
- Line ceilings (`scripts/long-files.txt`): `turn.rs` is near its ceiling and `toolrun.rs` must stay under 2,500; never
  raise a ceiling. New code goes in `open_view.rs`.
- **The lane gate:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` passes on your last commit (run the
  touched crates' tests first: `cargo nextest run -p theseus-core -p theseus-memory`). Say what it printed.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner".
  Never write the owner's company or private project names. Every fixture is invented ("Acme", "Widgetco").
- **The data rule binds this session too.** Your own model is Claude. No request of yours goes to a GLM or DeepSeek
  endpoint, and there is no key for one on the VM; never add one.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (`crates/theseus-core/AGENTS.md`: the place rule, the
  data rule's invariants, the long-file rule; `crates/theseus-memory/AGENTS.md`). Keep CLOUD_REPORT.md honest: what you
  ran, what passed, what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1
  tests fail on this VM; this row should need none: say so).
- Out of scope: the owner's config, daemon and machine; any benchmark run; `docs/status.md`, the README,
  `docs/design/` and the spec (the maintainer writes those: say in the report what should change).

## Your report
Commit `CLOUD_REPORT.md` at the repository root (written with your editor tools, never a shell heredoc; check its size
after the commit, it must not be empty). First section: item 3's seam, its name and signature, for the open-request
row. Then: what changed, file by file; every class reader, changed or kept, with why; each test and its plant (the line
broken, the failure seen, the restore); the first-request token counts of the open-view test; what could not run on the
VM and the exact commands for the maintainer; the keel guard's output; "store format: unchanged"; the follow-ups (the
imports' `public` label, anything you left in another row's file). Push the branch, and end with a short final message
that starts `CLOUD REPORT COMPLETE`.
