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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261009-self-ledger-kill`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the self-improvement ledger and the kill switch, off by default (theseus-pw1q.4 and theseus-pw1q.2)

Branch: `cloud/20261009-self-ledger-kill`. Every commit's subject carries `theseus-pw1q.4` or `theseus-pw1q.2` (the
part it is). Deadline for the report: 4 hours after you start.

**Main has moved since the preamble was written.** You clone main at `d348ac67` or later. The preamble's "What main
holds" and "Other changes in flight" are out of date: everything they list has joined, and so have batch 12's rows,
the 2026-10-08/09 joins (cli-status, seen-shared, work-types, spawn-ask-follow, push-fixes, client-names, terminals,
footer-cost) and the self-improvement plan's docs. The store's format on main is **25**: if your change adds a field
or a variant to a stored record, or a new record kind, bump `MANIFEST_FORMAT` (crates/theseus-store/src/store.rs) to 26
with the old layout's sample in theseus-core's tests_layouts.rs, as AGENTS.md's store version rule says, and say why
in the report (others may bump it too; the maintainer renumbers at the merge).

Line ceilings in scripts/long-files.txt, as of `d348ac67`: **crates/theseus-protocol/src/lib.rs is full (2,733 of
2,733)**, **crates/theseus/tests/golden.rs is full (2,506 of 2,506)**, crates/theseus-core/src/config.rs is at 2,926 of
2,927, the Discord binding's render.rs at 3,008 of 3,009, crates/theseus/src/render.rs at 3,108 of 3,128, and
crates/theseus-core/src/compiler.rs at 2,555 of 2,560: add no net line to them (new code in a module of its own, only
the `mod` line or the call in the long file, saving a line elsewhere in it if you must). turn.rs is at 3,428 of 3,525
and the kernel's kernel.rs at 2,854 of 3,030. A Rust file the list doesn't name fails past 2,500 lines
(crates/theseus-core/src/toolrun.rs is at 2,452).

**Why this work exists: the owner's decision on self-improvement (2026-10-09).** Theseus is to build its own code,
yardsticks and backlog, and act first: it files and ranks its own backlog, starts the top items, builds each on a
branch, gets an independent review from a different session, joins and installs when every gate passes, and tells the
owner after (what changed, why, what it cost). A regression after a join reverts on its own; the owner's veto is a
one-click revert; a weekly digest and a live ledger show it all. A short hard keel still needs the owner's yes:
deleting or loosening tests, budgets or ceilings; sealed holdout sets kept out of Theseus's reach; one-way store format
bumps; security, policy, secret, permission and IAM changes; spend limits ($10 a self-branch, $30 a day); and a kill
switch that halts all self-directed work. The plan is docs/design/rsi-plan.md on main (read its section 0 and the
"governance spine" in section 2). **The machinery is built now and ships off by default:** nothing self-directed runs
until the owner sets `[self] mode = "act"`, after the stopping point's benchmark rerun. So every row here builds a
mechanism, its tests and its switch, and changes nothing a running daemon does while the switch is off.

**Rows launched or staged with this one** (each on its own branch, joining main in any order; stay out of the others'
areas, keep any edit to a shared file small and surgical, and name it in your report):
- keel-guard (theseus-pw1q.1): a gate step and its checker (scripts/gate.sh, a new scripts/keel-guard script, its
  tests);
- self-ledger-kill (theseus-pw1q.4 and .2): the `[self]` config section, the `rsi` module in theseus-core, `self.*`
  ledger rows, `theseus self log`, and the kill switch (`theseus self halt` / `resume`);
- staged until self-ledger-kill joins: self-budget (pw1q.3), sealed-holdouts (pw1q.5), auto-revert (pw1q.10).

**For every row:** write no person's name anywhere (the owner is "the owner"); give every new config key a default,
and put it in `theseusd example-config`'s template with its template tests; follow the store's format rule above; run
the gate before every commit; commit CLOUD_REPORT.md as the branch's last commit.

**Why:** the owner chose act-first self-improvement, made safe by visibility and a brake rather than by approvals. So
before Theseus may change itself, two things must exist: a ledger that shows everything it changed about itself (with
the numbers and the undo), and a kill switch that stops all self-directed work at once and that only the owner can
release. This row is the foundation the other machinery rows build on (budgets, sealed holdouts, auto-revert, the
backlog, the branch builder and joiner): **lay it out so they can add to it without touching your files more than a
line.**

**What to build, in this order** (commit and push each green step; if time runs out, stop at a green step and say
what is left):
1. **The `[self]` config section** (a module under crates/theseus-core/src/config/; config.rs gains only the field and
   its `mod`/`use`): `mode = "off"` (`"off"` or `"act"`; an unknown value is refused at load with the key's name),
   `digest = "weekly"` (`"weekly"` or `"off"`). Leave room, as an empty table the docs mention, for `[self.budget]`
   (the self-budget row adds `branch_usd = 10.0`, `day_usd = 30.0`). Read at start. Its template lines and template
   tests in `theseusd example-config`.
2. **The `rsi` module in theseus-core** (crates/theseus-core/src/rsi/): the one place every self step asks "may I
   run?" (`rsi::gate(&ctx) -> Allowed | Halted{why, at} | Off`), which is `Off` while `mode = "off"` and `Halted` while
   the kill switch is on, whatever the mode. Nothing calls it yet but the tests and the CLI; write its doc comment as
   the contract the later rows follow (every self step calls it before it starts and between its phases).
3. **The kill switch** (theseus-pw1q.2): a durable halt state in the store (a new record kind or a ledger-derived
   state: choose, and if it is a new stored record follow the format rule), and ledger rows `self.halted { by, why,
   place }` and `self.resumed { by, place }`.
   - **Halt** is open to anyone who may speak to the daemon: protocol `self.halt { why? }`, the CLI's `theseus self
     halt [why]`, and the Discord binding's word in the owner's DM (`halt self` or the like: keep it one obvious word
     pair, and say what you chose). Halting is idempotent.
   - **Resume** only from the owner in a private place, the way an extension's Load ack is (crates/theseus-core/src/
     extend/: the ack only from the owner in a private place, never from a job's shell or L1). Protocol `self.resume`,
     CLI `theseus self resume`. A resume from a shared place, from a job's shell, or from anyone else is refused with a
     reason, and the refusal is a ledger row.
   - **The default is halted**: a store that has never seen a resume reads as halted, so switching `mode` to `"act"`
     alone does not start anything; the owner's first resume does.
   - The halt survives a restart and an install (test it with a restart in place).
4. **The ledger view** (theseus-pw1q.4): the `self.*` row kinds the later rows will write, declared now with their
   fields and doc comments (self.backlog.filed / ranked / started, self.branch.built, self.review.verdict, self.joined,
   self.installed, self.reverted, self.vetoed, self.exam.counted, self.budget.reserved / settled / stopped, plus
   self.halted / resumed), and one reader, `rsi::log(since, kinds)`, that returns them **together with today's
   self-change rows** (pack.mode, judge.proposal, extend.*, route.corrected, consolidation's syntheses: find their
   kinds in the ledger and the learning code), newest first, each with what, why, the numbers it carries and its undo
   (the command that reverses it, where there is one). Protocol method `self.log { since?, limit? }` (types in a
   protocol module file of their own: lib.rs is full; regenerate the TypeScript), and the CLI's `theseus self log
   [--since 7d] [--json]`, rendered like the other list commands. A new command group `theseus self …` in the CLI:
   keep the long files untouched but for a line.
5. **The digest** (if time allows, else write its design in the report): `rsi::digest(week)` builds the weekly text
   ("What Theseus changed about itself this week": counts by kind, each join with its numbers and its undo, halts,
   cost); `theseus self digest` prints it; while `digest = "weekly"` and `mode = "act"`, the Discord binding posts it
   to the owner's DM once a week (the binding's existing scheduled-notice path). Off while `mode = "off"`.
6. **The cockpit card** (only if steps 1 to 5 are green with time left; else design notes): "What Theseus changed
   about itself", reading `self.log`, with a halt button and the state shown. Follow cockpit/AGENTS.md.

**Tests and planted reverts, for each:**
- `[self]` parses its defaults, refuses an unknown mode, and the template carries it;
- `rsi::gate` is `Off` with mode off, `Halted` on a fresh store with mode act, `Allowed` after the owner's resume,
  `Halted` again after a halt from anyone;
- a resume from a shared place, from a job's shell (the confirm path's job origin) and from a non-owner is refused and
  ledgered; the owner's private resume works;
- the halt survives a restart in place;
- `self.log` returns the new rows and today's kinds together, newest first, filtered by `since`;
- the digest (if built) lists a seeded week's rows and is not posted while mode is off.
For each, plant the bug, show the test fail, restore and `touch`.

**Proof (stand-ins only):** a scratch daemon of your build on a fresh state dir with the stand-in model: `theseus self
log` empty but for today's kinds you can provoke; `theseus self halt "test"`, a restart, `self log` shows the halt;
`self resume` from the CLI as the owner works and from a job's shell is refused. Paste the transcript.

**FAST:** nothing on the turn path changes while `mode = "off"`: `theseus-sim bench turn --check --runs 5 --burst 0`
keeps a plain turn's 5 frames and a tool turn's 9. `rsi::gate` must cost under 1 ms (one read of a cached state, no
store scan). The gate skips the lifecycle bench here (`THESEUS_GATE_NO_BENCH=1`): if your halt state is read at start,
run `target/debug/theseus-sim bench lifecycle --runs 10 --check` once on your last commit and report it (on this VM a
miss by time alone is noise; a phase that fails for any other reason is a finding).

**Read first:** the root AGENTS.md, crates/theseus-core/AGENTS.md, the protocol's and the CLI's AGENTS.md; then
crates/theseus-core/src/ledger.rs, extend/ (its ack and the private-place rule), learning/ (its rows), config/ (how a
section is added), and the CLI's cmd.rs and one list command (e.g. `theseus sessions`).

**Not in this task:** the budgets (self-budget), sealed holdouts, auto-revert, the backlog, the branch builder and
joiner, any self-directed action. While `mode = "off"` nothing you build may do anything on its own.

The issues: `theseus-pw1q.4` (the ledger) and `theseus-pw1q.2` (the kill switch).
