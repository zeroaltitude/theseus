# Theseus design lane `stage2`: the operator's surfaces, and the protocol push they stand on

_Checked in 2026-09-30 from the design lanes. Scrubbed for this public repository: local paths to the agents' working files, research reports, and research checkouts._

*Roadmap steps 9 to 13 · Beads theseus-zaz.1 (issues theseus-in3, theseus-7yx, theseus-l1l, theseus-n4m,
theseus-yf1, theseus-ev1) · written by Tabitha/Claude, 2026-09-30, 15:12–15:34 · docs only: nothing in the
repo was changed.*

Sources: the design lanes' brief (lane `stage2`); the v1 roadmap;
the agents' operating notes for the repo; [the spec](../the-ship-of-theseus.md) (v0.61) §1, §2, §3.2, §3.2a, §3.18, §3.20, §4.5, §6.1, Appendix F, P5b, P5c, and
Part III A3c and A4; the herdr and epidemiology research reports; and the code at
`de880fc` (read only).

## At a glance

- **The key question: what do the TUI and herdr need from the push?** The same four things. So step 9 is built
  once:
  1. `execution.changed`, sent on every change to an execution's view, to that session's watchers and to
     all-session watchers.
  2. `executions.watch`: a snapshot and the events in one request, with a WAL `position` on every row and
     event, so a client reconciles by one rule.
  3. `attention`, computed once by the server with one function in `theseus-protocol`, and sent on the wire:
     needs you, working, ready, or idle, plus a label.
  4. `session.wait`, and a backlog cap with `events.lost`, so no client polls and none falls behind without
     knowing.

  Seen state, focus, debouncing, and sound stay in each client.
- **Where it is built.** One observer on `Kernel::commit`, the only path by which execution and action
  records are written, feeding a board that is seeded lazily. Nothing goes on the start path, and the hot
  path pays one branch or one send.
- **The plan.** 16 steps (§3). The spine carries about 10 slots. The TUI (4 steps) and herdr (2 steps) are
  LANEs in worktrees once the push and the CLI's client library have landed.
- **Corrections found in the code:**
  - theseus-n4m's premise is stale: nothing writes edges, and each compilation admits only its own session's
    nodes. So reach is derived when asked, and the one route to trace today is a task's report.
  - Caching's "one header" mostly holds already (identical tools and system for a session and its tasks), so
    step 13 measures before it changes anything.
  - The web UI polls `session.list` every 5 s, and each call decodes every action ever written. The push ends
    that poll.
- **From the owner:** nothing blocks. Zig for building herdr (a heads-up); whether herdr is in his daily stack;
  how the TUI should tell him; and the 1-hour cache TTL once 13b has measured it.

## 1. Scope and principles

### 1.1 What this phase is for

- **The owner's terms.** Two of his Appendix F decisions (2026-09-27): the herdr adapter ("Let's try 'A' -- I'm
  curious to see this!") and `theseus tui` ("'A' -- first we try everything!"). Both carry herdr's one product
  bet: **never hunt for the stuck one**. They also carry NOTIFY OVER BLOCK: "the operator should /know/ when
  something bad is going to happen."
- **The spec's terms.** §3.18 says "requests change state, notifications report it". Today an execution's
  state changes are not reported, so every client polls. Appendix F adopted four things for now:
  `execution.changed`, a watch over all sessions, a daemon-owned `session.wait`, and one `attention()`
  mapping that every surface uses. P5c's proves are the exit tests (§3 below).
- **The rest of the stage** rides on the same visibility:
  - reach, the first measure of context epidemiology, in the Observatory;
  - four telemetry corrections that have no consumer yet;
  - caching that is measured before it is changed.

### 1.2 What v1 needs on the happy path, and what waits

| Step | Built for v1 (the tracer bullet) | Filed, not built |
|---|---|---|
| 9, the push | `execution.changed`; `executions.watch` (snapshot, then events); `session.wait`; `attention` on the wire; the lag rule; the web UI and the CLI as first consumers | Discord as a consumer; MCP exposure (M7); a herdr-style agent skill for driving `theseus` |
| 10, the TUI | a sidebar with task trees; the attention queue and jump-to-next; inline approve, decline, and trust; a detail pane and an input line; done-until-seen; transition-only notices | the mouse, themes, split layouts, search in history, per-state sounds |
| 11, herdr | the reporter in `theseus watch`, `watch --interactive`, and `theseus herdr sync` | the herdr plugin (a startup hook and key actions) |
| 12, reach | `node.reach` over a session's compilations and loops, plus the task-report route; the Observatory; the CLI | reach counters as a projection, population measures (M6), advisories (M4), `relies_on` (M5) |
| 13, telemetry and caching | yf1's four corrections, with provider attributes; cache figures in the Observatory; a header breakpoint; a test that prefixes are byte-identical; a TTL setting | lesson placement (M6), Bedrock and Vertex specifics |

### 1.3 The principles, as constraints on this phase

- **FAST.** Nothing new goes on the start path.
  - The push's projection (the "board", §2.7) is built on first need, after serving, off the turn path.
  - The kernel's commit path gains one enqueue.
  - Each step with startup work adds a bench row.
- **EXQUISITE VISIBILITY.** Every surface shows the same attention, computed by one function. The push also
  gets a health line, metrics, and an Observatory row.
- **QUIET BY CONSTRUCTION.** No surface polls once the push lands:
  - the web UI's 5 s `session.list` poll goes;
  - `session.wait` parks on events, not on a loop.
- **EFFICIENT.** With 10,000 parked sessions, the board holds about 150 bytes per execution, and snapshots
  are bounded.
- **Client isolation (§3.18).** The TUI and the herdr adapter link `theseus-protocol` and the CLI's client
  library, never the core.
- **One agent (§1).** herdr panes show sessions, not agents.
- **License.** herdr is Apache-2.0: its ideas are re-implemented, never copied, and Theseus talks only to its
  documented socket API.

### 1.4 What already exists (code at `de880fc`)

