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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261008-terminals`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: terminals for real work: background processes outlive the session as `proc.run`'s do, a read can wait for the typed command to finish, and an ended terminal frees its slot (theseus-ggqf)

Branch: `cloud/20261008-terminals`. Every commit's subject carries `theseus-ggqf`. Deadline for the report: 4 hours after
you start.

**Main has moved since the preamble was written:** main is at `b6dab4a6` or later, and the store's format on main is
now **24**, not 23. If your change adds a field or a variant to a stored record, bump `MANIFEST_FORMAT`
(crates/theseus-store/src/store.rs) to 25 with the old layout's sample in theseus-core's tests_layouts.rs, as AGENTS.md's
store version rule says, and say why in the report. Two long files sit at their ceilings in scripts/long-files.txt:
crates/theseus-core/src/config.rs is at 2,914 of 2,915 and crates/theseus/src/render.rs at 3,099 of 3,100. Add no net
line to either: put new types and logic in a module of their own (config/ and render/ already hold many) and add only
the line that names them, saving a line elsewhere in the file if you must. Another session in flight (a daily spend
ceiling) also adds to config.rs.

**Launched with this one** (each on its own branch, joining main in any order; stay out of their areas):
- terminals: `term.*`'s background processes at the session's end, `term.read`'s `until_idle`, an ended terminal's
  slot (core term/, a `[tools.term]` config key);
- spawn-ask-follow: `theseus --spawn ask` follows late results and near wakes (crates/theseus's cmd.rs and a new
  module, wake.rs's result words, the spawned daemon's flag);
- fs-read-big: `fs.read` reads a window of a file over its size cap (theseus-tools fs.rs);
- web-search-unkeyed: `web.search` offered only when its key is configured (core web/, health, the tool listing).

**Why:** the overnight Terminal-Bench run (2026-10-07/08) ran every Theseus command through `term.*`, because
`proc.run` could not spawn there (theseus-f7tz, since fixed). That exposed three defects in the terminals themselves,
each of which a real user hits whenever they drive a shell, a REPL or a server through a terminal:

1. **A service started in a terminal dies silently when the session ends or the daemon stops.** `Pty::close`
   (crates/theseus-core/src/term/pty.rs, about line 196) sends a hang-up and SIGTERM to the program's process group
   *and to every descendant, including ones that left the group*, then SIGKILLs what lingers. It runs at `term.close`,
   at the session's end (`close_session_recorded`, called from crates/theseus-core/src/turn.rs about line 1335 when
   the execution's state is terminal), at a cancel or `/stop`, and at the daemon's stop (`Core::close_terminals`,
   term/mod.rs about line 400). In two trials the model started a Flask service with `nohup python3 app.py … &` in a
   terminal and tested it (200 and 400). It told the user "the service is running in the background", and the daemon's
   stop then SIGTERMed it (`reaped an orphan a job left pid=124 status=signal: 15 (SIGTERM)`). In a third, nginx and
   websockify went the same way. `proc.run` does the opposite: its wrapper is a child subreaper that "lingers until no
   descendant remains … It kills nothing" (crates/theseus-kernel/src/job.rs, `run_wrapper_process`, about line 199),
   so the same `nohup … &` through `proc.run` stays up. The two ways of running a program disagree, and the terminal's
   way makes Theseus's own report false.
2. **There is no way to wait for the command you typed to finish.** `term.read` waits for `quiet_ms` (the screen
   unchanged), an `until` text, or the program's end, for at most 60 s (term/tools.rs, `wait`, about line 323). A
   shell finishing a command is none of these, so the model guesses a prompt string or polls. One trial made 47 polling
   reads (most with `until: "zzzz"`, a string chosen never to appear, used as a sleep) across 23 model calls at 85k to
   110k context, $0.50 of its spend. Another waited the full 60 s for `$ ` under a root shell whose prompt ends `# `.
