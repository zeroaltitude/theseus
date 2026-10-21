# CLOUD REPORT: threads-cross (theseus-98bz.9, .8, .10; .11 not started)

Branch `cloud/20261021-threads-cross`, from `6c998b9` (the task commit on `88ddae5`). Session start 07:53 UTC,
report written ~12:20 UTC.

| Row | State | Commit |
| --- | --- | --- |
| 9: place digest, `classify.v4`'s `scope` | done | `b52a81e` |
| 8: `own_top` counts only what can still be admitted | done | `a5838e1` |
| 10: Jev judges the near-miss cross candidates, `recall_cross.v1` | done | `d336573` |
| 11: owner-confirmed thread links (stretch) | **not started** | none |

No file is over its line ceiling, no ceiling was raised, no dependency was added (Cargo.lock and the package-lock
files are unchanged), and the store format is **not** bumped (no stored record gained a field: see each row).

Things the maintainer should hear first:

- **The pack is `recall_cross.v1`, not `recall.cross.v1`.** A pack id must be lower snake case (`Rule::Identity`
  in `pack.rs`), so the dot form cannot load; the precedent is `people_seen.v1`.
- **`scope` has a source of its own (`Source::Scope`).** `only_when = "threads"` would have asked it only when the
  router's early judgment lists threads; an explicitly addressed or pre-router-decided message (the common case for
  the threadbench's "also this week" probes, which name a file in the thread's working set) would never be asked.
  Giving a thread's own turn the list would also ask `thread`, which
  `tests_thread_route::addressing_comes_first_and_is_certain` forbids ("addressing asks Jev no thread question").
  So the inbound state gains `in_thread: true` (left out when false, so every other state is byte for byte the one
  before) and `scope` is asked when threads are listed *or* the message arrives in a thread's own session.
- **The pack bump is not under the switch.** With `place_digest = false` no turn waits for `scope`, no `in_thread`
  is sent and no request carries a digest; but the router's early judgment, which lists threads, still asks
  `classify.v4`'s `scope` (and its reworded `thread`). Only the Jev request changes there, never the model's.
- **`cargo deny`'s advisories fail on this VM's freshly fetched database**: RUSTSEC-2026-0381 and RUSTSEC-2026-0382
  (bundled root certificates / distrusted intermediates, via the TLS stack under `twilight-gateway`). Not this
  branch's: Cargo.lock is untouched. Bans, licences and sources pass. `deny.toml` needs the owner's call (bump or
  ignore with a reason).
- **Disk.** This VM's writable allowance is about 39 GB; the gate's clippy plus an incremental test build of every
  crate does not fit. I ran every gate with `CARGO_INCREMENTAL=0` after a clean `target/` (artifacts once, ~20 GB).

## Row 9: a cross-scope request gets a place digest (theseus-98bz.9), `b52a81e`

