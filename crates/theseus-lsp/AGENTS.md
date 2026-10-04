# theseus-lsp

The Language Server Protocol, written by hand: a client for one language server over its stdin and stdout, with no
core. Read by theseus-core's board (`crates/theseus-core/src/lsp/`, L2, theseus-n88g.8): the servers it starts, the
`lsp.*` tools, and the gate's step for a start. L3 hooks its diagnostics into `fs.write`, `fs.edit`, and `fs.patch`.

## What is here

- `src/framing.rs`: `Content-Length` messages over any tokio stream, whatever the read sizes.
- `src/jsonrpc.rs`: messages classified by their fields; LSP's error codes.
- `src/client.rs`: `Client`, `Server` (what the caller hands over), `Options`, `Error`, `Event`, readiness, the
  server's own requests, the stop.
- `src/docs.rs`: document sync (full text, a version per document), the resync from disk before each request,
  `file_changed`.
- `src/diagnostics.rs`: pushed and pulled diagnostics, and `Client::diagnostics`, the bounded wait L3 calls.
- `src/nav.rs`: definition, references, hover, document and workspace symbols, prepare-rename and rename.
- `src/position.rs`: `locate`, the tools' addressing (a 1-based line and a symbol's text, made a UTF-16 position).
- `src/types.rs`: the hand-written types; `src/uri.rs`: file URIs.
- `src/servers.rs`: the presets for ty, pyright, basedpyright, TypeScript 7, typescript-language-server, and
  rust-analyzer, with their quirks.
- `src/spawn.rs`: a plain spawn, for tests and the probe only.
- `src/fake.rs`, `src/bin/theseus-lsp-fake.rs`: the scripted fake server.
- `fixtures/`: one small project per language for the live tests.

## Invariants

- **The client never spawns a server in production code.** The caller does (L2, through
  `theseus_kernel::children::spawn`, in a process group of its own) and hands over the pipes and a kill. `spawn.rs` is
  for tests and the probe.
- **An edit is never applied here.** `rename` returns the `WorkspaceEdit`; `workspace/applyEdit` from a server is
  answered `applied: false`.
- **A request never reads a document older than its file**: every request about a document syncs the open documents
  with the disk (modification time and size) first.
- **The stop always ends the server**: `shutdown`, `exit`, then the kill after `exit_grace` (1 s). The last clone
  dropped without a stop kills it at once.
- **A diagnostics answer says how fresh it is** (`Freshness`): pulled or pushed for the current version, or stale
  when the bound ran out.
- No new dependencies: `url` and `libc` were already in the lockfile.

## Tests

- Unit tests in each module (framing split and joined, the position helper on multibyte text, the types' shapes).
- `tests/fake.rs`: the client against the fake, in this process over pipes and as a process (the stop's kill, a
  crash, a dropped client).
- `tests/live.rs`: ignored; one per real server, with one command each (the file's header says which variables name
  the servers). `cargo nextest run -p theseus-lsp --run-ignored only --no-capture -E 'test(=live_ty)'`.

## Traps

- TypeScript 7's server (`tsc --lsp --stdio`) answers `shutdown` and ignores `exit`: only the kill ends it.
- ty and TypeScript 7 only pull diagnostics; pyright pushes; rust-analyzer pushes its own diagnostics on a change and
  `cargo check`'s after a save, later.
- typescript-language-server waits forever on a project without `node_modules/typescript` unless
  `initializationOptions.tsserver.path` is set (its preset takes the path).
- rust-analyzer needs rustup's `cargo` first on `PATH`, or it never loads the workspace; its readiness is
  `experimental/serverStatus`'s `quiescent`, which the preset waits for.
