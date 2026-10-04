# Cloud report: the LSP client, `theseus-lsp` (lane L1, theseus-n88g.7)

Branch `cloud/20261004-lsp-client`, from `main` at d9b0931. Started 02:30 UTC, report written 04:00 UTC.

| Commit | What |
|---|---|
| ae2e4ca | `lsp: a hand-written LSP client crate, theseus-lsp, with a scripted fake server` |
| 3775e4d | `lsp: a cut-short stream reads as the server's end, and rust-analyzer gets a 5 s exit grace` |

No change outside the new crate except the workspace's `members` line and `Cargo.lock`, which gains only the new
crate's own entry: `url` and `libc` were already in the lockfile. No new packages, no `deny.toml` change.

## Step 1: the crate (ae2e4ca)

### What I built

`crates/theseus-lsp`, modelled on theseus-mcp (a hand-written JSON-RPC client with a scripted fake):

- `framing.rs`: `Content-Length` messages over any tokio stream; unknown headers skipped, a non-UTF-8 charset, a
  missing or bad length, and an oversized body refused.
- `client.rs`: `Client::start(Server, Options)`. **The caller spawns** and hands over `Server { reader, writer, kill,
  pid }`. `kill` is a closure that kills the process group; L2 builds it around `children::spawn`. Requests are
  routed by integer id, each with a timeout. A timed-out or dropped request sends `$/cancelRequest`; `initialize`
  and `shutdown` are never cancelled. The server's requests get answers: `workspace/configuration` comes from
  `Options::settings` by dotted section. `client/registerCapability` is recorded, and a `textDocument/diagnostic`
  registration turns pull on. `window/workDoneProgress/create`, `workspaceFolders`, and the `*/refresh` requests are
  answered (a diagnostic refresh drops the pulled cache). `workspace/applyEdit` gets `applied: false`, and anything
  else gets method-not-found. Readiness comes from `$/progress` begin/end and rust-analyzer's
  `experimental/serverStatus` (`quiescent`); `wait_ready(bound)`. Events (`Event`) go on an unbounded channel the
  caller may drop.
- **The stop** (`Client::stop`): `shutdown` (2 s bound), `exit`, then wait for the server's stdout to close for
  `exit_grace` (1 s), else kill and wait 1 s more. It returns `Stopped { shutdown_answered, exited, killed, took }`.
  The last clone dropped without a stop sends `exit` and kills at once.
- `docs.rs`: full-text `didOpen`, `didChange` (a version per URI), `didSave` (with the text when the server asks),
  `didClose`, and `workspace/didChangeWatchedFiles`. `sync_disk` resends every open document whose file's mtime or
  size changed (and closes a deleted one), under a lock so versions go out in order. It runs before every request
  about a document. `file_changed(path)` is L3's call after a write: it resyncs and saves an open document and tells
  the watcher (created, changed, or deleted).
- `diagnostics.rs`: pushed lists are kept per URI with their version and the client's send counter at arrival;
  pulled ones with their `resultId`, and an `unchanged` answer reuses the list. **`Client::diagnostics(path,
  bound)`** opens and syncs the document, waits for readiness on a status-reporting server, then pulls or waits for
  a push. A pull retries `ServerCancelled` and `ContentModified`. A push counts when its version is the document's,
  or, when it has no version, when it arrived after the change was sent. The answer says how it was had:
  `Freshness::{Pulled, Pushed, Stale}`. Stale means the bound ran out, and the list is the last known one.
