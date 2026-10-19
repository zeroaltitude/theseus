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

**Other changes in flight.** Several other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

Merged or merging into `main` tonight (your clone may hold some of them already): turn-end facts, the shared tool
catalog (speed-send-path), the store's one blocking wait (speed-idle-size), the fleet's full-screen view, a self task's
acts in its own tree, the task's home thread, and the owner's needs-you cards.

Other cloud sessions like you, each on its own branch: voice at the join (theseus-voice, theseus-discord's voice
runtime); the ontology in core; the job truths; per-thread dynamic context (thread rows) if they launch beside you.

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 45 as you clone it; this row should need no bump), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-timing-flakes`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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


## Your task: timing-flakes, the tests that go red under load become deterministic (theseus-1b2zm, theseus-nyczg)

Branch: `cloud/20261019-timing-flakes`. Every commit's subject carries `theseus-1b2zm` (or `theseus-nyczg` for the
defer test). Deadline for the report: 6 hours after you start.

**Why.** The gate's suite goes red under load on a family of timing tests, each in code the joined change never touched,
each green alone. Every red costs a full rerun of the gate, and some joins have been landed over them, which teaches
nobody to read a red. Each one either times something with the wall clock or reads state before an ordering guarantee
holds. Make each deterministic **at its cause**: a paused clock, an explicit sync point (wait on the event, not on a
duration), an ordering guarantee in the product, no wall-clock sleeps, no reaping races. **Never** by a retry, a longer
timeout, a larger budget, or a flaky-list entry (.config/nextest.toml's list is empty; keep it so).

**First, classify each failure.** For each test read the failing assertion before you touch it:
- A **timing bound** (something must happen within N ms): replace the wall clock with tokio's paused clock
  (`#[tokio::test(start_paused = true)]`, sleep steps, assert on `tokio::time::Instant` elapsed) or with an event the
  test awaits. Note: under a paused clock a running `spawn_blocking` task holds auto-advance (tokio's blocking scheduler
  inhibits it), so blocking store work costs no virtual time; a timer still fires at its virtual time once all is idle.
- A **negative or ordering property** (a row written but the map not yet updated; "never X"; a child reaped by someone
  else): one red means the bad interleaving really happened. That is a product race until shown otherwise: find it,
  reproduce it (a planted delay or a barrier at the window), and fix the order in the product, not in the test.

## The family (failing line on the owner's machine; lines move)
1. theseus-core `tests_task_checkpoint::a_slow_verdict_never_holds_a_loop_and_applies_when_it_comes` (tests_task_checkpoint.rs
   ~584, "applied when it came": a 500 ms verdict against 60 ms loops applied 7 loops late under load). A partial fix
   ("at least three loops on the base after the ask, move by loop 16") is on main and it still went red under load
   after it: finish it with a clock the load can't move.
2. theseus-core `watch::tests_reply::words_match_as_text_or_a_pattern_and_with_from_both_must_hold` (watch/tests_reply.rs
   ~205): two `watch.met` rows were written but `watches.get(id)` still held the met watch: the met row is written
   before the watch leaves the map. Ordering, likely in the product.
3. theseus-core `tests_horizon::a_met_watch_ends_done_and_status_short_counts_the_future` (tests_horizon.rs ~423):
   `status_short().next_due_ms` is `None` right after `wake.create`: the status cache is behind.
4. theseus-core `tests_work_digest::the_slot_waits_behind_a_just_written_unprompted_post` (~477): the slot was claimed
   while the pacer's window was closed.
5. theseus-store `store::tests_frame_times::the_slowest_frame_since_a_mark_is_the_one_made_slow`: an fsync slower than
   the 40 ms delay frame the test plants.
6. theseus-core `tests_recall_words::a_failed_vector_search_leaves_the_words_and_a_stalled_index_the_deadline` (~180,
   2.07 s past a 2 s bound).
7. theseus-discord `tests_board::commands::task_autocomplete_ranks_needs_you_first_stops_at_25_and_asks_the_daemon_nothing`.
8. theseusd `tests/whole_replay.rs::a_whole_turn_replay_on_a_real_daemon_leaves_the_directory_as_it_was`: `proc.run`
   failed with `wait: No child processes (os error 10)`, the `(Err(e), End::Finished)` arm of the L1 job's wait
   (job_l1.rs): the L1 init's wait got ECHILD, so something else reaped it. That is a reaping race (a second waiter, a
   SIGCHLD handler, a `waitpid(-1)`, or a reaper thread): find who reaps it, and fix it in the product. If the L1 path
   can't run on this root VM (see "Known on this VM"), reproduce the race in a unit test of the wait logic, and give the
   maintainer the exact live command.
9. theseusd `tests/defer.rs::a_restart_reads_the_ledgers_tool_use_back_and_keeps_a_conversations_tools` (theseus-nyczg,
   defer.rs ~284): after `r.restart()` the `tool.defer` list said `warmed=true` but cli `turns=0` (want 2). Suspect:
   `tools_defer/warm.rs`'s read pages the ledger by tag through the index once it is shaped, and the rows the two turns
   wrote just before the stop may not be visible to the first page after a loaded restart (or the board's `live_since`
   skips them), so `set_warmed` lands with zero counts. A warm that reads nothing must not say warmed: that part is a
   product bug whatever the test does.
