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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-test-infra`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: test-infra, the Discord task-thread tests pass in a full suite, a gated test leaves no shell loop behind, and the personality exam's promise probes pass again (theseus-w3lba, theseus-ynfq9, theseus-kncci)

Branch: `cloud/20261021-test-infra`. Every commit's subject carries the id of the piece it fixes (`theseus-w3lba`,
`theseus-ynfq9` or `theseus-kncci`). Deadline for the report: 5 hours after you start.

**Why.** Three test problems slow every join of every other row, because a red gate that is not the row's own fault
has to be argued past by hand:
- **The Discord task-thread tests and the acceptance journey go red in a full suite on fresh hosts (theseus-w3lba).**
  On 32-core x86 lane hosts and in a cloud VM like yours, these failed in a full suite and passed alone:
  `theseus-discord` `tests_task_threads::{a_task_in_a_guild_channel_gets_a_thread_where_its_stream_lands_and_one_rename_at_the_end,
  a_shared_forum_shows_a_shared_places_tasks_and_never_a_private_ones, a_shared_channels_task_thread_is_shared_and_no_private_title_reaches_it,
  the_owners_message_for_an_ended_task_goes_to_the_conversation_with_the_shared_line}`,
  `tests_task_react::in_a_shared_channel_a_tap_on_a_tasks_thread_does_nothing`,
  `tests_war_room::a_shared_place_refuses_a_share_and_keeps_its_guests_rule` (each `timed out: a thread for the task`
  after 100 s, while the task itself finished and posted in `#lab`), and `theseusd`'s `acceptance_journey`
  (`step 3: the card of … came 5010 ms after the question`; and an `Option::unwrap` on `r.post(quay)`). Which ones
  failed varied per run. A cloud session on an unmodified base saw the first and the react test fail **2 of 3 runs in
  its cloud VM**, so your VM is a place where this reproduces. The owner's own machine is the outlier where they
  pass.
  **Part of this landed:** theseus-ihu7l (`76237a96`, 2026-10-20, "tests hermetic on fast hosts") found four causes
  of the same family: a scripted task ended before its thread was made (the rig now holds a task's ending answer until
  its place exists: `Places`/`HOLD` in `crates/theseus-discord/src/tests_task_threads.rs`), a thread begun after its
  task's turn started never adopted the turn (a product fix, `adopt_turn`), records of one millisecond ordered by
  random id on rebuild, and a `sleep` multicall in a user-service script. The 2-of-3 cloud reproduction was on
  `74a797ed`, **before** that join. Nobody has run the list above in a full suite on a fresh host since. So the first
  job is to find out what is still red on your base.
- **A gated test leaks a shell loop that never ends (theseus-ynfq9).** `gate(dir)` in
  `crates/theseus-discord/src/tests_task_threads.rs` makes a task's first step a `proc_run` of
  `sh -c 'while [ ! -f <dir>/go ]; do sleep 0.05; done'`; the test lets it go with `open_gate(&r)` (writes `go`). A
  test that panics, times out or is killed by nextest before `open_gate` leaves the job running: the job outlives the
  test's daemon and its temp dir (it is re-parented to the user's init), and with the dir gone `go` can never appear.
  One machine had **128** such loops, 5 to 7 hours old and one 2 days old, every temp dir gone; each forks `sleep`
  20 times a second, so the idle load was 10 to 12 on 16 cores, enough to make the gate's lifecycle bench miss. Six
  Discord test files use `gate`/`open_gate` (`git grep -l open_gate crates/theseus-discord`), and
  `crates/theseusd/tests/acceptance_journey.rs` has its own loop of the same shape (`rules()`, near line 59:
  `while [ ! -e {dir}/{gate} ]; do sleep 0.05; done`, opened by `Rig::open_gate(name)`), with the same leak.
- **The personality exam's promise probes fail on `main` (theseus-kncci).** `python3 -m unittest test_e2e` in
  `bench/personality` fails two tests, with the debug binaries of a batch and with an installed build of `51a05841`:
  `OnADaemon.test_the_manifest_and_the_wakes_are_read_from_the_daemons_own_record` (`initiative.promise` is False in
  `oracle` mode) and `OnADaemon.test_a_promise_without_a_wake_fails_and_a_wake_unasked_fails`
  (`initiative.nopromise` is True in `wrong` mode, where it must be False). The gate does not run these, so nobody
  saw them go red. The exam counts a turn's wakes as `wake.set` ledger rows whose session is the step's session
  (`bench/personality/exam_drive.py`: `wakes()`, `_session_of()`, `step()`'s `before`/`wakes_new`); `xchecks.py`
  `wake_created` needs `wakes_new >= 1`, `no_wake` needs 0. The suspects, from the time it broke (after the exam
  landed, `ea1f29a8`): a join that changed **which session or scope a wake's `wake.set` row carries** (wakes owned by
  an execution: `theseus-kernel/src/wake_owner.rs`, `wakes.rs`; a horizon/home session for wakes; the morning note), or
  how the stand-in's scripted `wake` call reaches the daemon. Both tests failing together (one sees no wake, the
  other sees one where none was asked) fits a row landing in the wrong place: a wake made in step N counted in step
  N+1's window, or under another session.

