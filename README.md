# theseus

My first agent harness, be gentle.

Design document: [`docs/the-ship-of-theseus.md`](docs/the-ship-of-theseus.md) (Part I specification, Part II build plan).

## What exists (M0 First light, then M1 Keel and M2 Kernel below)

Two static binaries and one protocol:

- **`theseusd`** — the server. Loads config, resolves every secret from 1Password through a
  service account (or refuses to start), opens the embedded store, and speaks the protocol on a
  Unix socket (daemon) or on stdin/stdout (spawned by a client).
- **`theseus`** — the CLI. A thin client that links only the protocol crate. Prompt from an
  argument or stdin, streamed reply on stdout, diagnostics on stderr, `--json` for machines.
- **The protocol** — JSON-RPC 2.0, one JSON object per line. Types in `crates/theseus-protocol`.
- **The web UI** — `http://127.0.0.1:7433/`, served from the binary (Vite + React, source in `web/`).
  The browser is a protocol client over a WebSocket; it shows prompts, streamed replies, tokens
  in/out per exchange and per session, timing, and the event stream behind each turn.

A turn today is exactly one loop: the user's prompt goes to the Anthropic Messages API with no
other context and no tools, the reply streams back, and the Advancer's only policy
(`stop_after_one_loop`) ends the turn. Every hook site is visited with zero handlers installed;
handlers can be registered over the protocol and observe.

## Run it

Only one secret may reach the process outside 1Password: the service-account token.

```bash
export OP_SERVICE_ACCOUNT_TOKEN=...           # or --op-token-file / THESEUS_OP_TOKEN_FILE
theseusd example-config > ~/.theseus/theseus.toml   # op:// references only, no values
export THESEUS_CONFIG=~/.theseus/theseus.toml       # default is the 1Password item theseus-config
theseusd check                                      # resolves every secret, then exits
theseusd &                                          # daemon on ~/.theseus/theseus.sock

theseus health
theseus hooks list
theseus ask "Say hello."                            # streamed
echo "Summarize: ..." | theseus ask --json          # pipelines
theseus --spawn ask "no daemon needed"              # spawns theseusd --stdio
theseus hooks watch turn.ended                      # observe a hook over the protocol
theseus shutdown
```

Exit codes: `0` ok, `1` server or provider error, `2` usage, `3` cannot connect.

Profiles and providers: a `[profiles.<name>]` entry is provider + model + max_tokens + system; exactly
one is **live** (`[model].live` at startup, `theseus profile use <name>` at runtime, persisted across
restarts, also a selector in the web UI). `theseus ask -P glm "…"` runs one turn under another
profile without switching. Any endpoint that speaks the Anthropic Messages API is a
`[providers.<name>]` entry (`api_base`, `api_key_secret`, optional `timeouts`); Z.ai's GLM models
are in the example config. Raw `-p provider -m model` overrides still exist for experiments.
Secret references may end in `#label` to select one `label: value` line of a multi-line note.

OpenTelemetry is built in and off-wire until you point it somewhere:

```toml
[telemetry]
otlp_endpoint = "http://127.0.0.1:4318"   # any OTLP/HTTP collector: Collector, Tempo, Honeycomb, Datadog, ADOT
headers_secret = "honeycomb_key"          # optional; a [secrets] entry holding "x-honeycomb-team: …"
hook_spans = false                        # true: every hook site is a span; false: an event on its parent
```

Each turn becomes one trace (turn > loops > provider.call with GenAI attributes, first_byte/first_token
events) with the exact timestamps the ledger recorded, plus metrics: `theseus.turns`, `theseus.tokens`,
`theseus.provider.errors`, `theseus.turn.duration_ms`, `theseus.provider.call.duration_ms`,
`theseus.provider.first_token_ms`.

Visibility: `theseus health` (totals), `theseus sessions list` (tokens per session),
`theseus ledger -n 20 [-k provider.call|provider.error|turn.ended|hook.site]` (every row).

