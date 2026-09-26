# theseus

My first agent harness, be gentle.

Design document: [`docs/the-ship-of-theseus.md`](docs/the-ship-of-theseus.md) (Part I specification, Part II build plan).

## What exists (M0 First light)

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
corruption elsewhere; deleting the index loses nothing.

```bash
theseus-sim crash-test --iterations 40 --restarts 3 --engine redb   # kill -9, tear the tail, verify
theseus-sim bench --engine redb --records 20000                      # append/read throughput
theseus-sim bench --engine fjall --records 50000 --no-fsync           # index cost without the disk
```

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
