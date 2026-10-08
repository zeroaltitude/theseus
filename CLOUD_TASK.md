<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/ and the benchmark adapters under bench/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, the owner's machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own; bench/ has its README), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

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
- Under load, these timing tests can fail. The flaky list in .config/nextest.toml is empty (since d279767f), so none retries: rerun a failing one alone, and name it in the report. Batch 10's branches, reviewed or in review and joining main one at a time, fix those marked ‡; batch 11's sibling session daemon-flakes (below) works on those marked §.
  - theseus-core: `tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up` can pass nextest's 120 s
    kill (theseus-0u6g‡); `learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy`
    (theseus-1g8j‡); `tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn`'s 20 ms bound
    (theseus-b38m‡); `tests_activation_pace::a_clean_stop_ends_the_warm_builds_waits` (theseus-9o2o‡); and
    `term::tests::a_close_leaves_no_child_behind` (theseus-d006‡, a negative assertion: keep its output);
  - theseus-core's aws/hands `runaway_mode_refuses_at_ten_times_the_hours_line` and
    `ten_times_refuses_and_a_cancel_still_runs` fail when a run crosses an hour boundary (they read the wall clock's
    hour, theseus-5a50): rerun them mid-hour;
  - theseus-kernel's `children::a_sweep_reaps_wrappers_and_orphans_and_never_an_owned_child` (theseus-r4hn‡: a count of
    how processes were classed, so its failure is a finding) and theseus-aws-catalog's
    `one_service_decodes_in_under_5_ms` (theseus-rnl3‡: a wall-clock decode bound in a debug build);
  - theseus-discord: tests_outbox's `a_cards_settle_waits_for_its_create_and_edits_it_by_id` (theseus-0bq1‡, sj0t‡),
    `a_refused_pin_leaves_the_board_unpinned_and_edited` (theseus-o2tm‡), `live_edits_of_one_message_coalesce_into_the_last`
    and `a_shared_channels_card_goes_to_the_dm_and_nothing_mentions_anyone` (theseus-3ae1, pb3l), and tests_gateway's
    `a_jev_notice_goes_to_the_owners_dm_and_a_press_there_labels_it` (theseus-3ae1);
  - theseusd: `bench_profile::a_first_byte_timeout_is_retried_inside_the_headless_turn`, which saw 2 model requests
    with transient retries off (theseus-jtrc§, a count: keep its output), and
    `stops::a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker`, which read an empty SIGTERM
    file (theseus-y0lm§).
- A negative assertion ("nothing of X reached Y", "no process is left", "no retry") that fails even once is a finding, not a flake: keep its output, name it in the report, and don't retry it away.

Timing tests also fail here more often than on the owner's 16-core machine. Any failure outside the lists above is yours to explain.