When the Claude API does not answer: four timeouts (connect 10 s, first byte 60 s, stream idle 60 s,
total 600 s; `[model.timeouts]` in config) end the call with a classified error (`timeout`, `network`,
`rate_limited`, `overloaded`, `server`, `auth`, `invalid_request`, `stream`, `truncated`). The turn
fails, the class and whether usage is unknown are ledgered and returned in `error.data`, and nothing
retries on its own.

## The keel (M1)

Storage is a WAL of checksummed atomic frames (the truth) plus a rebuildable index in `redb`
(`[server].store_engine`, `fjall` also available). Every append is durable when it returns; a
frame with several records commits all or none; recovery truncates a torn tail and refuses
corruption elsewhere; deleting the index loses nothing. Concurrent appenders share one
`fdatasync` (**group commit**): sixteen writers get about seven times the frame throughput of one,
and a single writer pays exactly one sync per frame as before. Records carry an optional **scope**
(a session id) and the index keeps a per-scope position table, so a session's own records are one
range scan (§4.4b).

```bash
theseus-sim crash-test --iterations 40 --restarts 3 --engine redb   # kill -9, tear the tail, verify
theseus-sim bench --engine redb --records 20000                      # append/read throughput
theseus-sim bench --engine fjall --records 50000 --no-fsync           # index cost without the disk
theseus-sim bench --engine redb --records 20000 --writers 16          # group commit under concurrency
```

## The kernel (M2)

Every session has one durable **execution**; every turn is a kernel turn: the execution is woken
by input, admitted under a concurrency ceiling (`[kernel].admission_ceiling`), holds the
per-execution turn lock while it runs, and parks again when the Advancer ends the turn. The
provider call inside a turn is an **action**: `planned → authorized → dispatched` are three WAL
frames committed before the call is made, the budget reservation is taken in the first, and the
response settles it as a `Completion` in the same frame that continues the execution. Every
transition is a ledger row (`action.planned`, `action.dispatched`, `action.succeeded`,
`execution.running`, `execution.waiting`, …).

- **Completions** are one envelope from every source, accepted idempotently: a duplicate is a
  logged no-op, a stray (no matching action) is quarantined and surfaced in `theseus health`, a
  late one after cancel is recorded but revives nothing.
- **The spool** (`<state_dir>/spool`) is where the detached **job wrapper** (`theseusd job-wrapper`,
  its own session via `setsid`, own deadline) writes a result before any delivery attempt, then
  pokes the harness over `spool/notify.sock`. Startup drains it before accepting events; the
  heartbeat reconciler drains it every `heartbeat_secs`.
- **Startup is five idempotent steps** (store, load + requeue interrupted turns, drain spool,
  reconcile, accept); a crash inside any of them is finished by the next startup.
- **Budgets are hard limits** with reservations; unknown usage (an interrupted provider call) is
  held, never released; exhaustion is a terminal state.
- **`/cancel`** is a deterministic control path: `theseus executions cancel <id>` never queues
  behind admission, terminates the execution's wrapper processes by process group, and walks each
  action's cancel lifecycle (`requested → acknowledged → verified | unsupported | uncertain`).

```bash
theseus executions                     # one line per execution: state, turns, outstanding, budget
theseus executions cancel exe_…        # deterministic cancel
theseus health                         # kernel line: accepting, turns held/ceiling, counts by state
theseus ledger -n 20 -k action.succeeded
theseus-sim kernel-sim --seed 1 --seeds 40 --steps 400   # the M2 exit test: seeded fault injection
```

The kernel simulator runs the kernel under a virtual clock with a real store and spool in a temp
dir and injects crashes between any two frames and inside every startup step, lost and duplicate
completions, dropped notifies, jobs that never finish, and cancels; it checks the kernel
invariants after every step and is reproducible from its seed.

## Build

```bash
cargo build                                           # dev
cargo nextest run && cargo clippy --all-targets -- -D warnings && cargo deny check
(cd web && npm ci && npm run build)                   # web UI → crates/theseusd/web/dist (committed)
cargo build --release --target x86_64-unknown-linux-musl   # static binaries
scripts/smoke.sh                                      # end to end against the real API
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or https://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or https://opensource.org/licenses/MIT)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
