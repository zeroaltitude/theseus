# AGENTS.md: programming Theseus

The guide for any agent that works on this repository: Theseus itself, Claude Code, Codex, or another. It holds the
map and the rules, and links out for the depth. Read it whole before your first change. Then read the `AGENTS.md` of
every directory you touch (listed under "The map"): each says what is there, its invariants, its tests, and its traps.

## What Theseus is, and where the truth lives

Theseus is a durable agent runtime in Rust: `theseusd`, a daemon that owns the turn loop, the tools, and a write-ahead
log of everything it does; `theseus`, a thin CLI over the daemon's JSON-RPC protocol; and the cockpit, the web app
the daemon serves. AI agents build it in small, reviewed steps.

- **The spec, `docs/spec/`**, is the source of truth: *The Ship of Theseus* in chapters of under about 150 KB, so read
  a chapter whole. Start at its index, `docs/spec/README.md`, which says what each chapter holds and how they fit
  together (`docs/the-ship-of-theseus.md`, the one file until v0.79, now points there). Part I is the specification
  (§1 settled decisions, §2 principles, §9 budgets), Part II the plan (P0 holds the standing rules), and Part III the
  record: one item per step, with what it built, how it was proven, where it diverged, and what it left open.
- **`docs/status.md`**: what works today, what is being wired in, and the roadmap.
- **`docs/design/`**, indexed in its README: `roadmap-v2.md` (every step, by "row"), `roadmap-v1.1.md` (the week
  after v1), `stage2-operator-surfaces.md`, `aws-toolset.md`, `m4-boundaries.md`, `m5-judgment.md`, `m6-memory.md`,
  `m7-surface.md`, and `review-2.md`.
- **`docs/technical-overview.md`**: the core in depth, with commands to see each part work.

## The map

### Crates

One line each; a crate's key modules and its readers are in its own `AGENTS.md`.

- `theseus-protocol`: The wire types (JSON-RPC 2.0 over NDJSON). Types only: no runtime, no clock.
- `theseus-store`: The keel: a WAL of checksummed atomic frames (the truth), and a redb index rebuilt from it.
- `theseus-kernel`: The durable kernel: executions, actions, completions, the spool, budgets, locks, tasks, wakes, stops, the outbox's actions, the job wrapper.
- `theseus-tools`: Toollets: `fs.*`, `git.diff`, `git.log`, `text.diff`, and `proc.run`'s spec.
- `theseus-files`: Files people give the model, read for it: a PDF's pages, text, and parts; Office, OpenDocument,
  EPUB, RTF, and notebooks as sections; archives; ffmpeg and tesseract when present. Each conversion runs in a
  child with a time limit and a memory cap (theseus-c9l6).
- `theseus-core`: The agent: config, secrets, the turn, the compiler, tool calls and the gate, the RPC server, the push, the outbox, telemetry, AWS's accounts and tools.
- `theseus-aws-catalog`, `theseus-aws`: Every AWS operation's model, and one caller for all six protocols (the AWS design, §3.1).
- `theseus-discord`: The Discord binding, in-process; it acts through the protocol.
- `theseus-ontology`: The ontology's pure types (M4 §2.8): the kinds table, categories, guidance, memberships, and
  the compile walk's composition. No store, no policy; theseus-core keeps its records and runs the walk (21b).
- `theseus-sandbox`: L1: a job in its own namespaces under seccomp, with no cgroup of its own; the egress proxy (wired at 18c)
- `theseusd`: The daemon: serving, `job-wrapper`, `job-sandbox`, `check`, `restore`, `install`, the web server.
- `theseus`: The CLI, and its library `theseus_client` (client, render), which the terminal UI shares.
- `theseus-tui`: The terminal UI: every session in a sidebar, what needs you answered inline, a session's history and input line. A protocol client.
- `theseus-index`: The index tender (M6): a child of the daemon that follows the WAL read-only into BM25, exact entities, and vectors, and answers on `<state>/index/sock`. An installed binary of its own, beside `theseusd`.
- `theseus-mcp`: The Model Context Protocol by hand: the client the core's MCP board reads (36b), a fake server, and Theseus's own server (feature `server`, wired at 41b).
- `theseus-follow`: The WAL follower: a store's log read from outside the process that writes it, from a cursor, woken by inotify.
- `theseus-exam`: A tool beside the binaries: M6's memory exam, one scratch daemon per `[memory] arm`, and its report.
- `theseus-sim`: A tool beside the binaries: the crash test, `kernel-sim`, the lifecycle bench and its history, fake Discord and model servers, and the Discord proof.

