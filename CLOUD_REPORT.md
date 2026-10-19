# CLOUD_REPORT: threadbench-threads (theseus-l7z1)

Branch `cloud/20261019-threadbench-threads`. Bench code only (standard-library Python); **no product code changed**.
Commits: `2ecde33` (the work), `edac0b5` (a docstring), then this report.

## Design choices the owner should hear about

1. **The bench profile has no Jev.** With no `[judge]` the daemon's pre-router places alone. Its working sets start
   empty, so on a thread the owner made by hand it finds no entity to match, and every A1 message ran at home (smoke:
   top-1 0 of 40, `how: guess`). The real after run needs Jev's endpoint: `th_drive.py --config-overlay <toml>` lays a TOML
   over the scratch config and keeps its sha256 in `run.json`. Without it A1 and A1n measure nothing about routing.
2. **No thread is live at the first message, so the router never offers one** (`route_to_thread` returns at once with no
   live thread). The owner policy therefore starts a stream's thread by hand when its first message ran at home with no
   offer (`run.json` `owner.started_by_hand`, 5 on the smoke). The first message of each stream is scored as it ran (home).
   This is a finding about the product's bootstrap; say so in the T12 write-up.
3. **A1p is `NotMeasurable`.** The persona layer is every persona's files whenever two or more `[personas]` exist
   (`config.rs` `context_paths`, `persona_engaging`); there is no switch for a layer per persona. No product switch built.
4. **A prefix-track `reopen` is not driven.** A thread goes cold when its cache window passes, which is wall-clock time in
   the daemon (`[cache]`); a pause/resume or a restart does not move it. The message is sent as written and `run.json` says
   `reopen_not_driven: N`.
5. **A2** is scored as the owner's own routing (`OWNER_ROUTED` gained `A2`); its `routing.jsonl` is still written (the
   addressed thread) so the persona and prefix records exist for it.
6. **Chart colours.** `bench/report/charts.py` has no key for the threads arms and its test pins the registry. I did not
   touch it: `report.py`'s `figures()` maps the three threads arm keys onto the memory family's slots 6 to 8
   (`CHART_KEY`). The maintainer may prefer real entries (and the pinned test and README palette table updated).
7. **No overhead probe for the threads arms.** `measure_theseus` runs a plain session with no persona layer or thread
   card, so it would not be their overhead; `run.json` says `overhead.source: not measured`.
8. **Modelled dollars.** The scorer's repricing groups messages by the progression view's sessions; for the threads arms
   that is one session per true stream (exact for A2, the ideal router for A1).

## What I found (the product, as the driver meets it)

- Where a message ran: the reply's `thread` tag (`TurnSubmitResult.thread`), present only when the place had live threads;
  an `ask --thread` reply carries none. The router's question is printed by the CLI on stderr (`thread/ask.rs`) even
  under `--json`; the driver reads it as it comes and answers with `theseus thread answer` (no product change needed).
- The record per message is the tag, plus the turn's `thread.routed` (`timed_out`, Jev's pick), `persona.engaged` and
  `prefix.picked` ledger rows (read by `position` after each turn). On the smoke `persona.engaged` rows appear (A2, how
  `prior`); **no `prefix.picked` row was written in any smoke run** (the bench has no context text for the library to fit),
  so that capture is proved by a unit test on synthetic rows only. Check `routing.jsonl`'s `prefix` on the real run.
- A thread session's history holds `recall` nodes quoting another thread's turns (`[from thread «…»]`: the product's
  cross-thread recall). The A2 key test therefore counts only `user_message` nodes.
- Twice in about eight live runs `left running: ['bash N']` was reported (exit 1); a later loop of four runs and the
  smoke tests saw none. Not reproduced, cause not found; an existing 5 s wait in `th_drive.stop_stand_in_services` races it.

## What changed

- `th_drive.py`: the arm table on the design names (A1, A1n, A1p, A2, layout `thread`), `MEASURABLE`/`REFUSAL`
  (`unmeasurable`), `--config-overlay`, the threads path (`th.ThreadsTheseus`); A3 to A7 untouched.
- `th_owner.py` (new): the simulated owner and the CLI-line parser. `th_threads.py` (new): the arm config, the daemon
  driver, the ask loop, the routing record.
- `th_score.py`: `OWNER_ROUTED` has A2; `engagement()` adds a `threads` key (persona engaged vs the label, prefix picks)
  only when records carry it. `report.py`: "What T12 needs", the not-measured line, `CHART_KEY`. `README.md`.
