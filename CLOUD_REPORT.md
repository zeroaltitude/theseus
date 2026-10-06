# Cloud report: learned-shadow (theseus-nwa5, theseus-clbx, theseus-gf8j)

Branch `cloud/20261006-learned-shadow`, from d279767f (store format 23). Three steps, one commit each, each after a
gate run. Started 19:43 UTC and finished about 21:00 UTC (date), inside the 4-hour deadline.

| Step | Commit | Issue |
|---|---|---|
| A promotion names the learned version standing in its place | 33ae7147 | theseus-nwa5 |
| The prove's window names the version standing when it opens | 22939914 | theseus-clbx |
| The learning cut's mark is never ahead of what it cuts | cf55f0b8 | theseus-gf8j |

There is no store format bump. The one new stored thing is the key `cut_ms` in the untyped META value
`learning.last_run`. There is no protocol change and no config change, and no dependency was added.

---

## Step 1: theseus-nwa5, the promote names the learned version in its place (33ae7147)

**What I found.** The code matches the brief. `placed` walks `names_of_root` newest first. It returns the first
version whose latest non-declined row is shadow, canary or live (a canary only in its canary arm), and otherwise the
root. A learned version in shadow or live therefore takes the root's place in every session. A learned canary leaves
the root only that canary's control arm, and even there an older placed learned version comes before the root. Nothing
told the owner this at the promote.

**Where main differs from the brief.**
- Main has no `read_ladder` or `placed_read`; those come with judge-turn-cost. On main, `ladder()` loads lazily on
  its first `standing` or `rows_of`, the same way `promote_with`'s own `ladder().write` does.
- So the check reads `names_of_root`, then each version's `rows_of` and `ladder().standing`, as `placed` does. It
  does not call `placed`, and `placed` is unchanged.
- When judge-turn-cost lands, the maintainer may want an explicit `read_ladder` call at the top of
  `ahead_of`. Main has nothing to call yet.

**What I changed.**
- New module `crates/theseus-core/src/rpc/packs_ahead.rs`, holding `Core::ahead_of` and `Core::ahead_words`.
- `promote_with` in `rpc/packs.rs` changed by a few lines: it computes the sentence and appends it to `said`, on both
  the written path and the card path. I did not touch `warm_ladder` or `pack_list`.
- The sentence is a warning only; the promotion is never refused. The forms:
  - **Shadow:** "loop.v101 stands in loop.v1's place in shadow, so loop.v1's canary 0.5 judges in no session until
    loop.v101 moves (`theseus packs rollback loop.v101`)."
  - **Canary:** "loop.v101 stands in loop.v1's place as canary 0.3, so loop.v1's canary 0.5 judges only in
    loop.v101's control arm until loop.v101 moves (…)."
  - **Live:** "… live, so loop.v1 live judges in no session …"
  - **Several versions placed:** the walk follows `placed`. Canaries continue the walk and a shadow or live version
    ends it. All of them are named, with one rollback command each.
- Every move goes through `promote_with`: the owner's `pack_promote`, `promote_automatic` and `promote_learned`.
  - **A learned version moved:** only the newer learned versions ahead of it are named. This goes slightly beyond
    the brief, but it is the same hazard one rung down.
  - **The judge off:** no sentence, since `placed` answers the root then.
- `tests_prove::learned_loop` is now `pub(crate)`, so the ladder test can use it.
- `crates/theseus-core/AGENTS.md` (the ladder paragraph) now has one line about this.

**Security root's card.** It needs the same treatment, and gets it. `gate.rs` (`at_gate`, `notices_live`) and
`notice.rs` all read `placed(SECURITY_CANDIDATE, …)`, so a learned security version in shadow silences the root's
promotion the same way. On the card path the sentence is appended to the answer's `said` with "would judge", because
nothing is written until the card is approved.

The card's own question text, which Discord shows the owner, is unchanged. If the owner wants the warning on the
card too, add `ahead_words(…, true)` to the `question` in `ask_promotion`.

