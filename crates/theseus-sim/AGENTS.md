# theseus-sim

The proving tools, installed beside `theseusd` and `theseus` (a tool, so a reader of its own: Part III Item 32).
The gate runs its lifecycle bench, and its crash test and kernel simulator on small fixed seeds (`tests/sim.rs`).

Key modules: `lifecycle.rs`, `kernel_sim.rs`, `fake_discord.rs`, `discord_proof.rs`. Read by: the gate, and tests.

## What's here

- `src/main.rs`: the subcommands.
  - `crash-test` (the M1 exit test): a `worker` appends records and is SIGKILLed at random; each reopen must hold
    every record the worker reported durable, byte for byte. `--tear` also tears the WAL's tail; `--restarts`
    restarts within an iteration.
  - `kernel-sim` (the M2 exit test): the kernel under a virtual clock with seeded faults (a crash between any two
    frames or inside a startup step, lost, duplicate, and late completions, cancels; wakes, one-shot and repeating,
    in `kernel_sim/wakes.rs`, and crashes that keep the daemon down for minutes) and invariants checked at every
    step. `--p-race` races a second thread against turns; 0 is fully deterministic. sim2 (theseus-celu.35):
    `/stop` between turns, in one, beside a raced one, and of one call (`kernel_sim/stops.rs`); tasks under a
    parent, their carve and reports (`kernel_sim/tasks.rs`); and the outbox's posts, sent by a fake binding to a
    fake channel with crashes around each transition (`kernel_sim/outbox.rs`). Each operation goes in a module of
    its own; `kernel_sim.rs` holds the roll, the race's arm, and the checks' calls. tests/sim.rs reads its coverage
    counts from the `--p-race 0` run only, since a raced run reproduces only up to its first race (theseus-81ig).
  - `bench lifecycle` (`src/lifecycle.rs`): §9's budgets on a real `theseusd`: cold start, the same from a vault
    note's copy, clean shutdown with a job running, the same with a reply's post in flight to the in-process fake
    Discord (`inflight`, its own rig), SIGKILL and restart, a binary swap with the job's wrapper adopted, restore
    (with `theseusd restore`'s own phases), and the push's seed. `--check` fails a p95 over its budget plus the
    phase's margin (`lifecycle::margin_ms`, measured on the build machine).
  - `bench turn` (`src/perf.rs`, theseus-goa8): a plain turn and a tool-call turn on the stand-in model, each run N
    times on one warm session: wall time by the bench's clock and the daemon's, and **frames per turn**, counted from the
    daemon's WAL by `src/walcount.rs` (a read-only tail; the daemon reports no frame count, and the core is not changed
    for a bench). A frame is one `fdatasync`, so frames are §9's per-turn overhead in a unit that does not depend on the
    disk. `--check` fails a plain turn that writes more than `perf::PLAIN_TURN_FRAMES` (5; the floor is 2). Beside them:
    this disk's `fdatasync`, probed before the daemon starts and after it stops (the quieter is used, so the harness's own
    share of a turn reads off as an upper bound), and the daemon's resident memory after the start and after a burst. The scratch daemon has Discord and the web UI off.
    `--session-nodes N` (step 33, `src/perf/long.rs`) measures turns in one session of N nodes instead, written before
    the daemon starts (`synth::long_session`: five-node exchanges whose tool results are `--result-bytes`, 8192 by
    default): each turn's wall time, frames, and the nodes the daemon decoded for it (health's `store.node_cache`
    `decodes`, read around the turn; a build before tiering has none, and the row says so), and memory with the index
    tender's. No budget: its frames include the turn that compacts the session.
  - `bench idle`: an idle daemon over a window (30 s): CPU time, wakeups (its threads' voluntary context switches, from
    `src/procfs.rs`), frames written (a quiet daemon writes none), and memory, on an empty store or `--sessions N`.
    `--settle N` waits up to N s (default 60) for the daemon to go quiet before the window begins, and shows the CPU in
    each ten seconds meanwhile, so a daemon that never goes quiet shows whether it is slowing. Measured, no budget yet:
    an empty store is quiet (5 ms of CPU in 30 s, 4.5 wakeups a second), and at 10,000 parked sessions a release build is
    still at 5.8 % of a core 300 s after its first answer, writing no frames (review 2's S1). The index tender's memory is
    read beside the daemon's (the `theseus-index` child), and `--active N` then opens N sessions and runs a turn in each
    and reads both again: with `--sessions 10000 --active 50`, design M6 §2.10's row (§9: under 1 GB together).
  - `bench size`: the shipped binaries' sizes against §9's 60 MB. Meaningful on a release or install build.
  - `bench history` (`src/history.rs`): each phase's recent runs and headroom, from the CSV every gate appends. The
    other benches' columns (`history::OTHER`) are in the same file, each with its unit.
  - `synth-store` (`src/synth.rs`): a store of parked sessions, for `bench lifecycle --sessions N`.
  - `fake-discord`: a stand-in for Discord's REST API, and with `--gateway` its gateway (theseus-6g62); `--guild`
    gives it a guild for the viewer check (theseus-ck0k).
  - `fake-model --rules <file>` (`src/fake_model.rs`, 37b): a scripted stand-in for the Messages API, for a scratch
    daemon's `api_base`: each turn's last user text takes the first rule (`{when, calls: [{name, input}], text}`) it
    holds, so a live check can script any tool's calls (`task_create`, `wake_at`, …); a tool's result gets `Done.`.
  - `discord` (`src/discord_cli.rs`): `proof`, kl8m's steps against a real daemon (`src/discord_proof.rs`); and
    for a live check across processes, `rig` (a scratch daemon's config, fake `op`, and bindings), `model` (the
    proof's scripted model), and `say`, `press`, `read` against a running `fake-discord`.
- `src/lib.rs` exports `fake_discord`, `fake_gateway`, and `discord_proof` for other crates' tests.
  `src/fake_model.rs` stands in for the Messages API for the bench.

## Invariants

- **The budgets are §9's, and they don't move to make a gate pass.** A margin is the measured noise of a phase's
  p95 on the build machine, and changing one is a decision with its data (theseus-zay1).
- **New startup work lands with its bench row**, so the gate times it.
- **The plain turn's frames budget only goes down.** A step that writes fewer frames lowers `PLAIN_TURN_FRAMES` in the
  same commit (C6 and S5 aim at 2). A step that must write one more raises it on purpose, with the reason in the
  commit, as `tests_m3::a_plain_turn_stays_within_its_frame_budget` is raised. The bench holds it at the daemon, over
  the protocol; the test holds it inside the core.
- **A new kernel transition belongs in kernel-sim's random operations**, with any invariant it must keep.
- The fake Discord never records a header, so no token reaches its log; its gateway never keeps what an IDENTIFY
  or a RESUME carries, and an interaction's token is cut out of a recorded path.
- Its payloads are checked against twilight-model's own types (a dev-dependency): a shape the binding's model
  can't read fails here, not as a silent drop in a binding test.

## Tests and use

- `tests/sim.rs`: the crash test and the kernel simulation on fixed seeds, in the gate.
- The Discord proof is in the gate through theseusd's `tests/discord_proof.rs` (it needs the daemon's binary).
- Release numbers of record: `target/release/theseus-sim bench lifecycle --theseusd target/release/theseusd --runs
  10`, with `--sessions 10000` for the synthetic store, or `--store` on a copy of a real store.
- Long runs stay out of the gate: `theseus-sim kernel-sim --seeds 40`, or a crash test with `--restarts 8`, so
  stores cross checkpoints; `bench idle` (30 s), and `bench size`, which needs a release build.
- The turn bench's numbers of record are release's: `target/release/theseus-sim bench turn --theseusd
  target/release/theseusd --runs 10`.

## Traps

- With ten runs, a p95 is the slowest run, so one stalled fsync decides it. The gate reruns a miss once; read
  `bench history` before calling a miss a regression.
- A raced kernel-sim run reproduces from its seed only up to its first race.
- A bench of release binaries while a build runs: copy them first, since cargo replaces them mid-run.
- `bench turn` counts every frame the WAL gains from just before a turn until it has been still for 50 ms, so a frame
  another writer put in that window counts as the turn's. The gate reruns a miss once; a regression writes its frame
  every time, and the output names each frame by what it holds.
- A tool-call turn writes 9 frames since Tier 7.1 (theseus-kpfv), held by `--check` as the plain turn's 5 is: the
  turn takes its job's completion itself, with its result, in one frame. It wrote 10 to 12 before, as the drain
  usually took the completion first.
- The stand-in model (`fake_model::FakeModel::start_mixed`) asks for its tool only when a turn's input holds
  `fake_model::TOOL_MARK`; the lifecycle bench's `start` asks on every call.
