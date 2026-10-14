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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261013-chat-sessions`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: chat-sessions, `theseus -c` continues this directory's last conversation, `theseus -r` picks one with search, and reopening shows the transcript (theseus-6yka)

Branch: `cloud/20261013-chat-sessions`. Every commit's subject carries `theseus-6yka`. Deadline for the report: 3
hours after you start.

**Prerequisite:** `chat-join`: you build on main after the chat (theseus-cyha) has joined. Read its merge first. The
issue also named project-context (theseus-wadu) as a prerequisite: it isn't one now, because session-dir (joined,
theseus-aab7) gives every session the directory it was started in, which is what "this directory's conversations"
needs. Key on that directory (its git root when it is in a repository).

**Main, as you clone it.** You clone main at `e8444d1c` or later. The preamble above is older; where it differs, this
section wins. **The store's format is 32** (`MANIFEST_FORMAT`, crates/theseus-store/src/store.rs); a bump is one at a
time, with the old layout's reader and its `tests_layouts` sample, and changes two goldens (core_output.txt and
kernel_frames.txt). Read `git log --oneline -30` first. Joined since the preamble and worth knowing: session-dir (a
session keeps its directory), tool-input-fit (a result's first line `[exit code 0 · in /app]`), output-kept, job
handles (`proc_run {background}`, `job_read`, `job_wait`, `job_stop`), earned-autonomy (`policy/earned.rs`: argv-prefix
grants, `theseus policy earned` and `revoke`), why (`theseus why`, `context.explain`'s parts with their tokens,
protocol `why.rs`), fork-a (`session.fork` by reference), standing, money-soft, tui-paste (a paste is one event), and
the F10 driver (`scripts/f10/`).

**Line ceilings** (scripts/long-files.txt; a Rust file not listed may not pass 2,500 lines; never raise one, because
the keel guard fails it): CLI `crates/theseus/src/render.rs` 3,016 of 3,128; protocol `lib.rs` 2,647 of 2,733;
`crates/theseus-core/src/toolrun.rs` **2,499 of 2,500**; `compiler.rs` **2,559 of 2,560**; `turn.rs` 3,496 of 3,525;
Discord `render.rs` 2,886 of 3,009 and `runtime.rs` 3,476 of 3,500. New code goes in modules of its own; a long file
gains only a `mod` line or a call, and where it has no line left, move a few lines out first. **The keel guard** runs
first in every gate: never delete or weaken a test, add an `#[allow]`, or raise a budget, a cap or a ceiling. List
anything you could not avoid under **Keel findings expected**. Never edit scripts/gate.sh or scripts/keel-guard.py.

**F10, the yardstick this row answers to.** F10 is the first ten minutes with a coding agent: 12 steps, 42 match checks
and 5 beat checks, read from the screens (docs/benchmarks/f10/README.md; the checks are `scripts/f10/checks.py`, their
ids never renumbered; the Theseus arm is `scripts/f10/arm_theseus.py`, on the stand-in model, $0). The live run of
2026-10-12: **Theseus 11 of 42** at both sizes (it passes S1d, S3a, S3b, S3d, S6c, S6d, S7c, S8a, S8c, S10b, S11a) and
all 5 beats; **Claude Code 25 of 42**. Its cause, in that report's words: "a bare `theseus` opens no conversation,
replies are raw markdown cut at the edge, a tool line shows only the tool's name, and an edit is approved without its
diff". This row:
1. makes the checks named below pass on its build's screens, and keeps every check that passes today passing;
2. teaches `arm_theseus.py` the key or command its feature adds, in the step methods it names only (other rows edit
   the other steps; an arm with no key records `None`, and its checks say so);
3. runs the driver on its build, `python3 scripts/f10/driver.py theseus --bin-dir <build> --out /tmp/f10` (both sizes),
   then `scripts/f10/scorecard.py`, and puts the per-step table, main's 11 against the branch's, in CLOUD_REPORT.md.
   Use `scripts/build.sh --profile release-thin` when time allows; on a debug build the timing checks (S1b, B1) don't
   count, so say which build ran.

Never change a check, its words, its weight or the scorer to make it pass: a check is a test (keel). A check you think
is wrong goes in the report, unchanged. Don't re-record `scripts/f10/fixtures/` (the maintainer re-records after the
join); `scripts/f10/test_f10.py` must pass as it is. The Claude Code arm is not yours to run.