**The owner's other option, not built:** `placed` keeps the root wherever the root acts, and the shadow version
records beside it.
- **The points that read `placed` today:**
  - `judge/mod.rs` `plan_loop_end` (loop.v1)
  - `judge/compile.rs` (continue.v1)
  - `judge/gate.rs` `at_gate` and `notices_live` (security)
  - `judge/notice.rs` (three sites)
  - `judge/categorize.rs` (categorize.v1)
  - `judge/inbound.rs` (route, classify and role roots)
  - `learning/propose.rs` (the parent of a new proposal, with session "")
  - `pack_list`'s `standing`
- **What changes at each point:** each would ask two things: the root, under its own ladder mode and arm, and every
  placed shadow learned version, always in shadow with arm `all`.
- **What the shadow version would then write:**
  - a second `judge.call` row per dispatch, with its own judgment id (the gate's ids are hashed from pack and
    correlation id, so they stay distinct);
  - its own Jev call and spend against the shadow day budget, which roughly doubles judge cost at those points.
- **How the prove would read it:**
  - Today `learning/prove.rs` leaves out a task when any of its stops was judged by a non-loop.v1 version
    (`learned_version`).
  - With side-by-side recording, the prove would take loop.v1's judgments (canary or control, by `pack_arm`) as the
    record, and ignore learned judgments whose mode was shadow rather than leaving the task out. "records by arm"
    would then count the root's arms again.
  - A learned version that acts (canary or live) would still have to leave its tasks out.
- **Where it would land:** `placed`'s callers would need a "who acts" answer plus a "who records beside" list. That
  touches lineage.rs and judge/mod.rs, both in flight with judge-sink and judge-turn-cost.

**How I proved it.** Test `tests_ladder::a_promotion_names_the_learned_version_standing_in_its_place`:
- loop.v101 is written as `learned_loop` writes it.
- loop.v1 is promoted to canary 0.5 in each state, and the test asserts on `said`:

  | loop.v101's state | Expected `said` |
  |---|---|
  | No row | Plain line, no sentence |
  | Declined row only | No sentence |
  | Shadow | Exact shadow sentence |
  | Canary 0.3 | Exact control-arm sentence |
  | Live (loop.v1 → live) | Exact live sentence |
  | Rolled back | Plain line again |
  | loop.v101's own move to shadow | No sentence |

- **Planted revert:** `ahead_words` given an empty list (the check removed). The test FAILED at the shadow assertion
  (tests_ladder.rs:871). Restored, `touch`ed, `git status` checked.

---

## Step 2: theseus-clbx, the window names the version standing when it opens (22939914)

**What I found.** The code matches the brief. `learned_placed` filtered to rows inside `[since, until]`.

**What I changed.**
- New function `learned_before(rows, learned, since)` in `rpc/judge_prove.rs`. For each learned version, newest first,
  it takes the latest non-declined row at or before `since`. If that row is shadow, canary or live, the line gains
  "; loop.v101 stood in loop.v1's place from before it (canary 0.5 since 2026-10-06)", in `mode_words`'s words.
- With no `since`, it adds nothing.
- The "inside" clause is unchanged and comes after the "before" clause when both hold.
- A version whose latest pre-window row was declined is judged by its last non-declined row, as the brief says.

**The changed assertion.** The window test's last step asserted the old line: the window alone, after loop.v101
became a canary before loop.v1's canary opened the window. It now expects the clause, and the commit body says so.

I extended the test:
- A rollback of loop.v101 before a later window: loop.v101 is not named.
- Shadow before the window and live inside it: both clauses.

**How I proved it.**
- Test `tests_prove::the_window_names_a_learned_version_placed_inside_it` passes.
- **Planted revert:** the old filter (rows at or after `since`). The test FAILED at tests_prove.rs:700:
  left `"tasks that ended since loop.v1's move to canary 0.5 on 2026-10-06"`, right `"…; loop.v101 stood in loop.v1's
  place from before it (canary 0.5 since 2026-10-06)"`. Restored, `touch`ed, `git status` checked.

---

## Step 3: theseus-gf8j, the cut's mark is never ahead of what it cuts (cf55f0b8)

**What I found.** The code matches the brief, including the probe: the run at now + 20 min read 0 sessions, closed 1,
and wrote 0 labels. The planted revert below reproduces exactly that: closed 1, sessions 0.

