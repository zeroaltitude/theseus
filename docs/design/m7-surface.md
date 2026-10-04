# Theseus design lane `m7`: M7 Surface

_Checked in 2026-09-30 from the design lanes. Scrubbed for this public repository: Discord guild and channel ids, the operator's server name, an internal tool's name and paths, a vault item name, and local paths to the agents' working files and reports._

Beads theseus-zaz.5 (M7 epic theseus-ext, with theseus-7kg). Roadmap steps 36 to 39 and 41 to 45; step 40, the
AWS shells, belongs to [the AWS design](aws-toolset.md). Written by Tabitha/Claude, 2026-09-30, from 15:26 MST, against
`main` at 27e1237 and [spec](../the-ship-of-theseus.md) v0.61. It is docs only: nothing in the repo, the spec, or Beads was changed.

**Status:** complete (all six sections), for Tabitha's review with Eddie. §0 is the phone summary.

## 0. At a glance

M7 is where Theseus meets everything beyond the DM:
- other programs' tools (the MCP client), and other programs driving Theseus (the MCP server);
- time (recurring wakes) and more places (bindings);
- shared work (the task graph), and the operator's view (the web UI);
- its own new tools (self-extension), and speech (voice).

| Step | Sub-steps | Kind | The thin path it proves | Waits on |
|---|---|---|---|---|
| 36 MCP client | 36a crate · 36b tools wire-in · 36c prompts | LANE · SPINE · SPINE | A configured server's tool is called in a turn, through the gate. A prompt runs from `/prompt` or `theseus prompt`. | 36a: nothing. 36b: step 17 (L1), else L0 with a health warning. |
| 37 Recurring wakes | 37a a repeating wake · 37b tasks set wakes (theseus-7kg) | SPINE · SPINE | "Every day at 21:00, write me a haiku." "Check again in 10 minutes, then report." | T1b (4b) |
| 38 Bindings | 38a guilds and ceilings · 38b gliding | SPINE · SPINE | Places in two guilds, each with its own ceiling. A post into another place, and a read from one. | T1b's routing fix (theseus-e89) |
| 39 Task graph | 39a records, layers, CAS · 39b leases and the board | SPINE · SPINE | "Plan this as three tasks." A split, a close with evidence, a stale edit refused, a claim held. | 37b |
| 41 MCP server | 41a crate · 41b wire-in | LANE · SPINE | Claude Code (or our own client) opens a Theseus conversation over MCP and gets the reply. | 36a; step 9 (the protocol push) |
| 42 Web UI | 42a reads · 42b tabs | SPINE · LANE | Budgets, Ledger, and Policy tabs. | step 9; 38a |
| 43 Self-extension | 43a propose and ack · 43b load and revoke | SPINE · SPINE | The model writes a small MCP server. It is tested in L1, acked, called, and revoked. | 36b; step 17 |
| 44 Voice, part 1 | 44a engine (a spike first) · 44b wire-in | LANE · SPINE | On `/join`, the bot hears Eddie and answers with a stand-in voice. | a test voice channel; the DAVE check (§6) |
| 45 Voice, part 2 | 45a providers · 45b accounting | LANE · SPINE | Real speech to text and text to speech, each priced and settled as spend. | Eddie's providers and keys |

