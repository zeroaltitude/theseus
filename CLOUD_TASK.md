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

**What main holds.** You clone main at fdd14925 or later, with **store format 27** (27 since the per-message effort cache fix). Your clone has v1's milestones, everything batches 8 to 12 joined, and since 2026-10-07:
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
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (27 on main today), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Of the rows in flight, session-dir and project-context may need a bump: if yours needs one, say why. Others may bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling:** read scripts/long-files.txt and `wc -l` the files you touch before you add to them; a Rust file the list doesn't name fails past 2,500 lines. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. config-split has joined (its tests in config/tests.rs), so crates/theseus-core/src/config.rs has room for a key's field line; a new section's type goes in a module under config/. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report. Harbor 0.23.0 needs Python 3.12 or later: where python3 is older, run Harbor's tests in a venv (`python3.12 -m venv /tmp/hvenv && /tmp/hvenv/bin/pip install harbor==0.23.0`); bench/async's Harbor tests also need `ASYNC_HARBOR=1` there. Every benchmark run gets a report in docs/benchmarks/: the maintainer writes those, and no task here runs a benchmark.

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is unless your task names it). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you would grow a file past its ceiling, split it: never raise a ceiling (the keel, below).

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261013-self-join`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: self-join, an independent review on another model with a planted revert, the join lock, one gate, a signed merge, the install and a notice (theseus-pw1q.9)

Branch: `cloud/20261013-self-join`. Every commit's subject carries `theseus-pw1q.9`. Deadline for the report: 5
hours after you start.

**Main, as you clone it.** You clone main at `9fa84735` or later. The preamble above is older than that and some of
its facts are stale; where they differ, this section wins. **The store's format is 32** (`MANIFEST_FORMAT`,
crates/theseus-store/src/store.rs). config-split has joined: crates/theseus-core/src/config.rs is 1,745 lines, and the
`[self]` section has its own module, crates/theseus-core/src/config/rsi.rs. Read `git log --oneline -20` first.

**Line ceilings** (scripts/long-files.txt; never raise one: the keel guard fails it):
crates/theseus-protocol/src/lib.rs is at 2,647 of 2,733, crates/theseus/src/render.rs at 3,016 of 3,128, and
crates/theseus-core/src/turn.rs at 3,496 of 3,525. A Rust file the list doesn't name fails past 2,500 lines. Put new code
in modules of its own and add only a `mod` line, a field or a call to a long file.

**Why this work exists: the owner's decision on self-improvement (2026-10-09).** The owner: *"I do want to try to build
the code, yardsticks and backlog -- can we do more than just propose? I'm a 'shoot now ask questions later' kinda
guy."* So Theseus acts first and tells the owner after. It files and ranks its own backlog and starts the top items. It
builds each item on a branch and gets an independent review from a different session on a different model. When every
gate passes, it joins and installs the change, then tells the owner what changed, why, and what it cost. A regression
after a join reverts on its own, and the owner's veto is a one-click revert.

A short **hard keel** still needs the owner's yes. A change that touches the keel is not refused: it stops at an owner
card and waits, and the rest of the queue goes on. The keel covers:
- deleting or loosening tests, budgets or ceilings;
- sealed holdout sets, kept out of Theseus's reach;
- one-way store format bumps;
- security, policy, secret, permission and IAM changes;
- the spend limits ($10 a self-branch, $30 a day);
- the kill switch.

The plan is docs/design/rsi-plan.md on main. Its section 0 is the decision and wins over the rest; section 2 is the
governance spine. Epic theseus-pw1q.

**Built already, on main:** keel-guard (scripts/keel-guard.py and the gate's `door`, scripts/AGENTS.md); self-kill
(the META record `self.switch`, `theseus self halt|resume`, a resume only from the owner); rsi-ledger (`self.*` rows,
`theseus self log`, the cockpit's "What Theseus changed about itself" card, cockpit/src/components/SelfChanges.tsx, and
the weekly digest). Read crates/theseus-core/src/rsi/mod.rs ("The contract"), crates/theseus-protocol/src/rsi.rs
(`SELF_ROWS`: every `self.*` row kind is already declared there with its fields, before its writer lands),
crates/theseus-core/src/fact/rsi.rs, crates/theseus/src/self_cmd.rs and crates/theseus/src/render/rsi.rs. Add to these;
don't redesign them.

**Off by default, always.** Nothing self-directed runs until the owner sets `[self] mode = "act"` and resumes the
switch. That switch-on is a separate row, after the benchmark rerun, and not yours. Every self step asks `rsi::gate`
before it starts and between its phases, and stops on anything but `Gate::Allowed`. With mode off, a running daemon
behaves exactly as today: no new frame, no new task, no new timer firing work.

**The seams the self-improvement rows share.** Eight rows are launched together on their own branches. They are
defined so that most of them build at once on main:
- **S1, the self principal.** Every self step's model work runs in a task (and its session) whose `origin.principal`
  is the literal string `"self"` (`TaskOrigin`, crates/theseus-protocol/src/tasks.rs; a reserved string, no format
  change). A `"self"` principal never holds an owner handle and can never ack anything.
- **S2, the budget.** self-budget enforces both spend lines at the model-call reservation of every turn in a
  `"self"`-principal session. For spend outside a turn, it adds `rsi::budget::reserve` and `settle`. A row that
  spends only through turns needs nothing from self-budget to compile.
- **S3, the start.** self-backlog starts an item by opening a `"self"` task. The task's model then calls
  `self.propose_change { item, issue, why }`, which is self-pr's tool.
- **S4, the branch.** self-pr ends each attempt with a `self.branch.built` row (`SELF_ROWS` fields) and a frozen report
  at `<state>/self/reports/<branch-with-slashes-as-dashes>.md`, ending `<!-- REPORT COMPLETE -->`.
- **S5, the sealed store.** sealed-holdouts owns `[self] sealed_dir` and `rsi::sealed`. Code outside Theseus may read
  the sealed dir; code inside never can.
- **S6, the tree.** self-pr owns `rsi::tree`, which gives a sandboxed worktree with its own build tree, the gate run
  through main's door, and a push to the `self/` namespace. self-join and auto-revert reuse it.

**Rows launched together and where each one works.** Stay in your own area. Keep any edit to a shared file small and
put it at the end of its block, so that the joins stay simple:
- self-budget: `rsi/budget.rs`;
- sealed-holdouts: `rsi/sealed.rs`;
- self-bench: `bench/self/`;
- self-backlog: `rsi/backlog.rs`;
- self-pr: `rsi/tree.rs` and `rsi/pr.rs`;
- self-join: `rsi/join.rs`;
- auto-revert: `rsi/revert.rs`;
- self-exams: `rsi/exams.rs`.

These files are shared by several rows, so add only your own lines to each:
- `rsi/mod.rs`: your `mod` line;
- `config/rsi.rs` and the template's `[self]` block: your keys, as lines of their own;
- `theseus-protocol/src/ledger.rs`: your `LedgerKind` variants, on the commit that first writes them (the reader rule);
- `theseus-protocol/src/rsi.rs`: your wire types, at its end, under a comment naming your row;
- `self_cmd.rs`: one match arm, with the verb's code in `crates/theseus/src/self_cmd/<verb>.rs`.

Put your facts in `fact/rsi_<row>.rs`, not in `fact/rsi.rs`. A new `LedgerKind` is not a format bump. A new stored
record kind or field is: bump `MANIFEST_FORMAT` by one with the old layout's sample in tests_layouts.rs, and say why
in the report. Others may bump it too, and the maintainer renumbers at the merge.

**The keel guard runs first in every gate.** It fails a branch that does any of these, unless a commit signed by the
owner's key acks it, and you can't sign:
- raises a ceiling;
- removes a test or lowers an assertion count;
- adds an `#[allow]` or `#[expect]`;
- raises a bench budget or a spend or loop cap.

