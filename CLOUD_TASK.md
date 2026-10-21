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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail-discord`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from `main` at `6918e3c1` (2026-10-21, after the patch1 stage-1 landing), unless your task names another base (an integration branch). Where the
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
## Your task: tail-discord, Discord's link card and /thread link, and four small fixes to the binding's task views (theseus-98bz.12, theseus-stpqg, theseus-0mq02, theseus-x4s7g, theseus-0prmm)

Branch: `cloud/20261021-tail-discord`. Every commit's subject carries the Beads id of what it does (`theseus-98bz.12`,
`theseus-stpqg`, `theseus-0mq02`, `theseus-x4s7g` or `theseus-0prmm`). Deadline for the report: 6 hours after you start.

**Why.** Five things a person meets on Discord today:
- **Jev's link offer has no buttons, and Discord cannot link two threads at all (theseus-98bz.12).** Since threads-links
  (`theseus-98bz.11`), when Jev's recall admits notes of one live thread into another's twice, the owner's DM gets one
  sentence: `🔗 Notes of «stats» answered questions in «pipeline» twice. If the two belong together: theseus thread link
  pipeline stats`. It is posted as an `onto_card` with no write and no undo, which renders as plain text with no
  button. To act on it the owner must leave Discord and type the CLI command; `/thread` on Discord has `new`, `list`,
  `bind`, `unbind` and `merge`, and no `link`. A seal proposal, the same kind of offer, already has its card:
  **Seal this thread** / **Not needed**.
- **A long-running binding's memory grows by one task id per task thread, forever (theseus-stpqg).** The task threads'
  `Inner.born` set gains each task given a thread or a post and is never trimmed; its neighbour `urls` is capped at 512.
- **An ended task's view can stay in memory forever (theseus-0mq02).** `Tasks::snapshot` keeps every ended view it does
  not carry (a snapshot carries no ended child, so its absence says nothing); only a `WorkChanged.gone` frame removes
  one. A lost gone frame (events lost, then a rewatch) leaks the view: invisible on the board after `DONE_SHOWN_MS`
  (24 h), never counted, never freed.
- **Two concurrent `work.watch` asks can apply out of order (theseus-x4s7g).** `Place::adopt` (`runtime/succession.rs`)
  awaits `tasks::ask` while `watch_loop`'s own `ask` may be in flight; the RPC gives no reply order, so an older snapshot
  (roots without the new session) applied after a newer one drops the new root's active views until the next rewatch
  ~100 ms later. And `tests_task_watch` asserts `bus.work_watchers() >= 1`: any watcher, not one that watches this
  place's session.
- **A test can see the owner's DM draw the board while the task home is being set up (theseus-0prmm).** One ARM run of
  `tests_task_home::the_task_home_holds_the_board_reports_and_one_pointer_per_task` failed with a V2 board in the DM.
  `home_up` (`tests_task_home.rs`) waits for the META record `discord.task_home`, which `find_task_home` writes before
  `ensure_task_home` runs `home_lane`, `state.send_replace(Some(channel))` and `register_task_homes`; between the two,
  `TaskHome::instead_of` still says no home and the DM lane draws the board. Not reproduced in 3 later green runs.

**Risk: low to medium.** The link card is new UI on an existing core method (`thread.link`, judged as today). The four
fixes change in-memory bookkeeping only; nothing stored changes, no store format bump (the base is at format 48).

**Prerequisite: `main`.** (threads-links, `theseus-98bz.11`, is on `main`.)

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail-discord` | theseus-98bz.12, stpqg, 0mq02, x4s7g, 0prmm | link card + `/thread link`/`unlink`; `born` trimmed; ended views pruned at a snapshot; snapshots applied in ask order; two tests wait on the right thing | `theseus-core/src/thread_link_seen.rs`, `theseus-core/src/rpc/tests_thread_link.rs` (its `proposals` filter and the text assertion), `theseus-core/src/bus.rs` (one read-only method), `theseus-discord/src/runtime/seal/link.rs` (new), `runtime/seal.rs` (the `mod` line), `runtime.rs` (edit the existing `mod seal;` line to `pub(crate) mod seal;`, no new line), `runtime/earned.rs` (one dispatch arm), `courier.rs` (the kind in the DM list and its card arm), `render.rs` (one `Buttons` variant), `runtime/thread.rs` (two subcommands), `runtime/threads.rs` (`born`), `runtime/tasks.rs` (snapshot), `runtime/succession.rs` (ask's ticket, if needed there), `tests_gateway/link.rs` (new) + its `mod` line in `tests_gateway.rs`, `tests_task_watch.rs`, `tests_task_home.rs` | M |

## What exists (lines at `6918e3c1`; search by name)
- **The proposal (core).** `crates/theseus-core/src/thread_link_seen.rs` (117 lines): `Pair { here, other }` (slug,
  title each), `Pair::text` (the sentence above), `count` (META `thread.link_seen.<a>.<b>` `{recalls, asked}`; at the
  second recall with the owner's DM bound it stages `json!({"kind": "onto_card", "undo": false, "text": text})` through
  `outbox.stage_to_operator` in one frame with the META record and a `ThreadLinkProposed` fact row), and
  `TurnRunner::link_seen` (spawned blocking, after the recall; the text runs through `self.tools.scrubber`). A pair is
  asked once whatever the owner does. Switch: `[threads] link_proposals`. Its tests: `rpc/tests_thread_link.rs`
  (`a_pair_admitted_twice_is_proposed_once` near :495 asserts the text ends with `theseus thread link pipeline stats`;
  `proposals` near :483 filters `b["kind"] == "onto_card"`).
- **The link itself (core).** `rpc/thread_link.rs`: `thread_link` / `thread_unlink` take
  `theseus_protocol::threads::ThreadLinkParams { slug, other, author, discord }` and judge the pair
  (`judge_thread_pair`: the owner, from a private place) before writing; one frame holds both records and a
  `thread.linked` / `thread.unlinked` row. Methods `method::THREAD_LINK`, `method::THREAD_UNLINK`. Nothing to change here.
- **The seal card (the model to copy).** `crates/theseus-discord/src/runtime/seal.rs` (180 lines): `PREFIX = "seal"`,
  `buttons(slug)` (custom ids `seal:<yes|no>:<slug>`), `parse`, `slug_of`, `Shared::seal_press` (defers ephemerally,
  calls the core as the presser with the `DiscordOrigin`, answers with `followup`; `failed` turns a REFUSED error into
  `🔐 Your answer did not count: <why>.`). Its press is dispatched in `runtime/earned.rs` (the `else if` chain near
  :96-118). In `courier.rs`: the owner-DM kinds list near :889 (`"seal_card"`, `"onto_card"` → `self.jev_post`), the
  card arm near :1432 (`"seal_card" => (text, format!("seal:{}", a.correlation_id), Buttons::Seal(...))`), and
  `Buttons::Seal(slug)` → `crate::runtime::seal_buttons(slug)` near :1720 and :1794. `Buttons` is in
  `crates/theseus-discord/src/render.rs` near :80. Gateway test to copy: `tests_gateway/seal.rs`
  (`a_seal_card_goes_to_the_owners_dm_and_the_owner_alone_answers_it`, a press built as a component interaction).
- **`/thread`.** `runtime/thread.rs`: `command()` builds `new`, `list`, `bind`, `unbind`, `merge`;
  `thread_interaction` dispatches on `parsed(&c.options)` and answers ephemerally; the fallback line lists the
  subcommands; a unit test near :372 asserts `names == ["new", "list", "bind", "unbind", "merge"]`. Registered in
  `runtime/slash.rs` (`cmds.push(thread::command())`): no change there.
- **Task threads.** `runtime/threads.rs`: `Inner` near :209 (`born: HashSet<String>` near :224, `urls` near :228);
  `TaskThreads::add` near :442 inserts into `born` and caps `urls` at 512 by clearing it; `next` near :347 reads
  `born` in the `(None, true)` arm: an ended task with no `Made` row gets a post only if `owed` and not born, so `born`
  is what stops a second post for a task whose thread row already left (`retire_row` / `drop_made` near :468-493,
  `retire_later` after `GRACE` = 600 s).
- **Task views.** `runtime/tasks.rs`: `DONE_SHOWN_MS` (24 h) near :40; `Tasks::snapshot` near :170 (its `retain`
  never drops an ended view); `Tasks::changed` near :200 (a gone frame removes a view); `ask` near :647 (calls
  `work.watch`, then `tasks.snapshot(r)`); `watch_loop` near :609; tests `a_snapshot_keeps_an_ended_task_it_does_not_carry`
  near :985 and `done_tasks_show_until_the_place_clears_them` near :1016. `Place::adopt` in `runtime/succession.rs`
  near :205 awaits `super::tasks::ask(&self.shared)` then `rewatch()`.
- **Bus.** `crates/theseus-core/src/bus.rs`: `work_watchers()` near :122 counts watchers; each `WorkWatcher` has a
  `scope: WorkScope` (`crate::push::WorkScope { roots: Option<HashSet<String>>, guarded }`; `roots: None` sees all).
- **Task home.** `runtime/task_home.rs`: `instead_of` near :117 (reads `self.channel()`), `ensure_task_home` near :300
  (after `find_task_home` succeeds: `home_lane`, `state.send_replace(Some(channel))`, `register_task_homes`,
  `wake_places`), `find_task_home` near :395 (writes the META record near :389). `tests_task_home.rs`: `home_record`
  near :44, `home_up` near :49.
- **Line ceilings.** `crates/theseus-discord/src/runtime.rs` is 3,499 / 3,500: **add no line to it.** The link card is a
  child of `seal.rs` (`runtime/seal/link.rs`, declared `mod link;` in `seal.rs`); reach it from `courier.rs` by changing
  the existing `mod seal;` line in `runtime.rs` to `pub(crate) mod seal;` (same line count) and naming
  `crate::runtime::seal::link::…`, or re-export from `seal.rs`. `crates/theseus-discord/src/render.rs` is 2,976 / 3,009
  (one variant line is fine; a sibling row of this batch edits that file's `fn footer` near :1223, so touch only the
  `Buttons` enum and the two match arms that render it, and reformat nothing else there). `courier.rs` (2,163) and `runtime/thread.rs` (451) are unlisted.

## What to build
1. **The link card (theseus-98bz.12).**
   - Core: `count` stages `{"kind": "link_card", "text": …, "here": <slug>, "other": <slug>}` instead of the plain
     `onto_card`. The text states the fact and names the threads: `🔗 Notes of «stats» answered questions in «pipeline»
     twice.` (keep the CLI form as a last line, `theseus thread link pipeline stats`, so a DM with no buttons still says
     how). Everything else in `count` stays (once per pair, one frame, the scrubbed text, the switch). Update
     `tests_thread_link.rs`'s `proposals` filter to the new kind and its text assertion to the new text; do not delete or
     loosen any other assertion.
   - Discord: `runtime/seal/link.rs` with `PREFIX = "link"`, `buttons(here, other)` (**Link these threads**, Primary;
     **Not needed**, Secondary; custom ids `link:<yes|no>:<here>:<other>`, both slugs checked with
     `theseus_protocol::threads::slug_ok` in `parse`; mind Discord's 100-char custom id limit: a slug is short, but
     test the longest pair `slug_ok` allows, and if it can exceed 100, carry the pair key instead), `parse`, and
     `Shared::link_press`: **yes** calls `thread.link` with `ThreadLinkParams { slug: here, other, author: Some(who),
     discord }` as the presser and answers `🔗 \`here\` and \`other\` are linked: their notes pass between them.`;
     REFUSED answers as `seal.rs`'s `failed` does (share it: make `failed` `pub(super)`). **No** writes nothing (the pair
     is already marked asked) and answers `Not linked. Jev does not ask about this pair again.`.
   - `courier.rs`: `"link_card"` joins the owner-DM kinds list (→ `jev_post`), and its card arm is
     `(text, format!("link:{}", a.correlation_id), Buttons::Link(here, other))`; `Buttons::Link(String, String)` in
     `render.rs` renders through `seal::link::buttons` at both places `Buttons::Seal` does. One arm in `earned.rs`'s
     dispatch chain.
