# Cloud report: the judgment surfaces, step 23b (theseus-0j2.4)

Branch `cloud/20261004-judgment-surfaces`, cut from `3add3f5` (main as cloned) under the task commit `bbaf16f`.
Started 09:26 UTC, done 10:40 UTC.

| Commit | What |
|---|---|
| `cef79d3` | core: trace marks, the judge's sentences and metrics, `judge.list` / `judge.get`, health's `shed` and `key` |
| `a1db8c9` | cli: `judge log` on `judge.list` (`--pack`), `judge show <id>`, a judgment's mark in `ask --trace` |
| `585e27b` | cockpit: the Judgment section (`/judgment`), each judgment beside its loop in a session's timeline, the fold's step |
| `dbcc848` | docs: theseus-core's and the cockpit's AGENTS.md |

No new dependency. `Cargo.lock` and `package-lock.json` are unchanged. No store format bump: only rows were added
(`judge.resumed`, and `disagrees` and `headline` fields in the `judge.call` row's data).

## 1. The marks (`cef79d3`)

**Found.** 23a spawns `loop.v1` from `TurnRunner::run` after the turn's last frame (`after_turn`). The trace is finished
in `finish` just before that frame, so the mark has to be made there.

**Changed.**
- `theseus_judge::Ask` gains `pub id: Option<String>` (`Ask::new` sets `None`). `Judgment::pending` uses it when set,
  and otherwise mints an id as before (`theseus_judge::new_id`, now public).
- New `judge/mark.rs`:
  - `Dispatch::new(pack, point, mode)` mints the id.
  - `Dispatch::mark(trace, extra)` writes a zero-length span named `judge`, kind `mark`, with attributes `pack`,
    `point`, `mode` and `judgment`, plus extras.
  - `marks(span)` reads the marks back.
- `JudgeService::plan_loop_end` decides the dispatch purely, from config mode and sample. `mark_turn_end` marks the
  trace, adding `loop` and `class` for `loop.v1`. In `turn.rs` this is one call in `finish`, just before
  `trace.finish`.
- `after_turn` reads the marked id back from the trace and spawns the judgment with it. A trace without a mark
  re-plans as 23a did, so the result is the same, because planning is pure. The rest of 23a's loop path is unchanged.
- Cost to the turn: no new frame, no read, no await.

**Proved.**
- `tests_judge_surfaces::a_shadow_dispatch_marks_the_trace_with_the_id_its_row_carries`:
  - the turn result's trace has exactly one mark, `loop.v1` / `loop_end` / shadow, zero length, `loop` 0, `class`
    `reply`;
  - the `turn.trace` row, written in the turn's last frame, carries the same mark;
  - the `judge.call` row's id and its record key are the mark's id;
  - with the judge off there is no mark.
- `a_judged_turn_keeps_its_frames_and_its_request_bytes`: with the judge on and off, the second turn writes the same
  number of frames (≤ 5), and the provider's requests serialize byte for byte the same.
- `judge::mark::tests::a_mark_is_read_back_as_the_dispatch_it_marked`.
- **Planted revert:** `at_loop_end` spawned with a fresh `new_id()` instead of the marked id. The mark test failed with
  `the row is the mark's` (left `jdg_…6a58b…`, right `jdg_…74cfa…`). Restored, touched, `git status` clean, test
  passes.

## 2. Sentences and metrics (`cef79d3`)

**Changed.**
- **Sentences.** The judge's facts (`fact/judge.rs`) now say their lines. The sink says them after its frame is
  written, as the session's and turn's lines, with no notification. Examples:
  - a judgment landing: "Jev, in shadow, judged the stop: progressing (0.95, act band). The baseline ended the turn;
    recorded, not acted on." It adds " Jev disagrees." when it does, a drift note when another model answered, and
    "skipped (shed)" or "the call failed (timeout)" otherwise;
  - the pause: "Shadow judging paused: today's $1.00 is spent. It resumes at local midnight.";
  - the new `judge.resumed` row and line, written at the first reservation of a new local day after a paused day;
  - a booked block, the breaker opening, re-opening and closing, and shedding.
- **Reservations say what they wrote.** `spend::Reserve` now also returns what its records say (`spend::Said`), so
  pause, resume and booked-block lines are spoken only after the reservation's frame is written.
- **The headline answer.** A judgment is described by its pack's deciding Choice or Score (the pack's own verdict
  question: `loop.v1`'s `work_state`), else a deciding Noul, else any whole answer. The row carries it as `headline`.
  Pack questions load from a sorted map, so a plain "first question" would have been `announced_unfinished`.
- **Metrics.** `Telemetry::record_judgment` runs from the sink after its frame. `Telemetry` is now `Clone`, so the
  judge holds the core's pipeline, set at build and again when telemetry is built after serving.
  - `theseus.judge.calls` {`theseus.judge.pack`, `.mode`, `.band`, `.class`}. Band is the headline answer's band, or
    `skipped` / `failed`.
  - `theseus.judge.duration_ms` {pack, class}, only for calls that reached Jev.
  - `theseus.judge.on_path_ms` {pack, class}. I added these two attributes beyond the design's bare name, because
    §2.2 wants p95 per class.
  - `theseus.judge.errors` {`theseus.error.class`}, the provider errors' own key.
  - `theseus.judge.disagreements` {pack}.
  - `theseus.cost.usd` {`theseus.spend` = `judge`, `theseus.judge.pack`}.
- **How I define a disagreement** (`fact::judge::disagrees`, also stored as the row's `disagrees`): the judgment was
  answered by the model the pack pins, and the pack's deciding questions reach the **act** verdict
  (`theseus_judge::decide`). In other words, Jev was sure enough to act and would have done otherwise than the
  baseline. For `loop.v1` that means `announced_unfinished` leaning true in the act band: the baseline ended a turn
  whose last message promised more. An "ask" verdict does not count.

**Proved.**
- `tests_judge_surfaces::a_judgment_landing_says_its_sentence`: the exact line above, as the session's and turn's,
  and `disagrees: true` on the row.
- `telemetry::tests_judge::the_judges_metrics_carry_their_names_and_attributes`: two judged turns through a whole core,
  the second with the fake Jev down, read back from the OTLP test receiver:
  - every attribute key set;
  - the answered and failed call points;
  - duration and on-path counts of 2, with on-path summing to 0;
  - `errors{network}` 1 and `disagreements` 1;
  - the judge's `cost.usd` equal to the row's `cost_micros`.

## 3. The protocol (`cef79d3`)

**Changed.**
- `judge.list {pack?, session_id?, since?, limit}` returns `{scopes, matched, judgments}`: rows without states, oldest
  first, the newest `limit` kept (default 50, at most 500).
  - With no pack, it reads every embedded pack's scope: 7 today.
  - A pack named with its version (`loop.v1`) returns that version only. An id (`loop`) returns every version.
- `judge.get {id}` returns `{judgment, state, state_missing?}`. It looks the row up by key (new
  `Store::ledger_by_key`) and reads the state from the blob the row names (`context.blob`), checked against its
  digest.
- Types are in `theseus-protocol/src/judge.rs`, served by `rpc/judge.rs`. The TypeScript is regenerated.
- Health's judge block gains `shed` (since the client was built) and `key` (`ready`, `resolving`, `failed: …` or
  `not configured`; never a value).

**Proved.**
- `judge_list_filters_by_pack_session_and_time` covers all packs, `loop.v1`, `loop`, `loop.v2` and `security.v1`
  (empty, scope `judge:security`), each session, `since` before and after, and `limit` 1 (newest, `matched` 3, row
  carrying the state's record but not its body).
- `judge_get_gives_the_row_and_its_state_from_the_blob`: the row equals the stored row; the state equals the blob's
  JSON and the state the fake Jev was sent; an unknown id gives `NOT_FOUND`.
- **Planted revert:** `judge.list` ignoring its pack filter (every scope, no version filter). The test failed at
  `loop.v2` (listed when it must be empty). Restored, touched, `git status` clean, test passes.

## 4. The CLI (`a1db8c9`)

- `theseus judge log` now reads `judge.list` and has `--pack`. When the list is cut it says so: "(n of m judgments in
  <scopes>; `--n` shows more)".
- `theseus judge show <id>` prints:
  - header, session and turn, class and baseline;
  - outcome, model, timing, cost, and agrees or disagrees;
  - the state's record, then the state as fields (each cut at 160 characters, saying so);
  - each answer with its band and probability bars, its lean marked.
- `ask --trace` prints a judgment's mark as `loop.v1 shadow at loop_end · reply · jdg_… (theseus judge show jdg_…)`
  with the id whole. The generic span line cut it at 90 characters.
- Health's `judge:` line adds `N shed`, and the key's state when it is not ready.
- Tests: `render::judge::tests::a_judgment_shows_its_state_as_fields_and_its_answers_as_bars` (a full golden of
  `judge show`) and `a_trace_names_a_judgments_mark_whole`.

## 5. The cockpit (`585e27b`)

- **The Judgment section** (`/judgment`, key `j`, a nav item):
  - per pack: mode (from health), version, calls, cost, and p50 and p95 of Jev's time per workload class over the
    judgments read, plus today's counts from health;
  - a judgment log with filters (pack, session, 1h / 24h / 7d), and a link to each session;
  - one judgment: its fields, answers as probability bars with bands, and the state as fields.
- It follows the cockpit's invariants:
  - it reads `judge.list` every 4 s only while open, and `judge.get` once per judgment;
  - its state is in the address (`?pack= ?session= ?since= ?id=`);
  - it keeps no ledger loop of its own.
- **The time machine.** `timemachine.ts` folds `judge.call`, `judge.paused`, `judge.resumed` and `judge.circuit` into
  `World.judge` (calls, failed, skipped, cost, paused, breaker). The section shows the fold's counts and stops its
  list at the moment. `/judgment` is added to `FOLDS`.
- **The session view.** The timeline tab shows each judgment the turn dispatched beside the loop it judged ("loop 1 ·
  Jev (shadow): progressing 0.93 · disagrees", or "judging…" until the row lands). It builds this from the trace's
  marks and the session's own ledger rows, and links to the judgment. The flame chart draws a `judge` mark as a larger
  diamond in the judge's colour.
- **`src/lib/judgment.ts`** is pure and tested by `test/judgment.test.ts`: 4 tests, 27 in the suite.
- **Checks.** `npm run lint` raises no findings in the new files (an impure `Date.now` became `useTick`). `npm test` and
  `npm run build` pass.

## The live check I ran here (no real key)

I ran a scratch daemon of `585e27b`'s debug build:
- fresh state dir `/tmp/live/state`, its own socket, web on port 7499;
- `[judge] enabled = true` against a throwaway Python stand-in for Jev (not committed);
- `theseus-sim fake-model` as the provider;
- `[telemetry] otlp_endpoint` at a throwaway Python OTLP receiver with `metrics_interval_secs = 2`.

What it showed:
- **Three `ask` turns.** The `--trace` one printed
  `@24.6 ms judge [mark] loop.v1 shadow at loop_end · reply · jdg_…29cd3 (theseus judge show jdg_…29cd3)`.
- **`judge log`** listed three judgments.
- **`judge show <that id>`** showed the state's 9 fields and 6 answers with bars.
- **`narrative.watch`** returned the three "Jev, in shadow, judged the stop: progressing (0.93, act band)…" lines.
- **The receiver** got `theseus.judge.calls`, `duration_ms`, `on_path_ms` and `disagreements`, and `theseus.cost.usd`
  with `theseus.spend=judge`.
- **Headless Chromium** (playwright) loaded `/judgment`, `/judgment?id=…` and `/session/<id>?tab=timeline` with no
  console or page errors:
  - the pack row read `loop.v1 · shadow · v1 · 3 · $0.000099 · reply 1.0 ms / 1.0 ms (3)`;
  - the bars rendered;
  - the timeline showed `loop 1 · Jev (shadow): progressing 0.93 · disagrees` and the mark in the flame chart.

I stopped the daemon with `theseus shutdown` and killed the stand-ins by their pids.

## The live check for the maintainer (real key)

```bash
D=$(mktemp -d); mkdir -p $D/projects
cat > $D/config.toml <<EOF
narrative = true
[secrets]
jev_api_key = "op://<vault>/<Jev item>/notesPlain"      # and the provider key as the owner's note has it
[server]
state_dir = "$D/state"
socket = "$D/theseus.sock"
[tools]
projects_dir = "$D/projects"
[discord]
enabled = false
[web]
port = 7499
[judge]
enabled = true
[telemetry]
otlp_endpoint = "http://127.0.0.1:4318"
metrics_interval_secs = 5
EOF
# a local OTLP/HTTP receiver on 4318 (the repo has none; the design's otlp-receiver.py is not in the tree):
# any collector, or this branch's report's throwaway: a Python http.server that appends each POST body to a file
theseusd --config $D/config.toml --socket $D/theseus.sock --state-dir $D/state &
T="theseus --socket $D/theseus.sock"
$T ask "Name three rivers in Europe."
$T ask "Say done."
$T ask --trace "What is 2 + 2?"     # the trace ends with: judge [mark] loop.v1 shadow at loop_end · reply · jdg_… (theseus judge show jdg_…)
sleep 3; $T judge log               # three loop.v1 (shadow) lines, each with answers, cost, and Jev's ms
$T judge show <the id from the trace>   # the state as fields; each answer with its band and bars
$T health | grep judge              # 3 calls today · $… of $1.00 · (no "key …" while it is ready)
# Cockpit: http://127.0.0.1:7499/judgment shows loop.v1 · shadow · v1 · 3 calls · cost · reply p50 / p95;
# /session/<id>?tab=timeline shows "loop 1 · Jev (shadow): …" above the flame chart, and the mark as a diamond.
# The receiver shows theseus.judge.calls {pack loop.v1, mode shadow, band …, class reply}.
$T shutdown
```

## Left, or uncertain

- **`judge.resumed` is in-process only.** A restart forgets that yesterday paused, and says nothing. Persisting it
  would add a field to the `judge.budget` META record, which would need a format bump.
- **Disagreement counts the act verdict only.** It does not count a deciding Choice's lean (for example `work_state`
  progressing in the act band, which the design's nudge rule also uses). The owner may want that rule per pack
  (`loop.v1`'s live nudge, 26b).
- **The Judgment section's per-pack numbers are over the newest 500 judgments** that match its filters, not all
  history. A `pack.list` or a summary method could give totals later.
- **No live `judge` spans.** Per the brief they come with the first live pack (26b). The CLI and the cockpit render the
  kind already: the CLI shows any span; the flame chart treats a `judge` mark specially, and a live `judge` span would
  draw as an ordinary span.
- **Shared files touched, small:**
  - `turn.rs` (one call), `rpc/server.rs` (two arms), `rpc/mod.rs` (`mod judge;`, two setter calls);
  - `theseus-protocol/src/lib.rs` (two method names) and `ledger.rs` (one kind), `ts.rs` (the export list);
  - `fact/mod.rs` (one `FACTS` line), `store.rs` (`ledger_by_key`), `telemetry.rs`, `telemetry/tests.rs` (six helpers
    made `pub(super)`);
  - the CLI's `main.rs` (`JudgeCmd::Show`, `--pack`): the prove-report change adds `JudgeCmd::Prove` beside it;
  - the cockpit's `main.tsx` (one route) and `Shell.tsx` (one nav item, key `j`).
- **Docs for the maintainer.** Part III's 23b item and `docs/status.md`. The design's §2.13 Telemetry row could name
  `on_path_ms {pack, class}` and errors under `theseus.error.class`. §2.5's table could list `judge.resumed` as
  written.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` ran before each commit. Every phase before the suite passed (fmt, shape,
features, clippy, cockpit lint, test, build, test build, reader rule), and so did every phase after it, run by hand:
the generated TypeScript (no diff), `theseus-sim bench turn --check --runs 5 --burst 0` (plain turn 5 frames, tool turn
9: ok), and `cargo deny --offline check` (ok).

The suite failed only on cases that are not this step's:
- **33 sandbox tests** (`theseus-sandbox::contract` and `bench`, and 15 tests in `theseusd::sandbox` including
  `the_jobs_bench_l1_row`): theseus-pv6i. This VM runs as root, and L1 refuses a root daemon: "Linux exempts root from
  RLIMIT_NPROC".
- **`tests_output::the_cores_output_matches_its_golden`** fails on this VM only because its TZ is UTC. The golden's
  wake lines carry the owner's negative offset (`-#:#`), and here they print `+#:#`. It passes with
  `TZ=America/New_York`, and the last three gates ran under that TZ. The golden file is unchanged. This is not in the
  brief's known list: the golden's offset sign probably wants a `#` normalisation (worth an issue).
- **Flaky:** `the_kernel_holds_its_invariants_under_seeded_faults` (on the flaky list, theseus-81ig) flaked once and
  passed on a retry.

Final suite: 1970 run, 1937 passed, 33 failed (all sandbox), 17 skipped.

**Under load** (AGENTS.md's recipe: four busy loops at nice 0, the tests at nice 19): the 15 judge tests (23a's,
23b's and the telemetry test), five rounds: 75 of 75 passed.

**Lifecycle bench** with the judge on (endpoint at 127.0.0.1:9): `LIFECYCLE OK in 16.7 s` on this VM. The maintainer
measures it on the owner's machine.
