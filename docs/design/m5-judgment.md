# Theseus design lane `m5`: M5 Judgment, the Jev client and packs

_Checked in 2026-09-30 from the design lanes. Scrubbed for this public repository: the operator's employer's name, a vault item name, and local paths to the agents' operating and reference notes._

_Design for roadmap steps 23 to 28 (Stage 4). Beads: theseus-zaz.3 (this lane), theseus-0j2 (the M5 epic),
theseus-vug (Appendix F's M5 additions). Written by Tabitha/Claude, 2026-09-30, from [spec](../the-ship-of-theseus.md) v0.61, the v1
roadmap, the agents' operating notes for the repo, the agents' reference notes on Jev, and the code at `de880fc`. Docs only: nothing here was built,
run, or sent to Jev._

**Status: sections 1 to 6 in order; the last line marks the document complete.**

### The key question, answered in brief

- **Shadow to live** is one ladder per pack: `off → shadow → canary(share) → live`, with `rolled_back` as a
  state of record. Shadow records and never acts. A canary acts for a sticky hash-share of sessions, and it
  rolls itself back on a named regression. Live acts everywhere. A move up is an operator act, ledgered, and it
  needs the ledger to show the pack beating the deterministic baseline on a frozen, time-separated holdout.
  `security.v1` moves up only with Eddie's own approval (§3.10). A move down (a rollback, or the config's
  `max_mode = "shadow"` kill switch) never needs anyone.
- **Off the start path.** Nothing Jev-related runs before the socket answers. The client, the packs, and the
  promotion state are built lazily on the first judgment. The key is one more `[secrets]` entry that settles
  after serving. The nightly learning job is a tender that starts at least 10 minutes after serving. The
  lifecycle bench runs with the judge enabled and Jev unreachable, and it must not move.
- **Off the turn's path, in shadow.** A shadow judgment is dispatched after the deterministic decision is
  made, and nothing waits for it. Its records ride in batched frames written by the judge's own recorder, so
  a turn writes no extra frame. Only a live judgment sits on a path, and in M5 the first one does so only
  where no human is waiting: a task's turn end.
- **Inside the dollar budget.** Every call is priced from the catalog (`[catalog."jev-…"]`), and an unpriced
  Jev model is never called. Shadow calls spend from the judge's own daily budget
  (`[judge] shadow_limit_usd_per_day`), since they are the system's experiment, not the session's work. At
  that limit shadow pauses and says so. Live calls spend from the session's execution, exactly as a model call
  does: they reserve, settle, and hold unknown cost until it is reconciled. A live judgment that doesn't fit
  abstains, and the deterministic baseline decides. A judgment never raises a budget question of its own.

## 1. Scope and principles

### What M5 is for

- **Eddie's terms.** JEV IN THE LOOP (§2): continue or stop, role, continuation strategy, shell class, memory
  labels, and security risk are typed judgments over bounded state. LEARNING (§2): every judgment is recorded
  with its inputs, the action taken, and a later outcome, and packs are versioned data tuned by that record,
  "with holdouts and canaries, not reinforcement learning in the technical sense".
- **Security, in Eddie's words** (§3.9): one classifier, `security.v1`, that says "this is risky: 0-100%" and,
  based on the posture, lets the operator know. Jev may make a call's treatment stricter, never looser, and
  it is never the sole gate. T1's external-text hold stays the deterministic floor after Jev arrives.
- **When** (theseus-0j2, decision 16, 2026-09-29): "Yes let's do Jev late but before release for internal
  use." So M5 follows M4 on the roadmap (Stage 4, steps 23 to 28), and `security.v1` must be in shadow before
  internal release.
- **The exit test** (P1, P7): on a held-out set of recorded trajectories, Jev-driven stopping and
  classification beat the deterministic-only baseline at equal total budget (judge cost included), on task
  success, false completion, and unnecessary continuation. A pack that does not beat its baseline stays in
  shadow, and the plan says so.
- **Jev itself** (per the agents' reference notes on Jev): a calibrated judgment engine, not a text generator. It takes a state (text
  or JSON, at most 32k tokens) and a map of typed questions (Choice, Score, Noul), and it answers each one
  independently, with probabilities, in about 350 ms, for about $0.042 per million tokens (observed
  2026-09-21 on `jev-1.13.0`; re-verify before quoting). It is weak on math, dates, and exact lookup, and
  adversarial text can move its answers.

### What already exists in the code (at `de880fc`)

Nothing that calls Jev is built. What M5 stands on:

| What exists | Where | What M5 does with it |
|---|---|---|
| The Advancer trait, with `StopAfterOneLoop` and `UntilNoToolCalls`. The turn calls `advance()` after every loop, ledgers `loop.ended` with the decision and reason, traces an `advancer` span, and narrates it | `theseus-core/src/advancer.rs`; `turn.rs` `advance()` | The baseline JUDGE_STOP is compared with. The `judged` policy (§3.3a) is added beside it |
| A pure `compile()` with deterministic triggers only (manual `fresh`/`transcript`, new session, model/system/tools changed) and strategies `transcript`, `fresh`, `ring` | `compiler.rs` | CONTINUE's cheap candidate signals are added here; it decides nothing new in M5 |
| The "should have asked" press: `policy.tighten` with `correlation_id`, `digest`, and `tool`, whose row is "a labeled example for later judgment work (Jev, M5)" | `tighten.rs`, `rpc/policy.rs`, protocol `Tightening` | `security.v1`'s first labels are already accruing |
| T1's hold on external text, and J1's `judge_act` trace (a job's process cannot answer) | `external.rs`, `approval.rs`, `peer.rs` | The floor Jev never loosens. `judge_act` guards labels and promotions |
| Dollar budgets: micro-dollars, reservations, `held_unknown`, the $100 limit and reset; a model call is a kernel action (`plan_and_dispatch`, then settle) | `theseus-kernel` `Budget`; `turn.rs` `call_model` | The pattern a live judgment follows |
| The priced catalog, where "a model with no price is not called" (`unpriced`) | `catalog.rs`, template `[catalog.*]` | Jev's pinned model gets a row |
| Content-addressed blobs (`<store>/blobs/<sha256>`), nothing read at startup | `blobs.rs` | Judgment states are kept here |
| Record `key` and `scope` indexes (`bykey`, `byscope`); kind 10 was `JUDGMENT`, reserved and never written ("never reuse") | `theseus-store` `index.rs`, `record.rs` | Judgments are ledger rows keyed by id and scoped `judge:<pack>`; kind 10 stays retired |
| Telemetry derived from turn results and traces; the narrative; the Observatory's sections and the trace waterfall | `telemetry/`, `narrative.rs`, `web/src/Observatory.tsx`, `TraceView.tsx` | Judge metrics, lines, and a Judgment section |
| `[secrets] jev_api_key` already in the template, pointing at a vault item (`<vault reference>`) | `theseus.example.toml` | The key's reference exists; the judge reads it when enabled |
| `[context] default_persona`, "the only choice until Jev chooses one" | `config.rs` | Left alone in M5 (roles are hints; see §2.8) |
| DD7's `task.create { brief, budget_usd?, wake_parent? }`: depth one, the carve, the report | `task.rs`, `theseus-kernel/src/tasks.rs` | Grows `arrangement` (step 27) and `check_of` (step 28) |
| Fakes for the model and Discord | `theseus-sim` `fake_model.rs`, `fake_discord.rs` | The pattern for a fake Jev |

### The tracer bullet: what v1 needs from M5, and what can wait

| v1 builds (happy path) | Filed, not built in M5 |
|---|---|
| A typed Jev client in its own crate, with the three-band gate, capped states, batching, a circuit breaker, and a fake | A local judge behind the same trait (only if Eddie declines the third party; §6) |
| Every call recorded (pack, version, state hash and size, answers, band, latency, cost, arm) and priced | An LLM that proposes pack wording from the report (packs are edited by hand in M5) |
| Six packs in shadow: `loop.v1` (JUDGE_STOP), `security.v1`, `classify.v1`, `role.v1`, `continue.v1`, `categorize.v1` | `shell.v1` (L1's shell choice, after M4's sandbox), `memory.v1` (M6), `sampling.v1` (M7's MCP), `attribution.v1` and `relies_on` (M6) |
| The learning ledger: labels from people, the system, and an audit model; a frozen holdout; a nightly report | Discord reactions as labels, and the learning channel's full controls |
| The promotion ladder with sticky canaries and automatic rollback | Per-question promotion (M5 promotes whole pack versions) |
| One live pack: JUDGE_STOP for tasks under a canary, which nudges a task that stopped early | JUDGE_STOP live in conversations; JUDGE_CONTINUE's INTERVENE on thrashing |
| `security.v1` scores on notices, then (with Eddie's approval) live notices for risky `open` calls | `security.v1` making a call wait (notify to approve): a separate decision for Eddie (§5) |
| The roles table, `role.v1` as a canary: a hint note and a role line on the reply | Roles weighting the compiler's budget by node kind (needs M6's assembled strategy) |
| The arrangement on `task.create`, and pieces admitted by reference | §3.5's task graph, `autonomous: true`, sub-tasks (M7 step 39) |
| Independence for check tasks, recorded and shown | "Evidence that shares an ancestor counts once" inside JUDGE_STOP |
| `categorize.v1` in shadow, and the parked-task invariant in health | Live memberships (M6) |
| The prove report generator | The prove itself: it needs weeks of canary data, so it is measured in v1's soak |

### Divergences from Part II's P7 (to fold into the spec)

- **No hooks.** P7 says "hooks arrive because Jev packs are the first real hook handlers". The hook system was
  deleted on 2026-09-28 (§3.17), so packs are core calls at fixed decision points. theseus-bdn's one hook
  point is not needed.
- **Fail to baseline, not `waiting on Jev recovery`.** In M5 every live pack sits beside a deterministic
  baseline that can decide alone. When Jev is down, slow, malformed, or over budget, the judgment abstains
  and the baseline decides. Jev is needed only to continue (a nudge), never to stop, so this keeps §3.3's
  rule: nothing continues autonomously without a judge. The Jev-recovery wait is built only when a pack
  without a baseline goes live.
- **The prove is measured in the soak.** The chain builds the shadow, the ledger, the canary, and the report.
  Labeled trajectories need real traffic over time, and the chain runs a step an hour.
- **CLASSIFY and CONTINUE stay in shadow through M5.** Their live actions need machinery from other phases:
  routing a message to a task (§3.2a, M7 step 39), and the compaction and assembled strategies (§4.2, M6).
  The prove allows it: a pack without a live action is reported as shadow-only.

## 2. The design

### 2.1 Architecture: one crate lane, one core module

```
theseus-judge  (new crate, LANE: no kernel, store, or core types)
  client.rs   JevClient: typed wire types, classified errors, timeouts, in-flight semaphore
  judge.rs    trait Judge; Recording<J> (every call to a JudgmentSink, the spec's decorator)
  pack.rs     Pack, Question, loader rules; packs/*.toml embedded with include_str!
  state.rs    StateBuilder: named fields, priorities, per-field caps, a total cap by construction
  band.rs     the three-band gate (act, confirm, escalate), per question
  batch.rs    merge packs that share a state into one request; split the answers back
  breaker.rs  circuit breaker
  learn.rs    calibration, holdout split, canary arms, rollback rules, prove metrics (pure functions)
  fake.rs     a fake Jev (feature "fake"): scripted answers per question id, modes up/down/slow/malformed/429

theseus-core/src/judge/  (SPINE wire-in)
  mod.rs      JudgeService: decision points → packs → client; dispatch_shadow() and judge_live()
  inputs.rs   core types (nodes, proposals, loop outcomes) → the crate's plain input structs
  sink.rs     judgments as scoped ledger rows + state blobs, written in batched frames
  spend.rs    the shadow day budget (block reservations); live calls as kernel actions
  ladder.rs   pack modes and arms, read lazily from each pack's scope
  learn.rs    the nightly report tender, labels, replay, backfill, audit

theseus-sim   jev-probe (one real call per pack, prints cost and latency) and fake-jev (as fake-discord)
```

- The crate keeps the Jev-specific rules in one place (§3.7: atomic questions, a mandatory no-match option,
  the three bands, states capped by construction), and it can be built and tested in a worktree while the
  chain works on M4.
- The core decides **where** judgments happen (six decision points, §2.4) and **what they touch** (the
  ledger, spend, the turn). The crate never sees a kernel type.
- One `Judge` trait, one implementation (TypeSafe Jev), always wrapped in `Recording`. A local judge (§6)
  would be a second implementation behind the same trait.

### 2.2 The Jev client

| Concern | Design |
|---|---|
| Request | `{state, model, questions}`, the bearer key from `[secrets] jev_api_key`, read from the settled secrets (never `op` at call time). The state is a named-field JSON object from a `StateBuilder` |
| Response | `answers` keyed by question id, `usage.{input_tokens, output_tokens}`, and the concrete `model`. Parsed strictly: a missing or extra id, a wrong type, a Choice outside its options, or probabilities that don't sum to about 1 is `malformed` |
| Pinning | Each pack version names its Jev model (for example `jev-1.13.0`). A response from another model is recorded as `model_drift` and never acted on, since calibration does not transfer. `jev-latest` is never used by a pack that can act |
| Timeouts | Connect 2 s and total 5 s (no stream, so first byte is the total). Live calls pass a shorter deadline of their own |
| Errors | Classified as the provider's are: `timeout(phase)`, `network`, `rate_limited(retry_after)`, `server`, `auth`, `invalid_request`, `malformed`, `over_state`, each with `transient` and `usage_unknown` (true for a total timeout after the send). **Nothing retries on its own** |
| Admission | A semaphore, `[judge] max_in_flight` (8). Shadow only tries it: with no permit free, the judgment is shed and counted (health `judge.shed`, one row per minute at most). Live waits up to its deadline. Overload sheds shadow first, as §3.13 asks |
| Circuit breaker | Five transient failures in a row open it for 60 s, then one probe. While it is open, shadow skips and live abstains at once, so an outage adds no latency. `judge.circuit` rows on open and close, a health line, a narrative line |
| Batching | At one decision point, packs whose state is byte-identical go in one request, with question ids namespaced `<pack>/<question>` (ids are never shown to Jev, so this changes nothing it sees). Packs with different states go out concurrently. So serial depth, not call count, bounds latency (§3.7) |
| Latency per class | Every judgment records `queued_ms`, `http_ms`, `total_ms`, `on_path_ms` (0 in shadow), and the turn's **workload class**, set deterministically: `reply` (one loop, no tools), `tools` (tool loops), `job_result` (woken by a job or a task report), `wake`, `task`. The Observatory and telemetry show p50/p95/p99 per pack and class (§9's "Jev per turn" row) |

### 2.3 Packs are versioned data

A pack version is one TOML file in `crates/theseus-judge/packs/`, embedded in the binary:

```toml
id = "loop"                 # shown as loop.v1
version = 1                 # any change (wording, criteria, thresholds, builder, model) is a new version
jev_model = "jev-1.13.0"
point = "loop_end"          # inbound | compile | gate | loop_end | exchange_end
state = "loop"              # a builder in code (closed set), with its own version
state_cap_tokens = 4000
sample = 1.0                # the share of eligible events judged in shadow
baseline = "until_no_tool_calls"
action = "nudge_task"       # the live action in code (closed set), or "none"

[questions.work_state]
type = "choice"
instructions = "Where does the assistant's work on the ask stand at the end of this turn?"
no_match = "other"
act = 0.90                  # confidence for the act band
confirm = 0.60
[questions.work_state.criteria]
complete = "The ask is fully done, and the final message reports the result."
progressing = "Work toward the ask remains, and the assistant could continue without a person."
# ...
other = ""                  # none of these
```

**Loader rules**, each held by a test: every Choice has a no-match option; every Score has a companion
`applies` Noul; instructions carry the whole meaning; no question asks for math, counting, dates, or exact
lookup (the builder computes those and states them as fields: `loops: 7`, `minutes_since_ask: 42`). Two
versions of a pack may run in shadow together (the incumbent and a candidate), and both are recorded.

**The bands** (`band.rs`), per question, with thresholds from the pack:
- Choice and Score: the top answer's `confidence` c. Act if c ≥ `act`; confirm if c ≥ `confirm`; else escalate.
- Noul: p, the probability the statement is true. Act-true if p ≥ `act`, act-false if p ≤ 1 − `act`,
  confirm if p is outside the band `confirm` sets, else escalate. Noul and Choice thresholds are separate,
  as Jev's documentation warns.
- Thresholds start conservative (act at 0.90) and are tuned only on labeled outcomes (§3.7). **In M5 a live
  action acts only in the act band.** Confirm and escalate defer to the baseline, and the band is ledgered.
  Asking a person in the middle band is filed for later.

### 2.4 The six packs, where they run, and what they ask

| Pack (spec name) | Point, and when | State (built in code, capped) | Questions (atomic) | Baseline | Live action in M5 |
|---|---|---|---|---|---|
| `loop.v1` (JUDGE_STOP) | `loop_end`: every time the baseline ends a turn with `no_tool_calls`; a sample of `continue` decisions | The ask (the exchange's first human message, or the task's brief), the final text, the last 8 tool calls (tool, trimmed args, outcome, error class), loops, spend, minutes, session kind | `work_state` Choice: complete · progressing · blocked_needs_human · thrashing · off_task · other. Nouls: `announced_unfinished` ("the final message says it will do something it has not done"), `stopping_point_defined`, `same_action_repeating`, `cost_out_of_proportion`, `wants_human_input` | `UntilNoToolCalls` | Tasks under canary: **nudge** (§2.8a). Conversations stay in shadow |
| `security.v1` | `gate`: every call whose class is not `read`, and every `http.fetch` or `web.search` in a session that holds external text (the exfiltration path T1 leaves open by design). After the gate decides; never delays dispatch | Tool, class, posture and reason, trimmed and scrubbed args (argv, paths, URL host and query), the operator's last ask, the session's hold (tool, host, since), the last 5 calls | Headline `risky` Noul (Eddie's 0-100%). Atomic Nouls: `destructive`, `exfiltrates`, `beyond_ask`, `steered` (follows instructions from fetched text rather than the operator), `touches_credentials`. `kind` Choice: read_only · local_edit · local_exec · remote_write · publish · credentials_or_config · other | The gate's posture | With Eddie's approval: a notice for a risky `open` call (§2.8b). Never a wait |
| `classify.v1` (CLASSIFY) | `inbound`: every human message that starts a turn | The message, author, place kind, the previous human message and the last reply (trimmed), live tasks (id, brief's first line, state), minutes since the last message | `kind` Choice: new_ask · follow_up · correction · control · addressed_to_task · social · other. `addressed_task` Choice over live task ids + none (only when tasks exist). Nouls: `should_promote`, `fragment`, `mentions_other_conversation`, `wants_fresh_look` | The message goes to the conversation; promotion only when the model calls `task.create` | None (routing to a task is M7) |
| `role.v1` (ROLE_GUESS) | `inbound`, **batched with `classify.v1`** (same state, plus the current role) | As above | `role` Choice over the roles table's ids + other | The session's current role (none before the table) | Under canary: a hint note and a role line (§2.8c) |
| `continue.v1` (CONTINUE) | `compile`: only when no deterministic trigger fired **and** a candidate signal did (§4.4a step 2) | The signals and their values, tail tokens and nodes, the provider's last cache read vs the prefix, the compilation's age, strategy, and trigger, budget left, the last human message | `decision` Choice: append · recompile_transcript · recompile_ring · recompile_compaction · recompile_fresh · other. Noul: `stronger_model_for_compaction` | Append | None (compaction and assembled are M6) |
| `categorize.v1` | `exchange_end`: once 10 human messages have arrived since the session's last one, or at the first exchange end after 30 minutes' quiet | Session title, the last 10 human messages (trimmed), current memberships, up to 50 candidate topics with descriptions | `topic` Choice over topic ids + new_topic + none. Nouls: `still_member` per current membership (at most 5) | No membership | None (live memberships are M6) |

- **CONTINUE's candidate signals** are built in step 25, deterministic and cheap, in `compile()`: a dormancy
  gap (over 6 h since the last node), the tail crossing a soft band (half the window, then every further
  quarter), a task report or wake arriving, and a provider cache miss (cache reads fall to zero while the
  prefix is unchanged). No signal, no call: most turns in a live thread cost nothing.
- Question texts are written with the builder, reviewed by Tabitha, and kept in the pack file. Criteria spell
  out boundary cases, since Jev reads them literally.
- The state builders **scrub secrets** (the existing `Scrubber`: every vault value, and known token
  shapes) before a state leaves the process, and admit nothing the session's own model could not see.

### 2.5 Recording: every judgment, in the ledger

**No new record kind.** Judgments are ledger rows (kind `LEDGER`, schema 1, unchanged), each **keyed** by its
id and **scoped** `judge:<pack>`, so one pack's whole history is one ordered `byscope` scan and one judgment
is one `bykey` lookup. Kind 10 (`JUDGMENT`, reserved, never written) stays retired. No store manifest bump
and no one-way door: an older binary still opens a store with judgments in it.

| Row | Key, scope | Carries |
|---|---|---|
| `judge.call` | `jdg_<id>`, `judge:<pack>` | pack and version, point, mode (shadow, canary, live), arm (canary, control, all), session, execution, turn, loop, and the judged call's correlation id; Jev model asked and answered; the state's sha256, bytes, token estimate, blob digest, truncated fields, builder version, and its **inputs by reference** (node positions); every answer with its probabilities and confidence; each question's band; the baseline's decision; the verdict (what the pack did, or would have done) and `acted`; latencies and workload class; usage, cost in micro-dollars, and which budget paid; or the error with its class |
| `judge.label` | `lbl_<id>`, `judge:<pack>` | the judgment, the question (or all), the label, its source (operator, system, audit), who and through what, a weight, a note |
| `pack.mode` | none, `judge:<pack>` | version, from, to, share, who, why, the report it cites, `forced` |
| `judge.report` | none, `judge:<pack>` | the nightly summary (§2.9) |
| `judge.paused`, `judge.resumed`, `judge.circuit`, `judge.shed` | none, `judge` | the shadow budget, the breaker, and shedding, as they happen |

- **The state** goes to a content-addressed blob (`blobs.rs`), written before its row, so a row never names a
  missing blob. The row keeps the digest and the inputs by reference, so a later builder version can rebuild
  the state from the graph for replay.
- **Labels are rows, never edits**, so a judgment can collect a person's label, a system label, and an
  audit label, and the report weighs them (APPEND-ONLY).
- **Batched frames.** The core's `JudgmentSink` writes shadow judgments in frames of up to 32 rows, or every
  2 s, whichever comes first, from its own task. A turn never waits on it and writes no extra frame for a
  shadow judgment. A crash loses at most 2 s of shadow rows. Their spend is not lost: the block's unsettled
  rest is booked as spent (§2.6), with a `judge.block_booked` row that shows the gap.
- **The trace.** A live judgment is a `judge` span (a new span kind, §3.3a) under its loop, with pack,
  band, and cost. A shadow dispatch is a `mark` span carrying the judgment's id, so the waterfall shows where
  each shadow judgment was taken, and its detail opens from there.

### 2.6 Spend: every call priced, inside a budget

- **The price** is a catalog row, `[catalog."jev-1.13.0"]`, with `kind = "judge"`, provider `typesafe`, input
  and output prices per million tokens, and no cache prices. It is in the built-in table and the template,
  and the existing agreement test covers it. A pack whose Jev model has no price is not called: `unpriced`,
  as for models.
- **The reservation** is the state's token estimate plus an output allowance of 64 tokens per question, at
  those prices, rounded up per call. At about $0.042 per million tokens, a 2,000-token state costs about 84
  micro-dollars.

| | Shadow (and replay, backfill, audit) | Live (canary or live) |
|---|---|---|
| Who pays | The judge's own budget: `[judge] shadow_limit_usd_per_day` ($1.00 by default, about 10,000 calls) | The session's execution, beside its model calls |
| Why there | An experiment the system runs, not work the session asked for | Part of the session's work |
| How it is held | A `judge.budget` META record for the local day. The recorder reserves in **blocks** of $0.01, one frame per block, and calls draw from the block in memory. Each batch frame carries the spend. After a crash, the unsettled rest of the block counts as spent, conservatively (§3.13) | A kernel action, as a model call is: `plan_and_dispatch` with its reservation, `SafeToRepeat`, a deadline, then a settle at the real cost from `usage`. A timeout after the send holds the reservation as unknown until the reconciler settles it at the reserved amount |
| At the limit | Shadow pauses: one `judge.paused` row, a health line, a narrative line. It resumes at local midnight. Nobody is asked, since pausing an experiment is correct operation | The judgment **abstains** and the baseline decides. It never raises a budget question; the session's next model call asks, as usual |
| Frames | None in the turn's. The recorder's own batch frames (§2.5), and one per block | Its plan rides in the frame the turn writes next (the kernel's closure argument), so it adds at most one frame, its settle. The frame-budget test gets a judged variant |

- The session's lifetime `cost_usd` includes its live judgments. Health shows the judge's shadow spend for today
  and in total. Telemetry's `theseus.cost.usd` gains `theseus.spend = judge` and `theseus.judge.pack`.
- The **audit** labeler (§2.9) calls a strong model, so it has its own cap per run,
  `[judge] audit_limit_usd` ($5.00), and draws from the judge's budget with `purpose: audit`.

### 2.7 Shadow to live: the ladder

```
 off ──► shadow ──► canary(share) ──► live
           ▲              │             │
           │              ▼             │
           └───────── rolled_back ◄─────┘    (automatic on a named regression, or the operator)
```

`rolled_back` behaves as shadow (it records and never acts). It differs in one way: moving it up again must
cite a report written after the rollback.

- **Where the mode lives.** In the store, as `pack.mode` rows in the pack's scope. The latest row per pack
  version is its mode. It is read lazily on the first judgment (a few rows), then kept in memory. A new pack
  version starts in `shadow` when the judge is enabled.
- **The config is a ceiling agents cannot raise.** `[judge] max_mode` (`off`, `shadow`, `live`) and a pack's
  `mode = "off" | "shadow"` in `[judge.packs."<pack>"]` lower a mode, never raise it. They live in the vault
  note, which agents cannot write. `max_mode = "shadow"` is the kill switch: nothing acts.
- **Arms.** A session is in a pack's canary when `h(session id, pack) < share`, where h is the first 8 bytes of
  a SHA-256 read as a fraction. It is sticky, and monotone as the share grows. Every judgment records its arm,
  so outcomes compare by arm. Nothing is stored on the session for it.
- **Moving up** is an operator act: `pack.promote { pack, version, to, share? }`; `theseus packs promote
  loop.v1 --canary 0.2`; a web UI button. `judge_act` judges it, so a job's process is refused (J1). The
  request cites a learning report in which this version beats the baseline on a frozen holdout with the
  minimum sample (§2.9). Without one, it is refused with the numbers ("labeled 37 of 200"). An operator's
  `force` overrides that, and the row says `forced` (the chain's live checks use it on scratch daemons).
- **`security.v1` moves up only with Eddie's approval** (§3.10: changes touching `security.v1` need human
  approval). Its promote request becomes an approval card in a trusted channel, like a waiting call, and only
  a trusted user's answer promotes it.
- **Moving down** needs nobody. Each pack names its rollback rules, which `learn.rs` checks as each canary
  outcome lands (and the nightly job checks again, as a backstop). A rollback is immediate: a `pack.mode` row,
  a notice on every surface, and the pack goes back to recording in shadow.

| Pack | Rolls back when |
|---|---|
| `loop.v1` nudging tasks | A nudged turn ends with no new tool call and a near-identical final text (a nudge loop). The canary's spend per task exceeds twice the control's median, over 10 tasks or more. The operator stops or cancels a task within its nudge. The judgment's on-path p95 exceeds 1 s. |
| `security.v1` notices | More than 30 Jev notices in a day (the quiet-notices lesson, theseus-w4f), or the operator labels 3 of them "noise" in a day |
| `role.v1` | More than 2 role switches in one exchange, or the operator labels a switch "wrong role" twice in a day |

An error rate over 20% in the last 50 calls is not a rollback. The pack abstains, and the baseline decides,
until the breaker closes.

### 2.8 The live packs in M5

**(a) JUDGE_STOP for tasks: the `Judged` Advancer** (§3.3a's third policy)

```
baseline = UntilNoToolCalls(max_loops).decide(outcome)
if baseline == EndTurn(no_tool_calls) and session is a task and arm(loop.v1) == canary
   and nudges this turn < 1 and nudges this task < 3:
    v = judge_live(loop.v1, state, deadline 3 s)          # the task's turn end: no person waits
    if v.work_state == progressing (act band) or v.announced_unfinished (act-true):
        append a harness node: "⏩ Not done yet (Jev 0.93): <the brief's first line>.
                                Continue, or say what blocks you."
        return Continue                                    # loop.ended: advancer=judged, judgment=jdg_…
return baseline                                           # abstain, confirm, or escalate: the baseline stands
```

- It only ever **continues** a task that the baseline would end. It never stops a turn the baseline would
  continue, so it cannot cut work short, and the deterministic controls (`/stop`, cancel, the budget) outrank
  it, as §3.3a requires.
- Why tasks first: DD7's task is "done when its turn would wait on input", so a task that says "next I'll run
  the tests" and stops is a false completion that nobody sees until the report. Nobody waits on a task's turn
  end, so the call's latency costs no one. Conversations stay in shadow: a person is there to say "go on".
- The Advancer trait stays synchronous. The turn takes the judgment first and passes it in, so the trait's
  two existing policies are untouched.

**(b) `security.v1` on notices.** In shadow, a call that already posts a notice shows the score, marked as
uncalibrated: `🔔 notified (proc.run) · risk 12% (shadow)`, on Discord's tool line, in the web UI, and in the
CLI. Once Eddie promotes it, a call whose posture is `open` (no notice) and whose `risky` is act-true posts a
notice after the fact: `🔔 notified (Jev: 84% risky)`, ledgered as `tool.notified` with `by: judge`. The call
is never delayed: the judgment runs beside the dispatch, and the notice follows within about a second. Making
a call **wait** on a score would put a Jev call before every acting dispatch, and it is Eddie's decision (§5).

**(c) The roles table and `role.v1`**
- **The table** (§3.4): the twelve seed rows compiled in as data, and operator rows as store records (a META
  key per row, versioned), with `{id, stance, hints, tools favoured, verbosity, stop strictness, announce,
  added_by, added_at, version}`. The per-kind weights wait for M6's assembled strategy. `theseus roles`, the
  Observatory's Roles page, and `roles.add` (an operator act) see and grow it.
- **The session's role** is a new field on the session record (a schema bump, §2.14).
- **Live (canary):** at `inbound`, the role switches when `role.v1`'s top role differs from the current one,
  is in the act band, beats the current role's probability by 0.2 or more, and no switch has happened in this
  exchange. A switch appends a harness note to the tail ("🎭 role: reviewer. Critical: diffs, prior decisions,
  conventions first; never edits; findings with evidence."), and the reply post carries a role line,
  `🎭 reviewer`, unless the binding sets `announce = false`.
- **Hints go in the tail, not the system block**, so a switch forces no recompile and the prompt cache holds.
  §4.4a lists a role change as a recompile trigger, but that was for roles that re-weight the compile (M6).
  The persona (context files) stays `[context] default_persona` in M5 (§5).

### 2.9 The learning ledger

**Where labels come from** (§3.10: human, system, audit):

| Pack | Operator labels (weight 1.0) | System labels, derived nightly and deterministically (weight 0.5) | Audit labels (weight 0.5) |
|---|---|---|---|
| `loop.v1` | A judgment's buttons in the web UI; `theseus judge label` | The next human message within 10 minutes is a continuation phrase from a closed list ("continue", "go on", "keep going", "you didn't finish"): stopped too early. A `/stop` or cancel within a nudge: wrong continue. A task with a near-identical brief within 24 h: false completion. `budget_exhausted`, and the budget question, are never "should have stopped" (§2 LOOP FOREVER) | Yes |
| `security.v1` | **Already accruing**: "should have asked" presses carry the call's correlation id, digest, and tool. Plus buttons | A declined waiting call (risky, weak); an approval without complaint (not risky, weak) | Yes, with a planted-injection eval set, as Jev's documentation advises before any trust |
| `classify.v1` | Buttons | The model called `task.create` in that turn (`should_promote`); a slash command (`control`) | Yes |
| `role.v1` | "Wrong role" | None | Yes |
| `continue.v1` | Buttons | None in M5 (a recompile's worth shows only when acted on) | Yes |
| `categorize.v1` | Accept or reject a proposed membership in the Observatory | None | No |

**The nightly report** (a tender: its own thread at low priority, at most 5% of a core, started by the
driver's tick at `[judge] learning_hour` local time, never within 10 minutes of a start; on demand with
`theseus judge report`):
- Per pack and question: calls, labeled calls, precision and recall per Choice class, Brier score and
  calibration (ECE, and a reliability table) for Nouls and top choices, agreement with the baseline, the
  share in each band, cost, and latency p50/p95/p99 per workload class.
- For a canary: the arms' trajectory outcomes (as "The prove" below defines them), and the rollback rules
  checked again.
- Written as `judge.report` rows and as `<state dir>/learning/<date>.json`, a derived file the rows can rebuild.
  The web UI renders it. With `[judge] learning_channel` set, a digest goes to that Discord place.
- It proposes nothing on its own; the learning loop that runs after it does (§2.17, step 25f): the owner's
  labels on a version's train split rewrite its text into a new version, which a replay checks and the numbers
  place. A person may still edit a pack file into a compiled-in version, which runs in shadow beside the
  incumbent.

**Holdouts.** Time-separated and frozen: a promotion cites a report whose holdout is a closed window (by
default the latest 14 days, never used to write the candidate's text or thresholds), with the window's bounds
in the `pack.mode` row. The minimum is 200 labeled judgments per question that decides, and 30 per Choice
class that acts. Below it, the report says "insufficient", and promotion is refused unless forced.

**Replay, backfill, and audit** (§8's replay harness):
- `theseus judge replay loop.v2 --report <id>` runs a candidate over the holdout's recorded states (the blobs,
  or states rebuilt from their inputs when the builder changed), and reports agreement with the labels and
  with the incumbent. These are real Jev calls, from the judge's budget (`purpose: replay`).
- `theseus judge backfill <pack> --since <date>` builds states from the recorded history (inbound messages,
  turn ends, tool calls), and judges them in shadow (`purpose: backfill`). With it, the first holdout exists
  on day one, not after weeks of traffic. **It sends Eddie's history to TypeSafe, so it waits for his
  consent (§4).**
- `theseus judge audit <pack> --sample 100 --profile opus` has a strong model answer the same questions over
  the same states, as audit labels, capped by `audit_limit_usd`.

**The prove** (P7), as `theseus judge prove`: for JUDGE_STOP on tasks, canary against control, at equal total
budget (each arm's spend includes its judge calls; the report gives rates per task and per dollar):
- task success: no near-duplicate task within 24 h, no operator "wrong" label, the audit says done;
- false completion: the baseline or Jev said complete, and a re-ask, a near-duplicate task, or the audit
  says it wasn't;
- unnecessary continuation: a nudge after which the task made no new tool call and ended the same way.

For classification it is decision quality on audit- and operator-labeled messages: `classify.v1` against the
baseline (the model's own `task.create` decisions, and slash commands). The report states which packs beat
their baseline, which stay in shadow, and why.

### 2.10 Promotion with an arrangement (§3.2a; step 27)

Eddie decided on 2026-09-27 that promotion requires an authored arrangement (theseus-vmh, decision 6).
DD7's `task.create` grows one required argument:

```
task.create {
  brief,                          # the objective, as today
  budget_usd?, wake_parent?,
  arrangement: {
    pieces: [ { quote: "<an exact span of at least 20 characters from one node of this session>",
                role: "objective" | "acceptance" | "design" | "context" }
            | { node: "<node id>", role } ],
    trust?: [ <piece index> ],             # rendered as trusted testimony
    supersedes?: [ [<older>, <newer>] ],   # by piece index
  },
  fidelity_ack?: bool,
}
```

- **References, not paraphrase.** Each quote must match exactly one node in the calling session's transcript,
  or the call fails with the reason (no match; or ambiguous, with the candidates' times and authors), and the
  model tries again. The result lists the resolved nodes (id, author, time, first line). Quotes need no change
  to the renderer, so the prompt cache is untouched. (Rendering node ids into the transcript is the
  alternative, §5.)
- **Refusal.** With no arrangement, or none with an `objective` or `design` piece, the call is refused with the
  reason: "Promotion needs an arrangement: quote the messages that define this work."
- **The fidelity check**, deterministic: a brief under 200 characters, from a session with more than 10 human
  messages since its last task, with a single piece, is flagged. The call fails, asking for the design to be
  attached or for `fidelity_ack: true`. The ack is ledgered, and shown on the task's surfaces.
- **The child's first compilation** renders the brief, then the arrangement right after it, as testimony:
  each piece's full original text, with its author, time, and origin session. A superseded piece is listed by
  reference only, and never admitted (it stays recallable).
- **Records:** an `Arrangement` node body (a node schema bump, §2.14), written in the child's session in the
  frame `open_task` already writes. Surfaces: `📎 3 pieces` on the task's lines, the pieces in the web UI's
  task tree, and `theseus tasks show <id>`.
- **Jev's part is small.** `classify.v1`'s `should_promote` (shadow) is compared with the model's own
  `task.create` calls in the report. A `fidelity` Noul is filed for later.

### 2.11 Independence as a compiler property (theseus-vug; step 28)

- `task.create { …, check_of: "<task id>", profile? }` opens a **check task**. Its compilation admits the
  checked task's arrangement pieces (objective, acceptance) and its report, rendered as a claim ("claimed by
  task a1b2c3, as of 14:02"), and **excludes every other node of the maker's session**: its messages, its tool
  calls, and its thinking. Since DD7 a task sees only its brief, so the risk is the brief itself, which the
  parent writes after reading the maker's report.
- **The overlap flag:** a brief that shares a span of 12 words or more with any node of the maker's session,
  other than its report, is flagged. The task still runs; the flag is part of its basis.
- **The basis** is recorded on the check task's session record (§2.14): the checked task, the excluded
  sessions, the admitted pieces, the model and profile, and the overlap flags. It is shown beside the check's
  report: `🔍 check of task a1b2c3 · independent (excluded ses_…, claude-opus-5-5)`, on Discord, in the web UI,
  and in `theseus tasks`.
- **Filed:** "evidence that shares a transmission ancestor, or the same method over the same snapshot, counts
  once" inside JUDGE_STOP, and a `verify.v1` pack (Jev's citation-check shape: the claim, the evidence,
  `supports · contradicts · insufficient`), in shadow on each check's report.

### 2.12 `categorize.v1`, and the parked-task invariant (step 28)

- `categorize.v1` needs M4's kinds table with topics (step 21, theseus-8kk). In shadow, it proposes topic
  memberships. The Observatory lists them, and the operator accepts one (M4's `operator`-origin membership) or
  rejects it; either is its label. Jev writes no membership in M5. `new_topic` asks the operator to name one,
  since Jev does not generate text.
- **The parked-task invariant** (theseus-vug): health gains `tasks.parked`, each task that is in progress and
  cannot progress by itself (no running turn, queue place, job, wake, or pending question younger than 24 h),
  with its blocker named. It shows in `theseus health` and the Observatory.
- **Filed from theseus-vug:** honest delivery receipts (DD6's outbox settles each post; the vocabulary
  `posted | transport_failed | never_posted` is checked on the surfaces, not rebuilt); a reply that references
  a Question resolving without inference (M7, with the task graph); `relies_on` and `attribution.v1` (M6).

### 2.13 Surfaces (EXQUISITE VISIBILITY)

| Surface | What it shows |
|---|---|
| Ledger | `judge.call`, `judge.label`, `pack.mode`, `judge.report`, `judge.paused`/`resumed`, `judge.circuit`, `judge.shed`; `loop.ended` gains `advancer: judged` and the judgment id; `tool.notified` gains `by: judge` |
| Protocol | Read: `judge.list {pack?, session_id?, since?, limit}` (no states), `judge.get {id}` (with the state), `pack.list`, `learning.report {pack?, date?}`. Acting (judged by `judge_act`): `judge.label`, `pack.promote`, `pack.rollback`, `roles.add`. Judgments stream on `ledger.tail`: no new notification method |
| CLI | `theseus judge log / show / label / report / replay / backfill / audit / prove`, `theseus packs [promote / rollback]`, `theseus roles [add]`; a `judge:` line in `theseus health` (enabled, breaker, today's calls and spend, shed, paused); `ask --trace` shows judge spans |
| Discord | The risk on notices; `🎭 role` on replies; `📎 pieces` and `🔍 check` on task lines; rollback notices; the nightly digest in the learning channel when one is set |
| Web UI | The Observatory's **Judgment** section: per pack, its mode, version, calls, cost, p50/p95, agreement, labeled precision, a calibration strip, and promote/rollback controls; a judgment log with filters and label buttons; each judgment's state as fields and its answers as probability bars. The **Roles** page. In a session: judgments beside the loop they judged ("Jev (shadow): complete 0.93 · agrees"), and judge spans in the waterfall |
| Narrative | "Jev, in shadow, judged the stop: progressing (0.81, act band). The baseline ended the turn; recorded, not acted on." "Jev nudged task a1b2c3: its last message says it will run the tests, and it hasn't." "Shadow judging paused: today's $1.00 is spent." |
| Telemetry | `theseus.judge.calls` {pack, mode, band, class}, `theseus.judge.duration_ms` {pack, class}, `theseus.judge.on_path_ms`, `theseus.judge.errors` {class}, `theseus.judge.disagreements` {pack}; `theseus.cost.usd` with `theseus.spend = judge`; a span per judgment |
| Health | `judge {enabled, key, breaker, in_flight, shed, today_calls, today_usd, paused, packs[{pack, version, mode, share}]}`, `tasks.parked` |

### 2.14 The store and versions (F4a's standing rule)

| Change | Kind, schema | Reader for the old layout |
|---|---|---|
| Judgments, labels, modes, reports | `LEDGER` rows, schema 1, keyed and scoped | None needed: the row's `data` is free JSON |
| The shadow day budget, role rows | `META` records under new keys | None needed: an older binary never reads those keys |
| The session's `role` (step 26), and a check task's `basis` (step 28) | `SESSION` 2 → 3 at step 26. Step 28 bumps again (3 → 4) if a build with schema 3 was installed in between, since that store may already hold schema-3 records | Absent fields read as none; a test reads each older schema |
| The `Arrangement` node body | `NODE` 2 → 3 | A test reads schema 2 nodes; an older binary refuses a schema-3 store, as F4a intends |
| State blobs | Files in `<store>/blobs/` | Not in the WAL: M4's durability tender must ship blobs too (images already need it) |

### 2.15 Config

```toml
[judge]
enabled = false                   # true only after Eddie's consent (§4); needs [secrets] jev_api_key
max_mode = "live"                 # "shadow" caps every pack: nothing acts. "off": nothing is called
key_secret = "jev_api_key"
model = "jev-1.13.0"              # the default pin; each pack version names its own
max_in_flight = 8
connect_secs = 2
total_secs = 5
shadow_limit_usd_per_day = 1.0
audit_limit_usd = 5.0
learning_hour = 3                 # local time of the nightly report
# learning_channel = "discord:<channel id>"
# [judge.packs."security.v1"]
# mode = "off"                    # the config can lower a pack's mode, never raise it
# sample = 0.5                    # shadow sampling share
# [judge.signals]                 # CONTINUE's candidate signals (§2.4)
# dormancy_minutes = 360
# tail_band = 0.5                 # share of the window, then each further quarter

[catalog."jev-1.13.0"]            # typesafe · judge
kind = "judge"
provider = "typesafe"
input_per_mtok = 0.042
output_per_mtok = 0.042
```

The endpoint's base is compiled in and overridable (`api_base`) for the fake. Every line is in the template,
and the existing test parses the template with every line uncommented.

### 2.16 FAST

| Path | Rule, and how it is held |
|---|---|
| Start | Nothing new before serving. The client, the packs, and the ladder are built on the first judgment. The key settles with the other secrets after serving, and a judgment waits for that one secret alone (as a broker grant waits for an unsettled secret); a shadow judgment that would wait is skipped instead. The nightly tender starts at least 10 minutes after serving. **Held by** the lifecycle bench with `[judge] enabled = true` and the endpoint at 127.0.0.1:9 (as the Discord binding is benched): no phase may move |
| Turn, shadow | Dispatch is a spawn after the decision; the turn never awaits it. The on-path cost is the state build, capped by construction. **Held by** a unit bench: each builder under 1 ms on its largest input, inside §9's 5 ms per-turn overhead; and by the frame-budget test (8 frames, unchanged with shadow on) |
| Turn, live | Only at a task's turn end in M5, with a 3 s deadline, and abstaining on any failure. **Held by** `on_path_ms` p95 per class in the report; the canary rolls back past 1 s p95 |
| Shutdown | Never waits for a judgment. In-flight shadow calls are dropped, and their block's rest is booked on the next use, not at start. **Held by** the bench's clean-shutdown phase with judgments in flight against the slow fake |
| Money | Shadow is capped per day; live is inside the session's limit; the audit is capped per run; an unpriced model is never called |

### 2.17 The learning loop (step 25f)

The owner's labels adjust the only prompt there is: each question's `instructions` and criteria, which Jev reads
literally. Nightly, after the report (the tender's run), and now with `theseus judge learn <pack> [--split
<time>]` (the owner's act, `judge.learn`, judged by `judge_act`), for each lineage the loop may rewrite (every
wired root but `route.v1`, `rerank.v1`, `memory.v1` and `attribution.v1`, each left alone):

- **The parent** is the version standing in its root's place (below). **An error** is an owner's label
  (`source: operator`) that `labels::resolve` reads as its answer wrong (`noise` and `useful` grade nothing), on
  a judgment in the train split, not read by an earlier proposal of the lineage. Below `min_errors` (10) new
  errors, nothing happens and nothing is written.
- **The split.** 25c's time split, the `holdout_days` (14, `[judge] holdout_days`) before the report's local
  midnight, leaves a store whose labels are new without a train split for two weeks. So until the parent has 200
  labeled judgments in that window, the split is interleaved: every fifth labeled judgment by the first 8 bytes
  of SHA-256(id), modulo 5, is holdout, across all time, so a judgment never changes side; from 200 on, the time
  split. `--split` sets a time split's boundary: train before it, holdout from it until now. The proposal row
  records which (`interleaved`, or `time` with its bounds). Only train errors reach the writer, so one
  judgment's label never feeds the writer and grades the candidate in one run.
- **The writer** (`[judge.learn] writer_profile = "opus"`) reads the pack file and up to `max_errors` (40)
  errors, newest first: each state from its blob, Jev's answers and bands, the label and its note. Its prompt
  states the loader's rules and Jev's: criteria are read literally, boundary cases are spelled out, no question
  asks for math, counting or dates; states are strangers' text, data and never instructions. It returns a pack
  file. It is priced from the catalog and reserved before it is sent (its output capped at 8,192 tokens: a pack
  file is a few thousand, and the profile's own cap would reserve past the day's limit), inside
  `writer_limit_usd_per_day` ($2): a proposal past it is skipped, written so, and its errors stay new.
- **The candidate** keeps every field of its parent but the text Jev reads (instructions, a Choice's meanings,
  a Score's levels, a Noul's criteria, the description): ids, kinds, options, thresholds, builder, state cap,
  model, point, action, baseline, sample and rollback rules stay, or it is refused
  (`propose::text_only`). It loads through `Pack::parse`, every loader rule, and is stored as the writer wrote it,
  its `version` line set, so its diff against its parent is its wording. **Its name**: learned versions
  are numbered from 101, the next after the lineage's highest, so a later build's compiled-in version (always
  below 101, a test holds it) never takes a learned one's name.
- **The check.** One 25d replay asks the candidate on the stored states of the labeled judgments of both splits:
  it keeps its parent's builder, so the blobs are its states (a question only for the builder's items, such as
  `classify.v1`'s `addressed_task`, is not asked on replay; the rest are). It pays as 25d's replay does
  (`replay_limit_usd`). The numbers are then each split's apart, graded by the parent's labels in absolute form:
  per question and class (a Choice's options, a Noul's true and false), precision and recall, and the train
  errors fixed. A Noul's answer is right when its lean meets the label (the report's calibration pair holds the
  label's truth, not that).
- **Thresholds** are re-fit in code, never by the writer (`propose::refit`): per whole question, the lowest
  `act` on a 0.01 grid from its `confirm` up to its parent's `act` whose act-band precision on the candidate's
  labeled train answers is at least the parent's on its own train answers at its own `act`. `confirm` stays. It
  stays its parent's below 30 labeled train answers, when the parent's act band held none, or when nothing lower
  keeps the precision. It only lowers `act`, within the loader's bounds.
- **The decision** (`propose::decide`). A class worse on the holdout (its precision or its recall lower) holds
  it, always. At 25c's minimum (200 labeled per deciding question, 30 per acting class): each deciding
  question's macro precision and macro recall (the mean over its classes) must each rise by `margin` (0.02);
  then a live (or canary) parent's candidate goes live at once, with the ladder's rollback rules as the brake,
  and a shadow parent's takes its place in shadow. Below it: some train errors fixed; a live parent's goes to a
  0.2 canary, a shadow parent's takes its place in shadow. A security pack that would move goes to the owner's
  card instead (26a's), with the numbers. Anything else is held, with why. Each move is 26a's own act
  (`Core::promote_learned`), its row the system's, citing the proposal as its report; where 26a's bar would
  refuse it (no report of the new version, or short of the minimum), the proposal's replay is the evidence.
- **One version per role.** Each point names its root as a constant; `JudgeService::placed` gives the version
  standing in its place: the newest learned one whose latest `pack.mode` row is `shadow`, `canary` or `live`
  (a canary's only in its canary arm; the control keeps the parent), else the root. A learned version with no
  row, rolled back, or rejected (`off`) stands nowhere, so a rollback gives the place back. The root's config
  line and `max_mode` cap its whole lineage. At the gate a learned version keeps its root's judgment id, and a
  successor of `security.v3` takes over its notices and their brake (its rows carry `root`).
- **What the owner sees.** A `judge.proposal` row per run that asked the writer (scoped `judge.learn:<id>`):
  parent, version, digest, the errors read, the replay's numbers, the re-fit, the decision and why, and the
  diff. A `pack.version` row per version, with its whole TOML; `<state dir>/packs/<name>.toml` (beside the store,
  wherever `--state-dir` put it) is written from it after serving and as it lands. One notice to the owner ("classify.v101 from 12 of your labels: holdout
  precision 0.80 → 0.86, recall 0.70 → 0.75 (kind); replacing classify.v1 in shadow"). The cockpit's Versions
  panel: each lineage, every version's mode and source, the diff between any two, promote (a learned version
  may also go to `shadow`) and reject (`pack.rollback { off: true }`, a mode row to `off`).
- **One open proposal per lineage**: a card not yet answered holds the next.

## 3. The build plan

Sixteen steps of about an hour each: 13 SPINE on `main`, in order, and 3 LANE. **The LANE steps can start
now**, in a worktree beside Stages 1 to 3, because the crate touches nothing on the spine. Stage 4 then begins
with the client, the packs, and the math already built and proved against the real API.

**Every live check runs on a fresh scratch store with scratch prompts until Eddie consents** (§4, item 1),
never on a copy of his store: a judgment sends its state to TypeSafe. After his consent, the checks go back
to the chain's usual copy of his store. Each live check with real Jev costs well under a cent. The chain's
live-check rules hold as always: the scratch daemon never binds Discord, and the fake Jev covers the failure
modes.

| # | Roadmap | Kind | Step | Depends on |
|---|---|---|---|---|
| L1 | 23 | LANE | `theseus-judge` crate: the typed client, errors, breaker, semaphore, bands, batching, `StateBuilder`, `Judge` + `Recording`, the fake; `theseus-sim jev-probe` | None: can start now |
| L2 | 23, 25 | LANE | The six pack files and their builders over plain inputs; `learn.rs` (calibration, holdouts, arms, rollback rules) | L1 |
| 23a | 23 | SPINE | Wire-in: `[judge]`, the catalog row, `JudgeService`, the sink, the shadow budget, `loop.v1` in shadow at `loop_end`, health, `theseus judge log` | L1, L2; Stage 3 done (the roadmap's order, decision 16) |
| 23b | 23 | SPINE | Surfaces: trace marks and `judge` spans, telemetry, narrative, `judge.list/get`, the Observatory's Judgment section and in-session judgments | 23a |
| 24 | 24 | SPINE | `security.v1` in shadow at the gate; scores on notices; labels from presses, declines, approvals; the T1 floor tests | 23a (23b for the UI lines) |
| 25a | 25 | SPINE | `classify.v1` and `role.v1` at `inbound`, batched in one request | 23a |
| 25b | 25 | SPINE | CONTINUE: the compiler's candidate signals, and `continue.v1` in shadow | 23a |
| 25c | 25 | SPINE | The learning ledger: `judge.label` everywhere, system labels, the nightly report tender, holdouts, the report page | 24, 25a, 25b |
| 25d | 25 | SPINE | Replay, audit, and backfill (backfill runs on Eddie's history only after consent) | 25c |
| 26a | 26 | SPINE | The ladder: `pack.mode`, config ceilings, arms, promote and rollback, `security.v1`'s approval card | 25c |
| 25f | 25 | SPINE | The learning loop: the owner's labels rewrite a pack's text as a learned version, checked by a replay, placed by its numbers through the ladder (§2.17) | 25c, 25d, 26a |
| 26b | 26 | SPINE | JUDGE_STOP live for tasks under canary: the `Judged` Advancer, live judgments as kernel actions, the nudge | 26a |
| 26c | 26 | SPINE | The roles table, `role.v1` under canary: the hint note and the role line | 26a, 25a |
| 27 | 27 | SPINE | The arrangement on `task.create`: references, refusal, the fidelity check, pieces admitted by reference | 23a (for the `should_promote` comparison only); DD7 (built) |
| 28a | 28 | SPINE | Independence: `check_of`, the exclusion set, the overlap flag, the basis | 27 |
| 28b | 28 | SPINE | `categorize.v1` in shadow; `tasks.parked` in health | M4 step 21 (the kinds table, topics); 23a |
| L3 | exit | LANE | `theseus judge prove`: the exit metrics, as a report generator, plus a one-command wire-in | L2; 26b for data |

### Each step's tests and live check

**L1. The crate** (LANE, worktree with its own `CARGO_TARGET_DIR`)
- *Tests:* request and response round-trips against fixtures of the verified shape (from the agents' reference notes on Jev); each
  `malformed` case (a missing id, an extra id, a wrong type, a choice outside the options, probabilities that
  don't sum); bands at their edges, for Choice and for Noul; batching merges namespaced ids, splits them back,
  and refuses states that differ; every builder's output stays under its cap on huge inputs, with truncation
  marked; the breaker's transitions; shedding when no permit is free; each fake mode maps to its error class
  (`down` to network, `slow` to a total timeout with `usage_unknown`, `429` to `rate_limited` with its retry
  after, `malformed` to `malformed`); a response from another model is flagged `model_drift`.
- *Live check:* `theseus-sim jev-probe` makes three real calls (one Choice, one Score, one Noul, on a synthetic
  state), and prints each answer, the usage, the cost at the catalog price, and the latency. Its key is read
  from the vault at run time and never printed.

**L2. Packs and learning math** (LANE)
- *Tests:* the loader's rules on all six packs; each builder's state on fixture inputs, as golden JSON; the
  math on synthetic data with known answers (a calibrated set gives an ECE near 0, a shuffled one does not);
  arms sticky and monotone as the share grows; each rollback rule fires on its synthetic sequence and not on a
  near miss.
- *Live check:* `jev-probe --pack <pack> --input <fixture>` for each of the six: every answer parses, and its
  bands are computed. About a tenth of a cent in all.

**23a. The wire-in, and the first shadow pack** (SPINE)
- *Tests* (core, with the fake):
  - a turn ending `no_tool_calls` dispatches one `loop.v1` judgment, whose row is keyed and scoped, and whose
    blob exists;
  - its cost goes to the judge's budget, and the session's cost is unchanged;
  - the frame-budget test still counts 8;
  - with the fake `down`, the breaker opens and turns take no longer than with the judge off;
  - `enabled = false` makes no call, and an unpriced Jev model makes none, with `unpriced`;
  - a $0.0002 day limit pauses shadow with one `judge.paused` row;
  - a daemon test: `kill -9` in the middle of a block, and the next start books the block's rest at the first
    judgment, not before serving.
- *Bench:* the lifecycle bench, with the judge enabled and the endpoint at 127.0.0.1:9, holds every phase.
- *Live check:* on a scratch daemon with real Jev and GLM, three `ask` turns. `theseus judge log` shows three
  judgments with their answers and costs. `theseus health` has the `judge:` line, and `ledger --json` has the
  rows.

**23b. Surfaces** (SPINE)
- *Tests:* a `mark` span per shadow dispatch; a narrative line when a judgment lands; the metrics' names and
  attributes against the OTLP test receiver's fixtures; `judge.list` filters; the web build and lint.
- *Live check:* the same scratch turns. The Observatory's Judgment section shows the calls, cost, and p50/p95;
  a turn's view shows its judgment beside the loop; the telemetry receiver (`otlp-receiver.py`) shows
  `theseus.judge.calls`.

**24. `security.v1` in shadow** (SPINE)
- *Tests:*
  - a property test over (posture, hold, floor, score): the treatment with Jev is never looser than without it;
  - in a holding session, a call Jev scores at 1% still waits;
  - the floor still asks;
  - a `web.search` in a holding session is judged, and one in a clean session is not;
  - a "should have asked" press with `--call` writes a `judge.label` for the matching judgment;
  - a notice shows `risk N% (shadow)`;
  - `tool.started` comes as fast as with the judge off, since dispatch never waits.
- *Live check:* on a scratch daemon, GLM with real Jev:
  - `proc.run echo hi` at `notify` shows its score on the CLI's notice line;
  - a deletion of scratch files scores higher;
  - `web.search`, then `ls`: the `ls` waits under T1 whatever its score;
  - a press on a notice writes a label row.

**25a. `classify.v1` and `role.v1`** (SPINE)
- *Tests:* one request per inbound message (the fake counts one call), two judgment rows that share a blob,
  with the cost split by question count; no judgment for a slash command, a wake's turn, a report's turn, or a
  task's first turn.
- *Live check:* three scratch messages: a new ask, a fragment ("and the tests too"), and "stop" typed as plain
  text. The log shows the kinds, and the roles guessed.

**25b. CONTINUE** (SPINE)
- *Tests:* pure `compile()` tests, one per signal; no signal, no judgment; a deterministic trigger, no
  judgment. The signals' thresholds are config (`[judge.signals]`), so tests and live checks can shorten them.
- *Live check:* with `dormancy_minutes = 1`, a turn after a two-minute gap shows the signal on
  `context.compiled` and a `continue.v1` judgment.

**25c. The learning ledger** (SPINE)
- *Tests:*
  - `judge.label` through the protocol, the CLI, and the web UI, and refused from a job's process (J1);
  - each system label derived from a scripted history;
  - the tender never runs within 10 minutes of a start, and runs at low priority;
  - the report's numbers on a synthetic store equal `learn.rs`'s;
  - a holdout window is frozen into the report.
- *Live check:* on the scratch daemon's judgments from the earlier checks, label five with the CLI, run
  `theseus judge report`, and open the report page.

**25d. Replay, audit, backfill** (SPINE)
- *Tests:* a replay against the fake over a frozen holdout; a state rebuilt from its inputs when the builder
  version changed, and refused with the reason when it can't be; the audit's per-run cap; backfill's states
  built from scripted history equal the live builders' output.
- *Live check:* `theseus judge audit loop.v1 --sample 5 --profile glm` gives five audit labels. A replay of
  `loop.v1` over the scratch holdout. **Backfill on Eddie's history only after his consent.**

**26a. The ladder** (SPINE)
- *Tests:*
  - the config lowers a mode and never raises it;
  - a promotion without a qualifying report is refused with the numbers;
  - a forced one is ledgered as forced;
  - a job's process is refused;
  - `security.v1`'s promotion is a card, and only a trusted answer promotes it;
  - a synthetic rule trigger rolls a pack back;
  - `max_mode = "shadow"` caps every pack after F1b's restart in place.
- *Live check:* `theseus packs promote loop.v1 --canary 1.0 --force`, then the `pack.mode` row and the health
  line; a rollback; `security.v1`'s card answered with `theseus confirm`.

**25f. The learning loop** (SPINE; §2.17)
- *Tests* (the fake Jev scripted per state, the writer scripted):
  - nine new train errors propose nothing and ask no writer; ten propose once; a second run with none new
    proposes nothing; a holdout label's note and state never reach the writer's request;
  - a candidate that changes a question's id, the builder, or a threshold is refused; the re-fit's thresholds
    equal the rule's;
  - better on train but worse on one holdout class: held; better with no class worse below the minimum: a
    shadow parent's place in shadow, a live parent's canary; at the minimum (pure): live, the card for a
    security pack, shadow for a shadow pack;
  - the writer's day budget stops a run; the version is a row and a file the rows rebuild; it stands at its
    root's point, and a rollback gives the place back; the nightly split is the interleaved one below 200.
- *Live check:* on a scratch daemon with `[judge.learn] min_errors = 2`, label `classify.v1`'s judgments, run
  `theseus judge learn classify.v1 --split <time>`, read the proposal, the diff in the cockpit's Versions panel,
  and `<state dir>/packs/`; roll a placed version back; a `security.v3` proposal is held or waits on its card.

**26b. JUDGE_STOP live for tasks** (SPINE)
- *Tests* (fake scripted per call):
  - `progressing` nudges a task once, with the harness node, and the task then ends;
  - `complete` ends it as the baseline would, and so does the confirm band;
  - a timeout abstains, and its held reservation is reconciled;
  - over budget, it abstains and asks nothing;
  - the session's cost includes the judgment;
  - the frame-budget test's judged variant holds its number;
  - a conversation in the canary arm stays in shadow;
  - the nudge-loop rule rolls the pack back.
- *Live check:* on a scratch daemon with real Jev and GLM, with the canary forced to 1.0:
  - a plain task (`git log -3`, then report) is not nudged;
  - a task whose brief tells GLM, plainly as the operator's test, to announce that it will run `git status`
    next and then end its turn without running anything, is nudged, runs it, and reports once;
  - the rows show `advancer: judged` and the judgment.

**26c. The roles table and `role.v1`** (SPINE)
- *Tests:* the switching rule (act band, margin, one per exchange); the hint note lands in the tail and the
  compilation id is unchanged, so no recompile; the role line on the reply post; `announce = false` silences
  it; schema-2 sessions read; an operator row added and versioned.
- *Live check:* with the canary forced, "review this diff: …" switches to reviewer, with the note and the line.
  The next message keeps the role.

**27. The arrangement** (SPINE)
- *Tests:*
  - quote resolution: exact, unique, at least 20 characters; no match; ambiguous;
  - the refusal without an arrangement;
  - the fidelity flag, and its ack;
  - the child's first compilation renders each piece verbatim after the brief, with origin and as-of, and
    lists superseded pieces by reference only;
  - the prefix stays the same across the task's later turns;
  - schema-2 nodes read.
- *Live check:* a 12-message scratch discussion, then "do it as a task". GLM quotes the pieces, and the task's
  first compilation (`compilation.list`) shows them. A one-line brief is flagged. The report arrives once.

**28a. Independence** (SPINE)
- *Tests:* a check task's compilation holds no node of the maker's session but its report; a copied span
  raises the overlap flag; the basis is recorded and shown; a check of a task with no report yet is refused.
- *Live check:* a maker task, then a check task. `compilation.list` shows the exclusion, and the task's line
  shows the basis.

**28b. `categorize.v1` and parked tasks** (SPINE)
- *Tests:* the `exchange_end` trigger; candidates from the kinds table; accept and reject write labels and an
  operator membership; parked detection on scripted states.
- *Live check:* with two topics declared, ten scratch messages about one of them bring a proposal to the
  Observatory. Accept it.

**L3. The prove report** (LANE, then a one-command wire-in)
- *Tests:* synthetic canary and control cohorts with known outcomes give exact metrics, per task and per
  dollar.
- *Live check:* on the scratch daemon's small canary, the report honestly says "insufficient", with its
  counts. The measurement that closes M5 runs in v1's soak.

### How the phase's data grows while the chain works

- Install 23a as soon as it is reviewed, and turn shadow on in Eddie's note on consent. Every later step then
  lands on a daemon that is already collecting judgments, so 25c's first report has real data, and 26a's
  promotion can cite a real holdout by the soak.
- The chain never waits for data. A step that needs a canary to act uses `force` on a scratch daemon.

## 4. What it needs from Eddie

| # | Item | Blocks? | When |
|---|---|---|---|
| 1 | **Consent to send session content to TypeSafe.** A judgment's state holds his messages, tool calls, and their arguments, trimmed and scrubbed of secrets. It goes to a third-party API. This includes his employer's work (a SOC2 vendor question), and the backfill would send his recorded history too. What TypeSafe keeps, and whether it trains on inputs, needs checking against its terms before he answers | **Yes**, for running the judge on his daemon, on copies of his store, and for backfill. **No**, for the build: every step is built against the fake and live-checked on scratch content | Ask with this design's review, so shadow can collect data from the first install. Default without an answer: the judge stays `enabled = false` in his note, and nothing of his reaches TypeSafe |
| 2 | **One line in his vault note**: `enabled = true` under a new `[judge]` table (every other key defaults), and a check that his `[secrets]` has `jev_api_key`. The template has it, and on 2026-09-30 his note had seven of the template's eight secrets, Brave's being the one missing, so it is probably there. The service account is read-only, so he pastes it | Only for his daemon, after item 1. Install the build first, since an older binary refuses the unknown table | At 23a's install or later |
| 3 | **Approve `security.v1`'s promotion** when its report qualifies: a card in a trusted channel (§3.10) | No | During the soak |
| 4 | **Decide whether `security.v1` may ever make a call wait** (turn `notify` into `approve` on a high score), or only add notices | No. Default: notices only (§5, Q2) | Any time; it matters only after item 3 |
| 5 | **A heads-up, not a question: the arrangement changes his daily `task.create` flow.** He decided it on 2026-09-27 (theseus-vmh, decision 6: promotion requires an authored arrangement). In practice the model must quote at least one message that defines each task | No | Told at 27's install |
| 6 | **A learning channel** (a private Discord channel, like `#theseus-test`) for the nightly digest and promotions | No. Default: none; the web UI and the CLI carry it | Any time |
| 7 | **A few labels a week** in the web UI (right or wrong, on judgments that matter) | No. Audit labels stand in, at lower weight | During the soak |
| 8 | **The TypeSafe account**: whose it is (his or his employer's), and a look at the console's billed amounts against the ledger's | No. The ledger's cost comes from `usage` and the catalog price | Once, after the first week of shadow |

## 5. Open questions

Each has the default the build takes, so nothing waits on an answer.

| # | Question | Default the build takes | Why |
|---|---|---|---|
| Q1 | A new record kind for judgments, or ledger rows? | Ledger rows, keyed by id and scoped `judge:<pack>` | No manifest bump, no one-way door for older binaries, and the spec already names the ledger as the home. If ledger scans ever cost too much, a dedicated kind is a clean move later, since the rows are keyed |
| Q2 | May `security.v1` make a call wait (notify to approve), or only add notices? | Notices only (Eddie's item 4) | A wait puts a Jev call before every acting dispatch, and adversarial text can move the score. Notify over block (§2) points the same way |
| Q3 | Who pays for shadow calls? | The judge's own day budget, $1.00 | An experiment the system runs is not the session's work. At about $0.0001 a call, $1 is roughly 10,000 calls, ten times the expected volume |
| Q4 | What unit does a canary pick? | The session, by hash, sticky and monotone | Trajectories compare cleanly when a session never changes arm. Nothing is stored for it |
| Q5 | Where do role hints go? | A harness note in the tail; the persona stays `[context] default_persona` | The system block would force a recompile and a cache miss on every switch. Letting Jev choose the persona is a later decision, once switches are measured |
| Q6 | How small may an arrangement be? (That one is required is Eddie's decision 6, theseus-vmh.) | One quote with role `objective` or `design`, plus the fidelity check for a one-line brief from a long discussion | One quote of the operator's ask is a light burden, and the fidelity check catches the case the decision was about |
| Q7 | How does the model name the pieces? | Exact quotes, resolved to nodes (node ids also accepted) | No renderer change and no cache cost. If the ledger shows GLM failing to resolve quotes on more than 20% of calls, render short ids into the transcript under a renderer version bump |
| Q8 | JUDGE_STOP live in conversations? | Not in M5: shadow only | A person is present to say "go on", and a nudge in chat is noise. Revisit with the prove's data |
| Q9 | How is the Jev model pinned? | Per pack version; `jev-latest` never acts; a new Jev model means new pack versions in shadow | Calibration does not transfer between models |
| Q10 | Pack files: compiled in, or operator-editable? | Compiled in, versioned in git, reviewed; a version the learning loop writes (25f, §2.17) lives in the store, a `pack.version` row with its whole TOML, and `<state dir>/packs/<name>.toml` is derived from it, never read | A pack's text is policy-adjacent. A learned version changes only the text Jev reads, loads through `Pack::parse`, and is placed by its numbers through the ladder; a security pack's waits on the owner's card. Overrides from the state dir are filed |
| Q11 | What may a state contain? | Only what the session's own model can see, trimmed, scrubbed, from a per-pack field list. Once M4's confidentiality labels exist, nodes above `[judge] max_label` stay out | A third party reads it |
| Q12 | Which calls does `security.v1` judge? | Every call whose class is not `read`, and fetches and searches in a session holding external text | That second group is the exfiltration path T1 leaves open by design. Judging every read would multiply volume for little value |
| Q13 | What does the middle (confirm) band do in a live pack? | Defers to the baseline, and is ledgered | Asking a person needs its own card and its own tuning; filed |
| Q14 | Jev down, for a live pack: fail to the baseline, or park the execution? | Fail to the baseline | Every M5 live pack has a baseline that can decide alone, and Jev is needed only to continue. The Jev-recovery wait comes with the first pack that has no baseline |
| Q15 | What sample earns a promotion? | 200 labeled per deciding question, 30 per acting class, a frozen 14-day holdout | Conservative; tuned after the first real report |
| Q16 | Shadow sampling? | 1.0 for every pack | About 1,000 calls a day, about $0.10. Config lowers it if rate limits bite |
| Q17 | Nudge limits? | 1 per turn, 3 per task, a fixed text naming the judgment | Bounded by construction; the rollback rule catches loops |
| Q18 | P7's hooks, and the epic's "hooks Gate/Transform/Claim/Observe"? | Not built; theseus-0j2's description is updated at 23a's review | The hook system was deleted on 2026-09-28 (§3.17) |
| Q19 | Start L1 and L2 before Stage 4, although decision 16 says "Jev late"? | Yes, when a lane slot is free (the AWS lanes come first, per theseus-zaz) | Decision 16 is about when Jev acts in the product. The crate touches nothing on the spine, ships nothing live, and turns two serial hours at Stage 4's start into parallel ones |

## 6. Risks

| Risk | What guards against it | What would change the plan |
|---|---|---|
| **Adversarial text moves Jev's answers** (documented by TypeSafe) | `security.v1` only adds notices, never loosens; T1's hold and the floor stay deterministic; Jev is never the sole gate; a planted-injection eval set runs before `security.v1`'s promotion | If the eval shows the score is easy to steer, `security.v1` stays in shadow. It still orders what a person reviews, which the agents' reference notes on Jev rank as its safe use |
| **Eddie declines the third party**, or his employer's content can't go to TypeSafe | Everything builds on the fake, and live checks use scratch content | The `Judge` trait gets a second implementation: a local reproduction (Jevlike or OpenJev from awesome-jev, whose calibration differs and must be re-earned), or an LLM judge (Haiku 4.5 with structured output: slower and dearer, but no new vendor). Packs are re-tuned in shadow |
| **Too few labels for the prove** within the two-week soak | Weak system labels, audit labels, and (with consent) backfill for a day-one holdout | M5's exit slips past the soak, and the prove says so. The spec allows it: a pack that has not beaten its baseline stays in shadow |
| **Jev's model changes, or a pin is retired** | `model_drift` is detected, and never acted on; pack versions pin their model | If pins retire quickly, keep a standing candidate version of each pack on the newest model, always in shadow |
| **Latency** higher than about 350 ms | Shadow is off every path; the one live pack runs where nobody waits; the canary rolls back past 1 s p95 on its path | Nothing live moves onto a path a person waits on (a conversation's turn, a call's dispatch) until the per-class numbers say it is cheap |
| **Notice noise** (the quiet-notices lesson, theseus-w4f) | A Jev notice rides the call's existing line; a rollback past 30 a day | Raise the threshold, or keep `security.v1` to scores on existing notices |
| **Role churn breaks the prompt cache** | Hints go in the tail; a margin and one switch per exchange; rollback past 2 switches | Roles stay in shadow if switches don't change outcomes |
| **Exact quotes are hard for GLM** | Errors that say how to fix the call; the result lists what resolved; the failure rate is ledgered | Render node ids into the transcript (Q7) |
| **Store growth** from shadow rows and state blobs | Batched frames, deduplicated blobs, the day budget, and sampling | Keep states only for labeled or sampled judgments. **M4's durability tender must ship `blobs/`** (images already need it; a note for the [`m4` lane](m4-boundaries.md)) |
| **Jev's rate limits are unknown** | The semaphore, shadow shed first, and the breaker | Set `max_in_flight` from the first 429s' `retry_after` |
| **A forced promotion reaches Eddie's daemon** | `forced` is on the row and every surface; his note's `max_mode` is a ceiling agents can't write; the chain forces only on scratch daemons | Refuse `force` on a daemon whose config is the vault note, if it ever happens |
| **Cross-phase slips** | 28b alone needs M4 (step 21's kinds table) | 28b waits; the rest proceeds |
| **Spec drift** | This design names four divergences from P7 (§1) | Fold them into Parts I and II at 23a's review, and add Part III's M5 entry as each step lands |

**What would change the plan most:**
- **Eddie's answer on consent** (§4, item 1). Yes: shadow starts collecting at 23a's install, and the prove can
  land in the soak. No: the build is the same, but the judge is a different implementation, and M5's exit
  moves later.
- **The first real report.** If `loop.v1`'s `announced_unfinished` and `progressing` are precise on tasks, 26b
  is the win this phase exists for, and JUDGE_STOP for conversations follows. If not, M5 ships the ledger and
  the ladder, with every pack in shadow, honestly reported.
- **When the LANE steps start** (Q19). If L1 and L2 run beside Stages 1 to 3, Stage 4 opens with a proven
  client and proven packs, and its SPINE steps are wire-ins. If they wait for Stage 4, the phase takes about
  two more serial hours (L3 runs beside Stage 4 either way).

*Written by Tabitha/Claude, 2026-09-30, for design lane `m5` (theseus-zaz.3).*

<!-- REPORT COMPLETE -->