10. theseus-core `tests_reminders::a_snooze_from_a_button_sets_a_fired_reminder_again_and_it_fires_again`.

Also red once under the same loads, and in scope if the fix is the same shape (say which you took):
`rpc::tests_peek::last_line_goes_out_at_most_once_a_second_under_a_chatty_job`,
`tests_war_room::ending_a_task_thread_removes_the_guests_the_war_room_added` (meta read right after the members clear),
`fake_model::tests_paced::a_paced_answer_keeps_its_first_byte…`.

## What to build
For each test: one commit (or one per shared cause), its classification, the cause in one sentence, and the fix. Keep
every assertion and every bound the test has: where a bound is wall-clock, the same number holds on the paused clock.
Where a fix is in the product, add the test that shows the old interleaving (a planted delay, a barrier, or a `cfg(test)`
hook at the window) and that the fix closes it.

## The proof: under load, before and after (this is the key test)
For each test, run it 50 times under load at your base, then 50 times at your fix, and put both counts in the report:
- Load: the preamble's recipe (priority, not count, makes it): the test at `nice -n 19`, beside it four busy loops at
  nice 0 (`sh -c 'while :; do :; done' &`, killed by their pids). Where you have `stress-ng`, `stress-ng --cpu 8` at
  nice 0 is an equal choice; add disk pressure (`stress-ng --hdd 2` or a `dd` loop) for the store's fsync test.
- Command: `nice -n 19 cargo nextest run -p <crate> --retries 0 --no-fail-fast -E 'test(=<full name>)' --stress-count 50`.
  **`--stress-count` exits 0 even when iterations fail**: count the `FAIL [` lines, or read the
  `Summary … N passed, M failed` line. Never judge it by the exit code.
- A test that never goes red at the base on this VM even under load: say so, show your strongest attempt, and prove the
  fix another way (a planted delay at the window that fails the old test and passes the new one).
- KEY: the report's first table is test x (base failures/50, fix failures/50) under the load, with the load named.

## Tests (each with a planted revert)
- Each product fix: plant the old order back (the exact edit), show the named test fail, restore and `touch`.
- Each test moved to a paused clock: plant the behaviour it guards (e.g. the verdict made to hold a loop) and show the
  test still catches it on the paused clock. A paused-clock test that can't fail guards nothing.

## The checks for this row (these replace the preamble's gate line)
- `cargo fmt --all --check`; `bash scripts/shape.sh`; `cargo clippy -p <each crate you touch> --all-targets -- -D warnings`.
- `cargo nextest run -p <each crate you touch>` whole, once at the end (theseus-core is large on this VM: run it alone,
  in the background). `python3 scripts/keel-guard.py --base <your base> --head HEAD --quiet` empty: no assertion
  removed, no bound raised, no test ignored.
- Not here: the full gate, the benches, cargo-deny. Say so in the report.

## Rules for every row of this batch

- **Model roles:** you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller, and make an old config key fail the load with
  "X was renamed to Y". Stored data stays readable.
- **On, with a named rollback.** The behaviour ships on by default. Where the row names a switch, it is the rollback: a
  test shows that with it off the old behaviour is byte for byte back.
- **Planted reverts, with notes for the reviewer.** Every behaviour change gets a test and a planted revert: break it,
  see the named test fail, restore the file and `touch` it, then `git status`. The row's **key test** (named first under
  Tests) must be shown failing on its plant. In the report, give each plant as a note the reviewer can replay: the file,
  the exact edit (the line before and after), and the test that failed with its failure line. A pure move needs no plant:
  say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. **Never edit a
  keel-guarded file** (`scripts/keel-guard.py`, `scripts/keel-signers`, `scripts/test_keel_guard.py`,
  `crates/theseus-core/src/toolrun/guarded.rs`, `crates/theseus-core/src/config/self_improve.rs`,
  `crates/theseus-core/src/rsi/sealed*`, `crates/theseus-exam/exam/`, and the first request's budgets in
  `crates/theseus-core/src/first_request.rs`): any change there needs a signed commit with a `Keel:` trailer quoting
  the owner's yes, which this session can't make. If your design needs one, stop at that point and put the exact change
  in the report.
- **Signing:** you can't sign here. Commit without `-S`; sign nothing. The maintainer re-signs at the join.
- **First-request budgets stay** (cli/task 8,000, dm 15,000 tokens; `tests_first_request.rs` must pass on the
  defaults). A plain turn stays 4 frames. Nothing new on the path before a request leaves.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner"
  and "collaborator". Fixture names, guild and channel ids, and paths are invented. Run anything that leaves through
  `core.tools.scrubber` as the code already does.
- **Docs in code.** Where your change makes a crate's or the cockpit's AGENTS.md wrong, fix those lines in the same
  commit, in plain words, in the file's existing style. Read the AGENTS.md/CLAUDE.md of every crate you touch first.
- **CLOUD_REPORT.md last.** It is the branch's last commit, as the preamble says, and your final message starts
  `CLOUD REPORT COMPLETE`. Keep it honest: what you ran, what passed, what you could not run here and the exact commands
  for the maintainer.

