# Every gate, wait and stop: the inventory (theseus-nmod9, theseus-lyxn7)

Written 2026-10-26 from the code at `78fcfb6d` (store format 51, kernel SCHEMA 2), and re-read at its join against
`origin/main` at `2557601f` (install #91). **States are as of `2557601f`**: the rows that were "under review" when it was
written have joined, and each says in which install (#90 `50f8dcf2`, #91 `2557601f`). The one row of the wave that is
**not** on `main` is task depth (`cloud/20261026-p3n-task-depth`, theseus-tecj5): its review found two blocking
findings that are the owner's to decide, so it is held, and every line below that depends on it says so. Store format
51 and kernel `SCHEMA` 2 are unchanged by the row that wrote this.

The owner's rule, quoted: *"we typically are going to want very long sessions or turns -- we're less afraid of
infinite loops and more afraid of stopped work"* (2026-10-25 13:22), and *"i want there to be precious few hard gates
/anywhere/ in theseus"* (13:58). Theseus stops on **money rate** (the burn line, the day), the AWS lines, and the
keel list below, and on nothing else. Every other wait becomes notify-then-work; every clock-only stop becomes
progress-based, counts a background job as progress, and names what it cut.

**Method.** Each row was read in code, not taken from a note. Columns: the stop and its anchor; its default and key;
what it does on fire; clock- or progress-based; whether a background job counts as progress; whether the stop names
what it cut; whether it is a generous backstop; the state. `done` rows name the commit, install or branch that did
them. `unknown` is an honest "I could not read it".

**Scope, which is narrower than "every".** This inventories the stops reachable from **a turn, a job, a task, a routine,
a self build or the money lines**: the owner-visible gates. It does **not** inventory the daemon's own internal client
timeouts (about twenty production `tokio::time::timeout` sites in `secrets`, `judge`, `recall`, `mcp`, `google`,
`telemetry`, `outbox`, `cancel`, `cgroup`, `rpc/server.rs`, and the turn-step waits `route_step`, `nudge_step`,
`digest_step`, `families_step`, `term/tools.rs`, `rpc/thread_flow.rs`). They were enumerated, **not individually
assessed** for whether any is a stop on the owner's work or ordinary client plumbing; a later row that needs the
answer must read them. Size and shape caps (page sizes, byte budgets) are left out on purpose.

**What I could not read, and what was checked later.** The written pass read `integ/p3-o` from `origin/main`
(`e3c7d0e0`, install #89) and the `cloud/20261026-p3n-*` branches from `origin`; `task-depth` had no report on its
branch, so its diff was read. `docs/design/o1h3f-design.md`, which the task said to start from, is **not in the
repository**; the rows are from the code. A reviewer then re-read 11 claims (the money rows, the store and `SCHEMA`
numbers, the `DEPTH_REFUSAL` count, `default_deadline_secs`, `toolrun.rs:593`); the other rows in section A, and in B
the `job_wait` / `fan_out_wait` / `task_lease` family, `UNKNOWN_AFTER` / `GIVE_UP` / `BUSY_BOUND` / `READY_WITHIN`, the
AWS settle clocks, the per-tool inproc caps, the progress steer's numbers, `max_wakes`, `max_watches` /
`max_reminders` / `max_ahead_days`, the external-text hold, `approve_argv` / `approve_paths`, the carve rows and
`place_limit.rs`, are **spot-read, not verified**. At the join every `path:line` anchor in this file was checked to
exist; that is an existence check, not a claim that each row's behaviour was re-read.

## A. Done before this row (verify, never redo)

| item | what the notes asked | what the code is | evidence |
|---|---|---|---|
| lyxn7 (4) misroute guard | notify, then write, with `files.restore` the undo | the first write outside the thread's roots goes on at the notify posture, one notice naming path, thread, next-likeliest thread and `Undo: files.restore <path>` | `toolrun/misroute.rs:6,9,261,263-264`, tests `:362,372,387`; theseus-g85jy, install #88 (`8b9ed399`). **Confirmed done.** |
| nmod9 (1) `of_slow` | drop elapsed time as a defect, keep first token | `rsi/backlog/read.rs:351-359`: the doc says a long turn is work; `elapsed_ms` only confirms the row is a finished turn; the one defect is `turn.first_token:<model>` over `FIRST_TOKEN_BUDGET_MS = 10_000` (`:22`). `git grep 'TURN_BUDGET_MS\|turn\.slow' crates/` is empty. | theseus-x5xth, install #88 (`1a6d1419`). **Confirmed done.** A loop-cap cause is also gone (`read.rs:640`). |
| lyxn7 (1) task depth one | depth 2-3, bound by spend | `DEPTH_REFUSAL` has 8 references at base (`task.rs:87,556,777`, `task_fan_out.rs:350,435`, `task_wait.rs:266`, `tests_task_fan_out.rs:463`, `tests_task_wait.rs:552`) and **zero** on `cloud/20261026-p3n-task-depth` (`tasks: depth 1 to 10 …`, theseus-tecj5): `[kernel] task_depth` default 10 (1 to 32), a parent-chain walk bounded at 64, refusal names the depth reached, a child carries root goal and chain, a repeated brief is refused as a cycle. Derived, no store change. | Branch only, **HELD: not joined**, `waiting_for_user`. Two blocking findings are the owner's: at depth 2 or more the *conversation's* reported spend is wrong (`carry_to_parent` is one level; the sim pins `task_depth_limit` at 1, so the gate cannot see it), and the routine fire path (`routines.rs:327`) calls `carve_task` without the depth bound at `tasks.rs:191-203`. Spend still bounds it (the carve). **The branch does what it says; whether it lands is not decided here.** |
| question TTL (o1h3f #10 family) | an unanswered question cancelled the work | a call's question past `confirm_ttl_secs` stays pending; one quiet notice every life (15, 30, 45 min); a routine run's out-of-grant call is dropped and the run goes on | `rpc/confirms.rs:550-595` (`held_past_life`), `b18c6c75`, theseus-v2dnc, install #88. An `owner_ask` still applies its default at its own life; an extension ack and a pack promotion still expire (they act on nothing). |
| lyxn7 (2) layer 1 of a model-started task | objective / acceptance / abandon wait for the operator | a layer-1 change to a task the model wrote applies at once; only the owner's tasks (`!origin.by_model`) keep the card. A task a turn with no owner input opens is marked `by_model` | `task_graph/tools.rs:312-352` (`waits`, `authority_for`), `theseus-protocol/src/tasks.rs:274`, `task_self.rs:88-92` (theseus-ext.10, o1h3f.1). **Done.** The owner's own tasks keeping the card is the "agent cannot redefine success" rule (§3.5): **keep**. |
| lyxn7 (7) task tools deferred unless asked | load them on a turn with no owner input | `defer_step.rs`: a turn whose `continuation` is true (wake, report, routine run) loads the tasks family, past the place's budget by exactly its own size; a person's turn after a wake drops it again | `turn/defer_step.rs`, `tools_defer.rs` (`dropped`), theseus-o1h3f.3, install #89 (`89cfe147`). **Done on main, not in the `78fcfb6d` base.** |
| lyxn7 (5) arrangement quote | `task.create` needs quoted messages | the quote still resolves from the session's own transcript, **including a tool result or the model's own line**; the refusal text says so (`arrangement.rs` REFUSAL, o1h3f.3), and the sentence is out of `task.create`'s description. It is input validation, not a wait. | `arrangement.rs`, `c5fd58e8`, install #89. **Partly done; the requirement stays (M5 decision 6).** |
| may-build wait | gate failed when main's rule said no | waits as long as the rule says no; `MAY_BUILD_WAIT` (`rsi/tree.rs:131`, 30 min) is a **notice interval**, with one inform notice each wait | `dc016c9b`, theseus-ccik8, install #88 |
| `proc.run` default | 10 minutes | default and cap 3,600 s (`config.rs:546-551`); a clamped limit is told in the result | theseus-spntl, install #88 (`dce5747a`) |
| transient provider failure | failed the turn | `[model.retries] transient` = 2 (`config.rs:1225`); a whole-call timeout goes to the fallback once | theseus-3cf5c: retries in install #88; the whole-call-timeout fallback joined in **install #91** (`60e598d2`, `turn/fallback_step.rs`, `turn/retry_step.rs`). **Done on `main`.** |
| admission, a person | failed at the stuck window | a person's waiter is never failed; one quiet inform notice names the holders, once per stuck episode | `turn/admission.rs`, theseus-5tl9v, **done: install #90** (`ab9fa14a`, join fixes `7f0d4e84`, `ad9d495c`). The owner's typed input into a task is a person's too (theseus-fzaep; `Class::is_typed`); a wake or any other task turn still fails at the window: see B2. |
| halt on infra | a gate that never started filed a cause | two such in a row halt self builds and file no cause | `rsi/infra.rs`, theseus-otkwu, **done: install #90** (`4ccb1351`, join fixes `af3dfee5`, `310d6811`) |
| the door fetch | a stale `origin/main` base | `tree::open` fetches first, once per 60 s per repo, warns and goes on if the fetch fails | `rsi/tree.rs`, theseus-g5wxc, **done: install #91** (`7078c63d`, join fix `81518edc`: the owner is told when a tree is cut from a base that could not be refreshed) |
| session wake count | 5 pending wakes | 20 on main (`config/wake_limits.rs`); **5 at the base** (`wake_limits.rs:14-16`, and the tool description's "at most 5") | theseus-o1h3f.8, install #89 |
| `max_loops` | the turn ended at the loop cap | default `on_max_loops` is **notify**: at the cap and each multiple, one notice and the turn goes on; `end` is opt-in | `config/limits.rs:67-77`, `turn/budget_step.rs:141,150`, theseus-ev8 era; bench sealed runs set `end` explicitly (`rsi/sealed/runner.rs:317`) |
| spend limit | stopped at the limit | `spend_limit_mode` default notify; a hard stop only at 10 times the limit | `config.rs:807-815`, kernel `overdraw.rs:15,78` |

## B. The inventory, one row per stop

`C` clock-only, `P` progress-based, `—` not a clock. `bg` = does a background job count as progress. `names` = does a
fire say what it cut. `bs` = generous backstop (keel-class).

### B1. Money (all keep)

| stop | default / key | on fire | C/P | bg | names | bs | state |
|---|---|---|---|---|---|---|---|
| burn line (`runaway_usd_per_10min` → `burn_line_micros`) `config.rs:831,946`; kernel `day_ceiling.rs:70` | $200 per 10 min per execution family; children count to their parents | a question: stop the family, resume on the owner's answer (`BurnStop`) | rate | — | yes (a question with the window) | yes | **keep (keel)**. Test: `theseus-kernel tests_runaway::a_session_that_burns_its_line_in_ten_minutes_is_stopped_and_resumes_on_the_answer`, `two_children_trip_their_parents_burn_line` |
| day ceiling `daily_spend_ceiling_usd` `config.rs:817-818`, `day_ceiling.rs:3` | $200/day; `daily_spend_ceiling_mode` default notify | notify (warn at the ceiling and each step); stops only at the runaway | amount | — | yes | yes | **keep (keel)**. `tests_day_ceiling::a_reservation_holds_the_day_and_one_past_the_ceiling_is_refused_unwritten`, `a_refusal_stops_the_day_until_midnight_or_a_higher_ceiling` |
| daily runaway `daily_runaway_usd` `config.rs:823`, `day_ceiling.rs:64` | $2,000/day, 0 never | stops model calls until midnight | amount | — | yes | yes | **keep (keel)**. `tests_runaway::the_runaway_line_stops_the_day_and_holds_until_midnight` |
| session spend limit `spend_limit_usd`, `spend_limit_mode` `config.rs:807-811` | $100, notify | notice (or the question under `ask`) | amount | — | yes | yes | **keep (keel)**. `tests_overdraw::a_provider_call_past_a_notifying_limit_is_reserved_and_goes_on`, `ask_mode_and_a_pinned_limit_still_refuse` |
| hard multiple `spend_limit_hard_multiple` `config.rs:815`, `overdraw.rs:55` | 10x the limit, 0 never | refuses the call | amount | — | yes (`HardStop`) | yes | **keep (keel)**. `tests_overdraw::a_notifying_limit_stops_at_its_hard_multiple` |
| a task's carve / helper carve (`task_fan_out`, `helper_step::at_carve`) | the parent's remainder, split | under a notifying limit a notice; under `ask` the helper ends `budget` | amount | — | yes | yes | **keep** |
| place limit (`place_limit.rs`, bindings) | per place | refuses past the place's cap | amount | — | yes | yes | **keep (keel: pinned limit)**. `tests_place_limit.rs` |
| AWS hour/day/month lines (`runaway_factor`, `hourly_alert_usd` `config/aws.rs:165`; runaway mode) | alert $1/h; refusal past the lines | AWS reservation refused past the limit even under the soft settings | amount | — | yes | yes | **keep (keel); no AWS call made by this row.** `tests_runaway::an_aws_reservation_past_the_limit_is_still_refused_under_the_soft_settings` |
| per-purpose day budgets (`initiative.day_usd` 0.50, `card.day_usd` 2.0, `people.sweep_usd_per_day` 2.0, `ontology.sweep_usd_per_day` 1.0, `[self]` `branch_usd` 10 / `day_usd` 30, `helper_floor_usd` 1.0, routines `run_usd` 0.50 / `month_usd` 10, `route.*_usd`) | per key | the background call is not made / the run is capped | amount | — | mostly (row, `waits`) | yes | **keep**: they bound Theseus's *own* initiative, not the owner's work. **A routine's `run_usd` / `month_usd` are defaults the caller may override, not caps:** `routine_tool.rs:138-144` takes them from the model's input and the only checks are `usd_ok` (`routine.rs:324-326`, finite and positive) and `run <= month`, so a task may propose `month_usd: 5000`. The amounts are printed on the card the owner answers. The money backstops that actually hold are the session limit, the burn line and the day ceiling above. |

### B2. Time and waits

| stop | default / key | on fire | C/P | bg | names | bs | state |
|---|---|---|---|---|---|---|---|
| `admission_stuck_secs` `config.rs:804,874`; `turn/admission.rs:27,464` | 600 s with no turn admitted | a **person** (or the owner's typed input into a task, theseus-fzaep): keeps waiting + one notice (`admission-notify`, branch). A **wake or other task** turn: `Err(Stuck{window_s,class,in_use,ceiling,waiting})` and the turn fails | queue movement (P: a queue that moves is never stuck) | a foreground job's wait lends its slot after 2 s (`LEND_AFTER`) | yes: class, holders | no (not money) | **person: done, install #90. Wake/task: to change** (a wake turn that cannot start within 10 min of a full queue fails; it should wait and notify too; `admission.rs`'s own header says "Wakes and task turns fail as before"). Finding, **tests only**: `toolrun.rs:593` hard-codes `Admission::new(8, 600 s)` inside `ToolRuntime::disabled()` (`:533`), which builds from `ToolCtx::for_tests`; production builds from `[kernel] admission_ceiling` / `admission_stuck_secs` (`config.rs:802,806`), so it reaches no owner. |
| `default_admission_ceiling` `config.rs:871` | 8 turns at once | queue | count | — | the queue | — | keep: a concurrency bound, nothing is cut |
| `heartbeat_secs` `config.rs:877` | 60 s | the driver's tick | — | — | — | — | not a stop |
| `default_deadline_secs` `config.rs:836,880`; `kernel.rs:1768` | 600 s | `deadline_at_ms` of an action planned with no tool deadline. Past it the **reconciler probes** (`kernel.rs:2725`: completed / still running / gone → `OutcomeUnknown`), it does not cut. `toolrun.rs:1266` always passes the tool's own deadline, so the default reaches no model-visible call. | C (probe) | a probe finds a running job still running | `still_running_past_deadline` | no | **keep; verified not a cut.** Recorded because the notes suspected a cut. |
| inproc tool deadline `INPROC_DEADLINE_MS` `toolrun.rs:461` | 120 s, or the tool's own | `timed out after N ms` | C | n/a (a call, not a job) | yes | no | keep: a call that holds a turn with no event; long work is a job. Per-tool caps (web 30 s ≤ 110, MCP 110 ≤ 115, LSP 30 ≤ 600) are the same class |
| `proc_timeout_secs` / `proc_timeout_max_secs` `config.rs:546-551`; kernel wrapper | 3,600 / 3,600 s | the wrapper kills the job; the result says "timed out" and a clamp is told (spntl) | **C** | **no**: output or CPU does not extend it | yes | no | **to change (next row, not this one)**: a foreground-or-background job that is making progress (output growing, CPU) past an hour is killed. The right rule is silence-based (no output and no CPU for N min) with a notice at the hour; it lives in the job wrapper (theseus-kernel `job`), shared with the sandbox — larger than a row-sized change and with L1 tests failing on this VM, so I left it. |
| `job_wait_max_secs` `config/limits.rs:171` / `job_wait_beat_secs` `:178` / `job_wait_status` `:184` | 1,800 / 600 / on | `job_wait` **returns** (the job runs on) with "still running after N s"; at a silent beat, facts + one status line | — | the wait is *for* the job; ends at once on its end, a message or a stop | yes | — | **keep: not a stop.** A return is not a cut; the model waits again. |
| `fan_out_wait_secs` 900 / `fan_out_max_wait_secs` 1,800 `limits.rs:191,197`; `task_wait.rs` | 900 / 1,800 s, less `max(60 s,15 %)` of a deadline | `task_wait` returns, tasks keep running, told in a note | — | tasks' end is the wake | yes | — | keep (a return, not a cut). `task_wait.rs` is mid-flight (task-depth). |
| provider `Timeouts` `provider.rs:370-409` | connect 10, first byte 60, idle 60, total 600; a live stream to `3 × total` = 1,800 s (`LIVE_STREAM_TOTALS`, `provider.rs:388`) | the call fails as a transient; retried twice in the turn; a whole-call timeout goes to the fallback once (**done, install #91**) | stream idle **P**; whole call **C** | n/a | yes (the failure) | no | **keep, with a finding.** The 1,800 s cut of a *live* stream is clock-only. It is bounded in practice by `max_output_tokens`, and `retry_step` retries it once. The files it touches (`turn/retry_step.rs`, `provider.rs`) are no longer in flight. Recorded as a candidate, counted once here: make a stream that is still emitting events immune. |
| `confirm_ttl_secs` `config.rs:841,883`; `ask.rs`; `rpc/confirms.rs:550` | 900 s | a call's question: notice every life, **never declined** (done); `owner_ask`: default applies at its own life (30 min, ≤24 h); extension ack, promotion: expire | — | — | yes | — | **done (v2dnc).** A **budget** question holds until answered (keep: money). |
| `task_lease_minutes` `config.rs:889`; `task_graph/lease.rs` | 30 min | a lapsed claim frees the task; a holder's every edit renews | C on a claim, renewed by edits | an edit is progress | row `task.lease_expired` | — | keep: it frees a claim, it cuts no work |
| routine `run_deadline_secs` `config/routines.rs:31` | 900 s | the life of a run's out-of-grant question; the call is dropped and the run goes on (v2dnc) | C | — | quiet notice | — | done (v2dnc) |
| `GATE_LIMIT` `rsi/tree.rs:125,491,544,712-727`; `BUILD_LIMIT` `rsi/join/install.rs:98`; `TESTS_LIMIT` `rsi/join/planted.rs:178`; `lock_wait` `rsi/join.rs:111`, `rsi/revert.rs:93` | 2 h each | `tokio::time::timeout` → SIGKILL of the group → "the gate ran past N minutes and was stopped; its log's last phase line: …" | **C** | no | **yes** (last phase line) | no | **DONE (theseus-x1zz8): renamed `GATE_SILENCE` / `BUILD_SILENCE` / `TESTS_SILENCE` and now bounds silence, with a ceiling of 24x; see the E addendum. The original note follows.** (its own row; `rsi/tree.rs` was in three in-flight branches when this was written, and the door fetch joined at #91, so a row that edits it starts from the merged file).** Turn the 2 h kill into "no new log bytes for N min" with a notice at 2 h; names what it stopped already. The jailed path (`rsi/jail.rs:100-109`) shares the clock. |
| `MAY_BUILD_WAIT` `rsi/tree.rs:131` | 30 min | **notice interval**, not a deadline (done) | — | — | the rule's last words | — | **done (ccik8).** |
| `GRACE` `rsi/pipeline.rs:101`, `FIRST`/`EVERY` of the tenders | 5 min / 10-30 min | tender cadence, not stops | — | — | — | — | not a stop. **But the tenders also hold real deadlines on a model call, which are not cadence:** `tender.rs:850` `timeout(deadline, ask)` fails `"no answer within N ms"`; `tender/pair.rs:145`; `tender/status_cache.rs:64,76`. Clock-only, not read for whether a healthy answer is cut: **unassessed**. |
| `extend` (self-extension): `TRIAL_LIMIT` `extend/mod.rs:87`, `CALL_TIMEOUT_SECS` `:86` | 100 s trial; 20 s per call | the trial is `tokio::time::timeout(TRIAL_LIMIT, &mut task)` (`:645`); on expiry `task.abort()` (`:650-651`) and `"the trial took over 100 s"`. A call fails `"no answer within 20 s"` (`:538-539`) | **C** | no | the duration only | no | **to change.** A **hard abort** of a trial that may be compiling or running tests and is perfectly healthy, with no progress check: the same shape as `GATE_LIMIT`'s 2 h SIGKILL, **about 70 times tighter**. Not in the written pass, which is why section E did not list it. `extend.propose` is in `routine/grant.rs:40`'s `NEVER`, so it is deliberate machinery, not a forgotten corner. Not changed by this row: a first item for the clock-only row. **theseus-x1zz8 examined it and LEFT it**: every `Backend::Harness` call is already abandoned above it at `INPROC_DEADLINE_MS = 120_000` (`toolrun.rs:458`), and a trial waits on a handshake and test calls with their own 20-30 s bounds rather than producing long work. **The residual stands**: a healthy trial is still cut at 100 s, and nothing watches its progress. |
| `UNKNOWN_AFTER` 24 h `sweep.rs:36`, `GIVE_UP` 6 h `rpc/index_rebuild.rs:38`, `BUSY_BOUND` 600 s `memory_pass/mod.rs:103`, `READY_WITHIN` 600 s `lsp/mod.rs:970` | as shown | bookkeeping: an outcome-unknown marker, an index rebuild's retry bound, a memory pass yielding to a busy turn, an LSP start | C | — | logged | — | keep: they stop Theseus's own infrastructure, never the owner's work |
| AWS settle clocks (`aws/stack.rs:45,47`, `bootstrap.rs:73`, `tend.rs:321`, `narrow/live.rs:24`, `session.rs:87-89`; also `aws/durable.rs` `BACKSTOP` 60 s at `:87,1029` and `aws/mod.rs` `BIND_WAIT` 30 s at `:146,789`) and the Google analogue (`google/mod.rs` `BIND_WAIT` 30 s at `:100,461`, `google/auth.rs:165`) | 4-30 min, session 15 min-12 h; 30-60 s binds | a stack operation reports unsettled / a credential session ends / a bind fails as unbound | C | — | yes | — | keep; AWS, no call made |

### B3. Counts, depth and refusals that are gates

| stop | default / key | on fire | C/P | bg | names | bs | state |
|---|---|---|---|---|---|---|---|
| `max_loops` / `on_max_loops` `config.rs:1109-1113`, `limits.rs:67-77`, `budget_step.rs:141` | 40 / **notify** | one notice per multiple, the turn goes on | count | — | yes | — | **done; default is notify.** `end` only if the operator sets it |
| the progress steer / stop `turn/progress_step.rs` (theseus-2uy56, duzha, ikr75) | steer at 50 % of a **caller-given** turn deadline, stop at 85 %, both only after 3+ loops with no passing check | a harness note; the stop ends the turn after one more loop (`progress_stop`); each is a `turn.progress` row with numbers | **P** (a passing check, new ground read, or a background job running or just ended all hold the counts); the stop's own count is reset only by a check | **yes** (duzha) | yes (the note says what was used) | no | **keep.** It only exists for a turn a caller gave `deadline_secs` (`clock_step.rs`: a turn without one has none), so the owner chooses it; and it is progress-based already. No change. |
| task depth | 1 at base and **on `main` today** | refuse `task.create` | structural | — | yes | — | **branch `task-depth` makes it 10, configurable, but it is HELD (see A): it is not on `main`.** The effective depth under the shipped $25 conversation limit is about 3-4, not 10, because the carve quarters at every level (the branch's own test sets `spend_limit_usd = 100_000_000.0` to reach a tenth); and the cycle check on its repeated brief compares the text byte for byte, so it is not a loop guard. |
| `max_wakes` `config/wake_limits.rs:14` | 5 at base, 20 on main | `wake.at` refused | count | — | yes | — | done on main (o1h3f.8); `wake_limits.rs` is another row's |
| a task's repeating wake `wake.rs:69-75` (`TASK_REFUSAL`), `wake.rs:313` | refused | "a task must end … cannot set a repeating wake" | structural | — | yes, and says the one-shot alternative | — | **to change, optional.** Not a dead end (a chain of one-shots works) but a gate; left, because `wake.rs` is touched by `main`'s o1h3f.8 and the owner has not named it. **OWNER CALL.** |
| a task cannot set up a routine: `routine_tool.rs` (lyxn7 (3)) | refused **after** the owner's yes: `plan` raises the card, and `run` returned "a task cannot set up a routine" | refused | structural | — | one sentence | — | **done for `routine_tool.rs`: this row's change (see D).** `task-depth` did not settle it (neither file is in its diff). |
| a task cannot set up a project's review routine: `project/create.rs:184` (`a task cannot set up a routine: give \`review: "none"\``) | `task.create`'s plan naming a project's review routine waits for the owner (`task_graph/tools.rs:346-349`); the owner says yes; `project/create.rs:184` then refuses | refused | structural | — | one sentence | — | **kept: not changed by this row, still a dead end after a yes.** Left on purpose: a retained stop is the safe side, and a second behavioural edit is not a join-sized one. If the owner wants it gone it is a one-line follow-up row. |
| `max_routines` 50, `max_watches` 20, `max_reminders` 100, `max_ahead_days` 365 | | refusal at creation | count | — | yes | — | keep: bounds on standing state, cheap and visible |
| `task.fan_out` | no count cap (by the owner's word); a floor of `helper_floor_usd` | refused only for the floor, depth or bad input | — | — | yes | — | keep |
| external-text hold `external.rs` | until the owner trusts again; under notify one notice | a call that acts waits (ask) or is told once (notify) | — | — | yes | yes (security) | **keep (keel: outward / secrets)** |
| `approve_argv` / `approve_paths`, the floor, private data to CN-origin models, L1 egress for model-requested L1 jobs and self builds, sending outward or to shared places | per `[policy]` | a question | — | — | yes | yes | **keep (keel)** |
| the thread misroute guard | notify, then write | notice + `files.restore` undo | — | — | yes | — | **done (g85jy)** |
| self-improvement fail-open `toolrun/order.rs` (`ToolPolicy::fails_open`) | a self session's held call runs with a notice unless it is the floor / store / spool / layer 1 / AWS / L1 | notice | — | — | yes | yes (floor) | **done; keep the floor** |

## C. Counts by state

Counted from the tables above at the join, one row each, and re-countable by reading them (the written pass's totals
did not reconcile; these do).

Section A: **16 rows**: 15 done on `main` (the arrangement quote only partly: its requirement stays) and 1 held, not
on `main` (task depth).

Section B: **40 rows** (B1 9, B2 18, B3 13), counted once each. A row that repeats a section-A item is counted as
**done** here, and the provider `Timeouts` row's live-stream cut is counted once, under **keep**.

| state | rows | which |
|---|---|---|
| keep | **24** | B1 9; B2 9 (`default_admission_ceiling`, `default_deadline_secs`, inproc deadline, `job_wait`, `fan_out_wait`, provider `Timeouts`, `task_lease`, the `UNKNOWN_AFTER` family, the AWS and Google settle clocks); B3 6 (progress steer, the project-review routine refusal, `max_routines` family, `task.fan_out`, external-text hold, `approve_argv`/`approve_paths`) |
| done | **8** | B2 3 (`confirm_ttl_secs`, routine `run_deadline_secs`, `MAY_BUILD_WAIT`); B3 5 (`max_loops`, `max_wakes`, the misroute guard, the self-improvement fail-open, a task's routine in `routine_tool.rs`, which is this row's change) |
| to change | **4** | admission for a wake or task turn; `proc_timeout_secs`' clock-only hour; the 2 h self-build clocks; `extend`'s 100 s trial abort |
| optional / owner call | **1** | a task's repeating wake |
| held, not on `main` | **1** | task depth (B3; also in A) |
| not a stop | **2** | `heartbeat_secs`; `GRACE` / the tenders' cadence (whose model-call deadlines are **unassessed**, see its row) |
| unknown | **0** | |

24 + 8 + 4 + 1 + 1 + 2 + 0 = 40.

## D. The change this row makes

**A task may propose a routine.** `routine_tool::run` refused when `tc.task.is_some()`; that refusal, and **only that
one**, is removed. The refusal came *after* the owner's yes, so a task raised a card for something it could never do.
The weekly review's refusal in `project::create` (`project/create.rs:184`) is **unchanged and still there**: a task's
plan that names a project's review routine still dead-ends after the owner's yes (B3).

The card stays (`routine.create` is `ASKS`, "always waits" — `task_graph/tools.rs:342`, theseus-pvm5c, and in
`routine/grant.rs:40`'s `NEVER`, so no grant covers it), the owner's yes is the gate, and a routine run cannot make a
routine unasked. What the change removes is the dead end, not the owner's say. **What the card shows is not a bound:**
the routine's `run_usd` / `month_usd` are whatever the proposing model wrote (see B1), so the owner reads them on the
card before saying yes; the stops that hold regardless are the session limit, the burn line and the day ceiling.
Everything else in B stays as listed.

**One interaction to know, read and not run.** A routine's fire opens a task from the *routine's own execution*
(`routines.rs:327` calls `carve_task` with the routine as parent) and the depth bound lives only in the `task.create`
path (`tasks.rs:191-203`). So a task that sets up a routine, once the owner has said yes, gets a fire that the depth
walk does not see. With depth at 1 on `main` each such rung is one owner-approved routine; with the held depth-10 row
it is the reviewer's second blocking finding (theseus-rrqmr). Nothing in this row adds a depth check there.

## E. What was left standing, and why

- **Every row marked keep (keel)** in B1 and B3. No dollar stop was touched.
- `default_deadline_secs`: not a cut (a probe), changing it would change nothing visible.
- `confirm_ttl_secs`: already notify.
- `max_loops`: already notify by default.
- The progress steer and stop: already progress-based, already counts a background job.
- The live-stream whole-call, the 2 h build clocks, admission for wakes and tasks, `proc.run`'s hour and **`extend`'s
  100 s trial abort (B2)** are clock-only stops that should become progress-based. At the time they were not touched
  because each sat in a file another row was editing, or in the job wrapper the sandbox shares; the provider and
  admission rows have since joined, so those two are open to a row now. They are the first items for the next row.

### E addendum, 2026-10-27 (theseus-x1zz8)

Two of those five are now progress-based, and two were examined and deliberately left. Recorded
here because the line above said "the first items for the next row", and this was that row.

- **The may-build RULE keeps a hard clock**, `RULE_LIMIT` 120 s (`rsi/tree.rs`, `Bound::Clock`).
  It is a predicate polled in a loop whose stop check and operator notice both sit outside the
  run, so bounding it on silence would have turned a 120 s window into a 48 minute one and
  delayed a stop aimed at the build by the same factor. Found by the join review, fixed at join.
- **`proc.run`'s hour, and the 2 h build clocks: now bounds on SILENCE**, through
  `crates/theseus-kernel/src/patience.rs`. A job is stopped after its limit of *silence* rather
  than of wall clock; one still printing past the limit runs on and says so in
  `detail.ran_past_limit_ms`, and is stopped at a **ceiling of 24 × the limit**
  (`CEILING_FACTOR`, a constant rather than a config key — say so if the owner wants it
  configurable). Default values are unchanged. The gate, build and test constants in
  `rsi/tree.rs` were renamed `GATE_SILENCE`, `BUILD_SILENCE`, `TESTS_SILENCE` to say what they
  now bound; **anything still citing `GATE_LIMIT` is citing a name that no longer exists.**
- **`extend`'s 100 s trial abort: a real deadline, left alone.** Every `Backend::Harness` call is
  already abandoned by the layer above at `INPROC_DEADLINE_MS = 120_000` (`toolrun.rs:458`), and
  a trial is waiting on a handshake and test calls with their own 20-30 s bounds, not producing
  long work.
- **The tenders' deadlines: real, and not model calls - left alone.** `tender.rs`, `tender/pair.rs` and
  `tender/status_cache.rs` bound a local unix socket to the index tender, not a model call. A
  silent socket is the failure being bounded, and every caller degrades rather than failing.
- **Still open from the list above:** the live-stream whole-call, and admission for wakes and
  tasks. Two residuals from this row are open too: `extend`'s healthy trial is still cut at 100 s
  (the refusal is about the layer above it, not about the trial), and `ran_past_limit_ms` has **no
  reader** outside `probe_run::Logged` - for `proc.run` the value reaches the model in the
  completion's `detail`, but on the RSI and gate paths the "it was let run on, and told" half of
  the design currently goes nowhere, so do not read this section as saying the notice is delivered.
- **Known gap, deliberate:** the `proc.run` tool description and `toolrun/proc_cap.rs` still
  describe the old clock. Rewriting them moves the **locked** DM flow golden (the cost line
  changes), so the edits were reverted and the wording waits on the owner. Suggested text is in
  the row's report.
