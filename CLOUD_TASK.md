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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261013-prefix-library`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: prefix-library, T13: the prefix library: canonical layered headers (core, persona set, project/topic) stored by digest, chosen by Jev at compile points, reused verbatim across threads and sessions, with a false-fit guard (theseus-rr15)

Branch: `cloud/20261013-prefix-library`. Every commit's subject carries `theseus-rr15`. Deadline for the report: 6 hours after you start.

**Main, as you clone it.** You clone main at `e8444d1c` or later. The preamble above is older and some of its facts
are stale; where they differ, this section wins. Read `git log --oneline -30` first: rows from the same evening
(the work board, the self-improvement machinery, the thread spine) may have joined since this was written, and you
build on what is there. **The store's format is 32** (`MANIFEST_FORMAT`, crates/theseus-store/src/store.rs) at
writing. A new stored record kind or field is a bump: by one from the number you clone, with the old layout's literal
sample in `tests_layouts.rs` and the two goldens (`core_output.txt`, `kernel_frames.txt`); others bump too and the
maintainer renumbers at the join. A new `LedgerKind` is not a bump.

**Line ceilings** (scripts/long-files.txt; never raise one: the keel guard fails it): protocol `lib.rs` 2,647 of
2,733; CLI `render.rs` 3,016 of 3,128; core `turn.rs` 3,496 of 3,525; `compiler.rs` 2,559 of 2,560; Discord
`runtime.rs` 3,476 of 3,500; Discord `render.rs` 2,886 of 3,009. A Rust file the list doesn't name fails past 2,500
lines, and **`toolrun.rs` is at 2,499**. Put new code in modules of its own and add only a `mod` line, a field or a
call to a long file.

**The keel guard runs first in every gate.** It fails a branch that raises a ceiling, removes a test or lowers an
assertion count, adds an `#[allow]` or `#[expect]`, or raises a bench budget or a spend or loop cap, unless a commit
signed by the owner's key acks it, and you can't sign. Split rather than raise; rewrite an assertion rather than
delete it. Never edit scripts/gate.sh, scripts/keel-guard.py or scripts/keel-signers. List anything you could not
avoid under **Keel findings expected** in the report.

**For every row:** no person's name anywhere (the owner is "the owner"); no secret in code, tests or the report; every
new config key has a default and a line in `theseusd example-config`'s template, with its template test; tests with
planted reverts (plant the bug, show the test fail, restore it and `touch` the file, and record what caught it);
**FAST**: a plain turn's frames are unchanged (the turn bench's 5 and 9), no new timer on a turn's path, and
`theseus-sim bench lifecycle --runs 10 --check` passes (record it); **minimal prompting**: the levers in order are the
API's contract, the tool's shape, its description, its result's metadata, and only then one factual prompt sentence,
never examples or "you MUST"; **improve, never max**: nothing tuned to a benchmark or a probe's wording; **doing more,
filing less** (the owner, 2026-10-10): fix a defect you find in code you are already in, with a test, and name it in
the report.

**Why threads exist.** The owner (2026-10-09; the bracket generalised): "If you note my conversations with you [and
my other agents], you'll notice I separate out concerns and keep them separate. This is /exactly/ the pattern Theseus
is supposed to solve. I want the streams of different clearly separable paths of thought … to cleanly be addressible
to theseus with dynamic contexting smoothly causing each activity thread to not unnecessarily burden any one
context." He splits by hand today (a Discord channel per concern), and the DM where the rest lands grows toward a
million tokens; more than half his DM messages arrive to a cold cache, and a cold message's price scales with the
context it rewrites (about $7.85 at 626K tokens on Fable, about $0.50 at 40K). The decisions he took (2026-10-09
11:04): a thread is a durable record bound to a virtual place; a new thread is offered with one tap, never made
silently; route ≥ 0.90 with a quiet tag, 0.60 to 0.90 with a move handle, below that ask in one line; ambient
cross-thread recall only above a higher bar and always naming its thread; sealed threads (Jev recommends, the owner
seals); one Discord thread per activity thread; persona engagement is a per-message Jev verdict in any thread.