**Found.** `classify.v3`'s `thread` question is asked only when the router lists live threads, which happens only
in `route_to_thread`'s non-decisive path (`decide_with_jev`). A turn addressed explicitly (`ask --thread`, `#slug`,
`/t slug`, "in the … thread:"), a reply to a routed turn, a single live thread, and a decisive pre-router all begin
the inbound point in the turn with `threads: Vec::new()`. **So, on main, an explicitly addressed turn does get a
`classify` judgment (every person's message does), but it carries no thread list and asks no `thread` question.**
With this row it is asked `scope` (via `in_thread`), and `tests_place_digest` drives every case through
`turn.submit` with `thread: Some(slug)`, the `ask --thread` path.

**Changed.**
- `crates/theseus-judge/packs/classify.v4.toml`: v3 word for word, plus `scope` (choice `thread`/`place`, no_match
  `thread`, decides nothing, `only_when = "scope"`), and `thread`'s instructions gain one leading sentence: "A cue at
  the start of a message that names a point of view or a voice picks how the answer is said, not which line of work
  it belongs to." (It leads because the loader requires instructions to end in a question.) v2 and v3 stay embedded.
  `CLASSIFY_PACK` is `classify.v4`; every reader of the name moved (core tests, theseusd tests, theseus-sim's perf
  judge, the template's comments); `learning/prove.rs` reads v4's `should_promote` as it read v1 to v3's.
- `builders.rs`: `InboundInput.in_thread` (state scalar `in_thread`, only when true) and `Source::Scope` (one item
  when threads are listed or `in_thread`), in a helper `threads_dynamic` (clippy's 100-line limit on `inbound`).
- Core: `Inbound.in_thread` (set by `turn/inbound_step.rs` when `[threads] place_digest` is on and the session is a
  thread's own; `false` at the router's, the brief's and the checkpoint's points); a `ScopeWait` channel on `Early`,
  filled by `hand_thread_answer` from `scope_answer` (`place` at the confirm bar, in a judgment that acts); the turn
  takes it in `inbound_point` (not for a steered message).
- `turn/digest_step.rs`: before the first loop's recall, in a thread's session only, `try_recv`, else wait at most
  `[threads] route_wait_ms`; on `place` it renders `compiler/place_digest.rs` (one line per other live thread,
  `- from thread «title»: goal: … | latest: <card's last line> | dated: <card lines with a YYYY-MM-DD>`, each cut to
  `place_digest_line_chars` (200), a held thread only as "N embargoed thread(s) (not shown)"). The text is attached
  by `task_graph::view::attach_text` in `compile_step::tail_blocks` (every loop of that turn), which moves the
  conversation's mark onto the last block first: the digest is after the last cache mark. A trace mark (`threads`,
  kind `place_digest`: scope, confidence, judgment, waited_ms, answered, shown, embargoed) records it. Nothing is
  stored: no store format change.
- Config: `[threads] place_digest = true`, `place_digest_line_chars = 200` (after `base_distance`), in the
  template, validated (at least 40).
- Eval: `eval_labelled.rs` (a labelled set runner shared with row 10; `jev-probe --eval classify.v4`), fixture
  `fixtures/eval/classify.v4.json`: the four probes `pipe.11`, `pr.11`, `qh.9`, `ops.8` → `place` (two with the
  router's list, two `in_thread` with none), "what's left on the tarnlib PR?" → `thread` (and `thread: pr-round`), a
  "Security view:" PR comment → `thread: pr-round`, and "Regenerate the report with the new cutoff." → `thread`.
  Golden request `fixtures/golden/classify.v4.request.json` from `fixtures/inputs/inbound.threads.json` (three
  threads, so `scope` shows).

**Proved.**
- theseus-judge: 207/207 (`tests_threads::classify_v4_is_classify_v3_with_a_scope_question`,
  `eval_labelled::tests::*`, the loader rules over all packs, goldens).
- `rpc/tests_place_digest.rs` (whole core, fake Jev): **`a_place_digest_never_shows_a_sealed_thread`** (five threads:
  the asker, two open, one sealed, one seal-pending; exactly two labelled lines and "2 embargoed threads (not
  shown).]", and none of the held threads' titles, goals, card words or hosts anywhere in the model request; the
  digest block sits after the block that carries the cache mark); `a_thread_scope_and_a_late_answer_get_no_digest`
  (scope `thread`; Jev at 3 s against the 400 ms wait, and the turn did not wait past it);
  `the_cached_prefix_is_the_same_bytes_with_and_without_a_digest`; `with_the_digest_off_the_request_is_the_one_before`.
  `compiler::place_digest::tests` (line shape, the cap, the asker left out, held only counted,
  `five_threads_fit_in_400_tokens` at the tail's 3 bytes a token), `config::threads::tests::the_place_digest_is_on_by_default_and_turns_off`.
- **Plants.** (1) *List every thread* (drop the held filter in `render`): the key test failed, `left: 5, right: 3`
  (lines). (2) *Ignore `scope`* (drop `.filter(|a| a.place)` in `digest_first`): `a_thread_scope_and_a_late_answer_get_no_digest`,
  `the_cached_prefix_is_the_same_bytes…` and `with_the_digest_off…` failed. Both restored and touched; `git status`
  clean of them.
- Whole theseus-core + judge + memory once before the gate: 3,887/3,888; the one failure
  (`tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused`) passed alone,
  under both TZs.

## Row 8: `own_top` counts only what can still be admitted (theseus-98bz.8), `a5838e1`

**Changed.** `ThreadScene::lens` takes the in-context set (its one caller, `Scene::lens`, passes
`&self.in_context`) and leaves in-context nodes out of `own_top` when `[threads] own_top_admissible` (default true)
is on. `beats_own`, the bar, `needs_entity` and `base` are untouched; the `own_top` doc comment in
`theseus-memory/src/threads.rs` says it. Config key after `base_distance`, in the template, named in the
streams-do-not-mix template test.

**Proved.** `rpc/tests_own_top.rs`: `a_note_in_context_holds_no_cross_note_back` (msg 113's numbers: cross 0.097,
own in-context 0.104 = top, no other own candidate → admitted, the own note `in_context`);
`an_own_note_that_can_be_admitted_still_outranks_it` (own 0.08 not in context > paid 0.063 → `thread`);
`with_the_key_off_the_old_verdict_is_back`. Every `tests_thread_mix.rs` case passes unchanged (114/114 with
theseus-memory and the thread-recall tests). **Plant**: count in-context notes again (drop the clause): the first
test failed at `tests_own_top.rs:134`; restored.

## Row 10: Jev judges the near-miss cross candidates (theseus-98bz.10), `d336573`

**Changed.**
- Pack `recall_cross.v1` (point `recall`, state `recall_cross`, cap 4,000, no live action): one per-item Noul
  `same_subject` ("Is the note numbered {item} about the same subject the message asks about, not only the same kind
  of fact?"). Builder `builders/cross.rs` (`Builder::RecallCross`, `CrossInput {message, notes: [{key, thread,
  text}]}`): message trimmed and clipped, notes numbered with `from_thread`, at most ten, each scrubbed and clipped;
  replay identity and rebuild reason added. Golden state and request; labelled eval set
  `fixtures/eval/recall_cross.v1.json` (msg 113's cutoff → high; another stream's port → low; the tarnlib merge
  date vs another stream's deadline → high/low; another stream's branch → low). `eval::check_expect` matches a
  per-item Noul by its asked id (`same_subject.1`).
- Core `judge/cross.rs`: `JudgeService::cross_live` (mode from the ladder, `WIRED` live; skipped when Jev is known
  unreachable), one request in a task of its own as the rerank's (state, blob, reservation beside the call,
  `judge.call` row through the sink with context `purpose: "recall"` and `cross: {asked, same_subject}`, budget
  settled); the turn waits at most `[threads] route_wait_ms`, with a `judge` span of kind `wait`. A yes is a Noul
  true at the confirm bar or above in a judgment that acts.
- `turn/cross_step.rs`: after the pack (in `recall_reranked`, before the rerank so the rerank and its repack see the
  admitted notes), the drops whose reason is the thread distance (`is_thread_drop(Reason) -> matches!(r,
  Reason::Thread)`, the one line the drop-cause split changes) and whose share of `top_fused` is above `bar`, best
  first, one per node, at most `cross_recall_judge_k`; a held thread's are filtered (they drop `sealed` before this
  anyway). On a yes, `ThreadScene::admit` names the nodes, and `Memory::refill` packs again with the same filters
  and budget, in the order the pack stood. Skipped when the place digest fired. `rerank_step.rs` builds `Parts`
  (the scene's owned half) once; with rerank off and no thread scene it is the path of before.
- Lens hook (theseus-memory `threads.rs`): `ThreadLens.admitted` (the last field) and one early return at the top
  of `judge` (`Costs(1 - penalty)`). The core fills it only with nodes whose thread is not held (`ThreadScene::lens`
  filters by `of_node`/`of_session`). The header names the thread as every other stream's note's does
  (`render::thread_mark`, via `other_title`); a note of the place's own conversation has no thread title, so it is
  admitted unlabelled (as a `Costs` base-distance note is today) and Jev's state calls it "the place's own
  conversation".
- Config: `cross_recall_judge = true`, `cross_recall_judge_k = 3` (1 to 10), after `base_distance`, in the template.

**Proved.** `rpc/tests_cross_judge.rs` (whole turns, recall live, fake Jev, `own_top_admissible` off so the lens
alone would drop): `a_yes_is_admitted_with_its_thread_and_a_no_stays_out` (the cutoff note in the request under
`from thread «datatidy stats»`, the port note still `thread`, one Jev request holding both);
`a_late_answer_leaves_the_drops` (Jev at 2 s); `k_caps_the_request_and_a_sealed_note_is_never_sent` (five near
misses → three `same_subject` questions, the best first; "zircon" in no Jev request);
`with_the_switch_off_nothing_is_asked`; `no_call_when_the_digest_fired` (classify live, scope `place`: the digest is
in the request, no `recall_cross` request, and the recall did have thread drops). theseus-memory
`tests_threads::a_note_jev_admitted_pays_and_passes_under_the_bar`; `config::threads::tests::the_cross_judge_reads_three_and_turns_off`;
builder test `each_note_is_asked_once_with_its_thread`. **Plants**: (1) *admit without asking* (every near miss
admitted, no call): `a_yes_is_admitted…`, `a_late_answer…` and `k_caps…` failed. (2) *drop the digest guard*: first
the test passed, which showed it was not exercising the step (the message "What else is going on this week?" named a
span, so recall kept no undated candidate, theseus-w9qv); with a message without time words and an assertion that
the recall had thread drops, it failed at `tests_cross_judge.rs:224`. Both restored.

Gate fixes made on the way, before each commit: clippy's 100-line limit on the judge's `inbound` builder (the
threads' dynamic items moved to `threads_dynamic`) and on `recall_reranked` (`Parts::of`), and the keel guard's
finding on an `#[allow(clippy::too_many_arguments)]` I had added (removed: the function has seven parameters).

