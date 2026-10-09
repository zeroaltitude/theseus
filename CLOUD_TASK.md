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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261008-seen-shared`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: one "seen" per machine, shared by the TUI and the CLI (theseus-yus0)

Branch: `cloud/20261008-seen-shared`. Every commit's subject carries `theseus-yus0`. Deadline for the report: 2 hours
after you start.

**Main has moved since the preamble was written.** Main is at `2eed643c` or later, and it moves tonight as other
joins land. The preamble's "What main holds" and "Other changes in flight" are out of date: those changes have joined,
and so have batch 12's (budgets that notify, route feedback, route.v3, recall fallback, memory.lookup, the daemon's
memory bound, a routed bench arm, session states). The store's format on main is **25**, not 23: if your change adds a
field or a variant to a stored record, bump `MANIFEST_FORMAT` (crates/theseus-store/src/store.rs) to 26 with the old
layout's sample in theseus-core's tests_layouts.rs, as AGENTS.md's store version rule says, and say why in the report
(others may bump it too; the maintainer renumbers at the merge). Line ceilings in scripts/long-files.txt, as of
2eed643c: crates/theseus-core/src/config.rs (2,927 of 2,927), crates/theseus/src/render.rs (3,128 of 3,128) and
crates/theseus-kernel/src/kernel.rs (3,030 of 3,030) are full: add no net line to them (new code in a module of its
own, only the `mod` line or the call in the long file, saving a line elsewhere in it if you must).
crates/theseus-protocol/src/lib.rs is at 2,715 of 2,719, and a branch in review (spawn-ask-follow) adds five lines
there: add at most your new modules' `pub mod` lines and re-export nothing from it. turn.rs is at 3,418 of 3,525, the
Discord binding's runtime.rs at 3,409 of 3,500 and its render.rs at 3,006 of 3,009. A Rust file the list doesn't name
fails past 2,500 lines (crates/theseus-core/src/toolrun.rs is at 2,431).

**In review, and may join before you** (each written on `b6dab4a6`): daily-ceiling (a daemon-wide daily spend
ceiling), terminals (a terminal's background processes outlive the session), spawn-ask-follow (`theseus --spawn ask`
follows late results and wakes), fs-read-big (`fs.read` reads a window of a big file) and web-search-unkeyed
(`web.search` offered only when keyed). Between them they touch config.rs, render.rs, crates/theseus-core/src/rpc/
methods.rs, the protocol's lib.rs, health and the CLI's cmd.rs.

**Launched with this one: task management, wave A.** The owner wants task management to be a Theseus killer feature:
"you can have lots of tasks going and you'll always be kept in the loop on status, and when your feedback is needed",
in the theseus CLI and TUI, herdr and Discord. Five investigations (read-only on main 2eed643c, with live runs on
scratch daemons and stand-in models) planned it. Their reports are not in this repository, so what each row needs from
them is quoted in its task. Six rows build at once, each on its own branch, joining main in any order: stay out of the
others' areas, keep any edit to a shared file small and surgical, and name it in your report (the joiner resolves).
- push-fixes (theseus-q5af): the push's warts in crates/theseus-core/src/push.rs, `confirm.requested`'s expiry
  (toolrun.rs), `confirm.list` (rpc/confirms.rs), two stale comments in the kernel, and `session.open` in one frame
  (rpc/methods.rs and the kernel's open);
- work-types (theseus-753z): crates/theseus-protocol/src/work.rs and notify.rs (the work view and the notification
  policy), adopted by the TUI's notice.rs and the CLI's herdr.rs reporter;
- client-names (theseus-0n1v): a task named by its title (the TUI's board.rs and app.rs, `theseus sessions`, herdr.rs's
  title and agent name), the TUI's input line's profile, one id resolver in cmd.rs, local times in render/;
- discord-silent (theseus-l1y1): silent Discord writes (crates/theseus-discord's courier.rs, a new policy.rs,
  tests_outbox.rs, theseus-sim's fake Discord);
- cli-status (theseus-lweh): `theseus status` and `theseus wait --any` (the CLI's main.rs, a new status.rs, the
  client's connect timeout, a theseus-sim bench row);
- seen-shared (theseus-yus0): one seen file per machine (the TUI's seen.rs moved into `theseus_client`; `history`,
  `watch` and `confirm` record what they showed).
A seventh row reviews spawn-ask-follow.

**For every row:** write no person's name anywhere (the owner is "the owner"); give every new config key a default,
and put it in `theseusd example-config`'s template with its template tests; follow the store's format rule above; run
the gate before every commit; commit CLOUD_REPORT.md as the branch's last commit.

**Why:** what the owner has seen stays the client's, on this machine (design `stage2` §2.9), but today only the TUI
keeps it, in a file of its own. On the CLI investigation's tour the TUI's title read `theseus (9)`, counting as "done
until seen" sessions the owner had already read in another terminal with the CLI. Every client on the machine should
share one record of what was shown (the owner's decision D7), so that a session read in the CLI is not new in the TUI.

**What to build** (the CLI investigation's §4.9, its row C2):

1. **One seen file per machine:** `$XDG_STATE_HOME/theseus/seen.json` (by default `~/.local/state/theseus/seen.json`),
   today's `tui-seen.json` renamed, with a reader for the old file: when the new one is absent, read `tui-seen.json`
   (crates/theseus-tui/src/seen.rs, format version 1: per execution a `Mark {displayed, finished, position, level,
   turns}`) and write the new name from then on.
2. **Move seen.rs into `theseus_client`** (the `theseus` package's library, crates/theseus/src/lib.rs, which the TUI
   already uses as `theseus_client::…`), with the TUI using it from there. A move keeps the bytes: the TUI's tests and
   goldens pass unchanged before anything else changes.
3. **`history`, `watch` and `confirm` record what they showed** (crates/theseus/src/cmd.rs): a session's history
   printed, a session watched, and a question listed or answered mark it displayed at the position shown. Give one call
   for it (`theseus status`, this wave's cli-status row, will record through the same call at its join). Record after
   the output, never before it, and never fail a command because the file can't be written (one line on stderr at
   most).
4. **Two writers merge by the greatest position:** a write reads the file, takes per execution the greatest of each
   position it holds, and writes the result; **writes are atomic** (a temporary file in the same directory, then a
   rename), so a reader never sees half a file, and two clients writing at once lose nothing that matters.

**Tests and planted reverts:** a session read by `history` is not ◆ (done until seen) in the TUI; two writers merge by
the greatest position (interleaved, in one test); the write is atomic (a reader during a write sees the old file or the
new one whole; a failed write leaves the old one); the old file is read when the new one is absent; a failure to write
doesn't fail the command. Plant each bug, show the test fail, restore and `touch` (the preamble's recipe).

**Proof (live, a scratch daemon of your build, stand-ins only):** several sessions finish; the TUI (a capture on `tmux
-L scratch`) counts them as done until seen; read some with `theseus history <id>`, and the TUI's count drops by as
many (on main it doesn't). Give the commands.

**FAST:** no new startup work: only the commands that show sessions read the file, and each writes it once, after its
output. Time `theseus history` and `theseus confirm` (the median and p90 of 30 runs) before and after, on the same
scratch daemon.

**Shared files in this wave:** client-names edits cmd.rs (one id resolver for `history`, `watch`, `confirm` and
others) and the TUI's app.rs and board.rs: keep your change to one call per command. cli-status reads the seen file for
its ◆ count through `theseus_client::seen` (or a stub until you join).

**Read first:** the root AGENTS.md, and the AGENTS.md of crates/theseus and crates/theseus-tui if present; then the
TUI's seen.rs, app.rs and main.rs (where the file is opened), the CLI's lib.rs, and cmd.rs's `history`, `watch` and
`confirm`.

**Not in this task:** the title's count rule (needs you only: a later row); notices; `theseus status` itself.

The issue: `theseus-yus0`.
