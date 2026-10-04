# theseus-mcp

The Model Context Protocol, written by hand (M7 §2.1): the protocol alone, over serde, tokio, and reqwest, with no
core. Read by theseus-core (its MCP board, 36b) and theseus-sim (`fake-mcp`), both with `default-features = false`;
the `server` feature is Theseus's own MCP server, whose reader is row 72 (41b).

Key modules: `client.rs`, `fake.rs`, `names.rs`, `types.rs`, `server.rs`. Read by: core, sim.

## What's here

- `client.rs`: one connection to one server, over stdio, pipes, or streamable HTTP: the handshake and its
  revision, every page of a list, calls with a timeout, cancel on drop (`notifications/cancelled`), and a stream
  of the server's notifications (`Event`). A stdio server is in its own process group; a stop sends SIGTERM to it
  and never waits.
- `fake.rs` and `bin/theseus-mcp-fake.rs`: a fake server (modes `ok`, `slow`, `crash-after N`, `change-tools`,
  `error`) over pipes, stdio, or HTTP. `theseus-sim fake-mcp` serves the same one.
- `names.rs`: a tool's canonical name (`mcp:<server>/<tool>`) and its wire name (`mcp__<server>__<tool>`, at most
  64 characters, a digest suffix when changed or cut, unique on a board).

## Invariants

- **No core.** What a core needs on top (the board, the `Tool` contract, the gate, the store) is the core's.
- **A call's outcome is unknown** after a timeout or a lost connection (`Error::outcome_unknown`), never "not sent".

## Tests

- `tests/client.rs` (the client against the fake: handshake, pages, calls, crashes, cancel, SSE, sessions) and
  `tests/server.rs` (feature `server`).
