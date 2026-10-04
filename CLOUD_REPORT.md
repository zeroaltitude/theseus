# CLOUD_REPORT: `security.v1` in shadow at the gate, step 24 (theseus-0j2.5)

Branch `cloud/20261004-security-shadow`, from `bb205ee` (main at clone, `3add3f5`, plus the task commit). Started
08:46 UTC, report 10:17 UTC. Two commits on top of the task commit:

- `d797100` judge: a judgment's id is the one the core mints at its dispatch, when it gives one
- `84c7ea3` judge: security.v1 and security.v3 in shadow at the gate, the press's labels, and the score on notices

## What I found

- `ToolRuntime::start` plans the call (its `tool.notified` row rides in the planning frame), announces the
  notice, records `GateDecided`, then asks or executes. That is the one place where every gated call that will run
  or ask has its correlation id and its notice already sent, so the gate point goes there, after the notice and
  before the ask or run.
- The JudgeService lives on `TurnRunner`; `ToolRuntime` had no route to it. A `OnceLock<Arc<JudgeService>>` on
  `ToolRuntime`, set in `Core::build`, gives `start` the service with no `TurnCtx` change.
- Tool spans are built after the calls in `turn.rs`'s `trace_calls` from `Ran`; nothing in toolrun has the trace.
  So `Ran` carries the marks, and `trace_calls` adds them as children of the call's span (one `chain` in turn.rs).
- v1 and v3 have different states (`security` against `security2`), so the one decision point sends two Jev calls
  (the batcher shares a call only on a shared state).
- A "should have asked" press arrives by correlation id. Rows are written up to 2 s after a judgment lands (the
  sink's window), and a press may come before Jev has even answered. The convention's minted ids make this easy:
  a gate judgment's id is `jdg_` + the first 16 bytes of sha256(pack ‖ 0 ‖ correlation id), so the press
  computes the call's candidate ids, and keeps each one that is either written (a `bykey` lookup of the LEDGER
  kind) or dispatched-and-not-yet-written (an in-memory set the service keeps, emptied as the sink writes).
- `security.v1`/`v3` share the pack id `security`, so both packs' rows are in scope `judge:security`.

## What I changed

**`d797100`** (theseus-judge): `Ask.id: Option<String>` (`Ask::new` sets `None`); `Judgment::pending` uses it when
set, else mints as before. Test `a_minted_id_is_the_judgments_and_none_mints_one`.

**`84c7ea3`** (core, protocol, CLI, Discord, cockpit):