**Risk: low.** Tests and their rigs, plus at most one small product fix if a race proves real (then it is a bug a
person could hit, and it gets its own test).

**Prerequisite: `main`.**

## Your row
| Piece | Beads | Scope | Files |
|---|---|---|---|
| A | theseus-w3lba | find what is still red in a full suite on the VM; fix each cause at its root | `crates/theseus-discord/src/tests_task_threads.rs` and the test files that share its rig, `crates/theseusd/tests/acceptance_journey.rs`, and the product file a real race lives in (small, with its own test) |
| B | theseus-ynfq9 | a gated job ends when its test ends, however the test ends | `gate`/`open_gate` in `tests_task_threads.rs` (the one definition the Discord files import), the journey's loop in `acceptance_journey.rs` `rules()` |
| C | theseus-kncci | the two promise probes pass, for the right reason | `bench/personality/` (the exam's reading), or the daemon if the exam reads right and the row is wrong |

### A. The full-suite reds (theseus-w3lba)
1. **Measure first, on your unmodified base**, and save the numbers for the report. Build once, then run the
   Discord crate's whole suite three times and the journey three times:
   ```
   cargo nextest run -p theseus-discord --no-fail-fast 2>&1 | tee /tmp/d1.log     # then d2, d3
   cargo nextest run -p theseusd --test acceptance_journey --no-fail-fast 2>&1 | tee /tmp/j1.log   # then j2, j3
   ```
   (The repo's `.config/nextest.toml` caps 8 test threads and kills a test at 120 s; leave it.) Record, per run, which
   tests failed and their first assertion line. Also run the theseusd suite once in full
   (`cargo nextest run -p theseusd --no-fail-fast`) if time allows: the journey failed beside the rest of that crate.
2. **If all six runs are green:** the w3lba family is closed by `76237a96`. Say so in the report with the six
   summaries (`Summary [ …s] N tests run: N passed`), and spend the time on B and C. Do not invent a fix.
3. **For each test still red,** find the cause; do not raise the 100 s wait, a latency budget, or nextest's timeout,
   and do not add a retry or a flaky-list entry. Leads, by symptom:
   - `timed out: a thread for the task` while the task finished in `#lab`: the thread was never made for a task that
     was working, or was made and the test looked at the wrong list. ihu7l's rule: a place is made for a task that is
     working, never for one that has ended. Check whether a shared place's task (which gets no `proc_run`, so `gate`
     cannot hold it; see the `Places` comment) can still end before its place under the `HOLD` of 3 s on a loaded
     4-core VM, and whether the place count `Places::made` reads (threads plus non-deleted kind-0 channels) can be
     satisfied by another task's place in the same test. The fix holds the task by an event (the place exists), never
     by a longer sleep.
   - `the card … came 5010 ms after the question` (bound 2,000 ms, `acceptance_journey.rs` step 3, `ask()`): 5,010 is
     a 5-second timer firing, not a slow machine. Find the 5 s wait on the path from a question to its Discord card.
     One lead: `tokio::time::timeout(Duration::from_secs(5), mailbox.recv())` in `crates/theseus-discord/src/runtime.rs`
     (near line 2913): a card that misses a wake-up there is posted at the timeout. If a wake-up is lost, that is a
     product bug (a person's card arrives 5 s late): fix it so the card follows the question, with a unit test that
     fails on the lost wake-up. **`runtime.rs` has one line of room under its ceiling (3,499 / 3,500):** put new code in
     a module of its own under `runtime/`, and change only what you must in `runtime.rs`.
   - `Option::unwrap` on `r.post(quay)` (steps after `appear`, near lines 759, 788, 803): the forum post the test saw
     earlier is gone or renamed by then. Find out which, from the stand-in's record (`FakeDiscord` in
     `crates/theseus-sim`), before changing the test: a post that disappears mid-task is a product bug.
4. Each fix is shown by the test that was red: three full-suite runs green after it (the same commands), and the
   product fix's own test failing on a planted revert.

### B. A gated job never outlives its test (theseus-ynfq9)
- `gate(dir)` today: `sh -c "while [ ! -f {go} ]; do sleep 0.05; done"`. Make the loop end on any of three things:
  `go` exists; **the test's directory is gone** (`[ -d {dir} ]`, so a dropped rig's `TempDir` ends it); or **a time
  cap** past nextest's kill (300 s), so a test killed by SIGKILL, whose temp dir is never removed, still leaves
  nothing after five minutes. The loop also writes its own pid to `{dir}/gate.pid` (`echo $$ > …`) as its first act,
  for the test below. Keep it one `proc_run` with plain POSIX `sh`.
- The journey's loop in `acceptance_journey.rs` `rules()` gets the same three exits (its directory, its gate file,
  the cap). No pid file is needed there; the key test below covers the shape.
