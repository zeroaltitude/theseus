# Cloud report: the repeating wake (row 64, step 37a, theseus-d4pt)

Branch `cloud/20261003-wakes-repeat`, from `main` at a59b7c1. Started 20:00 UTC, report written 21:10 UTC.
These commits are not signed. The maintainer re-signs them.

| Commit | Subject |
|---|---|
| be2d986 | kernel: a wake repeats, re-armed in the frame that takes it |
| 9083fcf | wake: wake.at takes every, days, and until, and a repeating one is held after external text |
| 826e6b0 | wakes: a series shows its span, occurrence, and next time, and is counted by repeat |
| 13d27b9 | sim: kernel-sim reads back that a taken series is pending at its next occurrence |

## 1. The kernel (be2d986, plus 13d27b9 for the sim's check)

**What I found.**
- Wakes are one-shot. `take_wakes` removes every due wake in one frame.
- No clock control exists in the daemon. The existing daemon wake tests use real waits of 2 s and 9 s.

**What changed.**
- New `crates/theseus-kernel/src/repeat.rs`:
  - `Repeat { every, first_ms, days, until_ms }`, with `Every { n, unit: m|h|d|w }` and `Day`.
  - Occurrence k is due at `first + k·every`, computed with jiff's zoned arithmetic in `KernelConfig::zone`. That zone is the system's; a test can set its own.
  - Days and weeks are calendar spans, so 21:00 stays 21:00 across a change of offset. Minutes and hours are exact time. A wall time skipped by the spring change lands after the gap, by jiff's compatible rule.
  - `next_after`, `first` (applies `days`), and `between` (counts occurrences in a range).
- `PendingWake` gains `repeat: Option<Repeat>` and `occurrence: u32`.
  - Both are skipped when empty, so a one-shot wake's bytes are unchanged.
  - EXECUTION's schema goes from 2 to 3 in `kinds::SCHEMAS`, and the record-schema golden gains `execution @ 3`. The payload's own `schema` field stays 2, as ACTION's does.
- `take_wakes` puts a due series back in the same frame: same id, at the first occurrence after now.
  - `FiredWake` gains `missed` and `next_due_at_ms`.
  - `wake.fired` carries `every`, `occurrence`, `missed`, and `next_due_at_ms`, but only for a series, so one-shot rows and goldens are unchanged.
  - `wake.set` carries `every` (and `days`, `until_ms`).
  - Past `until`, the series is not put back, and a `wake.ended` row (a new `LedgerKind`) is written.
- A cancel removes the one wake, which ends the series. A series counts once against the cap of 5.
- `set_wake` takes `repeat` and refuses a span under `min_repeat_ms`.
- `[kernel] min_repeat_minutes = 5` is added to `KernelSection`, `to_kernel_config`, and the template (the operator pastes it).
- jiff joins theseus-kernel's dependencies at the locked 0.2 with default features. `Cargo.lock` gains only that edge, no package.
- The kernel-sim (`kernel_sim/wakes.rs`, a submodule so the file stays under 2,500 lines):
  - Turns set wakes (half repeating every 1 to 4 minutes, some with `until`), take due ones as the core's catch-up does, and the operator cancels some.
  - A fifth of crashes now keep the daemon down for 1 to 10 minutes, so occurrences get passed over. This changes every seed's random stream.
  - Its checks:
    - every wake is taken at or after its time, and no `(id, occurrence)` runs twice;
    - a taken series is pending again at its reported next time with the right number, read back from the store (13d27b9);
    - a one-shot wake that ran is gone;
    - a series ends only past its `until`;
    - per execution: at most 5 wakes, soonest first, unique ids, none on a terminal execution;
    - a series is due on one of its occurrences, and its occurrence never goes back.
  - The sim's floor is 1 minute.