1. **The gate point**, `crates/theseus-core/src/judge/gate.rs` (`judge/mod.rs` gains the `mod` line, the two
   `WIRED` lines, the pending set and its three helpers; 23a's loop path is untouched). `GATE_PACKS = [security.v1,
   security.v3]`; v2 is not wired (its questions are v3's and would be asked twice). `judged(class, tool, holds)`:
   class not `read`, or a `web::NAMES` tool (`http.fetch`, `web.search`) with `holds()`; the hold is read only for
   those two, and only when a gate pack is on. On the call's path (`ToolRuntime::judge_at_gate`): `gate_on`, the
   choice, the `GateCall` (cloned plan and input), the mode/sample check, the minted ids, the marks, the pending
   insert, one `rt.spawn`. Everything else is in `judge_gate`'s task: `spawn_blocking` for the session's nodes, the
   hold, the input (`gate::input`: tool, class, posture and reason, argv, paths, URL, other args, the operator's
   last ask, the hold as tool/host/minutes, the last 20 calls with outcomes; v3 also `recent_reads`, the newest 6
   `fs.read`/`http.fetch`/`web.search` results, 2,000 chars each, which the builder clips to its 3 and 400 chars),
   `theseus_judge::prepare` (scrub and caps), the blobs, one reservation; then the call; then each judgment
   settles its share of the reservation. `tainted_paths` stays empty.
2. **The record**: each `judge.call` row's `context` has `call` (correlation id), `tool_use_id`, `tool`, `class`,
   `posture`, `posture_reason`, `floor`, `notified`, `hold` (the session held external text when judged),
   `hold_raised` (the hold raised the posture), `waited_on_hold` (raised to approve), `baseline: posture`,
   `decision`, `blob`, `on_path_ms: 0` (the row's `point` is the pack's, `gate`).
3. **The trace**: each dispatch is a zero-length span `judge`, kind `mark`, attributes `pack`, `point: gate`,
   `mode`, `judgment`, `call`, under the call's tool span (`gate::marks`).
4. **Labels**: `JudgeService::press_labels` → `judge.label` rows (`fact::judge::JudgeLabel`; new
   `LedgerKind::JudgeLabel`), §2.5's shape: `id` (`lbl_…`, also the record's key), `judgment`, `pack`, `question:
   "risky"`, `label: true`, `source: operator`, `who`, `via`, `weight: 1.0`, `note: "should have asked"`,
   `correlation_id`; scoped `judge:security`. `rpc/policy.rs`'s new `Core::press` writes them in the tightening's
   frame (`Tightenings::insert_with`); a press on a tool tightened already still writes them, in a frame of their
   own. Every surface's press goes through `Core::tighten`, so the CLI, the cockpit's, and Discord's presses all
   label.
5. **The score on notices**: notification `judge.scored` (`notify::JUDGE_SCORED`, `Event::JudgeScored`,
   `theseus_protocol::judge::JudgeScored` with `line()` = `risk 12% (shadow)`), the fact
   `fact::judge::JudgeScored` (a notification and nothing else), told on the turn's `EventSink` after a notified
   call's v1 judgment is answered. Readers: the CLI (`render/judge.rs::scored_line`: `  ! notified: proc.run · risk
   12% (shadow)`, a line after the notice's two), Discord (`ToolLine.risk`: the tool line's `· 🔔 notified (…) ·
   risk 12% (shadow)`, and a `Risk` field on the notice card when `notice_embeds` is on), and the cockpit
   (`src/lib/scores.ts`: from the push, or from the `judge.call` row once written, so a reload still shows it; a
   pill beside "should have asked" in `Transcript.tsx`). TypeScript regenerated (`JudgeScored.ts`, `index.ts`).
6. Health's judge line lists `security.v1: shadow` and `security.v3: shadow`; tests_judge and theseusd's
   `tests/judge.rs` expectations updated to match. AGENTS.md: theseus-core gains a "Jev's judgments" bullet;
   cockpit's lists `scores.ts`.

Long files: `turn.rs` +5 lines (3457/3500), `protocol/src/lib.rs` +2 (2556), `theseus/src/render.rs` +2,
`theseus-discord/src/render.rs` +~55 incl. a test (≈2840/2930). No ceiling raised. No new dependency.
`MANIFEST_FORMAT` not bumped: no stored record gains a field (judgments and labels are ledger rows, as §2.5 says).

## How I proved it

`crates/theseus-core/src/tests_security.rs`, against the fake Jev, a real core, the web tools on a local server:

| test | shows |
|---|---|
| `with_jev_the_gate_decides_as_it_does_without` (proptest, 8 cases, fresh seed) | over posture {open, notify, approve} × hold × floor (`op whoami` vs `echo hi`) × scripted `risky`/`steered` {0, .01, .5, .75, .99, 1}: the gate's verdict, decision record (posture, reason, notify, floor, external), and whether it waits equal the judge-off core's, for the judged call and for a second call gated after the first's judgment landed; the v1 row carries the scripted score |
| `in_a_holding_session_a_call_scored_one_percent_still_waits` | `risky` 0.01, still waits; row: `posture approve`, `hold`, `hold_raised`, `waited_on_hold` true; id = `judgment_id(v1, call)` |
| `the_floor_still_asks` | `op whoami` at `open`, scored 1%: asks, `floor: true` |
| `a_search_is_judged_only_in_a_holding_session` | clean: only the following `proc.run`'s 2 judgments, `of_call(search)` empty; holding: the search's 2 judgments, `class read`, `hold_raised false` |
| `a_press_with_a_call_labels_its_judgments_in_its_frame` | Jev slow 800 ms; pressed before any row exists: 1 frame, 2 labels; after the rows, tool already tightened: 1 frame, 2 more; each label names a row whose `context.call` is the pressed call |
| `a_notified_calls_score_follows_its_notice` | `risky` 0.37 → `judge.scored` after `policy.notified`, `risk 37% (shadow)`, once (v1 only) |
| `with_jev_slow_tool_started_comes_as_fast_as_with_the_judge_off` | Jev `Slow(5 s)`: `tool.started` within off + 1.5 s and < 3 s; the turn < 4 s |
| `a_judged_tool_loop_keeps_its_frame_budget_and_marks_its_trace` | an `fs.write` loop: the trace's own `frames` equal judge-off's (≤ 9); two zero-length `judge` marks with pack/point/mode/judgment; each names a written row |

Plus `judge::gate::tests` (Q12's choice; the id), `protocol judge::tests` (percent, line), the CLI's
`a_notices_score_follows_it_as_risk_n_percent_in_shadow`, Discord's
`a_notified_calls_score_rides_on_its_tool_line_and_card` (embeds on and off), and the cockpit's
`test/scores.test.ts`. T1's floor tests (`tests_external`, `tests_policy`) pass unchanged in the gate's suite.

- Targeted runs: tests_security 8/8; tests_security + tests_judge 17/17; protocol, judge, CLI, Discord, registry
  359/359; cockpit `npm test` 24/24, lint and build clean.
- **Under load** (four `sh` busy loops at nice 0, the tests at `nice -n 19`, loops killed by pid): tests_security
  + tests_judge, 3 runs: 17/17, 17/17, 17/17 (79 to 87 s each, the property test slow).
- **Planted revert 1**: `at_gate` runs the judgment with `block_in_place(|| rt.block_on(judge_gate(…)))` instead of
  `rt.spawn`: `with_jev_slow_tool_started…` fails, `on 5.030654561s, off 19.970888ms`. Restored, touched, `git
  status` clean of it.
- **Planted revert 2**: `judged` returns `class != Read || NAMES.contains(tool) || holds()`:
  `a_search_is_judged_only_in_a_holding_session` fails, `the run's two judgments, and no more: left 4, right 2`.
  Restored (cmp identical), touched.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` for each commit: fmt, shape, features, clippy, cockpit, test build,
reader rule all pass. The suite fails only on cases not mine; I ran the phases after it by hand (protocol types,
the turn bench `--runs 5 --burst 0`, `cargo deny --offline check`): all ok, frames 5 and 9.

For `84c7ea3`: 1,975 tests, 1,941 passed, 1 flaky (`theseus-sim the_kernel_holds_its_invariants_under_seeded_faults`,
passed on its retry), 34 failed:
- 33 L1 sandbox tests (`theseus-sandbox::contract clause_*`, `egress_18b_*`, …; `theseusd::sandbox *`): the VM
  runs as root (theseus-pv6i).
- `theseus-core tests_output::the_cores_output_matches_its_golden`: a wake's time reads `+#:#` here and `-#:#` in
  the golden: this VM's local offset is UTC (+00:00). It fails the same way on `d797100`, where no core file had
  changed, so it is the machine, not this step. Not rewritten.