**I built (a), with a different bound from the brief's, and (b) as a clamp.**
- **The bound:** the mark gains `cut_ms` (const `learning::system::CUT_KEY`). Its value is
  `min(run's clock, newest judgment row's time among the scopes the run read)`. `Cut::of(mark, now)` reads `cut_ms`,
  not `at_unix_ms`.
- **What stays the same:** `at_unix_ms` stays the run's own clock, so the tender's `due` and the prove's
  `settled_ms` are unchanged.
- **Why not "plus its longest window":**
  - With the newest judgment's own window, it cannot pass `a_second_run_reads_only_what_can_still_change`'s third
    step. That step and the probe have the same shape: the mark is from a run long after the newest judgment (a day
    in the test, 5 h in the probe), and the next run comes after that. Any bound built only from "newest judgment's
    time" and "the run's clock" gives both the same cut. The probe needs the newest judgment to stay open; the third
    step needs it closed.
  - With the pack's longest window (24 h), the probe still closes its judgment wrongly.
  - In general, "newest + window" can still run ahead of real time by up to that window minus the 1 h margin. That
    is up to 23 h for classify, so a fast clock could still close a classify judgment early.
  - The newest judgment row's time is real-time evidence: the daemon wrote that row before the run read it. So
    `min(clock, newest)` never cuts ahead of real time.
- **The cost:** judgments within their window plus margin of the newest judgment stay open, and are re-read, until a
  newer judgment is written. On a live daemon that is minutes. In a quiet stretch it means at most the last day's few
  judgments.
- **(b):** a cut later than the next run's own `now` reads as `now`. This is a clamp in `Cut::of` rather than "no
  cut". It is as safe, given the run's `now`, and cheaper.
- **Old marks:** a mark without `cut_ms` (written before this change) reads as **no cut, once**. That run walks
  everything, as a first run does, and writes the key. I chose this over "read as today" because an old mark may be
  exactly the fast one this fixes. It costs one full walk after the upgrade.

**Test change, against "stays green".** `a_second_run_reads_only_what_can_still_change` stays green, but only with
one added row:
- Before its second run it now writes a later judgment (`jdg_witness`, at first_at + 1 day − 1 min). That judgment
  has no session in its context, so it reads no session.
- Its time is what the second run's mark cuts at, so the open judgment closes for the third run.
- Every count the test asserts is unchanged: read1 201 sessions; read2 1 session and 200 closed; one label; read3
  (0, 0).
- For the reason above, no bound of type (a) can keep that third step unchanged and also fix the probe. The owner
  should know this.

**New tests, in tests_learning.rs.**
- **`a_run_whose_clock_read_ahead_closes_no_open_window`:** the probe exactly as the brief describes it, run twice,
  with the next run at now + 20 min and at now + 1 day. Each time the next run reads the session (closed 0,
  sessions 1) and writes the continuation label. The test also asserts the mark: `at_unix_ms` = now + 5 h,
  `cut_ms` = now.
- **`a_mark_without_its_cut_walks_everything_once`:** an old-layout mark (no `cut_ms`) whose `at_unix_ms` would have
  closed a 3-day-old judgment.
  - The first run reads it: no cut, closed 0, sessions 1.
  - The next run cuts again: closed 1, sessions 0.

`crates/theseus-core/AGENTS.md` (the learning ledger paragraph) and `learning/system.rs`'s module doc now describe
`cut_ms`.

**Planted revert:** the mark from the run's clock alone (`write_run(…, now_ms)`).
- `a_run_whose_clock_read_ahead…` FAILED, first at its `cut_ms` assertion.
- With that assertion skipped as a second plant, it FAILED on the behaviour itself: `(closed, sessions)` was (1, 0),
  expected (0, 1). That is the probe's bug.
- `a_mark_without_its_cut…` FAILED: (2, 0), expected (1, 0).
- `a_second_run…` passed, as expected.
- Restored, `touch`ed, `git status` checked.

---

## Proof, offline

**New and changed tests, under load.** The five tests were each run 3 times under load (AGENTS.md's recipe: four busy
loops at nice 0, nextest at nice 19, the loops killed by their own pids): 15/15 passed. The tests:
- `tests_ladder::a_promotion_names_…`
- `tests_prove::the_window_names_…`
- `tests_learning::a_run_whose_clock_read_ahead_…`
- `tests_learning::a_mark_without_its_cut_…`
- `tests_learning::a_second_run_reads_only_…`

