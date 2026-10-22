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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail2-google`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: tail2-google, four Google follow-ups: a write into a shared folder asks first, every call's `google.called` row is kept, small Sheets/Slides fixes, Gmail-added events are not the owner's text (theseus-urpt2, theseus-2j9dr, theseus-nbxwz, theseus-d3ysk)

Branch: `cloud/20261021-tail2-google`. Every commit's subject carries the Beads id of what it changes (`theseus-urpt2`,
`theseus-2j9dr`, `theseus-nbxwz`, `theseus-d3ysk`). Deadline for the report: 4 hours after you start.

**Why.** The reading reviews of the Google part 2 rows (gw-drive, gw-calendar, gw-sheets-slides) left four follow-ups.
Each one is either a permission the owner did not grant, a call the ledger never saw, or another person's text read as
the owner's:
- **A write into a shared folder shares the file without asking (theseus-urpt2).** `drive.write` creates a file with
  `folder` = a folder, or moves one with `move_to` = a folder. If that folder is shared (or sits in a shared drive),
  the folder's members get access to the file by inheritance. But the plan of `drive.write` is posture-only (no
  `authority`), and `from_path` can upload any readable workspace file. The Google design puts every permission change
  under the owner's authority (`drive.share` always asks). This is a permission change by another road.
- **The ledger undercounts real calls (theseus-2j9dr).** A Drive tool that makes several calls keeps only the last
  call's `google.called` row. A Sheet read makes three calls (`drive.files.get`, `sheets.spreadsheets.get`,
  `values.batchGet`) and records one. A Slides read, a move (the parents `files.get` and then the PATCH) and a revoke
  (`permissions.list` and then the DELETE) also keep only their last call. Several paths make a successful call and
  then fail with no row at all: a folder or another native type after `files.get`, "the spreadsheet has no tabs", "no
  such access to remove", the save_to write failing, a move whose PATCH fails after a content replace went through.
  Also: the model's raw `q` is wrapped in parentheses but its own parentheses are never checked, so `a) or (b` escapes
  the grouping and `trashed = false` and the typed fields stop constraining the query.
- **Small Sheets/Slides fixes and one ledger convention (theseus-nbxwz).** (1) A Slides write on an existing deck
  whose notes batch fails says "The deck X was made, but the call then failed", which is false: this call did not make
  it. (4) `fact/google.rs` records every row of `google_called_also`, but no test shows the ledger getting N rows.
  (5) `sheets.write` without an `id` sends its values to `/v4/spreadsheets//values/...` when the create answer has no
  `spreadsheetId`. (6) The ledger has two conventions for a tool that makes several calls: mail's `merged()` (one row,
  attempts and units summed) and `google_called_also` (one row per call). Drive and calendar's `failed()` drop earlier
  rows. Pick one.
- **Gmail-added events may read as the owner's own (theseus-d3ysk).** `calendar.events` treats an event as the
  owner's text when the account organizes it (`organized_by`: `organizer.self` or the organizer's email). Events Gmail
  adds from mail (`eventType: "fromGmail"`: flights, reservations) carry text from a stranger's email in their
  summary, location and description, and Google may still name the account as their organizer. That text would reach
  the model as not outside text, past the hold.

**Risk: low to medium.** The one behaviour change a person sees: `drive.write` into a shared folder now stops and asks
(or refuses). Everything else adds rows to the ledger, corrects a message, or marks more text as outside text. No
store layout change: `google.called` rows keep their shape, there are just more of them.

