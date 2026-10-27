<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/ and the benchmark adapters under bench/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, the owner's machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own; bench/ has its README), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

**Setup** (about 15 minutes, once):
- First, in the foreground and alone, run `cargo --version` and wait for it: it installs the toolchain that rust-toolchain.toml names (about 30 seconds). Start no other cargo or rustup command until it is done: two at once collide in rustup's download directory. If it fails, run it again.
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- `npm ci` in cockpit/.
- `cargo build --workspace --all-targets` (about 9 minutes cold). Export `CARGO_INCREMENTAL=0`: incremental artefacts are most of a target directory's size and this VM has no room to spare.
- `cargo deny fetch`, so the gate's deny phase can run offline. If the fetch fails, skip that phase and say so in the report.
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.
- Never delete anything outside the repository and /tmp (nothing under /root/.rustup or /root/.cargo). This environment refuses some commands, and three refusals in a row stop the session until a person looks: when one is refused, take another route instead of retrying it.

**Known on this VM, and not yours to fix** unless your task names them (other changes fix them):
- About 33 L1 tests fail here: theseus-sandbox's contract tests, its bench's `spawn_100`, and theseusd's sandbox tests. The VM runs as root, and L1 refuses a root daemon's job that has no job cgroup (theseus-pv6i).
- Under load, these timing tests can fail. The flaky list in .config/nextest.toml is empty (verified at your base), so none retries: rerun a failing one alone, and name it in the report.
  - theseus-core: `tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up`; `learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy`; `tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn`; `tests_activation_pace::a_clean_stop_ends_the_warm_builds_waits`; and `term::tests::a_close_leaves_no_child_behind` (a negative assertion: keep its output);
  - theseus-core's aws/hands `runaway_mode_refuses_at_ten_times_the_hours_line` and `ten_times_refuses_and_a_cancel_still_runs` fail when a run crosses an hour boundary (they read the wall clock's hour, theseus-5a50): rerun them mid-hour;
  - theseus-kernel's `children::a_sweep_reaps_wrappers_and_orphans_and_never_an_owned_child` (a count of how processes were classed, so its failure is a finding) and theseus-aws-catalog's `one_service_decodes_in_under_5_ms`;
  - **theseus-discord is broadly flaky under parallel load** (theseus-a0kte, measured 2026-10-27): five distinct tests red across six runs with none repeating, at the same rate on plain main as on a merge. If a theseus-discord test reds once, rerun it alone and record both results; a single red there is not by itself a regression. Named in that measurement: `tests_default_forum::a_dm_task_with_a_channel_gets_an_index_card_linking_it`, `tests_task_home::a_voice_places_task_posts_nothing_in_the_voice_chat`, `tests_forum_archive::a_read_post_a_day_over_is_archived_and_locked_at_the_sweep`, `tests_task_react::pause_parks_a_running_task_without_a_report_and_resume_lets_it_report`;
  - theseusd: `bench_profile::a_first_byte_timeout_is_retried_inside_the_headless_turn` (a count: keep its output), and `stops::a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker`.
- A negative assertion ("nothing of X reached Y", "no process is left", "no retry") that fails even once is a finding, not a flake: keep its output, name it in the report, and don't retry it away.

Timing tests also fail here more often than on the owner's 16-core machine. Any failure outside the lists above is yours to explain.

**What main holds.** You clone main at `c36c437e` or later, with **store format 52** (`MANIFEST_FORMAT` in crates/theseus-store/src/store.rs:692 -- verified at your base; check it yourself before you bump it). Your clone has v1's milestones and everything the patch1, patch2 and patch3 bundles have joined, including, in the last two days: content flags (`9470df70`), the recall refill pin (`47b94066`), "Not this" -- the feedback leg of notify-work-feedback (`79a5f2d7`, theseus-o1h3f.2), one definition of a content item (`9cc63f93`), the RSI door's bounded git fetch (`5342a01c`, theseus-6vlr4), the static fan-out cue (`71b105aa`, theseus-z9mze.4), and the three rival-arm gauntlet cells with their docs (`f3fdd4eb`), and the Benchmarks CARD view hotfix (`c36c437e`, theseus-s5qwv, merged 2026-10-27 13:43).