3. **An ended terminal keeps its slot.** A session holds at most `PER_SESSION` (4) terminals (term/mod.rs) whether
   their programs run or not. That caused 5 refusals in two trials ("this session has 4 terminals open, the most it
   may: close one first").

**What to build:**

1. **Background processes outlive the session's end and the daemon's stop.** At the session's end
   (`BY_SESSION_END`) and at the daemon's stop (`BY_DAEMON`), the terminal's program and its **foreground** process
   group get the hang-up and the kill as today. Processes that are neither are left running. That is everything else
   in the tree at the close: `&` jobs (interactive bash gives each its own group), `nohup`, `setsid` children,
   daemonized servers such as nginx's master, and the pty's other holders that are not in the foreground group.
   - "Foreground" is read at the close: `tcgetpgrp` on the master, and the program's own group. Decide and document
     what a background job that still holds the pty's slave open gets once the master closes: the kernel hangs up the
     session's foreground group only, but `holders()` lists slave holders. My recommendation is to leave it, since a
     `nohup` job ignores SIGHUP and its writes fail with EIO harmlessly. Say what you chose.
   - What is left running is handed to the bookkeeping `proc.run`'s orphans get: the daemon adopts and reaps them
     (theseusd main.rs `reap_children`; theseus-kernel's `children`), health lists them, and a ledger row (an additive
     fact beside `term.closed`) and the close's line name each one (pid, program, why it was left). A `/stop` or a
     cancel of the session's execution still ends them, as it ends `proc.run`'s lingering jobs. Find and reuse that
     path; don't build a second one.
   - `term.close` (the model's explicit act), a cancel and `/stop` keep killing everything, as now.
   - The words: `term.open`'s description gains one sentence ("Programs it starts in the background keep running
     after the session ends, as with proc_run; term_close stops everything it started."). `term.close`'s description
     says that it stops background jobs too. Keep both short; the model reads them on every call.
   - A config key, `[tools.term] keep_background`, **default `true`**: background processes a terminal started
     survive the session's end and the daemon's stop, as `proc.run`'s already do (one rule for both). `false`
     restores today's kill-everything. The owner may flip the default at review, so keep it one constant, and
     your report must state the default plainly in its first lines. There is no `[tools.term]` table on main
     yet: add it (its type in a config/ module of its own), and add it to `theseusd example-config`'s template
     with its template tests, as AGENTS.md says.
2. **`term.read` with `until_idle: true`.** Return when the terminal's program is back in the foreground of its pty
   (`tcgetpgrp(master) == the program's pgid`): a shell at its prompt, or a REPL waiting for input. Return at once if
   it already is. Check it often enough to answer within ~50 ms of the change; it is cheap. It is bounded by
   `timeout_ms`, and for `until_idle` alone the bound may go up to the config's `proc_sync_secs` (default 60, so no
   change by default; an operator who lets a command block for 15 minutes lets a terminal wait as long). The read says
   which ended the wait (idle, timeout, the program's end). It combines with `until` and `quiet_ms` as the existing
   options combine (whichever comes first). `term.read`'s description names it as the way to wait for a typed command.
   Add `until_idle` to `term.send` too if it fits cleanly (send, then wait idle, in one call); say if you did.
3. **An ended terminal frees its slot.** A terminal whose program has exited no longer counts toward `PER_SESSION`.
   Its last screen stays readable (`term.read`) until `term.close`, or until an open finds the session full of running
   terminals plus ended ones and reclaims the oldest ended one. The open's result names the terminal it reclaimed.
   Health's terminal lines show ended ones as such.

**Tests and planted reverts, for each:**
- F1's shapes, each at the session's end *and* at the daemon's stop: `nohup sleep 300 &` survives; `sleep 300 &`
  (no nohup) survives; `setsid sleep 300` survives; a foreground `sleep 300` dies; a daemonizing server (a small
  double-forking script) survives; a TERM-ignoring foreground program is still SIGKILLed after the grace. `term.close`,
  a cancel and `/stop` still kill all of them. The survivors are reaped by the daemon when they exit (no zombies), and
  listed in health while they run. `keep_background = false` restores today's behavior. Mind theseus-z0kk (a child
  forked between the last scan and the kill; it is still open, and docs/spec/29-part3-item-225.md, "What the review
  found", describes it): show no foreground child is ever spared by mistake.
- A headless turn through the real binaries (theseusd's `headless` test is the model). The model's terminal starts a
  listener with `nohup … &`, the turn ends, the daemon stops, and the listener still accepts a connection.
- `until_idle`: returns within 100 ms of `sleep 1` finishing in bash; returns at once at an idle prompt; reads busy
  while `less` (its own group) or `sleep 5 | cat` runs; reads idle when `python3` waits at `>>>`; times out cleanly.
- Slots: four ended `bash -c true` terminals don't block a fifth open; reclaiming names the reclaimed terminal; a
  read of an ended terminal returns its last screen.
For each, plant the bug, show the test fail, restore and `touch` (the preamble's recipe).

**FAST:** the close path gets no slower for a terminal with nothing in the background. Measure `term.close` and the
daemon's stop with 4 open terminals, before and after. `until_idle` must not busy-loop: measure the daemon's CPU
during a 30 s idle wait, before (a `quiet_ms` wait) and after.

**Read first:** the root AGENTS.md and crates/theseus-core/AGENTS.md (its `term` paragraph), then
crates/theseus-core/src/term/ (mod.rs's module doc for the lifetime rules, pty.rs's `close` and `holders`, tools.rs's
`wait`), theseus-kernel's job.rs (`run_wrapper_process`, `linger`) and tree.rs, theseusd's main.rs `reap_children`,
and theseus-z0kk (above). Check scripts/long-files.txt before adding to any file.

**Not in this task:** changing `proc.run`; a terminal that survives the daemon (the pty itself still closes); keeping
a background process alive across a `/stop`.

The issue: `theseus-ggqf`.