Under load they took 2 to 13 s.

The first attempt at the loaded runs was refused by the environment (an inline `sh -c` loop). I ran the loops from a
script file instead.

**Suites.** The core suite whole, at the third gate: theseus-core 1,390 passed and 1 failed (the flake below). By
suite:

| Suite | Passed | Failed |
|---|---|---|
| tests_judge | 17 | 0 |
| tests_ladder | 14 | 0 |
| tests_learning | 11 | 0 |
| tests_prove | 10 | 0 |
| tests_notices | 9 | 0 |
| tests_learn_loop | 9 | 0 |
| judge::ladder | 6 | 0 |

## The live check (the maintainer's)

Use a scratch daemon on a copy of a stopped state dir, with Discord and the web off and the judge on. Seed loop.v101
in shadow, as the learning loop places one.
1. `theseus packs promote loop.v1 --canary 0.5` prints "loop.v1 is canary 0.5, forced short of the bar (…). loop.v101
   stands in loop.v1's place in shadow, so loop.v1's canary 0.5 judges in no session until loop.v101 moves
   (`theseus packs rollback loop.v101`)."
2. Run four tasks, then `theseus judge prove`. The window line reads "tasks that ended since loop.v1's move to canary
   0.5 on <day>; loop.v101 stood in loop.v1's place from before it (shadow since <day>)". As before, the tasks are
   left out as `learned_version`, since nothing changed which version judges.
3. `theseus packs rollback loop.v101`, then the promote from step 1 again: the line ends at "…forced short of the bar
   (…)." with no sentence after it. Four more tasks, then the prove: records by arm canary/control are no longer 0.
4. Optional:
   - With loop.v101 at `--canary 0.3` instead of shadow, the promote's sentence says "only in loop.v101's control arm".
   - For a security card, seed a learned security.v1 version in shadow; `theseus packs promote security.v1 --live`
     answers the card line followed by "… would judge in no session …".

gf8j has no live check; its test is the proof. After the upgrade, the first learning run's log line
`learning: the rules read what can still change` shows `first=false` but `judgments_closed=0` and every session read
(the old mark read as no cut, once). Runs after it close again.

## Left open, and choices for the owner

- **The gf8j bound** is `min(clock, newest judgment)`, not the brief's "plus its longest window". The reasons are in
  step 3. `a_second_run…` needed a witness judgment for its third step; no type-(a) bound can avoid that.
- **The promote's warning reaches the card answer's `said`, not the card's question.** Add it to the question if the
  owner wants it on Discord's card too.
- **A learned version's own promotion** also names newer placed learned versions ahead of it. This is a small
  widening beyond "a root"; easy to restrict to roots if unwanted.
- **Once judge-turn-cost lands**, `ahead_of` may want its explicit ladder read (`read_ladder`).
- **Docs for the maintainer:**
  - Part III's items for nwa5, clbx and gf8j.
  - `docs/status.md`'s recently landed list.
  - The spec's learning section, where it describes the cut's mark. It should now say the next run cuts at `cut_ms`,
    the run's clock but never past the newest judgment it read, and that a mark without it walks everything once.

## The gate

I ran `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit, three times, about 5 to 7
minutes each. Its suite phase failed each time only on cases the brief already names:
- **All three runs:** the 33 L1 tests (theseus-sandbox's contract tests and theseusd's sandbox tests: a root daemon's
  job with no job cgroup, theseus-pv6i).
- **The third run only:** `learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy`
  (theseus-1g8j‡, "the pool thread took the idle thread's policy", left 0, right 5). It passed alone 3 times out
  of 3.

The phases after the suite all passed each time, so I counted each commit green:
- protocol types unchanged;
- `theseus-sim bench turn --check --runs 5 --burst 0`: frames_plain 5 against a budget of 5, frames_tool 9 against
  9;
- deny: advisories, bans, licences and sources ok (the database fetched at setup).

The earlier phases also passed: fmt, shape, features, clippy `-D warnings`, cockpit, test build, and the reader rule.
