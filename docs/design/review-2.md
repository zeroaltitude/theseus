# Theseus review 2: complexity, speed, hardening (theseus-9co)

_Checked in on 2026-10-01 from the agents' working reports. Local paths became plain words or links; nothing else changed. Times are MST._

Reviewed 2026-09-30 from 15:27 MST (clock times from `date`), `main` at `27e1237`.
Read only: nothing in the repo, the state dir (`~/.theseus`), or Beads was changed; no cargo, build,
test, bench, or daemon was run. Scratch counts live in the review's scratch directory, not checked in.

## The short version

Since the first review, the tree has grown from 28,523 lines of Rust to 81,312: production code 2.1 times,
tests 5.9 times, and from about 140 tests to 523. Of the first review's 21 findings, 17 are done, 1 is
partly done (the kernel's open set is not built), and 3 are open, each worse than before (the CLI, the
stringly-typed renderers, and the Discord binding's reach into `Core`). The cuts that a test holds stayed
cut: a plain turn went from 27 frames to 5, and serving comes first. Everything no test holds grew back.
The CLI's `run()` is 879 lines, where the first review found it at 652. `TurnRunner::run` is 260 lines,
where the split left nothing in `turn.rs` over 113.

Speed is good on the start path, which F1 to F4a built:
- 18 ms cold on Eddie's store and 122 ms at 10,000 sessions (release);
- the lifecycle budgets are met.

What remains is work that grows with history *after* serving, and blocking work on the runtime's worker
threads. Hardening found three things worth fixing this week:
- the web UI accepts any origin;
- one web page can abort the daemon;
- raw job output sits world-readable on disk.

**The ten things to do first, in order:**

1. **H1. Refuse cross-origin WebSockets on the web UI.** There is no `Origin` or `Host` check
   (`theseusd/src/web.rs:131`), no auth, and no `[approval]` in Eddie's note. So any page in a browser
   that reaches 127.0.0.1:7433 can read every session, submit turns, approve calls, and clear T1's hold.
   S, LANE.
2. **H2. Fix the HTML reader's byte-index panic, and fuzz the readers of outside text.** `raw_until`
   (`web/html.rs:352`) slices at a byte count. `"</b>中文"` inside a `<script>` panics, and under the
   release profile's `panic = "abort"` a fetched page kills the daemon. S, LANE, plus a panic-policy
   decision (M, SPINE).
3. **H3. Set `umask(0o077)`, make the state dir 0700, and delete or lock down raw job output.** Eddie's
   store and `spool/results/*.out` are world-readable (0644 under 0755 and 0775). The results are unscrubbed,
   and never deleted (17 files, the oldest from 09-28). S, LANE.
4. **S3. Optimize dependencies in debug builds, and keep the bench's p95 history.** The gate's debug
   cold-start p95 has read 56.7 ms against its 57 ms line. The FAST gate is timing unoptimized serde and
   sha2. S, LANE.
5. **C1. Put a shape budget in the gate.** `too_many_lines` and `cognitive_complexity`, with `#[expect]` on
   today's 36 functions over 100 lines, and a file ceiling. Nothing else keeps the cuts cut. S, LANE.
6. **S2 and R3. Take the blocking work off the runtime's workers, and cap job output.**
   - `/stop` and cancel sleep-poll up to 2.5 s per job on a tokio worker (`job.rs:438-465`).
   - A job's whole output file is read into memory (`toolrun.rs:258`), and it has no size cap.

   S, LANE.
7. **R1 and R4. Close the books on every exit after a paid loop, and let a corrupt record degrade, not
   stop.** A raw `?` after a paid call skips the session's cost (`turn.rs:1170-1212`). One refused record
   silently stops every continuation (`harness.rs:113`). S, SPINE.
8. **H4, R5, and R9. Fix three small bugs that content can trigger.**
   - `git.diff` reads through symlinks, past the roots and the floor (`git.rs:234-235`).
   - Discord's splitter can loop forever on a long code-fence line (`render.rs:1227-1256`).
   - A FIFO in the roots holds a CPU permit forever (`fs.rs:136-152`).

   S, LANE.
9. **S1. The kernel's open set (theseus-lv2).** The driver decodes every execution ever written on a
   500 ms tick, forever. At 10,000 executions that is 25.6 ms per pass, and health takes about 70 ms. The
   Observatory pulls every execution every 2.5 s. M, SPINE.
10. **C3, then C2 with C6, before M5.**
    - Now: typed notification payloads, a generated `protocol.ts`, and one `Tool::describe` in place of
      the four tool describers. M, LANE.
    - Then, before Jev writes its first judgment row: one typed event per fact, replacing the five or six
      hand-written channels, and a kernel transaction replacing the combined `a_and_b` transitions. L,
      SPINE.

theseus-kol (R2) is already queued: fix batch 1, ahead of the dogfood pilot. Keep it there: a failed
continuation still eats its input (`turn.rs:1486`). Items 1 to 6 and 8 are lanes that can run beside the chain today. Items 7 and 9 are spine
work. Item 10's second half is the one large reshaping I would do before M5.

## Considerations for Eddie

Questions of direction, not code:

1. **Panic policy.** The release profile aborts on any panic, and your daemon has no supervisor. Do you want
   `panic = "unwind"` with per-task isolation, or abort plus a crash file and a user systemd unit that
   restarts it? The 20 ms start makes a restart nearly free.
2. **What "no `[approval]`" should mean.** Today it means that every surface may approve, trust, and reset
   spend. Should the default be your CLI and your Discord DM only, with the web UI added only by name?
3. **Dogfooding before separation.** theseus-14s puts a builder daemon's jobs, as your uid, in reach of
   `~/.local/bin/theseusd`, the repository, and the gate script. Should the builder run as another uid, or
   in L1, until M4's control-plane separation lands?
4. **How wide the hold is.** T1 holds a session only for the web tools' text. Should strangers' repository
   content (`git.log`, `git.diff`, and `fs.read` in a clone with a foreign remote) and Discord attachments
   hold it too? It is safer, and it will ask more often.
5. **The licence rule.** §1 says "permissive-only dependencies", but `deny.toml` allows MPL-2.0, and one
   crate uses it (`option-ext`, under `shellexpand`). Amend §1 to allow file-level copyleft, or drop
   `shellexpand` and take MPL-2.0 off the list?
6. **Shape as a gate rule.** Do you want a function-length and complexity budget held by the gate, the way
   the frame budget is? It keeps the codebase simple, at some friction per step.
7. **A large refactor with no new feature.** The typed-event recorder (C2) and the kernel transaction (C6)
   change no behaviour, and cost about two chain steps each. They pay off at M5 and M7. Do them before M5,
   or accept a sixth hand-written channel for judgments?
8. **Per-turn budgets.** §9's "per-turn harness overhead under 5 ms" cannot hold on this disk, where one
   fsync is 7 ms. Should the budget be restated in frames per turn, which does not depend on the machine
   (2 is the floor), and gated like the start?
9. **Group commit through a writer thread** (S2) makes callers wait for a batch's fsync instead of their
   own. Durability is the same, and latency is lower under load. Is that trade acceptable for the kernel's
   commit path?
10. **Verifiable binaries.** Once Theseus builds Theseus, should an installed `theseusd` be reproducible
    from its commit (SC3), and checked at install?

## 0. Where the first review's 21 findings stand

The first review (theseus-hco, 8a1e41d) found 21 things. I checked each one against the code at `27e1237`,
not only against the spec:
- **17 are done**, though the functions behind two of them (4 and 18) have grown long again;
- **1 is partly done**: finding 1's quadratic is gone, but its open set (1b and 1c) is not built;
- **3 are open** (11, 12, and 19), and all three are worse than before.

| # | Finding (short) | Status | Evidence at 27e1237 |
|---:|---|---|---|
| 1a | `session.list` re-decoded every action once per session | **Done** | 95ee929 counts open actions once per list. |
| 1b | Kernel open set: `open_*`, `stats`, the reconcile, and the driver read all of history | **Open** (theseus-lv2) | `kernel.rs:538-561` still decodes every EXECUTION or ACTION record and then filters. Callers: the driver's 500 ms tick (`harness.rs:113`), `stats` (`kernel.rs:576-594`, used by health), the reconcile (`kernel.rs:2260,2272`), `stops.rs:81`, `wakes.rs:275`, `tasks.rs:290`, and `execution.list`/`action.list` (`rpc/methods.rs:749,760`). F4a measured 25.6 ms for the kernel's load step and about 70 ms of `health` at 10,000 sessions. |
| 1c | The driver's 500 ms tick should go once (b) lands | **Open** (it depends on 1b) | `harness.rs:104`: `interval(500 ms)`, and every tick calls `open_executions()`. |
| 2 | Delete the hook system | **Done** | 11d2f43. `hooks.rs` is gone. |
| 3 | 27 fsyncs a plain turn: batch the rows that are not state transitions | **Done, and beyond the target** | 11d2f43 (the hooks) took it to 17 frames, 6402e80 to 8, and 27e1237 to 5. `tests_m3.rs:1225` asserts `frames <= 5`. |
| 4 | `run_inner`: 785 lines, 9 parameters | **Done, then regrew** | 54b0918 split it (longest function 113 lines). Now `TurnRunner::run` is 260 lines (`turn.rs:763`), `run_inner` 194 (`turn.rs:1027`), `call_model` 162, `settle_call` 134, and `compile_step` 128. `turn.rs` went from 1,247 to 2,651 lines: narration added 390 (e3ba8d6), budgets 246, serve-first 159, and tasks 140. See C1 and C2. |
| 5 | Finish the gate teardown | **Done** | bb3ce56. `kernel/src/gate.rs` keeps `digest_json` and the proposal. |
| 6 | The kernel's second session model and dead API | **Done** | b242258. No `promote` or `session_tail` is left. |
| 7 | Startup decoded every node; a GitHub call blocked the start | **Done** | 95ee929. F1 made serving come first. |
| 8 | Each turn decoded the transcript three or more times | **Done** | 49c4db4: one read per turn, extended by the turn's own frames. A debug build asserts it equals a fresh read. |
| 9 | Two canonical-JSON copies | **Done** | bb3ce56: `digest_json`, with a golden-digest test (`gate.rs:67`). |
| 10 | `rpc.rs`: `Core` did six jobs and `dispatch` was 496 lines | **Done** | 69851ad: `rpc/` holds 9 files, and `dispatch` is a table (`rpc/server.rs:166`). `Core::build` is now 195 lines (`rpc/mod.rs:355`). |
| 11 | The CLI's `run()` was 652 lines with no tests | **Open, and worse** | `theseus/src/main.rs:513` `run` is **879 lines**, with 18 subcommand arms. `Printer::on` is 208 lines (`:2456`). The file went from 1,597 to 3,194 lines. It has 13 tests now, all on line formatting. |
| 12 | Stringly-typed JSON between the core and its renderers | **Open, and wider** | Discord's `on_notification` is 273 lines (`render.rs:243`) and still matches literal strings (`"turn.started"`, `render.rs:246`) instead of the `notify::*` constants. `web/src/protocol.ts` is 458 lines (231 before), 372 of them hand-written type mirrors. Tool inputs are described separately in 4 places (see C3). |
| 13 | "Pending confirm" computed four ways | **Done** | 750ec12: `Kernel::pending_confirms` is the only derivation. |
| 14 | OpenTelemetry compiled into every binary | **Done, differently** | O1 (6ad89a2): a native OTLP/JSON exporter. `opentelemetry-proto` is a dev-dependency only (`theseus-core/Cargo.toml:35-38`). |
| 15 | A second index engine (fjall) | **Done** | ea06ff8. |
| 16 | Config shims, string enums, maps cloned on every read | **Done**, with 2 kept on purpose | b407dd3. `all_profiles()` and `all_providers()` return references (`config.rs:1162,1215`). `default_budget` and `control_reserve` still load with a warning, because Eddie's note sets them (`config.rs:464-513`). |
| 17 | The simulator ran in no test and no gate | **Done** | b242258: `theseus-sim/tests/sim.rs`. The crash test's `--tear` still defaults to off (theseus-4x6). |
| 18 | `toolrun.rs`: an 11-argument constructor and a 250-line `resume` | **Done, then regrew** | 6e76a5f. The file went from 1,355 to 2,399 lines. `run_job` is 164 lines (`toolrun.rs:1342`) and `run_inproc` 137 (`:1155`). `run_group` takes 7 parameters (`:681`). |
| 19 | The Discord binding "is just a client" but reaches into `Core` | **Open, and worse** | `lib.rs:1-3` still says "a protocol client of the core, exactly like the web UI and the CLI". Production code makes **37 direct `core.*` calls** (26 in `runtime.rs` and 11 in `courier.rs`, not counting `clone`), against 13 before. They reach `core.kernel.action` (`courier.rs:544`), `core.outbox.*` (11 calls), `core.config_gate`, `core.secrets`, and `core.approval`. The doc also still says `/stop` goes through `execution.cancel`; since W1 it goes through `execution.stop` (`runtime.rs:1957`). |
| 20 | Policy leftovers | **Done**, with `[policy.mcp]` kept by Eddie | f8a68c2. |
| 21 | Small dead code and lints | **Done** | cf258c7. The workspace `[lints]` table holds `unused_async`, `unused_self`, and `redundant_clone` (`Cargo.toml:14-17`). |

**What that means.** The cuts held where a test holds them: the frame budget went from 17 to 8 to 5, and
serving comes first. They slid back where nothing does. Every "giant function" the first review split has
regrown, because no gate checks function length or file size. The three findings left for "after the Daily
Driver" (11, 12, and 19) are exactly the surfaces the Daily Driver grew fastest: the CLI by 1,597 lines,
Discord's `render.rs` and `runtime.rs` by 2,758, and the protocol by 974.

## 1. Complexity

### Growth since the first review, and where it went

| measure | 8a1e41d (first review) | 27e1237 (now) |
|---|---:|---:|
| Rust lines, total | 28,523 | 81,312 |
| production / test, cut at a file's first `#[cfg(test)]` (the first review's method) | 22,803 / 5,720 | 47,838 / 33,474 |
| production / test, cut at the first `#[cfg(test)] mod … {` (more exact: `rpc/mod.rs` declares `mod tests;` at line 20) | n/a | **49,646 / 31,666** |
| tests (`#[test]` and `#[tokio::test]`) | 136 at the first cut (95ee929) | **523** |
| production functions over 60 / 100 / 150 lines (brace-matched count, simulator excluded) | 54 over 60 (clippy, tests included) | 92 / 36 / 16 |
| kernel `pub fn`s in `kernel.rs` | 44 (36 after the dead-API cut) | 50 |
| packages in `Cargo.lock` | 325 external crates (by `cargo tree`) | 365 lockfile packages, which include dev and other-target ones (O1 counted 292 in `theseusd`'s normal build) |
| `theseusd` release binary | 24.3 MB before batch C (48fd2c8), 19.0 MB after | 21.7 MB (installed at 14:09) |
| cargo features in workspace crates | `otel` came and went | none (no `[features]` in any crate) |

Production code has grown 2.1 times; tests have grown 5.9 times. Half the production code is `theseus-core`
(23,811 lines). The growth came from features, not rot. 49 new production files landed (75 with tests),
among them `approval`, `peer`,
`broker`, `outbox` (core and kernel), `wake`/`wakes`, `task`/`tasks`, `external`, `config_gate`,
`config_copy`, `narrative`, `attach`, `web/*`, `children`, `stops`, `courier`, `files`, `viewers`, and the
simulator's `lifecycle` and `fake_discord`. Five existing files each grew by more than 1,000 lines:

| file | then → now | what added it |
|---|---:|---|
| `theseus/src/main.rs` (CLI) | 1,597 → 3,194 | a subcommand and a health line per feature |
| `theseus-discord/src/render.rs` | 934 → 2,390 | budgets, cards, wakes, tasks, reports, refusals, notices |
| `theseus-core/src/turn.rs` | 1,267 → 2,651 | narration +390, budgets +246, serve-first +159, tasks +140, W1 +122 |
| `theseus-discord/src/runtime.rs` | 1,401 → 2,703 | the outbox, slash commands, viewers, trusted-channel checks |
| `theseus-core/src/toolrun.rs` | 1,355 → 2,399 | parallel calls, the broker, web tools, the hold |

The 16 production functions over 150 lines: the CLI's `run` 879 (`theseus/src/main.rs:513`),
`on_notification` 273 (`render.rs:243`), `TurnRunner::run` 260 (`turn.rs:763`), `daemon` 213
(`theseusd/src/main.rs:189`), `on_interaction` 213 (`runtime.rs:1001`), `Printer::on` 208 (`main.rs:2456`),
`Config::validate` 207 (`config.rs:919`), `Kernel::startup` 199 (`kernel.rs:2333`), `Core::build` 195
(`rpc/mod.rs:355`), `run_inner` 194 (`turn.rs:1027`), `accept_locked` 185 (`kernel.rs:1885`), `end_turn_with`
172 (`kernel.rs:943`), `serve` 168 (`runtime.rs:236`), `git.diff`'s `run` 164 (`git.rs:180`), `run_job` 164
(`toolrun.rs:1342`), and `call_model` 162 (`turn.rs:1634`). Parameters stay in check: clippy's default
threshold of 7 holds everywhere but one `#[allow]` (`catalog.rs:239`, 9 arguments), and 6 functions take 7.

### Ranked findings

#### C1. Nothing holds the shape, so every split the first review made has regrown

- **Evidence.** Section 0: findings 4, 10, 11, 18, and 19 were cut or left for later. The split left no
  function in `turn.rs` over 113 lines (54b0918); now its `run` is 260 and `run_inner` 194. The CLI's `run`
  went from 652 to 879. The frame budget held (17 → 8 → 5) because a test holds it
  (`tests_m3.rs:1225`). No test or lint holds function length, file length, or complexity. The workspace
  `[lints]` table has three lints (`Cargo.toml:14-17`), and none is about size.
- **Proposal.** Add clippy's `too_many_lines` (threshold 100) and `cognitive_complexity` (25) to the
  workspace lints. Mark today's offenders with `#[expect(clippy::too_many_lines, reason = "…")]`, so the
  list can only get shorter: `expect` fails once a function is fixed. My raw count finds 36; clippy counts
  only code lines, so it will find somewhat fewer. Add a 20-line check to `gate.sh` that
  fails when a production file passes 2,500 lines without an entry in a checked-in list.
- **Payoff.** It keeps every later cut (C2 to C7) from sliding back, at no cost to run time. **Risk.** It is
  friction for the chain, and wrong thresholds breed `expect` noise. Start loose and tighten.
- **Size** S. **LANE.** **Do first**: it is an hour's work, and it protects everything else.

#### C2. One fact is written by hand in five or six channels, and narration made it worse

- **Evidence.** The first review counted four hand-written channels for each fact: a trace span, a ledger
  row, a notification, and a hook site. The hooks went, and narration took their place, plus the outbox and
  OTel:
  - `turn.started` is written three ways by hand: the notification (`turn.rs:343`), the ledger row
    (`turn.rs:355`), and a narrative line. Discord matches it again by a literal string (`render.rs:246`).
  - The core has about 98 `narrate!`/`narrate_turn!` sites, 75 ledger writes, and 28 notification sends.
    `turn.rs` alone has 43 narration lines, 10 ledger rows, 13 notifications, and the trace. e3ba8d6
    (narration) added 390 lines to `turn.rs`, the most of any commit.
  - About 83 distinct ledger kinds are bare strings, with no registry or enum
    (`scratch/ledger_kinds.txt`). 11 of the core's 22 notification sends build an ad-hoc `json!`.
- **Proposal.** One typed `Event` per fact, emitted once into a recorder whose projections write the
  ledger row, the notification, the narrative sentence (`impl Display`, or a narrator function per
  variant), the trace span, and the OTel counters. The ledger kind and the notification method become
  derived names, so a registry test can list them. Keep payloads byte-identical, and prove it with batch
  C's output-diff probe.
- **Payoff.** It is the largest structural lever left. `turn.rs`, `toolrun.rs`, and `rpc/confirms.rs` shed
  their parallel bookkeeping, and the renderers can take typed events (C3). M5's judge rows and M4's labels
  then plug in once, instead of adding a sixth and seventh channel. **Risk.** It is a large, mechanical
  change across the spine, and ledger rows are durable, so old kinds must keep decoding.
- **Size** L. **SPINE.** Before M5 begins writing judgment rows.

#### C3. The three renderers each re-derive notifications and tool calls from untyped JSON

- **Evidence.**
  - Discord's `on_notification` is 273 lines of literal method strings (`render.rs:243-515`). The CLI's
    `Printer::on` is 208 lines (`main.rs:2456`). `web/src/protocol.ts` has grown from 231 lines to 458;
    lines 4–375 are 52 hand-written type declarations that mirror the Rust types. The protocol crate has 92
    structs and 0 tests.
  - "What does this call do" is written four times, each by probing JSON keys:
    - `narrative::subject` (`narrative.rs:267`), which toolrun's `subject` feeds (`toolrun.rs:494`);
    - Discord's `summarize` (`render.rs:1131`: `argv`, `pattern`, `path`, `rev`, `file`, `url`, `query`);
    - the web UI's `callSummary` (`Transcript.tsx:51-67`, one `case` per tool);
    - the CLI's `print_node` (`main.rs:1844`, 120 lines).
- **Proposal.**
  1. Give each of the 21 `notify::*` methods a payload struct in `theseus-protocol`, and have the renderers
     match on an enum, not on strings.
  2. Generate `protocol.ts` from the Rust types (`ts-rs`, or a JSON Schema step), with a gate check that
     the generated file is committed. This is the same pattern as the `web/dist` check.
  3. Add `Tool::describe(&self, input) -> String` to the toollet trait, since the toollet knows its schema.
     Send the result as `summary` on `tool.proposed`, `tool.started`, and `tool.ended`, and store it on the
     node. Every renderer then prints the same line.
- **Payoff.** A renamed field fails the build instead of blanking a Discord line, four describers become
  one, and a new tool renders correctly everywhere on day one. **Risk.** Low: rendering only. Old nodes
  without `summary` fall back to today's code once, in one place.
- **Size** M. **LANE**, with small touches to the core's send sites.

#### C4. The CLI's `run()` is 879 lines

- **Evidence.** `theseus/src/main.rs:513-1391`: 18 subcommand arms inline, each repeating
  `conn.call`, `from_value`, and print. The file has 13 tests, all on health and line formats
  (`main.rs:2754-3194`).
- **Proposal.** One module per subcommand group (`ask`, `sessions`, `confirm`, `policy`, `tasks`/`wakes`,
  `health`, `ledger`), each with `async fn run(args, &mut Conn, &Out)`. `Printer` becomes a per-notification
  function table once C3's enum exists.
- **Payoff.** It is the most branchy function in the tree, and the next surfaces (`theseus tui`,
  theseus-7yx) want its pieces. **Risk.** Low. The output is text; snapshot the `--json` and text outputs of
  each subcommand against a scratch daemon first.
- **Size** S to M. **LANE.**

#### C5. The Discord binding says it is a protocol client, but it reaches into `Core` 37 times

- **Evidence.** `theseus-discord/src/lib.rs:1-4` says "a protocol client of the core, exactly like the web
  UI and the CLI". In production code it makes 26 direct `core.*` calls in `runtime.rs` and 11 in
  `courier.rs` (13 at the first review): `core.outbox.*` (11), `core.kernel.action` (`courier.rs:544`),
  `core.kernel.outbox_action` (`:619`), `core.kernel.pending_confirms`, `core.config_gate.*`,
  `core.secrets.subscribe`, `core.approval.trusts_dm`, and `core.binding_ledger` (11). The same doc still
  routes `/stop` through `execution.cancel` (W1 changed it; `runtime.rs:1957`).
- **Proposal.** Say what is true: the binding is in process, because durable delivery (DD6) needs it to
  be. Name the seam as a `BindingPort` trait (outbox lanes, the approval check, the binding's ledger rows,
  the config gate, and secrets), implemented by `Core`. The binding depends only on that trait and the
  protocol. Fix the doc.
- **Payoff.** The binding can be tested against a fake port, and M7's voice binding has a seam to follow,
  not 37 calls to copy. **Risk.** Low. **Size** M. **LANE.**

#### C6. Merging frames is adding combined kernel transitions, one per merge

- **Evidence.** F2 and F2b took a plain turn from 17 frames to 5, which is excellent. They did it by
  adding combined functions: `admit_input` (wake and admit), `authorize_and_dispatch`, `plan_and_dispatch`,
  `plan_frame`, and the `_with` family (`plan_action_with`, `plan_confirm_with`, `accept_completion_with`,
  `end_turn_with`, `cancel_execution_with`). `kernel.rs`'s public functions went from 44 at the first
  review to 36 after the dead API went (b242258), then 43 at F2 (6402e80), 46 at de880fc, and 50 at
  27e1237. `end_turn_with` (172 lines) and `accept_locked` (185) are among the longest in the tree.
- **Proposal.** A kernel transaction: `kernel.frame(&[execution ids], |tx| { tx.wake(..)?; tx.admit(..)?;
  Ok(()) })`. It takes the K1 locks in id order, lets the ordinary transitions stage their records in
  memory, and commits one frame at the end. The combined functions become compositions, and the core's
  `Store::for_turn` deferral can ride the same frame.
- **Payoff.** Frames keep falling without the API growing, and M5's judge recorder has somewhere to put
  its records ("in batched frames", per the M5 design) without a second commit path. **Risk.** It touches
  the kernel's central invariants (K1, crash recovery). kernel-sim with `--p-race` and the crash test must
  cover it. **Size** L. **SPINE.** Pair it with C2, or right after.

#### C7. The test suite is big, rig-heavy, and untimed

- **Evidence.**
  - 523 tests in 31,666 lines, which is 64% of production. `tests_m3.rs` is the largest file in the tree
    (5,422 lines).
  - Nine daemon test files each build their own rig: 12 fake `op` shell scripts in `theseusd/tests/*.rs`
    (count of `#!/bin/sh`), while the shared `common/` is 244 lines.
  - The gate's wall time is not recorded anywhere since 2026-09-29 (16 s warm in the 2a review, 33 s at
    10:29 on 09-29; the agents' operating notes for the repo).
  - 473 tests passed "on the third run" once (DD8 report §8).
- **Proposal.**
  - A shared rig builder in `theseusd/tests/common`: the config, a fake `op`, the fake model, the fake
    Discord, and the socket, each opted into.
  - Split `tests_m3.rs` by subject.
  - `gate.sh` prints each phase's seconds (fmt, clippy, nextest, the bench, deny, and the web build).
  - Add `[profile.dev.package."*"] opt-level = 2`, which also helps S4.
  - Keep a flaky-test list from nextest's retries, so a re-run is recorded instead of forgotten.
- **Payoff.** Faster, cheaper test writing for every step, and a gate whose own cost is visible.
  **Risk.** Low. **Size** S to M. **LANE.**

#### C8. `Config::validate` is 207 lines, and every feature adds a table

- **Evidence.** `config.rs` went from 1,172 to 2,110 lines. `validate` (`config.rs:919`) is 207. The
  template test that un-comments every line is the right guard, and it holds.
- **Proposal.** Move validation beside each table's struct (`impl Validate for BrokerConfig`), so a feature
  lands with its own validation, and `validate` becomes a list.
- **Payoff/Risk.** Readability, and low risk. **Size** S. **LANE.** Low priority.

### What M4 to M7 will make harder unless something is reshaped first

- **M4 (labels, compile-time confidentiality, L1).** Every node gains a label, and a withheld result shows
  as a placeholder. Each of the four tool describers and three renderers would learn placeholders
  separately. C3 first makes that one change.
- **M5 (Jev).** The design has shadow judgments writing "in batched frames by the judge's own recorder"
  ([the M5 design](m5-judgment.md), the key question). Without C2 and C6, that is a second frame
  writer beside the turn, and a sixth hand-written channel (the judgment row, its narration, its span,
  its notification...).
- **M6 (memory).** Selection over the whole graph needs reads that cost O(selected). Today's model is one
  whole-session transcript kept per turn (F2), and O(all) kernel scans (S1). The projection work in S1
  (theseus-lv2) is the same muscle, so build it once, generally: an index by state and scope.
- **M7 (surfaces, voice, TUI).** A fourth renderer. Without C3 and the protocol push (theseus-in3, roadmap
  stage 1 item 9), it re-derives everything again, and polls (S1).

## 2. Speed (FAST)

### Where it stands (numbers from the step reports)

| | empty store | Eddie's copy (233 executions, 253 actions, 21 sessions) | 10,000 parked sessions | §9 budget |
|---|---:|---:|---:|---:|
| cold start to first `health`, release p50 (F4a) | 17.5 ms | 18.4 ms | 121.7 ms | 50 / 250 ms |
| clean shutdown, release p50 (F4a) | 36.9 ms | 32.6 ms | 24.8 ms | 100 ms |
| SIGKILL, then restart, release p50 (F4a) | 33.3 ms | 34.6 ms | 138.6 ms | 150 / 350 ms |
| cold start in the gate, **debug** p95, in time order | **56.7 ms** (DD8, 4c6b72c, 09:49), 42.8 (T1, 12:15), 48.6 (T1, 12:27), 38.8 (F4a, 13:32) | | | 50 + 7 ms margin |
| a plain turn's frames (one fdatasync each, about 7 ms on this disk) | 5 since 27e1237 (27 at the first review, 17, then 8) | | | |
| per-turn harness overhead | about 58 ms at 8 frames (F2); about 35 ms at 5 by the same arithmetic | | | under 5 ms (§9; the disk) |

F1 to F4a did what FAST asked: serving comes first, the network is off the start path, and the store's
open reads only the WAL's tail. What is left is work that grows with history *after* serving, and work
that blocks the runtime while it serves.

### Ranked findings

#### S1. Four readers decode every execution or action ever written, and two of them never stop (theseus-lv2)

- **Evidence.**
  - `Kernel::executions`/`open_executions`/`actions`/`open_actions` decode every record of the kind and then
    filter (`kernel.rs:538-561`). `stats` decodes both, and health uses it (`kernel.rs:576-594`).
  - The continuation driver calls `open_executions()` on a **500 ms tick, forever**, on a tokio worker
    (`harness.rs:104-113`). The reconcile does too (`kernel.rs:2260,2272`), as do `/stop` (`stops.rs:81`),
    `fire_due` (`wakes.rs:275`), and tasks (`tasks.rs:290`).
  - F4a measured the kernel's load step at **25.6 ms for 10,000 executions**, and about **70 ms of
    `health`** at 10,000 sessions (F4a report, "M3.5's exit test"). The driver's tick does the same decode as
    the load step, so at 10,000 executions it is about 5% of a core, idle, by that measure.
  - The Observatory polls every 2.5 s while live (`Observatory.tsx:92-98,142`):
    - `execution.list` returns **every** execution, with no limit (`rpc/methods.rs:746-752`);
    - `action.list` decodes every action, sorts, and keeps 200 (`:754-770`);
    - `session.list`, `ledger.tail` 400, and `tool.list`.

    The app also polls `session.list` every 5 s (`App.tsx:205`).
  - On Eddie's store, about 500 records, all of this is under a millisecond. It grows with every turn, since
    each loop adds an action.
- **Proposal.** The first review's 1b, which is still the design:
  - An in-memory open set (executions not terminal, actions not settled), built by `startup`, which already
    walks both, and updated in the same call that commits each transition. It is never persisted.
  - `open_*`, the stats counts, the reconcile, and the driver become O(open).
  - `execution.list` and `action.list` page newest-first from the index: `positions_of_kind_rev` already
    exists (`index.rs:196`).
  - The driver wakes on `admission` and on the next due wake, with a 30 s safety tick.
  - The Observatory moves to the protocol push (theseus-in3).
- **Payoff.** Idle cost goes from O(history) to O(open). At 10,000 sessions, about 25 ms off every start's
  load step and about 70 ms off the first answer. It also removes the last reason for the 500 ms tick.
  **Risk.** Medium: the set must match the store after a crash, which is why it is rebuilt in `startup`.
  kernel-sim can assert "open set equals scan" at every step.
- **Size** M. **SPINE** (kernel). The most valuable speed item. It comes after the S-sized fixes in the
  short version only because they are hours and this is a step.

#### S2. The runtime's workers block on fsync, on condvars, and on a 2.5 s sleep-poll

- **Evidence.**
  - **Fsync on workers.** Every WAL append runs its `fdatasync` inline, on whatever thread called it
    (`wal.rs:482`). Kernel transitions are synchronous and are called from async code: the turn and the
    tool runtime, and every RPC handler, which runs inline in a spawned task (`rpc/server.rs:129,166-230`).
    Group commit's followers wait on a `std::sync::Condvar` (`wal.rs:516-548`).
  - **Condvar locks.** The K1 execution lock and the session-record lock also wait on std condvars
    (`locks.rs:62-95`; `store.rs:95-120`), held across an append and its fsync.
  - **The checkpoint.** Every 1,000 records, the unlucky append runs a checkpoint inline
    (`store.rs:505-511`). It takes the `appending` lock for writing, which stalls every other appender, and
    commits redb durably (two fsyncs, about 14 ms) (`store.rs:574-581`).
  - **A 2.5 s sleep-poll.** `execution.cancel` and `/stop` call `terminate_all`, then `job::terminate` for
    each job, serially (`rpc/driver.rs:165,247`). It polls with `std::thread::sleep(10 ms)` for up to 2 s
    after SIGTERM, then 0.5 s after SIGKILL (`job.rs:438-465`). A job that ignores SIGTERM holds the worker
    for 2.5 s.
  - **A whole file into memory.** `read_result_file` reads a job's entire output file on the worker, and
    then keeps the last 4 MiB (`toolrun.rs:254-270`).
  - The daemon runs tokio's default multi-thread runtime, one worker per core (`theseusd/src/main.rs:161`;
    16 here). No bench runs concurrent turns, so none of this has been seen. It is invisible for one user,
    and a real tail at §2's target of 50 active executions.