The rest were merged ahead of their reader (Part III Items 16, 18, and 20). Each says so in its own manifest:
`reserved_for` under `[package.metadata.theseus]` names the roadmap row that wires it in (Item 32):

The reserved ones, with the row that wires each in, are listed in `docs/design/README.md`.

### Directories

- **`cockpit/`**: the cockpit, the daemon's web UI, served at `/`. Its build is not committed.
- **`scripts/`**: `gate.sh`, the commit gate, and `smoke.sh`, an end-to-end check with real secrets and real models.
- **`infra/aws/`**: the CloudFormation templates for Theseus's AWS account, with their stack policies and
  `check.sh` (see its README).
- **`docs/`**: the spec in chapters, `spec/` (its PDF is rendered and sent, never committed), `status.md`,
  `technical-overview.md`, `design/`, `research/`, and `notes/`.
- **At the root**: `deny.toml` (permissive licences only; each ignored advisory gives its reason),
  `rust-toolchain.toml` (one exact release), `clippy.toml` (shape), `.cargo/config.toml` (the musl target),
  `.config/nextest.toml` (a hung test dies at two minutes; named flaky tests retry), and `.github/workflows/ci.yml`.

Directory guides: each crate, `cockpit`, and `scripts` has an `AGENTS.md`.

### Generated files: never edit them by hand

- **`cockpit/src/protocol.gen/`**: the cockpit's TypeScript, written from the Rust types by theseus-protocol's test
  `the_cockpits_types_are_generated_from_the_rust_ones`. `git add` what it writes; the gate fails when it is stale.
- **`crates/theseusd/cockpit/dist/`**: the cockpit's build, ignored by git and embedded when present. Build it before
  a release build. The gate builds it before the suite, whose tests of `/` read it.
- **`Cargo.lock`**: regenerated, never merged by hand.

### Where the big things live

The kernel, the store, the turn, the compiler, tool calls, the RPC layer, facts, the push, the outbox, the config, L1,
and the index tender: see "Where the big things live" in `crates/theseus-core/AGENTS.md`.

## The principles that bind code

Each is a requirement, with its spec section.

- **FAST** (§2, §9). The lifecycle budgets (start to answering under 50 ms, clean shutdown 100, crash and restart
  150, a binary swap 200) fail the gate as a test does. Serve first: nothing before the socket answers waits on the
  network, a model, an index rebuild, or work that grows with history. The start path writes one frame of its own,
  and any other write goes after serving. New startup work lands with its bench row.
- **EXQUISITE VISIBILITY** (§2, P2b). Every turn, loop, call, judgment, and completion is timed and attributed as it
  happens, in the record. A new kind of work lands as facts (its row, its notification, its lines, its span in one
  type), never as hand-written channels, with its metric on the same commit.
- **Event-driven** (§1, "Execution model"; §2, QUIET BY CONSTRUCTION). No in-flight state lives only in memory: every
  dispatched thing is a WAL record with a correlation id, and its completion arrives as an event. No busy loops.