**Your base, and the fact that it moves.** Your base is `origin/main` at `c36c437e` as this prompt was written, on 2026-10-27 at about 13:5x MST. **Main may have moved by the time you are fired, and that is normal.** A base one or two merges stale is expected. **Do not rebase and do not merge main into your branch** -- the maintainer merges. Instead, at your start, read `git log origin/main` and say in your report what landed after your base. Re-verifying every `file:line` in this prompt against your own clone is step 0 of your task, below.

**Other changes in flight.** These are being built elsewhere and join `main` one at a time while you work. Stay out of their areas unless your task needs it; if it does, keep the edit small and say so in the report.
- `integ/p3z-wave4b`: three wave-4 cloud rows being integrated on the owner's machine -- theseus-o1jnt (two x86-only reds), theseus-zsddv (scripts/gate.sh: `phase suite` moves to the end of `machine_checks`) and theseus-6s7ll (a Resume button on the self-build halt note, theseus-discord). It may land before or after you start.
- Two P0 cockpit hotfixes the owner asked for on 2026-10-27. **theseus-s5qwv** (the Benchmarks **card** view: stale and duplicate cards, junk headlines, a ragged grid) **LANDED at `c36c437e` and is in your base**; it changed `cockpit/src/lib/bench.ts`, `cockpit/src/lib/benchcards.ts`, `cockpit/src/lib/benchindex.ts`, `cockpit/src/components/BenchIndex.tsx`, `cockpit/src/views/Benchmarks.tsx` and three tests. **theseus-6ln2q** (the Graph view's collapsed thread groups overlap -- `cockpit/src/ops/graph.ts`, `GraphNode.tsx`) is still being written on the owner's machine. If your task touches the cockpit, read those paths before you edit near them.

**Shared files, and how to stay mergeable.**
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). **It must not move unless your task says to move it.** If your task genuinely adds a stored field, variant or record kind, bump it by one from main's number as you cloned it (**52** at your base) and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says; say why in the report. Others may bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling:** read scripts/long-files.txt and `wc -l` the files you touch before you add to them; a Rust file the list doesn't name fails past 2,500 lines. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line. **Measured at your base: `crates/theseus-core/src/turn.rs` is 3,512 lines of its 3,525 ceiling -- thirteen lines of headroom -- and `crates/theseus-protocol/src/lib.rs` is near its 2,733 ceiling. Two branches can each stay under a ceiling and break it once merged**, so if your change adds to either, say in the report exactly how many lines you added.
- **If you add a `mod` line, put it inside the sorted block.** Wave 4 lost a gate to a `mod` line appended after the sorted `mod` declarations: it died in `cargo fmt --all --check` at eight seconds, and no reading review would have caught it. **Run `cargo fmt --all --check` yourself before you report**, not only before you commit, and paste its result.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test. Regenerate it if you add or change protocol types. `git diff --check` reports trailing whitespace in protocol.gen; that is expected and is not yours to fix.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh) and count the commit green. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you would grow a file past its ceiling, split it: never raise a ceiling (the keel, below).

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, claude-sonnet-5-5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/BRANCH`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- **First, before any of that: push a start commit.** As soon as your setup and first build finish, make an empty commit (`git commit --allow-empty -m "session started"`) and push your branch. Do this even though you have nothing proved yet, and do it before you begin the work. **Why it is mandatory and not merely advised:** on 2026-10-24, three of sixteen rows ended with nothing on their branch but the task commit, and because every other row had pushed only at the very end, a session that died at minute 5 and one that died at minute 110 left byte-identical branches -- so there was no way to tell a row that never started from one that worked for two hours and lost it. A start commit makes those two cases distinguishable, and it costs one second.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, the owner's machine, accounts, or anyone else.
- **Write no person's name anywhere**: not in code, comments, docs, fixtures, goldens, commit messages or your report. Where a person is meant, write "the owner" (the handle `zeroaltitude` where an identity string is genuinely needed), "collaborator" for anyone else; the AI assistant may appear only as "Tabitha/Claude", and your commit trailer names only Claude. **This is not a formality and it has cost this bundle real time:** two waves ago a sibling row shipped real people's names in its test fixtures, and clearing them was a blocking join fix that held the merge. Grep your own diff for names before every commit.
- **Scrub over the range, not the tip.** When you check your branch is clean of names, check **every commit's tree and every commit's message** across your whole range (`git log --format=%B origin/main..HEAD` and `git diff origin/main..HEAD`), not just the working tree. A cleanup commit placed on top of the names it cleans still leaves them in the public history, and that is exactly how a name reached a public repository in this project once already.
- Don't edit the spec (docs/spec/), docs/status.md, the README, docs/benchmarks.md, docs/benchmarks/, or docs/design/. The maintainer writes those at review. Where a doc under those paths should change, say exactly what and where in the report, with the text you would have written.
- Don't commit half a step. If time runs out mid-step, leave it out and report what you found.

**The report:** when you are done, or at your task's deadline (by the clock, waits included), whichever comes first, write CLOUD_REPORT.md at the repository root. Commit it as the branch's last commit, subject `cloud report (not for main)`, and push. The maintainer reads it from the branch and drops that commit at the merge. For each step it says:
- what you found;
- **step 0's result: every citation in this prompt you checked, and every one that was wrong.** Name them even when they were right;
- what you changed (commit hashes);
- how you proved it: the commands and their results, with test counts, the runs under load, and **each planted revert with the exact `left` and `right` values the assertion printed when it failed**;
- the live check the maintainer should run, as exact commands, and what each should show;
- what is left or uncertain, and any design choice the owner should hear about;
- the result of `cargo fmt --all --check`;
- **Integrator notes: exactly which of your commits are for main, and which are not.** Name each commit by hash. This section is not optional and its absence sends the branch back.

Then the gate's result, naming each failing test and why. **The last line of CLOUD_REPORT.md must be exactly `CLOUD REPORT COMPLETE`** and nothing after it. Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line per step, and the gate's result.

**A well-argued STOP is a good outcome, and it beats a speculative fix.** In the wave before yours, a row came back "the defect does not reproduce, and here is the line-by-line argument for why", with no production change at all. That was the right answer and it was useful. If your reading of the code says the defect described below is not there, or is not there in the way this prompt describes, **stop, write the argument with `file:line` evidence, and do not invent a replacement task.** Do not manufacture a change so the branch has something on it.

**The keel.** The gate's keel phase (scripts/keel-guard.py; its docstring lists the rules) fails a range that deletes a test, adds `#[ignore]` or a skip, lowers a test's assertion count, adds an `#[allow]`, raises a bench budget, a spend or loop cap, or a ceiling in scripts/long-files.txt, scripts/shape.sh or .config/nextest.toml, or edits the guard's own files, unless a commit signed by the owner's key acks it. **You can't sign, so you can't ack.** Rewrite an assertion rather than delete it (a rewritten assertion is not a finding), split a file rather than raise its ceiling, and keep caps. Where your task itself changes a cap or a ceiling, or a test must change its meaning, make that change in a commit of its own whose message says why, and list it in the report under **Keel findings expected** (rule, path, why) -- and because the maintainer's `Keel:` trailer has to quote the owner verbatim, **give the maintainer the exact sentence you would want the owner quoted on.** Run `python3 scripts/keel-guard.py` before each commit and paste its findings into the report, **including when they are empty** -- an empty run is evidence too.

