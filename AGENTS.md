# AGENTS.md: programming Theseus

The guide for any agent that works on this repository: Theseus itself, Claude Code, Codex, or another. It holds the
map and the rules, and links out for the depth. Read it whole before your first change. Then read the `AGENTS.md` of
every directory you touch (listed under "The map"): each says what is there, its invariants, its tests, and its traps.

## What Theseus is, and where the truth lives

Theseus is a durable agent runtime in Rust: `theseusd`, a daemon that owns the turn loop, the tools, and a write-ahead
log of everything it does; `theseus`, a thin CLI over the daemon's JSON-RPC protocol; and two web apps the daemon
serves. AI agents build it in small, reviewed steps.

- **The spec, `docs/the-ship-of-theseus.md`**, is the source of truth. It is about 6,800 lines: find a section with
  `grep -n '^##'`, and read it by ranges. Part I is the specification (§1 settled decisions, §2 principles, §9
  budgets), Part II the plan (P0 holds the standing rules), and Part III the record: one item per step, with what it
  built, how it was proven, where it diverged, and what it left open.
- **`docs/status.md`**: what works today, what is being wired in, and the roadmap.
- **`docs/design/`**, indexed in its README: `roadmap-v2.md` (every step, by "row"), `stage2-operator-surfaces.md`,
  `aws-toolset.md`, `m4-boundaries.md`, `m5-judgment.md`, `m6-memory.md`, `m7-surface.md`, and `review-2.md`.
- **`docs/technical-overview.md`**: the core in depth, with commands to see each part work.

## The map

### Crates

| Crate | What it is | Key modules | Read by |
|---|---|---|---|
| `theseus-protocol` | The wire types (JSON-RPC 2.0 over NDJSON). Types only: no runtime, no clock. | `lib.rs` (the `method`, `notify`, `error_code` tables), `events.rs`, `push.rs`, `gate.rs`, `ts.rs` | every crate on the wire, and the web apps (generated) |
| `theseus-store` | The keel: a WAL of checksummed atomic frames (the truth), and a redb index rebuilt from it. | `wal.rs`, `index.rs`, `record.rs` (`kinds::SCHEMAS`), `store.rs` | kernel, core, theseusd, sim |
| `theseus-kernel` | The durable kernel: executions, actions, completions, the spool, budgets, locks, tasks, wakes, stops, the outbox's actions, the job wrapper. | `kernel.rs`, `tx.rs`, `locks.rs`, `job.rs`, `children.rs`, `outbox.rs` | core, discord, theseusd, sim |
| `theseus-tools` | Toollets: `fs.*`, `git.diff`, `git.log`, `text.diff`, and `proc.run`'s spec. | `fs.rs`, `git.rs`, `proc.rs`, `paths.rs` | core |
| `theseus-core` | The agent: config, secrets, the turn, the compiler, tool calls and the gate, the RPC server, the push, the outbox, telemetry. | `turn.rs`, `compiler.rs`, `toolrun.rs`, `rpc/`, `config.rs` | theseusd, discord, sim |
| `theseus-discord` | The Discord binding, in-process; it acts through the protocol. | `runtime.rs`, `courier.rs`, `render.rs` | theseusd |
| `theseusd` | The daemon: serving, `job-wrapper`, `check`, `restore`, `install`, the web server. | `main.rs`, `web.rs`, `install/` | (a binary) |
| `theseus` | The CLI, and its library `theseus_client` (client, render), which the terminal UI shares. | `main.rs`, `cmd.rs`, `render.rs`, `client.rs` | (a binary) |
| `theseus-tui` | The terminal UI: every session in a sidebar, what needs you answered inline, a session's history and input line. A protocol client. | `run.rs` (the loop), `app.rs` (no I/O), `board.rs`, `ui.rs` | `theseus tui`, which execs it |
| `theseus-sim` | A tool beside the binaries: the crash test, `kernel-sim`, the lifecycle bench and its history, fake Discord and model servers. | `lifecycle.rs`, `kernel_sim.rs`, `fake_discord.rs` | the gate, and tests |

The rest were merged ahead of their reader (Part III Items 16, 18, and 20). Each says so in its own manifest:
`reserved_for` under `[package.metadata.theseus]` names the roadmap row that wires it in (Item 32):