- Do not find the loop by its text in `/proc/*/cmdline`: another row of this wave (job-truths) moves a job's command
  text off every command line Theseus owns, which would turn such a check into one that matches nothing. Find it by
  the pid it wrote, and check `/proc/<pid>` (gone, or a zombie, `State: Z` in `/proc/<pid>/status`).
- If the rig (`Rig` in `crates/theseus-discord/src/tests_gateway.rs`) can end its core's running jobs on drop through
  an existing call, also do that; if not, do not add one: the loop's own exits are the fix.

### C. The promise probes (theseus-kncci)
1. Build the debug binaries the test needs: `cargo build -p theseusd -p theseus -p theseus-index -p theseus-sim`.
   Run `cd bench/personality && python3 -m unittest test_e2e -v` and confirm the two failures. (The stand-in model is
   `exam_standin.py`; no network or key is needed.)
2. Find where the `wake.set` row goes. Print, for one `oracle` run, every `wake.set` row's `session_id`, `scope` and
   position beside each step's session and its before/after positions. Then decide which side is wrong:
   - the exam reads the wrong field or window (a row's session now sits in `data` or `scope`, or wakes are counted
     before the turn's frame is written): fix the exam's reading in `exam_drive.py`, and keep `test_xchecks.py` and
     `test_plan.py` green;
   - the daemon writes the wake under a session that is not the conversation that asked for it: that is a product bug
     a person sees in `theseus wakes` too. Fix it at the source, with a core test that a wake set in a conversation's
     turn is recorded under that conversation.
3. Both e2e tests pass, and the `wrong`-mode test still fails if its scripted wake is removed (show it).

## Tests (each with a planted revert; the first is the key test)
- **`a_gate_left_shut_ends_with_its_rig`** (key test, `crates/theseus-discord/src/tests_task_threads.rs`): start a rig
  whose task runs `gate(dir)`; wait for the task to be working; read the pid from `gate.pid`; drop the rig without
  `open_gate`; the pid is gone (or a zombie) within 5 s. Plant: the old loop (`while [ ! -f go ]`) must fail it.
- `a_gate_ends_at_its_cap`: a unit test of the loop text with the cap set small through a parameter (e.g.
  `gate_capped(dir, secs)`, `gate` calling it with 300): with the directory still there and no `go`, the loop exits
  after the cap. Plant: drop the cap.
- Each w3lba cause you fix: its red test green three runs in a row, and for a product fix its own unit test with a
  plant. If all six measuring runs were green, no test is added for A: say so.
- The kncci tests: both `test_e2e` OnADaemon tests pass; a plant that reverts your fix makes them fail again.

## Not in this row
- The lifecycle bench budgets (theseus-m1r2w, measured on the maintainer's machine), the load flakes on the ARM host
  (theseus-fpvn8), lane disk use (theseus-rkh86). Other Discord work: rough edges in a forum post's life (another row,
  `runtime/forum.rs`, `runtime/threads.rs`); stay out of those two files unless a w3lba cause is in them, and then keep
  the edit small.
- Areas busy in other sessions: job command lines (`theseus-kernel/src/job.rs`, `job_l1.rs`), task questions and
  reports (`owner_ask`, `turn/join_step.rs`, `task.close`), the TUI chat (`crates/theseus-tui`), herdr
  (`contrib/herdr`), the Google rows (`crates/theseus-google`), the threads rows (`thread_route.rs`,
  `thread_recall.rs`).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias. Stored data stays readable.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass). FAST: nothing new on a plain turn (a plain turn stays 4 frames,
  `tests_m3::a_plain_turn_stays_within_its_frame_budget`), nothing on the path before a request leaves.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap, timeout or ceiling (a test's own
  wait included). A flaky-list entry is not a fix. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It must be
  empty.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner".
  Fixture paths and names are invented.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (`crates/theseus-discord/AGENTS.md`, `bench/personality/README.md`).
  Keep CLOUD_REPORT.md honest: what you ran, what passed, what you could not run in the cloud (L1 needs a non-root
  daemon with a job cgroup: about 33 L1 tests fail on this VM; none of this row's tests should need L1).

## Your report
Commit `CLOUD_REPORT.md` at the repo root (written with your editor tools, never a shell heredoc; check its size after
the commit, it must not be empty), push the branch, and end with a final message that starts `CLOUD REPORT COMPLETE`.
The report holds:
1. Any file left over its line ceiling, with the reason (first section; "none" if none).
2. **The measurement** of A: per run (d1-d3, j1-j3, and the full theseusd run if made), the failing tests and their first
   assertion line, or the green summary line.
3. What changed, per piece (A, B, C), with the cause found for each red and for kncci (which side was wrong).
4. Each test and its plant, and the test that failed on it.
5. What could not run on the VM, with the exact commands for the maintainer (for example a full workspace run).
6. The keel guard's output.
7. Follow-ups: anything found and not fixed, each with what a person would hit.