So split rather than raise, rewrite an assertion rather than delete it, and keep caps. Never edit scripts/gate.sh,
scripts/keel-guard.py or scripts/keel-signers. List anything you could not avoid under **Keel findings expected**.

**For every row:**
- No person's name anywhere: the owner is "the owner".
- No secret in code, tests or the report.
- Every new config key has a default and a line in `theseusd example-config`'s template, with its template test.
- Tests with planted reverts: plant the bug, show the test fail, restore it and `touch` the file.
- Run the gate before every commit.
- **FAST:** a plain turn's frames are unchanged (the turn bench's 5 and 9), and a self step's checks cost under 1 ms
  on the hot path.
- **Doing more, filing less** (the owner, 2026-10-10): fix a defect you find in code you are already in, with a test,
  and name it in the report.
- Commit CLOUD_REPORT.md as the branch's last commit.

**Why:** the owner decided that self-joins are **a notice after, not a yes before**. What replaces the owner's yes is an
independent review on another model with a planted revert, the join lock, one gate on the merge, a plain signed merge,
the install, and a notice that carries the one-click revert. The hard keel still stops at the owner's card.

**Prerequisite:** self-pr has joined. Read its `rsi::tree` (worktree, own target, the gate through main's door, the
`self/` push), its `self.branch.built` rows and its frozen reports (seam S4), and build on them; don't fork them.

