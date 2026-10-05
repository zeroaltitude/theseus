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
- `src/diagnostics.rs`: pushed and pulled diagnostics, and `Client::diagnostics`, the bounded wait L3 calls, with the
  wait on a server's check after a save (`Options::check_token`).
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
- **The client watches no files for a server, and never offers to** (`didChangeWatchedFiles` without dynamic
  registration, theseus-m9hj): offered, rust-analyzer stops watching for itself and misses what a job writes in
  every file the client has not opened. The client still announces its own writes (`file_changed`).
- **An edit is never applied here.** `rename` returns the `WorkspaceEdit`; `workspace/applyEdit` from a server is
  answered `applied: false`.
- **A request never reads a document older than its file**: every request about a document syncs the open documents
  with the disk (modification time and size) first.
- **The stop always ends the server**: `shutdown`, `exit`, then the kill after `exit_grace` (1 s). The last clone
  dropped without a stop kills it at once.
- **A diagnostics answer says how fresh it is** (`Freshness`): pulled or pushed for the current version, or stale
  when the bound ran out.
- **A saved document waits for its server's check** (theseus-c6hv). A server with a check token (`Options::check_token`,
  set by rust-analyzer's preset: `rust-analyzer/flycheck/`) runs a check after a save and pushes its errors. For such
  a server `file_changed` opens and saves a file it has not opened, and `diagnostics` of a document whose current
  version was saved waits, within its own bound, until every check begun since the last save (of any document) has
  ended, then adds the list pushed for this version to the pulled one, each item once. A check is waited for only if
  it begins within `CHECK_GRACE` (1 s) of the save, or of the server's readiness if later (a loading server checks
  only once loaded). A check past the bound is `Stale`. A server without a check token, and an unsaved document, wait
  for no check.
- No new dependencies: `url` and `libc` were already in the lockfile.

## Tests

- Unit tests in each module (framing split and joined, the position helper on multibyte text, the types' shapes).
- `tests/fake.rs`: the client against the fake, in this process over pipes and as a process (the stop's kill, a
  crash, a dropped client). The waits on a check run against the fake's `PullAndCheck` (its pull answers `ERROR`
  lines; its check, begun 50 ms after a save, pushes `ERROR` and `CHECK` lines) on tokio's paused clock, so each
  asserts its wait exactly.
- `tests/live.rs`: ignored; one per real server, with one command each (the file's header says which variables name
  the servers). `cargo nextest run -p theseus-lsp --run-ignored only --no-capture -E 'test(=live_ty)'`.
  `live_rust_analyzer_reports_rustcs_errors_after_an_edit` replays theseus-c6hv's two edits as L3 makes them, each
  needing its error from rustc.

## Traps

- TypeScript 7's server (`tsc --lsp --stdio`) answers `shutdown` and ignores `exit`: only the kill ends it.
- ty and TypeScript 7 only pull diagnostics; pyright pushes. rust-analyzer, offered the pull, answers it with its own
  analysis alone (by default that misses rustc's E0277 and E0425), and pushes `cargo check`'s errors, labelled with
  the document's version at the push, after a save, and once after its workspace loads. Hence its check token
  (above). Its check begins 60 to 80 ms after the save (a 50 ms debounce); a save during a check cancels it at once.
  Don't turn on its experimental diagnostics to get rustc's errors by pull: false positives are their known cost.
- A server may write a check's progress end before that check's push, in one turn of its loop. So after a check
  ends, a pull is asked again unless the first was sent after the end arrived: its answer comes after the push.
- typescript-language-server waits forever on a project without `node_modules/typescript` unless
  `initializationOptions.tsserver.path` is set (its preset takes the path).
- rust-analyzer needs rustup's `cargo` first on `PATH`, or it never loads the workspace; its readiness is
  `experimental/serverStatus`'s `quiescent`, which the preset waits for.
- Under `cargo nextest`, rustup's `rust-analyzer` proxy reads the inherited `RUSTUP_TOOLCHAIN`, the pinned toolchain,
  which has no rust-analyzer: the live tests' server exits at once ("the server closed its stdout"). Set
  `THESEUS_LSP_RUST_ANALYZER` to a toolchain's own binary (`~/.rustup/toolchains/<toolchain>/bin/rust-analyzer`).