**F10 checks this row turns green:** S11b (one short command, at most two words, reopens this directory's last
conversation, in front and showing one of its messages), S11c (the reopened screen shows two or more of the earlier
prompts), S11d (a picker of past conversations, searched: typing a word of S8's question lists that conversation).
S11a passes today. **Arm steps you edit:** `s11`: after leaving, `theseus -c`, then `theseus -r` and a word typed, and
record both (`reopen_cmd`, `picker_cmd`, `picker_searched`).

**Why this work exists.** Theseus's daemon keeps every conversation across a client's exit and its own restart (F10's
S11a and B4 are Theseus's), but no short command takes a person back into one. Decided (B6): bare `theseus` starts a
new conversation; `-c` continues this directory's last; `-r` picks.

**What to build:**
1. **`session.list` gains a directory filter** (`dir: Option<String>`, matching the session's recorded directory or
   its git root), newest first, with each session's title, last activity, turns, cost and where it started (CLI,
   Discord, the cockpit). One paged read (the history-pages cursor).
2. **`theseus -c`** opens the chat on this directory's most recent conversation, never a DM, a task's session or a
   shared place's; with none, it says so and opens a new one. **`theseus -r`** opens the picker (the `delight` report's
   M7) in the chat's dock: this directory's conversations from every surface, newest first, searched as you type
   (title, labels, and the text of each conversation's first and last messages from the store, no model call); Tab
   widens it to every directory; Enter continues it here. `theseus chat SID` still opens one by id.
3. **The transcript on reopening:** the last 30 turns replayed above the dock through the chat's own transcript lines
   (your message, tool lines, replies), older turns folded into one line ("12 earlier turns · /history shows them").
4. **History per directory:** the editor's ↑ history kept under `$XDG_STATE_HOME/theseus/history/` per directory, so
   each project keeps its own.
5. **On leaving:** the line the chat prints on exit names the way back (`theseus -c`).

**FAST:** `-c` and `-r` ready ≤ 100 ms p90 on a 1,000-session `theseus-sim synth-store`; the search filters in ≤ 16 ms
per keystroke on that store (the list read once, filtered in memory).

**Tests and planted reverts:** `-c` picks this directory's last conversation and never a DM, a task's session or a
shared place's; `-c` with none opens a new one; the picker filters by a word of a message; Tab widens; the replay
renders the last 30 turns and folds the rest; each directory keeps its own ↑ history; the directory filter matches a
subdirectory's git root. Plant a revert per behaviour (`-c` taking a task's session; the filter ignoring the directory;
the replay unfolded) and record what caught it.

**Live check on this VM:** a scratch daemon on the stand-in: two conversations in one directory and one in another,
`theseus -c`, `theseus -r` with a search; captures in the report. Then the F10 driver.

**Siblings:**
- **context-meter** (theseus-wy6v.2) and **rewind** (theseus-23rk) also build on the chat now: separate files under
  `chat/`; each adds its own command or key to the chat's registry and table.
- **cli-tasks** (theseus-mnd9g) lists tasks, not conversations; **thread-record** (theseus-nhew) adds threads, which a
  later row (T5, theseus-j0yh) switches between in the chat. Keep the picker a list of sessions; don't model threads.

**Doing more, filing less:** fix a defect you find in code you are already in, with a test, and name it in the
report. No person's name anywhere: the owner is "the owner". No secrets. Every new config key has a default and a line
in the template. Run the gate before every commit, and commit CLOUD_REPORT.md as the branch's last commit.

The issue: `theseus-6yka`.

End with a short final message starting `CLOUD REPORT COMPLETE`.

## The owner's decisions (2026-10-13 23:24 MST, verbatim: "i'm happy for yes to all until we're at testing time to eval")
They hold until the evaluation pass; build so each is easy to revisit.
1. **One conversation surface.** Bare `theseus` opens the chat (row `chat`, theseus-cyha, owns it); `theseus chat` is the same thing; the task footer and one-key answers (row `cli-tasks`) live inside that chat, not in a second CLI.
2. **One plan.** Plan mode's steps are the work board's plan items (row `progress`), never a second list of steps.
3. **One owner for Discord threads.** Activity threads (row `thread-discord`, T6) decide which Discord thread a task or message lands in; task rows (`discord-guild-tasks` and others) use its helper and never create threads on their own.
4. **One pacing rule for unprompted posts** (the digest, the morning/horizon view, nudges, the always-on DM ping, proactive voice). Every unprompted post goes through one shared pacer: at most one unprompted DM post per 30 minutes, later items batched into it; the first such row to join builds the pacer (a small module beside the push), later rows use it.
5. The size check joins the gate (a build fails if the binary grows more than 1 MB; the 60 MB miss warns). 6. Programs at once 8 -> 16 (reads 32). 7. Scoped grants are built on Opus with a security review.
