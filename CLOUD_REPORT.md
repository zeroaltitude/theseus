# Cloud report: threads-routing-rest (theseus-98bz.5, theseus-98bz.3, theseus-98bz.7)

Branch `cloud/20261021-threads-routing-rest`, from `integ/threads` at `6d4e0dd9` (plus the task commit `d309965`).
All three rows are done, each in its own commit with its tests and planted reverts.

| Row | Issue | Commit | State |
|---|---|---|---|
| A: a thread's working set grows from its own turns | theseus-98bz.5 | `dd3ab5a` | done |
| B: the threadbench routes again, at the plan's gaps | theseus-98bz.3 | `9470652` | done |
| C: a lone thread offers a new one | theseus-98bz.7 | `9ff63e5` | done |

No file is over its line ceiling. `crates/theseus-protocol/src/lib.rs`, `crates/theseus-discord/src/runtime.rs` and
`crates/theseus-core/src/turn.rs` gain no line. `rpc/methods.rs` gains 6 lines (1,925 to 1,931), `thread_route.rs` 58,
and `rpc/thread_flow.rs` 71 (5 removed). The store format is unchanged at 48: row A writes the existing `ThreadInfo.working_set`,
row B adds only a wire parameter, and row C stores nothing. No dependency was added.

**A note on this VM's disk.** The first full build with incremental compilation filled the session's disk allowance
(`target/debug/incremental` was 12 GB). The `cp` that restored `rpc/methods.rs` after a planted revert then
failed mid-write and truncated the file. I restored the file from a copy taken before the plant and checked it line for line (1,930 lines, the
diff only mine) before going on. From then on every cargo run here used `CARGO_INCREMENTAL=0`.

## Row A: a thread's working set grows from its own turns (theseus-98bz.5), `dd3ab5a`

**What I found.** Nothing grew a thread's working set after it was made. `thread_split::working_set_of` already read
tool-call inputs (`path`, `paths`, `file`, `files`, `cwd`, `dir`, `url`), but only for a split.

**What I changed.**
- `thread_split::touched(inputs)` is the call-reading half of `working_set_of`, moved out whole (a pure move;
  `working_set_of` now calls it, and `thread_split::tests::the_working_set_is_what_the_chosen_turns_touched` still
  passes).
- `thread_grow.rs` (new, pure):
  - `touched_in(calls, dir)` makes paths relative to the session's directory. A relative path is kept as written; an
    absolute path under the directory is made relative to it. Each file gives the file and, when it has two or more
    components, its first component as a root.
  - An absolute path outside the directory, a path with `..`, and the directory itself give nothing. This was my
    choice; the brief named only the inside cases.
  - `grow(current, touched, named, elsewhere)` adds what the turn touched, then what the message named. It skips
    anything another live thread's set holds (any list, compared without case) and adds nothing to a list already at
    `MAX_SET`.
- `rpc/thread_grow.rs` (new) has two methods:
  - `Core::grow_for(routing, session, text)` runs before the turn. It returns nothing when the switch is off or the
    board is not held in memory, so it never reads the store. The thread is `routing.slug`, else
    `Board::of_session(run session)`; a home turn grows nothing. It captures the session's directory
    (`tools.cwd_for`), `store.last_position()`, and the message's entities (`extract::working_set`).
  - `Core::thread_grow(grow, turn_id)` runs after `runner.run` returns, beside `thread_routed_turn`. It reads one page
    of the session's NODE records after the start position (at most 1,000, through `theseus_store::blocking`), keeps
    the ToolCall inputs of that `turn_id`, and writes through `threads.write`. It writes only when the set changed,
    and then one frame holds the record and a `thread.edited` row: `changed: ["working_set"]`, `who`/`via` `harness`,
    with the session and turn ids. It does not call `thread_edit`.
- "Every other live thread" means every other thread whose state is `Active`; sealed ones count, since a seal is a
  field and not a state.
- `[threads] working_set_grows = true` is in `config/threads.rs` and the template, right after `ask_every_thread`.
- `turn_submit` gains 5 lines: the `grow_for` call before `runner.run` and the `thread_grow` call after it.
- Discord's `guild_thread::thread_changed` posts nothing and renames nothing for an `edited` change whose title did
  not change (it compares the computed name). A working-set-only edit therefore does nothing there, so I added no
  test.

**How long it takes.** The key test's whole turn (one `fs.write` loop and a reply, then the growth) took 84 ms in a
debug build here, from the submit to the response. The growth is one bounded page read and, when the set changed, one
frame. It runs after the reply has streamed, before the RPC answers.