- `nav.rs`: `definition` (Location, Location[], or LocationLink[], made one `Vec<Location>`; a link gives its
  selection range), `references`, `hover` (`Hover::text()` flattens every contents shape), `document_symbols`
  (nested or flat), `workspace_symbols` (WorkspaceSymbol's range-less location read), `prepare_rename`, and `rename`
  (returns the `WorkspaceEdit`, with `documentChanges` and resource operations read, never applied). A request the
  server did not declare is refused before sending (`Error::NotOffered`).
- `position.rs`: `locate(text, line, symbol, occurrence)` gives an LSP `Position` with a UTF-16 column. It counts
  only whole-word matches when the symbol is word-shaped and the line has one, and every match otherwise. A symbol
  on its line twice with no occurrence is refused as `Ambiguous { count }`, never guessed. It reads lines ended by
  `\n`, `\r\n`, or `\r`. `line_text`, `utf16_column`, and `byte_column` render answers back.
- `types.rs`: the subset by hand with serde. It is about 450 lines plus 120 of tests, against "a few hundred": over
  that, because of `WorkspaceEdit`'s `documentChanges` and resource operations and the two symbol shapes. `lsp-types`
  would still be two new packages; I don't think it's worth it yet.
- `servers.rs`: presets for ty, pyright, basedpyright, TypeScript 7 (`tsgo`), typescript-language-server (takes the
  `tsserver.js` path), and rust-analyzer. Each has its command, languages, init options, settings, whether it
  reports a status, and its exit grace.
- `spawn.rs`: a plain `tokio::process` spawn in its own group, with a kill that does nothing once the child is
  reaped. For tests and the probe only. `group_rss_kib(pgid)` gives the memory column.
- `fake.rs` and `theseus-lsp-fake`: push (with or without versions, optionally delayed), pull, pull registered
  200 ms after `initialized`, slow answers (cancellable), a crash after N requests (exit 3), and a server that
  ignores `exit`. It can also send its own requests (configuration, registration, progress create, applyEdit, an
  unknown one) and begin a progress it never ends. A custom request, `fake/seen`, reports what it saw.
- `AGENTS.md` and `CLAUDE.md` for the crate.
- **The reader rule.** The prompt's marker, `"L2 (theseus-n88g.8): …"`, fails `tests_registry`'s required form
  (`row <n> (<step>), <milestone>: <reader>`). I wrote `reserved_for = "row 0 (L2, theseus-n88g.8), M7: the LSP board
  and tools in theseus-core"`. **`row 0` is a placeholder**: the roadmap has no row for the LSP lanes. Please give it
  its row number (and correct the milestone if M7 is wrong) at the merge.

### How I proved it

- **Unit tests** (16): framing split byte by byte, two messages in one read, bad headers, a cut body, write and read
  back; classification of JSON-RPC messages; the types' shapes; `locate` on `é` (2 bytes, 1 unit) and `𝔁` (4 bytes,
  2 units), whole words, occurrences, `\r\n`/`\r`, and every error; URIs round-trip.
- **Fake-server tests** (`tests/fake.rs`, 14):
  - routing of 6 interleaved slow hovers;
  - each server request answered (configuration by section, missing, and whole; registration; progress create;
    applyEdit refused; unknown → -32601), plus a running progress blocking readiness;
  - push with versions, with a fix made on disk picked up before the next call (versions 1, 2 seen by the server);
  - push without versions waiting for the post-change list;
  - pull-only (declared) and pull registered late, with `unchanged` reusing the list and exactly two pulls sent;
  - the bounded wait returning `Stale` in under 1 s, then `Pushed`;
  - a dropped request and a timed-out one, each sending one `$/cancelRequest` that the fake receives;
  - navigation across files, and a rename's edit with versions that leaves the files untouched;
  - `file_changed` for a changed, new, and deleted file;
  - a change on disk sent before the next request;
  - a clean stop with no kill (process exit 0);
  - an ignores-exit server killed after the 1 s grace (SIGKILL);
  - a crash failing the waiting request, a `Closed` event, and exit status 3;
  - a dropped client killing its server.
  - `cargo nextest run -p theseus-lsp`: **30 passed**, plus 40 back-to-back runs clean after step 2.
- **Under load** (4 busy loops at nice 0, the suite at nice 19, 10 runs each):
  - The first pass gave **9 of 10**. The failure was a timing assumption in the cancel test (a 100 ms drop could
    land before the request was sent). I fixed it to drop only once the request is in flight.
  - After that, **10 of 10** on ae2e4ca's tests, and **10 of 10** again on 3775e4d.
- **Planted reverts**:
  - Pull path dropped (`diagnostics` always waits for a push): `a_pull_only_server_is_pulled_and_unchanged_reuses_the_list`
    failed, `left: (Stale, 0) right: (Pulled, 2)`. Restored, touched, `git status` clean but for the work.
  - Stop's kill dropped (the `s.kill()` in `stop`): `a_server_that_ignores_exit_is_killed_after_the_grace` failed,
    `the server is gone: Elapsed(())`. Restored, touched, 30/30 again. (The dropped-client test still passed: `Drop`
    kills on its own path.)