2. **`/thread link` and `/thread unlink` (theseus-98bz.12).** Two subcommands in `runtime/thread.rs`'s `command()`, each
   with required `a` and `b` string options ("The first thread" / "The thread to link it with"), dispatched in
   `thread_interaction` to `THREAD_LINK` / `THREAD_UNLINK` as the presser with `discord`; answers `🔗 \`a\` and \`b\` are
   linked.` / `\`a\` and \`b\` are no longer linked.`, and `⚠️ Nothing was linked: <message>` on an error. Update the
   fallback line and the names test.
3. **`born` trimmed (theseus-stpqg).** `born` becomes `HashMap<String, u64>` (task id → when it was born, unix ms). A
   born entry is needed only while the binding can still see that task ended with no `Made` row; after item 4, an ended
   view leaves the model `DONE_SHOWN_MS` after it ended. So in `add` (where `urls` is capped), drop born entries older
   than `DONE_SHOWN_MS + GRACE` whose task has no `Made` row. `next` reads `contains_key`. Never clear the whole map.
4. **Ended views pruned at a snapshot (theseus-0mq02).** In `Tasks::snapshot`'s `retain`, an ended view whose
   `ended_at_ms` (or `state_since_ms` when absent) is older than `DONE_SHOWN_MS` is dropped too, and its root is
   touched. A recent ended view stays (the existing `a_snapshot_keeps_an_ended_task_it_does_not_carry` must still
   pass unchanged).