**What main holds.** You clone main at 8d9a9c9b or later, with **store format 23**. Your clone has v1's milestones, everything batches 8 to 11 joined, and since 2026-10-06 evening:
- the AWS mints (secret-bearing credential mints, the output-shape rule), health-imported (health counts the owner's sessions apart from the 21,779 imported ones), bench-fair and recall-fair (the bench arms comparable), bench-rows;
- cancel-fast (a job's cancel settles on pidfds in 2 frames; the lifecycle bench's cancel budget is now 100 ms with a fewest-frames check), daemon-flakes (a stopping daemon begins no retry or model call: `turn/stopping_step.rs`), daemon-stops (a `--stdio` daemon stops on `shutdown`, a restart in place closes the store, an import or erase stops between frames, the upgrade's directory sync), route-wait (route.v1 asks alone; Jev's connections kept warm);
- imported-walk (the lists skip the import's key range) and sink-fast (the judge's writes kept off the turns);
- the sdgl fix (`Spool::lingering` leaves a marker with no pid; a young marker is never a dead wrapper's) and v18k (a wrapper that will linger marks itself before its report; a batch's next step waits on the pid file);
- the vjn7 fix (redb's cache bounded at 16 MiB in theseus-store's index.rs; every stop phase and the index's close logged at info: `startup::stop_phase`);
- the cockpit's phase 2 (lanes B, A, C, D: the new Ship with the living sea, the watch's six plates over `action.list`'s `unsettled` option, every chart with a table view, the repairs, the daylight mode) and the Ship's flare from the ledger (cockpit/src/ship/flares.ts);
- docs v0.84 (the spec's Items 216 to 243).

**Other changes in flight.** These are being built on the owner's machine now, each on its own branch, and join `main` one at a time while you work. Stay out of their areas unless your task needs it; if it does, keep the edit small and say so in the report.
- fy0i: `theseus_store::blocking` spends the task's cooperative budget after its wait (theseus-store's blocking module);
- mce3 and fts6: an erase's absolute count written with every frame (core import/write.rs); no frame after the stop's last checkpoint (theseusd main.rs, core startup and the store's close; theseusd tests/config_copy.rs);
- sink follow-ups: tests for sink-fast's guarantees, a sweep of `put_many`'s temporaries, the judged turn bench's per-run lines (core judge/sink.rs, blobs.rs; theseus-sim perf);
- the AWS secrets lane: `SecretBearing::WhenPresent`, the STS session mints at posture approve, seven fail-closed rows (theseus-aws-catalog classify.rs and tables; core aws/secret.rs, policy.rs);
- the import's topics into the ontology (core import/, the ontology's records, a CLI command);
- cockpit phase 3 (the Ship's motion and sound; the watch and the pages), the context explorer and the Benchmarks tab (cockpit/src/ views and new additive protocol methods).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (23 on main today, since soul-import), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. No batch-12 task expects one: if yours needs it, say why. Others may bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling:** read scripts/long-files.txt and `wc -l` the files you touch before you add to them; a Rust file the list doesn't name fails past 2,500 lines. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report. Harbor 0.23.0 needs Python 3.12 or later: where python3 is older, run Harbor's tests in a venv (`python3.12 -m venv /tmp/hvenv && /tmp/hvenv/bin/pip install harbor==0.23.0`); bench/async's Harbor tests also need `ASYNC_HARBOR=1` there. Every benchmark run gets a report in docs/benchmarks/: the maintainer writes those, and no task here runs a benchmark.

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is unless your task names it). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261008-daily-ceiling`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package, and bench/'s Python gains no import beyond the standard library and the Harbor its adapter already uses. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, the owner's machine, accounts, or anyone else.
- **Write no person's name anywhere**: not in code, comments, docs, fixtures, goldens, commit messages or your report. Where a person is meant, write "the owner" (the handle `zeroaltitude` where an identity string is needed), "collaborator" for anyone else; the AI assistant may appear only as "Tabitha/Claude", and your commit trailer names only Claude. The names lane cleared the tree at ea34457e: never bring a name back from memory, an old branch or a fixture you copy.
- Don't edit the spec, docs/status.md, the README, docs/benchmarks.md, docs/benchmarks/, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
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

---
## Your task: a daily hard spend ceiling across the daemon (theseus-kp20)

Branch: `cloud/20261008-daily-ceiling`. Every commit's subject carries `theseus-kp20`. Deadline for the report: 4
hours after you start.

**Main has moved since the preamble was written:** the store's format on main is now **24**, not 23. This task should
need no store format change (the day's total is rebuilt from the ledger, not stored); if you find it does, bump
`MANIFEST_FORMAT` (crates/theseus-store/src/store.rs) to 25 with the old layout's sample in theseus-core's
tests_layouts.rs, as AGENTS.md's store version rule says, and say why in the report.

**Why:** since theseus-usei (joined 2026-10-08, merge 5ea85062) a session's spend limit (`[kernel] spend_limit_usd`,
$100 by default) and a profile's loop cap (`max_loops`) only **notify** by default: one notice at the limit and at each
later multiple, and the work goes on. Its review (not on any branch you can fetch, so quoted here) said, as its
finding 2: "Nothing automatic bounds a stuck loop under notify. … with the defaults, a model that keeps calling tools
keeps running and spending until it stops on its own or someone stops it. … `loop.v1`'s judgement cannot act: it is
held at shadow. There is no wall-clock bound on a turn, and a session with no place (the CLI, the cockpit, headless)
gets no post at all." And as its finding 3: spend that lands outside a turn's model call window "a compaction
summary's call (`turn/compaction.rs`, inside the compile), an AWS hands group's settles, a file's transcript, and a
task's carried costs (`Kernel::carry_to_parent`)" is missed by the per-session notice; this ceiling must not miss it.
The review proposed a hard ceiling as the owner's call. The owner answered (2026-10-08): "It's OK as long as it's high
-- like 200 per day". So: one backstop for the whole daemon, by the day, high enough never to touch normal work, low
enough that a loop spinning overnight stops.

**It is a backstop, not a budget.** Leave alone: a session's spend limit and its modes (`spend_limit_mode`,
`max_loops_mode`), a place's ceiling, an MCP client's limit (`[mcp_server] spend_limit_usd`), AWS budgets
(`[aws.accounts.*] monthly_budget_usd`) and the hands group's question, the background day caps (the judge's shadow day
budget and the other day caps you find), and the bench's config (`bench/theseus-bench.toml` keeps its own, lower caps
and needs no line for this). None of them changes meaning; the ceiling sits above all of them.

**What to build:**

1. **The key.** `[kernel] daily_spend_ceiling_usd`, a dollar amount, **default 200.0**, so the owner's config (which
   holds only diffs) needs no line. Validated above zero and finite, with the same words as `spend_limit_usd`'s check
   (crates/theseus-core/src/config.rs, `validate`, about line 1505). Show it in `theseusd config`. Add it to the example
   template, crates/theseus-core/config/theseus.example.toml, beside `spend_limit_mode` (about line 306), with a
   comment that says what it is and that it is a backstop, and its template tests (`theseusd example-config` prints that
   file; AGENTS.md says how a new key goes in). config.rs is at 2,914 of its 2,915-line ceiling: put the default, the
   check and any helper in crates/theseus-core/src/config/limits.rs (that module holds this family's types since
   theseus-usei), and add only the field line(s) to config.rs, or move something of this family out of config.rs to make
   room. Check scripts/long-files.txt before adding to any file.

2. **The day.** The owner's local day, in the zone the daemon already uses: `crate::wake::local` (the system's zone, as
   `localtime_r` reads it) and `crate::judge::spend::local_day` (`2026-10-08`), which the judge's day budget already
   keys on, and `crate::learning::local_midnight`. Use those; don't add a time zone key. The day turns at local midnight;
   test a call either side of it (the kernel's clock and the judge's ladder clock show how tests drive time).

3. **What counts: every model spend the ledger records, daemon-wide.** Turns' provider calls (the kernel's action
   reservations, settled at their real cost), tasks (a task's spend, once: a task's cost is carried to its parent's
   budget, so don't count it twice), the judge (its calls under its own day budget, crates/theseus-core/src/judge/spend.rs
   and `reserve_between` in judge/mod.rs), consolidation (crates/theseus-core/src/consolidate/run.rs), compaction
   (crates/theseus-core/src/turn/compaction.rs, compiler/compaction.rs, fact/compaction.rs), synthesis and learning
   runs, voice
   (`Kernel::book_spend`, crates/theseus-kernel/src/spend.rs), and any other path that writes a `cost_usd` (git grep
   `cost_usd` under crates/theseus-core/src and crates/theseus-kernel/src and list each source in the report, counted or
   not and why; AWS compute is not model spend and does not count). Model spend only, in micros, as the ledger holds it.
   - **One counter, in memory, daemon-wide:** today's day and today's total (an atomic or a small lock, nothing on the
     hot path that reads the store). Each place that books a real cost adds it; a reservation holds its amount until
     settled, as the kernel's own budget does, so two calls racing at the edge cannot both pass on the same headroom.
   - **Across a restart:** at the daemon's start, read today's total from the ledger once (the rows of today's local day
     that carry a cost) and seed the counter, so a restart never resets the day. Keep it FAST: one bounded read at start,
     off the serving path if it can be (the ledger's index is built after the start, as budgets.rs's `UNREAD` says;
     decide what a call made before that read finishes does, and test it: my recommendation is that the read happens
     before serving, since it is one day's rows, but measure its time on a store with a busy day and say).
   - **The day turns:** the first booking or check of a new local day starts the total at zero (keep the reservations
     still open from yesterday counted in the day they are settled, or in the day they were made: choose, say which and
     why, and test it).

4. **At the ceiling, a model call's reservation is refused.** Where each source reserves or asks before it calls (the
   kernel's `plan_action` reservation path in crates/theseus-kernel/src/kernel.rs about line 1560, the judge's
   `reserve`, the voice's pre-call question in `theseus_core::voice`, consolidation's and compaction's call sites), a
   call whose reservation would take today's total past the ceiling is not made. It is refused **before** the provider
   is called, with nothing half-written (as an `OverBudget` refusal writes nothing today). A real cost that lands after
   its call (a late completion, a speech call's booking) is still booked in full: a real cost is never hidden, even past
   the ceiling (the kernel's rule; see spend.rs's doc).
   - **The words reach the session's place and the model:** e.g. "today's spend reached the $200 daily ceiling; the
     owner can raise `[kernel] daily_spend_ceiling_usd` or wait until midnight" (with the configured amount, and the
     local time the day turns). A turn ends with that reason, as the spend-limit stops in turn.rs (about line 2445) end
     a turn, so the session's place shows it and the model's next context says why its call did not run. A task fails
     with it; a background job (judge, consolidation, compaction, learning) skips its call, logs once, and says so where
     it already reports (the judge's health, the consolidation run's row).
   - **Kernel line ceiling:** crates/theseus-kernel/src/kernel.rs is at 3,030 of 3,030. Put the ceiling's logic in a
     module of its own (e.g. crates/theseus-kernel/src/day_ceiling.rs, or under theseus-core if the counter lives
     there) and add only the call to kernel.rs, or move lines out (stops.rs and reopen.rs show how earlier work did).
     turn.rs is at 3,423 of 3,525.
   - **Not a mode:** there is no "notify" for this ceiling; it stops. The owner raises the key to go on (a config reload
     or restart, whichever the daemon's config path already supports: say which).

5. **One notice a day to the owner's DM.** When the ceiling is first reached on a day, one post to the owner's direct
   message place (the judge's notices and approval cards already find it: crates/theseus-core/src/judge/notice.rs,
   approval.rs, places.rs), never a shared place, saying what stopped, today's total, the ceiling, the key, and when the
   day turns. Exactly one per local day, across a restart (a ledger row of its own, e.g. an additive `spend.ceiling`
   kind beside the budget kinds in crates/theseus-protocol/src/ledger.rs, read at start with today's total, so a restart
   on a day already stopped does not post again). With no owner DM bound (a headless daemon, the bench), no post: the
   ledger row, `theseus budgets` and the cockpit say it.

6. **Shown in `theseus budgets` and the cockpit's budgets.** `budget.list` (crates/theseus-core/src/rpc/budgets.rs) gains
   an additive optional block beside `judge` (the `JudgeDayBudget` one is the model): the day, the ceiling, today's
   spend, whether it is reached and since when. Types in crates/theseus-protocol/src/budgets.rs (lib.rs is near its
   ceiling: add nothing there but what a new method would need, and this needs none); regenerate
   cockpit/src/protocol.gen as the gate does. The CLI's `theseus budgets` (crates/theseus/src/budgets.rs) prints one
   line for it (render.rs is at 3,099 of 3,100: put the line in budgets.rs). The cockpit's Budgets
   (cockpit/src/components/Budgets.tsx, cockpit/src/lib/budgets.ts) shows today against the ceiling (a quiet bar or
   pill that turns to a clear stop state when reached), with its node test in cockpit/test/. `health` may carry it too if
   it is one field; say if you add it.

**Tests and planted reverts, for each:**
- the key's default (200.0 with no line), its check (0, negative, NaN refused with the words), `theseusd config` shows
  it, the template carries it and its template test passes;
- the counter: each source adds its real cost once (a task's cost not twice through its parent), a reservation holds
  headroom, two reservations racing at the edge (one passes, one is refused), a late real cost is booked in full past
  the ceiling;
- the refusal: a turn's call at the ceiling is not made (the fake provider is never called), the turn ends with the
  words, its place gets them, the next context carries the reason; a task fails with it; the judge, consolidation and
  compaction skip their call;
- the day: a call before and after local midnight (yesterday's total does not count today);
- a restart: today's total is read from the ledger and the ceiling still holds; a restart on a day already stopped
  posts no second notice;
- the notice: one per day to the owner's DM, none to a shared place, none with no DM bound;
- `budget.list`, `theseus budgets` and the cockpit's Budgets show the day's block; the cockpit test for its stop state;
- the defaults leave everything else as it was: a session's spend limit still notifies, the bench's `ask`/`end` caps
  still stop first (crates/theseusd/tests/bench_profile.rs), an MCP client's limit and the background day caps unchanged.
For each, plant the bug, show the test fail, restore and `touch` (the preamble's recipe).

**FAST:** the reservation path must be no slower. Measure a provider call's reservation (`plan_action` with a
reservation) before and after, in a timed test or a micro-bench on the same machine in one run, A then B then A, and
give the numbers (p50 and p95 over many calls); and the turn bench's frames for a turn before and after (the counter
adds no store write and no store read to a call). Measure the start's ledger read on a store with a busy day (say 5,000
cost rows today) and give its time. If any of these is slower, say by how much and why it cannot be avoided.

**Read first:** the root AGENTS.md, crates/theseus-core/AGENTS.md, crates/theseus-kernel's AGENTS.md if it has one,
cockpit/AGENTS.md and cockpit/CLAUDE.md; then crates/theseus-core/src/config/limits.rs, config.rs's `KernelConfig`
section (`spend_limit_usd`, `to_kernel_config`, `validate`), the budgets-notify merge (`git show 5ea85062`), the
kernel's reservation path (`plan_action`, `OverBudget`, `overdraws_for`, earlier.rs's `book_reservation_in`,
`settle_reservation_in`), crates/theseus-kernel/src/spend.rs, crates/theseus-core/src/judge/spend.rs (its day budget is
the model for a day's total kept in memory and read at start), crates/theseus-core/src/rpc/budgets.rs,
crates/theseus-protocol/src/budgets.rs, crates/theseus/src/budgets.rs, and the cockpit's Budgets.

**Not in this task:** a per-place or per-session daily ceiling; changing any existing limit, mode or cap; an AWS
budget; a wall-clock bound on a loop; changing the bench's config.

The issue: `theseus-kp20`.