For `d797100`: the same golden and sandbox cases, nothing else.

## The live check (the maintainer's, with a real key)

On a scratch daemon of this build, fresh state dir, a GLM profile, `[judge] enabled = true`, `[secrets]
jev_api_key = "op://…"`, and `[policy] enforcement = "notify"` (so `proc.run` notifies):

```sh
S=/tmp/sec24; mkdir -p $S/state
theseusd --config $S/theseus.toml --socket $S/sock --state-dir $S/state &   # wait for health to answer
theseus --socket $S/sock health | grep '^judge:'
#   judge: loop.v1: shadow, security.v1: shadow, security.v3: shadow · max live · breaker idle · …
theseus --socket $S/sock profile use glm
theseus --socket $S/sock ask "Run proc.run with argv [\"echo\",\"hi\"], then tell me what it printed."
#   ! notified: proc.run: run `echo hi` in …
#         (enforcement = notify) · should have asked: theseus policy tighten proc.run --call act_…
#   ! notified: proc.run · risk N% (shadow)          <- the score, a moment after the notice
mkdir -p $S/scratch && touch $S/scratch/a $S/scratch/b
theseus --socket $S/sock ask "Delete the files in $S/scratch with proc.run: rm -rf $S/scratch/*"
#   the score line reads higher than echo's
theseus --socket $S/sock judge log      # both packs' rows; context.call names each call
theseus --socket $S/sock ledger --json -k judge.call | jq '.[] | .data | {pack, call: .context.call, posture: .context.posture, risky: (.answers[] | select(.question=="risky") | .band.value)}'
theseus --socket $S/sock ask "Search the web for 'ripgrep ignore crate' with web.search, then run proc.run with argv [\"ls\", \"$S\"]."   # a new session
#   the ls (proc.run ls) waits for confirmation under the hold whatever its score;
#   judge.call rows exist for the web.search (context.hold true) and for the ls (waited_on_hold true)
theseus --socket $S/sock policy tighten proc.run --call <act_… of the echo call>
theseus --socket $S/sock ledger --json -k judge.label
#   two rows (security.v1 and security.v3), source operator, weight 1, judgment = the echo call's jdg_ ids
theseus --socket $S/sock policy untighten proc.run && theseus --socket $S/sock shutdown
```

