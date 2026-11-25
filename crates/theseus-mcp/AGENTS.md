# theseus-mcp

The Model Context Protocol, written by hand (M7 §2.1): the protocol alone, over serde, tokio, and reqwest, with no
core. Read by theseus-core (its MCP board, 36b), with the default features since the five shipped binaries build every
feature the workspace builds (the gate's `features` phase), and theseus-sim (`fake-mcp`, `default-features = false`);
the `server` feature is Theseus's own MCP server, whose reader is row 72 (41b).

Key modules: `client.rs`, `fake.rs`, `names.rs`, `oauth/`, `types.rs`, `server.rs`. Read by: core, sim.

## What's here

- `client.rs`: one connection to one server, over stdio, pipes, or streamable HTTP: the handshake and its
  revision, every page of a list, calls with a timeout, cancel on drop (`notifications/cancelled`), and a stream
  of the server's notifications (`Event`). A stdio server is in its own process group; a stop sends SIGTERM to it
  and never waits. `Options::serve` (theseus-pw1q.14) is the requests of the server's own one connection answers
  beyond `ping` (`Serve`: the core's hands broker for an extension acked with `hands`), at most `SERVED_AT_ONCE`
  at a time, declared under `capabilities.experimental.theseus` at `initialize`; with none, every other request is
  "method not found", as before.
- `fake.rs` and `bin/theseus-mcp-fake.rs`: a fake server (modes `ok`, `slow`, `crash-after N`, `change-tools`,
  `error`) over pipes, stdio, or HTTP. `theseus-sim fake-mcp` serves the same one.
- `oauth/`: signing in to a remote server (issue 55): `discover.rs` (the 401's challenge, the resource's and the
  authorization server's metadata), `register.rs` (RFC 7591), `request.rs` (PKCE, `state`, the address, the
  loopback listener), `exchange.rs` (the code, the refresh, the revoke), `fake.rs` (a fake authorization server,
  which the fake MCP server's `oauth` asks). An `HttpTarget` with a `token` (`TokenSource`) gets its token before
  each request, renews it once on a 401, follows no redirect, and keeps no body of a 401 but its standard code.
- `names.rs`: a tool's canonical name (`mcp:<server>/<tool>`) and its wire name (`mcp__<server>__<tool>`, at most
  64 characters, a digest suffix when changed or cut, unique on a board).

## Invariants

- **No core.** What a core needs on top (the board, the `Tool` contract, the gate, the store) is the core's.
- **No sign-in value in words.** No error or log line of `oauth/` holds a token, a code, a verifier, a client
  secret or a response's body: a refusal is its status and a standard OAuth code (`oauth::CODES`).
- **A call's outcome is unknown** after a timeout or a lost connection (`Error::outcome_unknown`), never "not sent".

## Tests

- `tests/client.rs` (the client against the fake: handshake, pages, calls, crashes, cancel, SSE, sessions),
  `tests/oauth.rs` (a sign-in against the fake authorization server, through the real client) and
  `tests/server.rs` (feature `server`).