- **Proposal.**
  1. Now (S): run `terminate_all` off the worker (`spawn_blocking`, or async with `tokio::time::sleep`),
     and read only the tail of a result file (`seek` to `len − 4 MiB`).
  2. Next (L): a single store-writer thread that owns appends. Callers send a frame and await a oneshot, and
     the writer commits whatever is queued with one fsync: true group commit across turns, and no worker
     ever waits on the disk. Kernel transitions stay synchronous inside it, so K1's thread-keyed locks keep
     their meaning.
  3. Move the checkpoint to a tender after serving (next to theseus-02k).
- **Payoff.** Tail latency under load stays flat, and §9's "WAL commit latency at the group-commit interval"
  becomes something a design can meet, since today's group commit only helps when two appends overlap by
  chance. **Risk.** (1) is low. (2) changes the commit path's threading: crash-test and kernel-sim with
  `--p-race` must cover it. **Size** S, then L. **SPINE.**

#### S3. The gate's FAST check has no headroom, and it measures the debug build more than the start path

- **Evidence.** Debug cold-start p95 in the gate, in time order: **56.7 ms against 57 (50 + the 7 ms
  margin) at 4c6b72c** (DD8 report §8, 09:49), then 42.8 and 48.6 at T1 (12:15 and 12:27), then 38.8 at
  F4a (13:32). The spread is 18 ms, and its worst case was 0.3 ms from failing. Release is 17.5–18.4 ms.
  The bench runs debug binaries (`gate.sh`: "never faster than release"), so unoptimized serde, sha2, and
  redb dominate what it times. The retry-on-miss (5240563) is right for writeback noise, but it also hides drift. No commit
  records its p95s, only pass or fail.
