# Cloud report: MCP tools in turns, step 36b (theseus-ext.1)

Branch `cloud/20261004-mcp-tools`, from `main` at `d9b0931`. This commit is not for `main`; the maintainer drops it
at the merge.

| Commit | Step |
|---|---|
| `29c91aa` | 1. `Tool::name()` / `description()` borrow from the tool; `Tool::wire_name()` |
| `06c14f2` | 2. `[mcp.servers.<name>]` config and the template's commented section |
| `5967b0f` | 3. `McpBoard`, `McpTool`, the stored list, `fake-mcp`, health, `theseus mcp`, rows, narrative, notice |
| `6aa253f` | 3, continued: a test for a server that never comes up (`mcp_unavailable`), and the error-result test renamed |

## 1. The trait change (`29c91aa`)

**What I found.** `Tool::name()`, `description()` and `family()` returned `&'static str`. An MCP server's tool gets its
name from the server's list at runtime, so it can't return a `'static` string.

**What changed.**
- The three methods now return `&str` borrowed from `&self`.
- An impl that still returns `&'static str` satisfies the new signature. So no built-in tool changed, and the AWS and
  language-server tools other changes add compile as they are. The merge stays mechanical.
- Only one test wrapper changed: `tests_m3`'s `Slowed`, which forwarded another tool's description.
- New trait method `Tool::wire_name()`. It defaults to the dotted name with underscores, so an MCP tool can call
  itself `mcp__<server>__<tool>`. `Registry::by_wire` and `definitions_of` use it.

**How I proved it.** `cargo check --workspace --all-targets` is clean, and every suite passes in the gate (below).

## 2. `[mcp.servers.<name>]` (`06c14f2`)

**What changed.**
- New file `crates/theseus-core/src/config/mcp.rs` with `McpConfig` and `McpServerConfig`. Their keys:
  - stdio: `command` (argv, no shell), `env` (variable = a `[secrets]` name);
  - HTTP: `url`, `auth_secret` (sent as a bearer token);
  - both: `read`, `sandbox`, `external` (default true), `enabled` (default true), `start_timeout_secs` (30),
    `call_timeout_secs` (110, at most 115).
- Every table uses `deny_unknown_fields`. The checks:
  - exactly one transport;
  - every secret is a `[secrets]` entry;
  - `env` only on stdio, `auth_secret` only on HTTP;
  - a server name is letters, digits, `_` and `-`, at most 32 characters, with no `__` (its tools' wire names carry
    the server name, and `__` is their separator).
- `config.rs` gains only the field, the `validate_mcp()` call, and two lines in the template tests.
- The template gets a commented `mcp` section (one stdio and one HTTP example, the design's own) and a commented
  `docs_mcp_token` secret.

**`sandbox = "l1"` is refused for now, and the refusal names the follow-up.** Starting a long-lived server under L1
needs the job wrapper's L1 path (`theseus-kernel/src/job_l1.rs`, the sandbox's spawn) to keep a child alive with live
pipes to the daemon. Today that path is built for a job that runs to an end and answers in the spool, and another
change is reworking it. So servers run at L0 in this step. **Follow-up:** "MCP servers in L1".

**How I proved it.**
- `config::mcp::tests`: 2 tests. One loads the defaults; the other checks 12 refused shapes, unknown keys first.
- Template tests:
  - `example_template_uncommented_still_parses` now calls `the_templates_mcp_section`;
  - `example_template_is_the_policy_to_set` checks the section stays commented;
  - `example_template_mentions_every_key` passes.

**This commit fails `cargo fmt --check` on its own.** rustfmt reflowed `config/mcp.rs` after I pushed it. The reflow
is in `5967b0f`. I did not rewrite pushed history.

## 3. The board, the tool, the stored list, the surfaces (`5967b0f`)

**Why it is one commit.** The board, the tool, the protocol types and the CLI only make sense together. Splitting them
would have needed a gate run per piece, and the gate takes most of an hour here.

### The board (`crates/theseus-core/src/mcp/mod.rs`)