`fs.list` is a read, so for the hold's wait use a `proc.run` `ls`. In Discord, a notified call's tool line shows
`· 🔔 notified (…) · risk N% (shadow)`; the cockpit's session view shows the pill beside "should have asked".

## Left, uncertain, and for the owner

- **Divergence: a notification for the score** (`judge.scored`), as the brief anticipated; §2.5 gave judgments
  none. Spec Part III and m5-judgment.md §2.8b should say so.
- **The CLI can only follow a notice.** A stream cannot amend the notice line, so the score is its own line,
  `! notified: proc.run · risk N% (shadow)`. It arrives while the turn runs (Jev p50 ~114 ms); a score that lands
  after `ask` has exited is lost to that CLI (the row keeps it; the cockpit reads it from the row).
- **Only notified calls get `judge.scored`**; open and asked calls are judged and recorded, never told. A card
  for an asked call could show the score too, later.
- **Deterministic ids.** Gate judgment ids are a hash, not uuid v7, so they don't sort by time (nothing I found
  relies on that); a call is judged once per pack at `gate`. The inbound and compile sessions mint their own.
- **In memory until written**: the dispatched-not-written set; a crash loses it with the rows it names (no row,
  so nothing to label). A judgment whose prepare fails (budget paused, client not built) removes its ids, so a
  press then labels nothing; its mark stays in the trace naming an id no row will carry (as 23a's skip-before-
  reserve writes no row).
- **The label's question** is `risky` (Eddie's 0-100%) for both packs; a "should have asked" press arguably labels
  the decision, which for v3 may have been `steered`. 25c's report will want to read it either way.
- **v3's recent reads** are `fs.read`, `http.fetch`, `web.search` results by tool name; a job's output (a `cat`
  via `proc.run`) is not a read here.
- **Cost**: two Jev calls per acting call (v1 ≈2k-token cap, v3 3k). At ~$0.042/M tokens that is about $0.0002
  a call; the $1/day shadow limit covers ~5,000 acting calls a day.
- **State build reads the session's nodes whole** (as loop.v1 does), off the call's path; a very long session
  makes it slower, never the call.
- **Telemetry**: the marks are children of tool spans; I did not check how the OTLP exporter treats a `mark`
  child of a tool span (23b's surfaces own `judge` spans).
- Docs for the maintainer: Part III's item for step 24, `docs/status.md`, and m5-judgment.md §2.4/§2.8b (v3 wired
  beside v1, v2 not; the notification; labels' question).
