# CLOUD_REPORT: threadbench (T0), theseus-9ofg

Branch `cloud/20261013-threadbench`, on main 9fa84735 (store format 27; no store or Rust change here: Python, a plan JSON
and Markdown only). Started 00:22 UTC, 2026-10-14; the report is written the same morning, well inside the 5 hours.

## What was built (one step, five commits)

| Commit | What |
|---|---|
| `46276c1` | `bench/recall/drive.py`: the Theseus and Claude Code drivers resume a session a progression returns to (A5 and A6 need it); Pi's loop is untouched (A7 is one session). |
| `125a795` | `bench/threads/`: the format and validator, the six fixtures with their end-state oracles, the beats, the generator. |
| `da36e36` | the drivers (arm table, layouts, pins, plan check, A8 probe, threads-arm refusals), the scorer, the cost model, the stand-in rules, the persona and prefix tracks, the report draft, the README, `bench/plans/threadbench-t0r.json`, the stand-in smoke. |
| `2fe7b00` | model calls and first token read from each harness's own raw answer; a stand-in service the local-ops arm leaves running is stopped by pid; more tests. |
| `7af2d58` | no cue or persona address on a stream's first message (retuned rates; plan digests moved); the report names the arms it could not run. |

Nothing is committed under `docs/benchmarks/`, `docs/spec`, `docs/status.md`, the root README or `docs/design/`. `bench/README.md`
gains one table row for `threads/` (the maintainer may move it).

### What it is, in the brief's order

1. **Format and validator**: `threads-progression-v1` (`th_format.py`): each message has text, true stream, expected persona,
   cue or none, gap, day (and `ambiguous`, `named`, `switch`, `correction_of`, `reopen`, the files that arrive `before`,
   the reference effect `ref`); facts are introduced, superseded and probed; the validator refuses a probe before its fact,
   a fact probed twice, a value said outside its own message, a bad check, an embargo probe before the embargo is stated.
2. **Generator** (`th_generate.py`, `th_beats.py`, `th_fixtures.py`): six streams, each its own fixture workspace
   (`ledgerweb/` sealed, `datatidy/`, `tarnlib/`, `quillhn/`, `homesvc/`, `home/`), each with an oracle that runs the work.
   Facts collide by design (8 kinds, every stream the same kinds, ports 8443/8444/9443/8143/8453, tickets
   SEC-4412/DAT-4421/PRR-4142/HRN-4124/OPS-4214, dates a day apart). Interleaving: Markov, stay 0.64, runs 1 to 8;
   gaps log-normal (median 6.2 min, sigma 1.711 so 55% exceed 5; clipped at 90 min within a day), three days with a
   10 to 14 h gap at each boundary; cues on 38 to 42% of messages (mean 42% over seven seeds), ambiguity 9%, persona
   switches 22%, named personas ~9%. Standard size: 143 owner messages, 43 facts, 47 questions (16 direct, 10 indirect,
   12 cross, 4 embargo, 3 abstention, 2 misroute) and ~100 label probes (66 routing, 33 persona, 3 corrections): the brief's
   "~60 probes" is 47 asked, because a fact is probed once and the streams hold 41 probe-able facts.
   **Held-out seed 29** beside the development seed 11 (both pinned by digest in the plan).
3. **Probes**, each scored (`th_score.py`): in-stream direct and indirect, cross-stream (value and source), embargo
   (the probe's file, plus a scan of every other stream's files), abstention (harness-dev has no due date; home has no
   ticket or port), routing (labels; scored from a `routing.jsonl` an arm writes), misroute recovery (ambiguous note,
   then "that NOTES.md line was for the security fix, not the PR"), persona and persona correction (labels).