- `bench/plans/threadbench-t12.json`: A1, A1n, A2, A3; `probe_only` A1p; T0r's three digests; $35 to $60, pause $170 with
  the before run, $25 per arm.
- Tests: `test_th_threads.py` (new, 20), `test_th_smoke.py` (the refusal test replaced by the arm-table test; the T0r plan
  test names its five arms literally instead of `MEASURABLE`; the report test's "need threads" line is now "A1p: not
  measurable" and "What T12"; `ThreadsSmoke`, 2 tests).

## How it was proved

`python3 -m unittest discover -s bench/threads`: 71 tests OK (smoke against debug binaries built here:
`cargo build -p theseus -p theseusd`, plus `-p theseus-index`, which the driver needs beside the daemon).
`python3 -m unittest discover -s bench/recall`: 93 tests, 7 skipped, **1 failure, not mine**:
`test_generate.Budget.test_the_prices_are_the_catalogs` (the catalog's cache-read price for a priced model is 0.1 where
`generate.PRICES` says 0.2); bench/recall is untouched by this branch. `py_compile` on every changed file, `bash
scripts/shape.sh` ok, `keel-guard.py --base e8a47eb --head HEAD --quiet` empty. Live smokes (41 messages, stand-in
model): A2 oracle 1.00, 40 of 40 facts delivered, 5 threads; A1 and A1n 40 of 40, 41 routing records; scored by
`th_score.py`. Not run here (as the brief says): the full gate, the benches, any real model call, cargo-deny, nextest, npm.

### Planted reverts (replay: edit, run the test, `git checkout <file>`, `touch <file>`, `git status`)

| plant | file: edit | test that failed |
|---|---|---|
| KEY: the driver drops `--thread` | `th_threads.py`: `target = ["--thread", m.stream] if addressed else ["-s", home]` → `target = ["-s", home]` | `ThreadsSmoke.test_a2_puts_each_streams_messages_in_its_own_thread_and_home_messages_stay_home`: `Lists differ: [] != ['harness-dev', 'local-ops', 'pipeline', 'pr-round', 'sec-fix']` (the daemon lists no thread session) |
| routing record not written | `th_threads.py`: `out_f.write(json.dumps(rec, sort_keys=True) + "\n")` → `pass` | `ThreadsSmoke.test_a1_sends_everything_home_…`: `Lists differ: [] != [0, 1, 2, …, 40]` |
| accept every offer | `th_owner.py`: `choice = "new" if self.first_of_stream(stream) and "new" in keys else "home"` (the offer branch) → `choice = "new"` | `Owner.test_the_policy_on_a_scripted_sequence`: `'new' != 'home'` |
| A1n's override dropped | `th_threads.py`: `"A1n": {"prefix": {"enabled": False}}` → `"A1n": {}` | `Config.test_a1n_turns_the_prefix_library_off…` and `test_the_config_writes_as_toml` (KeyError `prefix`) |
| A4 added to the after plan | `threadbench-t12.json`: arms gain `"A4"` | `Plan.test_each_plan_refuses_the_arm_it_does_not_name` (`SystemExit not raised`), `test_the_after_plan_names_…`, and the driver test (it reached the Claude Code pin check instead of refusing) |

Under-load runs: not asked for this row, not done.

## Live check for the maintainer

```bash
# the frozen release binaries, Jev reachable (a TOML with the [judge] table the owner's config uses)
python3 bench/threads/th_generate.py --seed 11 --size smoke --out /tmp/tb-smoke
python3 bench/threads/th_drive.py --arm A1 --bin-dir <bin dir> --config-overlay <jev.toml> --progression /tmp/tb-smoke --out /tmp/tb-a1
python3 bench/threads/th_score.py /tmp/tb-a1 --out /tmp/tb-s1
```
Expect: `routing.jsonl` with 41 records; `scores.json` `routing.top1.n` 40 (smoke) and a nonzero top-1; `run.json`
`owner.answers` listing any offer or question answered, `owner.started_by_hand` small (Jev should offer new threads); the
`prefix` and `persona` keys in the records and a `threads` key in the scores. `th_drive.py --arm A1p --out /tmp/x`
refuses with the reason. The full after run is `report.py`'s "What T12 needs".

## Left open

- Docs for the maintainer's commit: spec Part III / `docs/status.md` should say the threads arms are drivable, A1p is not
  measurable (no switch), and the bootstrap finding (design choice 2).
- The `bash` leftover (above), the prefix-record capture on a real run, and Jev's overlay are for the live run.

## Gate

The full gate was not run (this row's checks replace it). The row's checks all passed except the pre-existing
`bench/recall` price test named above.