| Crate | What it is | Wired in at |
|---|---|---|
| `theseus-sandbox` | L1: a job in its own namespaces, seccomp, and cgroup; the egress proxy | row 17 (17b) |
| `theseus-ontology` | The fungible ontology's first slice (§4.1a) | row 26 (21b) |
| `theseus-aws-catalog`, `theseus-aws` | Every AWS operation's model, and one caller for all six protocols | row 29 (C1, 14a) |
| `theseus-aws-guard` | The AWS guardrails: the gate's check, and the generated guards and SCPs | row 30 (C2, 14b) |
| `theseus-judge` | Jev: the typed client, bands, batching, the breaker, the question packs | row 37 (23a) |
| `theseus-follow`, `theseus-index` | The WAL follower, and the index tender (BM25, entities, vectors) | row 51 |
| `theseus-memory` | FSRS-6 and spreading activation, pure | row 52 (30a) |
| `theseus-exam` | The memory exam | row 55 |
| `theseus-mcp` | MCP, client and server, written by hand | row 66 (36b) |
| `theseus-voice` | The voice engine for Discord | row 77 (44b) |

### Directories

- **`web/`**: the Observatory, the daemon's first web UI. Its build, `crates/theseusd/web/dist`, is committed.
- **`cockpit/`**: the cockpit, served at `/cockpit/`. Its build is not committed.
- **`scripts/`**: `gate.sh`, the commit gate, and `smoke.sh`, an end-to-end check with real secrets and real models.
- **`infra/aws/`**: the CloudFormation templates for Theseus's AWS account, with their stack policies and
  `check.sh` (see its README).
- **`docs/`**: the spec and its PDF, `status.md`, `technical-overview.md`, `design/`, `research/`, and `notes/`.
- **At the root**: `deny.toml` (permissive licences only; each ignored advisory gives its reason),
  `rust-toolchain.toml`, `.cargo/config.toml` (the static musl target), `.config/nextest.toml` (a hung test is
  killed at two minutes), and `.github/workflows/ci.yml`.

Directory guides: `crates/theseus-protocol`, `crates/theseus-store`, `crates/theseus-kernel`, `crates/theseus-tools`,
`crates/theseus-core`, `crates/theseus-discord`, `crates/theseusd`, `crates/theseus`, `crates/theseus-tui`,
`crates/theseus-sim`, `web`, `cockpit`, and `scripts` each have an `AGENTS.md`.

### Generated files: never edit them by hand

- **`web/src/protocol.gen/`**: the web apps' TypeScript, written from the Rust types by theseus-protocol's test
  `the_web_apps_types_are_generated_from_the_rust_ones`. `git add` what it writes; the gate fails when it is stale.
- **`crates/theseusd/web/dist/`**: the Observatory's build (`npm run build` in `web/`), committed. The gate fails
  when a build changes it uncommitted.
- **`crates/theseusd/cockpit/dist/`**: the cockpit's build, ignored by git and embedded when present. Build it before
  a release build.
- **`Cargo.lock`**: regenerated, never merged by hand.

### Where the big things live

- **Kernel transitions and frames**: `crates/theseus-kernel/src/kernel.rs`, and `tx.rs`. Every mutating method writes
  one frame.
- **The store's WAL and index**: `crates/theseus-store/src/wal.rs`, `index.rs`, and `store.rs`.
- **The turn loop**: `crates/theseus-core/src/turn.rs`, with `advancer.rs`; the harness loop in `harness.rs`, and
  what it drives (continuations, the heartbeat) in `rpc/driver.rs`.
- **The compiler**: `crates/theseus-core/src/compiler.rs` and `context_files.rs`.
- **Tool calls**: `crates/theseus-core/src/toolrun.rs`; the gate's postures in `policy.rs`, `external.rs`, and
  `broker.rs`.
- **The RPC layer**: `crates/theseus-core/src/rpc/` (`server.rs`, `methods.rs`, `driver.rs`, `confirms.rs`).
- **The push's board**: `crates/theseus-core/src/push.rs`, fed by `Kernel::observe`; `attention()` is in
  `crates/theseus-protocol/src/push.rs`.