4. **Drivers** (`th_drive.py`): A3, A4, A5, A6, A7 through `bench/recall/drive.py`'s classes; A8 by `--probe-opencode`
   (`opencode run --help` for `--session`); A1, A1n, A1p, A2 have `requires = "threads"` and refuse with words. Claude Code
   and Pi are held to the Harbor arms' pins (2.1.290, 1.0.4; `--allow-version` to run another, said in the report).
   `--plan` holds a run to a committed plan (`bench/harbor/plan.py`'s rule) that names the arm and pins the
   progression's digest.
5. **Scorer and cost model** (`th_score.py`, `th_price.py`): everything in the brief. Modelled dollars: one rule for every
   arm (a session's cache lives one TTL; a gap since the session's own previous message over it re-writes the first
   request's cache reads; 5 min at the catalog's write price, 1 h at twice the input price), with the first-request share
   taken as reads / calls.
6. **Persona track** (`th_persona.py`, `packs/persona.v0.toml`): five personas, ~57 tokens per card, the draft question
   modelled on role.v1 (its `inbound` builder, the cards as `roles`), a stand-in answerer, three baselines.
7. **Prefix track** (`th_prefix.py`): two family streams reopened after an 18 to 20 h gap, a near-miss sibling, header probes
   with `family`/`sibling` values; the sibling's value is a false fit, counted apart.
8. **Report draft** (`report.py`): `docs/benchmarks/<date>-threadbench-<slug>.md/.json/.csv` and light/dark SVGs, in the
   order asked, with "Where threads would lose" and the T0r section with the exact command lines in the last section.

## How it was proved

All commands from the repository root.

- `python3 -m unittest discover -s bench/threads`: **50 tests, OK** (18 generator/oracle, 19 scorer/price/persona/prefix,
  13 drivers/smoke; about 105 s). The Theseus smoke needs the debug binaries (`cargo build --workspace` ran here).
- `python3 -m unittest discover -s bench/recall`: **87 tests, 1 error**, `test_drive.TheseusDriver.
  test_mains_sizing_fails_the_mark_on_the_counting_stand_in_as_it_did_live` (`no context.compiled row after the first
  turn`). It **reproduces with `bench/recall/drive.py` unchanged** (checked out from `HEAD~` and run alone): the
  test's 35,000-token window is most likely too small for what the daemon's current system prompt and tools leave (a plain first
  request here is already about 16,100 tokens); I did not dig further. It is a stale test on this base, not this lane's. Without the binaries the same
  suite is 87 OK, 7 skipped.
- **Every stream's oracle scores 1 on the reference effect and 0 on a planted wrong one** (`test_th_generate.Oracles`):
  the old handler restored (sec-fix), the summary at the old cutoff (pipeline), the old `retry` (pr-round), the flag
  without the `--top 0` fix (harness-dev), the unmigrated config (local-ops), a wrong recipe (home); each moves only
  its own stream's score. The start scores 0 everywhere. A sealed marker in `tarnlib/PR_DESCRIPTION.md` is found by the
  leak scan and fails that stream's oracle, not the sealed stream's.
- **The stand-in smoke** (`test_th_smoke.TheseusSmoke`): A3 and A5 end to end on a scratch daemon with
  `bench/recall/standin.py` counting like a provider, 5 streams, 41 messages, 11 questions; every turn exit 0, every fact
  delivered, every oracle 1 (relative to the ideal arm's work on the smoke), 0 contamination, 0 leaks, every probe
  right, nothing left running; A3 one session, A5 five; A3 mean request 21,523 tokens (p90 25,660), A5 17,064
  (18,337); then the scorer and the report render into a scratch directory, and again from `scores.json` through
  `report.py`'s command line. Also: A6/A4 on a stand-in `claude` (sessions carried per stream; `--session-id` once, `--resume`
  after), A7 on the recall bench's stand-in `pi`, and the prefix track on A6 and A4 (5 sessions and 2).
- **Planted reverts** (each restored and `touch`ed; `git status` clean after):
  - session revisit in `bench/recall/drive.py` removed: A6 `rows[1:]` not all `--resume` (CC test) and A5 `20 != 5` sessions
    (Theseus smoke) fail;
  - re-pricer ignores a miss: both `Price` tests fail (hand-worked: a turn at 14,000 warm, 25,500 after a 5 min miss,
    17,000 / 36,000 at the 1 h write);
  - contamination never found: the collision test fails;
  - validator's "after its fact" check off: the refusal test fails (after its first version did NOT, which showed it
    was guarding another rule; the test now asks a message early and names the reason);
  - leak scan blind: the oracle leak test and the embargo-probe test fail;
  - cross probes asked in-stream: the placement test fails;
  - calls read as tool calls + 1 instead of `loops`: the Theseus smoke's growing-curve assertion and the calls test fail.
