# theseus-sim

The proving tools, installed beside `theseusd` and `theseus` (a tool, so a reader of its own: Part III Item 32).
The gate runs its lifecycle bench, and its crash test and kernel simulator on small fixed seeds (`tests/sim.rs`).

## What's here

- `src/main.rs`: the subcommands.
  - `crash-test` (the M1 exit test): a `worker` appends records and is SIGKILLed at random; each reopen must hold
    every record the worker reported durable, byte for byte. `--tear` also tears the WAL's tail; `--restarts`
    restarts within an iteration.
  - `kernel-sim` (the M2 exit test): the kernel under a virtual clock with seeded faults (a crash between any two
    frames or inside a startup step, lost, duplicate, and late completions, cancels) and invariants checked at every
    step. `--p-race` races a second thread against turns; 0 is fully deterministic.
  - `disclosure` (M4 19b, `src/disclosure/`): a whole core in process (`rig.rs`: an unsynced temp store, the sim's
    model as its provider, stand-ins for `http.fetch` and `proc.run` over the built-ins) in a seeded world of people,
    guild channels whose viewers change (mid-turn too), DMs, the CLI, files in a private and a public tree, context
    files, tasks, graduations, held posts, trusts, and the test-only foreign node (another session's node with its
    label, standing in for M6's recall). The sim plays the binding (it pushes who views a channel and delivers
    posts with the binding's check at post time) and the driver (one continuation at a time). Every piece of
    content carries an atom, `zq<n>qz`; the model repeats every atom its request carried (`model.rs`); the oracle
    (`atoms.rs`) judges each atom by its own copy of §2.5's rules, never by the core's labels. `--strict` also fails
    on the known gaps (`atoms::KNOWN_GAPS`), which a run otherwise counts.
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
  - `bench idle`: an idle daemon over a window (30 s): CPU time, wakeups (its threads' voluntary context switches, from
    `src/procfs.rs`), frames written (a quiet daemon writes none), and memory, on an empty store or `--sessions N`.
    `--settle N` waits up to N s (default 60) for the daemon to go quiet before the window begins, and shows the CPU in
    each ten seconds meanwhile, so a daemon that never goes quiet shows whether it is slowing. Measured, no budget yet:
    an empty store is quiet (5 ms of CPU in 30 s, 4.5 wakeups a second), and at 10,000 parked sessions a release build is
    still at 5.8 % of a core 300 s after its first answer, writing no frames (review 2's S1).
  - `bench size`: the shipped binaries' sizes against §9's 60 MB. Meaningful on a release or install build.
  - `bench jobs` (`src/jobs.rs`, M4 17b): a job's start through the real wrapper, by class: `l0`, `l1`, `l1-egress`
    (18c: its listener and proxy), and `l1-cred` (18d: its credential socket's directory and the helper bound in, with
    the daemon's side, the directory and its listener, timed too). An L1 start's p95 against §2.2's 25 ms (`--check`).
    Give it the classes in palindrome order (`l1,l1-cred,l1-cred,l1`) to see the machine's drift.
  - `bench history` (`src/history.rs`): each phase's recent runs and headroom, from the CSV every gate appends. The
    other benches' columns (`history::OTHER`) are in the same file, each with its unit.
  - `synth-store` (`src/synth.rs`): a store of parked sessions, for `bench lifecycle --sessions N`.
  - `fake-discord`: a stand-in for Discord's REST API, and with `--gateway` its gateway (theseus-6g62); `--guild`
    gives it a guild for the viewer check (theseus-ck0k).
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
- **The disclosure simulator's invariants** (`disclosure/check.rs`, `courier.rs`): every atom a request carries
  may be read by the audience its compile was for, which is the session's; every node it carries whole has readers
  that cover that audience, and its placeholders are as many as the compile withheld; every post's atoms may be
  read by who views its place when it goes (the truth), or the post was held and the owner approved it; no text
  streams into a guild channel unless each atom in it fits any audience the channel can have; every `tool_use` is
  paired; and a session holds external text exactly when T1's sites say (I1). A new way content reaches a model or
  a place (a tool, a post kind, M6's recall) joins its world, with its atoms.
- **A gap the simulator finds is filed, then listed in `atoms::KNOWN_GAPS`** with its issue, so the gate's run
  counts it instead of failing; the step that fixes it deletes the entry. Never widen the oracle to make a seed
  pass: find whether the core or the oracle is wrong, and say which.
- The fake Discord never records a header, so no token reaches its log; its gateway never keeps what an IDENTIFY
  or a RESUME carries, and an interaction's token is cut out of a recorded path.
- Its payloads are checked against twilight-model's own types (a dev-dependency): a shape the binding's model
  can't read fails here, not as a silent drop in a binding test.

## Tests and use

- `tests/sim.rs`: the crash test, the kernel simulation, and the disclosure simulator on fixed seeds, in the gate.
  The disclosure seeds (3 to 6, 30 steps, about 2 s of CPU) are chosen so each planted bug fails in them (19b's: the
  compile filter rendering a withheld message's files; `held_state` reading a waiting question as released; a
  loop's `context.compiled` readers taken from the manifest's prefix). A change to the world or the model moves
  what every seed does, so after one, plant those three again and choose the seeds again if they slip.
- `src/disclosure/tests.rs`: the world generator, the oracle's `covers`, the pairing check, and that a seed
  reproduces its run exactly (its decisions' trace, and every count).
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
- The disclosure simulator reproduces a seed exactly, its node ids aside (UUIDv7): its decisions never read an id,
  only its own counters, names, and atoms. Keep it so: no `HashMap` iteration and no id in a decision or a `note`.
- A disclosure step costs about 20 ms in a debug build, most of it the core's own turn; 2,000 steps take about
  45 s. The live check runs seeds in parallel processes.
- A bench of release binaries while a build runs: copy them first, since cargo replaces them mid-run.
- `bench turn` counts every frame the WAL gains from just before a turn until it has been still for 50 ms, so a frame
  another writer put in that window counts as the turn's. The gate reruns a miss once; a regression writes its frame
  every time, and the output names each frame by what it holds.
- A tool-call turn writes 10 to 12 frames: whether the tool's result is consumed in the turn or by a wake varies. The
  plain turn's 5 never does.
- The stand-in model (`fake_model::FakeModel::start_mixed`) asks for its tool only when a turn's input holds
  `fake_model::TOOL_MARK`; the lifecycle bench's `start` asks on every call.