- **The outbox**: `crates/theseus-kernel/src/outbox.rs`, `crates/theseus-core/src/outbox.rs`, and
  `crates/theseus-discord/src/courier.rs`.
- **The config**: `crates/theseus-core/src/config.rs`, and the template `crates/theseus-core/config/theseus.example.toml`
  (`theseusd example-config` prints it).

## The principles that bind code

Each is a requirement, with its spec section.

- **FAST** (§2, §9). The lifecycle budgets (start to answering under 50 ms, clean shutdown 100, crash and restart
  150, a binary swap 200) fail the gate as a test does. Serve first: nothing before the socket answers waits on the
  network, a model, an index rebuild, or work that grows with history. The start path writes one frame of its own,
  and any other write goes after serving. New startup work lands with its bench row.
- **EXQUISITE VISIBILITY** (§2, P2b). Every turn, loop, call, judgment, and completion is timed and attributed as it
  happens, in the record. A new kind of work lands with its span, attributes, and metric on the same commit.
- **Event-driven** (§1, "Execution model"; §2, QUIET BY CONSTRUCTION). No in-flight state lives only in memory: every
  dispatched thing is a WAL record with a correlation id, and its completion arrives as an event. No busy loops.
- **The WAL and frames** (§6; Part III F2). The WAL is the truth, and every index a projection of it. A kernel method,
  or a kernel transaction (`Kernel::frame`), writes exactly one frame, and a fact lands in one frame with the rows
  that describe it. A plain one-loop turn writes 5 frames: `tests_m3::a_plain_turn_stays_within_its_frame_budget`
  holds it.
- **Append-only** (§2). The record only grows: compaction, supersession, and forgetting are new records. Payload
  erasure (§5.6) is the one receipted exception.
- **The reader rule** (P0, rule 3). Nothing is declared without its reader: a crate, protocol method,
  notification, edge kind, or label lands with what reads it, or with a marker naming the row that brings it.
  `tests_registry` in theseus-core enforces it, and the gate runs it first.
- **The store's version rule** (P5b; Part III F4a). A record layout change bumps its kind in `kinds::SCHEMAS`, with
  a reader for the old layout and a test that reads it. A frame or record encoding change bumps `MANIFEST_FORMAT`.
  A build refuses a store newer than it knows. Schema numbers are assigned when a step lands on `main`.
- **A typed protocol** (§1, "Wire protocol"; §3.18; Item 30). Every client, the CLI, Discord, and the web apps
  included, reaches the core only through the protocol. Each wire shape has one Rust definition, and the TypeScript is
  generated from it.
- **Opinionated** (§2, OPINIONATED and NATIVE FIRST). One blessed path and few knobs: no plugin architecture, and no
  hooks (deleted in A3b). A new capability is a native toollet unless a written reason says it can't be. "Ruthlessly
  remove complexity" (Eddie, A3b).
- **Jev in the core** (§2, JEV IN THE LOOP; §3.7). Judgments are typed questions inside the loop, recorded with
  their probabilities and outcomes, in shadow before they act. Jev is never the only guard on safety, and the
  deterministic controls (`/stop`, budgets) bypass it.
- **The security posture** (§1, "Secrets"; §2, NOTIFY OVER BLOCK; §3.9; §3.19).
  - Secrets live in 1Password and are named only by `op://` reference. Never print, log, or store a value.
  - The gate never refuses, and never guesses what a command does: each tool has a posture (`open`, `notify`, or
    `approve`), and the floor always asks.
  - Approvals come from the operator; a job's own process cannot answer one.
  - External text holds a session: after a session reads web text, a call that acts waits until the operator trusts
    it again.
  - Money is a gate: every call reserves its worst case before it runs.

## The principles discovered while building it

Each traces to the Part III item that taught it.

- **Tests first, and goldens before a move.** A fix starts from a failing test (A3b's cut 5; Item 26). A refactor
  starts with goldens of today's behaviour and ends byte-identical (Item 30; A3b's output-diff probe).
- **Prove each fix against a planted revert.** Switch the fix off, see its test fail as the bug did, and put the file
  back with a fresh mtime (`touch` it), or cargo keeps the reverted build (Items 12, 13, 25, and 34).