| Fact | Where | So |
|---|---|---|
| `session.watch` takes one session; its watchers get the turn, tool, and confirm events | `theseus-core/src/rpc/server.rs:203`, `bus.rs` | no client learns of another session's change without polling |
| No execution-state notification exists. The kernel writes `execution.*` ledger rows at about two dozen sites, but every kernel frame goes through one `Kernel::commit` | `theseus-kernel/src/kernel.rs:619` | there is one publish point: the commit |
| `session.list` and `confirm.list` find pending confirms by decoding every action ever written (`Kernel::pending_confirms` → `actions()`) | `kernel.rs:552, 1292`; `rpc/methods.rs:157` | O(history) per call |
| The web UI polls `session.list` every 5 s, and the Observatory runs 9 queries every 2.5 s | `web/src/App.tsx:205`, `Observatory.tsx:142` | the web UI pays that scan every 5 s; the push ends it |
| Each connection has one unbounded outbound queue, which the bus and the narrator feed | `rpc/server.rs:85`, `bus.rs`, `narrative.rs` | a stuck client grows the daemon's memory without bound, and no client can learn that it fell behind |
| `theseus watch` reads no stdin. The CLI's client (`Conn`) and formatter (`Printer`) live in one 2,500-line `main.rs` | `crates/theseus/src/main.rs:328, 2421` | the TUI can't reuse them until they move into a library |
| The CLI crate links only `theseus-protocol` | `crates/theseus/Cargo.toml` | TUI dependencies must not land in it |
| Every socket client is `Surface::Cli`; `[approval] channels` defaults to `["cli", "web"]`; J1 refuses a job's process | `theseus-core/src/approval.rs:28`; the config template | the TUI's and herdr's answers count as the CLI's, so no new surface is needed |
| The Discord binding's `session_info` fetches all of `session.list` to find one session | `theseus-discord/src/runtime.rs:897` | an `ids` filter on `session.list` helps it and the TUI |
| No EDGE record is written anywhere (the kind is reserved, at schema 1). `derived_from` is a field of `Compilation`, and every compilation admits only its own session's nodes | `theseus-store/src/record.rs:28`; `compiler.rs:90-106` | theseus-n4m's text is stale (§2.11); reach can be derived when asked |
| A task's report enters its parent as a new `Node::relayed`, with no link to the task's last node, though `Report.node` holds that node's id | `turn.rs:1373-1420`; `task.rs:402-411` | the one cross-session route today is untraced |
| Telemetry: histogram bounds stop at 10 s; `theseus.tool.calls` is by tool only; `gen_ai.response.model` carries the requested model | `telemetry/metrics.rs:92`; `spans.rs` | theseus-yf1's four items stand |
| Caching: one breakpoint on the whole system block, plus request-level automatic caching. A task keeps `task.create` in its tools and is refused at the call, so a session and its tasks send identical tools and system | `compiler.rs:420-436`; `task.rs:56` | much of "one header" already holds, so measure before changing |
| herdr is neither built nor installed. The reviewed clone is a local research checkout (0d1a39d, v0.9.1), and its terminal core needs Zig 0.16 or later, which is not on PATH | `crates/ghostty-vt/build.rs`; `vendor/libghostty-vt/build.zig.zon` | step 11's live check needs Zig |
| ratatui is not in the local cargo registry; crossterm 0.29 is | `~/.cargo/registry/src` | the TUI's first build fetches ratatui |

## 2. The design

### 2.1 The key question: what the TUI and herdr need from the push

Step 9 is built once, right, if it serves every consumer's needs without a second pass. The rule that makes
that possible is herdr's: **runtime facts in the server, presentation in the client.** The server owns state,
attention, and position; each client owns what it has seen, where it focuses, debouncing, and sound.

| Need | TUI | herdr | Web UI | Agents, scripts | Protocol piece |
|---|---|---|---|---|---|
| Every session's state now, without polling | ✓ | sync | ✓ | | `executions.watch`: its result is a snapshot |
| Each change as it happens | ✓ | ✓ | ✓ | ✓ | `execution.changed`, to the session's watchers and to all-session watchers |
| The same words everywhere: needs you, working, ready, idle, and a label | ✓ | ✓ | ✓ | ✓ | `attention`, on every view, from one function in `theseus-protocol` |
| The whole question, to answer inline | ✓ | `--interactive` | ✓ | | `confirm.requested` and `confirm.resolved` on the all-session watch; `confirms` in the snapshot |
| New sessions (a task, a new Discord place) | ✓ | sync | ✓ | | `execution.changed` with `previous: null`, then `session.list { ids }` for the title |
| The task tree | ✓ | | ✓ | | `parent_session_id` on every view |
| Recovery after a gap (a suspend, a reconnect, a slow terminal) | ✓ | ✓ | ✓ | ✓ | `position` on every event and every snapshot row, and `events.lost` |
| "Wait until it needs someone" | | `agent wait` | | ✓ | `session.wait` |
| Done until seen | ✓ | herdr's own | ✓ | | the client's, from transitions |
| Notices on transitions only, debounced, never for the session in focus | ✓ | herdr's own | ✓ | | the client's, from `attention` transitions |

Everything in the last column is small and additive (§2.14). Nothing in it is specific to the TUI or to herdr.

### 2.2 `attention()`: one mapping, computed by the server

```rust
// theseus-protocol: pure, no I/O
pub enum Level { NeedsYou, Working, Ready, Idle }     // "needs_you" | "working" | "ready" | "idle"
pub struct Attention { pub level: Level, pub label: String, pub since_ms: u64 }
pub fn attention(view: &ExecutionView) -> Attention   // since_ms is filled in by the board
```

- **The server computes it and puts it on the wire:** on `execution.changed`, on the snapshot, and on
  `session.list`, `execution.list`, and `task.list`. So the TypeScript web UI needs no port, and no two
  surfaces can disagree. A Rust client may call the same function on data it already holds.
- **First match wins:**

| # | When | Level | Label, for example |
|---|---|---|---|
| 1 | a budget question is pending | needs you | `budget: $10.02 of $10` |
| 2 | any other question is pending (a turn parks one at a time, `toolrun.rs:637`; the count is kept anyway) | needs you | `confirm proc.run: run cargo test` (`· floor`; `· +1 more` if ever more than one) |
| 3 | `blocked` | needs you | `blocked: <reason>` |
| 4 | `failed` | needs you | `failed: <reason>` |
| 5 | `budget_exhausted` (old executions only) | needs you | `budget exhausted` |
| 6 | `running` | working | `turn 4` |
| 7 | `queued` | working | `queued` (`· wake`, `· report`) |
| 8 | waiting on calls (a job) | working | `waiting on 2 calls` |
| 9 | waiting on another execution | working | `waiting on task a1b2c3` |
| 10 | waiting on a due time | working | `sleeping until 14:00`: a sleeping task is never ready, so its wake fires no "finished" |
| 11 | waiting on a confirm or the budget, with nothing pending (a race) | needs you | `waiting on you` |
| 12 | waiting on input (a conversation between exchanges) | ready | `ready` (`ready · wake 14:00` if a `wake.at` is pending) |
| 13 | `complete` / `cancelled` | idle | `complete` / `cancelled` |
| 14 | a state this build doesn't know (a newer daemon) | working | the state's name |

