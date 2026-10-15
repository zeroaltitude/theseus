# CLOUD_REPORT: turn-inspector (theseus-x9swu)

Branch `cloud/20261013-turn-inspector`, cut from main at `f8975aac` (store format 34). **The store's format is unchanged:**
nothing here adds a field to a stored record or a record kind, so there is no bump, no `tests_layouts` sample, and the
two goldens named in the brief (`core_output.txt`, `kernel_frames.txt`) did not need the bump's rewrite. `core_output.txt`
changed by one field (`inspect_url`), see step 1.

## Commits

| commit | what |
|---|---|
| `78d06cdb` | core: `turn.inspect`, `[cockpit] public_url`, `TurnSubmitResult.inspect_url`, `protocol.gen` |
| `b80d20fe` | clients: Discord footer link, CLI status line, chat line and `/inspect`, `theseus open turn` |
| `47945c17` | cockpit: the inspector view, the flame, the route, the Inspect links, `lib/inspect.ts` and its test |
| `739feedc` | core: rustfmt |
| `dc91c8d3` | inspector: floor answers join their turn, route fallback, latency, breakpoints, request messages |
| `752d272c` | tests: the link follows `public_url` through a real turn; the web UI serves the route |
| `2981b61a` | docs: AGENTS.md of core, cockpit, CLI, TUI, Discord |
| `3c297cbe` | core: turn.inspect's assembly in smaller functions and its fixture split, for clippy's shape rules |

## Step 1: the link in every footer

