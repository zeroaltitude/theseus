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

**What main holds.** You clone main at 57f265f2 or later, with **store format 23**. Your clone has v1's milestones, and:
- **batch 8's and batch 9's joins (2026-10-05 and 06):** timing-flakes, telemetry3, cli-tests, history-pages, turn-stack, queue-frames, smalls, wal-mark-skip, crash-hold, durability-on, learning-fixes and discord-live; **soul-import (79be3213): `theseus import openclaw|list|erase`, imported sessions with their provenance, erase by tag, recall's provenance; store format 23**; the bench stack, voice-turns, scrub-escaped, gate-tests, approvals-batch, judge-tests and judge-reads, memory-tests and telemetry-tests; core-waits, route-tests and daemon-proofs (d279767f: the core golden's wake and its offset's sign masked, so it passes in any zone; route's tests proof against load; a `--stdio` daemon's stop that drops its runtime; the lifecycle bench's `cancel` row; the flaky list emptied);
- **batch 10's joins and the local lanes (2026-10-06):**
  - voice-echo and voice-heard (f589d9cb, 4db4cfc0): in a voice call an echo and a closing "yes" are told from a real answer; the next voice turn is told what was heard, cut and never said; replies shaped for speech; a failed voice turn said aloud (theseus-voice engine.rs, heard.rs; theseus-discord runtime/voice/);
  - the docs lane v0.83 (ceba1520, docs only) and the names lane (ea34457e): no person's name in the tree (below);
  - bench-pi (cacad5c4): Pi as the benchmarks' fourth arm, Harbor's own Pi pinned at 1.0.4, measured as the other arms are (bench/harbor/pi_agent.py, pi_atif.py, efficiency.py; the report, recall and async benches);
  - imported-skip (5ac5c23c): the session lists read live sessions by key and never decode an imported record, and a page steps over the import's births in one walk (core store.rs's `live_sessions`, rpc/pages.rs, rpc/methods.rs; theseus-store index.rs);
  - the bench-reports lane (351680a1): docs/benchmarks/ holds a report for every benchmark run, with bench/report's charts, statistics and drafting tool (bench/report/charts.py, stats.py, draft.py);
  - judge-sink with judge-turn-cost (938a8dc8): the judge's sink writes its frames only between turns, with one clock a backlog pass and a busy-bound guard (480 s), a clean stop writes every settled judgment before the store closes, no judged point reads the ladder or the lineage (a warm read does), and `theseus-sim bench turn --judge` measures the judge's cost on a turn (core judge/sink.rs, judge/mod.rs, judge/ladder/, memory_pass/turns.rs's between-turns guard; theseus-sim perf/judge.rs).
  - learned-shadow (57f265f2): `pack.promote` warns while a learned version stands in the moved version's place, the prove's window names it, and the learning cut's mark is never ahead of the newest judgment it read (core rpc/packs.rs, rpc/packs_ahead.rs, rpc/learning.rs, rpc/judge_prove.rs, learning/system.rs).

**Other changes in flight.** Batch 10's other branches are reviewed and merged into `main` one at a time on the owner's machine while you work; a few are still being built. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.
- flake-causes: five load flakes at their causes (core term/tests.rs, tests_push.rs, push.rs, outbound.rs, rpc/server.rs, learning/tender.rs; theseus-kernel children.rs and its test; theseus-aws-catalog tests/catalog.rs and Cargo.toml);
- scrub-encodings: twice-escaped JSON, YAML and repr escapes, base64 inside an escaped string (core scrub.rs, scrub/);
- discord-bound and discord-tests: a lane and the renderer forget a dropped turn's messages; the outbox's load flakes, a binding's event loop ended at the stop (theseus-discord courier/, render/, runtime.rs, tests_outbox.rs; core outbox.rs);
- discord-watch: the bindings file's watch retries a failed bind and keeps the start's note (theseus-discord runtime/live.rs, tests_live.rs);
- core-gaps: a failed routed turn counted where it ran, four test gaps (core rpc/methods.rs's `count_failed_turn`, tests_route_keep.rs, tests_route_model.rs, tests_m3.rs, tests_audit.rs, tests_stack.rs);
- budget-upper: a turn's own call reserved on the estimate's upper bound (core turn.rs's reservation lines, fact/turn.rs, recall/adjacency.rs, rpc/mod.rs, the core golden);
- bench-bounds: bench/'s sampler bounds and the recall bench's retraction and overhead checks; it moves the recall plan's default overhead to 13,640 and repins the smoke's digest (bench/harbor/sampler.py and its test, bench/recall/);
- cancel-fast: `execution.cancel`'s waits event-driven and its frames fewer (theseus-kernel job.rs, tree.rs; core cancel.rs, rpc/driver.rs, toolrun/job.rs, toolrun/late.rs; theseus-sim lifecycle.rs's cancel row; theseusd tests/cancel_frames.rs);
- daemon-stops: a `--stdio` daemon stops on the `shutdown` method, a restart in place drops its runtime first, an import or erase stops between frames (theseusd main.rs, stdio.rs; core rpc/import.rs, import/write.rs; theseus-store store.rs's `upgrade_manifest`);
- route-wait: route.v1's verdict without a wait on every message, and the judge's connection warmed before the first (core judge/inbound.rs, a new judge/warm.rs, tests_route*.rs; theseus-judge fake.rs);
- voice-holds and voice-dave: the voice hold and floor bounded; a deaf voice call noticed and rejoined (theseus-voice engine.rs, heard.rs, vad.rs, tests/turns.rs, songbird_io.rs; theseus-discord runtime/voice/; core voice.rs, telemetry; protocol voice.rs, ledger.rs);
- a local lane reworking the cockpit's ship and charts (cockpit/src/ship/, the Economics, SessionDeck, Ship and Speed views).

These are other cloud sessions like you, batch 11, each on its own branch:
- aws-mints: the AWS catalog's three missing credential mints made secret-bearing, and a rule over every operation's output shape that catches the next one (theseus-ye7o; theseus-aws-catalog tables.rs, a new test file and the generator's doc; theseus-core aws/secret.rs and a new aws/ test file);
- sink-fast: the judge sink's staged blobs written before its between-turns guard with their syncs batched, categorize's mark and the budget's block off the turn, and a test of the busy-bound guard (theseus-ehkp; core judge/sink.rs, judge/mod.rs, blobs.rs, judge/categorize.rs, judge/spend.rs, outbox.rs's `flush_judgments`, new tests_sink_*.rs files);
- bench-fair: the benchmark's arms made comparable: reasoning effort medium on every arm, Claude Code pinned, a timed-out agent stopped, Pi's provider failures counted, Pi offline (theseus-n6p5; bench/harbor/, bench/report/, bench/async's tests, bench/README.md, bench/theseus-bench.toml, one assertion in theseusd's tests/bench_profile.rs);
- recall-fair: the recall bench's arms made comparable: Pi planned at its own overhead and offline, effort medium on every arm (theseus-a5we; bench/recall/; after bench-bounds joins);
- daemon-flakes: theseusd's two load reds at their causes (theseus-jtrc; theseusd tests/bench_profile.rs's first-byte test, tests/stops.rs, an accessor on tests/common/model.rs's `FakeModel`, and whatever each cause is);
- imported-walk: the session lists skip the import's key range without visiting it, and imported-skip's untested claims held (theseus-26jo; theseus-store index.rs, store.rs's `Store` trait and its WAL override, lib.rs; core store.rs's `live_sessions`, rpc/methods.rs's `compilation_list`, rpc/tests_imported.rs);
- health-imported: health counts the owner's own sessions apart from imported and erased ones (theseus-revl; core rpc/methods.rs's `health` and `session_totals`, protocol `HealthResult` with a type in import.rs and lib.rs's ceiling, the CLI's health line through a render/ helper, the cockpit's Systems view);
- bench-rows: the turn bench prints each run's wall and its slowest frame, and the lifecycle bench's restore row runs before the cancel row again (theseus-w7dk; theseus-sim perf.rs, lifecycle.rs's `run`; maybe theseus-store store.rs beside `frames_written_here` and turn.rs's trace attrs).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (23 on main today, since soul-import), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. No batch-11 task expects one: if yours needs it, say why. Others may bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,715) and crates/theseus-discord/src/render.rs (3,009); near it, crates/theseus/src/render.rs (3,099 of 3,100), crates/theseus-kernel/src/kernel.rs (3,025 of 3,030), crates/theseus-core/src/compiler.rs (2,546 of 2,560), crates/theseus-core/src/config.rs (2,887 of 2,910), crates/theseus-core/src/turn.rs (3,496 of 3,523), crates/theseus-discord/src/runtime.rs (3,428 of 3,500) and crates/theseus-core/src/tests_m3.rs (7,837 of 8,050). A Rust file the list doesn't name fails past 2,500 lines; near that today are theseus-core's telemetry/tests.rs (2,491) and toolrun.rs (2,418), theseus-sim's kernel_sim.rs (2,465), theseus-store's wal.rs (2,430) and theseus-kernel's tests.rs (2,407). Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report. Harbor 0.23.0 needs Python 3.12 or later: where python3 is older, run Harbor's tests in a venv (`python3.12 -m venv /tmp/hvenv && /tmp/hvenv/bin/pip install harbor==0.23.0`); bench/async's Harbor tests also need `ASYNC_HARBOR=1` there. Every benchmark run gets a report in docs/benchmarks/: the maintainer writes those, and no task here runs a benchmark.

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is unless your task names it). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261006-aws-mints`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: three AWS credential mints made secret-bearing, so their credentials reach the model only as handles, and a catalog rule that fails on the next mint a weekly update brings (theseus-ye7o)

Branch: `cloud/20261006-aws-mints`. Every commit's subject carries the id of the issue it fixes. Deadline for the
report: 4 hours after you start.

**Background.** Security. `aws.call` (theseus-core aws/tools.rs, about :302) treats a call as secret-bearing from its
classification (theseus-aws-catalog's `classify`, from the tables in tables.rs). For such a call, aws/secret.rs's
`hold` walks the output by its shape, puts each secret on the secrets board under an `aws-secret:` handle and masks
it; any other call's output goes to the model, the store and the ledger as JSON. The design (docs/design/aws-toolset.md
§3.1: "every STS credential mint" is secret-bearing) is kept by three tables, as the STS mints have them: `CLASS` (a
`MINT` row: Write, "whatever its name says"), `SECRET`, and `RETRY` ("a repeat mints another short-lived credential").
The catalog's models (aws-cli/2.34.15) hold three mints in none of them:
- sts `GetDelegatedAccessToken`: its output's `Credentials` are temporary AWS credentials; classed Read by its `Get`;
- sts `GetWebIdentityToken`: its `WebIdentityToken` is a signed JWT for outside OIDC services; classed Read;
- eks-auth `AssumeRoleForPodIdentity`: its `credentials` (`accessKeyId`, `secretAccessKey`, `sessionToken`); Write by
  its name, not secret-bearing.

**Read first:** AGENTS.md (FAST, the security rules); docs/design/aws-toolset.md §3.1 and §3.5; theseus-aws-catalog's
tables.rs (`CLASS`, `MINT`, `SECRET`, `RETRY`), classify.rs (`classify`; its test
`every_table_row_names_real_operations` fails a row that matches no operation), model.rs (`ShapeRef::is_sensitive`,
`members`, `list_member`, `map_value`; `OperationRef::output`; `Paginator::output_tokens`), tests/catalog.rs (`GOLDEN`:
the mints read `W 🔑`, `SafeToRepeat`), examples/theseus-aws-catalog-gen.rs (its doc: the weekly updater is "update
the CLI, run this, run the tests and the gate"); theseus-core's aws/secret.rs (`NAMED`, `walk`, `hold`, its test
`the_catalogs_secret_members_are_found_and_masked`), aws/tools.rs (the `p.secret` arm), aws/tests_handles.rs and
aws/tests.rs (`Fake`, `Seen::action`, `sts`).

**What the code says** (the code wins; report each difference):
- `walk` holds a member when its shape is marked sensitive or its name is exactly one of `NAMED` (case matters).
  `Credentials` is there; `credentials`, `secretAccessKey`, `sessionToken` and `WebIdentityToken` are not. A
  secret-bearing call whose walk holds nothing returns none of its output and says so (tools.rs): safe, but no handle.
  So unless the models mark those shapes sensitive, the rows alone make two of the three fail closed. Check each, and
  where the walk finds nothing, teach it the names (case-insensitive matching, or the names added): say which.
- The compiled catalog keeps every output shape whole: members, list members, map keys and values, the sensitive
  mark, and each paginator's output tokens. The rule in step 3 should need no generator change; if it does, say what.

**What to build,** each a green commit:
1. **The rows.** In `CLASS`, `GetDelegatedAccessToken` and `GetWebIdentityToken` as `Class::Write` with `MINT`, and
   one for eks-auth's mint too (sts `AssumeRole*` has one though its name already makes it Write: the note says why);
   all three in `SECRET`, and in `RETRY` as `SafeToRepeat`. If a name is missing from your clone's catalog, the table
   test says so: leave that row out and report it. Tests in a new file under theseus-aws-catalog's tests/ (e.g.
   `tests/mints.rs`): each operation's label is `W 🔑`, its retry `SafeToRepeat`, its note `MINT`'s. In secret.rs's
   `the_catalogs_secret_members_are_found_and_masked`, the three outputs as their models shape them, invented values:
   each secret held at its path, nothing of its value left in the body.
2. **Through the core.** A new test file beside aws/tests_handles.rs, using its helpers: the stand-in AWS answers each
   operation with its credential shape (STS speaks the query protocol: an XML answer, as `tests::sts` writes
   `GetCallerIdentity`'s; eks-auth is REST-JSON, found by its path); `aws.call` for each: the text and meta hold
   handles and never a value, and the board holds each value. A Write needs what the rig's other writes get: use theirs.
3. **The shape rule,** so the weekly update catches the next mint. A test over every operation of the built-in catalog
   that walks its output shape (with a seen-set: shapes recurse). Put it in theseus-aws-catalog (with `NAMED`'s names
   copied) or in theseus-core beside secret.rs, where it reads `NAMED` itself, and say which. A member is
   credential-shaped when it has:
   - a name that, case-insensitively, is `Credentials`, `SecretAccessKey`, `SessionToken` or one of `NAMED`'s;
   - or a shape the model marks sensitive and a name ending in `Token`, `Password`, `Secret`, `Key` or `Credentials`;
   - and it is not a paginator's output token (`NextToken`: some models mark it sensitive).

   An operation with one must be secret-bearing (`classify().secret`) or on an explicit allowlist in the test, each row
   with its reason (an idempotency or pagination token, a public key, a token that grants nothing); an allowlist row
   that matches no operation, or whose operation is now secret-bearing, fails too. Run it, and decide each operation
   it hits: a mint (it makes a new credential: `CLASS` with `MINT`, `SECRET`, `RETRY`), a stored secret read back
   (`SECRET` alone: secretsmanager `GetSecretValue` stays Read), or the allowlist. **List every hit and your decision
   in the report:** the owner reviews that list. Add a line to the generator's doc naming the rule. If the hits run to
   hundreds, tighten the rule (say how) before you allowlist.

**Plants,** each failing its test (quote the failure, then restore):
- each of the three `SECRET` rows removed: its row test and its core test fail;
- `GetDelegatedAccessToken`'s `CLASS` row removed: its label reads `R 🔑`;
- one new row removed from `SECRET`: the rule fails naming that operation;
- an allowlisted operation added to `SECRET`: the stale allowlist row fails;
- the names you taught `walk` removed: the core test fails (the call returns no output).

**FAST:** the rows are table lookups when a call is planned; nothing new on the turn path or the start path. Say so,
and give the rule's run time in a debug build (it decodes every service, as `every_service_decodes` does).

**Proof, offline:** the plants; theseus-aws-catalog's suite; theseus-core's `aws::` tests; every test across the
workspace whose name says secret, handle or redact; the gate.

**The live check is the maintainer's.** A real mint needs account changes (outbound identity federation, a pod
identity), so none is called. Exact commands for a scratch daemon with AWS bound (the maintainer has the keys) and the
stand-in model (`theseus-sim fake-model --rules`) calling `aws_describe` for each of the three: `theseus history
<session>` shows each Write and secret-bearing, with the mint's note; on main, the two STS ones read as reads, and none
is secret-bearing.

**Report also:** whether `aws.call` should refuse the STS mints to the model altogether (the design's table says
`AssumeRole` is "core only"), and whether `classify` should take secret-bearing from the output's shape at run time
rather than a table held by a test.

**Leave alone:**
- flake-causes (batch 10, in review): theseus-aws-catalog's tests/catalog.rs (its decode-time test) and Cargo.toml:
  your tests go in new files, and `GOLDEN` stays as it is;
- scrub-encodings (batch 10, in review): core scrub.rs and scrub/;
- the hands (core aws/hands/) and the session mints (aws/session.rs): they mint through the core, not `aws.call`.