- **Live, here, against the real servers** (`tests/live.rs`, all `#[ignore]`). Every server passed initialize, the
  planted error's diagnostic, a cross-file definition, a hover, references, symbols, a rename's edit across files
  (not applied), and a clean stop. Final pass, on 3775e4d:

| Server | Version | Spawn → `initialize` | Open → planted error | Definition | Memory (group RSS) | Diagnostics | Stop |
|---|---|---|---|---|---|---|---|
| ty | 0.0.84 | 7 ms | 37 ms | 1 ms | 35 MiB | pulled (registered late) | exited, 9 ms |
| pyright | 1.1.414 | 197 ms | 461 ms | 24 ms | 131 MiB | pulled | exited, 18 ms |
| basedpyright | 1.40.1 (pyright 1.1.414) | 357 ms | 487 ms | 3 ms | 167 MiB | pulled | exited, 32 ms |
| TypeScript 7 (`tsc --lsp --stdio`) | 7.0.2 | 65 ms | 87 ms | 3 ms | 47 MiB | pulled | **exited**, 5 ms |
| typescript-language-server | 5.3.0 over TypeScript 5.9.3 | 131 ms | 1,031 ms | 10 ms | 452 MiB | pushed | exited, 11 ms |
| rust-analyzer | 1.98.1 (48a229c 2026-09-01) | 52 ms | 3,605 ms | 1 ms | 602 MiB | pulled, after quiescence | exited, 388 ms |

  The JavaScript file under `checkJs` and `// @ts-check` got its error from both TypeScript servers (72 ms and
  1.1 s). The version column is the server's `serverInfo`; pyright and typescript-language-server send none, so
  theirs come from their packages. One 4-core VM, debug build of the client.

- **rust-analyzer on this repository's workspace** (`live_rust_analyzer_on_a_workspace`): `initialize` in 54 ms,
  **quiescent after 18–21 s** (two runs), **4.2 GB** resident. Its stop is the finding below.

### What I found

- **ty registers pull diagnostics dynamically after `initialized`** and declares nothing at `initialize`. The first
  live run waited out its 10 s bound on a push that never came. Fixed in ae2e4ca: a push wait that sees the
  registration switches to pulling. The fake's `--pull-registered` registers 200 ms late to hold it.
- **pyright, basedpyright, and rust-analyzer pull as well** once the client declares
  `textDocument.diagnostic`. Only typescript-language-server pushed here.
- **TypeScript 7.0.2 exits on `exit`.** The probe found it ignoring `exit`; that may have been an earlier build. The
  kill stays, and the fake's `--ignore-exit` holds it.
- **TypeScript renames an imported name where it was imported** (`import { total as sum_all }`) when asked at the
  import, as an editor does. A rename from the definition edits every file. L2's rename tool should say this, or
  locate the definition first.
- **rust-analyzer's diagnostics** came from its own analysis (pulled, "expected i32, found &'static str"), after
  quiescence, without `cargo check`. The first call's 3.6 s is mostly the wait for quiescence.

### The live check for the maintainer

The servers must be installed (as here: `rustup component add rust-analyzer` in the tree, so the pinned toolchain
has it; ty and basedpyright from pip; pyright, `typescript@7`, and typescript-language-server with `typescript@5`
from npm). Name them by environment variable, or put them on `PATH`:

```bash
export THESEUS_LSP_TY=<venv>/bin/ty
export THESEUS_LSP_PYRIGHT=<npm>/node_modules/.bin/pyright-langserver
export THESEUS_LSP_BASEDPYRIGHT=<venv>/bin/basedpyright-langserver
export THESEUS_LSP_TSGO=<ts7>/node_modules/.bin/tsc
export THESEUS_LSP_TYPESCRIPT_LANGUAGE_SERVER=<npm>/node_modules/.bin/typescript-language-server
export THESEUS_LSP_TSSERVER=<npm>/node_modules/typescript/lib/tsserver.js
for t in live_ty live_pyright live_basedpyright live_tsgo live_typescript_language_server live_rust_analyzer; do
  cargo nextest run -p theseus-lsp --run-ignored only --no-capture -E "test(=$t)"
done
THESEUS_LSP_WORKSPACE=$PWD cargo nextest run -p theseus-lsp --run-ignored only --no-capture \
  -E 'test(=live_rust_analyzer_on_a_workspace)'
```