## Row 11: not started

Nothing of it is on the branch. What it needs, as read: `ThreadInfo.links` (a stored record, META `thread:<slug>`:
bump `MANIFEST_FORMAT` from 47 and add the format-47 literal to `tests_layouts.rs`), `ThreadEditParams` link/unlink
fields, both ends written in one `threads.write`, refused for a sealed or seal-pending end, ledger kinds
`thread.linked`/`thread.unlinked` after `ThreadUnsealed`, the CLI's `thread link|unlink`, and in `thread_scene` the
asker's linked slugs joining `waived`.

## The gate

Each commit's gate (`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`) passed every
phase to the suite: door, keel, fmt, shape, features, clippy, clippy-probe, cockpit, test build, reader rule. The
suite failed only on:
- the 33 known L1 failures (theseus-sandbox's contract tests and `spawn_100`, theseusd's `sandbox` tests: a root
  daemon's job with no job cgroup, theseus-pv6i), in all three runs;
- row 8's run also: `theseus-discord tests_gateway::horizon::a_met_watch_posts_once_and_pings` and
  `theseusd::gone_jobs a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`: both passed alone
  (timing under the suite's load; outside this branch's code).
- row 10's first run also failed six tests that list the wired packs and their modes (`tests_continue`,
  `tests_inbound` x1 (its `INBOUND_ALONE` array grew from 17 to 18), `tests_judge` x2, theseusd `judge`, and the
  acceptance journey's timing): the lists gained `"recall_cross.v1: live"` after the rerank's line, nothing
  removed; the journey passed alone. The final run failed only on the 33 and two more, both passing alone:
  `theseus::markdown rendering_costs_little_per_hundred_lines` (2.2 ms per 100 lines under the suite's load;
  the crate is untouched here) and `theseusd::judge a_turn_is_judged_after_it_and_asks_the_model_the_same_bytes`
  (its two daemons straddled a minute: `05:11` against `05:12` in the turn's time line).
- Before the gates, row 9's whole-package run: `tests_horizon::the_agenda_expands_a_series…` failed once under load
  and passed alone.

After the suite I ran the rest of `machine_checks` myself: protocol types unchanged (no wire type changed),
lifecycle and jobs skipped by `THESEUS_GATE_NO_BENCH`, the turn bench (`theseus-sim bench turn --check --runs 5
--burst 0`) passed: a plain turn 4 frames, a tool call 8, first-request tokens dm 9,411/15,000, headless
7,846/8,600, task 6,585/8,000, cli 7,332/8,000; size skipped (no release build). `deny`: advisories fail as above,
bans/licences/sources pass.

Keel guard before the report: `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed
nothing (exit 0).

## What the maintainer should run

On the owner's machine, after the join (Jev's evals need the key; the VM has none):

```
cargo run -p theseus-judge --features probe --bin jev-probe -- --eval classify.v4
cargo run -p theseus-judge --features probe --bin jev-probe -- --eval recall_cross.v1
```
Each prints `EVAL <case> <question>: wanted …, got … ... met|MISSED` and a tally; `--fake` dry-runs the plumbing
(on this VM: `== eval classify.v4: 7 case(s); 9 expectation(s) met, 0 missed`, and `== eval recall_cross.v1: 4 case(s); 5 expectation(s) met, 0 missed`).

Live check on a scratch daemon of this build (fresh state dir, own socket), with two or more threads one of which is
sealed:
1. `theseus thread new` three threads with goals and cards (one card line dated `2026-11-19`), seal one
   (`theseus thread seal <slug>`), then `theseus ask --thread <a> "Add an 'also this week' section covering
   anything else we are shipping."`. Expect: the turn's trace (`theseus why` / the trace view) has a `threads`
   mark `place_digest` with `scope: place`, `shown: 1`, `embargoed: 1`; the provider request's last user message
   ends with `[Place digest: … - from thread «<b>»: goal: … | latest: … | dated: …` and "1 embargoed thread (not
   shown).]"; the sealed thread's title appears nowhere in the request.