> **The stale-base trap, which bit two sessions in the wave before yours.** A bare `python3 scripts/keel-guard.py` run against a base that has moved prints findings for *other people's already-merged work*, and both of those sessions nearly reported that noise as their own. **Scope the guard to your own range.** Read the guard's docstring for how it selects a range, run it over `origin/main..HEAD` only, and if you see a finding in a file your branch does not touch, that is the trap -- say so in the report rather than acting on it.

**Planted reverts:** to prove a test guards a behaviour, plant the bug, show the test fail, then restore the file and `touch` it, so cargo rebuilds it (a restored file with its old mtime keeps the planted build). Run `git status` after every restore.

**Red-first is the rule, not a preference.** A test that would have passed before your change is not a test of your change. For every behaviour you change, show the test **failing at your base** first, then passing on your change, and put both outputs in the report **with the assertion's printed `left` and `right` values**. "It failed" is not evidence; the values are. A step whose report has no red-first evidence will be sent back.

**Load:** where your task asks for runs under load, use AGENTS.md's recipe. Priority, not count, makes the load: run the test with `nice -n 19`, and beside it four busy loops at nice 0, each `sh -c 'while :; do :; done' &`. Kill the loops by the pids you started, never by a name pattern.

**Locked surfaces, which no task in this wave may move:**
- **`crates/theseus-discord/src/golden/dm_flow.txt` is LOCKED.** The Discord DM task flow is the owner's, by his explicit standing instruction. That golden must come back **byte-identical**: `git diff --stat origin/main..HEAD -- crates/theseus-discord/src/golden/dm_flow.txt` must be empty. It is never regenerated. A task that seems to need a DM task message changed must **STOP and say so in its report** rather than change it.
- **`MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs` must not move** unless your task body says to move it.