**How I proved it.**
- `tests_repeat.rs`: 11 tests, all pass:
  - parsing;
  - DST: New York (`EST5EDT,M3.2.0,M11.1.0`, a POSIX string, so no tz database is read) through both 2026 changes. A day crossing a change is 23 h or 25 h, with 21:00 kept. An hourly series moves by the change. A 02:30 wall time lands at 03:30 on the spring change day.
  - Phoenix (`MST7`): 300 days of exactly 24 h.
  - weekdays;
  - the re-arm (the take is exactly 1 frame, counted by the kernel's observer);
  - missed while down: 3 missed, `while_down`, the next is #5, no burst;
  - cancel; `until` (`wake.ended`, and down across `until`); the cap and the floor;
  - the take in the kernel's zone (NY 23 h and 25 h, Phoenix 24 h);
  - `an_execution_written_before_repeating_wakes_reads`: schema-2 bytes as a literal read as one-shot and encode back byte-identical.
- Planted revert of the re-arm (the next occurrence not pushed): 6 of 11 failed, for example `a_due_occurrence_is_put_back…` with "one wake: the series, left 0 right 1".
- Planted revert of the missed count (`0 * between(..)`): 2 failed, `occurrences_missed_while_down…` (left 0) and `until_ends_the_series…`.
- After each revert: restored from a copy, `touch`ed, `git status` clean.
- Kernel package: 134 tests pass.
- Frames golden: only `schema=2→3` on EXECUTION lines, plus the two `Debug` lines of `WakeSet` (they print the new fields).
- kernel-sim:
  - At the gate's seed count (the suite's `the_kernel_holds_its_invariants_under_seeded_faults`, seeds 1 and 2 at 300 steps): clean. One run reported 25 wakes set, 10 repeating; 12 taken, 8 series put back, 5 passed over, 1 ended by until.
  - Also `--seeds 40 --steps 300`: 623 set, 148 put back, 107 passed over, 7 ended. `--seeds 20 --steps 1500` and `--p-race 0` also ran. All invariants held.
  - Planted revert of the re-arm against the sim: before 13d27b9 the sim missed it. That is why 13d27b9 exists. After it, the sim fails with "series wak_… #1 was taken, and its next is not pending at …: None".
- A plain turn is still 5 frames: the gate's turn bench reported `frames_plain: 5 … ok`, and `tests_m3`'s budget test passed. The take adds no frame of its own (counted above).

## 2. The tool and its hold (9083fcf)

**What changed.**
- `wake.at` takes `every`, `days`, and `until`.
- Floor: `min_repeat_minutes`. Ceiling: 365 days.
- First time: `at` or `after`; with neither, one span from now in the zone.
- `days` only with `every = "1d"`, and is deduplicated.
- Invalid input: `days` or `until` without `every`; an `until` before the first time; a first time more than 30 days ahead.
- The result says `Set wake …, every 1d, first at …`.
- Each occurrence's node reads `⏰ wake (every 1d, #4): note`, with `; 2 missed while the daemon was down` or `, due …, N late` as they apply. One-shot text is unchanged.
- **The hold.**
  - `external::exempt(class, tool, input)` now exempts `wake.at` only when the input has no `every` (`wake::repeats`).
  - `external::gate` takes the input, since it re-checks `exempt` itself.
  - `toolrun::gate` passes `&call.input`.
  - I found no smaller form. `gate`'s own early return exempts `wake.at` by name, so a condition at the call site alone could not lift the exemption. Expect a conflict with the integrity change's rewrite of `external.rs`: the change is the `exempt` body, `gate`'s extra parameter, its 8 test call sites, and one new assertion.

**How I proved it.**
- `tests_wakes::a_repeating_wake_waits_in_a_session_holding_external_text_and_a_one_shot_does_not`:
  - in a session holding external text, a one-shot wake is set with no wait;
  - a repeating one waits for approval (`wake.at`, with the reason naming the external text) and is not set;
  - in a clean session the series is set at once, one day ahead, with `wake.set.every = "1d"`.
- `external::tests` asserts a repeating `wake.at` is not exempt.
- `wake::tests`: input and node-text tests (`the_input_takes_a_series…`, `a_repeating_wakes_line…`).
- Planted revert of the hold (`exempt` ignoring `every`): the hold test failed. The repeating wake was set with no wait, and `awaiting_confirm` was `None`. Restored, `touch`ed, status clean.
- Core tests for wakes, external, telemetry, schemas, registry, and config all pass.

**Golden.**
- The longer description and three new properties grow every request by 732 bytes. `core_output.txt`'s request sizes, token counts (+305 per request), and costs move with it, and nothing else does.
- On this VM, `HEAD`'s own golden already differs in 16 lines: the theseus-6a7o byte, and two lines where the host's offset prints `+` against the owner's `-`. I ran `HEAD`'s golden here to find them.
- So the committed golden is `HEAD`'s golden with my deltas applied: on those 16 lines I applied the same numeric deltas. Those 16 lines are computed, not observed: for example 6,476 → 6,781 tokens and $0.012992 → $0.013602, which matches 6,781 × $2/M + 4 × $10/M.
- **Please rerun `THESEUS_GOLDEN=write` on your machine** and check that the diff is empty.