2. `theseus ask --thread <a> "what's left here?"`: the mark says `scope: thread`, no digest.
3. Set `[threads] place_digest = false` and repeat 1: no mark, no digest.
4. Row 10: in thread `a`, a message whose answer is a fact said in thread `b` (or at home) that the lens drops:
   `judge.call` rows of pack `recall_cross.v1` with `context.cross.same_subject`, and on a yes the recall note's
   header `[from thread «b», …]`.

## Threadbench numbers to read after the join (plan targets, row-1 scorer)

Reach on the "also this week" sections ≥ 5/6 for A1 on the PR-round and pipeline streams; sealed leaks 0; embargo
probes 4/4; cross-stream contamination ≤ A3 + 0.5 per 12, owner-check contamination ≤ A3 + 0.5 per 26; cross cites
≥ 11/12; request tokens ≤ 45% of A3's (the digest is ≤ 400 tokens for five threads at the default cap); Jev's cents
≤ 10% of the run's dollars; p50 turn latency up ≤ 1 s (a threaded turn may wait up to 400 ms for `scope` and up to
400 ms for `recall_cross`); the pipeline summary right 3/3; and Jev's yes rate on the drops the scorer marks
contaminating. Stop rule: if contamination moves above its bar, `own_top_admissible = false` first.

## Follow-ups and uncertain points

- The digest waits for `scope` before the recall begins; overlapping that wait with the index's would save up to
  400 ms on a threaded turn whose batch is slow.
- `recall_cross.v1` shares the shared Jev breaker (not one of its own as rerank has).
- A home-conversation note admitted by Jev carries no `from thread` label (it has no thread); if the owner wants
  every cross note labelled, `hold_node` would need a label for base-distance notes.
- The cockpit's "why" view of a digest, and the TUI: not done.
- Docs to update at review: theseus-core `AGENTS.md`'s thread-recall paragraph (the three keys, the digest, the
  cross judge) and the inbound paragraph (`classify.v4`); `docs/status.md`'s Jev line (`classify.v4`,
  `recall_cross.v1`); the spec's Part III item for theseus-98bz.8/.9/.10.