- **Order**, for queues and rollups: needs you, then done (the client's), then working, then ready, then idle.
  Within needs you, the longest waiting (`since_ms`) comes first.
- **Done until seen** is the client's rule. A session is done for a client when:
  - its level moved from working or needs you to ready or idle;
  - that happened after the client last displayed the session;
  - and it did not end `cancelled` (whoever cancelled it was there).

  A task never waits on input (it completes instead, DD7), so a finished task is idle, and done until seen.
- **Tests:** a table over every state × every wake × {no question, one, a budget question}, and one for each
  label.

### 2.3 `execution.changed`

One struct, `ExecutionView`, is both this notification's params and a snapshot row:

```json
{ "position": 48213, "at_ms": 1790000000000,
  "execution_id": "exe_…", "session_id": "ses_…", "kind": "task", "parent_session_id": "ses_…",
  "state": "waiting", "previous": "running",
  "waiting_on": {"on": "confirm", "confirm_id": "…"},
  "pending": [{"correlation_id": "…", "tool": "proc.run", "reason": "run cargo test",
               "floor": false, "budget": false, "expires_at_ms": 1790000300000}],
  "turns": 4, "spent_usd": 0.42, "limit_usd": 25.0, "ended_reason": null, "why": null,
  "attention": {"level": "needs_you", "label": "confirm proc.run: run cargo test", "since_ms": 1790000000000} }
```

- **`position`** is the WAL position of the frame's last record, so it is monotonic per execution. `waiting_on`
  is the kernel's `Wake`, typed in the protocol, with an unknown-variant fallback. `pending` is a summary;
  the whole question (with its input) comes from `confirm.requested` or `confirm.list`.
- **When it fires:** once per committed frame that changes an execution's view. The view is the state, what it
  waits on, the pending questions, the turns, the ended reason, the attention, and spend (to the cent). One
  frame gives at most one event per execution.
- **Spend-only events** are sent, for the sidebar's cost. They carry an unchanged attention, so the clients'
  notifiers ignore them: a notifier acts on a change of level.
- **Where it goes:**
  - to every connection watching that session (`session.watch`), so `theseus watch <sid>`, and the herdr
    reporter inside it, see their own session;
  - to every `executions.watch` subscriber;
  - once to a connection that is both, deduplicated as `publish_all` does today.
- **The ledger rule** (§3.18: every notification is also a ledger row) holds without a new row. Each event
  comes from a committed frame that carries its rows (`execution.*`, `confirm.requested`, `action.*`).
  `events.lost` is transport, like `narrative.line`: counted in health, not stored.

### 2.4 `executions.watch`: a snapshot, then the events

- **Request:** `executions.watch { limit?: 200 }`, which replaces any earlier watch on the same connection.
  `executions.unwatch` ends it.
- **Result:**
  - `position`: the board's position when the snapshot was taken;
  - `executions`: every view whose level is needs you or working, plus the `limit` most recently active of the
    rest;
  - `confirms`: every pending question, as `confirm.list` gives them;
  - `total`: how many executions the board holds.
- **Then these notifications:** `execution.changed`, `confirm.requested`, `confirm.resolved`, and
  `events.lost`.
- **The position rule** is the client's whole reconciliation logic:
  1. Keep, for each execution, the `position` of the last view applied.
  2. Apply an event only if its `position` is greater.
  3. The subscription and the snapshot are one request: the server registers the subscriber, then reads the
     board. So there is no gap between them, and an event older than the snapshot is simply dropped by
     rule 2.
  4. An execution that the snapshot left out (an idle one beyond `limit`) is added when its first event
     arrives.
  5. A session never seen before is looked up once with `session.list { ids: [sid] }`, a new optional
     filter.
- **Order per execution is exact anyway.** The kernel commits an execution's frames under that execution's
  lock (K1), and the observer enqueues each frame before `commit` returns. Rule 2 makes the snapshot race
  harmless, and it survives any later change to that ordering.

### 2.5 Falling behind: a backlog cap, and `events.lost`

- **The count.** Each connection counts the messages queued but not yet written: an atomic, incremented at
  the send and decremented by the writer task (`rpc/server.rs`).
- **The cap.** Above 4,096 queued messages, the server stops queuing *notifications* for that connection and
  counts what it drops. Responses always go.
- **The notice.** Once the writer has drained the queue, the server sends one `events.lost { dropped,
  streams: ["executions", "session:ses_…", "narrative"] }`.
- **The client re-snapshots** each stream it had:
  - `executions.watch` again (idempotent);
  - `session.history`, then `session.watch`, for a watched session;
  - `narrative.watch` for the narrative.
- **This covers every stream, not only the new one.** Today a suspended `theseus watch` or a frozen browser
  tab grows the daemon's memory without bound.
- **Why a cap on the connection's queue, not a bounded channel per subscription.** Responses and
  notifications share one ordered queue, so that a turn's events precede its response. It must stay one
  queue, so the cap is on it.
- _(As built 2026-10-05, theseus-celu.36; spec Part III Item 172: a queue's item is a response,
  which its writer serializes, or a notification's line, serialized once at the first queue that takes it and shared
  by every queue as one `Arc<str>`. A line counts as one message toward the cap, and `events.lost` still follows
  whichever item drains the queue.)_
- **The Discord binding** reads its in-process connection continuously. Its live progress is best-effort by
  design (§3.2), so it ignores `events.lost`, and its outbox is unaffected.

### 2.6 `session.wait`

- **Request:** `session.wait { session_id, until, after_position?, timeout_ms? }`. The timeout defaults to 10
  minutes, with a maximum of 24 hours.
- **What each `until` means:**

| `until` | Returns when |
|---|---|
| `blocked` | the session's level is needs you |
| `settled` | nothing runs or is queued for it: needs you, ready, or idle. A job, a child task, or a due time still counts as working, so a sleeping task is not settled |
| `terminal` | a terminal state. A conversation never ends, so `terminal` on one is refused at once (-32602, "a conversation never ends; wait for settled") |

- **`after_position`:** only a view after that position counts. Without it, the current view counts, and a
  wait that is already satisfied returns at once with `already: true`. (herdr's rule: never block on
  something that is already blocked.)
- **Result:** `{ reached: blocked | settled | terminal | timeout, already, execution: ExecutionView,
  confirms: [ConfirmRequest] }`.
- **The daemon owns the wait:**
  - It is a parked task on the board's change feed, so it costs nothing while parked (QUIET BY
    CONSTRUCTION).
  - It subscribes before it reads, so no wake-up is lost.
  - It ends with its connection, and each connection may hold at most 64 waits.
- **It is a read.** So it is allowed before the vault confirms the config: theseus-2fo's gate lets reads
  through.
- **The session is pinned by its id.** `/new` makes a new session, so no replacement can satisfy a wait.
  herdr has to pin a pane's occupant for this; here it comes free.

### 2.7 Where it is built

```
Kernel::commit(frame) under the execution's lock
  └─ store.append → Ok(positions)            (durable and indexed)
  └─ observer: keep EXECUTION and ACTION records → push queue       one branch, or ~µs
core push task (tokio)
  └─ decode → Board { view per execution, pending questions per execution }
  └─ attention() → view changed? → ExecutionView
  ├─ SessionBus: the session's watchers, and the all-session watchers (once per connection)
  └─ board feed (tokio::sync::watch): session.wait waiters
each connection's queue, with the backlog cap → socket · WebSocket · in-process
```

- **The observer.** `Kernel::observe(Arc<dyn Fn(Committed<'_>) + Send + Sync>)` is shared with every kernel
  view, as the locks are.
  - It runs after the append returns, so no client sees a state the WAL does not hold.
  - It lives in a shared cell (an `Arc<OnceLock<…>>`), so the views of turns already running see it when the
    first watcher installs it.
  - It only sends into an unbounded channel, so it never blocks under the lock.
  - Until something asks for the push, no observer is installed and the commit path is unchanged.
- **Why the commit, and not the two dozen `execution.*` ledger sites.** Every write of an execution record
  passes through `Kernel::commit`, whoever makes it: a turn, the driver, the reconciler, startup, a stop, a
  task's end, a wake. And only the kernel writes EXECUTION and ACTION records. A publish at each site would
  miss the next site someone adds.
- **The board** (`theseus-core/src/push.rs`, new):
  - a view per execution, the pending questions per execution, and each execution's session and parent
    session;
  - about 150 bytes per execution, so 1.5 MB at 10,000.