**Prerequisite: `main`.**

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail2-google` | theseus-urpt2, theseus-2j9dr, theseus-nbxwz, theseus-d3ysk | the target folder's sharing read before a create or a move, with an explicit `into_shared` that asks the owner; every Drive call's row kept on success and on failure; raw `q` balance check; Slides `created` flag, Sheets missing-id refusal, a ledger test for N rows, one multi-call convention; `fromGmail` and someone else's events are outside text | `crates/theseus-core/src/google/{drive,sheets,slides,calendar}.rs`, `crates/theseus-core/src/google/{tests_drive,tests_sheets_slides,tests_calendar}.rs`, `crates/theseus-core/src/fact/google.rs` (tests only, if any), `crates/theseus-google/src/fake/{drive,calendar}.rs` (only what the tests need) | M |

## What exists (lines at `6918e3c1`; search by name, a line may have moved)
- **Drive's tools.** `crates/theseus-core/src/google/drive.rs` (1,405 lines). Constants near :10-55 (`FOLDER`, `DOC`,
  `SHEET`, `SLIDES`, `FIELDS` with `shared`). `fn failed(f: CallFailed) -> ToolFailure` near :257 puts only
  `f.row` in the meta. `async fn call(g, m, req) -> Result<Called, ToolFailure>` near :554 is `g.call(m,
  req).await.map_err(failed)`. `fn plan` near :453: `Op::Write` builds a Plan with `summary` and the `from_path`
  read resource only (no `authority`, no `class`); `Op::Share` sets `class: Some(ToolClass::Write)` and
  `authority: Some(..)`: copy that shape. Input schemas near :378-432 (`Op::Write` properties: `id`, `name`,
  `folder`, `content`, `from_path`, `as`, `move_to`, `rename`; `additionalProperties: false`). Descriptions near
  :354-372.
- **Read.** `Reading::run` near :649: `files.get` (its row `info.row` is dropped), then per type a second call:
  `files.export` for a Doc, `self.sheet(g)` near :741 (`sheets.spreadsheets.get` for the tabs, then
  `values.batchGet`; returns only `vals.row`; "the spreadsheet has no tabs" near :776 returns with no row),
  `self.slides(g)` near :815 (`slides.presentations.get`), `files.get alt=media` for a binary. The folder error and
  the "a Google {type}: it has no text read here" error (near :663-693), the over-1-MB and not-text errors and the
  save_to write error all return a `ToolFailure::new(..)` with no row, after `files.get` succeeded. It ends
  `finish(g.account(), text, m, row, i.source())`.
- **Write.** `Writing::of` near :933 (checks: no `id` with `move_to`/`rename`; `id` with `name`/`folder`/`as`),
  `Writing::run` near :1056 (`finish(.., done.row, Source::None)`), `create` near :1075 (`m["parents"] = [folder]`
  when `folder` is set; one `files.create` or multipart upload), `change` near :1121 (one `files.get` with
  `fields=mimeType,parents` when it needs the type or the parents, then the content PATCH, then the move/rename PATCH;
  each `call(..).await?`, and only the last `done` returns).
- **Share.** `Sharing::run` near :1307: Trash and Grant make one call; Revoke makes `drive.permissions.list` then
  `permissions.delete`, and "no such access to remove" near :1377 returns with no row.
- **`q`.** `pub fn compile_q(q, text, name, folder, type)` near :171: pushes `format!("({q})")` unchecked, then the
  typed parts and `trashed = false`. Drive's query strings are single-quoted with `\'` escapes (`fn quote` near :139):
  a balance check must skip parentheses inside quotes.