5. **Snapshots in ask order (theseus-x4s7g).** `Tasks` gains an ask ticket: `ask` takes the next ticket (an
   `AtomicU64`) before its `work.watch` call and passes it to `snapshot`; `snapshot` applies only a ticket newer than the
   last one applied and ignores an older one (it returns no sessions). A position guard alone is not enough: two asks
   with different roots can answer at the same position. Keep `snapshot`'s signature change to the two callers and the
   unit tests.
6. **`tests_task_watch` asserts this root (theseus-x4s7g).** Add `Bus::work_watched(&self, root: &str) -> bool` (true
   when a watcher's scope has no roots or holds `root`; read-only) and make the probe record, at the first model call,
   whether the turn's session is watched (the request names its session, or read the place's session after the turn
   and record the watched roots at the call). Assert this session, not any watcher.
7. **`home_up` waits for the home to stand (theseus-0prmm).** Make `home_up` wait until the binding's task home state
   holds the channel (what `instead_of` reads), not just the META record. Also look at what happens in production in
   that window: if a board drawn in the DM before the home stands is left there after `wake_places`, that is a finding;
   say so in the report with the code path (do not fix it in this row unless it is a few lines in `task_home.rs`).

## Tests (each with a planted revert; the first is the key test)
- **`a_link_card_goes_to_the_owners_dm_and_links_the_two_threads`** (`tests_gateway/link.rs`, modelled on
  `tests_gateway/seal.rs`): the core stages a `link_card` for two threads; the owner's DM gets one card with the two
  buttons; the owner's **Link these threads** press leaves both threads holding each other and one `thread.linked`
  row; a press by someone else is refused (`🔐`) and links nothing; **Not needed** writes nothing. Plant: route
  `link_card` as a plain post with `Buttons::Keep` (or make the yes press call nothing): the test must fail.