**No AWS, no spend, no install, no benchmark run.** Do not run `theseus-sim bench` for numbers of record, do not touch any cloud account, and install nothing on any machine.

---
# Your task: find the shared mechanism behind theseus-discord's broad flakiness under parallel load (`theseus-a0kte`)

## BASE UPDATE, 2026-10-27 14:4x by the patch3 conductor (read this before step 0)

**Your base is `origin/main` = `e461c62d`**, not the `c36c437e` written elsewhere in this file.
`integ/p3z-wave4b` — listed above as "may land before or after you start" — **LANDED**, at
`e461c62d`. `theseus-o1jnt`, `theseus-zsddv` and `theseus-6s7ll` are all in main and closed.

Three consequences, and the first one has already bitten this prompt once:

1. **Line numbers in `crates/theseus-core/src/rsi/*` and `crates/theseus-discord/*` may have
   shifted.** The join changed `rsi/pr.rs`, `rsi/tree.rs`, `rsi/infra.rs`, `rsi/tests_pr/infra.rs`,
   `term/pty.rs`, `term/tests_idle.rs`, `judge/tests_content.rs`, `courier.rs`, `render.rs`,
   `runtime/halt.rs`, `runtime/self_note.rs` and `tests_gateway/self_note.rs`. One anchor in the
   `x1zz8` prompt moved by exactly ten lines (`rsi/tree.rs:180` → `:190`) and has been corrected.
   **Re-verify every citation at YOUR OWN base and report any that are wrong** — that check is
   doing real work, not ceremony.

2. **The gate's locked part now runs the SUITE LAST** (`theseus-zsddv`). The order is registry,
   protocol types, lifecycle, jobs, turn, size, then the suite. So **any instruction in this file
   to "run the phases after the suite yourself" is obsolete: there are none.** If the suite reds
   only on the known cases, the phases before it have already reported. A failed locked part now
   also prints `gate: NOT RUN, no verdict, after the failure in <phase>: <phases>`.