- **The seed.** On the first `executions.watch` or `session.wait`:
  1. install the observer, so events start to queue;
  2. read `kernel.executions()` and `kernel.pending_confirms()` on a blocking thread;
  3. apply the queued events that come after the seed.

  That first request waits for the seed alone (§2 FAST: "a request that needs one of them waits for that one
  alone"). A daemon that no one watches (the bench, most scratch runs) never pays for it. Health says `push:
  seeded in 38 ms · 212 executions · 1 question`.
- **The hot path.** A frame with no execution or action record pays one branch. Any other frame pays a filter
  and one channel send, as telemetry's hand-over does: tens of microseconds. Decoding and fan-out run on the
  push task, never under the execution's lock.

### 2.8 How the push meets FAST and EXQUISITE VISIBILITY

| Principle | How |
|---|---|
| FAST, the start path | Nothing is added: the seed is lazy and comes after serving. The lifecycle bench, which runs with no watcher, is unchanged. A new bench row times the seed on the 10,000-session store |
| FAST, the turn path | The observer only enqueues. The frame-budget test (8 frames for a plain turn) and the bench's p95 still hold |
| QUIET BY CONSTRUCTION | The web UI's 5 s poll goes, and `session.wait` parks on the feed |
| Health | `push: 3 watchers · 1 waiting · board 212 · lost 0 · seed 38 ms`, a line in `theseus health` |
| The Observatory | The Kernel panel gains the push's row. The sessions sidebar shows each session's attention, and the header shows `2 need you` |
| Metrics | `theseus.push.events` (by method), `theseus.push.lost`, and the histogram `theseus.push.delay_ms` (commit to queued) |
| The ledger | No new rows: each event comes from rows in its frame. `events.lost` is counted, not stored |
| The narrative | Nothing added: it already tells each turn's end and each question |

### 2.9 `theseus tui` (theseus-7yx)

**Shape.**
- **Its own crate and binary:** `crates/theseus-tui`, building `theseus-tui`.
  - `theseus tui` in the CLI execs it, found beside `theseus` or else on PATH, as git runs its subcommands. So
    the CLI gains no dependencies.
  - Dependencies: ratatui and crossterm (both MIT), tokio, serde_json, `theseus-protocol`, and the `theseus`
    crate's new library.
  - It builds static on musl like the others, and the install recipe gains `theseus-tui`.
- **The shared client library (step 10a).** `Conn` (connect, call, next) and the `Printer`'s formatting move
  from the CLI's `main.rs` into `crates/theseus/src/lib.rs`, as `client` and `render`.
  - `render` returns lines, each text with a style tag, instead of printing. The CLI prints them, and the TUI
    styles them.
  - Golden tests pin today's CLI output, so the move changes nothing anyone can see.

**Layout.** It works at 80×24. Below 100 columns, the detail pane gives way to a full-width list, which is
friendly to a phone over SSH.

```
 theseus · 2 need you · 1 done                                  15:42 · socket ok
 ● confirm proc.run  DM        $0.42 │ ses …a1b2c3 · DM · glm-5.3 · turn 4
   └ ◐ turn 2  task b4c5d6     $0.10 │ you: run the gate and push if green
 ◆ done  spec review           $1.20 │ ⚙ proc.run scripts/gate.sh · notify · 41 s
 ◐ sleeping until 16:00  task  $0.05 │ ⏸ confirm proc.run: git push origin main
 ○ ready  scratch              $0.00 │   why: approve_argv · expires in 4:12
                                     │   [y] approve  [t] approve + trust  [n] decline
 > _                                                                [?] help  [q] quit
```

- **Icons:** ● needs you (red), ◆ done (teal), ◐ working (yellow), ○ ready (green), · idle (dim). The colors
  follow herdr's convention, re-implemented.

**Keys.**

| Key | Does |
|---|---|
| `↑` `↓` / `j` `k`, `enter` | move, and focus a session |
| `tab` / `shift-tab` | the next / previous session that needs attention: needs you first, then done |
| `y` / `t` / `n` | approve / approve and trust / decline (with an optional note) the focused session's question |
| `i` | the input line: `enter` sends it as `turn.submit` to the focused session, and `esc` leaves |
| `s` / `c` | stop the session (`execution.stop`) / cancel the task (`task.cancel`); each asks for a second key |
| `/`, then `b` `w` `r` `d` `a` | filter by text; show only needs you, working, ready, done, or all (herdr's b/w/i/d) |
| `?` / `q` | help / quit |

**Data flow.**
- **One connection.** `executions.watch` holds the whole picture.
- **The focused session only** gets `session.history`, then `session.watch`. A change of focus unwatches the
  old session.
- **`confirm.requested`** fills the queue for every session, in focus or not.
- **`events.lost`** leads to a re-snapshot.
- **A daemon restart** closes the socket. The TUI reconnects with backoff, as the web UI's `ProtocolClient`
  does, and re-snapshots, and its header says `reconnecting`.

**Answering.**
- The TUI answers with `action.confirm { correlation_id, approve, note, trust, author: "the TUI" }` over the
  socket. So it answers as the `cli` channel, and the answer counts when `[approval] channels` lists `cli`,
  which is the default.
- A refusal (-32005), or J1's `approval.refused` when the TUI runs under a job, shows its reason on the card,
  and the question stays.
- **The card** shows the tool, the input (clipped), the reason, the floor, the external text (with `t` to
  trust), and the expiry as a countdown.
- **A budget question** shows spent, limit, needed, and lifetime, and says "approve resets its spend to $0".

**Seen state and notices.**
- **The seen file:** `$XDG_STATE_HOME/theseus/tui-seen.json`, by default
  `~/.local/state/theseus/tui-seen.json`.
  - It holds, for each execution, the position last displayed.
  - It is written on a change of focus and on exit (debounced).
  - It belongs to this client on this machine, never to the server.
- **Notices fire on transitions only:**
  - into needs you: "needs you";
  - a session out of focus going from working to ready or idle: "finished".
- **Each notice** is debounced for 1 s and re-checked against the latest view before it fires. It is
  suppressed for the focused session while the terminal has focus (crossterm's focus events).
- **Delivery:** `--notify bell` (the default), `osc9`, `osc777`, or `off`. The terminal's title carries the
  count, for example `theseus (2)`.

**Trees.** A task sits under its parent (`parent_session_id`), oldest first, and folds when idle. Depth is one
today (DD7), but the view doesn't assume it.

**Quiet.**
- The TUI redraws on events, coalesced to at most 30 frames a second.
- It runs no timer, except a 1 Hz countdown while a question with an expiry is on screen.

**Tests.**
- ratatui's `TestBackend` buffer snapshots, over a scripted fake server (a JSON-RPC stream inside the test);
- the queue's order;
- done until seen, across a restart of the TUI;
- the keys;
- the lost-and-re-snapshot path.

### 2.10 The herdr adapter (theseus-l1l)

Three small pieces in the `theseus` CLI, all at the edge. Nothing in theseusd knows herdr.

1. **The reporter, inside `theseus watch <session>`.** It is on when `HERDR_ENV=1` and both `HERDR_PANE_ID`
   and `HERDR_SOCKET_PATH` are set; `--no-herdr` turns it off.
   - **The state:** needs you → `blocked`, working → `working`, ready → `idle` (herdr derives `done`
     itself), and idle → `idle`.
   - **The report:** `pane.report_agent { pane_id, source: "custom:theseus", agent: "theseus", state,
     message: <attention label>, seq }`, sent only when the state or the message changes.
     - Each report uses a fresh connection, with retries at 0.5 s and 1.5 s.
     - `seq` is monotonic, so a report that arrives out of order is harmless.
   - **Display only:** `pane.report_metadata`:
     - the session's title, and `display_agent: "theseus: <label>"`;
     - tokens: `session` (the short id), `cost` (`$0.42`), and `confirm` (the question's id);
     - within herdr's limits of 16 keys and 80 characters.
   - **On exit** (Ctrl-C, SIGTERM, EOF, or a panic hook): `pane.release_agent`. herdr's custom authority
     never expires, so a crash would otherwise leave stale state, which `herdr sync` sweeps.
2. **`theseus watch --interactive`**, which is useful without herdr too.
   - On a `confirm.requested` for its session, it prompts `approve? [y/N/t/note]` and sends
     `action.confirm`.
   - Any other line on stdin becomes `turn.submit`, and the reporter says `working` at once, inside herdr's
     5-second stall window.
   - It works in line mode: no raw terminal.
3. **`theseus herdr sync [--pin <sid>] [--close-finished]`** is idempotent, because theseusd holds the truth.
   - It makes the workspace labelled `theseus`, only if it is missing (`workspace.create --label theseus
     --no-focus`).
   - Every session that needs you or is working, or that is pinned, gets a pane if it has none:
     `pane.split --no-focus`, then `pane run "theseus watch <sid> --interactive"`, then `agent rename <slug>`.
     herdr's names match `[a-z][a-z0-9_-]{0,31}`, so the slug comes from the session's short id and label.
   - It finds existing panes by their `$session` token, and sweeps panes whose watcher died
     (`pane.clear_agent_authority`).
   - It closes the panes of finished sessions only when asked.

- **The API used.** Only herdr's documented NDJSON socket API (its `herdr api schema --json` is the
  contract), over tokio's `UnixStream` and serde_json. No new crate.
- **Installing herdr.**
  - Build it from the reviewed clone (a local research checkout, 0d1a39d, v0.9.1), not with its curl
    installer. Its vendored terminal core needs Zig 0.16 or later.
  - Set `[update] manifest_check = false` in herdr's config, so it never fetches detection manifests from
    herdr.dev.
- **Tests.** A fake herdr socket in the CLI's tests records the NDJSON it receives. It checks the mapping,
  change-only reporting, `seq`, the release on exit, and that a second `sync` sends only reads.
- **Why it stays thin.** herdr can say only "blocked". It cannot show the question, answer it, or show budgets,
  wake reasons, or trees. The TUI is the product, and herdr is an optional window onto the same facts
  (the research's §5.5).

### 2.11 Epidemiology, step 1: reach (theseus-n4m)

**First, the issue's premises have moved.** theseus-n4m (2026-09-27) says to write reverse compilation
membership in `persist_compilation`, and a reverse column for `derived_from`, "the one transmission edge the
kernel writes today". At `de880fc`:
- no code writes an EDGE record;
- `derived_from` is a field of `Compilation` (its predecessor);
- every compilation admits only its own session's nodes.

**So here is what reach means today, exactly.** Take a node X, in session S, at position p.
- **Direct exposure** is two sets, both read from S's own records (`Store::session_compilations`, and S's
  nodes after p):
  - every compilation of S whose `includes` holds X;
  - every loop of S after p whose context held X: each assistant node there carries its `compilation_id`.

  No index is needed, because nothing admits X anywhere else yet.
- **Indirect exposure** runs through the one cross-session route that exists: a task's report, relayed into
  its parent as a new node (`read_reports`, `turn.rs:1373`). Today that copy has no link to its source.

**Build.**
1. **The first transmission edge.** In the frame where `read_reports` writes the relayed node, add an EDGE
   record:
   - keyed `derived_from|<relayed node>|<the task's last node>`, as §6.1 keys edges (`Report.node` holds the
     task's node's id);
   - scoped `in:<the task's last node>`, so `scan_scope("in:X")` lists every node derived from X, in WAL
     order.

   That scope *is* the reverse column, built on the scope index that already exists, so there is no new
   index table and no layout change. EDGE is already in `kinds::SCHEMAS` at schema 1, so the first write
   marks the manifest once, and every build since F4a still opens the store.