**The thread spine (rows of the same day; check `git log` for what has joined, and build on it):**
- **T1 `thread-record`** (theseus-nhew): `theseus_protocol::threads::ThreadInfo {slug, title, goal, topics (≤ 3),
  working_set {roots, repos, branches, refs, files, hosts}, card (≤ 12 lines), persona, state Active|Paused|Done,
  sealed, seal_pending, surfaces, parent, merged_into, split_from, session_id, last_active_ms, created_at_ms}`; records
  as META keys `thread:<slug>`, append-only versions, held in an in-memory `Board` (`Core::threads()`,
  `Board::of_session`, `Board::live(place_class)`); the virtual place `thread:<slug>`, always private; `turn.submit
  {thread?}` and `--thread`; `thread.new|list|show|edit|pause|resume|done|reopen`, one frame and one `thread.*` ledger
  row each; CLI `crates/theseus/src/thread.rs`; Discord `/thread` in `runtime/thread.rs`; `[threads]` in
  `config/threads.rs` (each row adds its own keys there). T1 is the only format bump among the thread rows.
- **T2 `thread-context`** (h4a7): the layered request in `compiler/layers.rs` (layer 1 header + standing, layer 2
  every persona's files, layer 3 system context files + topic guidance, layer 4 messages, on the provider's four
  breakpoints), the card snapshot at the head of the messages (refreshed only at a recompile), `thread.note`, the
  cold-resume recompile under `[threads] working_tokens`.
- **T3 `thread-recall`** (f2g0): thread provenance headers, the distance penalty and `cross_recall_bar`,
  `thread.read`, sealed threads with `thread.recommend_seal` and the Seal/Not needed card (`runtime/seal.rs`).
- **T4 `thread-route`** (rha2): addressing (place, explicit, reply), the pre-router (`core/thread_route.rs`),
  `classify.v2`'s `thread` question, the reply tag (`→ pr round`), the ask card (`runtime/thread_ask.rs`),
  `route_wait_ms`, the misroute write guard, `/move` with `Steer::Thread`.

A thread is private always: a shared place never binds, addresses, lists or sees one. A plain turn with no thread
keeps its frames and reads no store on its path.

**The owner's idea (2026-10-09 10:49 and 10:53):** a library of canonical, layered prompt headers, about 10 MB,
persisted to disk and loaded at start, keyed by an embedding; Jev picks the stored header that best fits a request
("the security persona plus a bh-ai bug") and the request reuses it **verbatim**; what is missing goes as a delta
after the cached prefix, never inside it; some surplus context is tolerated. The saving is the provider's prompt
cache: an exact byte-prefix match per model, alive 5 minutes or an hour after its last read. So the library's real
job is to make the bytes identical across threads and sessions that work the same project.

**Your base: main after `thread-context` (T2) has joined** (its four layers in `compiler/layers.rs`: layer 1 header +
standing, layer 2 every persona's files, layer 3 system context files + topic guidance, layer 4 messages; each layer's
digest in the manifest). Read it whole, then `config/cache.rs` and the keep-warm worker (cache-fix), `routing.rs`'s
cold estimate (`cold_switch_tokens`), the judge's pack machinery (`pack.rs` `options_from`, and route-persona's
`Source::Personas` if it has joined: yours is a sibling), the embedding the index already computes (`theseus-index`),
and `provider.rs`'s usage fields (cache read and creation tokens).

**What to build:**
1. **Canonical layers.** Layers 1 to 3 render with nothing per request inside (no date, session id, place name, turn
   count): prove it with a byte-identity test (two sessions, two threads and two places on one project render
   identical layers 1 to 3). Anything per request moves to layer 4.