## 3. The surfaces (826e6b0)

- `WakeInfo` gains `every`, `occurrence`, and `next` (`21:00 Thu`, the daemon's clock, a series only). The TypeScript is regenerated.
- `theseus wakes`: a new column, `once` or `every 1d #4`.
- Discord `/wakes`: `• \`c5d6e7\` 🔁 every 1d · next <t:…:t> (<t:…:R>) · note`. The time is in the reader's own zone, like the other lines, rather than the daemon's `21:00`.
- Narrative for a series' turn: `Wake a1b2c3 fired (#4, 2 missed while down); next 21:00 Thu: "…" is this turn's input`, or `the series ended (until)`. A set series says `Wake … set, every 1d, first for …`.
- Telemetry:
  - `WakeCameDue` records a `wake.fired` span (kind `wake`) on the turn's trace, for every wake, one-shot included.
  - The metrics read it as `theseus.wakes.fired{theseus.wake.repeat}` and the histogram `theseus.wakes.late_ms` (same attribute).
  - Test: `the_wakes_a_turn_took_are_counted_by_repeat_with_their_lateness`.
- Daemon test `a_repeating_wake_at_the_floor_runs_twice_a_minute_apart` (`min_repeat_minutes = 1`, `after 2s, every 1m`):
  - both occurrences post once under `#1` and `#2`, a minute apart;
  - the `wake.fired` rows are `(1, 0 missed, first+60 s)` and `(2, 0, first+120 s)`;
  - `wake.list` shows occurrence 3 next;
  - a cancel ends it.
  - It takes about 63 s of real time. The daemon has no clock control, the existing wake tests also use real waits, and the floor is a minute. That is under nextest's 2-minute kill, and it is marked slow.
  - Under load (nice 19 beside four busy loops at nice 0) all 3 daemon wake tests passed; the repeating one took 68 s.
- CLI and Discord render tests updated and extended.
- `AGENTS.md`: the kernel's guide (`repeat.rs`, `tests_repeat.rs`) and the sim's (wakes, long downs).
- Ceilings in `scripts/long-files.txt`:

  | File | Ceiling |
  |---|---|
  | config.rs | 2,900 → 2,910 |
  | CLI render.rs | 3,100 → 3,120 |
  | Discord render.rs | 2,930 → 2,945 |

**What the cockpit's wakes view should show.** I did not touch the cockpit or the Observatory.
- For each wake: short id, session (title), note, and due time with its countdown, its state, and its target.
- For a series: `🔁 every 1d` (plus days and `until` when set), `#occurrence`, the next time, and how many it has missed, from the latest `wake.fired.missed`.
- An "ended" marker from `wake.ended`.
- The design's "cost to date" per series is not built. The cockpit could sum the cost of turns whose `wakes` (on the turn's result) name the series' id.

## Left or uncertain

- **Numbering.** After missed occurrences, the next is numbered by the schedule (`occurrence + 1 + missed`, so #4 with 2 missed is followed by #7). The design says `occurrence + 1`, which assumes nothing was missed. Easy to flip in `take_wakes` if you'd rather count only runs.
- **Missed while busy.** Occurrences passed over while the session was busy are counted too. The node then says `2 missed` without "while the daemon was down".
- **`next` is a display string**, not a time.
- **Unreadable record.** A repeating wake in a session whose record can't be read waits (fails closed), as any held call does.
- **Doc changes for you:**
  - the spec's Part III item for 37a;
  - `docs/status.md`;
  - m7-surface §2.2's "Seen in": `next` is a local string; Discord shows the reader's zone; numbering after missed occurrences; `wake.ended` carries `why: "until"`;
  - theseus-core's `AGENTS.md` could note that `external::exempt` reads the call's input.

## The gate

`THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run before each of the 4 commits.
- fmt, shape, clippy, and the reader rule pass.
- **Suite:** 1,703 run, 1,701 passed, 2 failed, both known on this VM:
  - `theseus-sandbox::contract clause_09_limits` (root, theseus-pv6i);
  - `theseus-core tests_output::the_cores_output_matches_its_golden` (theseus-6a7o, line 1041, the known byte).
- **After the suite**, run by hand: protocol types, turn bench (5 frames, ok), `cargo deny --offline check`, web lint+build, cockpit lint+test+build, web dist. All pass.
  - The first deny run failed because setup's `cargo deny fetch` had not run (setup stopped at my own mid-edit build). It passed after I fetched.
