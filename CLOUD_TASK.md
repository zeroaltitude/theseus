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
# Your task: a pending correction suppresses the route wait, so a held compile failure races the verdict (`theseus-ler3c`)

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


**Branch: `cloud/20261027-p3y-hold-compile-race`. Model `claude-sonnet-5-5`. Deadline: 6 hours from your start, by the clock, waits included.**

## Step 0, before anything else: re-verify every citation in this prompt

**Do this first and report the result.** Every `file:line` below was checked against `origin/main` = `f3fdd4eb` and re-checked against `c36c437e` on 2026-10-27 at about 13:5x MST. Open each in your own clone and say in your report what it actually was. **Name every citation this prompt got wrong**, even the ones that were right.

**One citation in the row's own notes is already known to be wrong and is corrected below**, as an example of what step 0 is for:

- The row quotes the mechanism as `route_step.rs:567-572`. At your base that block is at **`crates/theseus-core/src/turn/route_step.rs:570-575`** -- off by three. The three lines the row quotes are at `:570`, `:574` and `:575`. **The substance is exactly right**; only the numbers moved.
- `turn.rs:1909` is **exact**.

## The defect

This is a **pre-existing, reproducible, measured flake**, and that combination is rare enough to be worth a session on its own. It is independent of any patch3 row: the row it was first blamed on was exonerated by measurement.

**Measured red rate: 22.5% under CPU load, on an unmodified tree.** Two trees, 40 repeats per cell: 31/160 with the suspected row against 36/160 on the control -- no difference, and the control is marginally redder.

**The mechanism, read in the code rather than inferred.** At `crates/theseus-core/src/turn/route_step.rs`, verified at your base:

```
568:        let unreachable = live && self.judge.jev_unreachable();
569:        // A steered turn runs where the owner said: it waits for no verdict.
570:        let steering = t.route.steer.take().filter(|_| live);
571:        t.route.defer_persist = live;
572:        let max_wait = Duration::from_millis(self.cfg.routing.max_wait_ms);
573:        let compile = Box::pin(self.compile_step(t, session, spec, force, strip, overflow, i));
574:        let wait = (live && !unreachable && steering.is_none()) || self.judge.pins_wait();
575:        let (compiled, w) = beside(compile, &mut rx, max_wait, wait).await;
```

`tests_hold_compile::a_held_compile_failure_records_its_steer` is the **one test in that file that sets a pending correction**. At `crates/theseus-core/src/tests_hold_compile.rs:216-223` it calls `r.core.runner.judge.corrections.set_pending(...)`. That makes `steering` **`Some`** at `:570`, which makes `wait` **false** at `:574`, so the turn does **not** wait `routing.max_wait_ms` (default 200 ms) for Jev's verdict: it takes whatever has landed when the compile finishes.

Then at `crates/theseus-core/src/turn.rs:1909`:

```
1909:                Err(f) if !t.route.held => return Ok(Next::Fail(f)),
```

the compile's `context_overage` escapes unless `t.route.held` is **already** true. Verdict in -> HELD, the test passes. Verdict not in -> `context_overage`, the test fails. **The discriminator is visible in the ledger: `route.decided` with `mode="chat"` is a pass, `mode=null` is a fail.**

**The signature that confirms it, and you should reproduce this before you fix anything.** The sibling test `a_held_compile_failure_records_its_route` at **`tests_hold_compile.rs:155-162`** uses the **same** `reach(50_000)` and the **same** window but sets **no** correction -- so it waits up to 200 ms and does not flake. **One test flakes, its twin does not, and the only difference is the correction.** That is the whole argument, and it is checkable in twelve lines of diff between the two tests.

**Why it looks like a size problem and is not.** The failure message is `context_overage` with a token estimate, which invites a margin reading. It is not a margin:

- The request is **byte-identical** with and without the suspected row at all eight reach sizes measured (estimate 68,386 / upper 95,741 at `reach(50_000)` on both).
- The pass/fail is **non-monotonic in the reach**: N=37000 PASS, N=38000 FAIL, N=39000 PASS; and at a **fixed** N=31093 it went PASS/PASS/FAIL/PASS/PASS over five runs with identical estimate, upper and over every time.
- A bigger reach makes the **compile take longer** (0.070 s at 30k, 0.151 s at 100k), which gives the verdict more wall-clock to land. **`reach(50_000)` is buying TIME, not headroom.**

**So raising the reach would be the wrong fix.** At `reach(50_000)` the request is already 63% over the limit on the estimate and 128% over on the upper bound. The overage is **deliberate** -- the compile failure is the *intended* path of this test. Changing the number only perturbs the timing axis by accident, and leaves a magic number nobody will understand in six months.

**Second sighting, same mechanism:** this test tripped another lane weeks ago (recorded in a maintainer-side lane report that is not in this repository, so you cannot read it; it is mentioned so you know this is not a single sighting), also on `context_overage`, also green when rerun alone.

## What closes this row

The remedies, in the row's preference order. **Remedy 1 is the one to build if it works.**

### Remedy 1 (preferred): make the verdict's arrival deterministic rather than raced

The test is about *"the steer reaches the row though the compile failed"*. It is **not** about verdict latency. So the right fix removes the race from the test, not the wait from the production path.

Two shapes, both with precedent in this very file:

- **Have the FakeJev verdict in hand before `run(...)` is entered.** The rig is `theseus_judge::fake::FakeJev` with `Scripted as Jev` (imported at `tests_hold_compile.rs:6`), built by `rigged(shared)` at `:33-35` and `rigged_with` at `:37-40`, which calls `FakeJev::start()` at `:41`, `mode(&jev, "chat", 0.95)` at `:42` and `jev.script(...)` from `:43`. **`FakeJev` has `hold_when` (`crates/theseus-judge/src/fake.rs:246`) and `release` (`:251`), and `set_mode` with `FakeMode::Held`.** The neighbouring test at `tests_hold_compile.rs:186-207` already uses `jev.set_mode(theseus_judge::fake::FakeMode::Held)` at `:191` and `jev.release()` at `:206` to control verdict timing deliberately. **Note that `a_held_compile_failure_records_its_steer` binds the rig as `_jev` at `:215` and never touches it** -- it takes whatever the default scripting does, on whatever thread wins. Read `crates/theseus-judge/src/fake.rs` and work out whether a `hold_when`/`release` pair, or simply ordering the verdict before `run`, makes the arrival deterministic.
- **Or give the rig a knob that pre-seeds `t.route.held`.**

### Remedy 2 (if remedy 1 does not work): force the wait back on

**The row flagged this as unverified. I verified it, and it is available.** `routing.max_wait_ms` alone is **not** enough, because `steering` suppresses the wait at `:574` regardless of the configured bound. The lever is the `|| self.judge.pins_wait()` arm of that same line, and:

- `pins_wait()` is defined at **`crates/theseus-core/src/judge/mod.rs:839-841`**, reading an `AtomicBool` field declared at `:220`;
- its setter is **`pub(crate) fn pins_wait_for_verdicts(&self)` at `crates/theseus-core/src/judge/mod.rs:845-847`**;
- and **there is already a caller in a test in this same crate**: `crates/theseus-core/src/tests_route_effort.rs:294` does `r.core.runner.judge.pins_wait_for_verdicts();`.

So a test in `theseus-core` **can** set it, with precedent. The doc comment at `:838` even says so: *"anyway: never, but in a test that has called `pins_wait_for_verdicts`."* **Check this yourself at your base rather than trusting me**, but this is a verified lever, not a hope.

**Prefer remedy 1 over remedy 2 if both work**, because remedy 2 makes the test depend on a test-only pin rather than removing the nondeterminism, and the next reader has to understand the pin to understand the test.

### Remedy 3 (only if neither 1 nor 2 works)

Drop `reach(50_000)` to something honest like `reach(20_000)` -- the overage is still about 2,903 tokens and the scenario is unchanged -- **and fix the flake by 1 or 2 anyway**. **Do NOT raise the reach to buy milliseconds.**

## Red-first, which is awkward here and must be done properly

**A flake is not red-first evidence.** "I ran it and it failed" proves nothing when it fails 22.5% of the time. For this row the red-first requirement is specific:

1. **At your base, demonstrate the race itself, not just a failure.** Run `a_held_compile_failure_records_its_steer` repeatedly under the load recipe (nice -n 19 with four busy loops) -- at least 40 repeats -- and report the red count. Then run its twin `a_held_compile_failure_records_its_route` the same number of times under the same load and report **its** red count. **The twin should be clean.** That contrast is the red-first evidence for this row, and it is stronger than any single failing run.
2. **Capture the discriminator.** On a failing run, show the `route.decided` row with `mode` null; on a passing run, show it with `mode="chat"`. The test already reads these rows through its `rows(&r.core, "route.decided")` helper. That is what turns "it flaked" into "this is the mechanism".
3. **After your change, run the same 40-repeat cell under the same load and report zero reds.** If you get even one, your fix did not remove the race, it narrowed it: say so and do not claim the row.
4. **Plant a revert of your fix and show the flake return.** For a timing fix this means re-running the 40-repeat cell with the fix reverted and showing the reds come back. **Paste the `left`/`right` values from a failing assertion.**

Use the substring form of the nextest filter. **`test(=a_held_compile_failure_records_its_steer)` with a bare function name and no module path matches nothing, runs zero tests and exits 0, which reads as a pass.** Run `cargo nextest list` first and assert the filter selects exactly the test you mean, then paste the list output in your report.

## Scope, and what not to touch

- **Do not change `routing.max_wait_ms`'s default.**
- **Do not change the production behaviour at `route_step.rs:570-575` unless you can argue it is wrong.** A steered turn deliberately does not wait for a verdict -- that is a documented design decision with a comment on it at `:569`. This row is about a **test** that collides with that decision, not about the decision. **If your reading says the production line is itself wrong -- that a steered turn ought still to wait when a hold is possible -- that is a finding worth a section of its own, but write it as a recommendation and do not ship it as this row.**
- `turn.rs:1909` is likewise production and likewise out of scope for a change.
- You are in `theseus-core`. **`crates/theseus-core/src/turn.rs` has thirteen lines of headroom at your base** (3,512 of 3,525); you should not need any of them.

## A well-argued STOP

If your 40-repeat cell at the base comes back **clean on a 4-core x86 VM**, that is a real result and not a failure. The measurement behind this row was taken on a different host. Report the clean cell, report the twin's cell, and say plainly that the race did not reproduce here -- then **still read the mechanism and say whether the code supports it.** The code argument at `:570`/`:574`/`:1909` stands or falls on reading, independently of whether this VM is fast enough to lose the race. A report that says "did not reproduce here, and here is why the mechanism is nonetheless real / is nonetheless wrong" is the best possible outcome if the cell is clean.

## What will get this row rejected

- Raising `reach(50_000)` to buy milliseconds.
- A single run offered as red-first evidence for a 22.5% flake.
- A nextest filter that selected zero tests and exited 0.
- Changing the production route-wait decision without a separate argument for it.
- A fix that leaves one red in forty and is reported as a fix.
- Red-first evidence without the `left`/`right` values.
