# theseus-core

The agent itself: config and secrets, sessions, the turn, the context compiler, tool calls and the gate, the protocol
server, the push, the outbox, and telemetry. Read by theseusd, theseus-discord, and theseus-sim.

## What's here

- **The turn**: `turn.rs` (the turn runner: one turn under a session's turn lock, its loops, its frames), `advancer.rs`
  (continue or end the turn), `harness.rs` (the harness loop, parked on its events and the heartbeat), and
  `rpc/driver.rs` (what that loop drives: the heartbeat's spool drain and reconcile, continuation turns, due wakes,
  and the cancel path).
- **Context**: `compiler.rs` (manifests, recompiles, the cache layout, the token estimate), `context_files.rs`, and
  `catalog.rs` (each model's window, prices, and caching).
- **Tool calls**: `toolrun.rs` (every call the model makes becomes a kernel action), with the gate in `policy.rs`
  (postures and the floor), `external.rs` (the hold after external text), `broker.rs` (granted secrets),
  `approval.rs`, and `peer.rs` (who is asking: a job's process can't answer). Plus the harness's own tools,
  `task.rs` and `wake.rs`, and the web tools in `web/`.
- **The protocol server**: `rpc/` (`server.rs` routes each method by name; `methods.rs`; `confirms.rs`), with
  `bus.rs` and `outbound.rs` (one ordered, capped queue per connection).
- **Surfaces of the record**: `push.rs` (the board), `outbox.rs`, `narrative.rs`, `ledger.rs`, `trace.rs`, and
  `telemetry/` (OTLP, metrics, spans).
- **Facts**: `fact/` (theseus-j6qn, Review 2's C2). One type per thing that happened, which says in one place what
  each channel gets: its ledger row (`KIND`, `row`), its notification (`METHOD`, `event`), its sentences
  (`narrate`), and its span (`span`). A site builds the fact and records it once (`TurnCtx::record`, a turn's
  `record`, or `Core::rec`/`session_rec` for the core's own); `FACTS` lists every one. A row's kind is a
  `theseus_protocol::LedgerKind`; `LedgerRow::new` takes nothing else (tests write an old or unknown name with
  `LedgerRow::named`).
- **Start and stop**: `config.rs`, `config_copy.rs`, `config_gate.rs`, `secrets.rs`, `startup.rs`, `restore.rs`,
  `sweep.rs`, `disk.rs`, and `binary.rs` (whether jobs can write the daemon's own binary, read when health asks).
  The config template is `config/theseus.example.toml`.
- **`store.rs`** is the kernel's view of storage: `Store::for_turn`, a session's writes, and the turn's transcript.

## Invariants

- **The frame budget.** A plain one-loop turn writes 5 frames, and each loop with one in-process tool adds 4.
  Observability rows ride in the turn's next frame; the session's write rides in `end_turn`'s
  (`Store::defer_session`). `tests_m3::a_plain_turn_stays_within_its_frame_budget` fails a sixth frame.
- **Lock order**: a session's lock, then an execution's (`Store::with_session`, `update_session`).
- **Kernel writes made together are one `Kernel::frame`**, with the core's own records added by `stage`: an answer
  (the bind or the decline, an approval's trust, its row, and the wake) is one (theseus-jj9f). A transaction holds
  its executions' locks, which belong to an OS thread, so it never spans an `.await`; a session's lock goes around
  it, never inside.
- **Serve first.** Nothing new on the start path waits on the network or writes; it goes after serving. Secrets
  resolve behind the socket, and each consumer waits for its own.
- **Where work runs.** An in-process toollet computes on the CPU pool (`cpu.rs`, a permit per core). A tool that
  waits on the network is `Backend::Async`, a future on the runtime. A verb over Theseus's own state is
  `Backend::Harness`. Never block a runtime worker on a sleep or a blocking wait.
- **The gate never refuses** and never parses what a command does. The order is the floor, the approve lists, the
  allow list, then the posture; a granted secret's posture and the external-text hold apply after it, and the
  stricter wins.
- **Results tell the truth.** `toolrun::cap` cuts on line edges and says what it left out, with the tool's own way
  to get the rest (`Tool::rest`). A listing names its scope.
- **Thinking goes back only to the provider that wrote it**, and every recompile strips the prefix's thinking.
- **Nothing retractable goes in the shared header** (Appendix F), so sessions on a profile share one cache entry.
- **The config template is the contract.** Change a default there, not only in code. The loader rejects unknown
  keys, and `example_template_uncommented_still_parses` un-comments every line and parses it.
- **Approval fails closed** (review 2's consideration 2). Without `[approval]`, only the CLI and a Discord DM the
  bindings file binds answer; the web UI and a guild channel answer once the section names them, and health then
  says `approval: open`. A test's bare label answers as the CLI (`From<&str> for Answerer` is test-only).
- **Secrets**: no value in a log, a row, a node, a result, or an error. `scrub.rs` scrubs tool output: each board
  value verbatim, in base64, and percent-encoded, and the shapes of secrets never resolved here (token prefixes,
  AWS keys, private-key blocks, JWTs). The broker hands a value only to the program it is granted to, run by its
  own argv, and never to one the call could make it run (`broker::launches`: the call's own environment, gh's
  aliases and extensions, git's aliases, `-c`, and the programs its options and URLs name).
- **One fact, recorded once.** A new ledger row, notification, or narrative sentence is a fact's projection in
  `fact/`, not a hand-written channel at its site. Recording writes no frame: its row rides in the turn's next frame
  (or a frame now, outside a turn), and a row that must ride in a frame the site builds is `Rec::row`, with the rest
  announced once that frame is written (`Rec::announce`).

## Tests

- In-process suites in `src/`: `tests_m3.rs` (turns through the whole core), `tests_continuations.rs`,
  `tests_failures.rs`, `tests_tasks.rs`, `tests_wakes.rs`, `tests_external.rs`, `tests_overflow.rs`,
  `tests_push.rs`, `tests_config.rs`, and `rpc/tests.rs`.
- `tests_registry.rs` is the reader rule's test. The gate runs it alone, before the suite.
- `tests_output.rs` is the output golden (theseus-j6qn): scripted scenarios through whole cores, and every frame,
  notification, and narrative line they produce, against `tests/golden/core_output.txt`. A refactor of a channel
  leaves it byte-identical; `THESEUS_GOLDEN=write` rewrites it, for a change you mean, and the diff is the review.
  A narrative line that the machine's load decides is left out there (`by_the_load`); a new one joins it.
- `tests_outside_text.rs`: property tests over every reader of outside text (the HTML reader, the wake's time
  parsers, the provider's SSE lines, and the scrubber, which also never lets a planted value through), with a fresh
  seed each run, so the gate keeps looking (Item 13). The release
  profile aborts on a panic, so one panic on outside text takes the daemon down. A new reader joins them.
- `FakeProvider::scripted` scripts a provider's answers. `Parts::for_tests` builds a core around it, and a second
  provider can be inserted into `parts.providers`.
- `Store::fail_turn_frame` (test-only) fails the first turn frame whose records match, once, as a full disk would.
- Debug builds assert, at every read, that a turn's kept transcript equals a fresh read of the store. A node written
  past the turn's handle panics a debug daemon.

## Traps

- Every child process the core starts (an `op`, a job's wrapper) goes through `theseus_kernel::children::spawn`, or
  the daemon's reaper can take its exit status (`ECHILD`).
- A static or a detached thread holds the core or the store by `Weak`. One that held an `Arc<Core>` kept the store
  open past a clean stop, and every start then paid redb's repair.
- A test that needs a process outside every job skips that part when the test runs inside a job, and says so: an
  approval from inside a job is refused, correctly.
- A recompile inside a tool loop strips thinking between a call and its result, so context files change on the
  next turn's first loop, never mid-turn.
- `cargo test -p theseus-core` builds a second copy of every dependency. Use
  `cargo nextest run --workspace -E 'package(theseus-core)'`.
