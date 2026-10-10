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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261010-bench-parity`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: bench-parity, a fair Terminal-Bench rerun: the same packages in both containers, the trial limit at $2 plus one reservation, quiet graders, the attempt plan committed first, and the tool-friction counts in bench/ (theseus-w052, with theseus-3lqk, theseus-8xp0, theseus-p3jl)

Branch: `cloud/20261010-bench-parity`. Every commit's subject carries `theseus-w052` (and the carried issue's id where
a commit is that issue's). Deadline for the report: 3 hours after you start.

**This row is bench/ only: no harness change.** Nothing under crates/ or cockpit/ changes. Read bench/README.md, the
Harbor adapter (bench/harbor/: theseus_agent.py, claude_code_agent.py, pi_agent.py, measure.py, measured.py,
efficiency.py and their tests) and bench/report/ (charts.py, draft.py) before you start.

**Why this work exists.** The stopping point (the owner, 2026-10-08) reruns every benchmark once every known gap is
fixed. The first of those runs, Terminal-Bench R0 (89 tasks x 3, Theseus against Claude Code, Sonnet 5.5, effort
medium in every arm), must compare the harnesses and nothing else. Four things still differ between the arms, or
can't be read from what a run leaves. Fix them so R0's numbers mean what they say.

1. **The same packages (theseus-w052 1).** Harbor's Claude Code installer adds curl, bash, nodejs, npm and procps to
   the task's container before the agent's clock starts. The Theseus arm's container doesn't get them, so a task that
   needs `node` or `ps` costs Theseus a detour that Claude Code never pays. Install the same set in the Theseus arm,
   before the agent's clock, as the Claude Code arm does (read how its installer runs and copy the timing, not just the
   list). A test that the Theseus arm's install script names each package and runs before the timed phase.
2. **The trial limit (theseus-w052 2).** `THESEUS_BENCH_SPEND_LIMIT` becomes $2 plus one maximum reservation at the
   arm's model: 3.75 for Sonnet 5.5, about 4.75 for Opus 5.5 (derive it from the model's price and the reservation the
   kernel makes, and say how in a comment). The spend limit is what stops a trial; the extra reservation exists so that
   a trial that has spent $1.99 is not refused the call that would take it past $2.00 by the reservation alone. The
   report must name every Theseus trial past $2.00 of real spend: add that to the efficiency record and to the draft
   report (a line per such trial). **Keel:** this raises a spend cap in bench/, a `cap-raised` finding. Make it a
   commit of its own and list it under "Keel findings expected".
3. **Quiet graders (theseus-w052 3).** The verifier's timeouts were confounded with a busy host. The run's defaults:
   at most 4 trials at once, and a refusal to start (with the reason) while scripts/gate.sh's lock is held or the
   host's CPU pressure (/proc/pressure/cpu, some avg10) is above a threshold you name. A verifier's own timeout is
   recorded as the grader's (`grader_timeout`), never as the agent's failure, and the draft report lists those trials
   apart. Tests with a fake /proc/pressure file and a held lock.
4. **The attempt plan first (theseus-w052 4).** A run reads its attempt plan (k a task, or a per-task list such as a
   Neyman allocation) from a file under bench/plans/, and refuses to start unless that file is committed and clean in
   git (`git ls-files --error-unmatch` and an empty `git diff` for it), naming the commit in the run's record. A test
   for each refusal and for the record's commit.
5. **The tool-friction counts in bench/ (theseus-w052 5).** Every report carries T4 (tool friction) and T5 (cost and
   calls per solve). Today they come from a script outside the repository; write them into bench/report/ as a module
   with tests, reading Harbor's ATIF trajectories (each trial's `agent/trajectory.json`; steps with `source == "agent"`,
   their `tool_calls`, and `observation.results` matched by `source_call_id`). The definitions:
   - **T4, per arm:** trials with at least one tool-level error, where a tool-level error is a tool result flagged as an
     error (`extra.status == "error"`, or `extra.is_error` / `extra.tool_result_is_error`) whose text does not start
     with `[exit code` or `Exit code` (those are a command's own failure, not the tool's); **malformed inputs** (a tool
     call whose input was not valid JSON for its schema; count them by tool); **invented names** (a call to a tool the
     arm doesn't offer, or a field its schema doesn't have). Report counts and the share of trials.
   - **T5:** over the (task, attempt) pairs both arms solved: mean and median dollars, mean calls, mean agent seconds,
     and output tokens per call, per arm.
   `draft.py` prints both tables in every report it drafts. Tests on a small fixture of invented trajectories.
6. **Pi's slot in the charts (theseus-3lqk).** `bench/report/charts.py`'s `ARMS` has no `pi` key, so `draft.py harbor
   --arm pi=<jobs>` refuses a Pi job, and the four harness slots are taken (theseus, claude-code, theseus-batching,
   openclaw). Give Pi a slot and its display name (the palette's next validated colour; keep the palette's validator
   green), with a test that `draft.py harbor --arm pi=<job>` drafts a Pi job and charts it.
7. **The stop at a timeout (theseus-8xp0).** `measure.py`'s `stop_agent_script` signals the processes named `claude` or
   `pi` and their descendants as one read of /proc finds them. Two kinds escape and run on while the verifier runs:
   what the agent left orphaned (a command a tool's shell backgrounded through a subshell that exited, `(cmd &)`,
   reparented to PID 1), and what a SIGTERM-ignoring process forks during the 3 s grace. Stop by what the run started,
   not by its tree at one moment: record the container's process ids before Harbor's run (the arm already starts its
   sampler first, and can keep the list in its state dir); at the stop, SIGSTOP every process not in that list except
   the stop's own shell and its ancestors, read /proc again until no new one appears, SIGTERM and SIGCONT, then SIGKILL
   what is left after the grace. Tests: a child orphaned through an exited subshell, and a forker that ignores SIGTERM
   (forking every 0.2 s), both gone, with processes under names of the test's own (never `claude` or `pi`). A reviewer's
   probe found 1 of 1 and 21 to 23 of them left today; your test must fail on main's script (plant it back and show it).
8. **The record says what ran (theseus-p3jl).** (a) Claude Code's `effort` under an `--ak reasoning_effort=` ablation is
   held by no test: `MeasuredClaudeCode.populate_context_post_run` (and the async arm's) stamps
   `self.options.reasoning_effort`, and a plant that stamps `measure.EFFORT` instead passed every test. Add the test
   (an agent built with `reasoning_effort="high"`: its record's `effort` is `high`), for both arms. (b) The Theseus
   arm's `version` is `theseus 0.0.1` for every build: write the build's commit beside it (bench/build.sh writes one;
   or health's build commit), so the record tells two builds apart, with a test.

**Doing more, filing less** (the owner, 2026-10-10): a defect you find in code you are already in, fix it here with a
test and name it in the report. Only what is out of this row goes in the report's "Left" section.

**Siblings.** recall-plan-fix (bench/recall) runs beside you: stay out of bench/recall. Another session on the owner's
machine writes benchmark reports and may make small fixes in bench/harbor while you work: keep your changes in the
functions this task names, so the maintainer's merge is mechanical.

**Tests and proof.** bench/'s Python suites are the gate for a bench-only commit (the preamble says how; Harbor's in a
3.12 venv). Run every suite under bench/ before each commit, with counts in the report. Plant a revert for each of
1 to 8 (each test failing on the planted bug) and record what caught it. No benchmark is run here; nothing here
spends.

**Live check for the maintainer** (write it as exact commands): one Terminal-Bench smoke task (fix-git) in each arm on
the owner's machine with the real keys, showing the Theseus container's packages, the record's effort, version and
commit, a T4/T5 table in the draft, and the plan check refusing an uncommitted plan.

**Findings from the 2026-10-10 bench dig** (the maintainer reran the Theseus arms on 10-10):
- **The routed arm's list price.** `bench/harbor/efficiency.py`'s `LIST_PRICES` holds Sonnet 5.5 and the OpenAI
  models only, so a routed trial that Jev sends to Opus 5.5, Fable 5.1 or Haiku 5.5 is priced by Theseus's own table,
  not the bench's, and the as-shipped arm's dollars are on a different table from every other arm's. Add those
  models' list prices (from the repo's model catalog, the numbers `theseus catalog` prints) and a test that every
  model named in `theseus-bench-routed.toml`'s profiles has a price row.
- **The quiet-machine wait.** A run beside other builds stays readable only through its per-trial overlap column:
  keep that column in the data file and name it in the report draft's threats to validity.

Run the gate before your first commit and before your last. Commit CLOUD_REPORT.md as the branch's last commit, and end
with a short final message starting `CLOUD REPORT COMPLETE`.
