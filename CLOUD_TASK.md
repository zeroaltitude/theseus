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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail5-t4fix`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base for this wave (it wins over anything below about `main`)

Your branch starts from the lane `integ/tail4` at `795afd3b`, not from `main`: the code this row fixes was joined there
today and is not on `main` yet. `integ/tail4` sits on `main` `8260dbab` (release stage 2) and lands as the next release
stage; `main` has since moved to `f1fc9b03` (stage 3: `integ/tail3` and the threads lane's round 4). Line numbers quoted
below are at `795afd3b` but may still be off by a few: search by name. Run the keel guard against `795afd3b`
(`--base 795afd3b`), not against `origin/main`.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This row adds no
stored field; if you find you need one, stop and say so in the report instead.

**What your base holds beyond `8260dbab`** (`git log --first-parent 8260dbab..795afd3b` lists them): the tail4 rows
joined so far: the voice warm track (`crates/theseus-voice/src/warm.rs`), the TUI and herdr fixes (`herdr up` shows the
workspace, `c` on the board), thread titles in the pre-router (`crates/theseus-core/src/thread_title.rs`), the Discord
approval card (`render/approval.rs`) and default forum, and the test scratch dirs (`crates/theseus-core/src/scratch.rs`).

**Line ceilings** (`scripts/long-files.txt`; length / ceiling): `crates/theseus-discord/src/runtime.rs` 3,494 / 3,500
(add nothing), `crates/theseus/src/render.rs` 3,126 / 3,128. An unlisted file stays at 2,500 lines or fewer. Never raise
a ceiling yourself (the keel guard flags it).

**Work in flight beside you:** the `integ/tail4` lane may still take join fixes while you run; keep each edit small and
local so your branch merges onto its later tip. Do not touch the Discord approval card's words or layout
(`crates/theseus-discord/src/render/approval.rs`): the owner locked them.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail5-t4fix, the tail4 reviews' small fixes: the TUI's job rows, herdr up's focus error, the warm track's eviction, the scratch sweep (theseus-i7kmb, theseus-kg4kd, theseus-kbfl0, theseus-7nx13)

Branch: `cloud/20261021-tail5-t4fix`. Every commit's subject carries the Beads id of what it changes (`theseus-i7kmb`,
`theseus-kg4kd`, `theseus-kbfl0`, `theseus-7nx13`). Deadline for the report: 3 hours after you start.

**Why.** Four rows joined on the lane `integ/tail4` today; their reviews each left a few small, concrete fixes. They fix
code that is on `integ/tail4` only, so this row starts there (see the note above).

**Risk: low.** Each item is a few lines and a test.

**Prerequisite: base `integ/tail4` at `795afd3b`.** Your branch joins after `integ/tail4` lands (the maintainer decides
whether into `integ/tail4`'s last round or the lane `integ/tail5`). Run the keel guard against `795afd3b`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail5-t4fix` | theseus-i7kmb (1-3), theseus-kg4kd (F2, F3), theseus-kbfl0 (F2), theseus-7nx13 (F5) | the TUI's work view compares (position, at_ms); `herdr up` survives a focus error; `arm`'s ended check falls back; the warm track's eviction stops tracks; a template comment; the scratch test sweeps its own dir only; two nits | `crates/theseus-tui/src/work.rs` (+ its tests, `tests_work.rs`), `crates/theseus-tui/src/app.rs` (`arm` only), `crates/theseus/src/herdr_up.rs` (`show_workspace`), `crates/theseus/tests/herdr_up.rs`, `crates/theseus-voice/src/warm.rs`, `crates/theseus-voice/src/songbird_io.rs` (the clip's `add_event` error branch only), `crates/theseus-core/config/theseus.example.toml` (one comment line in `[voice]`), `crates/theseus-core/src/scratch.rs`, `crates/theseus-core/src/thread_title.rs`, `crates/theseus-core/AGENTS.md` (one line re-wrapped) | M |

## What exists (at `795afd3b`; search by name)
- **i7kmb 1.** `crates/theseus-tui/src/work.rs` (487 lines) near :87 `pub fn apply(&mut self, v: WorkView) -> bool`
  compares a job's position only; the daemon's work sample keeps the position and takes a later `at_ms`
  (`crates/theseus-core/src/push/work.rs`), so sidebar job rows may drop every later sample. The chat footer had the same
  bug, fixed by theseus-hwv3t in `crates/theseus-tui/src/chat/tasks.rs` `work_changed` (compares position and `at_ms`):
  read that fix and match it.
- **i7kmb 2.** `crates/theseus/src/herdr_up.rs` (700 lines) near :135-160 `show_workspace(ctx, focus)`: an error from
  `workspace.focus` or the second `workspace.list` fails `up` with `?` after the tabs are built. `crates/theseus/tests/herdr_up.rs`
  drives `up` against a fake herdr.
- **i7kmb 3.** `crates/theseus-tui/src/app.rs` (1,973 lines) near :1344 `fn arm(&mut self, arm: Arm)`: the ended check
  reads `board.work.of_session`; when the work view is trimmed (the job is not in it) the old prompt comes back instead
  of "that task has already finished", with no fallback to the execution view's state.