3. **`theseus-discord` is known to be flaky under parallel load** (`theseus-a0kte`, filed today
   with a reproducible recipe). A single unexplained red in `theseus-discord` or in
   `theseusd::...` is **not** automatically yours: re-run it alone before you treat it as a
   regression, and say in your report that you did. A test that passes alone in ~3 s and fails at
   ~105 s in company is that population, not your change.


**Branch: `cloud/20261027-p3y-discord-flake-mechanism`. Model `claude-sonnet-5-5`. Deadline: 6 hours from your start, by the clock, waits included.**

**Read this paragraph before anything else, because it defines what success is.** This row is a **diagnosis**, not a feature. Its deliverable is a mechanism named with `file:line` evidence, plus whatever fix that mechanism admits that you can prove red-first **on this VM**. **A report that names the mechanism and ships no code is a complete and valuable outcome.** A speculative fix that makes a flake rarer without naming why is not. The measurement that filed this row was taken on a different machine; you are not being asked to reproduce it, and you must not claim anything about that machine.

## Step 0, before anything else: re-verify every citation in this prompt

**Do this first and report the result.** Every `file:line` below was checked against `origin/main` = `f3fdd4eb` and re-checked against `c36c437e` on 2026-10-27 at about 13:5x MST. Open each in your own clone and say in your report what it actually was. **Name every citation this prompt got wrong**, even the ones that were right.

**One correction to the row's own notes, which matters for your scope:** the row speaks of "the ARM known.txt (19 entries)". **That file is not in this repository.** It lives on a build host you cannot see. Do not look for it, and do not propose edits to it. The in-repo equivalent is `.config/nextest.toml`'s flaky list, **which is empty at your base** -- verified, and the file's own comment at the top explains why a blanket retry would hide a real race.

## What was measured, and what it establishes

On 2026-10-27 the maintainer gated an integration branch on a build host. Because that branch changes `scripts/gate.sh`, the gate's door ran **main's** copy of the gate over the tree first and then handed to the branch's own copy. The side effect was a controlled experiment nobody designed: **the identical tree ran the identical 8,808 tests twice, back to back, on one host, minutes apart.**

```
pass 1 (door, main's gate.sh):   8,808 run, 8,808 passed, ZERO reds
pass 2 (the branch's gate.sh):   8,808 run, 8,806 passed, 2 failed

  FAIL [ 101.096s] theseus-discord tests_default_forum::a_dm_task_with_a_channel_gets_an_index_card_linking_it
  FAIL [   1.221s] theseus-discord tests_task_home::a_voice_places_task_posts_nothing_in_the_voice_chat
```

Same code, same machine, same test list. **Neither red is a deterministic regression, and that is settled by construction rather than by argument.**

A discriminating arm was then run -- the merged head and plain main, **concurrently**, so both arms saw the same neighbour load:

```
round | merged head                                  | plain main
------+----------------------------------------------+--------------------------------------------
  1   | 674 run, 0 reds                              | 670 run, 1 red: tests_forum_archive::
      |                                              |   a_read_post_a_day_over_is_archived_and_locked_at_the_sweep
  2   | 674 run, 1 red: tests_task_react::           | 670 run, 0 reds
      |   pause_parks_a_running_task_without_a_report_and_resume_lets_it_report
  3   | 674 run, 0 reds                              | 670 run, 0 reds
```

**One red in three runs on each arm. The same rate, and the base arm is unmerged main.**

**The finding: five distinct theseus-discord tests red across those six runs and not one repeated.** That is the signature of a broadly flaky package under parallel load -- not a defect in any one test, and not a regression. **The flakiness is pre-existing on main.**

All four named tests exist at your base, verified:

- `crates/theseus-discord/src/tests_default_forum.rs` -- `a_dm_task_with_a_channel_gets_an_index_card_linking_it`
- `crates/theseus-discord/src/tests_task_home.rs` -- `a_voice_places_task_posts_nothing_in_the_voice_chat`
- `crates/theseus-discord/src/tests_forum_archive.rs` -- `a_read_post_a_day_over_is_archived_and_locked_at_the_sweep`
- `crates/theseus-discord/src/tests_task_react.rs` -- `pause_parks_a_running_task_without_a_report_and_resume_lets_it_report`