**Found.** Every renderer already gets the whole `TurnSubmitResult` (Discord through the outbox's `reply` post, the
CLI and the chat through `turn.submit`'s answer and `turn.ended`), so the cheapest single source is one field on the
result, built where the result is built.

**Changed.**
- `[cockpit] public_url` (default `http://localhost:7433`; `config/cockpit.rs`, the template's `# [cockpit]`
  section, validation: `http(s)://`, no login, query, fragment or space). `CockpitConfig::turn_url` is
  `theseus_protocol::inspect::turn_url(base, session, turn)`, `<base>/session/<id>/turn/<turn>`, the base's trailing slash
  dropped. No port is hard-coded anywhere but the default.
- `TurnSubmitResult.inspect_url: Option<String>` (`skip_serializing_if` none, so an older peer's bytes are the same),
  set in `turn.rs` at the one site that builds the result (one line; `turn.rs` stays under its ceiling).
- Discord: the footer line ends `· [inspect](<url>)` (angle brackets: no embed). Same line, no part, no message, no ping;
  `is_text_part("…:footer")` and the lively-chat rules are untouched.
- CLI: the status line ends ` · inspect <url>`. **Not OSC 8**: that line goes through `print::footer`'s word wrap, and
  escape sequences in it would break the wrap's widths; the bare address is linked by the terminals the owner uses and the
  wrap keeps it whole. Say so if OSC 8 is wanted: it belongs in `render::md`'s link path, not in this line.
- Chat: `  inspect <url>` under a turn's line, and `/inspect` says the last again (`Chat::last_inspect`). The sessions
  TUI (`theseus-tui` board) has no reply footer, so nothing was added there.
- `theseus open turn [TURN] [--session S] [--print]` (`crates/theseus/src/open.rs`): resolves the session and turn as
  `theseus why` does, asks `turn.inspect` for the link the daemon built, prints it (`--json`: ids and link) and opens it with
  `$BROWSER`, `wslview`, `xdg-open` or `open` where one exists.
- The link carries no key or token (the config's validator refuses a login, query or fragment in `public_url`).

**Proved.**
- `cargo nextest run -p theseus-core -E 'test(config::cockpit)'`: the template is the default, a note changes it, a
  relay base with a trailing slash gives `http://localhost:17433/session/..`, bad bases are refused.
- `rpc::tests_inspect::a_turns_result_links_to_it_at_the_configured_address`: a real turn through the core with
  `public_url = http://localhost:17433` returns `inspect_url` at that address, and `turn.inspect` of it answers the same.
- Discord `render::tests::the_footer_links_to_its_turn_on_its_own_line` (one line, ends with the masked link, no mention,
  none without a url); CLI `render::later::tests::the_status_line_ends_with_the_turns_link`; chat
  `chat::tests::a_turn_ends_with_its_link_and_inspect_says_it_again`; CLI `tests/open.rs` (a scripted daemon: the reads are
  `session.list`, `context.explain`, `turn.inspect`, nothing written; `--json`).
- `tests_output::the_cores_output_matches_its_golden` was rewritten (`THESEUS_GOLDEN=write`): the only diff is
  `"inspect_url":"http://localhost:#/session/ses_#1/turn/turn_#4"` in the outbox post and `turn.ended` (35 lines).
- Planted reverts, each file `touch`ed back and `git status` clean after: Discord link push removed -> the footer test fails;
  CLI `inspect_words` replaced by "" -> the status-line test fails; chat `say_plain` removed -> the chat test fails;
  `trim_end_matches('/')` removed -> the config test fails; `inspect_url: None` in `turn.rs` -> the core golden fails.

## Steps 2 and 3: the route, the view and `turn.inspect`

### What was stored today, and what was not (the brief asks for this exactly)

Stored, per turn, before this work (nothing new is written):
- **Thinking: yes, in full.** Each loop's assistant node holds the provider's content blocks verbatim (`thinking` with its
  signature, `text`, `tool_use`), `model`, `provider`, `stop_reason`, `usage` (input, output, cache read, cache write, 1 h
  write), `cost_usd`, `request_id`, `compilation_id`, `request_digest`, and the per-message `effort`. (What a `thinking`
  block says is the provider's: with `thinking_display` set to summaries the text is the summary, with omitted it is empty.)
- **Tool calls: yes.** The call node has the input, the tool, the wire name, the gate record (validation, policy verdict,
  plan, proposal), and the action's `correlation_id`; the result node has status, `is_error`, the content as the model saw
  it, `bytes_total`, `truncated`, `full_ref`, `duration_ms`, `late`.
- **Each loop's compile: yes.** `context.compiled` per loop: decision and trigger, compilation id, strategy, prefix/tail
  node counts, messages, estimated tokens (and how), the request digest, tools offered, context files, **cache breakpoints,
  TTLs and each system layer's digest**, class, situation.
- **The trace: yes** (`turn.trace`): spans for admission, recall, compile, the provider call with `first_byte` and
  `first_token` marks, parallel tool calls under a `tools` span, advancer, store writes; `provider.call`'s timing row.
- **The route** (`route.decided`, when Jev routed), the compaction (`context.compacted`), recall (`recall.ran`), and a floor
  question (`tool.confirm_requested`, with its answer `action.confirm_answered` written **with no turn id**).

Not stored: **the system blocks' text and the exact request bytes.** A compilation keeps the digest (and the compile row the
layer digests), not the text. So the request view shows what is recorded (the compile row, the breakpoints, the cache state
from the answer's usage, the turn's own messages from its nodes) and asks `context.explain` for the system parts, which
rebuilds them as the session's next turn would and **says whether they are still the bytes the turn saw** (the digests
agree) or not. I did not add a per-loop record of part digests: the layer digests and the request digest are already in
`context.compiled`, and the bodies are not copied anywhere. The brief's "tools offered (loaded vs deferred)": no deferral
exists in this build, so the part lists the tools' names and count (`c.tools`).

`inspect_keep_turns` (the per-session ring) was **not added**: a turn's data lives as long as its records, so there is no
ring for the key to bound, and a key with no reader fails the reader rule. If the owner wants a bound on what the view
reads, it is the constants at the top of `rpc/inspect.rs` (`ROWS` 2,000 and `NODES` 3,000 per turn, `LEAD` 400 positions
before `turn.started`, `START_PAGES` x `STARTS` for finding an old turn: 6,000 turns back).

### `turn.inspect {session_id, turn_id}` (`rpc/inspect.rs`, `theseus_protocol::inspect`)

A read, on the blocking pool: the turn's `turn.started` row is found by paging the session's `ks:turn.started` rows back
(that also names the previous and next turn), then the session's ledger rows and nodes are read through the tag index from
400 positions before it and filtered by `turn_id`, plus the answers to the turn's floor questions (found by correlation id,
since they carry no turn). `assemble` is pure. No frame, no record, no model-visible change; the first request is
unchanged (`first_request_kb: 39.55 KB, budget 40` before and after). Thinking, the prompt, tool inputs and results,
gate records, row data and span attributes are given **to a private place only** (`Surface::reads_private`: the cockpit,
the CLI); any other surface gets shapes, sizes and times, and `withheld` says why. Never a Discord message or a log line:
Discord does not call it, and nothing logs a block.

**Proved** (`cargo nextest run -p theseus-core -E 'test(/rpc::tests_inspect/)'`, 8 tests):
- a 3-loop turn with 2 parallel calls per loop, thinking, a compaction, a recall row, a floor question answered with no
  turn id: outcome, totals (loops, model calls, tool calls, cache read, cost), the route written before `turn.started`,
  prev and next turn, link, trace, every row kind present, each loop's compile, thinking in full, the call's correlation id,
  `added` (5 nodes: the answer, two calls, two results), a text-only last loop;
- privacy: a non-private read has none of the planted words ("Plan for loop", "tide.log", "/harbour", the prompt) anywhere
  in its JSON, no gate, no input, no content, rows' data null, the trace's attributes null;
- a turn no route decided shows the profile it started on; unknown turn and session are refused;
- **a 20-loop turn with 38 tool calls: p95 under 150 ms** (20 reads, debug build; this VM: about 25 ms);
- it writes no frame (`last_position` unchanged).
- Planted reverts: privacy flag forced on -> the privacy test fails; `answers_of` removed -> the multi-loop test fails (the
  floor answer is missing); the `added` computation removed -> fails; a call's loop taken from the node instead of its
  answer (`loop_of_answer` removed) -> fails (the fixture's tool nodes carry no loop index); `LEAD = 0` -> two tests fail.

### The cockpit (`cockpit/`)

- Route `session/:id/turn/:turn` (lazy, like `context`); `session/:id?turn=` still scrolls the deck to the turn (it already
  did, `data-turn`), and each turn's header in the transcript and the Timeline tab now have an **Inspect** link.
- `views/TurnInspect.tsx`: header (prompt with who/when/size, outcome, stop reason, totals: wall time, loops, model calls,
  tool calls, input / cache read (with its share) / cache write / output tokens, cost, errors; the route, prev/next, copy as
  JSON); `components/TurnFlame.tsx` (SVG): one clock, lanes for loops, model (the call, `first_byte`/`first_token` marks),
  tools (parallel calls in rows of their own), waits (admission, `job_wait`), **you** (the question to its answer; a wait that
  outlasts the turn is drawn to the turn's end and says its length), compile/compaction, recall, judges, store, marks; hover
  tip, click selects (and opens its loop), wheel zooms about the pointer, drag pans, arrow keys walk the bars, `+`/`-`/`0`.
- Per loop (an accordion; `?loops=0,2` in the address): **request** (compile row, cache breakpoints, layer digests, the
  system parts from `context.explain` in order with a ◆ after the last part of each breakpoint's block, the digest verdict,
  the turn's own messages up to that loop), **response** (model, effort, stop reason, usage, cost, request id and digest,
  latency from the trace, and every block: thinking in full), **calls** (input, result as the model saw it, status,
  duration, bytes, `full_ref`, the gate's record on a click, the ledger rows that name the call by tool_use_id or action
  correlation id), **changed since the last loop** (tokens, messages, tools, digest, layers, what the last loop added).
- Pure parts in `lib/inspect.ts`, tested in `test/inspect.test.ts` (12 tests): lane choice, row packing (parallel calls stack),
  the keyboard walk, zoom and pan, the person's waits (by correlation id, clipped to the turn), loop diff, call timing,
  call's rows, breakpoint places. `npm test`: 392 pass; `tsc -b` clean; lint: no warning from the new files.
- The daemon's web test fetches `/session/ses_x/turn/trn_y` and gets the app shell (`web_ui.rs`).

**Live check I ran here** (scratch daemon, `theseus-sim fake-model --rules`, Chromium via Playwright; `[cockpit] public_url` =
`http://localhost:7434`): `theseus ask "inspect me"` printed `… · inspect http://localhost:7434/session/ses_…/turn/turn_…`;
`theseus open turn --print` printed the same; the page rendered with no console error: header, flame with `fs_list` and
`fs_read` in parallel rows, request tab with the breakpoint marks, calls tab with real tool results, the diff tab. A turn
parked on a real floor question (`/home/user/theseus/docs` outside the roots), approved with `theseus confirm`, showed
`next` and the route fallback. The stand-in model produces no thinking, so thinking's rendering is checked by the unit
fixture and the typed code path only: **the maintainer should look at a real thinking turn** (below).

**The live check for the maintainer** (a scratch daemon on a fresh state dir, with a real key, not the operator's):
```
theseusd --config <scratch.toml> --socket /tmp/x.sock --state-dir /tmp/x-state    # [web] port 7435, [cockpit] public_url = "http://localhost:7435"
theseus --socket /tmp/x.sock ask "explain what fs.list does, then list /tmp"      # a Sonnet/Fable profile with thinking on
theseus --socket /tmp/x.sock open turn --print                                    # the link; open it
```
Should show: the footer line ending `inspect http://localhost:7435/session/…/turn/…`; in the page the thinking block (purple,
"thinking · N chars") above the text in each loop's Response tab, cache read/write in the header on the second turn, and
`prev` linking back. In Discord (private place): the footer line `… · [inspect](<url>)` with no embed; the same turn in a shared
channel: still a link, and the page (private surface) is what shows the thinking.

## Speed (FAST)

The change on the turn path is one `Option<String>` built per turn (a `format!`) and one field in the result's JSON.
`theseus-sim bench turn --runs 15 --burst 0`, this VM (4 cores, debug builds), main's `f8975aac` daemon against this branch's,
alternating runs:

| | plain p50 / p95 | tool-call p50 / p95 | frames plain / tool / batch | syncs | first request |
|---|---|---|---|---|---|
| main | 36.8-38.2 / 51-56 ms | 78-82 / 107-117 ms | 4 / 8 / 14 | 2 | 39.55 KB |
| branch | 38.2-39.9 / 44-57 ms | 79-92 / 92-122 ms | 4 / 8 / 14 | 2 | 39.55 KB |

The spread between two runs of one build is as large as the difference. (One "after" run showed a p95 of 489 ms in a single
slow turn, not repeated in three further runs.) `bench lifecycle --runs 10 --phases cold,shutdown,kill`, alternating builds:
cold start p50 19.4-23.1 ms (main) against 21.3-23.1 ms (branch); shutdown p50 8.5-12.9 against 11.4-11.9; kill-restart p50
21.7-24.8 against 24.0-26.8; all inside their budgets with the same margin, no run over. `tests_m3::a_plain_turn_stays_within_its_frame_budget`
and `frames_counted` pass (full theseus-core run: 2,295 passed). These are this VM's numbers, not the owner's machine's:
the owner's gate runs the real benches.

## Keel

**Keel findings expected: none from this branch.** `python3 scripts/keel-guard.py` run on the branch as cut reports one
erosion that is **not this branch's**: `crates/theseus-sim/src/lifecycle.rs:155: budget-raised: "seed" => Some(250.0)`, acked by
`1c2beb2747` on main, whose signature cannot verify here (`%G? is E`: this VM has no signing keys). The same run says the
store test `a_checkpoint_for_close_is_made_durable_by_the_close` was renamed and rewritten, also main's history. To judge only
this branch's range I ran the gate with `THESEUS_KEEL_BASE=f8975aac` (the commit I was cut from): `keel: ok (f8975aac..the working
tree; 48 files changed, 0 findings acked)`. No test, budget, ceiling, cap or `#[allow]` was touched; `Cargo.lock` and the
package locks gain nothing; no ceiling in `scripts/long-files.txt` was raised (`discord/render.rs` 2,886 of 3,009; `turn.rs`
3,492 of 3,525; the CLI's `render.rs` 3,064 of 3,128).

## Gate

`THESEUS_KEEL_BASE=f8975aac TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (the base set as explained under Keel; without it the
keel phase fails on main's own unverifiable ack, before any compile). Phases that passed on this tree: door (main's copy), keel
(0 findings in the range), keel tests, fmt, shape, features, **clippy** (`-D warnings`; the first run failed on `assemble` and the
test fixture being over 100 lines, fixed by splitting them, no `#[allow]`), **cockpit** (lint, 392 tests, `tsc -b`, build), test
build, reader rule (`tests_registry`: `turn.inspect` has its dispatch arm and its cockpit reader).

The **suite** phase: `cargo nextest run --workspace --no-fail-fast`: **4,834 tests, 4,801 passed, 33 failed, 47 skipped**. The 33
failures are the documented L1 ones this VM cannot pass (a root daemon with no job cgroup, theseus-pv6i): 20 in
`theseus-sandbox::contract` and 13 in `theseusd::sandbox` (`clause_01_namespaces` .. `clause_12_no_ambient_credentials`,
`egress_18b_*`, `exit_status_and_signals`, `scratch_is_reported_and_discarded`, `sigterm_is_forwarded_to_the_command`;
`l1_argv_routes_a_call_to_l1`, `a_job_with_no_list_has_no_proxy`, `a_granted_program_in_l1_gets_its_grant_at_launch`,
`a_cancel_of_an_l1_job_is_verified_by_its_pid_namespace`, `the_jobs_bench_l1_row`, and the rest of that file). No timing test
failed, and no negative assertion failed.

A first suite run inside the gate showed 171 failures, **all from a full disk** (this VM's allowance ran out mid-run: `No space
left on device` in the build and the tests; the core's self-backlog, job-handle, spawn-follow tests among them). I freed the stale
test binaries and ran the suite again alone, as above. Those 171 are not findings.

Phases I ran myself after the suite, because the gate stopped there (it does the same two by hand): **protocol types**
(`the_cockpits_types_are_generated_from_the_rust_ones` passes, `git status` shows nothing under `cockpit/src/protocol.gen`).
**Not run here**: the `deny` phase (`cargo deny` is not installed on this VM; no dependency was added, `Cargo.lock` and the
package locks are unchanged), and the lifecycle, jobs and size benches under the gate (`THESEUS_GATE_NO_BENCH=1`, as the brief
says): I ran the turn and lifecycle benches by hand against main's build, numbers above.


## What is left or uncertain; design choices the owner should hear about

- **The system text of a request is rebuilt, not recorded** (see above): for a turn whose system blocks changed since, the view
  says so and shows today's parts. Recording each loop's part digests would be a new stored field (format 35) for what the
  compile row already carries in `cache.layers`; I judged that not worth a bump. The owner may decide otherwise.
- **No `[cockpit] inspect_keep_turns`**, no ring (above).
- **Bare URL, not OSC 8**, in the CLI line (above); the chat line is plain too.
- **A turn's wait on a person** is drawn only from the turn that asked; the continuation turn after the answer shows no wait
  of its own (it starts when the answer arrives). The question's turn shows the whole wait (clipped to its own end).
- **Limits of the read**: a turn with over 2,000 rows or 3,000 nodes is read from its start and cut there; a turn older than
  6,000 turns of its session is "not among its newest turns"; the answers to floor questions are read from 2,000 rows after the turn.
- **Docs the maintainer should write** (I may not touch them): the spec's Part III item for this step (the `turn.inspect`
  method, `[cockpit]`, the footer link, the unchanged store format, the numbers above) and its version line;
  `docs/status.md` (the recently landed step; the roadmap's cockpit row); `docs/design/` roadmap row; `docs/technical-overview.md`
  if it lists protocol methods. The `AGENTS.md` files of core, cockpit, the CLI, the TUI and Discord are updated in this branch.
- **Environment notes**: this VM's disk is a fixed allowance; two gate runs failed at the test build with ENOSPC and a stale-binary
  cleanup of `target/debug/deps` was needed. Not a finding in the tree. A shared `CARGO_TARGET_DIR` for a baseline build of main
  poisoned one build (the base's `theseus-protocol` was reused: "no field `inspect_url`"); touching the crates' `lib.rs` fixed it.
