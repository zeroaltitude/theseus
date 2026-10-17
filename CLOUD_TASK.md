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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-discord-emoji-catchup`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: discord-emoji-catchup, animated app emojis for task states, the 08:00 and 18:00 digest post when something happened, and one catch-up message after Discord was unreachable (theseus-ss36w)

Branch: `cloud/20261017-discord-emoji-catchup`. Every commit's subject carries `theseus-ss36w`. Deadline for the report: 3 hours after you start.

**The owner's words (Discord, 2026-10-17 09:30, verbatim):** "In tasks: what are discord forums and threads -- I didn't
know about these! Does the discord app now have the power to make new channels? If so, I'd love to start with every
task that's multi step or expected at more than 10 minutes of work to get a channel automatically. I love your other
ideas and not yet implemented ideas so please start those ASAP for quick implementation and join to main today".
This row is one of those "not yet implemented ideas", written 2026-10-13 and refreshed onto today's main. Its target is
the owner's (2026-10-08): "you'll always be kept in the loop on status". Three Discord pieces of the plan no row has
built: **animated app emojis** for task states (a spinning "working", a pulsing "needs you"), **a digest at 08:00 and
18:00, only when something happened**, and **one catch-up message after Discord has been unreachable** instead of a
flood. Discord stays lively (today's pings by default); the digest and the catch-up are single messages.

**Prerequisite:** `main` (c6f62abf or later; store format 45). Its 10-13 prerequisites (discord-board, work-digest) have
joined. Five sibling rows build in parallel today: discord-task-channels (theseus-bh3t.1), discord-reactions (bh3t.2,
which may add `paused` to `task_view::state_words`), voice-tasks (bh3t.3), discord-war-room (bh3t.4), task-offer-chip
(bh3t.5). You join last of the Discord rows; keep your edits in the files below small so the joins stay mechanical.

## Your row
| # | Slug | Beads | Scope | Files | Tests (planted revert) | Surface | Depends on | Where | Size |
|---|---|---|---|---|---|---|---|---|---|
| 6 | `discord-emoji-catchup` | theseus-ss36w | app emojis with a Unicode fallback through one function; the digest post at `[discord] digest_at` slots through the shared pacer; the outage fold (cards and replies never folded) | new `dc/runtime/emoji.rs`, `crates/theseus-discord/assets/emoji/` + the script that makes them; one call per emoji site in `dc/render/{board_v2,snapshot,task_view,card_v2}.rs`; new core module for the digest post beside `initiative/morning.rs`, one call in `harness.rs`, `pacer.rs`'s `UNPROMPTED`; new `dc/courier/catchup.rs` + one call in `courier.rs`; `config.rs` (two keys after `status_line`) + the template; `theseus-sim/src/fake_discord/emojis.rs` (new) | emojis uploaded once, reused after a restart, re-uploaded when deleted, Unicode when the route fails; the digest at a slot with lines posts once and never twice across a restart, nothing with no lines, waits while the pacer is closed (paused clock); the fake's down then up folds four silent posts into one message, each settled, while a card during the outage posts on its own | Discord + core | none | cloud | M |

## What is on main today (checked 2026-10-17; none of the three exists)
- **Emojis:** no `app_emojis`, no `/applications/{id}/emojis` call, no `assets/` directory. Every state emoji is a literal:
  `dc/render/board_v2.rs:92-98` (counts), `:134` (`working_line`, ⚙️), `:168-173` (`done_line`: ❌ ⛔ ⌛ ✅), `:221-233`
  (picker), `:253, :273, :288` (sections); `dc/render/snapshot.rs:79-115, 182-187`; `dc/render/task_view.rs:55-68`
  (`state_words`); `dc/render/card_v2.rs:55-57` (✅/🔴). The fake knows `/applications/@me` only
  (`crates/theseus-sim/src/fake_discord.rs:2113-2118`). After Ready: `Event::Ready` at `dc/runtime.rs:502`.
- **Digest:** `/digest` is ephemeral and on demand (`dc/runtime/digest.rs:71, 126, 179-195`; "Nothing is posted on its
  own", `:17`). The RPC: `work.digest` (`theseus-protocol/src/lib.rs:295`), `WorkDigestParams { since, since_ms, roots }`
  and `WorkDigestResult { from, to, lines, from_ledger, now, questions }` (`theseus-protocol/src/work_digest.rs:69-104`),
  words via `digest_words` (`:108`) and `digest_summary` (`:214`). **A morning note already posts** once a day in core:
  `crates/theseus-core/src/initiative/morning.rs` (`[initiative] morning_hour`, META `initiative.morning` `:23`, checks the
  pacer `:87-92`, posts kind `initiative` through the outbox `:150`), on the core driver's 500 ms tick
  (`harness.rs:118, 139-142`; via `due_and_routines` → `wake_home.rs:114-118`). horizon-discord's `/agenda` is on demand
  only (`dc/runtime/horizon_cmds.rs:623-629`).
- **The shared pacer exists** (the owner's decision 4, built by theseus-j7cje): `crates/theseus-core/src/pacer.rs`,
  `UNPROMPTED = ["needs_you", "initiative"]` (`:22`; its doc names the digest as a future joiner, `:3-4, :14-15`),
  `Pacer::open(now_ms, window_ms)` (`:33`), `took` (`:39`), held as `Outbox::pacer` (`outbox.rs:226`, recorded `:384-385,
  647-648`), window `[notify] unprompted_pace_secs` (1,800 s, `config/notify.rs:28, 38`).
- **The outage:** `deliver_posts` stops at the first error (`dc/courier.rs:556-584`); `away()` backs off to 60 s
  (`:527-550, 57`); resends dedupe by nonce (`:12-15, 55`); kinds are planned in `plan()` (`:752`; `needs_you` `:864`,
  `initiative` `:861`); silent creates carry `SUPPRESS_NOTIFICATIONS` (`dc/policy.rs:36, 121`; `courier.rs:1596, 1814`).
  The fake can be `Down` (`Mode { Up, Down, HangCreates, Fail }`, `fake_discord.rs:85-100`, `set_mode` `:625`).
- **Config:** `DiscordConfig` (`crates/theseus-core/src/config.rs:273-323`, `deny_unknown_fields`); the template's
  `[discord]` (`crates/theseus-core/config/theseus.example.toml:752-787`); `example_template_mentions_every_key`
  (`config/tests.rs:1180`). discord-task-channels adds its four keys after `live_board`: add yours after `status_line`.

## What to build
1. **App emojis** (`dc/runtime/emoji.rs`; art in `crates/theseus-discord/assets/emoji/`): six small images made by a
   script in the repo (simple generated shapes, the standard library only, or committed output with the script beside it:
   an animated spinner for working, a pulsing dot for needs you, a check for done, a cross for failed, a moon for
   sleeping, a gear for a job; animated ones as GIF ≤ 256 KB, 128×128). Uploaded once after Ready when missing
   (`GET/POST /applications/{id}/emojis`), their ids in META (`discord.app_emojis`), re-uploaded if deleted; every site
   above calls **one function** (`emoji::state(WorkState) -> String` and one for a job), **Unicode when absent**
   (`⚙️ 🔴 ✅ ❌ 💤 🔧`, and today's ⛔ ⌛ kept for cancelled/expired). Off by `[discord] app_emojis = false`. Goldens
   that render state emojis must stay byte-identical with emojis off (they are the fallback).
2. **The digest post**: at each `[discord] digest_at` slot (default `["08:00", "18:00"]`, local; `[]` off), one message to
   the owner's DM from `work.digest {since_ms: the last digest}` when it has lines, worded as `/digest` words it; nothing
   when nothing happened. A core module beside `initiative/morning.rs` (the same shape: claim the slot in memory, mark it
   in META `discord.digest.<date>.<slot>`, post kind `digest` through the outbox), called from the driver's existing tick
   (`harness.rs:139-142`): no new timer. Add `digest` to `UNPROMPTED`; when `Pacer::open` says no (the morning note just
   went), the slot waits and posts when the window opens, within the same slot's day; at most once per slot across
   restarts. The courier plans kind `digest` like `initiative` (`courier.rs:861`). The board re-posts below it as it does
   for any post.
3. **The catch-up** (`dc/courier/catchup.rs`): when the courier comes back from `away()` and a place has three or more
   waiting posts that would be silent (`policy::pings` says no ping) and were written during the outage, they go as one
   message ("While Discord was unreachable (14:02–14:19): 2 tasks reported, 1 failed: …"), each folded post settled with
   that message's id so none is sent twice (and a crash mid-fold resends by nonce, not twice). **Never folded:** a card
   (`card`, `needs_you`: questions always post on their own), a settle, a reply to the owner's own message, and a reply
   whose `answer` is owed (`Lane::owed`, AGENTS.md "Invariants").

**FAST:** the emoji upload runs after Ready, off every path that matters (the lifecycle bench's rule: nothing new before
Ready); the digest is one read at its slot; the fold is a plan step over posts the outbox already holds.

**The fake** (`crates/theseus-sim/src/fake_discord/emojis.rs`, new; fake_discord.rs is 2,406 of 2,500 lines, at most 3
router lines): `GET/POST/DELETE /applications/{id}/emojis[/{emoji}]`, a size check (≤ 256 KB), and a flag to fail the
route.

**Tests** (the fake Discord rig): emojis uploaded once, reused after a restart, re-uploaded when deleted, Unicode when the
route fails; the digest at a slot with lines posts once and never twice across a restart, nothing with no lines, and waits
behind a just-written morning note (paused clock, or explicit `now_ms` into a pure `due(now, slots, last)` fn); the fake's
`Down` then `Up` folds four silent posts into one message, each settled, while a card during the outage posts on its own.
Plant a revert per behaviour (a card folded; the digest posted with no lines; the slot posted twice) and record what
caught it.

**Live check for the maintainer:** the Discord rig: emojis on the board, a digest slot set to a minute ahead, an outage
made with the fake's `Down`. **Write the owner's check for the maintainer** (the emojis in the test channel).

**Limits for this row.** No store change (META keys). No new model tool, no model-visible text. `dc/runtime.rs` has 13
lines of room (3,487 of 3,500): the upload after Ready is one call there; nothing else. Every new key has a default and is
in the template. The owner's decisions of 2026-10-13 hold ("yes to all until we're at testing time"): one pacing rule for
unprompted posts (the shared pacer above); Discord threads have one owner (`guild_thread.rs`'s helper; you make none).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant (the owner's next step is a trim review).
- No new model tool and no always-loaded text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn
  bench and the owner-config fixture test must pass). FAST: nothing new on a plain turn (a plain turn stays 5 frames).
- Every behaviour gets a test and a planted revert (break it, see the named test fail, restore); list them in the report.
- No store format change unless unavoidable (then take the next free number and say so). Never loosen a test, budget
  or ceiling. No person's name anywhere in the repo.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud.