**The key question: LANE or core?**
- **LANE** (its own crate or directory, so it's built in a worktree, in parallel): the MCP protocol client and
  server (`theseus-mcp`), the voice engine and the speech providers (`theseus-voice`), and the web UI's new
  tabs (`web/`). The brief's guess holds for their engines.
- **Through the core** (SPINE): anything that changes what a turn sees or what the gate decides:
  - the tool contract's name type, and a tool list that changes at runtime;
  - the MCP config and servers' lifecycle;
  - recurring wakes, and tasks that set wakes (the kernel);
  - bindings' ceilings (the gate and session open), and gliding (the outbox and the gate);
  - the task graph (a new record kind, and the compiler);
  - the MCP server's surface (approval), self-extension's ack (approval), and voice's turns and spend (budget);
  - new protocol reads.
- So each LANE engine still needs an hour-long SPINE wire-in step. The MCP client's wire-in (36b) is M7's
  biggest SPINE step.
- **What can start now**, in worktrees, while Stages 1 to 5 run: 36a and 41a (one crate), and 44a's
  one-hour spike (songbird, twilight 0.17, DAVE). None of them touches the kernel.

19 sub-steps: 14 SPINE, one at a time on `main`, and 5 LANE. From Eddie, M7 needs a test voice channel, and
at the very end his speech providers and his voice for voice's live checks (§4). Nothing else blocks.

## 1. Scope and principles

### What M7 is for

- **Eddie's terms.** v1 is "everything listed". The approach is tracer bullets on the happy path, as
  autonomously as possible (2026-09-30, 08:57).
- **The spec's terms** (P9). *Prove:* Theseus carries Eddie's daily Discord work end to end for two weeks,
  with OpenClaw out of the loop for that channel. The ledger shows budgets and judgments, and the record shows
  no disclosure or authority violation. (P9 also names "hook runs"; hooks were deleted on 2026-09-28, so
  that clause is moot.)
- **The surfaces.** P9 lists them: bindings and gliding, tasks in chat, the MCP client and server, voice,
  the web UI's growth, and self-extension. The AWS shells (step 40) belong to the AWS design.

### What v1 needs from each step, and what waits

| Step | v1, the happy path | Filed, not built in v1 |
|---|---|---|
| 36 MCP client | stdio and streamable-HTTP servers named in the vault config; their tools through the gate; tool lists kept in the store; prompts as `/prompt` and `theseus prompt` | resources, elicitation, sampling (needs `sampling.v1`), OAuth, server-initiated requests, calls that survive a restart as durable jobs, servers in AWS classes |
| 37 Recurring wakes | `wake.at { every }` into a conversation; one-shot wakes in tasks | recurring wakes in tasks (with `until`), binding-declared automations under an owner grant, cron syntax |
| 38 Bindings | places in several guilds; per-place ceilings (a posture floor, a spend limit, the tools offered, the profile); `channel.post` and `channel.read`, with intersected ceilings | threads, Discord-role grants, one execution per author when people's messages coalesce, `switch`, a reload without a restart, binding edits from the web UI |
| 39 Task graph | task records with the three layers, CAS, claim leases, the board in the place | workspace locks, the mechanical-acceptance veto, move, merge, handoff, `JUDGE_STOP` over the graph |
| 41 MCP server | `/mcp` on 127.0.0.1 with the static key; conversation tools; the MCP surface never answers an approval | resources, prompts, memory tools (until M6's methods exist), sampling and elicitation toward clients, per-client tokens |
| 42 Web UI | Budgets, Ledger, and Policy (every tool's posture, layer by layer, per place) | editing the role-to-policy map (roles aren't built), the learning channel, in-thread observability |
| 43 Self-extension | propose, test in L1, ack, hot-load, revoke | `extend.promote`, `EXTEND_WARRANTED` (Jev), secrets for extensions |
| 44, 45 Voice | join when invited, hear, transcribe, answer in voice with barge-in; speech priced as spend | voice and text as two conversations, several voice channels at once, Jev judgments in voice beyond the latency class |

### The principles, as they bind this phase

- **FAST.** Nothing new goes on the start path. MCP servers, the MCP server's listener, and voice
  connections all start after serving, once the vault confirms the config.
  - The tool lists kept in the store let a start offer MCP tools without spawning anything.
  - Each step that adds startup work adds a bench row: `bench lifecycle` with three fake MCP servers
    configured, and with `[mcp_server] enabled`.
- **EXQUISITE VISIBILITY.** Each capability ships with its health block, a `theseus` command, an
  Observatory section, ledger rows, narrative lines, and telemetry. Each subsection of §2 lists them under
  **Seen in**.
- **NOTIFY OVER BLOCK.** No deny, anywhere:
  - MCP tools take postures (the `[policy.mcp]` layer is already built);
  - a ceiling is a posture floor and a list of the tools offered, never a refusal;
  - the strongest answer is still to wait.
- **One tool contract, NATIVE FIRST.** MCP tools implement the same `Tool` trait (§3.12). MCP is a small
  JSON-RPC protocol, written by hand, as the OTLP exporter and the HTML converter were.
- **TASKS** (§2): "a persisted task graph, read whole every turn, edited through structured actions the
  harness executes. No MCP, no free-text tool for tasks."
- **Channels are adapters, the kernel is not a tool** (§3.12, §3.24):
  - a glide's post goes through the outbox;
  - no tool can write bindings or policy;
  - the MCP server exposes conversations, not the kernel.
- **Derived work keeps its authority** (§3.9). A wake's turn runs as the session's principal, now and on
  every repeat.
- **Planks, never the keel** (§3.21). Self-extension loads MCP servers, and nothing else.
- **The reader rule** (P5b, F4a):
  - a new layout bumps its kind in `kinds::SCHEMAS`, with its reader and a test that reads the old layout;
  - a new record kind joins the table;
  - a new config table is installed before it is pasted into the vault note.
- **The config is the vault's.** Agents can't write it. Operator edits kept in the store may only tighten,
  as "should have asked" does.

### What exists today (read from the code at 27e1237)

| What | Where | What M7 does with it |
|---|---|---|
| `[policy.mcp]`, and posture resolution for `mcp:<server>/<tool>` (tools line, then `server/tool`, then `server`, then `enforcement`) | `theseus-core/src/policy.rs`, `config.rs`, template | used as it is |
| One `Tool` contract, four backends: `Inproc`, `Job`, `Async`, `Harness` | `theseus-tools/src/lib.rs` | MCP tools are `Async`. `name()` and `description()` return `&'static str`, which a tool found at runtime can't: one signature change (36b) |
| `ToolRuntime.registry`, fixed when the runtime is built | `theseus-core/src/toolrun.rs` | gains an MCP board beside it |
| `theseus_tools::External` on async results; T1's hold | `web/fetch.rs`, `external.rs` | MCP results are marked external by default |
| The secret broker: `[broker.programs]`, grants per tool | `broker.rs` | an MCP server's env |
| `children::spawn` (`Owned`, `Wrapper`), the subreaper, J1's peer trace | kernel `children.rs`, core `peer.rs` | MCP servers are registered `Owned`, and as the daemon's descendants they can never answer an approval |
| `wake.at`, pending wakes (at most 5), the driver's 500 ms tick, `take_wakes` | kernel `wakes.rs`, core `wake.rs` | a repeat is re-armed inside `take_wakes`' frame |
| `task.create { brief, budget_usd?, wake_parent? }`, the carve, depth one, reports, `task.list`/`task.cancel` | kernel `tasks.rs`, core `task.rs` | becomes the session of a task record |
| The bindings file: one `guild_id`, `[[channel]]` (id, name, users, mention_only), `[[dm]]`, read once when the binding starts | discord `bindings.rs`, `runtime.rs` | format 2 |
| `Authority { principal, delegated_by, ceilings }`: the ceilings map is "intersected on fork", but nothing enforces it, and the principal is always `operator` | kernel `types.rs` | 38 fills it and enforces it |
| The outbox: one lane per place, posts once and in order, live edits latest-only | `outbox.rs`, `courier.rs` | glides and the task board ride it |
| Surfaces `cli`, `web`, `discord`, `unnamed` | `approval.rs` | the MCP server adds `mcp` |
| axum 0.8 in `theseusd`, for the web UI | `theseusd/src/web.rs` | the MCP server's listener |
| jiff 0.2, already in `Cargo.lock` (through gix-date) | — | the calendar arithmetic for repeats |
| The web UI: Sessions, Transcript (with the trace), the Observatory (Context, Tools, Kernel, Startup, Discord, Approval, Wakes, External text, Executions, Actions, Ledger, Nodes, Model catalog, Sessions), and the Narrative. Lint and build only; there is no test runner | `web/src` | three new tabs, and MCP, Extensions, and Voice sections |
| Not there yet: an MCP crate (there's no `rmcp` in the local registry), songbird, or any speech key in the vault (checked by title: none) | — | built, or asked for |

**Usage (the theseus-p3k audit):**
- No MCP server is configured anywhere Eddie's agent runs.
- The recurring jobs are Slack and `gog` reports, and they stay on OpenClaw.
- The one recurring job that posts to Discord is Evening Haiku, daily at 21:00. That is the tracer bullet
  for step 37.

## 2. The design

### 2.1 The MCP client: tools, then prompts (step 36)

**Today:** the gate is ready for MCP. `[policy.mcp]` and the `mcp:<server>/<tool>` posture order are built and
tested, and the template shows them commented out. Nothing else exists: no client, no config table, no
tools.

**Three pieces.**
- **`theseus-mcp`**, a new crate (LANE). It is the protocol alone, over serde, tokio, and the workspace's
  reqwest, with no core.
  - `Client`: connect over stdio or streamable HTTP; `initialize`; list tools and prompts, all pages; call;
    get a prompt; cancel; ping; and a stream of the server's notifications.
  - `fake::Server`: a fake MCP server for tests, over stdio or HTTP. Its modes are `ok`, `slow`,
    `crash-after N`, `change-tools`, and `error`, as `fake_discord`'s are.
- **`McpBoard`** in the core (SPINE), beside `SecretBoard` and `BindingBoard`. It owns the configured servers:
  their processes, clients, states, and tool and prompt lists.
- **`McpTool`** (SPINE): one for each tool a server lists. It implements `Tool` with `Backend::Async`,
  because a call waits on another process, as `http.fetch` waits on the network.

**Config** (the vault note; the loader rejects unknown keys; the template gets a commented example):

```toml
[mcp.servers.github]                    # a server Theseus starts, over stdio
command = ["github-mcp-server", "stdio"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "github_pat" }   # [secrets] names, through the broker
read = ["get_issue", "list_pull_requests"]              # the tools the operator calls read-class
# sandbox = "l1"                        # the default once M4's L1 exists; l0 until then
# external = true                       # results give the session T1's hold (the default)
# start_timeout_secs = 30
# call_timeout_secs = 110               # under the 120 s in-process deadline

[mcp.servers.docs]                      # a remote server, over streamable HTTP
url = "<the server's endpoint>"
auth_secret = "docs_mcp_token"          # sent as a bearer token
```

**Lifecycle.**
- **After serving.** Once the vault confirms the config (`after_serving`, as telemetry does), the board
  starts every enabled server, all at once:
  - stdio: through `children::spawn(Kind::Owned)`, in its own process group (so a stop reaches the children
    `npx` starts). It gets the job's environment (`proc_env`, forbidden names filtered), plus the broker's
    grants to this server. Its stderr goes to `<state>/mcp/<name>.log`, capped. Its cwd is inside the
    workspace roots.
  - HTTP: no process, just the workspace's reqwest client.
  - Then `initialize` (the protocol revision is negotiated; v1 declares no sampling, elicitation, or roots),
    `notifications/initialized`, `tools/list`, and `prompts/list` when the server has prompts.
- **States:** `starting`, `ready`, `exited`, `restarting`, `failed`. A crash restarts after 1 s, then 5 s,
  then 30 s. A third crash within 10 minutes leaves it `failed`, with the reason, until
  `theseus mcp restart <name>`.
- **Stop.** `theseus shutdown` sends SIGTERM to each server's process group and does not wait, because
  shutdown never waits. A server whose stdin closes ends on its own.
- **`list_changed`** makes the board list again. The new list applies from the next turn's start, never
  mid-turn: a turn keeps the snapshot it started with.

**The tools.**
- **Names.**
  - The canonical name is `mcp:<server>/<tool>`, which the gate already resolves (`MCP_PREFIX`).
  - The wire name is `mcp__<server>__<tool>`: letters, digits, `_` and `-` only, cut to the provider's 64
    characters with a 6-hex digest suffix when too long, and unique across servers.
- **The trait change.** `Tool::name()` and `description()` return `&'static str`, so a tool found at runtime
  can't implement them. They become `&str`, borrowed from `&self`, a mechanical change across the callers.
  Leaking the strings instead was rejected: it would grow with every list change.
- **Order.** Offered after the built-ins, sorted by canonical name. The tool list is part of the cached prompt
  prefix, so it changes only when a server's tools do.
- **The stored list (FAST, and the cache).**
  - Each server's last good `tools/list` (names, schemas, descriptions, and a digest) is a `meta` record,
    `mcp.tools.<server>`, written after serving when it differs.
  - A start offers the stored list at once.
  - A call to a server that isn't up yet waits for that server alone, up to `start_timeout_secs`, then fails
    `mcp_unavailable` (transient).
  - So a restart costs no cache miss, and nothing waits on the start path.
- **Class and retry.** `Run` and `NonRepeatable`, unless the operator lists the tool in `read`, which makes it
  `Read` and `SafeToRepeat`.
  - A server's own hints (`readOnlyHint`, `destructiveHint`, `idempotentHint`) show on every surface, but
    they never loosen anything.
  - The server is untrusted, and `Read` would let a call past T1's hold and into F3's parallel batch.
- **External.** Results are marked external by default (`External { source: "mcp:github/get_issue" }`), so
  a session that reads one gets T1's hold. `external = false` opts a server out, for one whose output is the
  operator's own data.
- **The plan** names the server and the tool as its resource. The gate decides by name, as it always does,
  and never reads meaning into arguments.
- **Secrets.** Each call of a server that was given a secret runs at no looser a posture than that secret's
  (B1's rule). The spawn writes `secret.granted`.
- **Results.**
  - `content[]`: text as text; an image as an image block (as `fs.read` gives one, for a model with vision);
    resource links and embedded resources as text with their URIs.
  - `structuredContent` as JSON text.
  - `isError: true` as a failure the model reads.
  - Capped at `result_max_chars`, with the whole kept as a blob.
- **Cancel, stop, restart.** A cancel or a `/stop` sends `notifications/cancelled` for the request and drops
  it. A restart mid-call leaves it `outcome_unknown`, like any async call, and `NonRepeatable` means nothing
  runs it again.
- **Tool poisoning.** A tool's description is the server's text in every turn's definitions. So:
  - only servers the operator names are attached;
  - descriptions are capped at 2,000 characters;
  - a changed name, schema, or description is ledgered (`mcp.tools_changed`, with a summary of the diff) and
    posted as an operator notice (the "rug pull" case);
  - the Observatory shows every description in full.

**Prompts (36c).**
- The board keeps `prompts/list`: each prompt's arguments, and a digest of its definition.
- **Protocol.**
  - `mcp.prompt.list` reads.
  - `turn.submit { prompt: { server, name, arguments } }` acts. The core calls `prompts/get`, and the
    messages become the turn's input nodes: user-role, `origin: mcp`, author `prompt:<server>/<name>`, and
    external when the server is.
- **Discord:** `/prompt name:<server/prompt>`, with autocomplete (25 choices at most). A modal then asks
  for the prompt's arguments: up to 5 text inputs, or `args:` as `k=v` pairs when it has more. The name is
  bare, per Eddie's rule.
- **CLI:** `theseus prompt <server/prompt> [--arg k=v]… [--session <id>]`. **Web UI:** a prompt picker
  beside the composer.
- **A prompt whose definition changed** since its last use is ledgered as `mcp.prompt_changed` and noticed
  to the operator, and the use goes ahead (§3.8's "notice before reuse", notify over block).

**Seen in.**
- **Health:** `mcp[]` gives each server's transport, state, pid, tools, prompts, calls, errors, last error,
  and start time. `theseus health` gets an `mcp:` line.
- **CLI:** `theseus mcp` lists the servers and their tools, with postures and classes.
  `theseus mcp restart <name>` restarts one.
- **Observatory:** an MCP section. It shows each server, each tool's posture, class, and description, the
  stored and live digests, and a restart button.
- **Ledger:** `mcp.started`, `mcp.ready`, `mcp.exited`, `mcp.failed`, `mcp.tools_changed`, and
  `mcp.prompt_changed`. A call rides on its tool call's own rows.
- **Narrative:** "MCP server github ready in 412 ms: 23 tools, 2 prompts", and "mcp:github/get_issue —
  notify (policy.mcp github)".
- **Telemetry:** spans `mcp.call` (with `mcp.server` and `mcp.tool`), and the metrics `theseus.mcp.calls`,
  `theseus.mcp.call.duration`, and `theseus.mcp.servers.up`.

**Why hand-written.**
- MCP is JSON-RPC 2.0, which `theseus-protocol` already speaks.
- v1's subset is about a thousand lines: initialize, tools, prompts, ping, cancel, `list_changed`, and SSE
  framing for HTTP.
- The official Rust SDK would bring a large tree (macros, schema generation) for features v1 doesn't use,
  and it isn't in the local registry to vet.
- Revisit when elicitation, sampling, and OAuth arrive.

**The protocol revision.** The client offers the newest revision it implements (2025-06-18 at least: no
JSON-RPC batches, and the `MCP-Protocol-Version` header over HTTP). It accepts an older answer from a
server when that revision is on its supported list.

**Filed, not v1:**
- resources (a `resource.read` tool, and pinning into a binding's context);
- elicitation (Discord components, for servers marked `interactive`);
- sampling (M5's `sampling.v1`);
- OAuth for remote servers;
- requests from server to client (`roots` stays excluded, §3.8);
- calls that survive a restart as durable jobs;
- starting a server on its first use;
- servers in AWS classes.
### 2.2 Recurring wakes, and tasks that set wakes (step 37)

**Today** (DD8): `wake.at { at | after, note }` puts a one-shot wake on the execution's list: at most 5,
1 s to 30 days ahead. The driver's 500 ms tick fires it, and `take_wakes` writes its node in one frame. A task
may not set one. T1b makes `wake.at` exempt from the external-text hold.

**37a: a repeating wake.**
- **The tool** grows three optional fields: `wake.at { at | after, every?, days?, until?, note }`.
  - `every`: `Nm` (5 minutes at least), `Nh`, `Nd`, or `Nw`, a calendar span in the daemon's zone.
    `[kernel] min_repeat_minutes` (5) is the one new knob, so a live check can use 1.
  - `at` or `after` gives the first time. With `every` and neither, the first is one `every` from now.
  - `days` filters by weekday (`["mon", …, "fri"]`, with `every = "1d"`). `until` (RFC 3339) ends the series.
  - It stays one tool (§3.24, "one tool, one verb"): a repeat is a property of the wake.
- **The kernel.**
  - `PendingWake` gains `repeat: Option<Repeat>` and `occurrence: u32`. The execution's schema goes from 2
    to 3, with serde defaults as the reader and a test that reads schema 2 (F4a's rule). _As built (2026-10-03, Part III Item 84): one store format bump, 4 to 5, with a literal sample of the execution layout it replaces in `tests_layouts` (Tier 7.9)._
  - **Re-armed in the same frame.** `take_wakes` takes a due repeating wake, writes its node, and puts its
    next occurrence back on the list, with the same id and ~~`occurrence + 1`~~ the occurrence its time gives, `occurrence + 1 + missed` (as built; Eddie, 2026-10-03 17:14: "Built seems right, adjust the design to match").
    - The next time is the first `at + k·every` after now, in the zone (jiff's zoned arithmetic).
    - So "daily 21:00" stays 21:00 across daylight-saving changes. Phoenix has none; the code must still
      be right.
  - **Missed while down: one turn, never a burst.** One node says so: `⏰ wake (every 1d, #4; 2 missed while
    the daemon was down): write me a haiku`. Its `wake.fired` row has `missed: 2`.
  - A cancel (`/cancel id:`, `theseus cancel`, `wake.cancel`) ends the series. It counts against the 5.
  - The kernel-sim's random operations gain wakes, one-shot and repeating, closing DD8's gap.
- **The turn** is an ordinary turn: the session's authority, budget, and model. Its reply goes where the
  session posts, or to the wake's `target` if the place has moved on.
- **It stays in its session** across a `/new` in the place, so last night's haiku is in tonight's context.
- **The hold.** A repeating wake is persistence: set once under a page's influence, it runs every day. So
  `wake.at` with `every` waits for approval in a session holding external text, although T1b exempts the
  one-shot kind. This is a default Eddie may overturn (§4).
- **Cost.**
  - Each occurrence is a model call on the session's budget, and at the limit it asks, as any turn does.
  - The 5-minute floor bounds a runaway, which would be about 290 turns a day at most.
  - The Observatory shows each series' cost to date.

**37b: tasks set wakes (theseus-7kg, as the issue says).**
- A task may set one-shot wakes.
- A task's turn that would wait on input, while it has a wake pending, parks on the wake, and the task goes
  on. The wake's turn continues it. When a turn would wait with no wake left, the task ends and reports
  once, on DD7's path.
- The same cap of 5 applies. The carve still bounds the task, and the parent's spend still holds it. A
  cancel drops its wakes. `wake_parent` fires when it finally ends.
- `theseus wakes`, `/wakes`, and the Observatory name the task (`task a1b2c3`).
- A repeating wake in a task is refused in v1: "a task must end". A later option adds one with `until`.

**Seen in.**
- **Surfaces:**
  - `wake.list` gains `every`, `occurrence`, and `next`;
  - `theseus wakes` gets an `every` column;
  - `/wakes` shows `🔁 every 1d · next 21:00`;
  - the Observatory's Wakes section shows each series and its cost.
- **Ledger:** `wake.set` carries `every`. `wake.fired` carries `occurrence`, `missed`, and
  `next_due_at_ms`. `wake.ended` is written when `until` passes.
- **Narrative:** "wake wak_x fired (#4, 2 missed while down); next 21:00 Thu".
- **Telemetry:** `theseus.wakes.fired{repeat}`, and a histogram of `late_ms`.

**Filed: automations declared on a binding** (§3.9's owner grant). A schedule in the bindings file ("daily
21:00 in #haiku: <brief>") that opens a fresh task each time, under an owner grant recorded on the binding,
with no parent turn. That is OpenClaw's cron shape. The audit's recurring jobs don't need it in v1.

### 2.3 Bindings: many guilds and channels, ceilings, gliding (step 38)

**Today:**
- `bindings.toml` in the state dir has one `guild_id`, `[[channel]]` places (id, name, users,
  `mention_only`), and `[[dm]]` places. The binding reads it once when it starts.
- Each place has one conversation session, kept across restarts (`outbox.place_session`), and one lane.
- T1b's fix routes a guild interaction by its channel alone, and leaves an unbound place unanswered.

**38a: many guilds, and a ceiling per place.**
- **Format 2.** Each place names its guild. A format-1 file (a top-level `guild_id`) still loads, and its
  places take that guild.

```toml
[[channel]]
guild = "<guild id>"                    # the operator's server
id = "<channel id>"                     # #theseus-test
users = ["<user id>"]
mention_only = false
[channel.ceiling]
posture_floor = "approve"               # nothing here runs looser than this
spend_limit_usd = 5                     # the lower of this and [kernel] spend_limit_usd
tools = ["fs", "text", "git", "web", "mcp:github"]   # tool families and MCP servers offered here
profile = "glm"                         # this place's model, unless a turn names one

[[dm]]
user = "<user id>"
```

- **The ceiling becomes the session's authority.** The place's session gets it in `Authority.ceilings` when
  the place opens it. A task inherits it: the type already says "intersected on fork".
- **The gate.** The posture is the strictest of:
  - the config's posture;
  - a tightening;
  - the place's floor;
  - T1's hold.

  It is never a refusal.
- **Tools offered.** The built-ins and the board's MCP tools are filtered to the ceiling's families and
  servers. A call naming a tool that wasn't offered fails as an unknown tool, since the model never saw it.
  This is the operator's finite list, per place.
- **Spend.** The session's limit is the lower of the config's and the place's. It follows either when it
  changes, which extends 3pj's rule.
- **Profile.** The place's profile is §3.6's "session override (future)", built now for places.
- **Per guild.**
  - Slash commands are registered as guild commands in each bound guild, after serving.
  - The approval check (who can view a guild channel) reads each guild's members. It needs the Server
    Members intent, as it does today.
- **Where the file lives.** The operator's file stays in the state dir, under the floor, so a tool's write to
  it waits for approval at every posture. §3.1's "versioned config in the store, editable by slash command
  and the web UI" is filed. The file's revision (its SHA-256 prefix) is already ledgered.

**38b: gliding, on the place rule.** _Rewritten 2026-10-04 (theseus-ypy0). The first design gated a glide
by a subset rule on each place's users, until M4's confidentiality labels (step 19) took it over. The place
rule removed those labels on 2026-10-03 (Part III Item 76), so the rule they named is gone. Eddie's call
(2026-10-04, "Gliding with the place rule: yes!") is the rule below, which replaces both._
- **`channel.post { to, text }`** (§3.24's `channel` family) posts into another bound place.
  - It is an outbox post (`glide:<correlation id>`), written in the frame that settles the call, and sent
    once by that place's lane, in its order. Its message ends with a line naming the session that posted it.
  - `to` names a place by its label (`#deploys`, `DM @eddie`) or its key (`channel:<id>`, `dm:<user id>`).
    A place this daemon isn't bound to fails with words: "not a place Theseus is bound to".
  - **The destination's floor.** The call's posture is no looser than the destination's ceiling's floor
    (38a).
  - It is class `Write`, so T1's hold applies: a page could steer a post elsewhere.
  - At most 4,000 characters, two Discord messages.
- **`channel.read { from, last? }`** borrows another place's recent conversation: its last N messages (20 by
  default, 100 at most), people's and Theseus's, oldest first, as one node, the call's result, marked
  `borrowed from #x`. The node has a `derived_from` edge to each message it took (P0's rule 3), which
  `node.reach` reads. It is class `Read`.
- **The rule**, in one function both tools' gate and run take (`places::glide_rule(from, to)`). Words go from
  where they were said to where they go: a post's from its session's place to its destination, and a
  read's from its source into its session's place. The CLI and the web UI are private.

  | from \ to | private | shared |
  |---|---|---|
  | **private** | allowed | asks first, as `/publish` does |
  | **shared** | allowed; a read's text is outside text | asks first: different audiences |

  - **The same place** is allowed, since its audience is the same.
  - **Into a private place: always allowed,** at the call's own posture. What a read brings there from a
    shared place is outside text, as a fetched page is: the result is marked `external`, and the session
    takes T1's hold. A read from a shared place is marked so wherever it goes.
  - **Out of a private place: never without the owner.** A post from a private place into a shared one asks
    first, exactly like `/publish`. The answer counts only from the owner in a private place
    (`owner_in_private`), a shared place's card goes to the owner's DM, and the approved post is recorded as
    a publish is, with a `place.published` row naming who approved it and through what. A read of a private
    place's history from a session in a shared place asks first too.
  - **Between two shared places: asks first,** since they are different audiences.
  - **Private to private: allowed,** at the call's own posture.
  - Every place is offered both tools, a shared one too, since the rule asks wherever it must. A shared
    place's people can make a glide into the owner's DM; it is the owner's to read.
  - The run checks the rule again. A call that waited runs only as approved, so a place the binding rebound
    meanwhile with a stricter class is not glided past.

**Seen in.**
- **Health:** `bindings[]` gains each place's guild and ceiling. `theseus health`'s bindings line counts
  places by guild.
- **Observatory:** the Discord section groups places by guild and shows each ceiling. The Policy tab (42)
  shows each place's postures.
- **Ledger:** `discord.bound` gains `guild` and `ceiling`. New rows `glide.posted` and `glide.read`: from and
  to, the characters (and a read's messages, and whether it is outside text), and how the rule allowed it,
  `allowed`, or `approved` with the rule's words.
- **Narrative:** "session a1b2c3 posted to #deploys (1,204 chars, asked first and approved)", and "session
  a1b2c3 borrowed 20 messages from #ops (3,410 chars, allowed; outside text)".

**Filed:**
- threads (thread over channel over guild);
- Discord-role grants;
- one execution per author when people's messages coalesce (§3.9);
- `switch` (later turns post in the other place);
- `channel.react` and `channel.ask`;
- a bindings reload without a restart;
- binding edits from the web UI.
### 2.4 The task graph in chat (step 39)

**Today** (DD7): `task.create { brief }` opens a task *session*: a child execution with a carved budget, at
depth one, that reports once. There are no task *records*: no titles beyond the brief's first line, no
states beyond the execution's, no dependencies, acceptance, or claims. `task.list` reads executions.

**One record kind, two uses.** A task is a record, the new kind `TASK` (12), which joins `kinds::SCHEMAS`.
- It may have a session (DD7's execution), or none (an item of a plan).
- DD7's `task.create { brief }` stays the way to delegate, and now writes the record too.
- Old stores' task sessions, which have no record, are listed from their executions, as today, so nothing
  needs a migration.

```
Task { id: tsk_…, version, title, objective, acceptance: [text], state,
       parent?, deps: [tsk_…], owner: agent | <principal>, claim?: { by: exe_…, until_ms },
       session?: ses_…, origin: { session, principal },
       evidence: [{ node, identity }],          # identity: a commit, a job id, a snapshot id
       proposal?: { objective?, acceptance?, abandon?, by, card } }
```

**States** (§3.5's machine): `proposed`, `accepted`, `in_progress`, then `blocked`, `waiting_human`, or
`suspended`, then back to `in_progress`, and at the end `done`, `failed`, or `abandoned`.
- A task with a session follows its execution, in the same frame:
  - a turn running means `in_progress`;
  - a card waiting means `waiting_human`;
  - a budget question or a cancel means `suspended`;
  - the report closes it `done`, with the report as its evidence, and a failure closes it `failed`.
- The acceptance check (a mechanical veto on `done`) is filed.

**The tools** (§3.24's `task` family):

| Tool | What it does | Layer |
|---|---|---|
| `task.create { title, brief?, objective?, acceptance?, parent?, deps?, budget_usd?, wake_parent? }` | with `brief`, opens a session, as DD7 does; without, records a plan item | 2 (a new task's objective is its creator's) |
| `task.update { id, version, patch }` | title, deps, owner apply; objective or acceptance become a proposal | 2, or 1 |
| `task.split { id, version, into: [titles] }` | children under it | 2 |
| `task.claim { id, version }` | a lease | 2 |
| `task.close { id, version, outcome: done \| abandoned, evidence }` | appends the evidence; abandoning an accepted task is a proposal | 3, or 1 |

`move`, `merge`, and `handoff` are filed.

**The three layers** (§3.5: "so the agent cannot redefine success").
1. **Objective and acceptance: the operator's authority.**
   - The agent proposes a change. The proposal is a card to the task's requester (its origin's principal),
     or else the owner, through `[approval]`'s trusted channels.
   - It is judged by `judge_act`, so a job's process can't accept it.
   - Accept applies it in one frame (`task.change_accepted`). Decline leaves the task as it was
     (`task.change_declined`).
   - "Abandoning is not completing."
2. **The plan: the agent's.** Titles, children, dependencies, and claims are edited freely, under CAS.
3. **Evidence and outcomes: append-only.** Each entry is a node and its identity. Nothing removes one.

**CAS.**
- Every edit names the `version` it read. A stale edit is refused with the record as it is now ("task
  tsk_x changed since you read it: v7 → v8, title …"), and the model reads it again.
- It is enforced under a per-task lock in the core, as `Store::with_session` does for sessions. The lock
  order is session, then task, then execution.

**Claim leases.**
- `task.claim` sets `claim { by: exe_…, until: now + 30 min }`. Each turn of the holder that touches the task
  renews the lease.
- An expired lease frees the task, found by the due scan, as wakes are.
- A second claimer gets `blocked: claimed by session a1b2c3 until 14:05`, never a silent retry (§3.2a).

**What the model sees.**
- Each turn, the compiler admits the task graph for the execution's scope:
  - each open task on one line: id, title, state, owner, deps, one line of acceptance, and version;
  - each closed subtree on one line, with a count.
- It is bounded to about 1,500 tokens. Past that, it shows open tasks only, with a count of what it left out.
- It goes in the tail, after the cached prefix, because it changes often.
- It is admitted only when the scope has tasks, so a plain turn is unchanged, and the frame-budget test and
  the token counts hold.
- **The scope:** a conversation sees the tasks it started and their children. A task sees its own subtree and
  its parent's line.

**In chat.**
- `/tasks` shows the tree: states, owners, and claims.
- **The board:** one message per place, edited in place by the lane as a live edit (only the latest state,
  never replayed). It is made at the place's first `task.changed`. It is pinned when the bot may pin, and the
  pin is best-effort.
- **The layer-1 card:** "Change the objective of tsk_x? Before: … After: …", with Accept and Decline.
- Buttons on the board are filed.

**Protocol:** `task.list` returns records, and `task.get` reads one. A new notification, `task.changed`. The
web UI's task tree reads the records.

**Seen in.**
- **CLI:** `theseus tasks` shows the graph (tree, states, versions, claims).
- **Observatory:** a Tasks section with the graph, open proposals, and leases.
- **Ledger:** `task.created`, `task.updated`, `task.split`, `task.claimed`, `task.lease_expired`,
  `task.change_proposed`, `task.change_accepted`, `task.change_declined`, `task.closed`, and
  `task.stale_refused`.
- **Narrative:** "task tsk_x split into 3 (v4 → v5)".
- **Telemetry:** a counter per verb, and a `theseus.tasks.open` gauge.

**Filed:**
- workspace locks;
- the mechanical veto on `done` (a failing check vetoes it);
- `move`, `merge`, and `handoff`;
- `JUDGE_STOP` over the graph (M5);
- buttons on the board;
- task sessions nested more than one level deep (depth one stays).

### 2.5 The MCP server on localhost (step 41)

**Today:** nothing. The model for it is the web UI's loopback listener (axum, `Surface::Web`, the peer
trace), and the Discord binding's in-process protocol client.

**Config:**

```toml
[mcp_server]
enabled = false
port = 7434                             # 127.0.0.1 only (the reachability rule)
key_secret = "mcp_server_key"           # a [secrets] name: the one static key
# posture_floor = "approve"             # where an MCP-opened session's acting calls wait
# spend_limit_usd = 5                   # each MCP-opened session's limit
# requests_per_minute = 60
```

**Transport:** streamable HTTP at `/mcp`, on axum, beside the web UI in `theseusd`, bound after serving.
- A POST carries one JSON-RPC message, and the answer is `application/json`. v1 streams nothing: a long
  turn returns a status instead (below).
- A GET answers 405, since v1 has no server stream. `Mcp-Session-Id` is set at `initialize`, and a DELETE
  ends the session.

**Security.**
- It binds to 127.0.0.1, as the web UI does.
- `Authorization: Bearer <key>` is compared in constant time.
- An `Origin` header that isn't loopback is refused (DNS rebinding, as MCP's transport rules ask).
- A wrong key gets 401, and at most one `mcp_server.refused` row a minute.
- `requests_per_minute` bounds a client.

**Architecture: LANE, with a small wire-in.**
- The listener and the mapping from MCP to the protocol live in `theseus-mcp`'s `server` module.
- It talks to the core only through the protocol, as an in-process client (like the Discord binding's
  `rpc_client.rs`), behind a small `CoreClient` trait. A fake core stands in for tests.

**A surface of its own: `mcp`.**
- A new `Surface::Mcp` is never an approval surface. An answer, a trust, a press, or an undo from it is
  refused (`approval.refused`, `surface: mcp`).
- Its peer is traced as the web UI's is: the loopback owner, through `/proc/net/tcp`. So a session opened
  through MCP from a Theseus job's process takes that job session's hold (theseus-d64's rule).

**The principal is `mcp`:** one key, so one shared principal (§1, "Deferred").
- Each session it opens is labelled `mcp <clientInfo.name>` and takes `[mcp_server]`'s ceiling.
- The posture floor is `approve` by default, so every acting call in a client's turn waits for the operator.
- Its cards go to the operator: the operator's Discord lane, the web UI, and the CLI.

**Tools exposed.** Names use `_`, for clients whose tool names can't carry dots.

| Tool | Does |
|---|---|
| `conversation_open { label? }` | opens a session, and returns its id |
| `conversation_send { session_id, text, wait_secs? }` | submits a turn and waits for its end (step 9's `session.wait`), 60 s by default; returns `{ reply }`, or `{ status: "running", turn_id }` |
| `conversation_status { session_id }` | state, what it waits on, the last reply, and its cost |
| `task_list { session_id? }`, `wake_list { session_id? }` | reads |

- M6's `memory_search` and `memory_get` join when its methods exist.
- Filed:
  - `memory_propose` (through Jev's ingest);
  - re-exporting the built-in tools (`fs.*`, `proc.run`);
  - resources (task graphs, transcripts);
  - persona prompts;
  - sampling and elicitation toward clients;
  - per-client tokens.
- Each inbound call is a ledger row (`mcp_server.call`: the tool, the session, the client's name, the
  latency). Each turn is an ordinary turn in its session.

**Seen in.**
- **Health:** `mcp_server`: listening, the port, clients, sessions, calls, and refusals. `theseus health`
  gets a line.
- **Observatory:** the MCP section gains a server panel.
- **Narrative:** "MCP client claude-code opened session a1b2c3".
- **Telemetry:** spans `mcp_server.call`.
### 2.6 The web UI grows (step 42)

**Today:** Sessions, the Transcript (with the trace), the Observatory's fourteen sections, and the
Narrative. It is a protocol client over a loopback WebSocket, so everything it shows comes from protocol
methods.

**42a: two reads (SPINE, small).**
- **`budget.list`** returns each open execution's money: its limit, spent, reserved, held unknown, and
  lifetime cost; its carves (parent to tasks); the last reset (when, and by whom); and the totals.
  - It reads records, as `execution.list` does, with no scan of history (FAST, §9).
  - The CLI gets `theseus budgets`.
- **`policy.explain { session_id? }`** gives each offered tool's posture, layer by layer, and the result, for
  a session or for every place. That is "why did this wait?" on one screen. The layers:
  - `enforcement`, the `[policy.tools]` line, the `[policy.mcp]` line;
  - a tightening, the place's floor, a granted secret's posture, and T1's hold;
  - the allow and approve lists.

  The CLI gets `theseus policy explain [--session <id>]`.

**42b: three tabs (LANE, in `web/`).**
- **Budgets:**
  - a table of sessions and tasks: limit, spent, reserved, held, lifetime, and burn per hour, from the
    ledger tail's `provider.call` rows;
  - the carve tree, and recent resets;
  - the budget questions waiting, with their buttons.
- **Ledger:** full width.
  - Facets by kind prefix (with counts), by session, and by time.
  - Saved filters, kept in the browser.
  - A row's JSON, a follow mode, and an export of the rows shown, as JSON.
- **Policy:** every tool's posture, layer by layer, per place with its ceiling (from `policy.explain`).
  - The tightenings, with their undo.
  - The approval channels and trusted users, moved here from the Observatory.
- Earlier steps add their own sections: MCP (36b), Tasks (39), Extensions (43), and Voice (44).
- **Live, with no polling:** the tabs stand on step 9's push (`execution.changed`, the all-sessions watch).

**What the UI may change.** §3.14 wants "binding and policy editing with audit trail". In v1 the UI edits
only what already has a judged act: tightenings and their undo, trust, resets, and cancels.
- **Next (filed P2):** tighten-only edits of place ceilings, kept in the store (`binding.tighten`), as
  tightenings are.
- The Discord-role-to-policy table waits for roles, which v1 doesn't have: every principal is the operator.

**Why LANE.** `web/` builds in a worktree. Two branches' `web/dist` won't merge, so the join rebuilds it,
which the gate's dist check needs anyway.

### 2.7 Self-extension (step 43)

**Today:** nothing. §3.24 lists `extend.propose` and `extend.promote`, and neither is built.

§3.21: "planks, never the keel". The agent may add tools, as MCP servers in L1, under the operator's ack.

**43a: propose, test, ack.** `extend.propose { name, dir, command, description, tests? }` is a harness tool.
It is class `Run`, posture `notify` by default, and waits under T1's hold like any `Run` call.
1. **Freeze.** The harness copies `dir` (inside the workspace roots) into
   `<state>/extensions/<name>/<digest>/`, addressed by the tree's SHA-256. What the operator acks is frozen:
   a later edit in the workspace changes nothing that runs.
2. **Start it in L1**, with no network unless the proposal asks for it, through the MCP board. It starts in
   a `proposed` state, whose tools no turn is offered.
3. **Test it.** `initialize`, `tools/list`, then each declared test (`{ tool, arguments, expect: contains |
   equals }`) as a `tools/call`.
4. **The manifest**, a node: `kind: extension`, `trust: agent`, `derived_from` the proposing call.
   - It holds the name, the digest, the command, and the tools (names, schemas, descriptions);
   - the tests and their results;
   - the capabilities asked for (network, a scratch dir);
   - the proposing session and its principal.
5. **The ack:** an approval card in a trusted channel (Discord, the web UI, the CLI), judged by
   `judge_act(Act::Extend)`, so a job's process is refused.
   - "Load wordcount 3f2a1c: 1 tool, 3 of 3 tests passed, no network?", with Load and Decline.
   - The web UI shows the frozen files, with a diff against the version it would replace.
   - Jev may advise (M5), and never approves (§3.21).

**43b: load, restart, revoke.**
- **On ack:**
  - an `extensions` meta record (name to digest, command, acked by, when, and capabilities), with the rows
    `extend.acked` and `extend.loaded`;
  - the board moves the server to `ready`, and its tools, `mcp:ext-<name>/<tool>`, are offered from the next
    turn's start;
  - their posture is `notify` by default (`[policy.mcp] "ext-<name>"` overrides), class `Run`, and external
    unless the network is off.
- **Never wider.** Its ceiling is the proposing execution's at the ack. It can't ask for more authority than
  that (§3.9: never widens).
- **Restart:** acked extensions start after serving, from their frozen copies, as configured servers do.
- **Revoke:** `extension.revoke { name }` acts and is judged. It is reached by `theseus extend revoke
  <name>`, a button on `/extensions`, or the web UI. It stops the server, drops its tools from the next turn,
  and writes `extend.revoked`.
- **A new version** of the same name is a new proposal and a new ack. The old version runs until the new one
  loads.
- **The keel.** Nothing here compiles or restarts Theseus. `extend.promote` (a pull request) is filed.

**No L1, no step 43.** An agent-written server never runs at L0: it needs step 17.

**Seen in.**
- **Observatory:** an Extensions section with the manifest, digest, files, tests, who acked it, calls,
  errors, and a revoke button.
- **CLI and Discord:** `theseus extend list|revoke`, and `/extensions`.
- **Ledger:** `extend.proposed`, `extend.tested`, `extend.acked`, `extend.declined`, `extend.loaded`, and
  `extend.revoked`.
- **Narrative, and telemetry** as for MCP servers.

### 2.8 Voice (steps 44 and 45)

**Today:** nothing. The binding is text only (twilight's gateway and REST, with no voice intents). There is
no songbird in the tree, and an existing voice agent (Python, LiveKit) holds the working knowledge: its barge-in, and its
Deepgram setup.

**The shape:** `theseus-voice`, a new crate behind a cargo feature, `voice`, one of §3.12's compiled-in
feature crates (LANE). It holds:
- songbird's driver, to receive and send;
- a pipeline: an utterance per speaker (VAD), a playback queue with barge-in, short acknowledgments, and
  proactive speech at the next pause;
- the `Speech` trait: `transcribe` (audio in, text and usage out) and `synthesize` (text in, audio and usage
  out), with its stand-ins;
- a seam, `VoiceIo` (frames in per speaker, audio out). songbird implements it for real, and a WAV-file
  stand-in implements it for tests.

**The wire-in (44b, SPINE)** in `theseus-discord`:
- The gateway adds the `GUILD_VOICE_STATES` intent (today it asks for guilds, guild messages, DMs, and
  message content only). Its voice-state and voice-server updates go to songbird: through its twilight
  integration, or its generic shard trait if that lags twilight 0.17 (§6).
- A voice place in the bindings file: `[[voice]] guild, id, text = "<the paired text channel>", users`.
- `/join` and `/leave` in the paired text channel (bare names). Theseus joins only when a listed user
  invites it, never on its own (§1).

**One session.** A voice place shares its paired text place's session.
- Each utterance is a user message by its speaker (`🎙️`).
- The reply goes out as speech, and as text in the text channel.
- §3.2 makes them two conversations that borrow freely. One session is the thin path, and the split is
  filed.

**Receive.**
- songbird gives decoded audio per SSRC, and speaking events map an SSRC to a user.
- Only listed users' audio is used. Anyone else's is dropped, never transcribed.
- An utterance ends after 700 ms of silence (the stand-in VAD; a provider's endpointing replaces it).
- Utterances that arrive during a turn coalesce into the next one, each keeping its author (§3.2).

**Send.**
- The reply is split at sentence ends and synthesized one sentence at a time, so the first audio starts after
  the first sentence.
- **Barge-in** (from that voice agent): a listed user who speaks for 300 ms while Theseus talks stops the track and
  drops the rest. The text channel still gets the whole reply.
- A turn that takes over 2 s gets a short acknowledgment: a canned clip, with no speech-synthesis cost.
- A task's report waits for the next pause.

**Stand-ins (44).** Part 1 is fully testable with no keys:
- speech to text returns a fixed transcript per test fixture, or `[utterance 3.2 s]` live;
- text to speech plays a pre-made clip for the canned lines, and a tone for each other sentence.

**Providers (45a, LANE).** Eddie chooses.
- The recommended default is Deepgram for both: nova-3 streaming for speech to text, and Aura-2 for text to
  speech, on one key. It's what that voice agent runs today.
- §3.11's Cartesia is the alternative for text to speech.
- Streams over the workspace's rustls (ring). Tests run against fakes, as `fake_discord` does.

**Spend (45b, SPINE).** Speech is priced as models are.
- **Catalog rows:** `[catalog."deepgram:nova-3"] usd_per_minute`, and `[catalog."deepgram:aura-2"]
  usd_per_1k_chars`.
- **Reserve and settle.** An utterance reserves when it closes (its length is known then), and settles at the
  provider's usage. A synthesis reserves by its text's length before the call.
- **Both land on the session's budget**, in micro-dollars, with `speech.stt` and `speech.tts` ledger rows
  (provider, model, seconds or characters, cost, latency).
- **A dropped stream's usage is held as unknown** until reconciled, as a provider call's is.
- **A voice turn is a Jev workload class** in M5's latency table (§3.7).

**FAST.** No voice work happens before serving. songbird's threads start at the first `/join`.

**Seen in.**
- **Health:** `voice[]`: the channel, whom it hears, the SSRC map, the providers' states, and p50 latencies.
- **Observatory:** a Voice section with live meters: frames in and out per speaker, utterances, speech-to-text
  latency, time to first audio, and barge-ins.
- **CLI:** `theseus voice`.
- **Narrative:** "🎙️ eddie spoke 3.2 s; transcript 41 chars in 280 ms", and "🔊 reply of 2 sentences, first
  audio in 410 ms; barge-in at 1.1 s".
- **Ledger:** `voice.joined`, `voice.left`, `voice.utterance`, `speech.stt`, `speech.tts`, and
  `voice.barge_in`.
- **Telemetry:** histograms `theseus.voice.stt.latency`, `theseus.voice.tts.first_audio`, and
  `theseus.voice.turn.duration`.

**A spike first.** 44a starts with a one-hour spike. It answers three questions: does songbird's current
release build static with musl, libopus, and `cargo deny`? Does it work with twilight 0.17? Does it speak
DAVE? The answers settle 44's shape before any code lands (§6).

## 3. The build plan

Each sub-step is about an hour of one agent. Each passes the whole gate (fmt, clippy, the tests, deny, the web
build, and the bench's §9 budgets), is proved live on a scratch daemon over a copy of Eddie's store (the LANE
steps: on the crate's own example), and files what it left out as Beads issues. **SPINE** runs one at a time on
`main`. **LANE** runs in a git worktree, with its own `CARGO_TARGET_DIR` (per the agents' operating notes for this repo: a shared `target/` poisons
it), and joins `main` through its SPINE wire-in.

### The steps

| Id | Kind | Builds | Depends on |
|---|---|---|---|
| 36a | LANE | `theseus-mcp`: the client (stdio, streamable HTTP), and a fake server | nothing: can start now |
| 36b | SPINE | MCP tools in turns: the `&str` trait change, `McpBoard`, `[mcp.servers]`, stored lists, class and external rules, `theseus-sim fake-mcp`, every surface | 36a; step 17 (L1), or L0 with a warning |
| 36c | SPINE | MCP prompts: `turn.submit { prompt }`, `/prompt`, `theseus prompt`, the web picker | 36b |
| 37a | SPINE | the repeating wake: `every`, `days`, `until`; the re-arm in `take_wakes`; ~~execution schema 3~~ store format 5; the hold rule. **Done 2026-10-03** (Part III Item 84) | T1b (4b) |
| 37b | SPINE | tasks set one-shot wakes (theseus-7kg) | 37a |
| 38a | SPINE | bindings format 2 (many guilds), per-place ceilings in the gate, tools offered, spend, and profile | T1b (theseus-e89) |
| 38b | SPINE | gliding: `channel.post`, `channel.read`, on the place rule (rewritten 2026-10-04) | 38a |
| 39a | SPINE | the `TASK` record kind, three layers, CAS, the tools, the view in context | 37b |
| 39b | SPINE | claim leases; the board and `/tasks`; the layer-1 card; the web task graph | 39a |
| 41a | LANE | `theseus-mcp::server`: `/mcp`, the key, Origin, rate limit, a fake core | 36a |
| 41b | SPINE | the server's wire-in: `[mcp_server]`, `Surface::Mcp`, principal `mcp`, cards to the operator | 41a; step 9; theseus-d64 |
| 42a | SPINE | `budget.list`, `policy.explain`, `theseus budgets`, `theseus policy explain` | 36b; 38a |
| 42b | LANE | the Budgets, Ledger, and Policy tabs in `web/` | 42a; step 9 |
| 43a | SPINE | `extend.propose`: freeze, start in L1, test, manifest, the ack card | 36b; step 17 |
| 43b | SPINE | load on ack, restart, revoke, `/extensions` | 43a |
| 44a | LANE | voice engine: the spike, then `theseus-voice` (seam, pipeline, stand-ins) | a test voice channel |
| 44b | SPINE | voice wire-in: voice places, `/join`, utterances into turns, replies in voice | 44a; 38a |
| 45a | LANE | the chosen speech providers behind `Speech` | Eddie's choice and keys |
| 45b | SPINE | speech as spend: catalog prices, reserve and settle, `speech.*` rows, the voice latency class | 45a; 44b; M5's latency table |

### Order

- **On `main`** (SPINE): 37a, 37b, 36b, 36c, 38a, 38b, 39a, 39b, 41b, 42a, 43a, 43b, 44b, 45b.
  - 37a and 37b go before 36b, against the roadmap's numbering, because they are small and need nothing
    new. They run while 36a is still in its worktree.
- **In worktrees** (LANE):
  - **36a starts now**, beside Stages 1 to 5, since it touches nothing on `main` but the workspace's member
    list.
  - **41a** starts as soon as 36a's types settle.
  - **The 44a spike** runs now too, because its answers decide voice's shape.
  - **42b** follows 42a. **45a** follows Eddie's choice.
- **Joins:** 36a lands with 36b, 41a with 41b, 42b after 42a (rebuilding `web/dist`), 44a with 44b, and 45a
  with 45b.
- **From other phases:**
  - T1b and step 9's protocol push (Stage 1), and theseus-d64 (fix batch 2).
  - L1 (step 17), for 36b's sandbox and all of 43.
  - ~~Confidentiality labels (step 19) later replace 38b's audience rule.~~ The place rule removed the labels
    (2026-10-03), and 38b takes the place rule directly (2026-10-04).
  - M5, for the voice latency class; M6, for the server's memory tools.
  - None of this phase depends on step 40.

### Each step's tests and live check

The live checks reuse the harness patterns in the agents' operating notes for this repo: `mkconfig.py`, `start.sh`, `rpc.py`, `waitturn.py`,
and `waitevent.py`; the fake Discord REST (`theseus-sim fake-discord`); and, from 36b, `theseus-sim
fake-mcp`. Every wait is event-driven, and every tool call stays under two minutes.

**36a (LANE).**
- *Tests:*
  - the handshake, and version negotiation;
  - a call; paging;
  - `list_changed`;
  - a crash mid-call, a timeout, a cancel;
  - SSE parsing (split frames, comments);
  - the HTTP session id, and a 404 that forces a new `initialize`;
  - an error result.
- *Live:* the crate's example client against a real reference server started with `npx` (the "everything"
  test server), and against the fake over HTTP.

**36b.**
- *Tests:*
  - config: unknown keys refused; the template round-trips; `read` must name a real tool;
  - the gate: postures from `[policy.mcp]`; a `read` tool's class; a server's hints never loosen anything;
  - a core test with the fake server: a stand-in model calls `mcp__fake__echo`, and gets a result node, a
    notice, and T1's hold;
  - a restart offers the stored list before the server is up, and the first call waits for that server
    alone;
  - a crash restarts with backoff, then `failed`;
  - `list_changed` applies at the next turn, with a notice;
  - after the daemon's `kill -9`, no server is left running.
- *Bench:* the cold start's p95 is unchanged with three fake servers configured.
- *Live:* a scratch daemon with the fake and one reference server.
  - GLM calls a tool, and gets the notice, the result, and the hold.
  - `theseus mcp` lists the servers and tools.
  - A shutdown and a start: the tools are offered at once, and the frames show no spawn before serving.

**36c.**
- *Tests:*
  - core: a prompt's messages become input nodes (`origin: mcp`, external as the server is);
  - Discord: the autocomplete answer, the modal, and its parse;
  - the CLI;
  - a changed definition gives its notice.
- *Live:* `theseus prompt fake/greet --arg name=Eddie`, with the reply at the fake Discord. `/prompt` itself
  needs a person to type it: it goes into Eddie's testing.

**37a.**
- *Tests:*
  - the kernel, under the virtual clock: the re-arm; three missed while down make one turn with `missed: 3`;
    `until`; a cancel ends the series; the cap counts it;
  - a zone with daylight saving (a fixed zone in a unit test);
  - the core, through the driver: two occurrences, two replies;
  - the daemon: `kill -9` across two occurrences makes one catch-up turn;
  - `every` waits in a holding session;
  - the kernel-sim with wakes.
- *Live:* with `min_repeat_minutes = 1`: "every minute, tell me the time".
  - Two occurrences post at the fake Discord.
  - A shutdown, 2.5 minutes down, and a start give one turn that says what it missed.
  - `theseus cancel` ends the series.

**37b.**
- *Tests:*
  - the issue's two: a task sets a 2 s wake, parks, wakes, and reports once; a cancelled task's wake never
    fires;
  - a `kill -9` while parked;
  - the parent's spend holds the task's;
  - `wake_parent` fires at the real end.
- *Live:* a GLM task, "check `git log -1` now, and again in 20 s, then report". The report posts once, after
  the second check.

**38a.**
- *Tests:*
  - format 1 and format 2 both parse;
  - a floor makes a `notify` tool wait;
  - a tool outside `tools` isn't offered, and a call naming it fails as unknown;
  - the spend limit is the lower of the two, and follows changes;
  - a task inherits the ceiling;
  - interactions route correctly across two guilds.
- *Live:* the fake Discord with places in two guilds, and turns submitted into each place's session.
  - The floored place's `proc.run` waits, and the other's runs with a notice.
  - Health and the Observatory show both guilds.

**38b.**
- *Tests:*
  - the rule's whole matrix: private and shared, each way, and the same place;
  - a glide reaches the other lane once (the nonce), in order;
  - a post into a private place runs; one out of a private place asks first, posts once when approved, and
    posts nothing when declined;
  - the destination's floor applies, and T1's hold makes `channel.post` wait;
  - a place not bound here fails with words;
  - a read from a shared place is outside text, and holds its session; a read of a private place from a
    shared one asks first;
  - the ledger rows.
- *Live:* a scratch daemon, a CLI session (private) and a fake shared channel: posts and reads each way,
  showing allowed and asks first.

**39a.**
- *Tests:*
  - a stale version is refused;
  - a proposal waits; accept applies it; decline leaves it; a job's process can't accept;
  - evidence is never removed;
  - a crash between steps: records survive, and DD7's sessions without records are still listed;
  - the view is bounded, and gives a count of what it left out;
  - a plain turn's 5 frames and its token count are unchanged.
- *Live:* GLM plans a fix batch as three tasks, splits the second, and closes one with a commit as evidence.
  - A layer-1 change is proposed, then accepted with `theseus confirm`.
  - Two sessions edit one task; the stale edit is refused (id9's race harness).

**39b.**
- *Tests:*
  - a lease expires under the virtual clock;
  - two executions claim one task;
  - the board renders and edits (Discord's render tests);
  - the card's ids parse.
- *Live:* two sessions claim one task, and one gets `blocked … until`. The board at the fake Discord is one
  message, edited in place.

**41a (LANE).**
- *Tests:*
  - the handshake;
  - a wrong key gets 401;
  - a foreign `Origin` is refused;
  - `tools/list`;
  - a conversation round trip on the fake core;
  - `running` past `wait_secs`;
  - the rate limit.
- *Live:* the example server on the fake core, driven by 36a's client, and by Claude Code as a real client.

**41b.**
- *Tests:* a daemon test with the real listener and 36a's client:
  - open, send, and get the reply;
  - an acting call waits, and the CLI approves it;
  - an approval from the MCP surface is refused;
  - a job's process that opens a session through MCP passes on its hold;
  - the bench is unchanged with the server enabled.
- *Live:* `[mcp_server] enabled`, with a fake `op` giving the key. Claude Code opens a conversation and gets
  its reply. An acting call waits for `theseus confirm`.

**42a.**
- *Tests:*
  - the protocol shapes;
  - `policy.explain` agrees with the gate for every tool in the registry (a table test over the template's
    tools, one MCP tool, and one floored place);
  - the CLI's output.
- *Live:* `theseus budgets` with a parent and two tasks. `theseus policy explain` shows a tightening and a
  floor.

**42b (LANE).**
- *Tests:* the web lint and build (the gate). `web/` has no test runner; adding one is filed.
- *Live:* Tabitha checks it in a browser against a scratch daemon, with `[web]` on a port other than 7433.
  - Each tab loads.
  - Each tab follows live changes: a turn's cost appears, and a tightening appears and is undone.

**43a.**
- *Tests,* with `theseus-sim fake-mcp` as the proposed server:
  - the freeze's digest is stable;
  - an edit in the workspace after proposing changes nothing;
  - the tests' passes and failures are recorded;
  - no tool is offered before the ack;
  - a job's process can't ack;
  - a decline loads nothing.
- *Live:* GLM writes a tiny stdio MCP server (a word counter) in the scratch workspace and proposes it. The
  card shows at the fake Discord, and `theseus confirm` acks it.

**43b.**
- *Tests:*
  - the tool is offered from the next turn, never mid-turn;
  - a restart starts it after serving, from the frozen copy;
  - a revoke drops its tools and stops its process;
  - a second version replaces the first only once acked.
- *Live:* go on from 43a's check.
  - Call the counter. Restart the daemon, and call it again.
  - Revoke it, and check that the tool and its process are gone.

**44a (LANE).**
- *The spike's answers are its first deliverable* (§6).
- *Tests,* through the seam with WAV fixtures:
  - an utterance ends after 700 ms of silence;
  - two speakers are kept apart;
  - a barge-in stops playback within 300 ms of speech;
  - the acknowledgment comes after 2 s;
  - a report waits for the pause;
  - an unlisted speaker is dropped.
- *Live:* the crate's example joins the test voice channel while Eddie's daemon doesn't bind it (T1b's
  disjoint rule). It plays the stand-in clip, and logs frames received per SSRC.

**44b.**
- *Tests:*
  - the binding's tests, through the seam;
  - the core: an utterance becomes a user node by its speaker;
  - the reply's text reaches the text lane whole, even after a barge-in.
- *Live:* Eddie types `/join` in the test text channel, and the bot answers speech with the stand-in voice.
  This part is his end-of-build testing.

**45a (LANE).**
- *Tests,* against fakes:
  - partial and final transcripts;
  - a dropped stream;
  - usage figures;
  - the time to first audio.
- *Live:* the example against the real provider, with the key: a WAV fixture in, audio out.

**45b.**
- *Tests:*
  - the arithmetic: reserve at the utterance's close, settle, and hold unknown usage;
  - at the limit, voice says once (a canned clip) that it has reached the limit, and the budget question is
    asked as usual;
  - until the question is answered, utterances are dropped, and the text channel says so.
- *Live:* Eddie speaks. The `speech.stt` and `speech.tts` rows carry costs, and the session's spend rises by
  them.

## 4. What it needs from Eddie

| # | What | Blocks? | When | If he doesn't say |
|---|---|---|---|---|
| 1 | **Speech providers and their keys** (speech to text, text to speech). Recommended: Deepgram for both (nova-3 and Aura-2, one key), as the existing voice agent runs today. Alternative: Cartesia for text to speech, §3.11's pair. The vault has no speech key yet. | 45 only | at the very end, as he asked | voice stays on the stand-ins (the roadmap's item 4) |
| 2 | **A private test voice channel** in the operator's Discord server, beside #theseus-test, where the bot may Connect and Speak | 44's live checks | when 44a starts (the spike needs none) | 44 is proved through the seam only |
| 3 | **His voice**, for the receive half of 44b's and 45's live checks | those checks | last: "We'll do both last!" | the checks wait for his testing |
| 4 | **The MCP server's key:** a new vault item with a random value, its `[secrets]` line, and `[mcp_server] enabled = true` in the vault note | nothing in the build (scratch checks use a fake `op`) | when he wants the server on his daemon | the server stays off |
| 5 | **Which MCP servers** he wants on his daemon, and their secrets. The audit found none in use. The GitHub PATs already in the vault could back a GitHub server. | nothing | any time | the build proves with the fake and a reference server |
| 6 | **Paste lines:** `[mcp.servers.*]`, `[mcp_server]`, `[kernel] min_repeat_minutes` (optional), and the speech catalog rows. The service account is read-only. | nothing | each step's report names its lines; install before pasting | the defaults hold |
| 7 | **More places:** which guilds and channels, and their ceilings (he edits `bindings.toml`, or asks Tabitha) | nothing | any time | the DM and #theseus-test |

**Decisions taken by default.** He can overturn any of them later:
- a repeating wake waits in a session that read external text, though T1b exempts one-shot wakes;
- MCP results are external by default;
- acting calls from the MCP surface wait for approval;
- voice shares its text channel's session;
- extensions run only in L1, never at L0.

## 5. Open questions, each with its default

| # | Question | The default the build takes |
|---|---|---|
| 1 | The MCP client: hand-written, or the official SDK? | Hand-written (§2.1). Revisit with elicitation, sampling, and OAuth. |
| 2 | Which protocol revision? | The newest implemented (2025-06-18 at least); an older one a server answers is accepted from the supported list. |
| 3 | A server's own hints (`readOnlyHint` and the others)? | Shown, and never loosen anything. The operator's `read` list decides. |
| 4 | Are MCP results external? | Yes, by default. `external = false` per server. |
| 5 | A call before its server is up? | It waits for that server alone, up to 30 s, then fails `mcp_unavailable`. |
| 6 | When does a changed tool list apply? | At the next turn's start, with a notice. Never mid-turn. |
| 7 | Start servers eagerly, or on first use? | Eagerly, after serving. First use is filed: the stored lists make it possible. |
| 8 | `every` on `wake.at`, or a new `wake.every`? | A field on `wake.at` (one verb). |
| 9 | The shortest repeat? | 5 minutes, `[kernel] min_repeat_minutes`. |
| 10 | A repeating wake after `/new`? | It stays in its session, and its reply goes to the place. |
| 11 | Repeating wakes in tasks? | Refused in v1. Later, with `until`. |
| 12 | DD7's `task.create` and §3.5's? | One tool. With `brief`, it opens a session, as DD7 does. Without, it records a plan item. Every task gets a record. |
| 13 | When is a task with a session `done`? | When its session reports, with the report as its evidence. The mechanical veto comes later. |
| 14 | The lease's length? | 30 minutes, renewed by each of the holder's turns that touches the task. |
| 15 | Who accepts a layer-1 change? | The task's requester, else the owner, in trusted channels only. |
| 16 | Where do bindings live? | The operator's file, under the floor. Tighten-only edits kept in the store come later. |
| 17 | The audience rule for a glide's post and read? | The place rule (Eddie, 2026-10-04): into a private place it is allowed; out of one, or between two shared places, it asks first. The subset rule it replaces waited for 19a's labels, which the place rule removed. |
| 18 | The MCP server's tools? | `conversation_*`, `task_list`, `wake_list`, and memory once M6 has its methods. |
| 19 | Voice and text: one session or two? | One. |
| 20 | Where does an utterance end? | After 700 ms of silence (the stand-in), until a provider's endpointing replaces it. |
| 21 | An extension's posture after the ack? | `notify`. |
| 22 | An extension's network? | Off, unless asked for and acked. With it off, its output isn't external. |

## 6. Risks, and what would change the plan

| Risk | Where | What we do | What would change the plan |
|---|---|---|---|
| **DAVE.** Discord has been moving voice to its end-to-end encryption, DAVE, and has said it will be required. A songbird release that can't speak it can't join. | 44, 45 | The 44a spike answers it first, before any code lands. | A DAVE-capable songbird (a newer release or a fork), or voice moves past v1. |
| songbird's twilight integration may pin an older twilight than our 0.17 | 44 | Feed voice events through its generic shard trait, or drive its `Driver` from the voice-server update by hand. | Nothing, if either works. |
| libopus in a static musl build, and §9's binary budget (under 60 MB) | 44 | The spike builds it, and `voice` stays a cargo feature. | If it can't build static, voice becomes an opt-in build, off in the default binary. |
| **Tool poisoning, and rug pulls.** A server's descriptions steer every turn. | 36 | Only servers the operator names; descriptions capped; notices on change; results external; postures. | Jev's `security.v1` (M5) scores descriptions. Until then, a description stays in every prompt. |
| Memory per server: an `npx` server is a node process of tens of MB, and §9's RSS target is 1 GB. | 36 | Health shows each server's RSS. Starting on first use is filed. | Starting on first use becomes the default. |
| The prompt cache churns when a tool list changes | 36, 43 | Stored lists, a stable order, and changes only at a turn's start. | Nothing: one cache miss per change, per session. |
| Repeating wakes spend money | 37 | The 5-minute floor; the session's limit asks; the Observatory shows each series' cost. | A per-series limit, if the soak shows drift. |
| The task graph weighs on every turn (compile time, tokens, frames) | 39 | The view appears only when the scope has tasks, in the tail, bounded; the frame-budget test. | If it costs much, fold it into the conversation's task summary (§3.2a). |
| **Per-person authority isn't built.** Every principal is the operator, so a place with several users is safe only while they are all Eddie's trusted users. | 38 | Ceilings as floors, and the audience rule. | A second person in a place moves role grants and per-author executions (filed) up. |
| The MCP server lets a local program drive Theseus | 41 | The key, loopback only, the Origin check, the `approve` floor, cards to the operator, J1's trace. | Per-client tokens (deferred in §1) move up if a second client appears. |
| Self-extension needs L1 (step 17) | 43 | 43 waits for L1. Agent-written code never runs at L0. | Nothing: 43 moves with L1. |
| Step 9's push slips | 41b, 42b | Nothing else waits on it. | 41b polls `conversation_status`, and 42b refreshes on a timer, until it lands. |
| Review load: 19 sub-steps, 14 of them on `main` | all | LANE work in parallel; the dogfood pilot (theseus-14s) may take some steps. | More parallel lanes, once the pilot proves out. |

**What would move things.**
- **Eddie names MCP servers he wants now.** Then 36a and 36b move ahead of Stages 3 to 5. They run at L0 until
  L1 lands, with a health warning. Nothing else in M7 must come first.
- **The spike's answers** (DAVE, twilight, libopus) set voice's shape and timing.
- **The soak shows places with several people.** Then role grants and per-author executions are built,
  which v1 files.
- **Slash commands in threads turn out to matter.** Then threads move into 38.

---

*Sources:* the lanes brief (common part, and lane `m7`); the v1 roadmap; the agents' operating notes for the repo;
[the spec](../the-ship-of-theseus.md) v0.61 (§1, §2, §3.1, §3.2 and §3.2a, §3.5, §3.8, §3.9, §3.11 to §3.15, §3.18, §3.21, §3.24, §9,
Appendix E, P9, and Part III A3c and A4 items 7 to 10); Beads theseus-ext, theseus-7kg, theseus-e89, and
theseus-p3k; its audit (theseus-p3k's usage report, §3 to §5); the code at 27e1237 (read only);
and the existing voice agent's voice modules. No web search or fetch was used.

*— written by Tabitha/Claude*

<!-- REPORT COMPLETE -->
