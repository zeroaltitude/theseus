# CLOUD_REPORT: threads-routing (cloud/20261021-threads-routing)

Branch `cloud/20261021-threads-routing`, from `88ddae5` (the clone's `main`, store format 47; the brief's `1d092ded` is
its ancestor, and the keel guard's base). No store format change in any row. No new dependency. No line ceiling
raised, no file over one.

| Row | Plan id | State | Commit |
|---|---|---|---|
| 2: say why the lens drops a candidate | theseus-98bz.2 | done | `851caea` |
| 4: stickiness alone cannot overrule Jev | theseus-eqp1v | done | `74835a5` |
| 5: an ask always offers the right thread | theseus-98bz.4 | done | `b6a41fa` |
| 3: the bench routes at the plan's real gaps | theseus-98bz.3 | not started | |
| 6: a thread's working set grows from its own turns | theseus-98bz.5 | not started | |
| 7: a home turn that served a thread is claimed | theseus-98bz.6 | not started | |
| 12: a single thread's place offers `new` | theseus-98bz.7 | not started | |

Rows 3, 6, 7 and 12 were not reached: the session lost about an hour to the VM's disk allowance (below) and the gate
runs about 20 minutes a commit here. Nothing of them is on the branch.

## How this VM ran

- **Disk.** The first workspace build left 17 GB in `target/`, and `cargo nextest run -p <crate>` (per-package feature
  sets) rebuilt the dependencies beside it until the allowance was spent (`ENOSPC` in the harness's own temp dir). I
  deleted `target/debug/incremental` and the artifacts newer than the setup build, then ran everything with
  `CARGO_INCREMENTAL=0` and `--workspace` only. Nothing outside the repository and /tmp was touched.
- **The gate** was run as `CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each
  commit. Its results are in "The gate" at the end.

## Row 2: say why the lens drops a candidate (theseus-98bz.2), `851caea`

**Found.** `ThreadLens::judge` returned one `Verdict::Distant` for four rules, and recall wrote every one as
`thread`. No stored record carries `RecallDrop` (`tests_layouts.rs` has none): the recall manifest is a ledger row's
data (`recall.ran`, `recall.shadow`), so the new optional fields need no store format change.

**Changed.**
- `theseus-memory/src/threads.rs`: `Cause` (`Bar`, `Own`, `Entity`, `Base`) and `Distant { cause, own_top, share,
  paid }`; `Verdict::Distant(Distant)`. The rules' order is unchanged (entity, then bar, then own); `Base` replaces
  whichever rule took a node no thread claims. I did not touch the `own_top` field's doc or the top of `judge`, which
  the cross-stream row edits.
- `recall.rs`: `Reason::Thread(Cause)`, one variant, named `thread_bar`, `thread_own`, `thread_entity`,
  `thread_base`; `Reason::family()` gives `thread` for the four; `Dropped.distance: Option<Distant>`.
- `theseus-protocol/src/memory.rs`: `RecallDrop.own_top`, `share`, `paid`, optional, set only on a thread drop.
  `cockpit/src/protocol.gen/` regenerated.
- `theseus-core/src/recall.rs`: the drop's numbers onto `RecallDrop`; **the manifest's `drops` tally counts the four by
  their family, `thread`**. Design choice: `rpc/tests_thread_mix.rs` (the contamination set, "must pass unchanged")
  asserts `drops.get("thread")`, so the tally keeps the family and each `dropped[i].reason` names its rule. Readers:
  the CLI's `theseus memory` drop lines (`render/memory.rs`) now group by family and name each node's rule
  (`dropped 2 for thread: msg_3#0 (thread_own), msg_4#0 (thread_base)`); the cockpit (`ContextTurn.tsx`) and the bench
  read only the tally, which is unchanged. `rpc/tests_thread_recall.rs`'s one `dropped[0].reason == "thread"` became
  `"thread_bar"` (the drop it names is a bar drop).

**A tension in the brief, and what I did.** The cause definition says `base` wins over the other three; the msg-113
test says "a home node with share 0.93 ... gives `thread_own`". Those cannot both hold for a node no thread claims. I
followed the definition, and the test `tests_threads::a_drop_says_which_rule_and_the_numbers_it_weighed` checks the
msg-113 numbers both ways: the note as another thread's gives `thread_own` with own_top 0.104, share 0.93, paid 0.063;
the same note in the place's own conversation gives `thread_base` with the same numbers. If the owner wants the rule
named for a home node too (more useful when reading a bench), it is one line in `judge` (drop the `Base`
substitution), or a separate `base: bool` beside the cause.

**Proved.**
- Tests: `threads::tests::each_rule_of_the_distance_says_it_dropped_the_candidate` (one case per cause, with numbers),
  `recall::tests::each_filter_drops_with_its_reason` (every reason in `Reason::ALL`, now 17, met by some pack; the
  threaded pack meets the four), `tests_threads::a_drop_says_which_rule_and_the_numbers_it_weighed` (msg-113), the
  existing `tests_threads` cases now name their cause, `render::memory::tests::a_thread_drop_names_its_rule`.
  `cargo nextest run --workspace -E 'package(theseus-memory)'`: 93 passed; theseus (CLI) 795 passed; theseus-core's
  recall and thread tests 132 passed.
- Plant: every cause mapped to `bar` (`cause: Cause::Bar` in `judge`): 3 failed (`each_filter_drops_with_its_reason`,
  `a_drop_says_which_rule_and_the_numbers_it_weighed`, `a_question_shape_alone_pulls_no_other_streams_note`).
  Restored, touched, `git status` clean but for the row.

**Docs.** `crates/theseus-memory/AGENTS.md` updated in the commit. The spec's Part III item and `docs/status.md`
should name the four reasons and that the tally keeps `thread`.

## Row 4: stickiness alone cannot overrule Jev (theseus-eqp1v), `74835a5`

**Found.** As the brief says: `fuse` keeps 0.85 of a thread Jev did not pick, so a 0.88 sticky-plus-chain score fuses
to 0.748 and routes `visible`. Also: the extractor (`threads_import/extract.rs`) did not read an upper-case ticket
(`OPS-4214`, `PRR-4142`) at all, so the msg-5 message named one entity, not two.

**Changed.**
- `thread_route.rs`: `Pre.learned` (set by `rpc/thread_flow.rs`'s `learned_thread` when it lifts a score); `Rules {
  sticky_alone_asks, new_entities }` passed to the pure `decide`; `hold_sticky`: with the switch on and Jev answering,
  a top thread whose `Pre.entity` is 0 and not `learned`, which Jev did not pick, is held at `CONFIRM - 0.01` and the
  ranking re-sorted, so the decision falls to the ask. The ask offers `new` when Jev raised it or `rules.new_entities
  >= 2` (`new_entities(text, live)`: the extractor's refs, branches, files, hosts and repos of the message that no live
  thread's working set holds, case-insensitively; computed in `decide_with_jev`, only with the switch on). Design
  choice: the entity-based `new` applies to any ask while the switch is on, not only the one the cap made (the brief's
  "in that ask"); with no new entities nothing changes. OfferNew, `decisive`, `Voice::Alone` and `decide_single` are
  untouched.
- `extract.rs`: a ticket is a ref: 2 to 10 upper-case letters, a hyphen, 2 to 7 digits, but not a standard's name
  (`SHA`, `UTF`, `ISO`, `AES`, `CRC`, `MD`, `RFC`, `TLS`). This also reaches `thread import`'s working sets (the import's
  tests pass).
- `[threads] sticky_alone_asks = true` (after `ask_wait_secs`, in `config/threads.rs` and the template), its rollback.

**Proved.**
- Key test `thread_route::tests::stickiness_alone_does_not_overrule_jev` (the msg-5 text through `pre_route`: 0.88,
  no entity; `new_entities` 2; Jev security-fix at 0.3 asks `1 2 new home`; without new entities `1 2 home`; Jev `new`
  at 0.52 asks with `new`; off routes `visible` to the sticky thread; Jev `new` at 0.65 is still the offer).
- `evidence_beyond_stickiness_still_routes` (an entity hit routes `visible`; a learned lift; Jev agreeing; a router
  alone), `the_decision_table_by_band` unchanged but run through `today()` (the rules off), and
  `the_decision_table_is_the_same_with_the_sticky_rule_on` (each of the table's rows decides the same on and off; the
  existing "Jev overrules a sticky router when sure" row among them).
- `rpc::tests_thread_route::a_dissenting_jev_and_a_sticky_place_ask_instead_of_routing`: through the core, a PR
  message routes to pr-round, then the msg-5 message with a fake Jev dissenting: the row's pre score is 0.88, one
  `thread.asked` offering `new`, no `visible` route (nobody answers, so it runs home); with the key off, no ask and a
  `visible` route to pr-round.
- `threads_import::tests::a_trackers_ticket_is_a_ref`.
- Fixture: three messages appended to `fixtures/thread_route_40.json`: a security-fix probe (`check src/vault.rs
  again`), the msg-111 shape ("Make the calibration report rerunnable from a script.", tide-gauge, Jev 0.55, scripted
  `unclear`) and the msg-5 shape (truth `new`, Jev `new_thread` 0.52, `unclear`). The 40-message test and
  `scripts/thread-route-check.py` answer a `new` truth with the `new` key and count a fresh thread as right. At this
  commit the test read top-1 40/43 = 93%, asks 5 (1 of 39 clear), guesses 0; its misses were two asks that did not
  offer the right thread (row 5's problem) and one pre-existing (`which version does this target`).
- Plant: no cap (`*score = score.min(STICKY_CAP)` removed): `stickiness_alone_does_not_overrule_jev` and
  `a_dissenting_jev_and_a_sticky_place_ask_instead_of_routing` failed. Restored, touched.
- The decision table test was first split into helpers, which the keel guard read as `assert-removed`; I put it back
  in place (only `decide` → `today`, a five-argument wrapper), and the keel guard is clean.

## Row 5: an ask always offers the right thread (theseus-98bz.4), `b6a41fa`

**Changed.**
- `thread_route.rs`: `Choice::Other`; `askable` (live, unsealed); `ask_options(.., every)`: with `every`, every
  askable thread by fused score when four or fewer, else the best three and `other` (key `other`, label "another
  thread"); then `new` when raised, then `home`; off, the best two exactly as before. `question()` lists every thread
  named ("Is this about the a, the b or the c?") and ends "or another thread" with `other`; two and one read as before.
- `thread_flow.rs` (`State`): `Pending.slugs`, the live unsealed slugs the ask was asked over; `answer` takes an
  option's key or any of those slugs (case-folded); `other` itself and an unknown word are refused with the keys "or a
  thread's slug".
- `rpc/thread_flow.rs`: `Rules.ask_every_thread` from the config; `ask_over` fills the pending slugs and
  `ThreadAsk.others` (the threads `other` stands for); the `thread.answered` row's `choice` is the slug when a slug
  answered.
- Protocol `ThreadAsk.others: Vec<AskOption>` (empty skipped); TS regenerated.
- CLI (`thread/ask.rs`): `[o]ther (a thread's slug)` and a `threads: a b c d e` line; the command reads
  `…|<slug>|…`; `choice_of` takes a key, a label, or a listed thread's slug or title.
- Discord (`runtime/thread_ask.rs`, not `runtime.rs`): buttons five to a row; for `other`, a string select
  (`thr:<ask>:pick`, at most 25) of the threads in `others`; a pick answers with its value (the slug). Routed by the
  existing `notice_press` chain, so `runtime.rs` is unchanged.
- Bench: `th_owner.py` parses `[o]ther`, keeps each stream's slug, and answers a question that lists its thread by key,
  else by slug when `other` is offered. `th_threads.py` records the slug, and takes a thread made by a question's `new`
  answer as the stream's (it took only an offer's; with row 4 a question can offer `new`). That is a small edit outside
  the brief's list for row 5, needed for row 4's asks to score.
- `[threads] ask_every_thread = true`, its rollback.

**Proved.**
- `thread_route::tests::an_ask_lists_every_thread_up_to_four_then_three_and_other` (four: all four best first; five:
  `1 2 3 other new home` and its question; off: the best two; a sealed thread never listed), `thread_flow::tests::
  an_ask_takes_a_key_or_a_threads_slug` (the fifth thread's slug answers; `other` and an unknown word refused with the
  keys), the CLI's `another_thread_is_answered_by_its_slug`, Discord's `six_choices_take_two_rows_and_other_is_a_select`,
  `test_th_threads.Owner.test_a_question_that_lists_another_thread_is_answered_with_the_streams_slug` (21 Python tests
  OK). Existing assertions changed: none of the two-option tests changed (their places have two threads); the
  40-message check now offers all four threads.
- 52 of the router's, flow's, config's, CLI's and Discord's tests passed together. The 40-message check
  (`forty_unaddressed_messages_find_their_thread_and_ask_rarely`, 43 messages now) reads top-1 43/43 = 100%, asks 6
  (2 of 39 clear = 5.1%), guesses 0, against 40/43 at row 4's commit: the calibration and signing-key messages are now
  asked over all four threads and land right.
- Plant: back to two (`if n >= 2` in `ask_options`): `an_ask_lists_every_thread_up_to_four_then_three_and_other`
  failed, and the 40-message check fell back to 40/43 (93%) with the same two misses. Restored, touched.

## The live checks for the maintainer

On the owner's machine, with the installed build of this branch and a scratch daemon (`--config`, `--socket`,
`--state-dir` of its own):

1. `python3 scripts/thread-route-check.py` (the 43 messages on a real Jev): it should print top-1 of 90% or more, and
   the two new `unclear` messages should be asked about, the calibration one offering tide gauge and the homesvc one
   offering a new thread.
2. A threaded place with five threads (`theseus thread new` five times), a message none of them names: the ask lists
   three and `[o]ther`, and `theseus thread answer <ask> <fifth slug>` runs it in the fifth thread; on Discord the card
   has two rows and a select.
3. `theseus memory recalled <a thread session>` after a cross-stream question: each thread drop names its rule.
4. The threadbench (A1, k=3, seed 11) is the maintainer's to run after the join. The plan's targets: the home-server
   stream gets a thread in 3 of 3 A1 runs; top-1 on clear messages 92% or more; asked-on-clear 15% or less; entity hits
   on 30% or more of pre-router rows (row 6's, not reached, so expect this one to stay near 0); contamination not above
   its bar.

## Left, and follow-ups

- Rows 3, 6, 7 and 12: not started. Row 3's points are where the brief says (`route_to_thread`'s `now`, and
  `thread_routed_turn`, which `rpc/methods.rs` calls after the turn; a `Routing` field could carry the bench clock and
  the real start). Row 6's turn end is that same call site in `methods.rs` (after `self.runner.run`), which both a routed
  and an `ask --thread` turn pass; `rewind::written` is gone by then (`end_note_step` forgets it), so the touched paths
  must come from the turn's `tool_call` nodes.
- Without row 6 the working sets stay empty, so row 4's cap fires on most sticky placements Jev disagrees with: the
  asked-on-clear rate is the number to watch in the threadbench.
- `docs/`: the spec's Part III items for 98bz.2, eqp1v and 98bz.4, `docs/status.md`, and the core AGENTS.md's thread
  router paragraph (the ask's options and the sticky rule) should be updated at review; I left the core AGENTS.md alone
  (another row edits the same paragraph).

## The gate

Every commit was gated with `CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`. Each
run passed door, keel, fmt, shape, features, clippy, clippy-probe, cockpit, test build and the reader rule, and failed
only in the suite, on:

- the 33 known L1 tests (theseus-sandbox's contract tests and `bench spawn_100`, theseusd's `sandbox` tests: a root
  VM, theseus-pv6i), in every run;
- timing tests, each rerun alone: `theseus-judge builders::tests::every_builder_stays_under_its_cap_on_huge_inputs`
  (rows 2, 4 and 5's gates; passed alone each time), `theseus::markdown rendering_costs_little_per_hundred_lines`
  (row 2; passed alone), `theseus::herdr_up a_finished_tab_closes_a_grace_after_it_is_seen_and_a_cancelled_one_at_once`
  (row 5; passed alone), and `theseus::herdr a_change_reaches_herdr_within_ten_ms_at_the_99th_percentile` (row 5;
  failed once more alone, right after the suite; it passed in rows 2's and 4's gates, and this branch touches no herdr
  code, only `thread/ask.rs` and `render/memory.rs` in the CLI). Rerun after the gate it failed twice more alone (p99 11.6 ms against its 10 ms, p50
  1.5 ms) with the load average at 5.8 to 10 on 4 cores from the rebuild beside it; the test drives task views through
  `herdr`'s reporter, which no row here touches. **The maintainer should rerun it on the owner's machine**; I did not
  get a quiet window to rerun it here None of these is on the flaky list.

Totals: row 2's suite 7252 run, 7217 passed, 35 failed (33 L1, 2 timing); row 4's 7258, 7224 passed, 34 failed (33 L1,
1 timing); row 5's 7262, 7226 passed, 36 failed (33 L1, 3 timing).

The phases after the suite, run by hand each time: the protocol types (clean once `cockpit/src/protocol.gen` is
staged), the turn bench (`target/debug/theseus-sim bench turn --check --runs 5 --burst 0`: passed each time; the
first-request budget line read 6,585 of 8,000 tokens for a task), size (skipped: no release build), and `cargo deny
--offline check`: **advisories FAILED** on two advisories in the database the setup fetched today, RUSTSEC-2026-0381
and RUSTSEC-2026-0382 (root certificates and intermediate verification, under twilight-gateway's TLS stack); bans,
licences and sources ok. That is the dependency tree as on main (this branch adds no package); the maintainer's
deny-daily will see it too.

## Keel guard

`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` at `b6a41fa` (before this report's commit):
empty output, exit 0. (The gate's own keel phase on each commit: `keel: ok (... 0 findings acked)`.)