**What to build** (rsi/join.rs):
1. **Config:**
   - `[self] reviewer_profile` defaults to `"fable"`. It must name a different model from the branch's builder: when
     the two match, the review is refused with its reason.
   - `[self] join_lock` is a mkdir lock directory, defaulting to `<state>/self/join.lock`. The owner's machine points
     it at the outer loop's join lock, so a self-join and the outer loop's joins never overlap. Its `owner` file names
     the holder on its first line and ends with a `done:` line, the format the outer loop's locks use.
2. **The review.** It runs for each green `self.branch.built` whose branch is not yet reviewed, after `rsi::gate`:
   - **The reviewer** is a check task in a `"self"` session on `reviewer_profile`. Reuse crates/theseus-core/src/check.rs,
     which "checks a task's work by its claim, never by its working": it sees the diff, the report and the item's
     evidence, and nothing of the builder's transcript. Its verdict is accept or reject, with reasons.
   - **The planted revert** is done by code, not by the reviewer. In a tree, undo the branch's non-test changes and run
     the branch's new and changed tests: at least one must fail. If none fails, the branch is rejected as unproved.
   - Write `self.review.verdict { branch, head, reviewer, verdict, planted_revert }`. On a reject, the branch is kept,
     and the item returns to the backlog with the reasons in its evidence.
3. **The keel check.** Before the join, the merge's diff is classified. These go to the owner's card with the diff
   summary, and wait:
   - a keel-guard finding;
   - a `MANIFEST_FORMAT` change;
   - a path in a security class: crates/theseus-sandbox, policy and approvals, `aws/` IAM and policies, secrets,
     `scripts/keel-*`, `scripts/gate.sh`, `scripts/long-files.txt`, the sealed dir's code;
   - a spend key.

   A `"self"` principal can't answer the card. Keep the class list in one const with a test per class.
4. **The join.** Take the lock, then:
   1. fetch;
   2. dry-run the merge on the current `origin/main` (`merge-tree`);
   3. make a plain `--no-ff` merge in a tree (never a rebase);
   4. run the gate on the merge through main's door, with the benches;
   5. sign with the key `user.signingkey` names in `[self] repo`. If no key is configured, stop at an owner card: an
      unsigned merge never reaches main;
   6. push main to `[self] remote`;
   7. release the lock with its `done:` line.

   If main moves before the push, redo steps 2 to 4. Write `self.joined { branch, head, merge, gate, cost_usd }`.
5. **The install.** It runs only on the owner's machine and only when `[self] install = true` (default `false`; the
   owner's standing OK becomes this key):
   - build with `scripts/build.sh` as the install does;
   - hand the restart to a transient `systemd-run --user` unit, because the daemon must not kill itself mid-step.

   The new daemon finds the `self.joined` merge that matches its version and has no `self.installed` row. It writes
   `self.installed { merge, binaries, version }`, then sends **the notice** to the owner's DM: what changed, why, the
   numbers, the cost, and the revert (`theseus self revert <merge>`, which auto-revert builds next).

**Tests and planted reverts.** Use a throwaway origin repo, stand-in gates and a stand-in reviewer:
- the same model for the builder and the reviewer is refused;
- an unproved branch (its tests pass without the fix) is rejected;
- each keel class stops at the card;
- a held lock waits;
- main moving mid-join redoes the dry run;
- nothing is pushed without a signature;
- a red gate on the merge leaves main untouched;
- the restart's notice fires once, never twice;
- a halt between phases stops it and says where;
- mode off does nothing.

Then a scratch live check: a seeded green self branch is reviewed, joined to a local bare origin, and noticed.

**Model:** with self-pr, this is the round's design-heavy pair: it is the door from a self-made change to main.

**Not in this task:** auto-revert and the veto (the next row), building branches, and the budget (the reviewer's turns
run in `"self"` sessions, which self-budget counts).

The issue: `theseus-pw1q.9`.

End with a short final message starting `CLOUD REPORT COMPLETE`.