Each prints its steps and a table row like the one above, and passes. The workspace run prints `quiescent after …`
and its memory, then `exited: true` within the 10 s grace that test uses. Use the exact filter (`-E 'test(=…)'`): a
bare `live_ty` also matches `live_typescript_language_server`.

## Step 2: findings after the first commit (3775e4d)

- **A crash mid-message.** The fake's `exit(3)` can cut a message it is writing (once in about 10 runs). The client
  closed correctly but said "reading the server's stdout failed: early eof", and the stop took the stdout for still
  open. Now a stream that ends inside a message (`FrameError::is_cut_short`) counts as the server closing its stdout,
  "in the middle of a message". A framing test holds both kinds of cut, and the crash test accepts either true
  reason. 40 back-to-back runs were clean, then 10 of 10 under load.
- **rust-analyzer's exit.** With this workspace loaded it took 2.6 s from `shutdown` to exiting, so the default 1 s
  grace killed it (`exited: false, killed: true, took: 1.35 s`). Presets now carry `exit_grace`: 1 s for the others,
  5 s for rust-analyzer. The workspace test uses 10 s and showed `exited: true, killed: false, took: 2.6 s`.

## What is left, and choices for the owner

- **`row 0`** in `reserved_for` needs its real row (above).
- **The fixture's `Cargo.toml`** under `crates/theseus-lsp/fixtures/rust/` declares an empty `[workspace]` so cargo
  treats it as a project of its own. Its `src/lib.rs` fails to compile on purpose. Nothing in the gate reads it (fmt,
  clippy, and the shape check passed); flag it if a future check globs every `.rs` file.
- **Push without versions** counts any list that arrived after the change, so a list computed for the previous text
  but sent just after the change would pass for current. No server here sent unversioned lists; L3 could prefer
  pull wherever it is offered (it already does).
- **rust-analyzer and `cargo check`.** The client never relies on flycheck, whose results come later and unversioned
  after a save. For L3, its own analysis (pulled) is the fast path.
- **rust-analyzer's memory** (4.2 GB on this repository, 600 MB on a toy crate) argues for L2 starting it lazily, one
  per workspace, and stopping it when idle.
- **Events** are an unbounded channel, as in theseus-mcp. L2 should drain it or drop it; rust-analyzer and pyright
  log a lot.
- **Docs.** I edited none. Suggested at review:
  - `docs/status.md`: the LSP client as landed, with the server table.
  - Part III: an item for L1.
  - AGENTS.md's map: a `theseus-lsp` line beside theseus-mcp's ("The Language Server Protocol by hand: a client over a
    server's stdio, the tools' position helper, and a scripted fake").

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit. Every compile phase passed: fmt, shape, clippy with
`-D warnings`, bench build, test build. The reader rule passed (9/9), so the marker's form is accepted. The suite
ran **1781 tests: 1748 passed, 33 failed, 17 skipped**, the same 33 both times, none in this crate (theseus-lsp:
30/30):

- **32 sandbox tests**, the known root-VM case (theseus-pv6i): 19 in `theseus-sandbox::contract`, 1 in
  `theseus-sandbox::bench`, and 12 in `theseusd::sandbox`. Each says "the daemon runs as root, and Linux exempts root
  from RLIMIT_NPROC".
- **`theseus-core tests_output::the_cores_output_matches_its_golden`**, not on the known list. The golden holds a
  `wake.at` preview whose time carries a negative UTC offset (`-#:#`); this VM runs in UTC and prints `+#:#`. It
  passes with `TZ=America/Los_Angeles`. The golden depends on the machine's timezone, so it will fail on any UTC
  machine (CI included). The wakes lane is the likely owner.

The phases after the suite, run by hand, all passed on both commits: protocol types unchanged, the turn bench (5
frames at the p95, budget 5), `cargo deny --offline check` (the database was fetched here: advisories, bans,
licences, and sources ok), the web app's lint and build, the cockpit's lint, test, and build, and the Observatory's
dist unchanged.
