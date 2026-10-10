# Cloud report: session-dir (theseus-aab7)

Branch `cloud/20261010-session-dir`, cut from 2fd1f65 (store format 26). Three steps, each gated and pushed:

| commit | step |
|---|---|
| 42991f7 | protocol: `SessionOpenParams` and `TurnSubmitParams` moved, unchanged, into `theseus-protocol/src/submit.rs` |
| e8821a2 | core: the session keeps its directory (format 26 → 27), its tools work there, the CLI sends it |
| 15e3367 | compiler: the session's directory in system block 3, with the fourth cache breakpoint |

**Store format: bumped 26 → 27** (`SessionRecord.dir`), with the format-26 session layout's literal sample in
`tests_layouts.rs` (`SESSION_BEFORE_DIR`). **Goldens:** `crates/theseus-core/tests/golden/core_output.txt` changed
(in 15e3367): every `context.compiled` cache line's breakpoints go from `["header","conversation"]` to
`["header","session","conversation"]`. **`kernel_frames.txt` did not change**: the kernel golden does not print the
manifest's format, so the bump alone moved nothing there (the brief expected it to). `theseusd/tests/versions.rs` reads
format 27 (and 28 as "newer").

## Step 1: the move (42991f7)

- **Found.** `theseus-protocol/src/lib.rs` was at its ceiling (2,733 of 2,733).
- **Changed.** The two types and their docs moved into `submit.rs`, re-exported from the root
  (`pub use submit::{SessionOpenParams, TurnSubmitParams}`); no path changed and no generated TypeScript changed.
  lib.rs is now 2,673 (2,681 after step 2's two fields); its ceiling is untouched.
- **Proved.** The gate on that commit alone (see "The gate").

## Step 2: the session keeps its directory, and its tools work there (e8821a2)

- **Found.** `turn.submit` carried no directory; every tool's `ToolCtx.cwd` was the runtime's, from `[tools] cwd`.
  Every place a call is planned or run reads `self.ctx`: the gate's `tool.plan`, a batch's per-step plans
  (`toolrun/batch.rs`), a job's spec (`toolrun/job.rs`), and the run (`toolrun.rs`, which also hands the context to
  `term.*`, `file.read` and the LSP tools). Also: `TurnRunner::run` re-reads the session record under the lock and
  replaces the caller's copy, so anything `turn_submit` sets on its copy is lost (I hit it: the move test failed).