- **The WAL and frames** (§6; Part III F2). The WAL is the truth, and every index a projection of it. A kernel method,
  or a kernel transaction (`Kernel::frame`), writes exactly one frame, and a fact lands in one frame with the rows
  that describe it. A plain one-loop turn writes 4 frames (its admission rides its input's, theseus-2uby): `tests_m3::a_plain_turn_stays_within_its_frame_budget`
  holds it in the core, `bench turn` at the daemon.
- **Append-only** (§2). The record only grows: compaction, supersession, and forgetting are new records. Payload
  erasure (§5.6) is the one receipted exception.
- **The reader rule** (P0, rule 3). Nothing is declared without its reader: a crate, protocol method,
  notification, edge kind, or label lands with what reads it, or with a marker naming the row that brings it.
  `tests_registry` in theseus-core enforces it, and the gate runs it first. A ledger kind (`LedgerKind`) is declared
  by being written; the same test fails a kind nothing writes.
- **The store's version rule** (P5b; Part III F4a; theseus-ptx1). The store has one format number,
  `MANIFEST_FORMAT`. Any step that adds a field to a stored record, or changes the frame or record encoding, bumps
  it, with a reader for the old layout and a sample of it in theseus-core's `tests_layouts`, its old bytes a literal
  (Item 61). A build refuses a store newer than it knows. The number is assigned when a step lands on `main`.
- **A typed protocol** (§1, "Wire protocol"; §3.18; Item 30). Every client, the CLI, Discord, and the cockpit
  included, reaches the core only through the protocol. Each wire shape has one Rust definition, and the TypeScript is
  generated from it.
- **Opinionated** (§2, OPINIONATED and NATIVE FIRST). One blessed path and few knobs: no plugin architecture, and no
  hooks (deleted in A3b). A new capability is a native toollet unless a written reason says it can't be. "Ruthlessly
  remove complexity" (the owner, A3b).
- **Jev in the core** (§2, JEV IN THE LOOP; §3.7). Judgments are typed questions inside the loop, recorded with
  their probabilities and outcomes, in shadow before they act. Jev is never the only guard on safety, and the
  deterministic controls (`/stop`, budgets) bypass it.
- **The security posture** (§1, "Secrets"; §2, NOTIFY OVER BLOCK; §3.9; §3.19).
  - Secrets live in 1Password, named by `op://` reference: the recommended source. Where there is no vault (a
    container, CI), a `[secrets]` entry may be `env:NAME` or `file:PATH` instead, and `theseusd check` and health
    name each such secret (theseus-n88g.1). Never print, log, or store a value.
  - The gate never refuses, and never guesses what a command does: each tool has a posture (`open`, `notify`, or
    `approve`), and the floor always asks.
  - Approvals come from the operator, in a private place (the place rule, theseus-zmgb). The CLI refuses an answer,
    an undo of a tightening, a trust, a publish, and the AWS bootstrap inside a job (its `THESEUS_SESSION`), saying
    why and that the operator runs it from their own shell. That is a speed bump, said honestly: an L0 job can strip
    its environment, or talk to the socket itself. L1, whose view hides the socket, is the boundary.
  - The config copy acts (§3.19, amended by theseus-zmgb). A start serves from its copy of the vault's note and acts
    on it at once when the copy is the one the daemon wrote: as the daemon writes it, it records its sha256 in the
    store. A copy edited since is not used, and that start reads the vault first. After serving, the vault is read
    once: a changed note rewrites the copy and restarts the daemon onto it; a note that changed again, one that does
    not load, or a vault that does not answer is said in health, and the copy keeps serving. The digest is a light
    check, not a boundary: a job that can write the copy can write the store while the daemon is down.
  - External text holds a session: after a session reads web text, a call that acts waits until the operator trusts
    it again. So does a `proc.run` of a program `[policy] external_programs` lists (`gh`), and a session that a
    holding session's job opens or sends a turn to (its `THESEUS_SESSION`, sent as `opened_from`). Under default
    trust these two are light guards, not boundaries: a job can strip its own environment (theseus-b5cl).
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
  and network waits to async tools (F3; Items 5 and 25). A wait for the disk, or for a lock held across it, goes
  through `theseus_store::blocking`, which hands the worker's role to another thread first (theseus-vni9).
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
    the cockpit's routes (`cockpit/src/main.tsx`) and `cockpit/src/protocol.ts`, the CLI's `main.rs`, and any
    dispatch `match`. The one exception is
    the line that adds the lane's crate to the workspace's `members`.

  A lane pushes only its own branch. The reviewer merges it once reviewed, one lane at a time and never while a
  spine step works on `main`: rebased, `Cargo.lock` regenerated, the whole gate, `main` fast-forwarded, and the
  branch and worktree deleted.
- **The gate before every commit.** A lane: `THESEUS_GATE_NO_BENCH=1 scripts/gate.sh && git commit …`. The chain's
  gate on `main`: `scripts/gate.sh && …`, with its benches. The gate takes the shared lock itself, so never wrap it in
  `flock` or `theseus-quiet.sh`. Chain with `&&`, never `;`. `scripts/AGENTS.md` has the rest.
- **Commits** are signed (`git commit -S`), one per green sub-step. The subject is `area: what changed (<issue>)`,
  the area in lower case (`kernel`, `store`, `toolrun`, `cli`, `docs`, …), and the body says what changed and why, in
  plain words. A trailer names the agent, as `Co-Authored-By: Tabitha/Claude <noreply@anthropic.com>` does, or the
  pilot's `Co-Authored-By: Theseus (Claude Opus 5.5) <noreply@anthropic.com>`.
- **Every step is reviewed**: a written review, the gate rerun, and a live check of the install build. Then a docs
  commit records it: the spec's Part III item, its version line (the first heading of `docs/spec/01-front.md`), and
  `docs/status.md` (its "Updated" line, the recently landed step, the roadmap's row). Take every time you write from
  `date`, never a guess.
- **Installing** a reviewed build (`scripts/build.sh --profile release-thin`): copy-then-rename each of `theseus`,
  `theseusd`, `theseus-sim`, `theseus-tui`, and `theseus-index` into `~/.local/bin` (`cp target/release-thin/$b
  ~/.local/bin/.$b.new && mv -f ~/.local/bin/.$b.new ~/.local/bin/$b`). A running daemon survives the swap. The daemon
  runs the `theseus-index` beside its own binary as its index tender, and only that one; a running tender keeps its old
  image until it restarts. `scripts/setup.sh` does all of it, from a checkout to a running service (`docs/setup.md`).
- **The README stays stable.** It says what Theseus is and why. What changes with each step goes in
  `docs/status.md`.
- **Reviews are appendices.** A review of the design is answered in an appendix of the spec (Appendices A, C to F),
  and a review of the code is a design document (`docs/design/review-2.md`) whose accepted findings become steps.
- **The config is the operator's.** It is a file, `/etc/theseus/theseus.toml` (theseusd's default when there is no
  `~/.theseus/theseus.toml`, theseus-5aqz), or a note in a vault, and it holds only what differs from the defaults
  (theseus-vwar), so a new key or default reaches it with the build, and only a value of the operator's own needs an
  edit. Its secrets are `op://` references, never values. Every key has a default, documented in the template
  (`crates/theseus-core/config/theseus.example.toml`). The loader rejects unknown keys, and
  `example_template_uncommented_still_parses` holds the template to it.

## This machine

Theseus is built on one WSL2 machine, beside the operator's own running daemon and other agents' work. The rest of this
section, the scratch daemon's rules and ports, the disk, sccache, and shell traps, is in "This machine" in `scripts/AGENTS.md`.

- **The operator's daemon** runs as a bare `theseusd` on `~/.theseus`, its default socket, and web port 7433. Never
  touch it: no signal, no connection to its socket or port, and never a copy of its bindings file (two daemons would
  answer on Discord). Kill only the pids you started, never by name with `pkill` or `killall`.
- **Your own scratch daemon** has its own `--config`, `--socket`, and `--state-dir`, on a fresh state dir or a copy of
  the operator's store alone; stop it with `theseus --socket <its socket> shutdown`.

## Keeping these files current

- A step that teaches something durable updates the nearest `AGENTS.md`, or this one, in the same commit or in the
  step's docs commit. Each review checks the `AGENTS.md` files, as it checks Part III and `docs/status.md`.
- Every path and command named here exists. A rename updates it.
- Keep this file under 20 KB. A developing session carries it whole in every turn's system block. Put depth in the
  spec, the design docs, or a directory's `AGENTS.md`, which an agent reads when it works there.