**Why it matters.** It is a lane-health row. Every join gate pays a tax: roughly a one-in-three chance that some discord test reds and a reviewer spends a shift deciding whether their merge caused it. On the day this was filed that cost about forty minutes of host time and three rounds of arms. It also makes "the gate was clean" a weaker claim than it reads.

**One of the five reds is probably not in this population**, and you should keep it separate. The task post landing in the wrong channel is the signature of a separate, known, 5-7% task-card misplacement defect that the owner has deliberately shipped forward to the next release. **That one is not yours.** The 101-second red is a different shape again -- note that `.config/nextest.toml` sets `slow-timeout = { period = "60s", terminate-after = 2 }`, so nextest's own kill is at **120 s**; a 101 s failure is therefore a **test-internal** wait expiring, not nextest terminating it. That distinction is worth checking and reporting.

## What the row asks for, in order of value

### 1. Identify the shared mechanism. This is the task.

Five tests, five different assertions, one package. The hypotheses the row names are: **shared fixture state, a port or path collision between parallel rigs, or a fixed timeout that does not scale with `test-threads`.** The 101 s and 1.2 s shapes suggest **at least two** mechanisms, not one.

**Here is the reasoning that narrows it, which you should check rather than trust.** `cargo nextest` runs **each test in its own process**. If that holds (verify it -- read nextest's own documentation or demonstrate it), then **process-wide mutable state cannot be the mechanism**, and two of the three hypotheses collapse:

- **Shared fixture state is largely ruled out** for anything held in a Rust `static`. Note for your report that `crates/theseus-discord/src/rpc_client.rs` does hold process-wide mutable statics -- `static REFUSALS: Mutex<Vec<(String, String)>>` at **`:55`** and `static CALLS: Mutex<Vec<String>>` at **`:60`** -- and `crates/theseus-discord/src/courier/v2.rs:40` holds a thread-local `CARD_WINDOW`. Under process-per-test these are per-test and harmless. **Say so explicitly if you confirm it**, because the next person to read this row will have the same suspicion and deserves to have it retired.
- **Shared *filesystem* state is NOT ruled out**, because it crosses process boundaries. Look for any fixed path, any `/tmp` name that is not uniquified per test, any state directory derived from a constant, and any socket bound to a fixed path or port by the gateway rigs.
- **Fixed timeouts that do not scale with concurrency are NOT ruled out**, and on a 4-core VM at `test-threads = 8` they are the most likely mechanism by a distance. Count them: at your base `crates/theseus-discord/src/tests_outbox.rs` has **16** `sleep`/`Duration` sites, `tests_task_threads.rs` **12**, `tests_task_channels.rs` **7**, `tests_gateway/calendar.rs` **7**, `tests_default_forum.rs` **4** (one of the two reds), and `tests_task_home.rs` has a bare `tokio::time::sleep(Duration::from_millis(400))` at **`:276`** (the other red). **A `sleep(400)` that is a stand-in for "by now the thing has happened" is a race, not a wait**, and it is tighter on a loaded 4-core VM than on a 16-core desktop.

**Run the package yourself, repeatedly, under load, and report the numbers you got.** `cargo nextest run -p theseus-discord`, say ten rounds, with the load recipe from the preamble (nice -n 19, four busy loops at nice 0). Report every red with its test name and duration. **A clean ten rounds is a result; report it as "did not reproduce on this VM" and go on with the code reading, which does not depend on reproduction.** Use the substring form of the nextest filter when you narrow to one test, and run `cargo nextest list` first to assert the filter selects what you mean -- an exact filter with a bare function name and no module path matches nothing, runs zero tests and exits 0, which reads as a pass.

**Prefer the finding that explains several of the five over the one that explains one.** The row's whole value is that these are not five separate bugs.

### 2. Recommend the policy -- in writing, in the report, and nowhere else.

The row's three options are: fix the mechanism, give the discord gateway tests their own serialised nextest group, or put the measured population on the flaky list.

**You may build only the first.** The other two are decisions for the maintainer and the owner:

- **Do not add any test to `.config/nextest.toml`'s flaky list.** Its header says a test goes on that list in the commit that files its issue, with the issue's id. More to the point, **a flaky-listed assertion is a weakened assertion and the keel rules apply** -- `scripts/keel-guard.py` fails a range that edits that file's caps, and **you cannot sign, so you cannot ack.**
- **Do not add a serialised nextest group** without saying so first. A `test-group` with a concurrency limit is a real change to how every gate runs, it slows every gate, and it is the maintainer's call. **Write the exact configuration you would add, with the reasoning and the expected cost in gate seconds, and put it in the report as a recommendation.**
- If you fix the mechanism and your fix is provable red-first, **ship the fix** -- that is option one and it needs nobody's permission.

### 3. Say what a reviewer should do until it is fixed.

One sentence in the report, for the maintainer to lift: a single red in theseus-discord on a join gate should be re-run before it is treated as a regression, and the re-run must be recorded. Make it precise enough to paste into a convention document.

## Red-first, for a fix if you ship one

A timing fix is hard to prove red-first and the bar does not drop for it:

- Show the test **failing at your base** under the load recipe, with a red count out of a stated number of repeats, and the assertion's **`left` and `right` values pasted**. A single failing run is not evidence for a probabilistic defect.
- Show the same cell at the same repeat count on your change, and report the count honestly. **If you get one red in forty, say "one in forty", not "fixed".**
- **Plant a revert** of your fix and show the reds return. Restore the file, `touch` it, run `git status`.
- If you cannot make the defect appear at all on this VM, **do not fabricate a red-first story.** Say the cell was clean, ship no production change, and make the mechanism argument from the code. That is the STOP outcome and it is a good one.

## Scope, and what not to touch

- **`crates/theseus-discord/src/golden/dm_flow.txt` is LOCKED** and must come back byte-identical. This row lives in theseus-discord, so this is the one row in the wave most likely to brush against it. Diff it before every commit; if it moves, **STOP**.
- **Do not touch the task-card misplacement defect.** The owner has shipped it forward to the next release deliberately. If your reading leads you to it, note it and step away.
- **`integ/p3z-wave4b` is in flight on the owner's machine and it touches `crates/theseus-discord`** (a Resume button on the self-build halt note). Keep your diff small and say which discord files you touched.
- **No new dependency.** If the right fix is a test-time synchronisation primitive this workspace does not already have, say so in the report instead of adding one.
- You may change test code freely, but **you may not delete a test, add `#[ignore]`, or lower an assertion count** -- all three are keel findings and you cannot ack them. **Rewriting an assertion so it waits for a condition instead of a duration is not a finding and is very likely the right shape of fix.** Prefer "wait until X, with a generous bound" over "sleep N ms".
- Run `python3 scripts/keel-guard.py` scoped to `origin/main..HEAD` before every commit and paste the output **including when it is empty**. A bare run against a stale base prints other people's merged work as findings -- two sessions in the wave before yours nearly reported that noise as their own.

## What will get this row rejected

- Any claim about the build host the measurement came from. You cannot see it.
- Editing `.config/nextest.toml`'s flaky list, or adding a test group without it being a report-only recommendation.
- Deleting a test, adding `#[ignore]`, or lowering an assertion count.
- `dm_flow.txt` moving by one byte.
- A fix that makes one test pass and does not address or even mention the other four.
- "Fixed" claimed on a cell that still reds.
- Red-first evidence without the `left`/`right` values and the repeat count.