- **Changed.**
  - Protocol: `dir: Option<String>` on `SessionOpenParams` and `TurnSubmitParams` (serde default, absent when unset,
    `ts(optional)`); `SessionInfo.dir` (`session.list`, `session.open`'s answer); `TurnSubmitResult.outside_roots`
    (the roots, when this turn started its session in, or moved it to, a directory under no root). A relative `dir`
    is `INVALID_PARAMS` before anything opens (`rpc/session_dir.rs::check`, also on `session.open`, MCP's too).
  - Store: `SessionRecord.dir`, written in the session's creation frame (`open_session_as`). A `turn.submit` that names
    a session and a `dir` moves it: `rpc/session_dir.rs::moved` sets the turn's copy, the turn's re-read keeps the
    copy's directory over the stored one (`turn.rs`, the re-read), and the turn's own session write keeps it
    (`take_turns_fields`; no turn clears one). No extra frame. A task opened by a session works in its parent's
    directory (`task.rs`, one line).
  - Tools: **the resolver is `ToolRuntime::cwd_for(dir: Option<&str>) -> PathBuf`** in
    `crates/theseus-core/src/toolrun/session_dir.rs` (tool-input-fit's `proc_run {command}` should call it, or take
    `ctx_in(tc).cwd`); `ToolRuntime::ctx_in(tc)` is the call's `ToolCtx` with that cwd (a borrow when the session has
    none). `TurnCtx` gained `dir: Option<&str>`. toolrun.rs gained only calls (the gate's plan and proposal, the run);
    `order::At` gained `ctx` so a batch's steps plan in the session's directory. The gate is unchanged: a path or a
    program's directory outside the roots waits as before.
  - CLI: `ask` sends `std::env::current_dir()` when it starts a session (`--spawn ask` too), `--dir DIR` overrides it
    or, with `-s`, moves the session; a continued session sends none and keeps its own. When the answer carries
    `outside_roots`, `ask` prints once on stderr: `/tmp/x is outside the workspace roots (/work): reads and commands
    there will ask you first`. It comes after the turn's reply, not before: the CLI learns the roots from the daemon's
    answer (no extra request). `theseus sessions` shows `\tin <dir>` last (only for a session with one). Both are in
    `crates/theseus/src/render/session_dir.rs`; render.rs gained the `mod` line and one call.
  - Clients that send nothing (Discord, the cockpit, the TUI) are unchanged: `dir: None` at each literal.
- **Proved.**
  - `tests_session_dir.rs` (through a real protocol connection): `a_session_started_in_a_directory_works_there`
    (`proc_run ["pwd"]` prints X, `fs_read a.txt` reads X/a.txt, not the root's), `a_resumed_session_keeps_its_directory_and_dir_moves_it`,
    `a_directory_outside_the_roots_waits_as_today` (the call is `awaiting_confirm`, `outside_roots` names the root),
    `a_client_that_sends_no_directory_gets_todays_cwd`, `a_relative_directory_is_refused`. The wire test is
    `submit.rs::tests::dir_is_optional_on_the_wire`; the old layout is in `every_old_layout_on_disk_still_reads`; the
    CLI's lines are `render::session_dir::tests` (2).
  - Planted reverts (each restored with `touch`, `git status` clean of the plant after):
    1. the turn's re-read dropped the client's directory (`dir: fresh.dir`): `a_resumed_session_keeps…` failed
       (left `…/harbour-tides`, right `…/harbour-gauges`): the move was lost.
    2. the field dropped on resume (a `turn.submit` without `dir` cleared the copy, the re-read took the copy's, the
       session write took it unconditionally): `a_resumed_session_keeps…` failed (left `None`).
       Each of those three sites alone did not drop it (I planted the `moved` one alone first: all passed), so the
       keep is held at three places.
    3. the tools' default left at `[tools] cwd` (`ctx_in` always the runtime's): `…works_there`,
       `…outside_the_roots_waits…` and `…resumed…` failed; `…no_directory…` and `…relative…` passed, as they should.
  - Two theseusd suites ran `theseus --spawn ask` from the test runner's own directory: with the CLI now sending
    it, their reads landed outside the roots and parked. `headless.rs` and `spawn_follow.rs` now start the CLI in
    their `projects/` (`Command::current_dir`), where an operator would work. No assertion changed.

## Step 3: block 3 and the fourth breakpoint (15e3367)

- **Found.** The tools note in the shared header named `[tools] cwd`. `compiler.rs` was 5 lines under its ceiling.
  And a defect in code I was in: the task graph's view (`task_graph/view.rs::attach_to`) **copied** the top-level
  automatic breakpoint onto the block before the view and left the top-level one in place, so a request with a view
  already used 4 slots (header, context, that block, top-level); adding block 3 would have made 5, which the provider
  refuses. Its doc said "moved".
- **Changed.**
  - `compiler/session_block.rs`: `TurnRunner::session_block(dir, place)`, the text `Directory: <cwd_for(dir)>`;
    empty (no block) without tools and in a shared place (no owner path goes there). compiler.rs gained the spec's
    `session_text` field (one doc line), its entry in `system_blocks`, and the `mod` line, and one test literal's
    field: it is at 2,560 of 2,560 (I shortened two doc comments to keep it there). The header's tools note now says
    "Relative paths resolve against the session's directory, which its own block names."
  - The block is in `system_digest`, so a move recompiles once (`system_changed`, which strips the prefix's thinking,
    as an edited context file does); otherwise it is static for the session, and a keep-warm replay of the prefix
    replays it unchanged (cache-fix's concern). Every existing session recompiles once on upgrade anyway, since the
    header's text changed.
  - The breakpoints: `cache_layout` gives each system block one where its prefix can reach the model's minimum, so
    block 3 takes the fourth (header, context, session, conversation). `attach_to` now **moves** the top-level marker
    (restores nothing when there is no block to put it on, as before).
  - The turn and the routed turn's spec set the block (`turn.rs`, `route_step.rs`: one line each);
    `context_parts::built_parts` reads the session's record for it, so `context.explain`'s digest still matches.
  - `theseus-sim fake-model --dump DIR` writes each request's body (`request-000.json` …): the live check reads what
    the daemon sent.
- **Proved.**
  - `two_sessions_share_the_header_and_differ_only_in_block_3`: every system block but the last is the same bytes in
    two sessions in two directories, the tools too; the last is each one's `Directory:`; it carries `cache_control`;
    each request has at most 4 breakpoints in all. The step-2 tests now also read each request's `Directory:` and
    that the header names no directory.
  - `tests_task_graph::a_plan_splits…` now holds a request with a view to at most 4 `cache_control` and no top-level
    one; `task_graph::tests::the_view_is_the_last_block…` asserts the top-level one is gone.
  - Planted reverts: (4) block 3 inside the header (`spec.system_text += …` instead of its own block): `two_sessions…`,
    `…works_there`, `…resumed…`, `…no_directory…` failed. (5) the view's marker copied, not moved: `the_view_is_the_last_block…`
    failed, and so did `a_plan_splits…` (line 404, the count). theseusd's `cache_header` did not catch (5).
  - Test rewrites, no assertion removed (keel: 0 findings): tests_m3's `system_of` leaves out the `Directory:` block
    (those tests are about the header and the context), and two block counts went 1→2 and 2→3; theseusd's
    `cache_header` counts 3 blocks, checks block 3 starts `Directory: `, and compares the task's request with the
    first session's without the top-level breakpoint, which its view moved into its messages (asserted).

## Live check (on this VM, done)

A scratch daemon on the stand-in model, `/tmp/live/run.sh` (a fresh state dir; no other daemon here):

```bash
L=/tmp/live/run; mkdir -p $L/{work,state,dump,x}
cat > $L/rules.json <<'J'
[{"when": "where are we", "calls": [{"name": "proc_run", "input": {"argv": ["pwd"]}}]}]
J
cat > $L/theseus.toml <<T
[model]
api_base = "http://127.0.0.1:9448"
[secrets]
anthropic_api_key = "env:LIVE_KEY"
zai_api_key = "env:LIVE_KEY"
jev_api_key = "env:LIVE_KEY"
[discord]
enabled = false
[web]
enabled = false
[index]
enabled = false
[judge]
enabled = false
[tools]
projects_dir = "$L/work"
T
export LIVE_KEY=sk-live-check-not-a-key
theseus-sim fake-model --addr 127.0.0.1:9448 --rules $L/rules.json --dump $L/dump &
theseusd --config $L/theseus.toml --socket $L/sock --state-dir $L/state &
cd $L/x && theseus --socket $L/sock ask "where are we"        # parks: outside the roots
theseus --socket $L/sock confirm act_…                         # the id ask printed
theseus --socket $L/sock sessions
mkdir -p $L/work/harbour-tides && cd $L/work/harbour-tides && theseus --socket $L/sock --json ask "where are we"
theseus --socket $L/sock shutdown
```

What it showed: the first ask parked with `? proc.run needs your confirmation: run `pwd` in /tmp/live/run/x … (/tmp/live/run/x
is outside the workspace roots: /tmp/live/run/work)` and, on stderr, `/tmp/live/run/x is outside the workspace roots
(/tmp/live/run/work): reads and commands there will ask you first` (exit 6, parked); `confirm` ran it and the tool
result was `[exit code 0]\n/tmp/live/run/x\n`; `sessions` ended its row `in /tmp/live/run/x`; the ask inside the root
ran `pwd` at once (`/tmp/live/run/work/harbour-tides`, no notice, `outside_roots` absent). Each dumped request had two
system blocks (header, then `Directory: /tmp/live/run/x` or `Directory: /tmp/live/run/work/harbour-tides`), both
marked `cache_control`, and the top-level one.

**For the maintainer on the owner's machine:** the same, with the install build's binaries, a scratch state dir and
socket, and a port that is free (never the operator's daemon or 7433). Add a context file (`[context] files`) to see
three blocks with three marks plus the top-level one; and in a session that has a task, read a request: four
`cache_control` at most, none top-level. With real keys, `cd ~/projects/<a repo> && theseus ask "run pwd and read
README.md"` should run in that repository (inside the roots: no approval).

## FAST

One field in the creation frame; no frame added (the turn bench: plain 5, tool-call 9, at every gate). `theseus-sim
bench lifecycle --theseusd target/debug/theseusd --runs 10 --check` once, on the debug build: **LIFECYCLE OK in 52.4 s**:
cold start p95 39.8 ms (budget 50 + 7), from the copy 30.2, clean shutdown 18.2, SIGKILL and restart 26.6, binary swap
35.6, cancel 17.2.

## The gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=2fd1f65 scripts/gate.sh` before each
commit. Each failed only in the suite, then I ran the phases after it by hand (protocol types: ok; no "compiled under
the lock" note; turn bench `--runs 5 --burst 0 --check`: plain 5, tool 9, ok; `cargo deny --offline check`: ok).

- **42991f7:** 3,682 run, 33 failed: the 33 known L1 tests (theseus-sandbox's contract tests and `spawn_100`, theseusd's
  sandbox tests), root without a job cgroup (theseus-pv6i).
- **e8821a2:** 3,690 run, 34 failed: the 33 L1, and **`theseusd::gone_jobs::a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`**
  (`assertion left == right failed: the live wrapper's job runs on: {"dispatched":2,…}`), not on the known list. It
  passed 3 of 3 reruns alone. It is a count read twice: the test waits until `dispatched == 1`, then reads the
  actions again, and a late result's continuation's model call (which its own comment says is dispatched for a
  moment) can land between. Nothing in it sends a directory or reads a system block. I did not get a run under load:
  my load run rebuilt the workspace at nice 19 beside four busy loops and passed 10 minutes before any test ran, so I
  stopped it (the pids I started). Worth an issue.
- **15e3367:** 3,691 run, 33 failed: the 33 L1.
- **Keel:** with `THESEUS_KEEL_BASE=2fd1f65`, `keel: ok … 0 findings` at every commit. **Without it the keel phase
  fails on findings that are not this branch's**: this clone's `main`/`origin/main` is a9ad950, older than 2fd1f65,
  so merge-base..HEAD includes main's own unacked commits (5 findings: tests.rs in theseus-tui's assert count,
  two `#[allow]`s, two long-files ceilings). Keel findings expected from this branch: none.
- **Disk:** the first gate of 42991f7 failed 123 tests because the session's disk allowance ran out (939 MB free,
  under the jobs' 1,024 MB floor: "the job was not started"); `target/debug/incremental` was 17 GB. I deleted it and
  ran every later build with `CARGO_INCREMENTAL=0`.
- `cargo deny fetch` ran (setup's chain stopped at the build's first error, so I ran it on its own): ok.

## Left, uncertain, and for the owner

- **Health** carries no directory: it has a session count, not sessions. `session.list` (and `session.open`'s
  answer) carry `dir`. If health should name something (the default `[tools] cwd`?), that is a small follow-up.
- **The TUI has no "new session"**: it only sends to sessions that exist, so there was nothing to fill. When it gets
  one, it should send its current directory as `ask` does.
- **The outside-the-roots notice comes after the reply**, since the roots arrive in the turn's answer. Saying it
  before the turn would need the roots first (`tool.list` is heavy; a field on `session.open`'s answer would do).
- **A move costs a recompile** (block 3 is in `system_digest`), which strips the prefix's thinking. Moving a session
  is rare; I judged a correct block worth it.
- **The re-read and a stale copy:** the turn keeps its caller's directory over the stored one. A continuation the
  driver started from a copy read before a `turn.submit` moved the session could run in the old directory once; the
  lane orders one connection's requests, so I left it.
- `file.read`'s saves (`.theseus-files/`) and the job's `THESEUS_SESSION` stay as they were; `file.read save` now
  writes under the session's directory, like the other tools. `brokered`'s fallback directory and the narrative's
  `subject` still use the runtime's cwd (the plan's own directory is what they read for `proc.run`).
- **Docs to change (the maintainer's):** `crates/theseus-core/AGENTS.md` ("The system header": the tools note no
  longer names a directory; a third block, the session's, with the fourth breakpoint; "Tool calls": `ctx_in`,
  `cwd_for`); `crates/theseus/AGENTS.md` (`ask --dir`, the notice, the sessions column); theseus-sim's AGENTS.md
  (`fake-model --dump`); the spec's Part III item and `docs/status.md`. The store's format list in
  `theseus-store/src/store.rs` has its 27 line.
- Siblings: toolrun.rs gained only calls and one field's doc on `TurnCtx`; compiler.rs is exactly at its ceiling
  (cache-fix adds "at most a call": it has no line left, so its call needs a line taken elsewhere, or the render split
  the long-files note asks for).
