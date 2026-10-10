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

**What main holds.** You clone main at 2fd1f654 or later, with **store format 26**. Your clone has v1's milestones, everything batches 8 to 12 joined, and since 2026-10-07:
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
- your wave-1 siblings, each a cloud session like you: bench-parity (bench/harbor, bench/report), recall-plan-fix (bench/recall), session-dir, term-render, tool-words, output-kept, tool-input-fit, secrets-shapes, fd-soft-fail and cache-fix. Your task names the siblings whose files meet yours and who owns what.

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (26 on main today, since people), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Of wave 1, only session-dir may need a bump: if yours needs one, say why. Others may bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling:** read scripts/long-files.txt and `wc -l` the files you touch before you add to them; a Rust file the list doesn't name fails past 2,500 lines. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report. Harbor 0.23.0 needs Python 3.12 or later: where python3 is older, run Harbor's tests in a venv (`python3.12 -m venv /tmp/hvenv && /tmp/hvenv/bin/pip install harbor==0.23.0`); bench/async's Harbor tests also need `ASYNC_HARBOR=1` there. Every benchmark run gets a report in docs/benchmarks/: the maintainer writes those, and no task here runs a benchmark.

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is unless your task names it). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you would grow a file past its ceiling, split it: never raise a ceiling (the keel, below).

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261010-cache-fix`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: cache-fix, the owner's long conversation stays cached: an hourly keep-warm read, profiles inherit the 1-hour cache, big old attachments stubbed at a cold rewrite, and health lists each profile's TTL (theseus-ezeg)

Branch: `cloud/20261010-cache-fix`. Every commit's subject carries `theseus-ezeg`. Deadline for the report: 4 hours
after you start.

**Why this work exists: money, every day.** A long-lived conversation can carry a prompt of about 626,000 tokens (a
large attached document alone can add hundreds of thousands). Its profile caches for 5 minutes, so the first message
after more than 5 quiet minutes rewrites the whole prompt into the cache: about $7.85 a message on Fable 5.1 (a
5-minute cache write is 1.25x the input price; a read is 0.025x of it, $0.25 a million). Three such cold writes were
measured in one day, $7.84 to $7.92 each, where the warm turn before cost $0.84. The cause is in
the config: `[model]` sets `cache_ttl = "1h"`, but a profile does not inherit `[model]`'s cache_ttl
(crates/theseus-core/src/config.rs, its test `cache_ttl_is_five_minutes_or_an_hour_per_profile` holds that), and the
fable, opus and haiku profiles set none. The 1-hour cache alone is not the answer (its writes cost 2x, and most of the
owner's gaps are overnight, so alone it would have cost more); with an hourly keep-warm read it is. The owner approved
this design on 2026-10-08 (the stopping point's cost default, item 1); his config gets `cache_ttl = "1h"` on fable and
opus once this lands, never before.

**What to build:**
1. **Profiles inherit `[model]`'s `cache_ttl`** unless they set their own. A profile that must stay at 5 minutes says
   `cache_ttl = "5m"` explicitly, so "5m" must now serialize when set (today the default serializes as nothing).
   Rewrite `cache_ttl_is_five_minutes_or_an_hour_per_profile` for the new rule (keep or raise its assertion count: the
   keel guard counts them per test), and add a test of an explicit "5m" under a 1-hour `[model]`. Document it in the
   example config's template and its tests.
2. **The keep-warm read.** For a conversation session whose profile caches for 1 hour, after its last turn ends, the
   daemon sends one cache-refreshing request about every 55 minutes (a config key, under the TTL by a margin you
   justify), for up to 24 hours after the session's last message (a config key; 0 turns it off; inherited by profiles
   like cache_ttl). The request replays the session's prefix exactly as its next compile would send it, up to the last
   cache breakpoint, with the smallest output the provider allows (no tools run, nothing stored as a turn, no message
   in any surface), so it reads the cache and refreshes its TTL. Rules:
   - only where it pays: a prefix above a size floor (a config key; a keep-warm read of a small prompt costs more than
     the rewrite it saves); say the break-even in the report (on Fable 5.1 one 1-hour cold write costs as much as
     about 80 reads at 0.025x);
   - only for sessions a person talks in (a private place: the owner's DM, the CLI, the TUI), never for task sessions,
     jobs, the judge or Jev;
   - never while a turn of that session runs or is queued; a new message resets its 24-hour window; a retired or
     erased session stops it; a stopping daemon sends none (the stopping class: `turn/stopping_step.rs`);
   - each read is a model call like any other: reserved and refused under the spend limits and the daily ceiling, a
     ledger row of its own kind (`keep_warm`, with its tokens, cache read and cost), visible in the money river; a
     refused or failed read logs once and gives up for that session until its next message;
   - after a daemon restart, the windows are rebuilt from the sessions' last-message times (no stored field needed: if
     you find one is, bump the format and say why), or the report says plainly that a restart drops them.
   Put it in a module of its own (crates/theseus-core/src/keep_warm.rs or under turn/), scheduled off every turn's path.
3. **Stub big old attachments at a cold rewrite.** When a request will rewrite the cache anyway (the last call of the
   session is older than its TTL, and no keep-warm kept it), an attachment's extracted text above a size floor (a
   config key) that is older than the last few turns is sent as a stub instead: one line naming it, its size in
   tokens, and how to read it again (the file tool that reads attachments). Never inside a warm window: that would
   break the cache it keeps. The stored history is unchanged (the stub is the compile's, not the store's), and the
   manifest says which attachments were stubbed. A cold restart of a 626,000-token prompt should cost about $2.50
   instead of $7.85: show the arithmetic on a fixture.
4. **Health lists each profile's TTL** (and whether it inherited it), and the keep-warm's state: sessions kept warm
   now, reads and dollars today. Your health fields go in a type in crates/theseus-protocol/src/health.rs (lib.rs is
   at its ceiling and gains no line), and the CLI's health lines in a module under crates/theseus/src/render/.
5. **FAST.** Nothing on a turn's path: the keep-warm's timer and its call run off it, and the stub decision is a
   comparison the compile already has the facts for. Run `theseus-sim bench lifecycle --runs 10 --check` once and
   record it (the cloud gate skips the benches).

**Doing more, filing less** (the owner, 2026-10-10): a defect you find in code you are already in, fix it here with a
test and name it in the report.

**Siblings that meet your files** (each a cloud session beside you):
- **session-dir** adds a per-session system block (block 3) with **the fourth and last cache breakpoint**: you take
  no breakpoint. Your keep-warm replays whatever the compile produces, so its join changes nothing of yours; your test
  should compare the keep-warm request's prefix with the next turn's, byte for byte, so it holds after that join too.
- compiler.rs is 5 lines under its ceiling (2,555 of 2,560), and session-dir needs most of that room: you add at most
  a call and a `mod` line there; the stub logic goes in a module under crates/theseus-core/src/compiler/.
- config.rs is near its ceiling (see scripts/long-files.txt): new config types under crates/theseus-core/src/config/.
- fd-soft-fail adds health fields too: both in health.rs, different types.
- Another session on the owner's machine adds a `self` line to health today: expect a trivial merge beside it.

**Tests** (a fake provider, as the compiler's and the turn's tests use, with the paused clock): a 1-hour session gets
a keep-warm read at the interval and none after 24 hours; the read's prefix equals the next turn's; none during a turn,
none for a task session or a shared place, none past the daily ceiling (refused and logged), none from a stopping
daemon; a new message resets the window; inheritance and the explicit "5m"; an attachment stubbed at a cold rewrite
and not inside a warm window; health's lines. Plant a revert for each (the inheritance off, a read during a turn, the
stub inside a warm window, the ceiling unchecked) and record what caught it.

**Live check on this VM:** a scratch daemon with the stand-in (which reports cache reads and writes if it can; if it
can't, say so and use the fake provider's capture), a profile at 1 hour, a short keep-warm interval set in the config
for the check: a session's turn, then keep-warm reads at the interval with their ledger rows, then a cold rewrite of a
session holding a big attachment showing the stub in the request. Write the commands for the maintainer on the owner's
machine with the real model, including how to read the keep-warm rows in the ledger, and the config lines the
stopping point's config row then applies (1h on fable and opus).

Run the gate before every commit; commit CLOUD_REPORT.md as the branch's last commit, and end with a short final
message starting `CLOUD REPORT COMPLETE`.