2. **The library store**: headers by layer digest (core, persona set, project/topic stored apart and combined, so one
   project header serves every persona and thread), each with its embedding, per-model warmth (last read, TTL), and a
   hit log; a store record (**a format bump**: follow the rule), loaded at start into memory, under
   `[prefix] max_bytes` (10 MB) with least-recently-read eviction; a header whose source changed (a context file, a
   guidance section, a persona file, standing) is re-rendered under a new digest, and the old one retires once unread.
   A header is never edited in place.
3. **The pick, only at compile points** (a thread's birth, a cold resume, `/new`, a forced recompile; never under a
   warm session): candidates are the k nearest headers by embedding of the new message, the thread card and the
   working-set entities, filtered to the place class (a shared place reuses only headers built for shared places). A
   `prefix` question (a pack of its own at the compile point, `options_from = "headers"`, act 0.90, confirm 0.60)
   scores fit; then **reuse** the best verbatim with a delta in layer 4, or **reuse the nearest coarser** (the
   project's without the topic) with a bigger delta, or **mint** a new header only when the best fit is below confirm
   and `p(reuse within TTL) × reads × (write − read price) > write premium`, with `p(reuse)` from the hit log.
4. **Warmth.** Routing's cold estimate counts a warm library header as cached (per model). Keep-warm refreshes the top
   few headers by recent reads on the models in use, with a minimal request against the header alone, instead of one
   per session.
5. **The false-fit guard.** Every reuse logs its fit score, choice, cache hit and the turn's outcome (a correction, a
   re-ask, a misroute); a turn that went wrong after a reuse below act is a labelled false fit for Jev's learning
   report. `theseus prefix report`: reuse and mint rates, cache writes saved, false fits with their scores.
6. **Off switch:** `[prefix] enabled` (default true once the tests pass; false renders each session's own layers, the
   threadbench's arm A1n).

**FAST:** the pick runs only at compile points; a warm turn adds no frame, no timer, no store read (`theseus-sim bench
turn`); the lifecycle bench unchanged; the library's resident size reported against its 10 MB.

**Tests:** compiler goldens for the three-block request; the byte-identity test; reuse, coarser and mint decisions on
planted fits and hit logs; mint refused when reuse doesn't pay; shared/private never cross; re-render on a source
change; eviction at the cap; the false-fit log. Plant a revert per behaviour (a date inside layer 3; a private header
reused in a shared place) and record what caught it.

**Live check on this VM:** a scratch daemon on the stand-in with the token mirror: a second thread on a warm project
shows a cache read for layers 1 to 3 on its first turn in the ledger; a near-miss project (same layout, other test
command) is not reused. Write the maintainer's check on Sonnet 5.5.

**Model:** the reuse/mint economics and the false-fit guard are design work on the cost path; write the decision rule
and its numbers in the report before the code.

Run the gate before every commit; commit CLOUD_REPORT.md as the branch's last commit, and end with a short final
message starting `CLOUD REPORT COMPLETE`.

## The owner's decisions (2026-10-13 23:24 MST, verbatim: "i'm happy for yes to all until we're at testing time to eval")
They hold until the evaluation pass; build so each is easy to revisit.
1. **One conversation surface.** Bare `theseus` opens the chat (row `chat`, theseus-cyha, owns it); `theseus chat` is the same thing; the task footer and one-key answers (row `cli-tasks`) live inside that chat, not in a second CLI.
2. **One plan.** Plan mode's steps are the work board's plan items (row `progress`), never a second list of steps.
3. **One owner for Discord threads.** Activity threads (row `thread-discord`, T6) decide which Discord thread a task or message lands in; task rows (`discord-guild-tasks` and others) use its helper and never create threads on their own.
4. **One pacing rule for unprompted posts** (the digest, the morning/horizon view, nudges, the always-on DM ping, proactive voice). Every unprompted post goes through one shared pacer: at most one unprompted DM post per 30 minutes, later items batched into it; the first such row to join builds the pacer (a small module beside the push), later rows use it.
5. The size check joins the gate (a build fails if the binary grows more than 1 MB; the 60 MB miss warns). 6. Programs at once 8 -> 16 (reads 32). 7. Scoped grants are built on Opus with a security review.
