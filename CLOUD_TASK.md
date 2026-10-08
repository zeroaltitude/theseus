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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261008-session-states`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: session states (live, quiet, retired), supersession, and the Ship's state filter (theseus-emqx)

Branch: `cloud/20261008-session-states`. Every commit's subject carries `theseus-emqx`. Deadline for the report: 4
hours after you start.

**Main has moved since the preamble was written:** the store's format on main is now **24**, not 23. If you add a
field to a stored record (this task does), bump `MANIFEST_FORMAT` (crates/theseus-store/src/store.rs) to 25 and add the
old layout's literal sample to theseus-core's tests_layouts.rs, as AGENTS.md's store version rule says.

**Why:** the Ship (the cockpit's landing view, `cockpit/src/views/Ship.tsx`) draws every session the daemon holds as a
vessel, including ones the owner considers dead: a direct-message session still titled "Read /etc/hostname" that the
direct message moved off days ago, a 0-turn session a channel's bind opened and never used, a 1-turn blank web
session. Sessions only accumulate, and the protocol has no way to say one is done. The owner approved this framing
(2026-10-08), with one change of their own: "we can have ui elements in the ship to show all of those states,
defaulting to live (active within the last 24 hours)".

**The states, as approved.** Derived when read, from what the record and the store hold:
- **Live:** a turn within the last 24 hours. The owner's definition is time-based, not "a binding points at it". The
  window is a config key with a default of 24 hours (below). My recommendation, which you may change if you say why: a
  session whose execution is running, or that waits on the owner (`attention`, `pending_confirms`), also reads Live
  whatever its age, so nothing that needs the owner is ever filtered out of the default view.
- **Quiet:** not retired, and no turn within the window.
- **Retired:** one of three reasons, kept on the session so the UI can say which:
  - **superseded:** a place's binding moved to a newer session (a Discord `/new`, or the automatic rebind when a
    place's session can no longer take turns);
  - **empty:** zero turns. Existing 0-turn sessions read as retired. A session opened a moment ago and not yet used
    must not flash "retired": give empties a short grace (a config key with a default, e.g. one hour) or rely on
    point 4 below making new empties impossible; say which;
  - **retired by hand**, through the new RPC.

  **Never delete anything.** A retired session stays in the store, keeps its nodes, its memory and its index entries,
  stays searchable and readable, and can be reopened. Imported sessions stay as they are (session.list already leaves
  them out; don't change that).

**What to build:**

1. **The state on each session.** In `SessionInfo` (crates/theseus-protocol/src/lib.rs; built by `SessionRecord::info`
   in crates/theseus-core/src/session.rs and `session_info` in crates/theseus-core/src/rpc/methods.rs): `state`
   (`live` | `quiet` | `retired`), the retired reason and when, `superseded_by` on the old session, `supersedes` on the
   successor, and the title history (point 5). `last_active_ms` is already there. Every way `session.list` answers
   (all sessions, `ids`, the `n`/`before` page) carries them. **The default answer stays every session**: the CLI
   (crates/theseus/src/cmd.rs, interactive.rs, herdr_sync.rs), the TUI (crates/theseus-tui/src/app.rs) and the
   Discord runtime call `session.list` with no parameters and must see what they see today. An optional `state`
   filter parameter is fine if you need it; additive only. Give clients the window too (e.g. `live_window_ms` on the
   result), so the cockpit's time machine can derive the same states for a past moment.

2. **Supersession, recorded both ways.** Find every place a binding moves:
   - crates/theseus-discord/src/runtime.rs: `rebind()` (about line 2285), called by the `/new` control
     (`Control::New`) and by the automatic rebind when a place's session can't take turns ("so I opened a new one",
     about line 1986); and `session_for()` (about line 1052), which opens a new session when the stored one is gone or
     terminal;
   - the place record: `Outbox::bind_place` / `place_session` in crates/theseus-core/src/outbox.rs (a meta key per
     place).

   Record old -> new on both session records, in **one store frame** with the place record's move, so a crash can
   never leave half of it. Add a ledger row for it (an additive kind beside `session.opened` in
   crates/theseus-protocol/src/ledger.rs), and rows for retire and reopen, so the cockpit's time machine
   (cockpit/src/lib/timemachine.ts, which folds `session.opened` today) can show states in the past too.
   runtime.rs is at 3,467 of its 3,500-line ceiling: put the new logic in a module under
   crates/theseus-discord/src/runtime/ and add only the calls to runtime.rs.

3. **Retire and reopen RPCs** (additive protocol methods, e.g. `session.retire` and `session.reopen`, method names in
   lib.rs's list, types in a module of their own). The owner's surfaces only: the cockpit and the CLI. A Theseus job
   inside a session must not be able to retire or reopen sessions; reuse the refusal `theseus judge label` uses inside
   a job, and test it. Decide and test what retiring a session that a place is still bound to does (my
   recommendation: the place's next message starts a successor, recorded as a supersession, so the place never posts
   into a retired session silently), and what reopening a superseded session does (my recommendation: it clears the
   retired state only; the place stays on its successor; both "replaced by" links stay as history). Add retire and
   reopen to the CLI's `theseus sessions` command (crates/theseus/src/main.rs and cmd.rs), and show each session's
   state in its list.

4. **No session before its first turn.** Stop empties at the source:
   - Discord: a place's bind (`session_for` at start) and `/new` must not open a session; the place's first message
     opens it (and records the supersession then, so `superseded_by` always names a real session). The bind notice
     (`bind_notice`) and the `/new` answer must still read right (e.g. "a fresh session starts with your next
     message"). Mind the outbox's targets index (`bind_place`), which routes a session's posts to its place.
   - The cockpit's "New session" button (`NewSession` in cockpit/src/views/Fleet.tsx calls `session.open` at once):
     open a draft deck and call `session.open` with the first submit (cockpit/src/components/Composer.tsx,
     PromptPicker.tsx).
   - `session.open` itself keeps its contract for other clients (the CLI, the TUI, MCP clients): a session they open
     and never use reads as retired (empty) after the grace.

5. **Re-title after the first few turns.** Today a session's title is the first line of its first prompt, set once
   (`title_from` in session.rs; the call sites in crates/theseus-core/src/turn.rs about line 1652 and
   crates/theseus-core/src/turn/prompt_input.rs about line 36; `take_turns_fields` keeps the first one), so a test
   probe names a session for good. After the Nth turn (a config key with a default, e.g. 3; 0 turns it off), give the
   session a better title, and keep the old one visible (a `title_was` list or `first_title` on the record and in
   `SessionInfo`; the deck shows "was: ...", and the palette matches both). Choose how: a deterministic pick (the first
   prompt with substance among the first N, skipping short probes and bare commands) or a short model title made **off
   the turn path**, after the turn has ended, with the cheapest configured profile, its cost booked to the session like
   any provider call; say which and why. Never re-title a task session (task.rs sets its title from the brief), never
   touch `label` (the owner's), and never re-title twice unless the owner asks. turn.rs is at its line ceiling (3,525):
   put the logic in a module beside it (crates/theseus-core/src/turn/) and add only the call, or move the existing
   title lines out so turn.rs does not grow.

6. **Config.** A `[sessions]` section (or the nearest existing home under crates/theseus-core/src/config/): the live
   window (default 24 hours), the empty grace, the re-title turn. Every key has a default; add the section to
   `theseusd example-config`'s template with its template tests, as AGENTS.md says. The owner's config holds only
   diffs, so it needs no change.

7. **The Ship (cockpit), beautiful in its nautical idiom.** cockpit/src/views/Ship.tsx, cockpit/src/ship/ (model.ts
   builds vessels from `session.list` via useShipData.ts; labels.ts, HoverCard.tsx, Key.tsx, Minimap.tsx, Watch.tsx,
   synth.ts), cockpit/src/views/SessionDeck.tsx, and the ⌘K palette in cockpit/src/components/Shell.tsx (its
   `Command.Dialog` reads `session.list` itself, about line 365):
   - **A state control with counts:** Live · Quiet · Retired · All, each with its count, **default Live**, the choice
     kept in the URL (a new search parameter beside `s`, `n`, `b`, `call`, `scale`, `swell`, `calm`; a link with no
     parameter shows Live).
   - **State badges** on vessels (the label and the hover card), in the session deck's header, and in the Fleet view's
     table (cockpit/src/views/Fleet.tsx).
   - **"Replaced by <session> on <date>"** on a superseded session and **"Replaces <session>"** on its successor, both
     links, in the hover card and the deck.
   - **Retire and Reopen** in the session deck (and the hover card if it fits), with the reason shown on a retired
     session.
   - **No data unreachable** (the owner's standing rule for the cockpit: nothing the cockpit showed before may become
     unreachable): the ⌘K palette finds every session whatever the filter, with its badge, the retired ones ranked after
     the others; selecting a hidden session (`?s=<id>`, the palette, a "replaced by" link, a watch plate, a flare)
     shows it, with the filter widened or the vessel shown as a visitor, never a dead selection. The watch's plates
     and flares stay unfiltered. The fleet's stats say what the filter hides.
   - **The idiom:** e.g. live ships at sea under sail, quiet ships at anchor in the roads (sails furled, lamps low),
     retired ships laid up in harbour (when Quiet, Retired or All is shown), a superseded ship flying a signal toward
     its successor. Your design; make it carry information and make it lovely. Vessels keep their slots when the filter
     changes (model.ts's positions are deterministic and stable): switching the filter must not reshuffle the sea.
   - **The time machine:** in a past moment the states derive from the fold with the same window.
   - **The synthetic fleet** (synth.ts, `?synthetic=1`) gains sessions in every state and reason, so the control can be
     shown and measured without a daemon.
   - **Screenshots:** if a headless Chromium is available on this VM (or installable from the system's package manager
     without touching package-lock.json), take screenshots of the Ship at Live, Quiet, Retired and All, a retired
     session's hover card and deck with its "replaced by" link, and the palette finding a retired session, from the
     synthetic fleet; commit them under `CLOUD_SHOTS/` in the report's commit (dropped at the merge), and list them in
     the report. If no browser can run here, say so: the maintainer takes them.

**Tests and planted reverts, for each:**
- the derived state at the window's edges (just inside, just outside), the running-or-waiting rule, the empty grace;
- supersession written both ways in one frame, on `/new` and on the automatic rebind, and after a restart (the
  records, the place record and the ledger rows agree); a crash between "place moved" and "records written" is
  impossible by construction (one frame): show the frame;
- `/new` and a fresh bind open no session; the first message opens one and records the supersession;
- retire and reopen: the owner can, a job can't; a retired bound place's next message makes a successor; reopen keeps
  the history;
- re-title at the Nth turn, the old title kept and listed, tasks and labels untouched, no second re-title;
- `session.list` with no parameters returns every session as before (count and order), so the CLI, TUI and Discord
  readers are unchanged;
- the cockpit (node's test runner, cockpit/test/): the filter's counts, the URL round trip, the default, a hidden
  selection revealed, the palette listing every session;
- the store format bump with the old layout's sample (a record written at format 24 reads as before, with no state
  fields and derived states that are right).
For each, plant the bug, show the test fail, restore and `touch` (the preamble's recipe).

**FAST:** `session.list` must be no slower. Measure it before and after on a store with many sessions (the existing
timed test `the_session_lists_past_a_full_import_timed` in crates/theseus-core/src/rpc/tests_imported.rs is a model;
give numbers for the full list and a page), and the turn bench's frames for a turn before and after (re-titling and
supersession add nothing to the turn's path). The state is derived from fields the record already holds or that the
frames above write; no extra store read per session. While you are there: `session_info` in the Discord runtime
(runtime.rs about line 1105) reads the whole `session.list` to find one session; `ids` exists for that. Fix it only if
it is a one-line change inside your module's calls, and say so.

**Read first:** the root AGENTS.md, crates/theseus-core/AGENTS.md, cockpit/AGENTS.md and cockpit/CLAUDE.md,
crates/theseus-discord's AGENTS.md if it has one; then session.rs, rpc/methods.rs (`session_open`,
`open_session_as`, `session_list_of`, `session_page`, `session_info`), outbox.rs (`bind_place`, `place_session`), the
Discord runtime's `session_for`, `open_session`, `rebind`, and the `/new` control, and the cockpit files named above.
Files near their ceilings: crates/theseus-protocol/src/lib.rs is at its ceiling (2,719): put new types in a module of
their own and add only the `mod` line and method names (or move `SessionInfo` and `SessionListResult` to a module of
their own and re-export them, as history.rs did); turn.rs and runtime.rs as said above. Check scripts/long-files.txt
before adding to any file.

**Not in this task:** deleting anything; changing a retired session's memory or index entries; the TUI's and the CLI's
session pickers beyond keeping them working (say in the report what they could show).

The issue: `theseus-emqx`.