- **A live check on a scratch daemon** of the step's build, for every step that changes behaviour (every item
  since A3), on a fresh state dir or a copy of the store, never the operator's own.
- **Never block a runtime worker** on a sleep or a blocking wait: use tokio's timer. Compute goes to the CPU pool,
  and network waits to async tools (F3; Items 5 and 25). The fsync on a worker is still open (theseus-vni9).
- **One observer, not a publish at every site** (`Kernel::observe`, Item 33).
- **A wait owned by the daemon** (`session.wait`), not a client's poll (Item 33).
- **Invented names in fixtures, tests, and commits.** The repository is public (Item 16).
- **Read what the provider says, not only the estimate**: its token counts, its refusals, and its served model
  (Items 26, 31, and 34).
- **A race is reproduced under load before it is fixed.** Priority, not count, makes the load: loops at a lower
  nice than the test. Prove "waits for nothing" on tokio's paused clock instead of timing it (Items 31 and 35).
- **Detection written by hand can't be finished.** Tell the operator what ran instead (A3b, the reversal).
- **Hermetic tests.** A fixture never inherits the operator's global config, git's included (Item 7).
- **Never delete what can't be rebuilt.** Move it aside, as a bad index becomes `index.redb.bad-<ms>` (Item 17).
- **What must be seen is durable; live progress is best effort** (Item 6).
- **An attempt that may have run is unknown,** never "not sent" (Item 18).
- **One command, one effect** (Item 9).
- **Words that tell the truth.** A cut result says what it left out and how to get it, and a listing names its
  scope (Item 21; §3.24).
- **Choose the rule before the data.** Fix a choice rule before the grid runs, and ask held-out data once (Item 24).

## The workflow

- **Beads** (`bd`) tracks the work, with issue ids `theseus-*`. Its database is in the main checkout's `.beads/`, and
  `bd` works from any worktree. Read `bd show <id>`, notes included, before acting. Claim with
  `bd update <id> --claim`: it is a compare-and-set, and a nonzero exit means the issue is someone else's, so stop.
  Every issue has an owner. Add notes with `bd update <id> --append-notes "<text>"`. Commits, Part III, and briefs
  name issues by id.
- **The spine and the lanes.** Steps run one at a time on `main`: the spine. Independent work runs beside it in lanes:
  - a worktree at `~/projects/theseus-wt/<lane>`, on branch `lane/<lane>`;
  - with its own `CARGO_TARGET_DIR`, never the main tree's `target/` (a shared one poisons both), and a `target`
    symlink to it, since the gate runs `target/debug/theseus-sim`;
  - inside its own crate or directory. Shared files change only at the join: `scripts/gate.sh`, `deny.toml`,
    `web/src/App.tsx`, `web/src/protocol.ts`, the CLI's `main.rs`, and any dispatch `match`. The one exception is
    the line that adds the lane's crate to the workspace's `members`.

  A lane pushes only its own branch. The reviewer merges it once reviewed, one lane at a time and never while a
  spine step works on `main`: rebased, `Cargo.lock` regenerated, the whole gate, `main` fast-forwarded, and the
  branch and worktree deleted.
- **The gate before every commit.** `scripts/gate.sh`, under the shared lock:
  `flock -o ~/.cache/theseus-gate.lock scripts/gate.sh && git commit …`. Chain with `&&`, never `;`.
  `scripts/AGENTS.md` says what it runs and how long it takes.
- **Commits** are signed (`git commit -S`), one per green sub-step. The subject is `area: what changed (<issue>)`,
  the area in lower case (`kernel`, `store`, `toolrun`, `cli`, `docs`, …), and the body says what changed and why, in
  plain words. A trailer names the agent, as `Co-Authored-By: Tabitha/Claude <noreply@anthropic.com>` does, or the
  pilot's `Co-Authored-By: Theseus (Claude Opus 5.5) <noreply@anthropic.com>`.
- **Every step is reviewed**: a written review, the gate rerun, and a live check of the release build. Then a docs
  commit records it: the spec's Part III item, its version line, and `docs/status.md` (its "Updated" line, the
  recently landed step, the roadmap's row). Take every time you write from `date`, never a guess.