- `a_pair_admitted_twice_is_proposed_once` (existing, updated): one `link_card`, the new text, asked once. Plant: stage
  the old `onto_card` body.
- `link_card_ids_round_trip` (unit, `seal/link.rs`): buttons → parse gives the pair; bad verbs, bad slugs and a missing
  part are refused; the longest allowed pair fits Discord's limit.
- `slash_thread_link_and_unlink` (gateway): `/thread link a: b:` links, `/thread unlink` unlinks, an unknown slug answers
  `⚠️`. Plus the names unit test in `thread.rs`. Plant: dispatch `link` to the fallback.
- `born_is_trimmed_but_never_reposts_a_task` (unit, `threads.rs` tests): a task added, finished and its row dropped is
  still not given a second post while its born time is recent; after `DONE_SHOWN_MS + GRACE` and another `add`, its
  entry is gone; a task with a `Made` row keeps its entry. Plant: never trim (the size assertion fails), and plant 2:
  trim regardless of age (the no-repost assertion fails).
- `a_snapshot_drops_an_ended_view_older_than_done_shown` (unit, `tasks.rs`): an ended view ended `2 * DONE_SHOWN_MS`
  ago, not in the snapshot, is gone after it; one ended a minute ago stays. Plant: keep every ended view.
- `an_older_snapshot_never_undoes_a_newer_one` (unit, `tasks.rs`): ticket 2's snapshot (roots A and B, B's view
  running) applied, then ticket 1's (root A only) arrives: B's view stays. Plant: drop the ticket check.