- **kg4kd F2.** `crates/theseus-voice/src/warm.rs` (337 lines): near :171 `Warm::add(&mut self, track)` evicts the oldest
  kept track without stopping it; near :179 `stop`; the trait near :114 has `fn stop(&self)`; tests near :205 use a fake
  track. `crates/theseus-voice/src/songbird_io.rs` near :555-575: the clip path calls `add_event` for the warm track and
  then `track.add_event(Event::Track(TrackEvent::End), ...)`; when that fails, `Heard::Ended` goes direct and
  `AfterClip::ended` never runs, so the clip's warm track stays paused in the mixer until the call ends.
- **kg4kd F3.** `crates/theseus-core/config/theseus.example.toml` near :954, the `lead_in_ms` comment ends in a fragment
  ("Between a reply's sentences silence keeps playing").
- **kbfl0 F2.** `crates/theseus-core/src/scratch.rs` (138 lines): near :75 `fn sweep()` removes every dir in the
  process-wide `MADE` list; the test near :130 `a_made_dir_is_swept` calls it, so under `cargo test` (one process, many
  threads) it would delete other running tests' scratch dirs.
- **7nx13 F5.** `crates/theseus-core/src/thread_title.rs` near :41 `title.dedup()` drops only adjacent repeats;
  `crates/theseus-core/AGENTS.md` line 579 (116 characters with its newline) is past the file's wrap width: re-wrap that
  paragraph's lines only.

## What to build
1. **i7kmb 1.** `Work::apply` treats a later `at_ms` at the same position as a change (as `work_changed` does), and a
   test shows a job row's last line advancing through two samples at one position.
2. **i7kmb 2.** In `show_workspace`, an error from the focus call or the second list prints one line saying the
   workspace was built but could not be shown (and why), and `up` returns `Ok`. Test with the fake herdr refusing
   `workspace.focus`.
3. **i7kmb 3.** `arm`'s ended check falls back to the execution view's state when the job is not in the trimmed work
   view. Test: a finished task absent from the work view, `c` says it has already finished.
4. **kg4kd F2.** `Warm::add` stops the evicted track; in the clip path's `add_event` error branch, stop that clip's warm
   track. Tests with the fake track: an eviction stops the oldest; the error branch leaves no warm track playing or
   paused.
5. **kg4kd F3.** Finish the comment: "Between a reply's sentences silence keeps playing (0.7 s)." (match the constant
   `WARM` in `warm.rs`; if it is not 700 ms, write its value).
6. **kbfl0 F2.** Split the sweep: `remove_all(paths)` does the removal, `sweep()` calls it with `MADE`'s list, and the
   test calls `remove_all` on its own path only (or removes only its own path from `MADE`). No test may sweep the
   process-wide list.
7. **7nx13 F5.** `thread_title`'s dedup removes every repeat, keeping first-seen order (sort only if order does not
   matter there: read the caller); a unit test with a non-adjacent repeat. Re-wrap AGENTS.md:579's paragraph.

## Tests (each with a planted revert)
- **`a_job_rows_last_line_advances_at_one_position`** (TUI `work.rs`). Plant: compare position only.
- `up_survives_a_refused_focus` (`tests/herdr_up.rs`). Plant: restore the `?`.
- `c_on_a_finished_task_missing_from_the_work_view_says_finished` (TUI). Plant: drop the fallback.
- `an_evicted_warm_track_is_stopped` and `a_clip_whose_end_event_fails_stops_its_warm_track` (voice). Plant: drop each
  `stop()`.
- `a_made_dir_test_sweeps_only_its_own` (core `scratch.rs`): make two dirs, remove one through the test's path, the
  other still exists. Plant: call the process-wide sweep.
- `a_titles_repeated_word_counts_once` (`thread_title.rs`). Plant: `dedup()` back.
- `cargo nextest run -p theseus-tui`, `-p theseus herdr`, `-p theseus-voice`, `-p theseus-core scratch`,
  `-p theseus-core thread_title` pass.

## Not in this row
- kg4kd F4 (a `[voice] warm_ms` key): only if the owner's live check finds gaps past 0.7 s; list it as a follow-up.
- 7nx13 F4 (title hits versus the chain term) and the apart-thread title: owner/patch2 decisions; leave them.
- i7kmb 4, kg4kd's and 7nx13's live checks: the maintainer's, after install.
- theseus-kbfl0's host `/tmp` sweep and the recall progression's regeneration: local tooling and measurement.
- The Discord approval card and its follow-ups (theseus-hkq9q): held for the owner.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Near you:** the `integ/tail4` lane may still take join fixes while you run (`git log --oneline 795afd3b..origin/integ/tail4`
  shows them; read any that touch your files and keep your edits clear of their hunks). Do not touch Discord files.
- **Rows beside you** (on `main` f1fc9b03, joining `integ/tail5`): tail5-effort (core `routing/effort.rs`,
  `config/routing.rs`, the template's `[routing]` section), tail5-threads (core threads files, TUI `chat/events.rs`,
  `chat/tests_thread_ask.rs`), tail5-discord (Discord task home). Stay out of their files; your template edit is in
  `[voice]` only.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base 795afd3b --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; this row adds no stored field.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). One commit per Beads id. Put the Beads
  id in each subject. End each message with the same `Co-Authored-By:` trailer line the base's recent commits carry
  (copy it from `git log -3 --format=%B 795afd3b`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture ids are invented.
- Read the AGENTS.md of each crate you touch first. Run `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what changed per Beads id and item; each test and its plant with the failing line; the keel guard's
output; the store format (unchanged); `docs/status.md` sentences for the integrator (if any item is owner-visible: the
herdr line); follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