- **The rows helpers.** `crates/theseus-core/src/google/sheets.rs` near :69: `pub(super) fn failed(f: CallFailed,
  earlier: Vec<Value>, note: &str) -> ToolFailure` (meta `google_called` = the failed call's row,
  `google_called_also` = the earlier rows, `private: true`, `outcome_unknown`) and `pub(super) fn with_rows(meta,
  rows) -> (Value, Value)` (the last row is the result's own, the rest go in `google_called_also`). `slides.rs`
  imports them (`use super::sheets::{failed, request, segment, with_rows};`) and its `struct Run { g, id, rows }`
  near :285 keeps a row per call (`Run::call` pushes `done.row`, a failure takes every row): this is the pattern.
- **The fact.** `crates/theseus-core/src/fact/google.rs` `of_result` (near :74) records every row of
  `meta["google_called_also"]` and then `meta["google_called"]`. `toolrun.rs` near :1818 calls it for every result,
  failed or not (`google/tests.rs` `a_failed_call_is_a_row_too_and_says_what_google_said`).
  `watch/tests_mail_surfaces.rs` near :32 shows a test feeding a meta straight in:
  `crate::fact::google::of_result(&r.core.rec(None), &out.meta)`, then `ledgered(&r.core, "google.called")`
  (helper in `google/tests.rs`).
- **Mail's convention.** `google/mail.rs` `fn merged(rows)` near :90 collapses a tool's calls into one row
  (`calls`, attempts and units summed; `tests_mail.rs` near :846 asserts `calls == 2`). **You do not edit
  `mail.rs`** (another lane changed it); see What to build 3.
- **Slides.** `slides.rs` `Run::after_create` near :346 picks "The deck {id} was made, but the call then failed: " by
  `self.rows.len() > 0 && !self.id.is_empty()`. `Run::create` near :320 sets `self.id` from the answer. On an
  existing deck (`id` given) the notes read's row is in `rows` too, so a failed notes batch claims the deck was made.
- **Sheets create.** `sheets.rs` near :463-503: `let id = j["spreadsheetId"].as_str().unwrap_or("")`, then the values
  write goes to that id even when it is empty.
- **Calendar.** `crates/theseus-core/src/google/calendar.rs` `fn organized_by(e, account)` near :147. Used by
  `render` near :215 (the "(you)" note beside the organizer), `listed` near :352 (`stranger = events.iter().find(|e|
  !organized_by(e, account))`; `own = stranger.is_none()` becomes `Source::Event { organizer_is_account }`) and the
  change path near :705. Each calendar tool makes one `g.call` per run (near :411 and :688), so calendar needs no
  rows change for theseus-2j9dr: confirm it and say so in the report. Fake:
  `crates/theseus-google/src/fake/calendar.rs` `stub` answers a list with `{"items": []}` unless a test gives a body
  (`Fake::json("GET", "/calendar/v3/calendars/primary/events", ..)`), as `tests_calendar.rs` does.
- **Fake Drive.** `crates/theseus-google/src/fake/drive.rs`: `File` with `pub shared: bool`, `File::shared()`,
  `File::theirs()`, `parents`; `meta()` near :113 answers `shared`, `parents`. There is no folder constructor; add
  `File::folder(id, name)` if your tests need one (mime `application/vnd.google-apps.folder`), and `driveId` in
  `meta()` only if you test the shared-drive case.
- **Tests.** `crates/theseus-core/src/google/tests_drive.rs` (helpers `tool`, `run`, `ok`, `drive_rig`, `query`,
  `plan_of` near :16-68; `share_and_trash_always_ask_whatever_the_posture` near :71 checks `plan.authority`,
  `Posture::Approve`, `!posture_only` at every posture: the pattern for the new authority test;
  `every_drive_tool_is_deferred_and_small` near :464 holds each Drive tool's definition to `MAX_TOKENS = 220`;
  `a_write_names_what_it_needs_and_a_path_it_reads` near :490). `tests_sheets_slides.rs` near :261 inspects
  `meta["google_called_also"]`. `tests_calendar.rs` near :174, :236, :365 count `google.called` rows.

## What to build
1. **theseus-2j9dr first: every Drive call keeps its row (this is the base the others build on).**
   - One convention for the whole of `drive.rs`: one row per call, the last in `google_called`, the earlier ones in
     `google_called_also`, on success and on failure. Use `sheets::failed` and `sheets::with_rows` (import them as
     `slides.rs` does). Thread a `rows: Vec<Value>` (or a small struct like slides' `Run`) through
     `Reading::run`/`sheet`/`slides`, `Writing::run`/`create`/`change` and `Sharing::run`: each successful call pushes
     its row; a failed call returns `failed(f, rows, "")`.
   - An error after a successful call (folder, other native type, no tabs, over 1 MB, not text, the save_to write,
     no such access) carries the rows too: add one helper beside `failed` in `sheets.rs`, e.g.
     `pub(super) fn refused(message, rows) -> ToolFailure`, whose meta is `with_rows`' shape plus `private: true`.
     Drive's own `failed` goes away (one way to build a failure meta).
   - `compile_q`: refuse a `q` whose parentheses do not balance outside single-quoted strings (honour `\'`), with a
     message naming the problem. A balanced `q` compiles exactly as today.
2. **theseus-urpt2: a file put into a shared folder asks the owner first.**
   - At run time, before a create with `folder` and before a move with `move_to`, read the target folder:
     `files.get` with `fields=shared,driveId,mimeType` (its row kept, per item 1). The folder counts as shared when
     `shared` is true or `driveId` is set (a shared drive's members see everything in it).
   - A shared target without consent is refused before anything is written, with a message that says what it would
     do: e.g. "`<folder id>` is shared: a file put there is shared with its members. `into_shared: true` asks the
     owner first, or `drive.share` shares the file itself."
   - Add a boolean `into_shared` to `drive.write`'s input. When it is true, the **plan** carries
     `class: Some(ToolClass::Write)` and an `authority` sentence, exactly as `Op::Share` does, so the gate asks the
     owner at every posture; the run then skips the refusal. When it is false or absent the plan is today's.
   - `drive.write`'s definition must stay within `every_drive_tool_is_deferred_and_small`'s 220 tokens: keep the
     schema entry bare (`{"type": "boolean"}`) and add at most a few words to the description. Never raise
     `MAX_TOKENS`. If it cannot fit, drop `into_shared`, keep the refusal (pointing at `drive.share` and the owner),
     and say so in the report's first section.
   - `from_path` is unchanged (the plan's read resource keeps the floor and the roots).
3. **theseus-nbxwz: the small fixes and the convention.**
   - (1) `slides.rs`: `Run` gets `created: bool`, set in `Run::create`; `after_create` tests it instead of
     `rows.len()`.
   - (5) `sheets.rs` create: an answer without a `spreadsheetId` is a failure with the create's row (use the item 1
     helper): "the create answer named no spreadsheet id", and no values call is made.
   - (4) A test that a meta with three rows (two in `google_called_also`, one in `google_called`) makes three
     `google.called` ledger rows, in order, through `fact::google::of_result` (see the mail surfaces test for the
     rig). Put it in `tests_sheets_slides.rs` or `tests_drive.rs`.
   - (6) The convention is one row per call (`google_called_also`). Drive, Sheets and Slides use it after this row;
     calendar makes one call per run. Mail's `merged()` stays as it is in this row (you do not edit `mail.rs`): write a
     follow-up in the report ("mail moves to one row per call"), with the tests that assert `calls` it would change.
   - Items (2) and (3) of the issue (Slides `deleteText` on an empty placeholder, the layout→placeholder map) are live
     checks for the maintainer: list them in the report, no code.
4. **theseus-d3ysk: Gmail-added and other people's events are outside text.**
   - One function, e.g. `fn own_text(e: &Value, account: &str) -> bool`: `organized_by(e, account)`, and
     `e["eventType"] != "fromGmail"`, and the event's `creator` is absent or the account (`creator.self == true` or
     `creator.email` equal to the account, ignoring case).
   - `listed`'s `stranger` and the change path's `organizer_is_account` use it. `render`'s "(you)" note stays on
     `organized_by` (it says who organizes, which is still true).
   - Whether Google really marks a real `fromGmail` event `organizer.self` is the owner's live check: name it in the
     report; the code treats it as outside either way.

## Tests (each with a planted revert; the first is the key test)
- **`a_sheet_read_keeps_every_calls_row`** (in `tests_drive.rs`): a Sheet in the fake Drive, `drive.read` on it; the
  result's meta holds three rows in call order (`drive.files.get`, `sheets.spreadsheets.get`,
  `sheets.spreadsheets.values.batchGet`), and fed through `fact::google::of_result` the ledger gets three
  `google.called` rows. Plant: return to keeping only the last row; the test must fail.
- `an_error_after_a_call_keeps_its_row` (in `tests_drive.rs`): `drive.read` on a folder fails and its failure meta
  carries the `files.get` row; a revoke of an address with no access fails with the `permissions.list` row. Use the
  tool's `run_async` directly to see the `ToolFailure`'s meta (`run` keeps only the message). Plant: drop the rows
  from `refused`.
- `a_move_keeps_the_get_and_the_patch` (in `tests_drive.rs`): a move records both rows. Plant: keep only the last.
- `a_write_into_a_shared_folder_asks_first` (in `tests_drive.rs`): a shared folder in the fake. A create with
  `folder` = it and a move with `move_to` = it are each refused, no create or PATCH reaches the fake (check the fake's
  log), and the message names `into_shared`. With `into_shared: true` the plan has `authority`, `class` Write, and
  decides `Posture::Approve` at posture open (as `share_and_trash_always_ask_whatever_the_posture` does), and the run
  writes the file. An unshared folder writes exactly as today. Plant: skip the folder read; the refusal case must
  fail.
- `a_raw_q_must_balance` (in `tests_drive.rs`, or extend `find_compiles_typed_fields_to_drive_q`): `a) or (b` is
  refused; `name contains '(draft'` and `(a or b)` compile as today. Plant: drop the check.
- `a_failed_notes_batch_on_an_existing_deck_does_not_claim_it_made_it` (in `tests_sheets_slides.rs`): a slides write
  with an `id` and notes, the notes batch answered with an error by the fake; the message does not contain "was made".
  The existing "made, then failed" case on a new deck still says it. Plant: back to `rows.len()`.
- `a_create_answer_with_no_id_is_refused` (in `tests_sheets_slides.rs`): the fake answers the create without
  `spreadsheetId`; the tool fails with the create's row and no request goes to `/v4/spreadsheets//values`. Plant: drop
  the check.
- `three_rows_make_three_ledger_rows` (item 3 (4)). No plant needed (it pins existing code); say so.
- `a_gmail_added_event_is_outside_text` (in `tests_calendar.rs`): a listed event with `eventType: "fromGmail"` and
  `organizer.self: true` makes the list's source not the owner's (`organizer_is_account` false / an External marker),
  and so does an event whose `creator` is another address; a plain event the account organized and created stays the
  owner's. Plant: `own_text` back to `organized_by`.

## Not in this row
- `google/mail.rs`, `google/mime.rs`, `google/tests_mail.rs`, `crates/theseus-core/src/watch/*` and
  `crates/theseus-google/src/fake/gmail.rs`: another lane changed them. Mail's move to one row per call is a follow-up.
- Any `AGENTS.md`, `docs/status.md`, the spec, `theseus.example.toml`: the maintainer writes the docs. Put the lines
  they need in your report (the `into_shared` rule for the Google design's Drive section; the one-row-per-call
  convention; calendar's own-text rule).
- `turn.rs`, `turn/route_step.rs`, `theseus-protocol/src/lib.rs`, `google/external.rs`, `google/mod.rs`, the store
  layout, the cockpit.
- The live checks (a real `fromGmail` event, Slides `deleteText` on an empty placeholder, the placeholder map): the
  owner's.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias. Stored data stays readable.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure refactor needs no plant: say so.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). The Drive tools are deferred; their
  definitions stay within the 220-token test. FAST: nothing new on a plain turn (a plain turn stays 4 frames,
  `tests_m3::a_plain_turn_stays_within_its_frame_budget`), nothing on the path before a request leaves.
- **Line ceilings** (`scripts/long-files.txt` at `6918e3c1`): none of your files is listed, so each stays at 2,500
  lines or fewer (`drive.rs` is 1,405). Never raise a ceiling, a budget or a cap (the keel guard flags it, and this
  row cannot carry the owner's `Keel:` trailer): if something will not fit, split the code instead.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change
  (format 48 on your base; a bump would go to 49, and this row needs none).
- Commits: signed, the Beads id in each subject, and a `Co-Authored-By: Tabitha/Claude` trailer whose address is
  the noreply address at anthropic.com (as on `main`'s commits: `git log -1 --format=%B 6918e3c1` shows it).
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner".
  Fixture addresses use the `example.test` domain; ids and names are invented.
- Read the AGENTS.md of every crate you touch first (`theseus-core`, `theseus-google`) and the Google design section
  they point to. Keep CLOUD_REPORT.md honest: what you ran, what passed, what you could not run in the cloud (L1 needs
  a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM; this row needs none of them, so say which you
  ran). Run `cargo nextest run -p theseus-core google:: fact::`, `cargo nextest run -p theseus-google`, the tool-count
  and template tests (`cargo nextest run -p theseus-core config::`), and
  `cargo clippy --workspace --all-targets -- -D warnings` before the last commit.

## Your report
Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc; check its size after the commit, it
must not be empty) with: what changed per Beads id; each test and its plant, with the failing output's line; whether
`into_shared` fit in the 220 tokens (the measured count); the calendar check (one call per run, so no rows change); the
docs lines for the maintainer; the live checks for the owner; what could not run on the VM and the exact commands for
the maintainer; the keel guard's output; the store format (unchanged, 48); follow-ups (mail's one row per call, and
anything you found). Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
