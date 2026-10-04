# Cloud report: the MCP server's wire-in, step 41b (theseus-ext.2)

Branch `cloud/20261004-mcp-server`, from `main` at `d9b0931`. One step, one commit: `5fbd647`. Started 03:00 UTC,
report written 04:05 UTC.

## Step 41b: the MCP server wired in

### What I found

- `theseus-mcp`'s `server` module (41a) was complete: the listener, Host/Origin/key/rate checks, the five tools, and
  the `CoreClient` trait with a fake core. It had no peer (uid) check, which the web UI has (theseus-3qf).
- The design's "peer traced through `/proc/net/tcp`" no longer exists as such: theseus-zmgb retired the process walk,
  and `peer.rs` now only reads the client socket's uid. A job's hold passes on through `THESEUS_SESSION`, which only
  the CLI sends (`opened_from`). An HTTP client sends nothing like it, so I had to find the job from the connection.
- `Authority` (stored with every execution) already has `principal` and a free `ceilings` map that nothing read. That
  holds the MCP principal and floor with no new field, so **`MANIFEST_FORMAT` is unchanged** and `tests_layouts` is
  untouched. Tasks inherit it (the kernel copies a parent's authority).
- The Discord binding's in-process client (`rpc_client.rs`) is exactly what `CoreClient` needs. I made its module
  `pub` (one line in `theseus-discord/src/lib.rs`) rather than copy it.
- `turn.submit` returns at once with `stop_reason: "awaiting_confirm"` when a call parks. The reply then comes from
  the driver's continuation turn, so `conversation_send` has to wait past `turn.submit`'s own answer.

### What I changed (`5fbd647`)

- **Config:** `[mcp_server]` (`crates/theseus-core/src/config/mcp_server.rs`): `enabled` (false), `port` (7434; 0
  picks one), `key_secret` (`mcp_server_key`), `posture_floor` (`approve`), `spend_limit_usd` (5.0),
  `requests_per_minute` (60). Validation: when the server is on, the key must be a `[secrets]` entry, the limit must
  be above 0, and rpm at least 1. The template gets a commented `[mcp_server]` block and a commented
  `mcp_server_key` line in `[secrets]`. `example_template_uncommented_still_parses` asserts it. `mentions_every_key`
  and `parses_and_validates` pass unchanged.
- **Core rules** (`crates/theseus-core/src/mcp_server.rs`, `rpc/mcp.rs`):
  - `Surface::Mcp` (approval.rs). `Answerer::unknown` refuses it ("never an approval surface"), so an answer, trust,
    press, undo, or publish from it gets REFUSED. It is ledgered as `approval.refused` with `via: mcp` through the
    one judgment.
  - Dispatch on `Surface::Mcp` allows only the tools' methods and those owner's acts (which then get refused). Any
    other method is refused at once.
  - `session.open` on `Surface::Mcp` opens with principal `mcp`, ceiling `posture_floor = <floor>`, and the
    `[mcp_server]` limit.
  - A `turn.submit` from `Surface::Mcp` must name a session whose principal is `mcp`. So a client can't write into
    the operator's own conversations, which have no floor.
  - The gate (`toolrun.rs`, a call plus a small fn) raises every non-`Read` call in such a session to the floor,
    after external text's hold. If the execution can't be read, the floor is `approve`.
- **Place class: private.** The session's words go back to the MCP client. That client is a process of the daemon's
  own uid (the listener refuses any other) holding the operator's key. Any such process can already read every
  session through the 0600 socket, so `shared` would hide nothing. It would also strip the file tools and
  `proc.run` that the client's turns exist for. The floor is what keeps its acts the operator's.
- **Daemon** (`crates/theseusd/src/mcp.rs`, `mcp/trace.rs`): started in `after_serving` (socket daemon only), once
  its key's secret resolves. Nothing on the start path waits for it. `web.rs` is not edited: the server has its own
  port.
  - `McpCore` implements `CoreClient` over one in-process connection on `Surface::Mcp`.
  - `conversation_send` works like this: `turn.submit` races the wait. On `awaiting_confirm` or `budget` it calls
    `session.wait {settled, after_position}` until the view is ready or idle, then reads the reply from
    `session.history`. Past the wait it answers `{status: "running", turn_id}` (the id comes from `turn.started`)
    and the turn goes on.
  - `conversation_status` uses `session.history` and a zero-timeout `session.wait`.
- **Only the daemon's uid:** `theseus-mcp` `server::Config` gains `admit` (an `Ends -> Result` hook). It runs first on
  each request on the blocking pool and refuses with 403 and a `Why::Peer` refusal. The daemon's hook is the web
  UI's check (`peer::client_uid` and `admit`).
- **A job's hold passes on:** `mcp/trace.rs` finds the client socket's inode (`peer::client_inode`, new, reusing
  `peer.rs`'s table reader). It looks for the holder among the daemon's own descendants only (the daemon is a child
  subreaper, so jobs and their orphans are there). It takes `THESEUS_SESSION` from that process, or from its nearest
  ancestor under the daemon (so a client run with `env -i` still names its job). That is sent as `opened_from` on
  both `session.open` and `turn.submit`.
- **Surfaces:**
  - Ledger: one `mcp_server.call` row per call (tool, session, client, latency_ms, ok), and `mcp_server.refused` at
    most once a minute per kind, with `unreported`. Both are written on the blocking pool.
  - Health: `mcp_server` (a protocol module of its own, `theseus-protocol/src/mcp_server.rs`): state
    (`starting|listening|stopped|failed`), port, sessions, clients, opened, last_client, calls, errors, refused by
    kind, and error. TypeScript regenerated.
  - CLI: a `mcp server:` line in `theseus health` (`render/mcp_server.rs`).
  - Narrative: "MCP client <name> opened session <short>; its calls that act wait for the operator."
  - The tracing span `mcp_server.call` was already in the server.
- **Reader rule:** `theseusd` now depends on `theseus-mcp` (path dependency; Cargo.lock gains only that edge, no new
  package). The registry test fails a crate that is read and still marked, so `reserved_for` is removed entirely, not
  just its 41b half. The 36b lane will find it gone.
- **Bench:** `theseus-sim`'s `bench_config` turns the server on (port 0, key from the fake `op`), so every lifecycle
  phase runs with it, as the index tender does.

### How I proved it

- New tests, all passing:
  - `theseusd/tests/mcp_server.rs` (real daemon, real listener, `theseus-mcp`'s client, real CLI, real job wrappers):
    - `a_client_opens_a_conversation_sends_it_text_and_gets_the_reply`: five tools listed; the label is
      `mcp lantern-agent tide tables`; the limit is $2; the reply comes; status shows turns, last reply, and turn
      id; one row per call with tool, session, client, latency, and ok; a send into the operator's session is an
      `isError`; health counts.
    - `a_wrong_key_and_a_foreign_origin_are_refused`: a wrong key twice gives 401 twice and one `key` row. The right
      key from `http://lantern.example` gives 403 and an `origin` row. The CLI line is checked.
    - `an_acting_call_waits_and_the_cli_approves_it`: under `enforcement = notify`, `proc.run` waits.
      `conversation_send` with `wait_secs: 2` answers `running`. The file is absent. Status shows
      `waiting_on: confirm`. The card says why. `theseus confirm --approve` lets it run, and the client reads
      "Done.".
    - `a_jobs_process_that_opens_a_session_through_mcp_passes_on_its_hold`: a session runs a listed `gh`, so it
      holds external text. Its next `proc.run` waits, and the operator approves. The job's `curl` does `initialize`
      plus `conversation_open`. The new session holds via `job` from the reader session.
  - `tests_m3/mcp_surface.rs` (a new child module; one `mod` line in tests_m3.rs):
    - an MCP session's authority, floor, and limit; its write, open by config, waits; an answer from MCP is REFUSED
      with `approval.refused via mcp` and nothing moves; the CLI's answer counts and the write runs;
    - methods outside the allowlist, and turns into operator sessions, are refused;
    - the operator's own session has no floor.
  - Unit tests:
    - `mcp_server::tests::the_floor_holds_acts_and_leaves_reads`
    - `mcp::trace::tests::a_jobs_process_is_found_by_its_socket_and_names_its_session` (real children: direct, and
      stripped with `env -i`)
    - `theseus-mcp` `a_peer_the_admit_check_refuses_is_turned_away_first`
    - `render::mcp_server::tests::the_line_says_where_it_listens_and_what_it_refused`
    - the approval unit test's new Mcp case
- **Planted reverts.** Each was restored with `cp` and `touch`, and `git status` was clean after each.
  1. Remove `Surface::Mcp`'s arm in `Answerer::unknown` (accept its approvals): `an_mcp_sessions_act_waits_…` and
     `an_answerer_names_its_place_…` fail.
  2. Drop the Origin check (`if false && …`): `theseusd::mcp_server a_wrong_key_and_a_foreign_origin_are_refused`
     fails (`HTTP/1.1 200 OK` to the foreign page), and so does `theseus-mcp::server a_foreign_origin_is_refused`.
  3. Drop the floor at the gate: the core test fails ("the write waits: … awaiting_confirm: null, output: Written.")
     and so does the daemon's `an_acting_call_waits_…`.
  4. Return no job session from the trace: `a_jobs_process_…passes_on_its_hold` fails ("the MCP session holds
     nothing").
- **Under load:** the 9 MCP tests (4 daemon, 3 core, trace, admit) at `nice -n 19` beside four busy loops at nice 0,
  5 times: 45 of 45 passed (8.8 to 10.1 s a run).
- **Lifecycle bench, shape only** (debug, 3 runs, on this VM, with the server enabled in the bench config):
  `LIFECYCLE OK`. Cold start p95 12.7 ms, clean shutdown 7.7, SIGKILL restart 16.7, swap 15.6. The owner measures
  the real numbers.

### Live check (the maintainer's)

On a scratch daemon, never the operator's own. `$D` is a scratch dir, and the build is this branch's.

```bash
D=$(mktemp -d); mkdir -p $D/bin $D/projects
head -c 24 /dev/urandom | base64 > $D/mcp.key; chmod 600 $D/mcp.key
theseusd example-config --plain > $D/theseus.toml
# Edit $D/theseus.toml:
#  [secrets] add: mcp_server_key = "file:<D>/mcp.key"   (once the env:/file: sources land; until then, an op:// item
#            or a fake `op` in $D/bin as in crates/theseusd/tests/mcp_server.rs)
#  [mcp_server] enabled = true, port = 7444
#  [web] port = 7445 (or enabled = false); [discord] enabled = false; [index] enabled = false
PATH=$D/bin:$PATH theseusd --config $D/theseus.toml --state-dir $D/state --socket $D/sock &
theseus --socket $D/sock health | grep 'mcp server'
#   -> "mcp server: listening on 127.0.0.1:7444 · 0 session(s) · 0 opened · 0 call(s), 0 an error"
claude mcp add --transport http theseus-scratch http://127.0.0.1:7444/mcp --header "Authorization: Bearer $(cat $D/mcp.key)"
#   In Claude Code: "use theseus-scratch: open a conversation and ask it what 2+2 is"
#   -> conversation_open then conversation_send, and the reply comes back.
theseus --socket $D/sock sessions        # a session labelled "mcp claude-code"
#   In Claude Code: "ask the theseus conversation to create the file /tmp/x with proc_run"
#   -> conversation_send answers {status: "running", turn_id} after its wait (or blocks up to 60 s)
theseus --socket $D/sock confirm          # the call waits; its reason names "an MCP client opened this session"
theseus --socket $D/sock confirm --approve <act_id>
#   -> conversation_status then shows the reply; the file exists.
curl -s -o /dev/null -w '%{http_code}\n' -X POST http://127.0.0.1:7444/mcp -H 'Authorization: Bearer wrong' -d '{}'   # 401
curl -s -o /dev/null -w '%{http_code}\n' -X POST http://127.0.0.1:7444/mcp -H "Authorization: Bearer $(cat $D/mcp.key)" -H 'Origin: http://example.com' -d '{}'   # 403
theseus --socket $D/sock ledger --kind mcp_server.call     # one row per call
theseus --socket $D/sock shutdown
```

Expect `theseus health` to count the refusals (key 1, origin 1). Check the lifecycle bench numbers on the owner's
machine: the bench config now runs with the server on.

### Left, uncertain, and for the owner

- **One shared connection.** All MCP calls share one in-process protocol connection. `session.wait` allows 64 parked
  waits per connection, so more than 64 concurrent `conversation_send`s past a parked turn would get `LIMIT`. The
  rate bound (60 per minute) makes that unlikely. A connection per MCP session would remove the limit.
- **Reply after approval.** `conversation_send`'s reply after an approval is the session's last assistant text, and
  its `cost_usd` is absent (the continuation's cost isn't in hand there).
- **The hold trace is a light guard,** like the CLI's. It is per call, scanning `/proc` for the daemon's descendants
  plus their fds, on the blocking pool. A job can defeat it by double-forking to the daemon after stripping its
  environment, or by handing its socket to an outside process.
- **The key is not harness-only.** It is not in the broker's harness-only list (`broker::harness_only`). A job can
  only get it through an explicit `[broker.programs]` grant, but listing it as harness-only would make the line say
  so. I left `broker.rs` alone (another area, and its line test would change).
- **Telemetry:** only the existing tracing span `mcp_server.call`. There is no telemetry fact or metric beyond the
  ledger row and health counters. Say whether one should land.
- **Merge points:**
  - `theseus-mcp/Cargo.toml` loses `reserved_for` (the 36b lane edits the same lines).
  - `crates/theseus-core/src/config.rs` gains one field and two lines.
  - The template gains a section. Another change is reworking it.
  - Protocol TypeScript was regenerated into `web/src/protocol.gen` (moving to `cockpit/`).
  - `theseus-sim` lifecycle `bench_config` (the benchmarks lane).
  - `theseus-discord` `rpc_client` is now `pub`.
- **Fixture trap found:** the daemon tests' stand-in model numbers tool-use ids from `toolu_test_0` in every response.
  A second turn's call can collide with a first turn's answered call, and then the continuation finds "nothing_new"
  after an approval. My test works around it (a read first). Worth a note in `crates/theseusd/AGENTS.md`'s traps, or
  a fix in `tests/common/model.rs`.

### Docs to write at review (not edited here)

- Part III: the item for 41b.
- `docs/status.md`: the row and the landed step.
- `docs/design/README.md`: remove `theseus-mcp` from the reserved list. Part III's reserved list too.
- `docs/design/m7-surface.md` §2.5: the peer trace is now the descendant/inode trace. The place class is private.
  The principal is carried in `Authority`, so no format bump.
- `crates/theseusd/AGENTS.md`: `src/mcp.rs` under "What's here"; the stand-in model's id trap.
- **What the cockpit should show** (the Observatory's "server panel"): health's `mcp_server` block (state, port,
  sessions with clients, opened and last client, calls and errors, refusals by kind, a failed state's error);
  sessions labelled `mcp …` marked as MCP-opened with their floor; and the `mcp_server.call` and
  `mcp_server.refused` ledger rows in its ledger view.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` passed fmt, shape, clippy, both builds, the reader rule, and the suite
except these:

- **`theseus-sandbox::contract` (all clauses), `theseus-sandbox::bench spawn_100`, and 14 of `theseusd::sandbox`:**
  the known root-VM failure (theseus-pv6i). `clause_01_namespaces` fails the same way on unmodified `main` (checked
  with `git stash`).
- **`theseus-core tests_output::the_cores_output_matches_its_golden`:** the golden holds a wake time's UTC offset as
  `-#:#`. This VM runs in UTC, which prints `+00:00`. It passes with `TZ=America/Los_Angeles`, so it is environmental
  and not this change's. The golden could pin its TZ.

I then ran the phases after the suite by hand, and all passed:

- protocol types: the regenerated TypeScript is committed;
- `theseus-sim bench turn --check --runs 5 --burst 0`: 5 frames, budget 5, ok;
- `cargo deny --offline check`: ok (the advisory database was fetched at setup);
- web lint and build, cockpit lint, test, and build: ok;
- web dist: unchanged.

Also: the disk allowance filled once (17 GB of `target/debug/incremental`). I deleted that cache and rebuilt with
`CARGO_INCREMENTAL=0`.