- **Proposal.**
  - Add `[profile.dev.package."*"] opt-level = 2`: dependencies optimized, workspace crates still debug, so
    F2's debug-only transcript assertion stays. Re-measure the margins after.
  - Append each gate's bench p95s, with the commit, to a CSV in the repo (or print them), so drift shows
    before it fails.
- **Payoff.** The FAST gate measures the start path, stops failing on noise, and runs the test suite faster
  too (C7). **Risk.** Low. The first build after the change recompiles dependencies once. **Size** S.
  **LANE.** **Do first**: the next start-path feature fails the gate as it stands.

#### S4. The benches measure start and stop; the turn, idle time, and load are not measured

- **Evidence.** §9 rows with no bench:
  - per-turn harness overhead (under 5 ms): F2 worked it out by hand at about 58 ms, 8 fsyncs;
  - WAL commit latency;
  - RSS at 10,000 parked and 50 active (under 1 GB);
  - binary size (under 60 MB; 21.7 MB now, unchecked).

  Also unmeasured:
  - idle CPU at 10,000 sessions (S1's tick);
  - concurrent turns (S2);
  - long-session turns: F2 measured each loop's whole-transcript render at +7.9 ms at 200 nodes, and
    §4.5's rendered-prefix cache is not built;
  - Discord delivery latency;
  - health and the Observatory at 10,000 sessions.
- **Proposal.**
  - `theseus-sim bench turn --sessions N --concurrent K`: the fake model, frames and fsyncs per turn, p50
    and p99 turn latency, and a canary task that measures its own scheduling delay, which shows worker
    stalls.
  - `bench idle`: the daemon's CPU seconds over 30 s at 10,000 parked sessions.
  - RSS sampled in the lifecycle bench, and the binary's size checked in the gate.
  - All of it appended to the same history file as S3.
- **Payoff.** FAST reaches the turn and idle time, where Eddie actually waits. **Risk.** Low. **Size** M.
  **LANE.**

#### S5. A plain turn's floor is 2 frames, not 5, and each loop redoes work that rarely changes

- **Evidence.** 27e1237's plain turn: `queued+running | turn.started+node | the provider call's plan,
  authorization, and dispatch | its completion | loop.ended+session+turn.ended+turn.trace+waiting`. The
  first three frames are local work with no network between them. §1 needs a fsync before a dispatch, and
  one after the answer, so the floor is **2**: everything up to the dispatch, then the completion and the
  turn's end. Each loop also hashes the whole system text and clones and digests the whole tools array
  (`compiler.rs:185-217`, `manifest_for`). Both change only when the profile, tools, or context files do.
- **Proposal.**
  - With C6's kernel transaction, merge the first three frames, and let a plain turn's completion ride
    with its end frame. That is 5 → 2, about 21 ms a turn on this disk.
  - Cache the manifest's digests per request spec.
  - Build §4.5's rendered-prefix cache when long sessions matter.
- **Payoff.** About 20 ms per turn, and flat per-loop cost. Minor next to a model's seconds, but it is
  the disk time the operator waits through. **Risk.** The merged frames' crash semantics need the crash
  test: a crash between the answer and the end frame must still leave the call's cost reconcilable.
  **Size** M, after C6. **SPINE.** Lower priority than S1 to S3.

#### S6. Notifications fan out through unbounded channels, deep-cloned per watcher

- **Evidence.** Each connection has an `mpsc::unbounded_channel` (`rpc/server.rs:84`). `SessionBus::publish`
  clones the `Message`, which is a `serde_json::Value` tree, for every watcher, and each connection
  serializes it again (`bus.rs`, `publish`). `model.delta` is sent per token. A reader that stops (a
  sleeping laptop's tab, a wedged `watch`) grows the daemon's memory by every delta of every session it
  watches. This is R6 as well.
- **Proposal.** Serialize once (`Arc<str>`) and share it. Bounded queues per connection, where live-only
  notifications (deltas, tool progress) are latest-wins when a client lags, as DD6's live ops already are,
  and a client that stays behind is disconnected.
- **Payoff/Risk.** Less CPU per token, and bounded memory. Low risk. **Size** S to M. **LANE.**

#### S7. Every install pays a fat-LTO release build, and the proc-macro stack compiles twice

- **Evidence.** `[profile.release]` has `lto = "fat"` and `codegen-units = 1` (`Cargo.toml`). A release build
  after a large diff takes about 3 min (refs:80; F4a 3 min 01 s), and the chain installs several times a
  day. `Cargo.lock` holds 16 crate names in two or three versions:
  - syn 2 and **syn 3**: `clap_derive`, `serde_derive`, `thiserror-impl`, and `tokio-macros` are on syn 3,
    and others on syn 2;
  - sha2 0.10 and 0.11 (0.11 only from `rust-embed-utils`);
  - rand 0.9 and 0.10 (0.10 only from `quinn-proto`, under reqwest);
  - getrandom three times.
- **Proposal.**
  - An `install` profile (thin LTO, 16 codegen units) for the chain's installs, benched against release
    once. Keep fat LTO for tagged releases.
  - Run `cargo tree -d` and `cargo tree -i quinn` once, to see whether QUIC is even built and which
    duplicates one bump removes.
- **Payoff.** Minutes per install, many times a day. **Risk.** An install binary is a little slower than
  release. Measure with the lifecycle bench. **Size** S. **LANE.**

#### S8. After serving, two more readers grow with history

- **Evidence.**
  - `store.verify` re-reads the whole WAL at every start, at about 5% of a core (theseus-0dq).
  - The outbox's first use loads every post ever written: `outbox_actions()` reads all OUTBOX records
    (`outbox.rs:241`). A binding's first delivery after a restart waits for it.
- **Proposal.** 0dq's verified-to mark. An outbox index over open posts only, with settled counts kept as
  one meta record.
- **Payoff/Risk.** A restart's first minute stays flat as history grows. Low risk. **Size** S to M.
  **LANE** (verify), **SPINE** (outbox).

## 3. Hardening

The gate's order is sound as built. For each call, the toollet's typed `plan` comes first, then
`ToolPolicy::decide_with` (the floor, the posture, the tightenings, the allow list, and the roots), then
`brokered` (a granted secret's posture), then `external::gate`, the hold, for every class but `Read`
(`toolrun.rs:817-838`). Each later step can only tighten. F3 gates a whole response before any of its calls
run, so a call written before the model saw a page keeps its posture, correctly. What follows is what
the order does not see.

### Security, ranked

#### H1. The web UI takes a WebSocket from any origin, and a browser page can drive and approve

- **Evidence.** `ws_upgrade` checks neither `Origin` nor `Host` (`theseusd/src/web.rs:131-137`), and the
  module says "Loopback only; no auth yet" (`web.rs:4`). Browsers do not apply the same-origin policy to
  WebSockets. Any page in a browser that can reach 127.0.0.1:7433 can open `/ws` and call every method:
  `session.history` (every conversation), `turn.submit` (as the operator), `action.confirm`,
  `policy.trust` (it clears T1's hold), the spend reset, and `policy.untighten`.
  - J1's trace does not stop it. The loopback owner is the browser, which is outside every job, so the
    answer counts (`peer.rs:176-186`, `Peer::Loopback`).
  - Eddie's note has no `[approval]` section (2b1 review), so every surface answers (`approval.rs:4-6`).
  - DNS rebinding reaches it even from a page whose origin is not local.
  - Today the Hyper-V firewall blocks Eddie's Windows browser from WSL's 7433 (6qy report §7). That is luck,
    not design. WSL's mirrored networking, a browser inside WSL, or any Linux or macOS desktop (the
    open-source default) is exposed.
- **Proposal.**
  - Refuse the upgrade unless `Origin` is `http://127.0.0.1:<port>` or `http://localhost:<port>`, and check
    `Host` the same way on `/` and `/ws`.
  - Add a per-start random token that `index.html` carries and the socket must present (as a subprotocol or
    the first frame).
  - Until then, default `[web] enabled` to false. It is true in both the template
    (`theseus.example.toml:375`) and the code (`config.rs:560`).
- **Payoff.** It closes the one path from the open web to operator authority. **Risk.** Low: the app is
  served from the same origin. **Size** S. **LANE.** **Do first.**

#### H2. One web page can kill the daemon: a byte-index panic in the HTML reader, under `panic = "abort"`

- **Evidence.**
  - `raw_until` compares `s[at..end]`, where `end = at + close.len()` is a byte count, and never checks that
    it falls on a character boundary (`web/html.rs:347-358`). A `<script>` (any `DROPPED` tag,
    `html.rs:20-23`, or `<title>`/`<textarea>`) whose body holds `"</b>中文"` panics. From the `<`:
    `<` `/` `b` `>` is 4 bytes, 中 is bytes 4–6, and 文 is 7–9, so for `close = "</script"` (8 bytes)
    `end` falls inside 文. Found by reading. A unit test with that string would prove it.
  - The release profile has `panic = "abort"` (`Cargo.toml:59`), so the whole daemon aborts. tokio cannot
    isolate a task's panic under abort.
  - The fetch's `Err(e)` fallback for big pages (`web/fetch.rs:307-311`) is dead code in release, and small
    pages convert inline on the call's task (`:304`).
  - There is no panic hook, so there is no ledger row and no crash file. Eddie runs a bare `theseusd` in a
    terminal, with no supervisor.
  - The html tests have no multibyte text inside a raw element (`html.rs:509-589`).
- **Proposal.**
  1. Compare bytes: `s.as_bytes().get(at..end).is_some_and(|w| w.eq_ignore_ascii_case(close.as_bytes()))`.
  2. Fuzz everything that reads outside text or model text with arbitrary UTF-8, for a few seconds per gate
    run, as a proptest: `html::to_text`, `wake`'s duration parser, `split_text` (R5), and the SSE line
    reader (`provider.rs:665`).
  3. A panic policy, which is Eddie's call: either `panic = "unwind"` for `theseusd`, with `catch_unwind`
    around toollets and per-connection tasks, or keep abort and add a panic hook that writes
    `<state>/crash/<ts>.txt`, plus a user systemd unit with `Restart=on-failure`. With F1's 20 ms start,
    a restart is cheap.
- **Payoff.** It removes a remote, content-triggered denial of service, and makes the next panic visible.
  **Risk.** (1) and (2) are low. (3) is a policy choice. **Size** S for (1), M for (2) and (3). (1) is a
  **LANE**; (3) is **SPINE**. **Do (1) first.**

#### H3. Raw job output sits world-readable on disk, forever, and so does the whole store

- **Evidence.** From a stat of Eddie's state dir (metadata only; no contents read):
  - `~/.theseus` is 0775, `store/` and `spool/` are 0755, and `index.redb` is 0644.
  - `spool/results/*.out` are 0644: 17 files, the oldest from 09-28.

  In the code:
  - The daemon never sets a umask; there is no `umask` in the tree, and Eddie's shell has 0002.
  - `results/<id>.out` is the job's output **before** the scrubber, which runs on the node
    (`toolrun.rs:413`). The wrapper writes stdout and stderr straight to it (`job.rs:163-171`).
  - Nothing deletes it: the spool removes completions, pids, and lingering files, never results
    (`spool.rs:114-180`).
  - Its path stays on the node as `full_ref` (`toolrun.rs:1621`) and goes to clients (`rpc/info.rs:191`).

  So a brokered program that prints its token (`gh auth token`, `env`) leaves the token in a
  world-readable file indefinitely. theseus-l0d covers redaction, but not the mode or the retention.
- **Proposal.**
  - `umask(0o077)` as the first line of `theseusd`'s `main`, and `chmod 0700` on the state dir, the store,
    and the spool at start, after serving.
  - The wrapper scrubs granted values as it writes (l0d), and the core deletes `results/<id>.out` once it is
    absorbed, or keeps it at 0600 under a retention.
  - Stop sending `full_ref`'s path to clients.
- **Payoff.** Conversations, tool output, and any printed secret stop being readable by other local users
  and by later tools. This matters more at M4's control-plane separation and on a shared node. **Risk.**
  Low. **Size** S. **LANE.** **Do first.**

#### H4. `git.diff` reads through symlinks in the working tree: outside the roots, and past the floor

- **Evidence.** The working-tree mode reads `fs::read(wd.join(p))` for every tree entry, symlinks included
  (`is_blob_or_symlink`, `git.rs:55`; the read at `git.rs:234-235`). The roots and the floor check only the
  repository's path argument. `git.diff` is `Read`, so it keeps its posture even in a holding session.
  - A repository with a committed symlink, such as `notes -> ~/.ssh/id_ed25519`, puts the target's text in
    the diff, the model's context, and the WAL, and a symlink into `~/.theseus` does the same.
  - Such a repository reaches the tree by a clone the operator approved; `git clone` makes symlinks by
    default.
  - The scrubber catches only known values and seven prefixes (H9). `fs.grep` and `fs.glob` do not follow
    links (the `ignore` walker's default), so this is `git.diff` alone.
- **Proposal.** Call `symlink_metadata` first. For a symlink, compare `read_link` with the blob, which is
  what git itself does, and never read through it.
- **Payoff.** It closes the one read path around the roots and the floor. **Risk.** Low. **Size** S.
  **LANE.**

#### H5. Approval fails open, and Discord interactions still route to the DM (theseus-e89)

- **Evidence.**
  - Without `[approval]` "there is no rule, and every surface answers" (`approval.rs:4-6`), and Eddie's note
    has none. So the CLI, the web UI (H1), and every Discord user listed for a place can approve, trust,
    and reset spend.
  - `on_interaction` finds a guild interaction's place by channel, or else by the user's **DM** binding
    (`runtime.rs:1007-1012`). `allowed` is true for any user with a DM binding (`:1013-1018`). So `/stop`
    or `/cancel` (and T1b's `/trust`) typed in any guild channel act on that user's DM conversation, and a
    daemon that binds nothing answers every interaction. T1b's brief carries the fix.
- **Proposal.**
  - Land e89 with T1b.
  - Make "no `[approval]`" mean "the owner's CLI and the owner's Discord DM", not "everything". The template
    already documents the section (`theseus.example.toml:396`).
  - Health says "approval: open" loudly while the section is absent.
- **Payoff.** The trusted-channel rule protects by default. **Risk.** Changing the default is Eddie's
  decision (a question below). **Size** S. **SPINE.**

#### H6. T1's hold is only as wide as its marking, and today only the web tools mark

- **Evidence.**
  - Only `web/fetch.rs` and `web/search.rs` set `external`. Every other source of outside text enters the
    session unmarked:
    - `git.log` and `git.diff` on a cloned repository (strangers' commit messages and files);
    - `fs.read` of a cloned tree;
    - `proc.run` output (`gh issue view`, `curl`; theseus-20f);
    - Discord attachments (`attach.rs`, with no marking).
  - A job the operator approves can open a clean session through the socket (theseus-d64).
  - Reads keep their posture, so a fetch's URL can carry data out (by design, and noticed).
- **Proposal.**
  - d64's fix now: J1's trace at `session.open` and `turn.submit`, so a connection traced to a job carries
    that job's session's hold.
  - For the rest, M4's labels (integrity by transmission) are the real fix, and the M4 design says so. In
    the meantime, mark `git.*` and `fs.read` results external when the repository's remote is not one of the
    operator's (a config list), and mark attachments external.
- **Payoff/Risk.** The hold means what an operator thinks it means. It tightens only. **Size** M. **SPINE**
  (policy).

#### H7. The broker grants to a program by name, but a program can run others

- **Evidence.** B1's rule is direct argv only: `argv[0]` must canonically be the named program
  (`broker.rs`). But gh runs aliases (`gh alias set --shell x '…'`) and extensions (`gh-<name>` binaries),
  git runs `!` aliases, and cargo and npm run scripts. So `gh x` carries `GH_TOKEN` into whatever the alias
  says. On Eddie's machine this adds little, since gh's own login is readable in `~/.config/gh/hosts.yml`.
  It matters for secrets that exist nowhere else on disk, which is the broker's point, and M4 brings AWS
  credentials through it.
- **Proposal.** Grant only to leaf programs, and document which ones are launchers. For gh, pass
  `GH_CONFIG_DIR` pointing to a broker-owned, read-only config with no aliases or extensions. Treat an
  alias or extension call (argv[1] that is not a built-in command) as unbrokered. l0d's spool redaction
  belongs with this.
- **Payoff/Risk.** The broker's promise holds for M4's cloud credentials. **Size** S to M. **LANE.**

#### H8. Peer tracing is a speed bump against a same-uid job, not a boundary

- **Evidence.** J1 and Z1 refuse a job's descendants and orphans well, including pid reuse, which the peer's
  start time covers (`peer.rs:38-40`). Their known gaps are paths out of the job's process tree: a tmux
  server already running, `systemd-run --user`, cron, and a process started outside the job (6qy §7).
  Under L0 a job is also the operator's uid. It can rewrite `~/.local/bin/theseusd`, the repository, and
  the gate script, and Eddie's daemon's next manual start runs what it finds there. theseus-14s ("Theseus
  on Theseus") puts a builder daemon's jobs on exactly those files.
- **Proposal.** M4's control-plane separation (a `theseus` user owning the store, the socket, and the
  binaries) before the dogfood pilot widens. Until then, run the builder daemon as another uid or in L1,
  and have `theseusd` check its own binary's owner and mode at start (a loud health line if the operator's
  jobs can write it).
- **Payoff/Risk.** Approvals become a boundary, not an honor system. It is a direction question for Eddie.
  **Size** L (M4). **SPINE.**

#### H9. The scrubber knows exact values and seven prefixes

- **Evidence.** It covers the exact values on the secret board, plus `sk-ant-`, `ghp_`, `github_pat_`,
  `gho_`, `ops_`, `xoxb-`, and `xoxp-` (`scrub.rs:17-26`). Board values in encoded form (base64,
  percent-encoded, split across lines) pass, and so do secrets never resolved here: AWS keys
  (`AKIA…`/`ASIA…`), `-----BEGIN … PRIVATE KEY-----` blocks, and Discord bot tokens. Telemetry is clean:
  spans export ids, names, outcomes, and token counts only (`telemetry/spans.rs:80-150`). No log line
  carries content fields (a grep of `tracing::` macros for argv, body, input, text, or token found none).
- **Proposal.** Add the AWS key, private-key block, and JWT shapes, and scrub each board value's base64 and
  percent-encoded forms too (there are few values, so it is cheap).
- **Payoff/Risk.** It is needed before M4 brings AWS in. Low risk. **Size** S. **LANE.**

### Robustness, ranked

#### R1. A turn that faults after a paid loop still skips the books

- **Evidence.** After a paid provider call, `run_inner` exits with a raw `?` in several places:
  `stopped_by(&t.tc)?`, `self.tools.not_run(..)?`, `run_tools(..).await?`, a later loop's
  `compile_step(..)?`, and `ask_budget(..)?` (`turn.rs:1170-1212`). These bypass `fail()` and `close_books`,
  and `run` maps the error to `(Err(e), None)` (`turn.rs:852-855`). The session's cost, usage, and tool calls
  then miss the finished loops. The kernel's spend stays right. The failure path also drops its session
  write's result (`let _ = … update_session`, `turn.rs:2316`). This is A3b's first known gap, still open.
- **Proposal.** One exit: map every `?` after the first provider call through a `fail_raw(t, e)` that closes
  the books, then returns the error. Test it with the fault hook l6y added (`Store::fail_turn_frame`): fail
  `run_tools`' frame after a paid loop, and assert the session's cost.
- **Payoff/Risk.** The operator's dollar view stays true. Low risk. **Size** S. **SPINE.**

#### R2. A failed continuation eats its input (theseus-kol)

- **Evidence.** `has_news` treats a trailing tool result as awaiting a reply only for a task
  (`turn.rs:1482-1487`: `Some(Body::ToolResult { .. }) => t.tc.task.is_some()`). In a conversation, a job's
  result that a continuation wrote, but whose provider call failed, is `nothing_new` on retry, so it is
  never answered. The continuation also runs on the live profile, not the session's (spec A3c, the F4a
  live check).
- **Proposal.** kol's fix, queued in fix batch 1: answered-ness from the transcript (results after the last
  assistant message are unread), or an explicit unread count on the execution, and `last_target` for the
  continuation.
- **Payoff/Risk.** No job result is silently dropped. Low risk. **Size** S. **SPINE.** Already queued.

#### R3. One chatty job can take the daemon's memory, or the disk

- **Evidence.** A job's stdout and stderr go straight to `results/<id>.out`, with no cap (`job.rs:163-171`).
  `read_result_file` reads the **whole** file into memory before it keeps the last 4 MiB (`toolrun.rs:258`).
  A job that prints 20 GB takes the daemon down with it. The WAL's `max_total_bytes` defaults to none
  (`wal.rs:81`), and on a full disk every append fails, including the rows that would say so.
- **Proposal.**
  - Read the tail by `seek`.
  - Cap the output file in the wrapper (count, then truncate with a marker, or `RLIMIT_FSIZE` at about
    64 MiB), and say "truncated" in the result.
  - Health warns below a free-space threshold, and new jobs wait below a floor.
- **Payoff/Risk.** Bounded memory, bounded disk. Low risk. **Size** S. **LANE.**

#### R4. One corrupt record stops every continuation, silently

- **Evidence.** F4a refuses reads of a corrupt frame's records. `read_many` fails the whole list when one
  record is refused (`store.rs:451-459`). The driver does `let Ok(execs) = core.kernel.open_executions()
  else { continue; }` with no log (`harness.rs:113`). So one bad execution record stops every continuation
  and every wake on every tick, and says nothing. `stats` (health) and `session.list` fail too
  (theseus-15g covers list reads).
- **Proposal.** List reads skip and count refused records, and return the count so health can show it. The
  driver logs a failure once per distinct error. The crash test injects a corrupt record.
- **Payoff/Risk.** Degraded, not dead. Low risk. **Size** S. **SPINE.**

#### R5. Discord's message splitter has no progress guarantee

- **Evidence.** `split_text` sets `budget = limit − (prefix + 4)`, at least 1, where the prefix is a
  carried code-fence line (`render.rs:1227-1256`). A fence line of 1,895 bytes or more, with no space in its
  second half (minified JSON or base64 right after the fence), makes the budget 1. Then:
  - If the next character is multibyte, `floor_boundary(rest, 1)` is 0, `rest` never shrinks, and the loop
    pushes about 1.9 KB per pass until the daemon runs out of memory.
  - If it is ASCII, the splitter makes one Discord message per byte.

  It runs on every re-render of a streamed reply (`render.rs:637,678`). Found by reading; a unit test
  would prove it.
- **Proposal.** Carry only the fence's opening word (```` ```json ````), capped at about 64 bytes, and
  guarantee progress: when `cut` is 0, take one character.
- **Payoff/Risk.** It removes a model-triggered hang. Low risk. **Size** S. **LANE.**

#### R6. Queues are unbounded in front of every client

- **Evidence.** Each connection has an `mpsc::unbounded_channel` (`rpc/server.rs:84`). The bus sends to
  every watcher (`bus.rs`, `publish`), and the Discord route reads an `UnboundedReceiver<Notification>`
  (`runtime.rs:1640`). The web bridge's 256 KB duplex is bounded, but the core queues without bound in
  front of it. This is the same finding as S6.
- **Proposal.** S6's bounded, latest-wins queues, with a disconnect for a client that stays behind.
  **Size** S to M. **LANE.**

#### R7. The locks are checked at run time, not by the compiler

- **Evidence.** The session lock (`SessionHold`, `store.rs:77-120`) and K1's execution lock
  (`locks.rs:60-95`) name their holder by OS thread id, and panic on a second lock from the same thread.
  In a multi-thread tokio runtime that is sound only while no `.await` falls between taking a lock and
  releasing it. That is true today: `turn.rs:852-919` has none. But `SessionHold` is `Send` (a `Store`
  clone and a `String`), so the compiler would accept holding it across an await. The failure would be a
  "locked twice on one thread" panic in an unrelated task on the same worker, which aborts the daemon
  (H2), or a worker parked in a condvar.
- **Proposal.** Make `SessionHold`, `SessionLock`, and `ExecLock` `!Send` with a `PhantomData<*const ()>`,
  so holding one across an await in a spawned future fails to compile.
- **Payoff/Risk.** A class of latent deadlocks and panics becomes a type error. There is no run-time cost.
  **Size** S. **SPINE.**

#### R8. The store's version rule is enforced by review, not by a test

- **Evidence.** The P5b rule says a new field on a record's struct needs a bump in `kinds::SCHEMAS`
  (refs:444-451, `record.rs`), and "review a step for this". Also:
  - `mark()` rewrites the manifest outside the `appending` lock (`store.rs:476-487`), so two first appends
    of a newer kind can race. The race is benign, since both write the same content.
  - F4a's open gaps remain: 0dq (S8), 15g (R4), q49 (two schema numbers), and 02k (the clean stop pays
    redb's close: shutdown went from 23.3 to 36.9 ms p50 on the empty store).
- **Proposal.**
  - A golden-schema test: serialize a default of every record type, recursively collect its field names,
    and compare them with a checked-in snapshot keyed by kind and schema number. A changed field set with an
    unchanged number fails.
  - Take `mark()` under `appending.write()`; it happens once per kind per upgrade.
- **Payoff/Risk.** The rollback rule cannot be broken silently. Low risk. **Size** S. **LANE.**

#### R9. A FIFO in the roots takes a CPU permit, and a thread, forever

- **Evidence.** `fs.read` rejects a directory and a file over the size cap, then calls `fs::read(&path)`
  (`fs.rs:136-152`). A FIFO has length 0 and is not a directory, so the read blocks until a writer comes.
  refs:152-154 relies on exactly this to hold calls in flight. The call is abandoned at the 120 s
  in-process deadline, but its closure "holds the core until it returns, even when its caller stopped
  waiting for it" (`cpu.rs`, `CpuPool::spawn`). So each such read keeps a blocking thread and one of the
  pool's 16 permits for the daemon's lifetime. Sixteen of them, and every in-process tool waits forever.
  Also, the size check is from `metadata`, so a file that grows between the check and the read is read
  whole.
- **Proposal.** Refuse anything but a regular file (`meta.is_file()`), open with `O_NONBLOCK`, and read
  through `take(MAX_FILE_BYTES + 1)`.
- **Payoff/Risk.** The pool cannot be drained by a path. Low risk. **Size** S. **LANE.**

### Supply chain

#### SC1. The toolchain floats on `stable`

`rust-toolchain.toml` says `channel = "stable"`, and the workspace says `rust-version = "1.98"`. A new
stable's clippy lints can fail `-D warnings` on a commit that changed nothing. Two machines, or two days,
build with different compilers. **Proposal:** pin `channel = "1.x.y"`, and bump it as its own commit.
**Size** S. **LANE.**

#### SC2. The commit gate needs the network, and upstream can fail it

`cargo deny --log-level error check` (`gate.sh`) fetches the advisory database on each run unless it is
told not to, and `yanked = "deny"` (`deny.toml`). 27e1237 met exactly that: yoke-derive 0.8.3 was yanked
between two gates, and the chain had to bump `Cargo.lock` mid-step. Offline, the gate fails.
`multiple-versions = "allow"` hides the 16 duplicates (S7). **Proposal:** the commit gate runs
`check bans licenses sources`, plus advisories from a cached database (`--disable-fetch`). A daily lane job
fetches, runs the full check, and files a Beads issue. Set `multiple-versions = "warn"`, with a skip list.
**Size** S. **LANE.**

#### SC3. Two release builds of one commit differ, so a binary cannot be matched to its commit

The spec records that two builds differ in about 40 bytes: an embedded file time (13:19 against 13:41) and
the build id (A3c, F4a). The tree has no `build.rs` and embeds no build time, so the file time is
rust-embed's per-file `last_modified`, taken from `web/dist`'s mtimes (`theseusd/src/web.rs:30-32`). The
build id follows from any changed byte. **Proposal:** turn on rust-embed's `deterministic-timestamps`
feature, which the locked 8.12.0 has (line 63 of its `Cargo.toml` in the local registry), in
`theseusd/Cargo.toml:27`. Then add a nightly check that builds twice and runs `cmp`. **Payoff:** an installed binary can be verified against its commit, which matters once
Theseus builds Theseus. **Size** S. **LANE.**

#### SC4. Other notes

- The licence allow list includes MPL-2.0 (weak copyleft), OpenSSL, and CDLA-Permissive-2.0 (`deny.toml`).
  §1 says "permissive-only dependencies". One locked crate is MPL-2.0: `option-ext` 0.2.0, through
  `shellexpand` → `dirs` → `dirs-sys` (licence fields read from the local registry). The tree calls only
  `shellexpand::tilde`, at four sites (`config.rs:180,1238`, `policy.rs:254`, `theseus/src/main.rs:337`),
  which five lines over `$HOME` could replace. This is a question for Eddie, below.
- `[sources]` refuses unknown registries and git sources, which is good.
- The web UI's 70 npm packages (`web/package-lock.json`) build the JavaScript that ships inside
  `theseusd`. The committed `web/dist` and its diff check make it reviewable. There is no `npm audit` in the
  gate, which is acceptable for a devDependency toolchain; a daily lane job could run it with SC2's.

## 4. Issues I would file

Nothing was filed; this review changed no Beads state. Existing issues that already cover a finding are named
instead of duplicated. Every new one would be created with `--assignee` set and `waiting_for_user` until
Eddie picks (the Beads ownership rule).

| Finding | Issue | Pri | Size | Lane |
|---|---|---|---|---|
| H1 | web UI: refuse a WebSocket whose `Origin` or `Host` is not the loopback UI's; per-start token | P1 | S | LANE |
| H2 | html: `raw_until` byte-index panic on multibyte text; a proptest over `to_text`, the wake parser, `split_text`, and the SSE reader | P1 | S | LANE |
| H2 | the daemon's panic policy: unwind with catch, or abort with a crash file and a supervisor | P2 | M | SPINE |
| H3 | `umask(0o077)`, 0700 state, store, and spool; delete or lock down `results/*.out`; stop sending `full_ref` to clients (extends theseus-l0d) | P1 | S | LANE |
| H4 | `git.diff` reads through working-tree symlinks, past the roots and the floor | P1 | S | LANE |
| H5 | `[approval]` absent means the owner's CLI and DM only; health says "approval: open" | P2 | S | SPINE |
| H5 | theseus-e89 (in T1b) | existing | | |
| H6 | theseus-d64 and theseus-20f; add: mark `git.*` and `fs.read` from repositories with a foreign remote, and attachments | P2 | M | SPINE |
| H7 | broker: launcher programs (gh aliases and extensions, git `!` aliases) forward a granted secret | P3 | S | LANE |
| H8 | the builder daemon (theseus-14s) runs as another uid or in L1 until M4's separation; a health line when jobs can write `theseusd` | P2 | M | SPINE |
| H9 | scrubber: AWS, private-key, and JWT shapes; base64 and percent-encoded board values | P3 | S | LANE |
| C1 | a shape budget in the gate: `too_many_lines` (100), `cognitive_complexity`, `#[expect]` on today's offenders, and a file ceiling | P2 | S | LANE |
| C2 | one typed event per fact, projected to the ledger, notification, narration, trace, and OTel | P2 | L | SPINE |
| C3 | typed notification payloads, a generated `protocol.ts`, and `Tool::describe` as each call's one summary | P2 | M | LANE |
| C4 | the CLI's `run()`: one module per subcommand group | P3 | S | LANE |
| C5 | the Discord binding's `BindingPort` seam; fix the crate doc | P3 | M | LANE |
| C6 | the kernel transaction, replacing the combined transitions | P2 | L | SPINE |
| C7 | a shared daemon test rig; split `tests_m3.rs`; gate phase timings; a flaky-test list | P3 | M | LANE |
| S1 | theseus-lv2 (the kernel open set, paged lists, the driver off its tick) | existing | M | SPINE |
| S2 | `terminate_all` and result reads off the runtime's workers | P1 | S | LANE |
| S2 | a store-writer thread with true group commit; the checkpoint as a tender (next to theseus-02k) | P2 | L | SPINE |
| S3 | `[profile.dev.package."*"] opt-level = 2`; bench p95 history per commit | P1 | S | LANE |
| S4 | `bench turn`, `bench idle`, RSS, and binary size | P2 | M | LANE |
| S5 | a plain turn in 2 frames, after C6; cache the manifest digests | P3 | M | SPINE |
| S6, R6 | bounded, latest-wins client queues; serialize once | P2 | S | LANE |
| S7 | an `install` profile; `cargo tree -d` and `-i quinn` | P3 | S | LANE |
| S8 | theseus-0dq; the outbox index over open posts only | existing, plus P3 | S | LANE |
| R1 | close the books on every exit after a paid loop; test it with `fail_turn_frame` | P2 | S | SPINE |
| R2 | theseus-kol (fix batch 1) | existing | S | SPINE |
| R3 | cap job output; read the tail by seek; free-space health and a floor | P1 | S | LANE |
| R4 | list reads skip and count refused records; the driver logs its failure (with theseus-15g) | P2 | S | SPINE |
| R5 | `split_text`: cap the carried fence, and guarantee progress | P2 | S | LANE |
| R7 | `!Send` session and execution locks | P3 | S | SPINE |
| R8 | a golden-schema test for the P5b rule; `mark()` under the `appending` lock | P3 | S | LANE |
| R9 | `fs.read` refuses non-regular files and reads through a cap | P2 | S | LANE |
| SC1 | pin the toolchain | P3 | S | LANE |
| SC2 | an offline `cargo deny` in the commit gate; a daily fetch-and-check lane job | P3 | S | LANE |
| SC3 | reproducible release builds (rust-embed's `deterministic-timestamps`), and a nightly two-build `cmp` | P3 | S | LANE |

## Appendix: method and counts

- **Clock.** Started 15:27 MST (`date`). The tree was `27e1237` throughout, and `git status` was clean. No
  cargo, build, test, bench, daemon, or Beads command was run. Eddie's `~/.theseus` was stat'ed for modes
  only; no contents were read.
- **Line counts** (`scratch/loc.txt`, `scratch/loc2.txt`). Tests are `*/tests/*`, `tests.rs`, and
  `tests_*.rs`, plus each file from its test module on. `loc.txt` cuts at the first column-0
  `#[cfg(test)]`, which is the first review's method, so the comparison with 8a1e41d is like for like.
  `loc2.txt` cuts at the first `#[cfg(test)]` followed by `mod … {`, since `rpc/mod.rs` and `telemetry.rs`
  declare `mod tests;` near the top.
- **Function lengths** (`scratch/fnlen.txt`, and `fnlen_prod.txt` for production). This is a brace-matching
  awk over rustfmt'd code: a `fn` line up to the closing `}` at the same indent. Nested functions count
  inside their parent, and the simulator is excluded from the production list. Run on `turn.rs` at 54b0918,
  it reproduces the spec's 113-line maximum (`call_model`).
- **Parameter counts.** A second awk joins a signature's lines and counts commas at depth 1, excluding
  `self`.
- **Ledger kinds** (`scratch/ledger_kinds.txt`). Dotted string literals within two lines of a ledger write in
  production code. It is approximate: a few tool names may be counted among them.
- **Found by reading, not run.**
  - H2's panic and R5's loop come from the code's arithmetic, with the byte offsets worked by hand. Each
    needs its unit test to be proven.
  - H4 comes from `git.diff`'s read path.
  - R9's blocking read is what the chain's own FIFO harness relies on (refs:152-154).
  - H1 follows from the absence of any `Origin` or `Host` check in `theseusd/src` (grep), plus J1's
    documented loopback rule.
- **Read from the local cargo registry** (read only): the locked crates' licence fields (SC4), and
  rust-embed 8.12.0's `deterministic-timestamps` feature (SC3).
- **Numbers taken from reports, not measured here.** The F1 to F4a lifecycle tables (F4a's report,
  §5), F2's frames and fsync costs (F2's report), the DD8 and T1 gate bench p95s
  (DD8's report §8, T1's report), O1's binary size
  (O1's report), and O1's crate count (spec A3c, O1).
- **Finished** 16:00 MST (`date`). `main` was still `27e1237`, with a clean tree.

<!-- REPORT COMPLETE -->
