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

## Build

```bash
cargo build                                           # dev
cargo nextest run && cargo clippy --all-targets -- -D warnings && cargo deny check
cargo build --release --target x86_64-unknown-linux-musl   # static binaries
scripts/smoke.sh                                      # end to end against the real API
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or https://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or https://opensource.org/licenses/MIT)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
