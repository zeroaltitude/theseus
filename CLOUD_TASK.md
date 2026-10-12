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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261011-openclaw-sync`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: openclaw-sync, the OpenClaw import made incremental and nightly: a grown episode appends, the exporter lives in the repo with a cursor, people follow, health names the last sync (theseus-pjmj7)

Branch: `cloud/20261011-openclaw-sync`. Every commit's subject carries `theseus-pjmj7`. Deadline for the report: 5 hours
after you start.

**Why this work exists.** The owner, 2026-10-11, verbatim: "is the nightly people sweep and incremental memory writer
continuing to incrementally import openclaw stuff? that would be awesome since i anticipate using it for a while".
Today it does not. The OpenClaw import (`theseus import openclaw <file>...`, crates/theseus/src/import.rs,
crates/theseus-core/src/import/, the module comment there is the design) ran once on 2026-10-06 over episode files an
outside pipeline wrote (episode format 1: `import/episode.rs`). Two things stop it from being run again:
- **A changed episode is refused.** `write.rs` skips an episode whose id and hash are already stored, and rejects one
  whose id matches with another hash. So a still-growing OpenClaw session can never be imported a second time.
- **The exporter is outside the repo.** It is a Python pipeline on the owner's machine that you cannot read. It used a
  Jev triage keep-filter, gap and topic segmentation, Opus summaries and cross-source dedup. Its episode id hashed the
  first and the last message, so the id changed whenever a session grew.

**Your base: main after `import-a` joined** (theseus-o73y: `theseus import claude-code` and `codex`). That row
added the daemon-side scrub on the import path, a tolerant line reader, and a decision about a source file that grew.
Read its CLOUD_REPORT in the merge commit first. Use its scrub and its tolerant reading rather than building your own.
If its grown-file rule is "skip", your append (1) replaces that rule for every source, with its test updated.

**What to build:**
1. **The importer appends** (`import/write.rs`, `episode.rs`). Take an episode whose id is already imported with
   another hash. If the stored messages are an exact prefix of the new episode's messages (same count up to the
   stored `messages`, each with the same `idx` and `sha256`), then:
   - write only the new messages, as nodes `node_id_of(session, idx)`, each at its own time;
   - update the session record (`ImportedFrom.hash`, `messages`, `as_of.end_ms`) and the tag's counts in the same
     frame or frames, with the counts riding with what they count as today;
   - let the index take the new nodes in the background, paced as it is now;
   - replace the summary node only when the episode has a new summary.
   - Anything else is still rejected and named: a changed, removed or reordered message, or a different first message.
     The rejection names the first differing idx.
   - An erased session is never appended to.
   - Decide how the prefix is proven: read the stored message nodes, or keep a chained digest on `ImportedFrom`. Say
     which in the report, and **say whether it bumps the store format** (if it does, it is this row's one bump).
   - The append goes through the same path for an episode file and for (2).
2. **The exporter, in the repo.** Pick one form and say which and why in the report:
   - Rust: `theseus import openclaw --from <openclaw state dir> [--since <time>] [--agents a,b] [--tag T]
     [--dry-run]`;
   - or a small stdlib-only script, `scripts/openclaw-export.py`, that writes an episode file for `theseus import
     openclaw <file>`.

   Rust needs SQLite and zstd, and the workspace has neither. If `rusqlite` (bundled) plus `zstd` costs too much cold
   build or fails `cargo deny`, the script is the right answer; measure it and say. Whichever you pick, it reads
   OpenClaw's own state:
   - The stores are `<state>/agents/<agent>/agent/openclaw-agent.sqlite`. Open them read-only (`mode=ro`,
     `query_only`), one short statement at a time, and never hold a transaction across sessions.
   - The tables are `session_windows` (key, channel, chat type, times, harness), `transcript_events` (session_id, seq,
     `event_json` or `event_zstd`), `transcript_event_identities`, `transcript_rewrite_watermarks`, and
     `session_transcript_archives` (where a deleted session's transcript goes, as a blob).
   - Learn their shapes from OpenClaw's public source (github.com/openclaw/openclaw, src/config/sessions/), not from
     guesses. Pin the shapes you rely on in fixtures.
   - **One episode per OpenClaw session.** Its id is stable for the session's life: `ep_` plus the sha256 of
     `openclaw-sync`, the agent and the session id. It holds user and assistant text messages (custom, session and
     thinking-level events are skipped and counted). Each message's `idx` is its `seq`, so the prefix rule holds as
     the session grows. There is no triage, no segmentation and no summary.
   - **Integrity and author** follow format 1: the owner's own turns are `operator` (the owner's ids come from a
     config list in the owner's overlay, never the repo); the agent's are `agent`; other people's channel messages
     are `outside`.
   - **The place** comes from the session key and channel: `dm`, `slack-channel`, `discord-channel`, `cron`,
     `heartbeat` or `cli`. Sensitivity is `personal` for a DM and `company-confidential` otherwise. No partner label
     is ever set.
   - **A cursor**, kept in the store (a META record), so each run reads only what changed: per agent, the newest
     `transcript_events` rowid read and the newest archive read; per session, the last seq exported. A session
     deleted between two runs is read from its archive.
   - **The first run** with no cursor and no `--since` starts at the newest `as_of` end of the imported sessions whose
     source is `openclaw-store`, so the 2026-10-06 import is not duplicated. With none, it refuses and asks for
     `--since`. Say in the report that the 2026-10-06 episodes are never extended: their ids hashed their last
     message, and their text was filtered.
   - **The tag:** a new session is tagged `openclaw-sync-YYYY-MM` (the run's month). An appended session keeps the tag
     it was first imported under, so erasing a month removes the sessions that started in it.
3. **Nightly.** The run is either a daemon wake at `[judge] learning_hour`, beside the learning tender and the people
   sweep (`learning/tender.rs`: its `due`, nice 19, paced by pressure, never within 10 minutes of a start), or a
   systemd user timer that `theseus` installs beside `theseusd.service`
   (scripts/user-service.sh). This follows from (2): Rust means the wake, the script means the timer. Say which.
   - The new key `[import.openclaw] state_dir` is off when unset (the public template leaves it unset; the maintainer
     sets it in the owner's overlay).
   - After the import comes `import people` for the touched tags (handles, no model call).
   - Then the people extraction (`judge/people/`) runs over the private sessions the sync created or appended to,
     reading only their new messages. It runs under the existing `[people] sweep_usd_per_day` and shares the sweep's
     day with it.
   - The backfill's per-tag mark (`import.people.propose.<tag>`, the last session in id order) cannot see an appended
     session. Give each synced session a mark at its newest message read, as the live sweep's
     `judge.people.<session>` does.
   - Importing stays the owner's act: refused inside a job (`client::OPERATORS`). A nightly run is the owner's own
     schedule.
4. **Health names the last sync.** Add a `sync` part to `HealthImported` (theseus-protocol/src/import.rs, a new module
   if lib.rs has no room): when it ran, sessions new and appended, messages, rejected, the people step's spend, the
   next run, and its error if it failed. Show it in `theseus health`, the TUI and the cockpit. A failed or missed
   night is a health warning.

**Fixtures only.** Build small OpenClaw stores in the test (SQLite files made by the test, or the script's own
fixtures in `scripts/`), with invented text. They cover:
- a DM, a channel with another person, a cron run and a deleted session in the archive;
- a session that grows between two runs;
- a rewritten event;
- a zstd event;
- an unknown event type;
- a planted secret built at run time from parts.

Never write a whole key-shaped literal into the repository. **No person's name anywhere.**

**FAST:** the sync is a batch the daemon serves around, as the 21,779-episode import was. Nothing is on a turn's path.
A night with nothing new costs one cursor read per agent. Run `theseus-sim bench lifecycle --runs 10 --check` once
and record it.

**Tests:**
- an append writes only the new nodes and the counts add up;
- a second run writes nothing;
- a rewrite is rejected with its idx;
- an erased session is not appended to;
- the cursor reads only new rows;
- the first run's default `--since`;
- the planted secret is absent from the WAL and the spool;
- the people step reads only new messages and holds the sweep's cap;
- health's sync part (goldens);
- the sync is refused inside a job.

Plant a revert for each of these and record what caught it: the prefix check off, the cursor ignored, the scrub
skipped, and the per-session people mark.

**Live check on this VM:** a scratch daemon on the stand-in. Sync a fixture state dir, grow one session, sync again
(an append), then `theseus import list`, a recall that finds the new message from a private place, and health's sync
line. Write the same steps for the maintainer against the owner's real state dir: first a `--dry-run`, then the real
run (timed), then one night's run.

Run the gate before every commit. Commit CLOUD_REPORT.md as the branch's last commit, and end with a short final
message starting `CLOUD REPORT COMPLETE`.