- **Installing** a reviewed release build: copy-then-rename each of `theseus`, `theseusd`, `theseus-sim`, and
  `theseus-tui` into `~/.local/bin` (`cp target/release/$b ~/.local/bin/.$b.new && mv -f ~/.local/bin/.$b.new
  ~/.local/bin/$b`). A running daemon survives the swap.
- **The README stays stable.** It says what Theseus is and why. What changes with each step goes in
  `docs/status.md`.
- **Reviews are appendices.** A review of the design is answered in an appendix of the spec (Appendices A, C to F),
  and a review of the code is a design document (`docs/design/review-2.md`) whose accepted findings become steps.
- **The config is the operator's.** It is a note in a vault that agents can't write. A new key or default is a
  change to the template (`crates/theseus-core/config/theseus.example.toml`), which the operator pastes. The loader
  rejects unknown keys, and `example_template_uncommented_still_parses` holds the template to it.

## This machine

Theseus is built on one WSL2 machine, beside the operator's own running daemon and other agents' work.

- **The operator's daemon** runs as a bare `theseusd` on `~/.theseus`, its default socket, and web port 7433. Never
  touch it: no signal, no connection to its socket or port, and never a copy of its bindings file (two daemons would
  answer on Discord). Kill only the pids you started, never by name with `pkill` or `killall`.
- **Your own scratch daemon** has its own `--config`, `--socket`, and `--state-dir`: a fresh state dir, or a copy of
  the operator's store alone. Turn `[discord]` and `[web]` off in its config unless the check needs them. A
  Discord-enabled scratch daemon runs only on a fresh state dir (theseus-c3e). Stop it with
  `theseus --socket <its socket> shutdown`, and wait for its pid to exit.

  | Port | Whose |
  |---|---|
  | 7433 | the operator's daemon (web UI and cockpit) |
  | 7434 to 7439 | scratch daemons' web UIs, one each (7434 is the cockpit's dev default) |
  | 5173, 5174 | the Observatory's and the cockpit's dev servers |
- **The disk.** WSL's disk is a file on the Windows drive (C:), and it only grows. When C: fills, the whole VM
  pauses, while Linux's `df` still shows hundreds of GB free. Check C: with the operator's disk guard before a heavy
  build, and don't build under 30 GB. Keep one target dir per worktree. Delete build caches outright, never to the
  Trash, which keeps every byte on C:.
- **sccache.** No server survives a restart, and by default it exits after 10 idle minutes. Its client then hangs.
  If `pgrep -a sccache` shows no server, start one with `SCCACHE_IDLE_TIMEOUT=0 sccache --start-server`, or unset
  `RUSTC_WRAPPER`.
- **A connect to a loopback port with no listener hangs**: the packet is dropped, not refused. Bound every connect,
  and test a service that is down with a fake, never with a closed port.
- **`pgrep -f`, `pkill -f`, and `ps | grep` match your own shell**, whose command line holds the pattern. Find a
  process by pid (`$!`, `pgrep -P`) or by `/proc/<pid>/comm`.
- **`git stash` is shared by every worktree.** Set work aside as a patch file instead.
- **The gate lock.** Take it with `flock -o`, so nothing the gate starts can keep it. A gate that sits at 0% CPU is
  waiting on a lock, cargo's package cache or this one: find the holder before waiting longer.
- **A bench miss beside a neighbour's load.** The gate waits for the machine to settle and reruns a miss once. A
  miss that its rerun passes is the machine. Rerun the bench alone before calling a miss a regression.
- **`/tmp` is wiped by a WSL restart.** Keep harnesses, logs, and reports where they survive, and commit and push at
  every green point: a restart, or an account's usage limit, can end a run at any moment.

## Keeping these files current

- A step that teaches something durable updates the nearest `AGENTS.md`, or this one, in the same commit or in the
  step's docs commit. Each review checks the `AGENTS.md` files, as it checks Part III and `docs/status.md`.
- Every path and command named here exists. A rename updates it.
- Keep this file under 20 KB. A developing session carries it whole in every turn's system block. Put depth in the
  spec, the design docs, or a directory's `AGENTS.md`, which an agent reads when it works there.
