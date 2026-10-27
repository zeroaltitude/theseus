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
- Under load, these timing tests can fail. The flaky list in .config/nextest.toml is empty (since d279767f), so none retries: rerun a failing one alone, and name it in the report. Those marked ‡ and § had fixes joined on 2026-10-06 and 10-07; they are listed because a slow VM can still trip them.
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

**What main holds.** You clone main at 9470df70 or later, with **store format 52**. Your clone has v1's milestones, everything batches 8 to 12 joined, and since 2026-10-07:
- the reliability rows (spawn-enosys: proc.run starts where clone3 is refused, and a failed start says why; phantom-jobs; stop-notice; daemon-robust; daemon-memory), budgets-notify (a session's spend limit and loop cap notify instead of restricting) and daily-ceiling (a daemon-wide hard ceiling on model spend per local day, $200 by default);
- the judgment rows: jev-effort (route.v3: Jev picks the reply's effort), route-feedback, bench-routed (a Theseus-as-shipped bench arm), recall-fallback, memory-lookup (the `memory.lookup` tool), recall-in-deadline (recall's vector query inside its 250 ms deadline);
- the tools: terminals (background jobs a terminal started outlive the session; `term.read` until idle), fs-read-big (`fs.read` reads a window of a file over its size cap), web-search-unkeyed;
- the clients: session-states (sessions live, quiet or retired), cli-status (`theseus status`, `wait --any`), seen-shared, work-types (the work view and the shared notification policy in theseus-protocol), push-fixes, client-names, spawn-ask-follow, paste-order (one connection's requests that change a session apply in their arrival order), tui-paste (a paste is one event), discord-silent (today's pings by default, silence per place, a loop's thinking folded into its process message), footer-cost;
- people and people-jev (person categories, handles and merges; `people.v1` proposes people from text: a model extracts, Jev judges), and the cockpit's context explorer and Benchmarks tab;
- **the keel guard** (scripts/keel-guard.py, a gate phase; see "The keel" below).

**Other changes in flight.** These are being built on the owner's machine now, each on its own branch, and join `main` one at a time while you work. Stay out of their areas unless your task needs it; if it does, keep the edit small and say so in the report.
- people-ui: proposals grouped by person in `theseus ontology proposals` and the cockpit's proposals panel; the nightly people sweep and `[people] sweep_usd_per_day` (core judge/people modules, the CLI's ontology command, cockpit ontology views, config/);
- rsi-join: the self-improvement ledger and its kill switch (`halt self`, resume only from the owner), and health's `self` line (core self modules, theseus-protocol's health, the CLI's health render);
- keel-fixes: scripts/gate.sh, scripts/keel-guard.py and its suite (never edit those);
- discord-calls: theseus-discord's lanes and pings (`Lane::process_ping`), theseus-protocol's notices doc, crates/theseus-discord/AGENTS.md;
- client-recall: recall's short-turn query text (core recall.rs), the query pool's size as a config key, a `lane_us` field on the trace's root and a `theseus.rpc.lane.wait` metric (core rpc/), the TUI's SIGINT/SIGQUIT and a paste with no session (crates/theseus-tui/src/main.rs, app.rs), cli-status's leftovers (the CLI's status render);
- bench-dig: benchmark reports under docs/benchmarks/ and small bench/ harness fixes;
- parallel-calls, a local lane: the calls of one response grouped by class and run together (crates/theseus-core/src/toolrun/calls.rs, a `[tools] parallel_runs` cap, the turn bench's `batch` kind, one sentence in the tools note);
- the stopping point's cloud rows, each a cloud session like you, some joined by the time you start (read `git log origin/main`): wave 1's bench-parity (bench/harbor, bench/report), recall-plan-fix (bench/recall), session-dir, output-kept, tool-input-fit, secrets-shapes, fd-soft-fail and cache-fix; and the bench gate's config-split, turn-one-frame, job-handles, health-o1, cli-pages, h2h-bench, bench-fixes, asyncbench, f10, one-turn-compaction, admission-fifo, jobs-burst, money, stop-nudge, memory-flat, index-memory, warm-first-token, recall-offtopic, project-context, proc-env and file-tools-polish; and 2026-10-11's rows: routing-owner, recall-pool-gate, fresh-defaults, why, standing, provable-memory-a, import-a, fact-recall-a, money-soft, kill-family, earned-autonomy, fork-a, postmortem, mcp-memory, tamper-ledger, reminders, commitments, deferred, openclaw-sync. Your task names the siblings whose files meet yours and who owns what.

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (51 on main today), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Of the rows in flight, session-dir and project-context may need a bump: if yours needs one, say why. Others may bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling:** read scripts/long-files.txt and `wc -l` the files you touch before you add to them; a Rust file the list doesn't name fails past 2,500 lines. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. config-split has joined (its tests in config/tests.rs), so crates/theseus-core/src/config.rs has room for a key's field line; a new section's type goes in a module under config/. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report. Harbor 0.23.0 needs Python 3.12 or later: where python3 is older, run Harbor's tests in a venv (`python3.12 -m venv /tmp/hvenv && /tmp/hvenv/bin/pip install harbor==0.23.0`); bench/async's Harbor tests also need `ASYNC_HARBOR=1` there. Every benchmark run gets a report in docs/benchmarks/: the maintainer writes those, and no task here runs a benchmark.

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is unless your task names it). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you would grow a file past its ceiling, split it: never raise a ceiling (the keel, below).

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, MODEL) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/BRANCH`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- **First, before any of that: push a start commit.** As soon as your setup and first build finish, make an empty commit (`git commit --allow-empty -m "session started"`) and push your branch. Do this even though you have nothing proved yet, and do it before you begin the work. **Why it is mandatory and not merely advised:** on 2026-10-24, three of sixteen rows ended with nothing on their branch but the task commit, and because every other row had pushed only at the very end, a session that died at minute 5 and one that died at minute 110 left *byte-identical* branches — so there was no way to tell a row that never started from one that worked for two hours and lost it. A start commit makes those two cases distinguishable, and it costs one second. The point above ("push at every green point") was already in this preamble on that night and was not followed by any of the sixteen; this step is concrete and checkable, so treat it as part of setup rather than as advice.
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

**The keel.** The gate's keel phase (scripts/keel-guard.py; its docstring lists the rules) fails a range that deletes a test, adds `#[ignore]` or a skip, lowers a test's assertion count, adds an `#[allow]`, raises a bench budget, a spend or loop cap, or a ceiling in scripts/long-files.txt, scripts/shape.sh or .config/nextest.toml, or edits the guard's own files, unless a commit signed by the owner's key acks it. You can't sign, so you can't ack. Rewrite an assertion rather than delete it (a rewritten assertion is not a finding), split a file rather than raise its ceiling, and keep caps. Where your task itself changes a cap or a ceiling, or a test must change its meaning, make that change in a commit of its own whose message says why, and list it in the report under **Keel findings expected** (rule, path, why), so the maintainer can bring it to the owner. Run `python3 scripts/keel-guard.py` before each commit and paste its findings into the report. When the keel phase fails only on findings you listed there, run the gate's later phases yourself (as for the known failures above) and count the commit green.