**Start.**
- The board starts every enabled server after serving (`core.mcp.start()` in theseusd's `after_serving`). Nothing is
  spawned before serving.
- stdio servers:
  - spawned through `children::spawn(Kind::Owned, …)`, using theseus-mcp's `stdio_command` (its own process group);
  - environment cleared, then the job's environment (`proc_env`), then the server's `env` secrets read from the
    secret board;
  - working directory is the first workspace root;
  - stderr goes to `<state>/mcp/<name>.log`, capped at 1 MiB.
- HTTP servers use theseus-mcp's `HttpTarget` with the bearer key.
- How the board connects is behind a `Connect` trait. The daemon uses `Spawn`; the tests use an in-process fake
  (`McpBoard::set_connect`, so `Parts` and the Discord crate's constructions are untouched).

**States.** `stopped` (before serving), `starting`, `ready`, `restarting`, `failed`, `disabled`.
- A crash, or a start that fails or doesn't finish within `start_timeout_secs`, restarts after 1 s, then 5 s, then
  30 s.
- The third crash within 10 minutes leaves the server `failed`, with its reason, until `mcp.restart`. A restart also
  forgets the crash count.

**Stop.**
- Every clean stop (`stop_record`: `shutdown`, SIGINT, SIGTERM, a restart onto a changed note) calls
  `core.mcp.stop()`. That sends SIGTERM to each server's process group and never waits.
- A stdio server whose stdin closes ends on its own, so the daemon's `kill -9` leaves none running.
- I did not add `PR_SET_PDEATHSIG`. It follows the spawning *thread*, and tokio's blocking threads come and go.

**`list_changed`** (or an HTTP session made again) lists the tools again and rebuilds the catalog. A turn's
`RequestSpec` is fixed at its start, so a turn keeps the tools it began with and the new list applies from the next
turn.

**A changed tool list** (name, description or schema):
- writes `mcp.tools_changed` (with `added`, `removed`, `changed`) plus a narrative line;
- posts an operator notice to the outbox (`kind: "mcp_changed"`);
- the Discord courier gets a small arm for it, posted to the owner's DM via `operator_channel`, as `restarted` is.

**Catalog.** The catalog is rebuilt from every server's current list:
- each tool's wire name comes from theseus-mcp's `names::wire_names` (unique across servers);
- tools are sorted by canonical name;
- each tool is granted its server's secrets (`Broker::grant_tool`), so `ToolRuntime::brokered` holds every call to
  no looser a posture than its secrets'.

### The stored list

- Each server's last good list is a META record, `mcp.tools.<server>`: the tools, plus a digest of their names,
  descriptions and schemas.
- It is written off the runtime's workers, only when the list differs from the one offered.
- `Core::build` reads it, one META key per configured server, so a start offers the tools at once.
- A call to a server that isn't up yet waits for that server alone (`Server::client`, a watch on its state), up to
  `start_timeout_secs`. After that it fails `mcp_unavailable: … Nothing was sent`.

**The store's format is unchanged.** `mcp.tools.<server>` is a value under the existing META kind, and the new ledger
rows are new `LedgerKind` names in the existing LEDGER record. No new record kind and no new field on a stored
record, so `MANIFEST_FORMAT` stays and `tests_layouts` needs no sample. If the owner reads the version rule as
covering META keys too, this is the place to bump it.

### `McpTool` (`crates/theseus-core/src/mcp/tool.rs`)

- Canonical name `mcp:<server>/<tool>`, the name the gate and `[policy.mcp]` use. Wire name `mcp__<server>__<tool>`.
  Family `mcp`. Backend `Async`.
- Class and retry:
  - `Run` and `NonRepeatable` by default;
  - `Read` and `SafeToRepeat` only when the server's `read` lists the tool;
  - the server's hints are shown in `mcp.list` and never loosen the class.
- `input_schema` gets `"type": "object"` when the server omits it.
- `plan` refuses a non-object input. Its summary is `mcp:<server>/<tool> <args, cut to 120 characters>`. The gate
  decides by name, as everywhere.
- Descriptions are capped at 2,000 characters, and the cap says so.
- `deadline` is start wait + call timeout + 5 s, within the async path.

**Results.**
- `text_for_model()` renders the result: text as text; resource links and embedded resources with their URIs;
  `structuredContent` as JSON text.
- Results are outside text unless the server has `external = false`. That uses the async path's existing
  `External`, so the hold is written in the result's own frame (`external::with_hold`).
- An `isError` result is a failure the model reads, and it is still outside text.
- A timeout or closed connection gives status `unknown` and completion `Outcome::Unknown`, so the kernel records
  `action.outcome_unknown`.
- How toolrun reads these: `run_inproc`'s async branch now keeps the failure's `meta`, and a new `failure()` helper
  reads `outcome_unknown` and `external` from it. This is the one change to toolrun's existing call path.
- **Images are not sent as image blocks.** An async tool can't return an image today (`AsyncResult` has no image),
  so an image is a line naming it. Follow-up: give `AsyncResult` an optional `ImageData`.

**Cancel and `/stop`.** These abort the call's task (`Stops::track`, as for every async call). That drops the
client's request future, and theseus-mcp sends `notifications/cancelled`. A daemon restart mid-call is
`outcome_unknown`, as for any async call, and `NonRepeatable` means nothing runs it again.

### The runtime (`toolrun.rs`)

- New field `mcp: Arc<McpCatalog>`.
- `tool(name)` and `tool_by_wire(wire)` look in the registry, then the catalog. `tool_of`, `admit`, `unknown_tool`
  and `result_node_in` use them.
- `definitions_for(class)` appends MCP tools after the built-ins, filtered by `places::offered`, so a shared place
  gets none. Separately, the gate refuses an MCP call there (`places::refusal`).
- The tools note gets one line naming the attached servers and saying their results are outside text.

### Surfaces

- **Health.** `health.mcp[]` (`McpServerStatus`) gives, per server: transport, state, pid, tools, prompts, whether
  the stored list is in use, calls, errors, crashes, last error, start time, protocol, digest, and `read` entries the
  server doesn't list. `theseus health` prints an `mcp:` line.
- **Protocol.** New methods `mcp.list` and `mcp.restart`, dispatched in `rpc/server.rs`, with types in
  `theseus-protocol/src/mcp.rs` and their TypeScript generated. `tool.list` (`theseus tools`) lists MCP tools too.
- **CLI** (`crates/theseus/src/mcp.rs`, `render/mcp.rs`):
  - `theseus mcp` lists each server with its state, then its tools with wire name, class, posture, calls, and the
    server's hints;
  - `theseus mcp restart <name>`;
  - `main.rs` gains only the `mod`, the variant, and the dispatch line.
- **Ledger and narrative.** New rows `mcp.started`, `mcp.ready`, `mcp.exited`, `mcp.failed`, `mcp.tools_changed`, each
  a fact in `fact/mcp.rs` with its narrative line (for example "MCP server fake ready in 4 ms: 5 tools, 3 prompts.").
  A call rides on its tool call's own rows.
- **Telemetry.**
  - A call's span carries `mcp.call`, `mcp.server` and `mcp.tool`.
  - Calls and durations are already counted by `theseus.tool.calls` and `theseus.tool.duration_ms` with
    `theseus.tool.family = "mcp"` and the canonical name.
  - I did not add `theseus.mcp.calls` or `theseus.mcp.call.duration`: they would duplicate those series.
  - I did not add `theseus.mcp.servers.up` either: it needs a gauge kind the metrics module doesn't have.
- **`theseus-sim fake-mcp`** (`crates/theseus-sim/src/fake_mcp.rs`) serves theseus-mcp's `Fake`. By default it uses
  stdio and exits 3 on a crash; `--http` serves on 127.0.0.1. Its modes are `ok`, `slow`, `crash-after=N`,
  `change-tools`, `error`.
- **The reader rule.** theseus-mcp is now reached (core → theseus-mcp), and `tests_registry` refuses a `reserved_for`
  on a reached crate. So I removed the marker, and a comment in its `Cargo.toml` says the `server` feature waits for
  row 72 (41b). The core and the sim take the crate with `default-features = false`.
- **AGENTS.md.** One new map line in the root file, a new "MCP servers" bullet in theseus-core's, and a new
  `crates/theseus-mcp/AGENTS.md` (plus `CLAUDE.md`), since the crate had no guide.

### How I proved it

**Core tests** (`crates/theseus-core/src/mcp/tests.rs`, 9 tests, plus the 2 config tests). They use an in-process
fake over pipes and a stand-in model.
- `a_model_calls_an_mcp_tool_and_gets_its_result_a_notice_and_the_hold`:
  - MCP tools are offered after the built-ins;
  - the tools note names the server;
  - the result node is `ok` and outside text;
  - one `tool.notified` row for `mcp:fake/echo` (enforcement `notify`);
  - the session's T1 hold has tool and url `mcp:fake/echo`;
  - `mcp.started` and `mcp.ready` rows;
  - health and `mcp.list` show class `run`, posture `notify`, 1 call, hint `read-only`;
  - the stored list was written.
- `a_shared_places_turn_is_offered_no_mcp_tool`:
  - a session bound to an unbound guild channel is offered no `mcp__` tool;
  - the model's call anyway is answered `Not run: mcp:fake/echo is not offered in a shared place`;
  - the server saw 0 calls;
  - a private session on the same core is offered the tools.
- `a_changed_list_applies_at_the_next_turn_with_its_row`:
  - in `change-tools` mode, both loops of the calling turn offer the same tools;
  - the next turn is offered `mcp__fake__tool_v1`;
  - `mcp.tools_changed` records `added: [tool_v1]` and `changed: [echo]`;
  - the `mcp_changed` operator post is in the outbox.
- `a_stored_list_is_offered_at_once_and_a_call_waits_for_its_own_server_alone`:
  - the stored tool is in the catalog with no server started;
  - a call made before `start()` waits, then runs once its server is up;
  - a second server that never comes up delays nothing;
  - the live list then replaces the stored one.
- `a_crash_restarts_after_one_then_five_seconds_then_fails_until_restarted` (tokio's paused clock):
  - connects happen at 0 s, 1 s, and 1 s + 5 s;
  - then the server is `failed` with crashes 3 and the reason, and stays failed for an hour;
  - a call is told to run `theseus mcp restart fake`;
  - `restart` brings it to `ready` with 0 crashes.
- `a_servers_hints_never_loosen_a_tool_and_read_makes_it_read`:
  - `echo` has hint `read-only` and stays `Run` / `NonRepeatable`;
  - `add`, listed in `read`, is `Read` / `SafeToRepeat`;
  - tools are sorted, and wire names are at most 64 characters;
  - a non-object input is refused.
- `an_error_result_is_a_failure_and_still_outside_text` (renamed in `6aa253f`; its old name promised the down-server
  case it didn't build): in `error` mode the result's status is `error`, and it is outside text.
- `a_call_to_a_server_that_never_comes_up_is_unavailable` (`6aa253f`):
  - the stored list offers `echo`, and the server never answers within its 1-second start timeout;
  - the model's call is answered `mcp_unavailable: MCP server fake …` and says "Nothing was sent";
  - the result is `error`, not `unknown`, and not outside text.
- `a_description_is_capped`.

**Daemon tests** (`crates/theseusd/tests/mcp.rs`, 2 tests, with the real `theseusd` and `theseus-sim fake-mcp`).
- `a_server_starts_after_serving_and_none_outlives_the_daemons_kill_9`:
  - the server reaches `ready` with 5 tools;
  - `mcp.started`'s WAL position is after `server.serving`'s (no spawn before serving);
  - `mcp.list` names `mcp__fake__echo`;
  - after `kill -9` of the daemon, the server's pid ends within 10 s.
- `a_start_offers_the_stored_list_before_its_server_is_up`:
  - a clean stop, then a start on the same state whose server is `sleep 30` (never answers);
  - health says `stored: true`, 5 tools, not ready;
  - `mcp.list` has the 5 tools.

**Planted reverts.** For each one I planted the bug, saw the test fail, restored the file, ran `touch`, and checked
`git status`.
1. MCP results not external (`external: Option<External> = None` in `run_async`): the hold test failed at
   `tests.rs:463`, "an MCP result is outside text".
2. MCP tools offered in a shared place (the `offered` filter in `definitions_for` made `|_| true`): the shared-place
   test failed at `tests.rs:507`, "a shared place is offered no MCP tool: [http_fetch, task_create, wake_at,
   web_search, mcp__fake__add, mcp__fake__echo, …]".

Both tests pass again after the restore.

**Under load.** AGENTS.md's recipe: four `while :; do :; done` loops at nice 0, the tests at `nice -n 19`. Run on
`5967b0f`, before the ninth core test existed: the 10 core MCP and config tests plus the 2 daemon tests, 5 runs,
12/12 passed each time. I killed the loops by their pids.

**The cold-start bench with three fake servers.** `theseus-sim bench lifecycle --phases cold --runs 20`, with
`--config` set to the bench's own config without and with three `[mcp.servers]` tables running
`theseus-sim fake-mcp`. A stub `op` was on PATH, as the bench's own fake op. Debug build, this 4-core VM:

| Config | Run 1 p50 / p95 | Run 2 p50 / p95 |
|---|---|---|
| no servers | 12.0 / 16.0 ms | 12.2 / 14.5 ms |
| three servers | 12.8 / 17.4 ms | 11.9 / 12.9 ms |

- That is unchanged within the noise, and well under the 50 ms budget. The daemon reads one META key per server
  before serving and spawns nothing.
- `<state>/mcp/{a,b,c}.log` show the servers were started after serving, and no `fake-mcp` was left running after
  the bench.
- `bench turn --check`: plain turn 5 frames (budget 5).
- The maintainer measures the real numbers.

## The live check (the maintainer's)

Run it on a scratch daemon with a fresh state dir, on the branch's build. `$T` is the repo's `target/debug`. The
second server is MCP's reference "everything" server (needs `npx` and the network).

```sh
mkdir -p /tmp/mcp-live && cd /tmp/mcp-live
$T/theseusd example-config --plain > config.toml
# In config.toml: [server] state_dir = "/tmp/mcp-live/state", socket = "/tmp/mcp-live/sock";
# [web] enabled = false; [discord] enabled = false; the live profile set to GLM, as for any scratch check.
cat >> config.toml <<EOF

[mcp.servers.fake]
command = ["$T/theseus-sim", "fake-mcp"]
read = ["add"]

[mcp.servers.everything]
command = ["npx", "-y", "@modelcontextprotocol/server-everything"]
start_timeout_secs = 120
EOF
$T/theseusd --config config.toml --state-dir state --socket sock 2> theseusd.log &
S="--socket /tmp/mcp-live/sock"
$T/theseus $S health | grep '^mcp:'
#   mcp: everything ready (N tools, …) · fake ready (5 tools)   (everything may say starting at first)
$T/theseus $S mcp
#   each server, then mcp__fake__add read/notify, mcp__fake__echo run/notify ("server says read-only"), …
$T/theseus $S ask "Call the fake MCP server's echo tool with the text 'live check', then tell me what it returned."
#   the reply quotes "live check"
$T/theseus $S history
#   the call mcp:fake/echo with its notify notice (policy.notified), its result, and the session now holding external text
$T/theseus $S rpc ledger.tail '{"n":500}' | jq -c '.rows[] | select(.kind|test("^(server.serving|mcp\\.)")) | [.position,.kind,.data.server]'
#   server.serving first, then mcp.started / mcp.ready for each server: no spawn before serving
$T/theseus $S ask "Now write the word ok to a file called ok.txt"
#   fs.write waits for approval: the hold after the MCP result (until `theseus policy trust <session>`)
$T/theseus $S shutdown
$T/theseusd --config config.toml --state-dir state --socket sock 2>> theseusd.log &
$T/theseus $S mcp          # at once: both servers' tools listed (stored), states starting or ready
$T/theseus $S rpc health | jq '.mcp[] | {name, state, stored, tools}'
#   right after the start: stored true and the tool count; ready a moment later
$T/theseus $S ask "Call mcp__fake__add with a=2 and b=3"   # answered; with no server up yet, the call waits for it
$T/theseus $S mcp restart fake      # "MCP server fake restarting (it was ready)."
kill -9 $(pgrep -f 'theseusd --config config.toml'); sleep 1; pgrep -af 'fake-mcp|server-everything'   # nothing
```

## What is left, and what the owner should hear

- **MCP servers in L1** (the follow-up named in `sandbox = "l1"`'s refusal). It needs the L1 spawn path to keep a
  long-lived child with live pipes to the daemon, outside the job wrapper's run-to-an-end shape.
- **Images from an MCP tool** are a line of text, not an image block: `AsyncResult` carries no image.
- **`secret.granted` at the spawn.** A server's env secrets are read from the board and given at its spawn, but I
  wrote no `secret.granted` row then; each call is still held to the secret's posture through the broker's tool
  grant. Ledgering the spawn's grants needs a fact without a turn (the existing `tool::SecretGranted` borrows a
  turn's context).
- **Metrics.** No separate `theseus.mcp.*` series: calls are `theseus.tool.*` with `family = mcp`.
  `theseus.mcp.servers.up` needs a gauge kind.
- **Prompts.** Only counted (health's `prompts`); 36c owns their use.
- **The cockpit's MCP section should show:**
  - each server's state, transport, pid, tools and prompts, calls and errors, crashes and last error, protocol, the
    stored and live digests, and a restart button (`mcp.restart`);
  - each tool's wire and canonical name, class, posture and its setting, the server's hints, and the description
    in full (`mcp.list`).
- **Docs for the maintainer to write:**
  - Part III's item for 36b;
  - `docs/status.md`'s row 66;
  - `docs/design/README.md`'s reserved list: theseus-mcp is no longer reserved; its `server` feature waits for
    41b;
  - in `docs/design/m7-surface.md` §2.1: "after the vault confirms" is now "after serving", the Observatory section
    is now the cockpit's, and servers run at L0 until the follow-up.
- **The shared files I touched:**
  - `crates/theseus-core/src/turn.rs`: `trace_calls` takes the runtime (5 lines) and one call site;
  - `crates/theseus-core/src/config.rs`: the field, the call, two test lines;
  - `crates/theseus-protocol/src/lib.rs`: the `mod`, two method names, the health field;
  - the CLI's `main.rs`: the `mod`, the variant, the dispatch line;
  - `crates/theseus-discord/src/courier.rs`: one post kind;
  - `scripts/` untouched.
- **The `Tool::name()` change.** Other changes adding tools can keep `&'static str`. Only a tool that forwards
  another's name or description needs `&str`.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on `6aa253f` (the report's parent): fmt, shape, clippy, the builds, and the
reader rule pass. The suite ran 1,765 tests: 1,732 passed, 33 failed, 10 skipped, and no test passed only on a retry.
The same 33 failed on `5967b0f`.

The suite's failures, none of them this change's:
- 32 L1 tests. The VM runs everything as root, and L1 refuses a root daemon's jobs (theseus-pv6i):
  - 19 of `theseus-sandbox::contract` (`a_job_that_cannot_start_says_why`, `clause_01` to `clause_12`, the three
    `egress_18b_*`, `exit_status_and_signals`, `scratch_is_reported_and_discarded`,
    `sigterm_is_forwarded_to_the_command`);
  - `theseus-sandbox::bench spawn_100`;
  - 12 of `theseusd::sandbox`'s L1 tests.
- `theseus-core tests_output::the_cores_output_matches_its_golden`: the golden's wake line has a `-#:#` UTC offset
  (the owner's zone), and this VM is UTC (`+00:00`). It passes with `TZ=America/Los_Angeles` on this branch.

The phases after the suite, run by hand on the committed tree, all pass:
- protocol types (TypeScript committed);
- `bench turn --check --runs 5 --burst 0` (5 frames);
- `cargo deny --offline check` (advisories, bans, licences, sources all ok; the database was fetched this session);
- web lint and build; cockpit lint, test and build; web dist unchanged.