- `a_places_new_session_is_watched_before_its_first_turn_runs` (existing, strengthened to this session's root). Show it
  fails when `Place::adopt`'s awaited `ask` is removed (the plant the original test was written for).
- `tests_task_home`: run the whole file 10 times (`cargo nextest run -p theseus-discord tests_task_home
  --stress-count 10`, and read the summary: `--stress-count` exits 0 even with failures) after the `home_up` change;
  test-only, no plant.

## Not in this row
- Any change to `thread.link`'s judgment, the lens, or how links change recall (`rpc/thread_link.rs`,
  `thread_recall.rs`, `turn/cross_step.rs`).
- The Discord board's progress words and ages (`runtime/board.rs`, `render/board.rs`): another row of this batch
  (tail-hold) owns them.
- Files the threads lane is changing now: `runtime/{prompt,steer,thread_bind,voice}.rs`,
  `tests_gateway/{ask,cards}.rs`, and the core's `rpc/thread_*` modules. Do not touch them; if you need a change there,
  name it in the report.
- A link proposal for a pair already asked before this row (they were asked once as text; they stay asked).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code).
  Stored data stays readable.
- **On, measured, with a named rollback.** The link card ships on; its rollback is the existing
  `[threads] link_proposals = false` (nothing is counted, written or posted, as today). The other items are fixes with
  no switch.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass). This row adds no model tool and no model-facing text. FAST: nothing new on a
  plain turn (`tests_m3::a_plain_turn_stays_within_its_frame_budget`), nothing on the path before a request leaves
  (the count stays in its spawned task after the recall).
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A test-only change needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling (`urls`' 512 stays;
  `runtime.rs` gains no line). Updating an assertion to a new text is not weakening it: say which you updated and why.
  Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its
  output in the report. It must be empty.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change
  (nothing here is stored; if you find you must store something, say why and bump by one from the base's 48, adding
  the old layout's sample to `crates/theseus-core/src/tests_layouts.rs` as its AGENTS.md says).
- **Secrets and names.** Run anything that leaves (a post, a fact, a report) through `core.tools.scrubber` as the
  existing proposal does. No person's name anywhere in the repo, in commit messages or in the report: write "the
  owner". Fixture slugs, titles and ids are invented.
- Read `crates/theseus-discord/AGENTS.md` and `CLAUDE.md`, and `crates/theseus-core/AGENTS.md`, before you start.
  Keep CLOUD_REPORT.md honest: what you ran, what passed, what you could not run on the VM (the Discord gateway tests
  run against the in-process fake; L1 is not involved), with the exact commands for the maintainer for anything left.
- Line ceilings: `scripts/long-files.txt`; `scripts/shape.sh` checks them. Put new code in new files.

## Your report
Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc; check its size after the commit, it
must not be empty): what changed per Beads id; each test and its plant, with the test that failed; what you could not run
on the VM and the exact commands for the maintainer; the keel guard's output; the store format (unchanged at 48, or
why not); the task-home finding of item 7 (or "none: the DM board is <removed/never drawn> because <code path>");
follow-ups. Push the branch and end with a short final message that starts `CLOUD REPORT COMPLETE`.