**Planted reverts:** to prove a test guards a behaviour, plant the bug, show the test fail, then restore the file and `touch` it, so cargo rebuilds it (a restored file with its old mtime keeps the planted build). Run `git status` after every restore.

**Load:** where your task asks for runs under load, use AGENTS.md's recipe. Priority, not count, makes the load: run the test with `nice -n 19`, and beside it four busy loops at nice 0, each `sh -c 'while :; do :; done' &`. Kill the loops by the pids you started, never by a name pattern.
---






# Your task: pin a late result's content classification (`theseus-vahzo`)

## Step 0, before anything else: re-verify every citation in this prompt

**Do this first and report the result.** Every `file:line` below was checked against `origin/main` =
`9470df70` at 04:1x on 2026-10-27 and **all of them verified exactly** — which is unusual and is itself worth
confirming rather than assuming. Open each one in your own clone and say in your report what it actually was.
**Name every citation this prompt got wrong.** A correction is worth more to the maintainer than a fast
commit.

## Why this row exists, and the history behind it

Content flags replaced the old screen model in `9470df70` ("Merge integ/content-flags into main,
theseus-9ymz0"), deleting `crates/theseus-core/src/data.rs` and `open_view.rs` and adding about 6,161 lines.
That merge **closed** an older row (`theseus-utsso`) which had claimed a background job's late result was
never screened. It is screened: `toolrun/late.rs::absorb` ends by calling `self.queue_content(...)`, the same
call `toolrun/complete.rs` makes for a result that arrives inside the turn.

**This row is the one residual of that close.** The line that closes `utsso` is pinned by nothing. Delete it
and the gate is still green — verified at `9470df70`: no `tests_content_*` file mentions a late result, a
background job, or `absorb`.

**Why it matters more than a missing test usually does.** If that line is ever dropped in a refactor, a late
result re-enters the session **unjudged and silently**. `content_wait::unjudged` fails *closed*, so the next
restricted-origin request is **refused** rather than mis-routed. The symptom is therefore a confusing refusal
arriving far from its cause — not a visible break near the change. It is exactly the kind of line a reviewer
deletes as "duplicated with `complete.rs`" because nothing argues back. **Your test is the thing that argues
back.**

## Where it is (verified at `9470df70`; verify it again)

- `crates/theseus-core/src/toolrun/late.rs`
  - `:42` — `pub fn absorb(&self, tc: &TurnCtx<'_>) -> Result<(Vec<Action>, Vec<LateCall>)>` ✅
  - `:64` — `self.queue_content(tc.session_id, &late);` — **the line this row exists to pin**, and the last
    statement of `absorb` ✅
- `crates/theseus-core/src/toolrun/complete.rs`
  - `:51` — the in-turn call site, `self.queue_content(sid, std::slice::from_ref(node));` ✅
  - `:61` — `pub(super) fn queue_content(&self, session_id: &str, nodes: &[Node])`, with its
    `let Some(judge) = ... else { return }` ✅
- `crates/theseus-core/src/content.rs`
  - `:513` — `pub const UNJUDGED_TOOLS: [&str; 2] = ["task.update", "task.close"];` ✅
  - `:516-517` — `pub fn judges(tool: &str) -> bool { !UNJUDGED_TOOLS.contains(&tool) }` ✅
- `crates/theseus-core/src/turn.rs`
  - `:1861` — `self.take_late(t)?;` ✅
  - `:1869` — `.cn_move(t, session, &mut spec, &mut routed_to, data_moved)` — **`take_late` runs before
    `cn_move`, and that ordering is the whole reason the late result must be judged before the next request is
    routed** ✅
  - `:3313` — a **second** `take_late` call site, `let late = match self.take_late(&mut t)`. The row's notes
    do not mention it. Read it, work out whether it is on the same path, and **say in your report which call
    sites your test actually exercises.** If the two paths differ in whether the result is judged before
    routing, that is a finding and may be a second test.

## What closes this row — one test, written the right way

**One test**: a background job whose result lands in a **later continuation turn** is present in the session's
judged set **before the next request is routed**.

**Assert on the classification, not on the call.** This is the single most important instruction in this
prompt. A test that asserts `queue_content` was called is a test of today's implementation and will be
deleted by the next refactor along with the line it guards. A test that asserts *the late result is in the
judged set before routing* survives a refactor that moves the call somewhere better — and still fails if the
behaviour is lost. Write the second kind.

Put it in a `tests_content_*` file — a new one of its own if the existing ones have no natural home, which at
your base they do not (none mentions a late result). Follow the existing `tests_content_*` files for the rig
and the fixtures.

### Confirm your filter actually selects it

A nextest filter with a bare function name and no module path **matches nothing, runs zero tests and exits 0**,
which reads as a pass. Run `cargo nextest list` and paste the output proving your test is selected before you
claim any result from it.

## The plant — this is required and it is the whole point of the row

**Delete the `queue_content` line at `late.rs:64` and confirm your new test goes RED.** Then restore the file,
`touch` it so cargo rebuilds (a restored file with its old mtime keeps the planted build), and run
`git status`.

**A test that passes with that line deleted has not been written, and this row is not done.** Paste the failing
output. If your first attempt passes with the line gone, say so in the report and keep going — that is the
test telling you it is asserting on the wrong thing, which is the exact mistake this row was filed to prevent.

Plant **(b)** as well if your test depends on ordering: move `take_late` at `turn.rs:1861` to *after* the
`cn_move` at `:1869` and confirm the test goes RED. If it does not, your test is not pinning the "before the
next request is routed" half, only the "present in the judged set" half. Report either way.

## The disk — read this before your first build, it cost two sessions a gate cycle each

> Build with `CARGO_INCREMENTAL=0` from the start, and if the gate dies on a full disk or an `ld` Bus error,
> `rm -rf target/debug/incremental` and rebuild rather than investigating the compiler.

Both cloud VMs ran out of disk during a full workspace build on 2026-10-26 (`/dev/vda`, 252 G), and each
burned a whole gate cycle rediscovering it independently. The symptom does not look like a disk problem: it
looks like a linker or compiler fault. It is not. Set `CARGO_INCREMENTAL=0` in your environment now, before
the first build, and you will probably never see it.

## The known-failure set — do NOT read these as your own regression

> The gate's suite has a known failing set on a host without a delegated cgroup: about 34 failures, 20 in
> `theseus-sandbox` (contract, `spawn_100`) and 14 in `theseusd::sandbox` (root daemon, no job cgroup,
> `theseus-pv6i`). These are L1/root failures and are NOT yours. Separately, a test that fails once and then
> passes on rerun and on the untouched base is a fast-host ordering race, not your regression — say so and
> move on. Judge your change by whether a failure reproduces on the base.

A session that does not know this set reads it as its own regression and stalls. **Judge every failure by
whether it reproduces on the untouched base.** Run the base if you are not sure: it is cheaper than guessing.

## Other branches in flight — stay out of their hunks

These are live and unmerged at your base. Your task names the ones that actually meet your files; this is the
full list so you can recognise a conflict when you see one:

- `cloud/20261027-p3t-people-same` and `cloud/20261027-p3t-not-this` — **still running and gating now**
- `cloud/20261027-p3t-fanout-cue`, `cloud/20261027-p3t-rsi-fetch-bound`, `cloud/20261027-p3t-recall-refill-test`
  — returned, under review
- `cloud/20261026-p3p-onto-names-hang`, `cloud/20261026-p3n-task-depth` — older, unmerged
- the local `fix/phsri-crash-ledger-chain`
- the five siblings fired beside you in this wave: `cloud/20261027-p3u-may-build-stop` (rsi/tree.rs's
  `may_build_paced`, rsi/pr.rs), `cloud/20261027-p3u-tree-close-signal` (rsi/join/joining.rs),
  `cloud/20261027-p3u-burn-ancestors-test` (theseus-kernel: day_ceiling.rs, tests_runaway.rs),
  `cloud/20261027-p3u-late-content-test` (theseus-core: toolrun/late.rs, a tests_content_* file),
  `cloud/20261027-p3u-peek-follower-race` (theseus-core: rpc/tests_peek.rs),
  `cloud/20261027-p3u-paused-clock-sweep` (test-health, potentially many files — **if you are not that row
  and you see a paused-clock change land near you, it is theirs**)

**Keep your diff inside the files your task names.** Where you genuinely must touch a shared file, keep the
edit to the smallest possible line count, and say so prominently in the report so the maintainer can resolve
it in seconds.

## Your branch

`cloud/20261027-p3u-late-content-test`, already created from `main` at `9470df70` with this file as its first commit. Your commits go
on top of it. Push with `git push origin HEAD:refs/heads/cloud/20261027-p3u-late-content-test` at every green point, and push the
mandatory empty start commit as soon as your first build finishes.

## Keel

Run `python3 scripts/keel-guard.py` before each commit and paste its findings into the report. **You cannot
sign, so you cannot ack.** If the guard flags anything, put that change in a commit of its own whose message
says why, and list it in the report under **Keel findings expected** (rule, path, why) for the maintainer to
take to the owner. Do not edit `scripts/gate.sh`, `scripts/keel-guard.py`, `scripts/shape.sh`,
`scripts/long-files.txt` or `.config/nextest.toml`.

## Deadline

Six hours by the clock, waits included. If you reach it mid-step, leave that step out, push what is proved,
and report what you found. **Fewer steps, proved and green, is a better row than every step half-done.**

## The report

Write `CLOUD_REPORT.md` at the repository root, commit it as the branch's **last** commit with subject
`cloud report (not for main)`, and push. The run log cuts every message at about 2,000 characters, so the
report on the branch is the only full record that survives.

Say explicitly in the report:

- **what you checked in `git log origin/main` before you started**, and anything in this prompt that had
  already landed;
- **the `file:line` of everything this prompt cited and what it actually was at your base — name every
  citation this prompt got wrong.** This is step 0 and it is not optional. A correction is worth more to the
  maintainer than a fast commit;
- **the red you saw at the base for every behaviour change**, with the test's output;
- `cargo nextest list` output proving your test filters select your tests (a filter that matches nothing runs
  zero tests and exits 0, which looks green and is not);
- the planted reverts: the plant, the failing output, the restore, `git status` after each;
- the keel guard's output for every commit, and a **Keel findings expected** section;
- the gate's result, naming each failing test and why — distinguishing the known-failure set above from
  anything that is actually yours;
- which steps you did not do, and why;
- any `OWNER CALL:` you are leaving open, stated precisely.

Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its
head commit, one line per step, and the gate's result.

## Hard rules

- **Check `origin/main` first for what already landed.** Do not redo merged work.
- **Re-verify every `file:line` in this prompt against your own clone before acting on it, and report any
  citation that is wrong.** This is your first step. On 2026-10-26 a row was fired on a description that was
  later proved wrong in two places, and because there is no way to steer a running cloud session, the only
  thing that could have saved it was its own re-verification.
- **Write no person's name anywhere** — not in code, comments, docs, fixtures, goldens, commit messages or
  your report. No `/home/<user>/` path and no organisation name either. Where a person is meant, write "the
  owner" (the handle `zeroaltitude` only where an identity string is genuinely needed), "collaborator" for
  anyone else. The AI assistant may appear only as "Tabitha/Claude", and your commit trailer names only
  Claude.
- **`crates/theseus-discord/src/golden/dm_flow.txt` must be byte-identical to its state at the base commit.
  If your change moves even one character of it — including a cost figure — stop, change nothing further, and
  report it. Do not regenerate the golden.** The owner's Discord DM task flow is locked. A row has been
  blocked for over a day on exactly one cost-footer character; do not create a second one.
- **No AWS, at all.** No instance, no spot, no `aws` call. **No cloud spend beyond this session: run no
  benchmark and make no model call.**
- Commit subject style: `content: what changed (theseus-vahzo)`. End every message with
  `Co-Authored-By: Claude (cloud session, claude-sonnet-5-5) <noreply@anthropic.com>`. Commit without `-S`.
- Never push to `main`, never force-push, never delete a branch, open no pull request.
- No new dependencies: `Cargo.lock` must not gain a package.
- Don't edit the spec, `docs/status.md`, the README, `docs/benchmarks.md`, `docs/benchmarks/` or
  `docs/design/`.
- Don't commit half a step. If time runs out mid-step, leave it out and report what you found.
- Call no connector and no MCP tool. Use only Bash, Read, Write, Edit, Glob, Grep. **Never search for or
  install a skill, and never run a web search**: either taints the session and every write is then refused.
- End your report with the line `CLOUD REPORT COMPLETE`.