**Tests** (`rpc/tests_thread_grow.rs`, a whole core with a scripted provider, the judge off; `thread_grow::tests`;
the config test):
- `a_threads_turn_grows_its_working_set` (key test). Thread `pr-round` starts empty. A turn sent with
  `thread: Some("pr-round")` and the message "answer the first review comment on PRR-4142" has its scripted model
  write `<root>/tarnlib/reviews/comment-1.md`. Afterwards:
  - the thread holds roots `["tarnlib"]`, files `["tarnlib/reviews/comment-1.md"]` and refs `["PRR-4142"]`;
  - there is one `thread.edited` row, `changed ["working_set"]`, by `harness`, on the turn's id;
  - `pre_route("comment-2.md under tarnlib/reviews", ..)` gives `pr-round` a hit with `entity > 0`;
  - a second turn that adds nothing writes no second row.
- `a_home_turn_grows_no_thread` and `with_the_switch_off_no_set_grows`. With the switch off, the META record
  `thread:pr-round` is compared as JSON before and after: equal, and there is no row.
- Pure tests: `a_touched_path_gives_its_file_and_its_root`, `a_turn_adds_what_it_touched_and_named`,
  `an_entity_another_thread_holds_is_not_added` (a sealed thread's set included), `a_full_list_takes_nothing`,
  `nothing_new_changes_nothing`.
- Config: `config::threads::tests::the_working_set_grows_by_default_and_turns_off`.

**Planted reverts.**

| Plant | Test that failed | Result |
|---|---|---|
| Skip the `thread_grow` call in `turn_submit` | **key test** `a_threads_turn_grows_its_working_set` | `left: []`, `right: ["tarnlib"]` |
| `grow_for` ignores `working_set_grows` | `with_the_switch_off_no_set_grows` | the record gained the working set |

Each file was restored, `touch`ed, and checked with `git status`.

## Row B: the threadbench routes again, at the plan's gaps (theseus-98bz.3), `9470652`

**What I changed.**
- Wire: `TurnSubmitParams.route_at_ms: Option<u64>` (`serde(default, skip_serializing_if)`, `ts(optional)`) is in
  `submit.rs`. Every literal that lists all fields gained `route_at_ms: None`; Discord's `runtime.rs` uses
  `..Default::default()` and gained no line. `cockpit/src/protocol.gen/TurnSubmitParams.ts` was regenerated with the
  protocol test.
- `[threads] bench_clock = false`, after `working_set_grows`, is a test hook. `Core::refuse_bench_clock` (called once
  in `turn_submit`) refuses a submit that carries `route_at_ms` while the key is off, as INVALID_PARAMS: "route_at_ms
  is the threadbench's clock: this daemon takes it only with [threads] bench_clock = true".
- `may_route` places a submit with `via: "ask"` only when `bench_clock(p)` is some, meaning the key is on and the
  submit carries the clock. Without the clock, `ask -s` is never routed, and
  `a_session_named_with_ask_s_is_never_placed_in_a_thread` passes unchanged.
- `route_to_thread` uses the bench clock as `now`, which feeds `pre_route`, `state.arrived` and `Routed.at_ms`.
  `Routing.bench: Option<(u64, Instant)>`, and `thread_routed_turn` records `state.replied` at the clock plus the
  elapsed real time. Nothing else reads the clock.
- CLI: `theseus ask --route-at-ms MS` is a hidden flag (`AskArgs`), sent from `cmd.rs`.
- Bench:
  - `th_threads.py`: `CLOCK` (`[threads] bench_clock = true`) is merged into `OVERRIDES` for A1, A2, A1n and A1o.
  - `ask_target(m, addressed, home)` gives the target plus `--route-at-ms` set to `CLOCK_BASE_MS + round(m.t *
    60000)`, with a fixed base of `1_700_000_000_000`; `drive` uses it.
  - `README.md`'s line on gaps now says the gaps reach the router.
  - I did not touch `th_oracle.py`, `th_score.py` or `report.py`.

**Tests.**
- `rpc/tests_thread_bench.rs`:
  - `a_bench_clock_is_refused_unless_the_key_is_on`: the error message names `[threads] bench_clock`.
  - `the_bench_clock_routes_ask_at_the_plans_gaps`: one thread `pr-round` holds `#412`. Two `via: "ask"` submits 23
    bench-minutes apart are both routed (two `thread.routed` rows; the first lands in `pr-round`), and the second
    row's pre score for `pr-round` is below 0.15. The decision for a lone thread at that score is home.
- `config::threads::tests::the_bench_clock_is_off_by_default`.
- `bench/threads/test_th_threads.py`, 22 tests, OK:
  - new: `test_each_ask_carries_its_messages_plan_time`;
  - `test_every_threads_arm_gets_the_five_persona_cards_and_threads_stay_at_the_defaults` now expects `threads ==
    {"bench_clock": True}` instead of no `threads` table. That assertion changed because every arm now sets the hook;
    it is not loosened.

**Planted reverts.** These were run in the same build as row A's switch plant; each touches a different test.

| Plant | Test that failed | Result |
|---|---|---|
| Ignore `route_at_ms` and use the real clock | `the_bench_clock_routes_ask_at_the_plans_gaps` | score 0.59996, not under 0.15 |
| `refuse_bench_clock` never refuses | `a_bench_clock_is_refused_unless_the_key_is_on` | the turn ran |

## Row C: a lone thread offers a new one (theseus-98bz.7), `9ff63e5`

**What I changed.**
- `thread_route::lone_offer(threads, pre, new_entities, new_title, every) -> Option<Decision>` is new, beside
  `decide_single`, which is unchanged. When the thread's pre score is below `CONFIRM` and the message names at least 2
  new entities, it returns an `Ask` from `ask_options(threads, fuse(pre, None), Some(new_title), every)`: the thread,
  `new`, and `home`. Otherwise it returns `None`.
- `place_unaddressed`'s `live.len() < 2` branch calls `Core::offer_beside_one`. That method returns early, before any
  extraction, when the switch is off or the score is at or above `CONFIRM`. Otherwise it computes `new_entities` (as
  C9's `Rules` does) and `propose`'s title. An offer goes to `ask_thread`, as the multi-thread branch's does; a spoken
  turn runs at home at once through `answer_tag(None, ..)`. There is no Jev wait.
- A place with no live thread is still not routed.
- `[threads] single_offers_new = true` comes after `bench_clock`, in the config and the template.

**Tests.**
- Pure: `thread_route::tests::a_lone_thread_offers_a_new_one_beside_it`. One thread holds root `tarnlib`.
  - "homesvc maintenance: branch chore/bump-1-8, ticket OPS-4214" gives 2 new entities and an Ask keyed `1`, `new`,
    `home`, labelled "New thread «homesvc maintenance»".
  - "retry the flaky test in tarnlib" gives no offer, and `decide_single` routes it Visible as today.
  - A low score with one new entity gives no offer.
- `rpc/tests_thread_lone.rs`, through the whole core:
  - `a_lone_thread_asks_and_offers_a_new_one`: one `thread.asked` row with options `tarnlib work`, `New thread «…»`,
    `home`. Unanswered (`ask_wait_secs = 1`), the message runs at home. The tarnlib message then routes to the thread
    (Visible) with no second ask.
  - `a_spoken_turn_beside_a_lone_thread_runs_at_home`: a Discord connection with `surface: "voice"` is routed (one
    `thread.routed` row) but not asked, and runs at home well within the 60 s ask wait.
  - `with_the_switch_off_a_lone_thread_asks_nothing`: no ask; it runs at home with `how: home`.
- `config::threads::tests::a_lone_thread_offers_a_new_one_by_default_and_turns_off`.
- No fixture case moved. `forty_unaddressed_messages_find_their_thread_and_ask_rarely`,
  `the_decision_table_by_band`, `the_decision_table_is_the_same_with_the_sticky_rule_on`, `rpc/tests_thread_mix.rs`
  and `a_single_threads_place_waits_for_nothing` all pass unchanged.

**Planted reverts.**

| Plant | Test that failed | Result |
|---|---|---|
| Drop the offer (`lone_offer` never offers) | `a_lone_thread_offers_a_new_one_beside_it` | "an ask" |
| Same plant | `a_lone_thread_asks_and_offers_a_new_one` | `left: 0` questions |
| `offer_beside_one` ignores `single_offers_new` | `with_the_switch_off_a_lone_thread_asks_nothing` | a question was asked |
| Remove the spoken-turn arm | `a_spoken_turn_beside_a_lone_thread_runs_at_home` | "no wait": it waited 60 s |

## The gate

I ran `CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit. Every phase
before the suite passed each time: door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test
build, deny, and the reader rule.

| Row | Suite | Failures |
|---|---|---|
| A | 7,352 run, 7,318 passed, 34 failed | the 33 known L1 tests, and `tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused` |
| B | 7,355 run, 7,322 passed, 33 failed | the 33 known L1 tests only |
| C | 7,360 run, 7,327 passed, 33 failed | the 33 known L1 tests only |

- **The 33 known L1 tests** are theseus-sandbox's contract tests, its bench's `spawn_100`, and theseusd's `sandbox`
  tests. They fail because L1 refuses a root daemon here (theseus-pv6i).
- **The `tests_horizon` failure** (a routine agenda after `routine.pause`) is in an area this branch does not touch.
  It passed 4 of 4 times run alone; it is not on the flaky list.
- **`theseus::herdr a_line_sent_on_to_main_does_not_leave_the_pane_sending`** failed once in a loaded package run
  while I tested row B (20 s), and passed alone and in each gate. It is not on the flaky list.

**The phases after the suite**, which I ran by hand each time (`machine_checks`):
- protocol types: clean, after `git add` of the regenerated `TurnSubmitParams.ts` for row B;
- lifecycle and jobs: skipped by `THESEUS_GATE_NO_BENCH`;
- turn (`theseus-sim bench turn --check --runs 5 --burst 0`): passed, plain 4 frames (budget 4), tool-call 8 (budget
  8), first requests within budget (cli 7,332 of 8,000; dm 9,411 of 15,000);
- size: skipped, no release build.

## Keel guard

`python3 scripts/keel-guard.py --base 6d4e0dd --head HEAD --quiet` (`origin/integ/threads` is not in this clone):

(empty output, exit 0)

## Live checks for the maintainer (scratch daemon, fresh state dir)

1. **Row A.** Make a thread and send a turn in it that edits a file under the session's directory and names a ticket:

   ```
   theseus --socket S thread new pr-round --title "pr round"
   theseus --socket S ask --thread pr-round "fix the typo in tarnlib/reviews/comment-1.md for PRR-4142"
   theseus --socket S thread show pr-round
   ```

   - `thread show` lists roots `tarnlib`, the file, and ref `PRR-4142`.
   - `theseus --socket S ledger -k thread.edited` shows one row with `changed: ["working_set"]` and who `harness`.
   - A second message with nothing new writes no further row.
   - In a place with that one thread, an unaddressed "comment-2.md under tarnlib/reviews" then routes to `pr-round`
     (Visible): the `thread.routed` row's `pre` has the hit `tarnlib`.
2. **Row B.**
   - `theseus --socket S ask -s <home> --route-at-ms 1700000000000 "x"` against a default config fails with the
     `[threads] bench_clock` message.
   - With `[threads] bench_clock = true`, run the threadbench A1 (smoke, seed 11). Every unaddressed message writes a
     `thread.routed` row.
3. **Row C.** With one live thread holding root `tarnlib`, an unaddressed "homesvc maintenance: branch
   chore/bump-1-8, ticket OPS-4214" from the CLI prints the question with `[1] … [n]ew … [h]ome`; answering `n` makes
   the thread. On Discord, the same message from the owner's DM gets the buttons; a voice turn runs at home.

**The threadbench targets** (run by the maintainer after the join):
- A1 writes a `thread.routed` row for every unaddressed message again;
- entity hits on 30% or more of the pre-router rows;
- the home-server stream gets a thread in 3 of 3 A1 runs;
- top-1 on clear messages 92% or more;
- asked-on-clear 15% or less;
- contamination not above its bar.

## Left, and choices the owner should hear about

- **Not in this branch:**
  - plan row 7, theseus-98bz.6 (a home turn that served a thread is claimed by it);
  - eviction from a full working set (at `MAX_SET` a list takes nothing more);
  - narrowing C9's entity-based `new` offer;
  - the Discord select's gateway test.
- **Row A's growth is broad.** Every path a turn's calls name joins the set, reads included (an `fs.read` of a file is
  "touched"), up to 64 per list. With no eviction, a long-lived thread fills its file list and then stops learning
  new files. Eviction by recency is the natural follow-up.
- **Absolute paths outside the session's directory are dropped.** If the owner wants such files kept (for example
  `/etc/...` for a home-server stream), `touched_in` should keep them whole.
- **An entity another live thread already holds is never added**, even when this thread is now the one working on
  it. This is the brief's isolation rule; a contested entity stays with whoever named it first until the owner edits.
- **Row C asks where today's lone thread ran at home.** That is a new wait of up to `ask_wait_secs` (900) for a typed
  message that names two new entities and that the thread does not hold. The switch restores today's behaviour.
- **Docs to change** (I did not edit them):
  - the spec's Part III item for these rows, and `docs/status.md` (the threads row);
  - `docs/design/` threads routing notes: the bench now routes at plan time (`route_at_ms`, `[threads] bench_clock`);
  - `bench/threads/README.md` already says it.

I added three lines to `crates/theseus-core/AGENTS.md`'s thread-router entry, one for each new key.
