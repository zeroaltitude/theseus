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
    in `kernel_sim/wakes.rs`, a turn's moves of them (`wake.move`, theseus-lherl: a series' next occurrence moved off its grid until it is taken), and crashes that keep the daemon down for minutes) and invariants checked at every
    step. `--p-race` races a second thread against turns; 0 is fully deterministic. sim2 (theseus-celu.35):
    `/stop` between turns, in one, beside a raced one, and of one call (`kernel_sim/stops.rs`); tasks under a
    parent, their carve and reports (`kernel_sim/tasks.rs`); and the outbox's posts, sent by a fake binding to a
    fake channel with crashes around each transition (`kernel_sim/outbox.rs`). Every frame that moves an execution
    to queued holds its `execution.queued` row (a start's requeue its `execution.interrupted`), read from the kernel's
    observer (`kernel_sim/queues.rs`, theseus-2xep). Each operation goes in a module of
    its own; `kernel_sim.rs` holds the roll, the race's arm, and the checks' calls. tests/sim.rs reads its coverage
    counts from the `--p-race 0` run only, since a raced run reproduces only up to its first race (theseus-81ig).
  - `bench lifecycle` (`src/lifecycle.rs`): §9's budgets on a real `theseusd`: cold start, the same from a vault note's
    copy, clean shutdown with a job running, the same with a reply's post in flight to the in-process fake Discord
    (`inflight`, its own rig), SIGKILL and restart, a binary swap with the job's wrapper adopted, restore (with
    `theseusd restore`'s own phases), and the push's seed. A cancel's round trip too (a real `proc.run` job cancelled
    through `execution.cancel`, its frames a cancel printed; theseus-nh1k, theseus-dwoj), and the restore phase runs
    before it, since a phase that writes into the rig runs after the rows that read it (theseus-ma8r). `--check` fails a
    p95 over its budget plus the phase's margin (`lifecycle::margin_ms`, measured on the build machine; the seed's and
    health's since theseus-m1r2w, from 44 runs), and a run
    whose fewest frames a cancel are over `lifecycle::CANCEL_FRAMES` (2; its most is printed, not judged, since under
    load it picks up other work's frames; theseus-kq4n). The cancel's budget is 100 ms since cancel-fast.
    A restart after a clean stop must not repair the store's index (`Start.index_repaired`, from the store phase): the
    shutdown phase fails on a repair after each `shutdown`, and after one SIGTERM, sent once the MCP server listens,
    which is the stop a service manager sends (theseus-7okz9: a repair of a big index is seconds before serving).
  - `bench lifecycle --sessions N --phases surfaces` (`src/lifecycle/surfaces.rs`, theseus-7bee): what a prompt waits
    for on a store with a great many sessions: `theseus watch --interactive` from its spawn to its prompt line, against 50 ms
    at any size. The TUI's startup reads (`executions.watch`, `confirm.list`) print beside it with no budget: at 200,000
    sessions they took p50 330 ms (the push's seed, health-o1's), to be budgeted once it is O(1).
    Not in the default phases (the gate's run is on an empty store). It waits first for the index's shape, since until
    it is built `session.list {n: 1}` reads every session.
  - `bench turn` (`src/perf.rs`, theseus-goa8): a plain turn and a tool-call turn on the stand-in model, each run N
    times on one warm session: wall time by the bench's clock and the daemon's, and **frames per turn**, counted from
    the daemon's WAL by `src/walcount.rs` (a read-only tail; the daemon reports no frame count, and the core is not
    changed for a bench). A frame is one `fdatasync`, so frames are §9's per-turn overhead in a unit that does not
    depend on the disk. `--check` fails a plain turn that writes more than `perf::PLAIN_TURN_FRAMES` (4; the floor is
    2), or whose trace counts more syncs before its model's first byte than `perf::PLAIN_SYNCS_BEFORE_CALL` (2,
    `syncs_plain`; theseus-2uby). A third kind, `batch` (theseus-d1hi), is one response of four `proc.run`s of `sleep 2`
    (the stand-in's `bench-batch` mark), three runs: `--check` holds its frames to `perf::BATCH_TURN_FRAMES` (14: the
    tool turn's 8 and two for each call past the first) and its wall time at the p50 to `perf::batch_wall_budget`: one
    sleep, the tool-call turn's own p50 (measured beside it, so the load is in both), and `BATCH_OVER_ONE_CALL_MS`
    (300), which four programs run one at a time miss by six seconds. Its daemon answers jobs in their turn for 10 s
    (`proc_sync_secs`; the lifecycle bench's 1 s would put the sleeps in the background). Beside them: this disk's
    `fdatasync`, probed before the daemon starts and after it stops (the quieter is used, so the harness's own share of
    a turn reads off as an upper bound), and the daemon's resident memory after the start
    and after a burst (with `--check` and a burst, `rss_burst` more than 10% over the median of the last five recorded
    runs prints a `WARN` line and records the percentage in the `rss_burst_over` column; it never fails, theseus-2m7jg).
    Each run also gets a line (`src/perf/runs.rs`, theseus-w7dk): its wall, the daemon's time, its
    frames, and the slowest frame the store answered since its input arrived, with that frame's time and records (the
    turn's trace carries it as `attrs.slowest_frame`); a run over twice its kind's p50 is flagged. The turn's last
    frame, `end_turn`'s, is written after the trace takes it, so a stall there reads as a long wall with a small slowest
    frame, not as time outside the frames (theseus-67nz). The scratch daemon has Discord, the web UI, and the judge off.
    `--judge` (theseus-0j2.8, `src/perf/judge.rs`) measures the judge's cost beside it: three arms, each a scratch
    daemon (the judge off; on at theseus-judge's fake Jev, in process, with the inbound point's packs off; and every
    pack as wired, route.v1 waiting on the fake's verdict), each frame told the judge's (every record a `judge.*` or
    `pack.*` row or a `judge.*` META record; `walcount` labels a META record by its key) or the turn's (checked against
    its trace), each judge frame placed before a turn's answer, after it, or between turns, and the blobs the store
    gained (two syncs each, which the WAL never sees). Each judged arm first submits a turn the moment its fresh daemon
    answers and counts the ladder's `pack.mode` frames around it (theseus-289c: none since the warm read's adoptions
    wait between turns). Each run of every arm gets the per-run line too (theseus-v2ru): the turn's own frames, where
    the judge's fell (before its answer · after · between it and the run before), and its slowest frame, named the
    judge's or the turn's. Nothing new is gated: `--check` judges the judge-off arm, and the judged arms' wall times and
    frames go to `--record` under columns of their own (`*_jloop`, `*_jpacks`). **The head-to-head's counted rows** (`src/perf/counted.rs`,
    theseus-7gir.13), judged by `--check` beside the frames, counts and not times (the owner's D-6): the syncs a warm
    turn's WAL gains before the stand-in's first byte (counted by the stand-in from the WAL as it is about to send;
    `SYNCS_BEFORE_FIRST_BYTE`, 2, `PLAIN_SYNCS_BEFORE_CALL` since theseus-2uby, only goes down), a fresh session's first request in tokens as sent, per kind of place (a second
    scratch daemon of the default config, `context.census`'s total for `cli`, `task`, `dm`, `channel` and `headless`; at most
    `theseus_core::first_request::budget`: 8,000 in the cli and task places, 15,000 in the DM's, and the channel's and the
    headless run's own, theseus-to0xw; its bytes, and the bench config's, are recorded, not judged), and no timer
    between a delta and its write (a paced stream of 8 chunks reaches the client as 8 `model.delta`s, and in
    lockstep each chunk reaches it alone). A delta's way to the client is recorded. Each kind's harness overhead
    (`src/perf/overhead.rs`, theseus-4w1h: the turn less its model and its tools, the commits included, split into
    the commits, the compiles, the admission and the rest, the speed wall's split) is printed and in `--json`. `--session-nodes N` (step 33,
    `src/perf/long.rs`) measures turns in one session of N nodes instead, written before the daemon starts
    (`synth::long_session`: five-node exchanges whose tool results are `--result-bytes`, 8192 by default): each turn's
    wall time, frames, and the nodes the daemon decoded for it (health's `store.node_cache` `decodes`, read around the
    turn; a build before tiering has none, and the row says so), and memory with the index tender's. No budget: its
    frames include the turn that compacts the session. A list (`--session-nodes 2000,5000,10000`) runs each size on a fresh daemon,
    and `--first-turn-ms N` exits 1 when a first turn passes N (theseus-0l3g). `--soak-turns N` (`src/perf/soak.rs`)
    grows one session by N turns, every fifth with a call, and reads the daemon's CPU (`utime`+`stime`) and RSS every 50:
    `--check` holds the last window's CPU a turn to 1.5× the turn-100 window's and RSS growth to 10 kB a turn; after
    the turns it waits 12 s and reads RSS again, after the daemon's trim, which tells memory held from memory kept.
  - `bench idle`: an idle daemon over a window (30 s): CPU time, wakeups (its threads' voluntary context switches, from
    `src/procfs.rs`), frames written (a quiet daemon writes none), and memory, on an empty store or `--sessions N`.
    `--settle N` waits up to N s (default 60) for the daemon to go quiet before the window begins, and shows the CPU in
    each ten seconds meanwhile, so a daemon that never goes quiet shows whether it is slowing. Measured, no budget yet:
    an empty store is quiet (5 ms of CPU in 30 s, 4.5 wakeups a second), and at 10,000 parked sessions a release build is
    still at 5.8 % of a core 300 s after its first answer, writing no frames (review 2's S1). The index tender's memory is
    read beside the daemon's (the `theseus-index` child), and `--active N` then opens N sessions and runs a turn in each
    and reads both again: with `--sessions 10000 --active 50`, design M6 §2.10's row (§9: under 1 GB together).
  - `bench size`: the shipped binaries' sizes. Meaningful on a release or install build. §9's 60 MB is a target:
    printed with its gap when missed, and failing nothing. `--check` fails on **growth** (`src/perf/budgets.rs`,
    theseus-2m7jg): a binary more than 1 MB over the last size recorded by a run that passed, read from the history
    (`--history`, else `--record`'s file, else `$THESEUS_BENCH_HISTORY`) among rows whose label has the word
    `--baseline-tag` (`size:release`, so a thin build isn't judged against a fat one). With no baseline the run becomes
    it. `--accept-growth` records an accepted growth as passed. `scripts/build.sh` runs it after a build, and the gate's
    size phase runs it (`scripts/size-step.sh`) when a build of the tree under test exists.
  - `bench status` (`src/perf/status.rs`, theseus-lweh): what a prompt's `theseus status --short` costs, the CLI's whole run
    (spawn to exit) against a scratch daemon on a synthetic store (`--sessions 1000`), p50 and p90 over `--runs` (300).
    `--check` holds p90 to 5 ms: meant for a release-thin build on the owner's machine (a debug build on a 4-core VM
    reads p90 about 6 ms at 1,000 sessions, 4.9 at 10, where the process's own start is about 3.7). Outside lifecycle's
    gated phases; `--record` writes the `status_short` column.
  - `bench prompt` (`src/perf/prompt.rs`, theseus-7m7w.1): a fresh session's first request in tokens for each kind of place,
    by `context.census` on a scratch daemon of the default config (`cli`, `dm`, `task`) and of the benchmark's profile
    (`harness`), against the committed `baselines/prompt.json`. It **warns** (stdout and stderr) at more than +5 % over a
    place's baseline and never fails: other rows add tools every week. `--update` rewrites the baseline, for a change you
    mean (the commit says why). Nothing is sent to a model.
  - `bench agenda` (`src/perf/agenda.rs`, theseus-nuhsz): `work.agenda` at its 200 lines: a scratch daemon holding
    `--reminders` (90, half of them daily series) and `--routines` (3, hourly), the call timed over `--runs` (300) for a week's
    window, with `status.short` as the floor beside it. `--check` holds p90 to 5 ms: meant for a release-thin build on the
    owner's machine (the debug build on this 4-core VM reads p50 8 ms of which the floor is 0.6). Outside lifecycle's gated phases;
    `--record` writes the `work_agenda` column.
  - `bench concurrency` (`src/perf/concurrency.rs`, theseus-4g0l): M task turns (`--turns 8,16,32,80`) submitted 5 ms
    apart, then a person's turn 0.5 s after the first, on a stand-in that holds each call `--hold-ms` (2000): total
    wall, each turn's wall and admission (its `admission.wait` span), inversions inside the tasks' class (admitted and
    ended), the person's rank by admission and by end, failures, and the daemon's peak RSS and CPU. `--stuck-secs`
    sets `[kernel] admission_stuck_secs` below the queue's length. `--check` fails an admission inversion, a person's
    turn admitted behind more than the turns admitted before it arrived, or any failure. No history column.
  - `bench history` (`src/history.rs`): each phase's recent runs and headroom, from the CSV every gate appends. The
    other benches' columns (`history::OTHER`) are in the same file, each with its unit.
  - `synth-store` (`src/synth.rs`): a store of parked sessions, for `bench lifecycle --sessions N`.
  - `fake-discord`: a stand-in for Discord's REST API, and with `--gateway` its gateway (theseus-6g62); `--guild`
    gives it a guild for the viewer check (theseus-ck0k). Since theseus-sjzm (`src/fake_discord/threads.rs`) it makes threads
    (`POST /channels/<id>/threads`, a forum's with its first message; `set_forum`), archives, locks and renames them
    (`PATCH /channels/<id>`), lists and makes webhooks (`refuse_webhooks` answers 403, as a bot without Manage Webhooks
    gets), runs and edits a webhook's messages (`Msg::hook` is the name, `author` the webhook's id), types a reply
    (`say_reply`) and a slash command (`command`, in a thread under a `parent`).
    gives it a guild for the viewer check (theseus-ck0k).
    Since theseus-bh3t.1 (`src/fake_discord/channels.rs`) it makes guild channels and categories (`POST /guilds/<g>/channels`),
    changes (`PATCH /channels/<id>`, a made channel's name, parent, topic and overwrites; any other goes to the thread routes) and
    deletes them (`DELETE`), keeps Discord's 50 to a category, and answers 403 to all three under `refuse_channels`.
    It holds Components V2 messages to Discord's own rules (theseus-2j2ka; `v2_refusal`, written apart from the binding's
    validator): at most 40 components (boxes, sections and accessories counted), 4,000 characters of text, no
    `content` beside `IS_COMPONENTS_V2`, unique `custom_id`s, V2 types only with the flag, the flag not removable on an
    edit; a refusal is a 400. A V2 message's `content` is its text displays joined, `buttons` lists the buttons and
    selects inside boxes and sections, and `components` counts the rows (so a card whose buttons are gone reads 0). A
    select is pressed with `press_values`; a message the binding answered an interaction with (an ephemeral view,
    type 4 or 7) is kept by interaction (`reply_message`) and its buttons are pressed with `press_reply`. The
    gateway records each presence update (opcode 3) with its time (`GatewayState.presences`).
  - `fake-model --rules <file>` (`src/fake_model.rs`, 37b): a scripted stand-in for the Messages API, for a scratch
    daemon's `api_base`: each turn's last user text takes the first rule (`{when, calls: [{name, input}], text}`) it
    holds, so a live check can script any tool's calls (`task_create`, `wake_at`, …); a tool's result gets the
    rule's `then`, found by the turn's prompt, else `Done.` (theseus-qy2a).
    A rule's `hold_ms` holds its answer that long, each connection on its own thread, so a live check can find a
    provider call in flight (theseus-f3wr). A rule's `repeat` (theseus-3ki0) keeps one turn going: while the request
    holds its `when` (a pinned instruction does), each tool result asks for its calls again, `{n}` in their input's
    strings the count so far (three digits), until it has asked `repeat` times; then its text.
    The head-to-head's stand-in (theseus-7gir.13, `src/fake_model/`): one
    model for both harnesses. A rule's `steps` script a whole turn, matched on the turn's opening user text (the last
    user message with a plain text block: Claude Code sends
    `<system-reminder>` blocks, which never open a turn); step k is the k-th request the rule answers for the run's
    marker (Claude Code merges its messages by role, so a turn's results can sit before its prompt), or, with no
    marker, the request after k user messages of tool results;
    `{marker}` is the word after `marker=`. `--ttfb-ms`, `--chunks`, `--chunk-ms` (or a rule's own) pace every
    answer: its first byte that long after the request, then its deltas one write each, apart, with `TCP_NODELAY`;
    HTTP/1.1 keep-alive, chunked; a request with no tools is a side request; `"stream": false` gets the message
    whole. `--log` writes one JSON line a request, its times on `CLOCK_MONOTONIC` (`serve::Entry`). `bench/h2h/`
    drives it.
    provider call in flight (theseus-f3wr).
    A rule's text may hold `{handle}`, the first recalled handle in the turn (`m:a1b2c3`), so a live check has the
    model cite what its recall gave it (theseus-ejdk).
  - `discord` (`src/discord_cli.rs`): `proof`, kl8m's steps against a real daemon (`src/discord_proof.rs`); and
    for a live check across processes, `rig` (a scratch daemon's config, fake `op`, and bindings), `model` (the
    proof's scripted model), and `say`, `press`, `react` (a person's reaction: the gateway's `MESSAGE_REACTION_ADD`, in a channel or
    the user's DM; `/_fake/react`, `fake_discord/reactions.rs`), `read` against a running `fake-discord`.
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
- A tool-call turn writes 8 frames since theseus-2uby (9 since Tier 7.1, theseus-kpfv), held by `--check` as the
  plain turn's 4 is: the turn takes its job's completion itself, with its result, in one frame. It wrote 10 to 12 before, as the drain
  usually took the completion first.
- The stand-in model (`fake_model::FakeModel::start_mixed`) asks for its tool only when a turn's input holds
  `fake_model::TOOL_MARK`; the lifecycle bench's `start` asks on every call.