2. **The brief, if it fits in the step.** A task's first node (its brief) gets `derived_from` the parent's
   node that holds the `task.create` call, by the same mechanism. If it doesn't fit, it is filed.
3. **`node.reach { node_id, max_generations?: 3 }`**:
   ```json
   { "node_id": "…", "session_id": "ses_task", "position": 1234,
     "direct": { "compilations": [{"compilation_id": "cmp_…", "strategy": "transcript", "created_at_ms": 0}],
                 "loops": 17, "first_ms": 0, "last_ms": 0 },
     "descendants": [{ "node_id": "…", "session_id": "ses_parent", "generation": 1, "via": "derived_from",
                       "compilations": 2, "loops": 5, "first_ms": 0, "last_ms": 0 }],
     "totals": { "contexts": 24, "sessions": 2 }, "partial": false }
   ```
   - A context is a compilation or a loop.
   - `partial` is set when the walk reaches `max_generations` or a size cap.
4. **The Observatory's Nodes panel.** A reach cell, `seen by 24 contexts in 2 sessions`, with the first and
   last exposure. It is computed when clicked, not for every row.
5. **The CLI:** `theseus reach <node>`.
6. **A core scenario.**
   - Setup: a task's last node is relayed to its parent, and the parent's next turn compiles the relayed node.
   - Check: `node.reach` on the task's node names both sessions (generations 0 and 1) and the exact
     compilations.

**Why derive, not store.** §6.1 says the reverse `includes` edges are written in the same frame as the
compilation; this design differs, and Part III should record why.
- Every compilation's `includes` today is a run of its own session's nodes, and the Compilation record already
  holds it. Writing about 300 reverse edges per compilation would add records to every recompile's frame, for
  an answer the record already gives.
- The reader rule (P0 rule 3) puts the cost where it belongs. The first route that admits a node from outside
  its session (M6's recall, M7's borrowing) lands with its own reverse entries, and `node.reach` reads them
  then.

**FAST.**
- One more record rides in a frame that is written anyway, so no fsync is added.
- `node.reach` runs only when asked, and nothing is added to startup.

### 2.12 Telemetry follow-ups (theseus-yf1)

| # | Now | After |
|---|---|---|
| 1 | Histogram bounds stop at 10,000 ms, so every turn over 10 s lands in the last bucket | `theseus.turn.duration_ms` and `theseus.provider.call.duration_ms` get bounds up to 600,000 ms, roughly doubling from 100 ms. `first_token_ms` keeps the short bounds |
| 2 | `theseus.tool.calls` carries the turn's attributes and the tool | It adds `theseus.tool.family` (native, shell, or MCP), `theseus.tool.backend` (in-process, async, or job), and `theseus.tool.outcome`, and there is a new histogram, `theseus.tool.duration_ms`. The shell-fallback ratio (§2 NATIVE FIRST) is shell calls ÷ all calls, in health and in the Observatory's Tools panel |
| 3 | `gen_ai.response.model` is the requested model | It is the served model, which the trace already records (`served_model`) |
| 4 | A failed continuation turn counts in neither `theseus.turns` nor `theseus.provider.errors` | It counts in both, with `outcome = failed` and its class |
| 5 | `theseus.provider.call.duration_ms` has no attributes | It has the provider and the model |

- **Where:**
  - `telemetry/metrics.rs` and `spans.rs`;
  - two attributes (family, backend) on the tool span, where the turn builds it (near `turn.rs:2168`);
  - the failed continuation's hand-over.