- `python3 scripts/keel-guard.py` before each commit: `keel: ok (... 21 files changed, 0 findings acked, 0.0 s)`. No test,
  assertion, cap or ceiling was deleted, lowered or raised.
- Persona track on the stand-in (seeds 11 / 29, 143 messages): the oracle 100%; the thread prior 57% overall and **8% / 3% on the 36 / 31
  switch messages**; the cards-keywords baseline 52% / 57% overall, 58% / 55% on the switches; the stand-in answerer (20% seeded noise)
  82% / 85%. These prove the plumbing, not Jev.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (before the first commit, and again on the final tree):
Run three times: before the first commit (gate1), on the final tree twice (gate2, gate3). No Rust, store or config changed in this
lane, so what fails is what fails on the base. **gate3, the final tree: fmt, shape, features, clippy, cockpit, test build,
the reader rule and the keel phase pass; the suite fails on 34 tests, none of them this lane's:**

- **33 known L1 tests** (the VM runs as root, theseus-pv6i): theseus-sandbox `bench::spawn_100`, 19 `contract::*` and 13
  `theseusd::sandbox::*`.
- **1 not on the known list: `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`.**
  It fails alone too (`cargo nextest run -p theseus-discord <name>`: 21 s, "timed out: the thinking's create, held, and the reply's
  post"), on a tree whose Rust is the base's. It reads as a timing test on a slow 4-core VM; I did not dig further. The maintainer
  should run it on the owner's machine.

Then the later phases by hand (the suite phase failing only on the above): `protocol types` (no diff under `cockpit/src/protocol.gen`: ok)
and `deny` (`cargo deny --offline check`: advisories, bans, licenses, sources ok). The lifecycle, jobs and turn benches are skipped by
`THESEUS_GATE_NO_BENCH=1` as the task says. **gate2 failed in 154+ more tests for a reason that is not the code's:** the disk allowance
was spent (a 27 GB `target/`, 15 GB of it `debug/incremental`) and the daemon refuses to start a job below a 1,024 MB floor ("the disk under the
state dir has 270 MB free"). I deleted `target/debug/incremental` (a cache) and reran: gate3 above.

## Live checks for the maintainer (exact commands)

1. **The attempt plan and the pins**, on the owner's machine with the pinned builds installed: `python3 bench/threads/th_drive.py
   --plan bench/plans/threadbench-t0r.json --arm A4 --progression /tmp/tb-prog --out /tmp/x` must refuse here until the
   plan is merged and clean ("is not committed"), then run only when `claude --version` says 2.1.290 (this VM has 2.1.296: the
   driver refuses and names both).
2. **A real smoke**, one arm, about a dollar: `python3 bench/threads/th_generate.py --seed 11 --size smoke --out /tmp/tb-smoke &&
   python3 bench/threads/th_drive.py --arm A3 --bin-dir target/release --progression /tmp/tb-smoke --out /tmp/tb-live-a3
   --spend-limit 5 && python3 bench/threads/th_score.py /tmp/tb-live-a3 --out /tmp/tb-live-scores`. It should exit 0, leave
   `run.json` `left_running: []`, and print oracle, contamination and measured vs modelled dollars. With a real cache the modelled
   5-minute dollars exceed the measured ones (the smoke has long gaps). Read `scores.json` `probes` for any miss, and
   `usd.modeled.*.misses`.
3. **The persona question on Jev**: `python3 bench/threads/th_persona.py --seed 11 --out /tmp/tb-persona --dump-inputs
   --jev-probe target/release/jev-probe --pack role.v1`. The parse of `jev-probe`'s printed answers is best-effort and unverified
   (a regex for persona id and probability): read `/tmp/tb-persona/raw/persona-000.out.txt` first and fix the parse if the
   format differs. `role.v1` stands in for the draft pack, which `jev-probe` cannot load until it is embedded (`pack::by_name`
   reads embedded packs only).
4. **A8**: `python3 bench/threads/th_drive.py --probe-opencode` on a machine with OpenCode 1.18.35 installed: `measurable` with
   `--session`, or "not measurable" and why. (Not installed here: it says so.)
5. **Claude Code against the stand-in model** (the recall README's by-hand check), to see its flags and sessions on the real
   binary: `ANTHROPIC_BASE_URL=http://127.0.0.1:<standin port> ANTHROPIC_API_KEY=stand-in python3 bench/threads/th_drive.py
   --arm A6 --claude <claude> --allow-version --progression /tmp/tb-smoke --out /tmp/tb-cc` against `bench/recall/standin.py` serving
   `th_standin.rules_for(<smoke>)` with rules naming Claude Code's tools. Expect the same few repeated-tool-call timeouts the
   recall README names.

## Design choices and uncertainties the owner should hear about

- **The arms A1, A1n, A1p, A2** are declared as the brief lists them but their meanings are my reading (A1 threads routed by a
  router; A1n the owner names the thread; A1p with personas; A2 with the prefix library). If they mean something else, the table in
  `th_drive.ARMS` and the README are the two places to change.
- **Arm colours**: charts.py's registry has no slot for A5/A6, so they take `theseus-batching`'s and `openclaw`'s slots, named in
  each legend by the arm they are (README says so). A new arm would be a palette change and a palette validation; left alone.
- **Windows**: Theseus's scratch config sets the catalog window to Sonnet 5.5's 1,000,000 and an output cap of 16,000 (the recall bench's
  `output_cap`), so a 143-message run is unlikely to compact on any arm; Claude Code runs with its own default compaction (`marks`
  with no marks). If T0r should force compaction, the window is `--context-window`.
- **`left_running`**: the local-ops stream makes an arm start a stand-in service; the drive stops it by pid (found by its working
  directory, never by name) and records it in `run.json` `stand_in_services_stopped`.
- **First-token latency** exists for Theseus only (`first_token_ms` in its turn result); Claude Code and Pi report none, so their
  cells say so. Harness CPU and peak RSS are `getrusage` of the driven children (the Theseus daemon included), not the Harbor
  sampler's per-process classes.
- **The prefix track** measures what a fresh session already knows; with no prefix library built it has no arm that reuses a
  header. Its header probes are a family-vs-sibling test of false fits, as the brief describes; it is one small progression
  (16 messages, 6 questions), enough to prove the shape, not to rank arms.
- **The routing probes** are labels on messages (the true stream, ask on ambiguous); they are scored only from a `routing.jsonl` a
  threads arm writes (`{msg, stream, asked}` per message). That file's shape is this bench's proposal for the threads build.
- **The cost estimate** ($45 to $75, pause line $170) is the brief's figure; the smoke's stand-in dollars ($2.41 and $1.92 for 41
  messages at Sonnet 5.5's list price with no cache) suggest the 143-message run is near the upper end per Theseus arm without
  cache and well under it with one. No real arm was run.
- **The one change to a shared file** is `bench/recall/drive.py` (session revisit in two classes, 15 lines) and one row in
  `bench/README.md`. The pre-existing recall test failure above is not touched.
- **Docs to change at the merge** (not done here): the spec's Part III item and `docs/status.md` for theseus-9ofg; a
  `docs/benchmarks/README.md` row when T0r's report lands; if the owner wants the palette extended, `charts.ARMS` and its README table.
