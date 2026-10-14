# CLOUD_REPORT: own-benchmark (a, b), theseus-pw1q.12

Branch `cloud/20261013-own-benchmark`, built on main at `0baeed6`. Store format 33 -> **34**.

## What was built (commits 48dd94c, 7940680, f855042, 3f261d7, 00bdd0e, 6df2976)

1. **(a) System blocks by digest.** `BlockBreakpoint` (the compilation's cache layout) gains `digest` (SHA-256 of the
   block's text). `compiler/cache.rs` (`stamp`, `keep`): at a new compilation's persist (`compile_step.rs`,
   `route_step.rs::keep_first`) the request's own system texts are stamped and written once to the blobs in one
   `put_many` before the frame that names them. Digests come from the *request's* blocks, so guidance and context
   files are covered. Old compilations have no digest and a replay says so. `cache.rs` also took `impl CacheLayout`
   out of `compiler.rs` (now 2,547 of 2,560 lines).
2. **`turn.replay {session, turn, profile, effort?, scope?, compare?}`** (`own_bench/{rebuild,replay}.rs`). Rebuilds
   the first request (system blocks from blobs, tools checked against the manifest's digest, the nodes before the
   first answer, trying the profile's effort and then each level) and accepts it only when its digest is the one
   the answer recorded (`request_digest`). Otherwise `NotRebuildable{part, why}` (`system`, `tools`, `messages`,
   `compilation`, `profile`, `answer`, `session`). Sends it to the profile with no tool run (reserved, held on the day
   ceiling, priced), judges with `compare.v1`, writes a `turn.replayed` row and nothing else. Owner's run
   (`judge_act(Act::JudgeRun)`); `theseus turn replay`, in `OPERATORS`. Divergence from the brief: it takes `session`
   (a turn id does not name its session).
3. **(b) `compare.v1`** (`theseus-judge`: pack, builder `compare`, point `replay`): one Choice
   (`same`/`better`/`weaker`/`different`/`other`) over request, reply A, reply B. `WIRED` in shadow, no action. Pack
   goldens (`compare.state.json`, `compare.v1.request.json`) and the builder tests. Holdout baseline: the owner's labels
   and route corrections (the pack ships in shadow; nothing in this row adds a learning-report line beyond the
   standard per-pack ones the report already prints for any pack: **not separately built, see "left"**).
4. **The sampler** (`own_bench/sampler.rs`, `rung.rs`; `[route] replay_day_usd`, 1.00, 0 = off, in the template with
   its test). Nightly two hours after `[judge] learning_hour`, missed night once, not within 10 min of a start, machine
   quiet before each replay. Per (model, effort) cell of the last 7 days, the newest 3 unjudged turns, one rung down
   the routing table (`[routing.modes.*]` profiles, a model once, by catalog price). The day's spend is read back
   from `route.replay.judged` rows; the reservation is checked against what is left **before** the call. Never a
   shared place's turn. **Cross-provider check** (`Core::crossing`): if the target provider is not the original's,
   refuse when the session is in a sealed thread, a recall item's source is an imported `personal` /
   `partner-confidential` episode, or the rebuilt messages contain an image/document block.
5. **`theseus route report [--week]`** and `route.report` (`own_bench/report.rs`, any surface, a read): per cell turns,
   spend, replayed, matched (same+better >= 0.8), you-checked (= turns with a `route.corrected` row), one-sided 90%
   Wilson floor, `movable = spend x floor x (1 - replay/original first-request cost)`, replays' cost vs the cap. The
   cockpit Benchmarks view has a "Your work" panel (`YourWork.tsx`, `lib/yourwork.ts`, explainer `bench.yourwork`).

Defect found in code I was in, fixed with a test: an empty float sum is `-0.0`, so the report printed `$-0.00`
(`an_empty_report_totals_a_positive_zero`, 6df2976). Existing `tests_m3::blob_files` counted every blob and now
excludes system blocks (they are blobs now).

## Proof

- `cargo nextest run -p theseus-core -E 'test(/own_bench/)'`: 23 pass (blocks once per distinct text; byte-identical
  rebuild == the request the provider got, with `first_request.golden.json`; corrupt/missing blob and pre-34
  compilation refused naming `system`; replay writes one row, no node/session; replay uses the turn's *own* system
  after the context file changed; MCP surface refused with no call; shared place refused; compare via fake Jev;
  sampler: 3 of 4 turns one rung down to Haiku, none twice; cap before the call; spent day; zero cap off; shared place
  left alone; attachment never to another provider). Plus rung/report unit tests, judge builder tests and goldens,
  the CLI report-lines test, `yourwork.test.ts`, and the format-34 layout tests (`COMPILATION_AT_34`, a 33 store).
- **Planted reverts** (restore + `touch`, `git status` clean after each):
  R1 replay uses the freshly built system instead of the blobs -> `a_replay_sends_the_system_the_turn_saw_not_the_one_built_now` fails.
  R2 cap checked after the call -> `the_cap_is_checked_before_the_call` fails.
  R3 rung table goes up -> `a_rung_down_is_the_nearest_cheaper_model`, the sampler rung test and the attachment test fail.
  R4 shared-place skip removed -> `the_sampler_leaves_a_shared_places_turns_alone` fails.
  R5 `crossing` returns none -> `an_attachment_is_never_sent_to_another_provider` fails.
  R6 `keep_blocks` not called -> 8 tests fail (blocks, rebuild, refusal, replay, compare).
- Full workspace nextest (4,501 tests): 4,464 pass. Failures: theseus-sandbox / theseusd sandbox L1 tests (the known
  root-VM set), and `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`
  (21 s timeout, fails alone too; I did not touch Discord, and did **not** verify it on main: unexplained, not mine as
  far as I can tell). Two earlier failures (`tests_lsp_edits::the_block_adds_no_frame`, `gone_jobs::a_restart_...`)
  passed alone; both ran while the disk was full.
- `scripts/gate.sh` with `THESEUS_GATE_NO_BENCH=1`: door, keel (0 findings), fmt, shape, features, clippy, cockpit,
  test build, reader rule all pass; **suite failed only on the cases above**; deny could not run (offline, no advisory
  db fetched). `python3 scripts/keel-guard.py`: `keel: ok ... 0 findings acked`.
- **FAST**: `theseus-sim bench turn --check`: plain 4 frames (budget 4), tool 8, batch 14, syncs 2, all ok: a plain
  turn's frames are unchanged. `bench lifecycle --runs 10 --check` (debug build, this VM): start p95 29 ms, shutdown 38,
  crash 34, swap 36, seed 1.1, cancel 15 all ok; **`health after the seed` p95 6.2 ms vs 5 ms MISSED** (twice, 6.2 and
  6.6). Health is not on any path I changed (no health fields touched); I did not run it on main, so I cannot say it is
  pre-existing; it may be this slow VM's. Storing a block is one `put_many` at a compile that is already writing a frame,
  and nothing but a stat on later compiles. No timer added to a turn.

## Live check, this VM (done)

Scratch daemon (`/tmp/lc`, `env:` secrets), `theseus-sim fake-model` + `fake-jev`, `[profiles.haiku]`/`opus`.
Ten `theseus ask` turns (Sonnet stand-in); `theseus turn replay <ses> <turn> -P haiku` answered "the request was the
original byte for byte" **after a daemon restart** with `compare.v1 (shadow)`; ten minutes after the start the
sampler logged `replayed=3 judged=3 spent_usd=0.079`, and `theseus route report` printed:

    Your own work, today: $0.26 spent over 1 cells.
      claude-sonnet-5-5 @ default: 10 turns, $0.26; 3 replayed on haiku, 0 matched ...
    Replays today: $0.08 of $1.00 ([route] replay_day_usd).

(The fake model serves the same price whatever the profile, so the cost ratio reads 100% and the fake Jev's verdict is
below the bar; that is the stand-in, not the code.)

## Live check for the maintainer, on the owner's real turns (Sonnet down to Haiku, $1 cap)

On a scratch daemon on a *copy* of the store (never the operator's), with the owner's config copied minus bindings:

    theseusd --config SCRATCH.toml --state-dir COPY --socket /tmp/sc.sock      # [route] replay_day_usd = 1.0 (default)
    theseus --socket /tmp/sc.sock route report --week                          # cells, nothing replayed yet
    S=<a session id>; T=<a turn of it that ran on Sonnet, after this build>    # turns from before this build refuse:
    theseus --socket /tmp/sc.sock turn replay $S $T -P haiku                   #   "system: ... predates it" (expected)
    # for a new turn: "request was the original byte for byte", both replies, compare.v1 (shadow)
    # the nightly sampler runs 10 min after the start (missed night): then
    theseus --socket /tmp/sc.sock route report --week                          # replayed/matched/movable, Replays today <= $1

Expect: `Replays today` never above the cap; no replay of a Discord shared channel's turns; a turn that carries an image
sent to a different provider is refused (`crossing`). Existing turns have no stored blocks: only turns after this build
replay (a limit worth telling the owner).

## Left / uncertain / design choices for the owner

- **No new learning-report lines** for `compare.v1` beyond the standard per-pack report; its holdout baseline (owner
  labels, route corrections) is named but no label is yet written *on compare judgments* (labels attach by
  `judge.label` as for any pack).
- "you checked" counts routing corrections only; judge labels on route judgments are not joined to turns.
- Cell effort comes from `route.decided.effort_ran`; a turn with no such row is the "default" cell.
- The match bar 0.8 reads `same + better` probability; the 90% Wilson floor and the cost-ratio saving are my choices.
- Replays send the full conversation (not just the last message) to the cheaper model: that is the point, and why the
  cross-provider rule is conservative (any attachment blocks it).
- A sampler trigger RPC was not added (opinionated: fewer knobs); a live check waits the 10 minutes.
- Docs to change at review (not edited by me): spec Part III item, `docs/status.md`, `docs/design/killer-features.md` §4.5.

## Keel findings expected

None. No ceiling raised, no test deleted or loosened (count assertions changed value only: `INBOUND_ALONE` 14 -> 15,
judge pack count 18 -> 19, format 33 -> 34), no `#[allow]`/`#[expect]` added. `compiler.rs` shrank (2,547).