- **Tests:** the golden OTLP JSON files in `telemetry/testdata`, updated, and a fixture of a failed continuation.
- **Live:** the receiver (the OTLP receiver script from theseus-hee's report) on a scratch daemon with
  `metrics_interval_secs = 5`. A 15-second turn must land in the right bucket.
- **Nothing reads these yet.** The owner has no OTLP endpoint, so the step is cheap to run early, or to fold into
  the next step that touches telemetry.

### 2.13 Provider-safe caching (theseus-ev1)

**What already holds: measure it, don't rebuild it.**
- Anthropic's cache prefix runs tools, then system, then messages.
- A session and its tasks send identical tools, because a task keeps `task.create` and is refused only at the
  call.
- They send identical system blocks too, because a child inherits its parent's persona, files, and model.
- So, within the TTL, they already share one cache entry for tools and system.
- Request-level automatic caching follows each session's growing prefix.

**What this step adds.**
1. **Measurement first.**
   - The ledger already has each call's `cache_read_input_tokens` and `cache_creation_input_tokens`.
   - The Observatory's Context panel gains, per session and per profile, the share of input read from cache
     and the dollars that saved.
   - OTel already has what a dashboard needs: `theseus.tokens` carries the directions `cache_read` and
     `cache_write` (`telemetry/metrics.rs:233`).
2. **A test that the header is byte-identical.** For one profile, the serialized tools and system of the
   first request must be equal:
   - across two sessions;
   - between a session and its task.

   This guards "one header" against a future change that puts a session's own byte (a date, an id) into the
   system block.
3. **Two breakpoints in the system block.**
   - Block 1: the built-in persona, the tools note, and the profile's `system`. It is stable across sessions
     and across edits.
   - Block 2: the context files, the system level's and then the persona's.

   An edit to a context file then rewrites only block 2 and what follows, and block 1's entry, with the tools
   before it, still hits. With the automatic breakpoint, that uses 3 of Anthropic's 4.
4. **`cache_min_tokens`.** When block 1 with the tools is shorter than the model's minimum in the catalog, it
   gets no breakpoint of its own (it could never cache), and the manifest says so.
5. **A TTL setting per profile**, `cache_ttl = "5m" | "1h"`, with `5m` as the default.
   - A 1-hour entry costs 2× input to write, instead of 1.25×.
   - It pays when the header would otherwise be rewritten more than about once an hour.
   - The owner's DM has a median turn of 90 s, but gaps of more than 5 minutes between exchanges are common.

   The step measures from the ledger how often a DM's first call after such a gap writes the header, and
   reports the break-even. Changing the default is the owner's call (§4).
6. **Z.ai.** Record whether GLM, through Z.ai's Anthropic-compatible endpoint, reports cache reads on a second
   identical call.
   - The catalog gets `caches = true | false` per provider.
   - A provider that doesn't cache gets no breakpoints. That changes nothing on the wire, but the manifest and
     the Observatory stop implying savings.
7. **Two limits.**
   - Never rewrite earlier history to win hits: that breaks preserved thinking signatures.
   - Keep anything that can be retracted (M6's lessons, theseus-3nk) after the breakpoints, never in the
     shared header.

- **One cost, once:** after the install, every session's first call writes the new layout once, a few cents in
  all. The step's report says so.

### 2.14 The protocol and config surface, in one place

| Kind | Name | Step | Notes |
|---|---|---|---|
| Request | `executions.watch { limit? }` → a snapshot; `executions.unwatch` | 9b | new; reads |
| Request | `session.wait { session_id, until, after_position?, timeout_ms? }` | 9c | new; a read |
| Request | `session.list { ids? }` | 9b | a new optional filter |
| Request | `node.reach { node_id, max_generations? }` | 12 | new; a read |
| Notification | `execution.changed` (an `ExecutionView`) | 9b | new; to the session's watchers and to all-session watchers |
| Notification | `events.lost { dropped, streams }` | 9c | new; transport, not a ledger row |
| Notification | `confirm.requested`, `confirm.resolved` | 9b | existing; now also to all-session watchers |
| Field | `attention` on `SessionInfo`, `ExecutionInfo`, and `TaskInfo` | 9a | new |
| Field | `waiting_on` (the typed wake) on `ExecutionInfo` | 9a | new, beside the untyped `wake` |
| Types | `Level`, `Attention`, `ExecutionView`, `PendingConfirm`, `WaitingOn`, and `attention()` | 9a | in `theseus-protocol` |
| Health | `push`: watchers, waits, the board's size, lost, and the seed's time | 9b | new |
| Metrics | `theseus.push.events`, `theseus.push.lost`, `theseus.push.delay_ms` | 9b, 9c | new |
| Metrics | the tool metrics' family, backend, and outcome; `theseus.tool.duration_ms`; the bounds; the provider attributes | 13a | yf1 |
| Config | `[profiles.<name>] cache_ttl = "5m"` | 13b | new, documented in the template |
| Catalog | `caches = true` per provider | 13b | new |
| CLI | `theseus watch --all`, `theseus wait`, `theseus executions explain <id>` | 9b, 9c | new |
| CLI | `theseus watch --interactive` and `--no-herdr`, and `theseus herdr sync` | 11 | new |
| CLI | `theseus tui` (which execs `theseus-tui`) | 10 | new |
| CLI | `theseus reach <node>` | 12 | new |
| Binary | `theseus-tui` | 10 | new, installed beside the others |

- **All of it is additive.** Every new field is optional (`#[serde(default)]`), and no method changes its
  meaning.
- **`method::ALL` grows**, so the config gate (theseus-2fo) knows that the four new requests only read.

## 3. The build plan

### 3.1 The steps

Each step is about one agent-hour and is proved live on a scratch daemon. **SPINE** steps run one at a time on
`main`. **LANE** steps are built in a worktree with the theseus-zaz recipe:
- its own branch and `CARGO_TARGET_DIR`;
- sccache as `RUSTC_WRAPPER`;
- `nice -n 19 ionice -c3` with 4 jobs, so the chain's lifecycle bench stays honest.

A lane joins `main` through a small wire-in step.

| Step | Roadmap | Issue | What | Mark | Depends on |
|---|---|---|---|---|---|
| 9a | 9 | in3 | `attention()` and the typed views in the protocol; `attention` on `session.list`, `execution.list`, and `task.list`; the web UI's and the CLI's pills | SPINE | step 8 (the reader rule) |
| 9b | 9 | in3 | the kernel's observer, the board, `execution.changed`, `executions.watch`, `session.list { ids }`, the health line, `theseus watch --all` | SPINE | 9a |
| 9c | 9 | in3 | `session.wait`, the backlog cap and `events.lost`, the web UI off its poll, `theseus wait`, `theseus executions explain` | SPINE | 9b |
| 10a | 10 | 7yx | the CLI's client library: `client` and `render` move out of `main.rs` | LANE¹ | 9c |
| 10b | 10 | 7yx | the TUI's crate, the sidebar and trees, reconnecting | LANE | 10a |
| 10c | 10 | 7yx | the detail pane and the input line; stop and cancel | LANE | 10b |
| 10d | 10 | 7yx | the attention queue, jump-to-next, and inline approve, trust, and decline | LANE | 10c |
| 10e | 10 | 7yx | done until seen, the notices, and the title count | LANE | 10d |
| 10f | 10 | 7yx | wire-in: merge, `theseus tui`, the install recipe | wire-in | 10e |
| 11a | 11 | l1l | `theseus watch --interactive`, and the herdr reporter | LANE | 9b, 10a |
| 11b | 11 | l1l | `theseus herdr sync`, and a real herdr built from the reviewed clone | LANE | 11a; Zig (§4) |
| 11c | 11 | l1l | wire-in: merge, and the agents' operating notes | wire-in | 11b |
| 12a | 12 | n4m | the report route's `derived_from` edge, `node.reach`, `theseus reach`, a core scenario, and, if it fits, the Observatory's reach cell | SPINE | step 8 |
| 13a | 13 | yf1 | the telemetry corrections (§2.12) | SPINE | none |
| 13b | 13 | ev1 | caching, part 1: measure (the Observatory's figures), the byte-identical header test, the Z.ai probe, the TTL break-even from the owner's ledger | LANE (web and tests) | none |
| 13c | 13 | ev1 | caching, part 2: two breakpoints, `cache_min_tokens`, the catalog's `caches`, `cache_ttl` | SPINE | 13b |

¹ 10a is LANE by its crate, but it restructures the CLI's `main.rs`, and every spine step adds CLI commands to
that same file. So it runs in the chain's slot right after 9c, and joins before 10b and 11a branch.

### 3.2 Tests and live checks

| Step | Tests (in the gate) | Live check (scratch daemon) |
|---|---|---|
| 9a | A table over every state × wake × {no question, one, a budget question}; each label; the unknown-state fallback; `session.list` carries `attention` for a session parked on a confirm | A GLM turn whose `proc.run` waits for approval: `theseus sessions` shows `● confirm proc.run: …` |
| 9b | **The prove:** a core scenario (a turn, a job, a confirm, a task, a wake, a stop, a cancel) with an all-session watcher. Each `execution.*` ledger row has an event at its position or later with that state, and each event has its row. Also: the snapshot race under the position rule; a question parked before the daemon started appears in the seed; no observer until someone watches; the frame budget (8) holds | Over a copy of the owner's store: `theseus watch --all > watch.log` in the background, then a turn with a confirm, the answer, and a task. `watch.log` has every transition that `ledger -n 80 --json` shows. Bench row: the seed on the 10,000-session store |
| 9c | **The proves:** `session.wait` returns on `blocked`, `settled`, and `terminal`. Also: `already`; `after_position`; `terminal` refused on a conversation; the timeout; a closed connection ends its wait; the cap of 64. **The lag prove:** a client that stops reading while 5,000 events pass gets `events.lost` once drained, and after its re-snapshot it equals a fresh client. The web build | `theseus wait <sid> --until blocked &`, then a turn that parks on a confirm: the wait returns within about 100 ms of the `confirm.requested` row. `kill -STOP` a `theseus watch --all`, run a burst of turns, then `kill -CONT`: it prints the lost notice and re-snapshots. The web UI on a scratch port (never 7433): the sidebar changes with no poll in the network log |
| 10a | Golden tests of `render` for each notification the `Printer` handles. A recorded session prints byte-identically before and after | `theseus watch` and `theseus ask` output, diffed before and after the move |
| 10b | `TestBackend` snapshots over a scripted fake server: a snapshot plus three events give the expected buffer | The TUI in tmux, read with `capture-pane`: two sessions and a task show as rows; a new turn flips a row to working and back |
| 10c | Snapshots of the detail pane from a recorded history; the input line sends `turn.submit` (the fake server records it) | In tmux: focus a session, type a prompt, and GLM's reply streams into the pane |
| 10d | The queue's order; card snapshots; the answer carries `author: "the TUI"`; a refusal renders its reason | **The TUI prove:** a `proc.run` waiting for approval; `tab` lands on it; `y` runs the continuation; a second with `n` and a note, and the model is told. From the TUI alone: every session's state, the jump, the answer, and the input |
| 10e | Done until seen survives a restart of the TUI (the seen file); a notice is debounced away when the state flips back within 1 s; no notice for the focused session | A background task finishes: its row shows ◆, and the bell or the title count appears in `capture-pane`; focusing it clears it, and a restart of the TUI keeps it cleared |
| 10f | The gate (the crate is a workspace member, so fmt, clippy, the tests, and deny cover it) | `theseus tui` runs through the exec; install |
| 11a | A fake herdr socket records the NDJSON: the mapping for each level; only changes reported; `seq` monotonic; the release on EOF, SIGTERM, and a panic; `--interactive` sends `action.confirm` and `turn.submit` | `theseus watch <sid> --interactive` in tmux against a recording socket (a small script in the scratch dir): a confirm gives `blocked` within 1 s by the recorder's clock, and `y` resumes the session |
| 11b | Sync against a scripted pane list sends the expected requests; a second run sends only reads | **The herdr prove**, in a real herdr in tmux: `theseus herdr sync` makes the panes; a confirm shows `blocked` in `herdr agent list` within 1 s (from the `confirm.requested` row to herdr's `pane.agent_status_changed` event); `y` in the pane resumes the session (CLI trusted); a second sync changes nothing |
| 11c | The gate | the agents' operating notes, and the install of herdr documented |
| 12a | **The prove:** the core scenario, a node relayed to the parent, with `node.reach` naming both sessions, the generations, and the exact compilations. Also: a store with no edges answers direct exposure only; a turn that reads a report writes no extra frame; the reader-rule registry (step 8) lists `derived_from` with its reader, `node.reach` | A GLM task with `wake_parent: true` that reports; after the parent's turn, `theseus reach <the task's last node>` names both sessions; `walread.py <store> frames <pos>` shows the edge in the report's frame |
| 13a | The golden OTLP JSON files; a failed-continuation fixture | The OTLP receiver with `metrics_interval_secs = 5`: a 15 s turn lands in the right bucket, and the tool metrics carry family, backend, and outcome |
| 13b | The header is byte-identical across two sessions, and across a session and its task, for one profile | The Z.ai probe (two identical GLM calls: are cache reads reported?). A read-only pass over a copy of the owner's ledger: how often his DM's first call after a gap of more than 5 minutes wrote the header, and the 1-hour TTL's break-even |
| 13c | Where the two breakpoints go; `cache_min_tokens`; a provider that doesn't cache gets none; the template test that un-comments every line parses `cache_ttl` | Two scratch sessions on the Sonnet profile: the second session's first call reads at least block 1 and the tools from cache. After an edit to a context file, block 1 still hits while block 2 writes |

### 3.3 Order and parallelism

```
main (spine):  8 ─ 9a ─ 9b ─ 9c ─ 10a ─ 12a ─ 13a ─ 13c ─ 10f ─ 11c ─ 13b's join ─→ Stage 3
lanes:                               ├─ 10b ─ 10c ─ 10d ─ 10e ─┘        (the TUI's worktree)
                                     ├─ 11a ─ 11b ──────────────────┘   (the herdr worktree)
                                     └─ 13b ──────────────────────────┘ (web and tests; can start any time)
```

- **Serial**, Stage 2 is about 16 chain slots.
- **With the lanes**, the spine carries 9a–9c, 10a, 12a, 13a, 13c, and three joins: about 10 slots. The TUI
  (four steps) and herdr (two) build beside it.
- **Machine capacity.** theseus-zaz plans about 3 lanes plus the chain on this 16-core machine, and the AWS
  lanes (Stage 3) want slots too. If both run at once, the herdr lane waits for the TUI's or an AWS lane's
  slot. Nothing in Stage 3 depends on Stage 2.

### 3.4 Dependencies on other phases

| This lane | Other phase | How |
|---|---|---|
| 9a–9c | Stage 1: step 7 (batch C, theseus-0g4), step 8 (the reader rule, theseus-wjy), F4b (theseus-l6y) | They come first; 9b touches `Kernel::commit`, which F4b's frame merges also touch |
| 9c | M7 step 42 (the web UI grows) | step 42 builds on the web UI's event-driven client from 9c |
| 9b, 9c | M7 step 41 (the MCP server) | it can offer `session.wait` and `execution.changed` to MCP clients |
| 10d, 11a | theseus-sgh, theseus-6qy (built) | approval as the `cli` channel, and J1's refusal of a job's process |
| 12a | M4 step 20 (integrity labels by transmission, theseus-3vu) | the same edge, and the same reverse scope, carry labels |
| 12a | M5 (theseus-vug: promotion by reference, `relies_on`) and M6 (recall, borrowing, theseus-3nk) | each new route lands with its own reverse entries, which `node.reach` then reads |
| 13c | M6 (lessons stay after the breakpoints, theseus-3nk) and M5 (a persona that Jev switches rewrites block 2 only) | the layout leaves room for both |

## 4. What it needs from the owner

Nothing here blocks step 9, and nothing blocks the chain: each item has a default the build takes.

| # | Item | Blocks? | When | Default if he doesn't say |
|---|---|---|---|---|
| 1 | **Zig 0.16 or later on this machine**, to build herdr from the reviewed clone (its terminal core is Zig). A heads-up more than a question | Only 11b's live check; 11a proves itself against a recording socket | before 11b | Install it with linuxbrew (user space), and say so in 11b's report |
| 2 | **Is herdr in his daily stack?** It decides whether `herdr sync` is worth more than a demo | No | before 11b | Build it: "I'm curious to see this!" It runs as a lane beside the TUI |
| 3 | **Where he'll run the TUI, and how it should tell him.** Windows Terminal on WSL, or a phone over SSH? Bell, OSC 9, OSC 777, or off? | No | 10e | The bell and the title's count; OSC 9 and 777 as flags |
| 4 | **The TUI answers as the CLI.** An answer from the TUI or a herdr pane counts exactly as `theseus confirm` does: the `cli` channel, over the socket at mode 0600 | No | 10d | Yes; no new trusted channel |
| 5 | **The 1-hour cache TTL** pays twice the input price on a write, to save on reads. 13b measures the break-even on his DM | No | after 13b | Stay at 5 minutes. If the numbers show a saving, propose switching the DM's profile, with the figures |
| 6 | **An OTLP endpoint**, if he wants yf1's corrections seen anywhere but health | No | any time | None; the corrections land anyway |

## 5. Open questions, each with its default

| # | Question | Default taken |
|---|---|---|
| 1 | Name the all-session stream `executions.watch`, or `session.watch { session_id: "*" }`? | `executions.watch`: its payload (views and questions, no text deltas) differs from a session watch's |
| 2 | Send `execution.changed` for spend-only changes? | Yes, at most one per frame, for the sidebar's cost; notifiers ignore them |
| 3 | Seed the board after serving, or on first need? | On first need. The web UI's reconnect triggers it within a second of any restart anyway, and the bench and most scratch daemons never pay |
| 4 | The backlog cap: a constant 4,096 messages, or a setting? | A constant (OPINIONATED); revisit if a real client ever hits it |
| 5 | `session.wait`'s timeout | 10 minutes by default, 24 hours at most |
| 6 | Does the Observatory drop its 2.5 s timer? | Not yet: its diagnostic tables keep the timer and also refresh on `execution.changed` (debounced 1 s). The sidebar's 5 s poll goes |
| 7 | Compute `attention` in the server, or in each client? | The server, with the function in `theseus-protocol` |
| 8 | Is a finished task done until seen? | Yes: it is a background completion. A cancelled one never is |
| 9 | Store reverse compilation membership (§6.1), or derive it? | Derive it, and record the divergence in Part III |
| 10 | The reverse edge: a scope, or a new index table? | The scope. Revisit if edges number in the millions (M6), where a dedicated table may scan faster |
| 11 | The brief's route: in 12a, or a follow-up? | In 12a if it fits the hour; otherwise filed |
| 12 | The TUI: its own binary, or a feature of the CLI? | Its own binary, which `theseus tui` execs |
| 13 | herdr before or after the TUI? | Both after 10a, as parallel lanes; the roadmap's order (10, then 11) holds for the joins |
| 14 | `theseus wait`'s exit code on a timeout | 4, documented in its help (0 ok, 1 error, 2 usage, and 3 cannot connect are taken, §3.18) |
| 15 | Does Discord consume the push (say, a `/queue` of what needs him, with buttons)? | Not in this stage; filed. Discord's cards already reach him through the outbox |
| 16 | Should the agent itself get `session.wait`, as a tool (a task waiting on its child)? | No: W1's `wake_parent` covers that, with no parked turn |

## 6. Risks, and what would change the plan

| # | Risk | Mitigation |
|---|---|---|
| 1 | **The seed's cost grows with history.** `Kernel::pending_confirms()` decodes every action ever written | 9b measures the seed on the 10,000-session store. Over 250 ms, an index of open actions comes first, and it would also speed up `session.list` and `confirm.list` |
| 2 | **The observer sits on the commit path**, under the execution's lock | It is one branch, or a filter and a send. If the bench's p95 or the frame budget moves, the frame is handed over by reference and filtered off the lock |
| 3 | **The backlog cap changes every client's behavior**, not only the new ones': a slow tab or `theseus watch` can now lose deltas, and must re-read | The lag test covers the CLI and the web UI, and the Discord binding's tests gain a case (it ignores the notice by design) |
| 4 | **Order per execution rests on K1's locks.** A future frame written without its lock could reorder events | The position rule keeps every client correct regardless |
| 5 | **herdr churns** (v0.9.x, fast-moving) and needs a Zig toolchain to build | The adapter uses documented methods only, pinned to the reviewed clone. If herdr changes, the adapter breaks alone |
| 6 | **The TUI's size.** The research estimated 2–3 weeks of human work (2–3k lines); here it is four agent steps and a wire-in | A step that overruns splits. v1 needs 10b–10d; 10e's notices can follow |
| 7 | **Two operator surfaces drift apart** (the web UI and the TUI) | Everything that matters (attention, questions, positions) is computed in the server; the clients only present |
| 8 | **Caching changes can cost money**: the new layout writes each session's header once after the install, and a 1-hour TTL doubles the write price | 13b measures first, and 13c's default stays 5 minutes |
| 9 | **The first EDGE record marks the manifest** | Builds since F4a know EDGE at schema 1, and builds before F4a refuse a format-3 store anyway, so no rollback is harmed |
| 10 | **Terminal quirks under WSL and Windows Terminal** (focus events, OSC notices) | The default notices, the bell and the title, work everywhere |

**What would change the plan:**
- **herdr isn't in the owner's daily stack.** Keep 11a (`--interactive` is useful on its own), and park 11b.
- **The seed is slow on a big store.** Build the index of open actions before 9b lands.
- **Z.ai doesn't cache.** 13c applies to the Anthropic profiles only, and the catalog says so.
- **The owner lives in Discord, and the web UI on his phone is enough.** Stop the TUI after 10d, and spend the slots on a Discord `/queue` (question 15), which reaches him where he already works.

*— written by Tabitha/Claude*

<!-- REPORT COMPLETE -->
