# CLOUD_REPORT: tail2-threads

Branch `cloud/20261021-tail2-threads`, on `origin/integ/threads` at `8b641b6b` (with the task commit `f5b98fb`).

## The one `thread_flow.rs` line

`crates/theseus-core/src/rpc/thread_flow.rs`, in `place_unaddressed`, right after `self.learned_thread(&p.input, &mut pre);`:

```rust
        self.keep_apart_by_dir(p, session, live, &mut pre);
```

Nothing else in that file changed. `keep_apart_by_dir` lives in `rpc/home_claim.rs` (a file of this row), so the sibling
row's edits to `thread_flow.rs` merge around one line. No file is over its line ceiling.

## Per Beads id

Commits (subjects carry the ids; hashes as pushed):

| Commit | Issues | What |
|---|---|---|
| `ae5bf71` | theseus-vw8k7, theseus-q3ggn | the directory rule, the lifted roots predicate, the cap-gated `new` offer, their tests |
| `29163ac` | theseus-pt7lt, theseus-1uiiu | `recall_cross`'s own breaker and its test; the digest test's bar assertion; the rerank test's breaker count |
| `5a785fd` | theseus-pt7lt | rustfmt of that rerank assertion (the gate's fmt phase caught it) |
| `1b058a2` | theseus-l4n39 | the claim and unclaim under the home's execution lock, `kept`'s count, three tests |
| `170ba73` | theseus-powrh | the two families rigs wait 2,000 ms |
| `6e44770` | theseus-6juq4, zokhn, 3vb1z, 8cl9f | the guide, `docs/status.md`, `docs/design/threads.md` and its index line |

`ae5bf71` carries an intermediate `home_claim.rs` (the predicate's lift and `keep_apart_by_dir` only); `1b058a2` adds
the l4n39 half. Only the final tree went through the whole gate; each commit's own tests ran on the tree they landed in.

**One edit outside the row's file list:** `crates/theseus-core/src/tests_rerank_live.rs`.
`five_rerank_timeouts_open_reranks_breaker_alone` asserted `h.breakers.len() == 1`: health lists every breaker of its
own, and there are two now. It asserts two, rerank's open and `recall_cross`'s closed (it failed in the first gate
with `left: 2, right: 1`). Commits are signed with the VM's configured SSH key (`gpg.format = ssh`); signing was not
turned off.

- **theseus-vw8k7** (the directory rule). `thread_route::apart_by_dir` (pure, over `&mut [Pre]`, the threads, the
  message, the directory, a base for relative roots, and `home`), called through `Core::keep_apart_by_dir`. The roots
  predicate is lifted out of `home_claim::thread_of` as `thread_route::entry_path` and `holds_path`; `thread_of` now
  calls them. The module doc's step 2 says the rule. `pre_route`'s signature is unchanged.
- **theseus-q3ggn**. `hold_sticky` returns whether it capped the top thread; `decide` keeps it; the ask's `raised` is
  `Jev raised New || (held && new_entities >= 2)`. `lone_offer` unchanged. No existing assertion changed.
- **theseus-pt7lt**. `cross::BREAKER = "recall_cross"`; `built()` chains
  `.with_breaker(cross::BREAKER, &[cross::CROSS_PACK], BreakerConfig::default())`; `cross_live` returns `None` at once,
  with no mark, no call and no wait, while that breaker is open (`cross_breaker_open`, reading `own_breakers()` as
  `rerank_no_wait` does). `sink.rs` is unchanged: its `breaker_of` already names the pack's own breaker on the
  `judge.circuit` row (the new test asserts `breaker: "recall_cross"`), and its one rerank arm, `count_opened`, feeds
  rerank's ladder day brake, which the cross pack has no rule for. The ladder (`judge/ladder/adopt.rs`) is untouched:
  `recall_cross.v1` has no adoption rule, so a cross breaker opening lands no ladder event. Whether it should is the
  owner's call.
- **theseus-1uiiu**. `no_call_when_the_digest_fired` now asserts a `thread_*` drop whose `share` is above
  `cross_recall_bar`, printing the drops. Today's fixture has one, so the test was not vacuous; it can no longer become
  so.
- **theseus-powrh**. `a_family_jev_judges_relevant_loads_without_being_named` and `one_families_judgment_per_prompt` set
  `c.tools.defer.judge_wait_ms = 2_000` in their rigs (a rig's wait, not a default; the default and the deadline test
  are unchanged). `families_start` unchanged; the owner's decision is written in the docs.
- **theseus-l4n39**. `thread_unclaim`'s `kept` counts only keys holding another claim's or a split's value; a null key
  is neither (doc comment on `thread_unclaim`). The claim and the unclaim read the keys and write in one kernel frame
  under the home session's execution lock (`Core::under_exec`). Tests for the count, the pages and the race.
- **theseus-6juq4, zokhn, 3vb1z, 8cl9f** (docs). `crates/theseus-core/AGENTS.md`: the cross judge, its bar, k and own
  breaker; the place digest and `Inbound.in_thread`; the ask's options, the sticky rule with the cap gate, the ticket
  shape, the directory rule; `route_at_ms` under `bench_clock`; growth's `MAX_SET` and no eviction; the one-thread offer
  was already there; the families call's owner decision; the claim's lock and `kept`. `docs/status.md`: the Jev entry
  names `classify.v4` and `recall_cross.v1`; the threads entry says what the router, growth, the claim, links and the
  cross judge do; the detours line says a quick detour carries recall, the slice rides any detour, `theseus why` names a
  detour and `context.explain` shows its parts. Its "Updated" line is left for the maintainer's docs commit. New
  `docs/design/threads.md`, and its line in `docs/design/README.md`.

## theseus-vw8k7: the reproduction and the rule

The key test `a_message_from_another_directory_is_not_placed_by_stickiness` (two threads, `dev` with root
`/w/projects/lighthouse` and `ops`; a dry run routed to ops and moved by the owner to dev, so the correction is learned
and dev is the place's last thread; then `[dry-run] Start 2 tasks, $1 each: bash slow.sh census 2` with `dir`
`/w/projects/walkthrough`, Jev scripted to say home). On the base (the rule's call removed), the `thread.routed` row:

```
"how":"quiet","jev":null,"pre":[{"hits":[],"score":0.0,"thread":"ops"},{"hits":[],"score":0.92,"thread":"dev"}],"to":"dev"
```

The learned correction alone lifted dev to `LEARNED` (0.92), decisive, so it routed quietly with no Jev at all. That
test runs with `[threads] working_set_grows = false`, to try the priors alone.

**The reproduction found a second cause**, which I believe is the walkthrough's real one: growth. With growth on (the
default), the first dry run's turn in a thread adds the message's entities to that thread's working set, `slow.sh`
among them, as a bare relative file. The next dry run names `slow.sh`: an entity hit (`STRONG`, 0.92), and with
stickiness and the chain, decisive. The second key test, `a_file_named_from_another_directory_is_not_the_threads`
(the first dry run routed to dev by Jev, growth on), on the base:

```
"how":"quiet","jev":null,"pre":[{"hits":[],"score":0.0,"thread":"ops"},{"hits":["slow.sh"],"score":0.968,"thread":"dev"}],"to":"dev"
```

So the rule fixes both, and the test was not fitted to the hypothesis.

**The rule** (owner-visible): when the turn's directory is known (the submit's `dir`, else the session's own `dir`) and
a thread has roots (absolute or `~/`; a relative root is read under the thread's own session's directory, where growth
took it; with none it is not read), none of which holds the turn's directory or lies under it, that thread keeps only
the entity evidence that names it:

- stickiness, the reply chain and a learned correction count for nothing;
- a file or root hit counts only where the message names the entry's absolute path whole: a bare or relative name
  (`slow.sh`) is another directory's file;
- a ref, branch, host or repo hit still counts, and Jev's pick still fuses afterwards.

A thread with no readable root, or a turn with no directory, routes exactly as before; so does a turn from under a root,
or from a directory above one (`~/projects` with a root `~/projects/lighthouse`: "related", since the work may be
there). What changes for the owner: a message typed in a directory unrelated to every root of a thread is placed there
only when it names that thread's ref/branch/host/repo or an absolute path in it, or when Jev picks it; otherwise Jev
decides (here: home), or it is asked. To turn it back: delete the one line in `place_unaddressed`. I added no
`[threads]` switch.

`may_route` is unchanged: a message from elsewhere may still name a thread's PR, and Jev still reads every message, so
not routing at all would lose those; the rule removes only the priors that name nothing of the thread.

Known limit, for the owner: growth itself still files a moved message's entities in the thread the turn first ran in
(the wrong one), and grows message words that are bare file names. Not this row's (`thread_grow.rs`); worth an issue.

## theseus-l4n39: the race decision

(b): the lock. A split writes its `moved.of.<node>` keys in `self.kernel.frame(&[&exec], …)` over the source session's
execution (`thread_home.rs` `write_moved`; the unsplit's frames too), and refuses while a turn runs there. The claim
runs right after the home turn ends, exactly when a split may begin, so (c) is not guaranteed by the code. (a) only
narrows the window. So `try_claim` and `thread_unclaim` read the keys and stage their records inside one
`kernel.frame` over the home session's execution (`Core::under_exec`; a home with no execution has no turn and can take
no split, so it appends as before). A split lands wholly before the read (its value is kept) or after the write (it
takes the claimed nodes, as a split may). No new lock type. The reason is in `home_claim.rs`'s module doc and
`docs/design/threads.md`.

## Tests and plants

Each plant: break, run, see the line fail, restore from a copy and `touch`, `git status` showing only this row's files.

| Test | Plant | What failed |
|---|---|---|
| `rpc::tests_thread_route::a_message_from_another_directory_is_not_placed_by_stickiness` (key) | the `keep_apart_by_dir` line removed | `tests_thread_route.rs:1460`: the walkthrough case ran in dev (`"to":"dev","how":"quiet"`, row above) |
| `rpc::tests_thread_route::a_file_named_from_another_directory_is_not_the_threads` (key, growth) | same | `tests_thread_route.rs:1508`: ran in dev on the `slow.sh` hit |
| `thread_route::tests::a_thread_apart_from_the_turns_directory_keeps_only_its_entities`, `a_bare_file_name_from_another_directory_is_no_hit` | pure tests of the rule (the twins: under a root, above one, no dir, relative dir, no roots, unread relative roots, a ref still counts, an absolute path still counts) | (no plant beyond the call's; they exercise `apart_by_dir` directly) |
| `thread_route::tests::the_new_offer_rides_only_the_ask_the_cap_made` | the old `raised` (`rules.sticky_alone_asks && …`) | `thread_route.rs:2042` "no cap, no new": the ask offered `new` |
| `rpc::tests_cross_judge::a_failing_cross_judge_opens_only_its_own_breaker` | the `.with_breaker(cross::BREAKER …)` line removed | `tests_cross_judge.rs:288`: the `judge.circuit` row had no `breaker` (the shared one opened) |
| `rpc::tests_cross_judge::no_call_when_the_digest_fired` | `cross_recall_bar = 0.999` in its rig | `tests_cross_judge.rs:239` "no thread drop above the bar 0.999, so no call was owed" (the older assertion still passed: the vacuous case) |
| `rpc::tests_home_claim::an_unclaim_counts_a_nulled_node_as_neither` | `None => kept += 1` | `tests_home_claim.rs:713`: `restored: 0, kept: 5` |
| `rpc::tests_home_claim::a_long_home_turn_is_read_across_pages` (70 extra nodes, then 1,024 more) | the page loop stops after one page | `tests_home_claim.rs:743` "every node of the turn, across pages" |
| `rpc::tests_home_claim::a_split_landing_beside_the_claim_is_kept` | the claim's read and write outside the frame | `tests_home_claim.rs:799`: the record held the split's node |

The breaker test holds only the cross requests at the fake Jev (`hold_when` on the port note, which only the cross
state carries), so five cross calls time out at their 5 s urgency; the shared breaker stays closed, health lists
`recall_cross: open`, and the sixth turn sends a classify request and no cross request.

Line numbers in the table are as the files stood at each plant (one unit test was split for clippy afterwards).

## Live checks for the maintainer

On a scratch daemon of this build (its own `--config`, `--socket`, `--state-dir`, a fresh state dir; Jev's key needed
for the router's question), with two threads, one with an absolute root:

```sh
S=/tmp/tt/sock
theseus --socket $S thread new dev --title "lighthouse dev"
theseus --socket $S thread edit dev --root ~/projects/lighthouse
theseus --socket $S thread new ops --title "ops rota"
cd ~/projects/lighthouse && theseus --socket $S chat                 # send: "[dry-run] Start 2 tasks, $1 each: bash slow.sh census 1"
cd /tmp && theseus --socket $S chat                                   # send: "[dry-run] Start 2 tasks, $1 each: bash slow.sh census 2"
theseus --socket $S ledger --kind thread.routed -n 2 --json
```

What each should show: the first dry run, from under dev's root, may go to dev (and dev's working set grows
`slow.sh`). The second, from `/tmp`, must not be placed in dev by stickiness or by the `slow.sh` hit: its
`thread.routed` row's `pre` has dev at 0 (no hits), and the decision is Jev's (home, a thread, or an ask). The same
second message sent from `~/projects/lighthouse` still goes to dev. Both chats must be the same place's session (the
chat's own session is the place's home), so the second is routed with the first as its last thread. I could not run a
daemon with a model key here; the in-process tests are the proof I have.

The cross breaker: with `[threads] cross_recall_judge` on and Jev unreachable for `recall_cross` only, health's judge
line lists `recall_cross: open` after five failures and the shared breaker stays `closed`; this needs a stand-in Jev, so
the in-process test is the proof here.

## The families loop

20 runs of `cargo nextest run -p theseus-core --retries 0 --no-fail-fast -E 'test(tests_families)'` at `nice -n 19`,
beside four `sh -c 'while :; do :; done'` loops at nice 0 (killed by pid):

| Test | Passed | Failed |
|---|---|---|
| `a_family_jev_judges_relevant_loads_without_being_named` | 20 | 0 |
| `one_families_judgment_per_prompt` | 20 | 0 |
| `a_cli_detour_naming_aws_loads_it_past_the_budget_by_exactly_its_size` | 20 | 0 |
| `a_model_with_tool_search_asks_no_families_judgment_and_raises_nothing` | 20 | 0 |
| `jev_late_or_unreachable_falls_back_to_the_named_rule` | 0 | 20 |

The last is the deadline test the brief says to leave alone: it fails every time under this load (about 28 s a run) and
passes alone without load (0.35 s). It times the wait against `judge_wait_ms + 250 ms`, which four busy loops on four
cores break. Not changed; it is not on the flaky list, and the owner may want it waited on as an event.

## The gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on `5a785fd`: door, keel ("0
findings"), fmt, shape ("no file over 2500 lines but the 8 listed"), features, clippy, clippy-probe, cockpit, test
build and the reader rule passed; **the suite failed on 35 of 7,580** (7,545 passed, 53 skipped):

- 34 L1 tests, the VM's known root-daemon case (theseus-pv6i): theseus-sandbox's contract clauses 01 to 12, the 18b
  egress tests, `a_job_that_cannot_start_says_why`, `exit_status_and_signals`, `scratch_is_reported_and_discarded`,
  `sigterm_is_forwarded_to_the_command`, `bench spawn_100`, and theseusd's `sandbox` tests. None is in this row's files.
- `theseus::herdr a_line_sent_on_to_main_does_not_leave_the_pane_sending` (20 s, under the suite's load): passes alone
  (0.075 s). Not on the flaky list; not this row's (the CLI crate).

The first gate run (before the rerank fix) also failed `tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused`
and theseusd's `gone_jobs::a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`; both passed in the
second full run, and the horizon one alone. Neither is near this row's code.

The phases after the suite, run by hand: protocol types (`cockpit/src/protocol.gen` unchanged: no protocol type
changed); the turn bench (`target/debug/theseus-sim bench turn --check --runs 5 --burst 0`, exit 0: 4 frames a plain
turn, every first-request budget ok); lifecycle and jobs benches skipped (`THESEUS_GATE_NO_BENCH`); size skipped (no
release build). `tests_m3::a_plain_turn_stays_within_its_frame_budget` passed in the suite. **The deny phase did not
run**: the setup's `cargo deny fetch` was lost to a container restart, and `cargo deny --offline` needs the database.
`Cargo.lock` and `cockpit/package-lock.json` are unchanged from the base.

`cargo clippy --workspace --all-targets -- -D warnings`: clean (after splitting one unit test clippy's 100-line limit
caught). The theseus-core and theseus-judge suites ran whole inside the gate's suite.

Environment notes: the container restarted twice, and the disk allowance filled twice (the incremental cache, 19 GB),
which killed one gate run; later builds ran with `CARGO_INCREMENTAL=0` after `target/` was removed.

## The keel guard

`python3 scripts/keel-guard.py --base origin/integ/threads --head HEAD --quiet` printed nothing and exited 0 (run at
`6e44770`; `5a785fd` only reformats one test). The gate's own keel phase: `keel: ok (… 0 findings acked)`. No test was
deleted, ignored or loosened; the one changed assertion (the rerank breaker count) is stricter, not looser.

## Store format

Unchanged at 48. No stored field, record kind or encoding changed.

## Part III items for the maintainer

- **theseus-98bz.2**: in a thread, another thread's notes pay the distance and are dropped below the bar unless named,
  with `thread_*` drops carrying `own_top`, `share` and `paid`.
- **theseus-eqp1v**: stickiness alone no longer overrules Jev: a sticky-only top thread Jev did not pick is held below
  the confirm bar and the message is asked about, with a new-thread option when it names two new entities.
- **theseus-98bz.4**: an ask lists every live, unsealed thread up to four, else three and `other` answered by a slug.
- **theseus-98bz.8**: the thread lens: a thread's recall no longer pulls another stream's facts (provenance by node,
  the base distance, entity ties).
- **theseus-98bz.9**: the place digest: `classify.v4`'s `scope` answer `place` gives that turn a digest of the place's
  other live threads.
- **theseus-98bz.10**: the cross judge: `recall_cross.v1` reads the near-miss drops and admits those about the same
  subject.
- **theseus-98bz.3**: the bench's clock: `route_at_ms`, under `[threads] bench_clock`, refused without it.
- **theseus-98bz.5**: a thread's working set grows from its turns' paths and the message's entities.
- **theseus-98bz.7**: a place with one live thread offers a new one beside it when the message is not clearly about it
  and names two new entities.
- **theseus-xlri5**: deferred-tool families a message names or Jev judges relevant load past the place's budget, under
  `admit_ceiling`, said in `TurnSubmitResult.raised`.
- **theseus-98bz.6**: a home turn that wrote only one thread's files is claimed by that thread; `thread.unclaim` undoes it.
- **theseus-v4u66**: the conversation's project slice rides a detour's request.
- **theseus-r5ydi**: `context.explain` explains a detoured turn from its `context.detoured` row.
- **theseus-3bs4e**: a detour's whole first request is held to the place's budget as sent.
- **theseus-vw8k7** (this row): a turn's directory apart from a thread's roots keeps the priors, a learned correction
  and bare file names from placing a message there.

(This row's smaller ones, if the maintainer records them: q3ggn the `new` offer only on the cap's ask; pt7lt the cross
judge's own breaker; l4n39 the claim under the home's execution lock and `kept`'s count; 1uiiu and powrh tests.)

## The `TurnSubmitResult.raised` doc line

For `crates/theseus-protocol/src/lib.rs`, above `pub raised: Option<route::ToolsRaised>,`:

```rust
    /// The deferred-tool families this turn's first request loaded past its place's budget, each with its tokens and
    /// who admitted it (`named` or `jev`), under `[tools.defer] admit_ceiling` (theseus-xlri5); none when nothing passed.
```

## Follow-ups found

- Growth (`thread_grow.rs`) files a moved message's entities in the thread the turn ran in, and grows bare file names
  from message words; the second is what made dry runs "name" a thread.
- `jev_late_or_unreachable_falls_back_to_the_named_rule` fails under load every time (above).
- A cross breaker opening lands no ladder event (no adoption rule for `recall_cross.v1`); the owner may want one.
- `theseus::herdr a_line_sent_on_to_main_does_not_leave_the_pane_sending` failed once under the suite's load here.
