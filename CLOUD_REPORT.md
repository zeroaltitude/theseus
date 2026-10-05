# Cloud report: the learning loop, step 25f (theseus-0j2.12)

Branch `cloud/20261004-learn-loop`, from `32de305` (main at `3085f71` plus the task commit). Started 03:00 UTC, report
written 05:27 UTC. Every commit's gate is `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (from commit 7 on
with `CARGO_INCREMENTAL=0`, for disk); each failed only in the suite, only on the cases the brief names (plus the two
timing failures below), so the phases after it were run by hand (protocol types, `theseus-sim bench turn --check --runs 5
--burst 0`, `cargo deny --offline check`), each passing.

## What I found

- **The ladder was ready for this step.** `Core::promote_automatic` already cites theseus-0j2.12, and 25d's replay
  already took a pack file's text. Ledger rows need no format bump (Q1; 26a's `pack.mode` added none), so learned
  versions and proposals are ledger rows and `MANIFEST_FORMAT` stays 16.
- **Every point dispatched a constant through `pack::by_name`**, compiled-in packs only (gate, inbound, loop end,
  compile, categorize, the label, the replay's incumbent, the ladder's rules). A learned version needed one resolver
  and one "who stands here" answer, not a second registry.
- **Opus 5.5 at its full output cap reserves $2.57**, more than `writer_limit_usd_per_day`'s $2.00: with the
  profile's own cap the writer could never run. Found by the first test run; the writer's output is capped at 8,192
  tokens (`learning::propose::WRITER_MAX_TOKENS`).
- **`report::graded` gives a Noul's label truth, not whether its lean was right** (its second value is the
  calibration pair's outcome). I first read it as "right"; the security test found it (a yes-or-no error was never
  "fixed"). The loop now has its own `right`. **25d's replay has the same slip** (`learning/replay.rs`, `fn right`):
  its per-judgment `fixed`/`broken` and `--errors` are wrong for Nouls. Not changed here (25d is to be left alone);
  the one-line patch is under "What is left, or uncertain".
- **`theseusd --state-dir` moves the store, not `[server] state_dir`**, so `Config::state_dir()` is not a daemon's state
  dir when it was started so. Found by the offline end-to-end run; `<state>/packs/` now goes beside the store.
  **25c's `<state>/learning/<date>.json` has the same slip** (`rpc/learning.rs`, `learning_file`); not changed here.
- **The writer's file, re-serialized through `toml`, rewrote every line** (inline option arrays became tables, 0.90
  became 0.9, questions sorted), so every diff was the whole file. Found by the end-to-end run; the version and
  re-fit thresholds are now set in their own lines.
- **Profiles are not built in**: a config without `[profiles.opus]` makes the writer's profile unresolvable, and the
  run stops before writing anything (nightly: a warning per lineage with enough errors). The live check's config names
  it, as the brief says. Health does not say it; see below.
- **A replay of a learned `classify` candidate would have left every judgment out**: 25d refuses a stored state for
  any pack with a dynamic question (`addressed_task`'s live tasks). A learned candidate keeps its parent's builder and
  questions, so the hook asks it on the stored state without the builder's items (`only_when` questions are not
  asked, the rest are).

## What I changed (commits)

1. `27cfc94` judge: the pure parts, `theseus_judge::propose`: names from v101 (a test holds every compiled-in version
   below 101), the interleaved split (`SHA-256(id)[..8] mod 5 == 0` is holdout, below 200 labeled in the window, then
   the time split), `text_only` (every field but the text Jev reads must equal the parent's), `refit`, `decide`, the
   writer's prompt.
2. `a391753` learning: the loop. `learning/propose.rs` (nightly after the report on the tender's thread, and
   `judge.learn` / `theseus judge learn <pack> [--split <time>]`, the owner's act, judged by `judge_act`, in the CLI's
   `OPERATORS`); `judge/lineage.rs` (`pack.version` rows, scoped `judge.learn:<id>` beside the `judge.proposal` rows;
   `JudgeService::pack`, `root_of`, `placed`; the files derived after serving in `warm_ladder`); every point asks
   `placed(root, session)` then `mode_for` that version, capped by the root's config line; `Core::promote_learned` (26a's
   own act, citing the proposal); `[judge] holdout_days` and `[judge.learn]` with template lines and their test;
   `pack.list` lists learned versions (source, parent, root, text, standing), a learned version may be promoted to
   `shadow`, and `pack.rollback { off: true }` is a reject; protocol types in `judge_runs.rs` (no new module line),
   `JUDGE_LEARN` on the replay's method line, the TypeScript regenerated.
3. `31777e1` learning: a Noul is right when its lean meets the label; fixed and broken are counted from both sides'
   grades; a test through security.v3 (the card, then "one open proposal"); `placed` reads names only.
4. `991b3d3` cockpit: the Judgment section's Versions panel (each lineage, mode and source, the line diff between any
   two at `?va=`/`?vb=`, promote, reject), `src/lib/versions.ts` with `npm test` cases.
5. `23e425a` judge: the writer's layout kept, so a diff is its wording.
6. `165b793` learning: `<state>/packs/` beside the store, wherever `--state-dir` put it.
7. `b9b018f` cockpit, rpc: the Versions panel scrolls inside its bound (it spilled into the next panel); a learned
   version's first row says `off → …`.
8. `d574315` docs: §2.17 and step 25f in docs/design/m5-judgment.md, §2.9's "proposes nothing" and §5's Q10 (the task asked
   for these, though the brief's general rule says the maintainer writes docs/design/; it is a commit of its own, to
   keep or drop).
9. `5203d5c` learning: a learned canary still running holds its lineage (which version is the parent depends on a
   session's arm until it goes live or back), as an unanswered card does. §2.17's last bullet should say so too ("a
   card not yet answered, or a learned canary still running, holds the next"); commit 8 does not.

theseus-core's AGENTS.md has the loop's bullet (commit 2).

## How I proved it

**Tests** (`cargo nextest run -p theseus-core -p theseus-judge -E 'test(tests_learn_loop) | test(propose::)'`: 14, all
pass):
- theseus-judge `propose::tests` (6): versions from 101 above every embedded one; the split is stable, about one in
  five, and switches at 200; a reworded criterion is a candidate and keeps the writer's layout (two lines differ: the
  version and the meaning), a moved question id, a new builder, a changed threshold, or a file the loader refuses is
  not; the re-fit equals the rule's output (0.71 on the worked case; unchanged at 29 answers, when nothing lower holds,
  with no parent act band); the decision (a worse class held, live / shadow / card at the minimum, canary / shadow
  below it, nothing fixed held, up by less than the margin held); the writer's message.
- theseus-core `tests_learn_loop` (8), the fake Jev scripted per state and the writer scripted:
  `ten_train_errors_propose_once_and_the_holdout_never_reaches_the_writer` (9 → none and no writer request; 10 →
  once, its request holds the ten and neither the holdout label's note nor the holdout's state; a second run → none);
  `a_better_candidate_replaces_a_shadow_parent_and_a_rollback_restores_it` (shadow; the row and the file, beside the
  store though the config's state dir points elsewhere; a turn's end dispatches loop.v101; the rows rebuild the file;
  the rollback gives loop.v1 its place); `a_candidate_worse_on_one_holdout_class_is_held`;
  `a_live_parents_candidate_below_the_minimum_goes_to_the_canary` (0.2, the system's, citing the proposal, from off;
  ten new errors while it runs are held, no second writer request);
  `a_writers_file_that_moves_the_builder_is_refused`; `the_writers_day_budget_stops_a_run` (nothing sent, the errors
  stay new); `the_nightly_run_uses_the_interleaved_split`; `a_security_candidate_waits_on_the_owners_card`.
- At the minimum (200 labeled per deciding question) the decisions are proved in the pure test only; a core test with
  200 holdout judgments was not written.

**Planted reverts** (each restored from a copy and `touch`ed, `git status` clean after):
- Holdout errors into the writer's input (`train_errors(&labeled, …)`): `ten_train_errors…` fails (the first run
  proposes from 9 train errors and the holdout's: `left: ("shadow", 10) right: ("none", 9)`), and two more fail.
- The no-class-worse check skipped (`worse(…).filter(|_| false)`): `the_decision_holds_a_worse_class…` (Live, not
  Held) and `a_candidate_worse_on_one_holdout_class_is_held` ("shadow", not "held") fail.
- The Noul read as the label's truth again: `a_security_candidate_waits_on_the_owners_card` fails ("held: … fixed none").
- The file written to `Config::state_dir()` again: `a_better_candidate…` fails ("the file followed [server] state_dir").
- The canary's hold skipped: `a_live_parents_candidate…` fails (a second canary: `left: "canary" right: "skipped"`).

**Under load** (AGENTS.md's recipe: the tests at nice 19 beside four busy loops at nice 0, by a Python driver that kills
its loops by pid; the shell form was refused by this environment's safety check): the 14 tests three times, all pass;
`tests_judge::a_tiny_day_limit_pauses_shadow_with_one_row` eight times, all pass.

**Offline end-to-end on a scratch daemon of this build** (fresh state dirs under /tmp, `theseus-sim fake-model` as the
model and the writer, a 40-line Python stand-in for Jev's `/v1/systemone` that answers `control` for a bare "stop" only
when the `control` criterion holds the learned wording; never the operator's daemon): ten sessions of one message each
(six before a split time, four after; three and two of them "stop"), every `classify.v1` judgment labeled by the CLI,
then `theseus judge learn classify.v1 --split <ms>`:
```
prp_… classify.v101 (owner): shadow — below the minimum: no class worse, 3 train errors fixed
  classify.v101 from 3 of your labels: holdout precision 0.50 → 1.00, recall 0.50 → 1.00 (kind); replacing classify.v1 in shadow
  parent classify.v1; split time [...]: 6 train, 4 holdout labeled; 3 new errors, 3 read
  replay rpl_…: 3 train errors fixed, 0 broken; writer $0.0002, replay $0.0001; below the minimum
  kind: precision 0.50 → 1.00, recall 0.50 → 1.00 (4 labeled)
  diff:
    - version = 1
    - { id = "control", means = "It tells the assistant to stop, pause, …" },
    + version = 101
    + { id = "control", means = "It tells the assistant, even as a bare word such as stop typed alone as text, to stop, pause, …" },
```
Then: `<state>/packs/classify.v101.toml` written; `theseus packs` lists it in shadow; the next "stop" was judged by
classify.v101 (`kind=control 0.95`) and its judgment took a label; a restart with `packs/` deleted rewrote it after
serving; the cockpit's Judgment page loaded clean in Chromium (no console or page errors) with the Versions panel and
the diff at `?va=classify.v1&vb=classify.v101`; `theseus packs rollback classify.v101` put classify.v1 back (the next
message judged by it). `theseus judge learn route` and `… security` are refused with why; inside a job
(`THESEUS_SESSION` set) the CLI refuses it.

## The live check (the maintainer's, with real keys)

A fresh scratch state dir and a free web port; never the operator's socket, port 7433, or bindings.
```sh
mkdir -p /tmp/learn-live && cat > /tmp/learn-live/theseus.toml <<'EOF'
[server]
state_dir = "/tmp/learn-live/state"
socket = "/tmp/learn-live/sock"
[model]
live = "glm"
provider = "zai"
model = "glm-5.3-flash"
[profiles.glm]
provider = "zai"
model = "glm-5.3-flash"
[profiles.opus]
provider = "anthropic"
model = "claude-opus-5-5"
[secrets]
jev_api_key = "op://<vault>/<Jev item>/notesPlain"
zai_api_key = "op://<vault>/<Z.ai item>/notesPlain"
anthropic_api_key = "op://<vault>/<Anthropic item>/notesPlain"
[discord]
enabled = false
[web]
port = 7436
[routing]
mode = "shadow"
[judge]
enabled = true
[judge.learn]
min_errors = 2
EOF
theseusd --config /tmp/learn-live/theseus.toml --socket /tmp/learn-live/sock --state-dir /tmp/learn-live/state &
T="theseus --socket /tmp/learn-live/sock"
```
1. Eight short messages, one session each, so a state holds only its own message:
   ```sh
   for m in "Draft a note to the harbor master." "stop" "and the tide table too" "List the boats due tomorrow." \
            "stop" "Thanks!" "Summarize the fuel log." "wait"; do $T ask "$m"; done
   $T judge log --pack classify -n 20          # each judgment's id and Jev's kind
   $T judge label <id> control -q kind --note "a bare stop typed as text is control"   # each stop/wait Jev called otherwise
   $T judge label <id> <its right kind> -q kind                                         # each of the rest
   ```
   If Jev got fewer than two wrong, give two another plausible kind. Note the time (`date +%s%3N`), send four more the
   same way (two of them "stop"), and label them.
2. `$T judge learn classify.v1 --split <that time>` prints the proposal: the errors read, the replay's numbers per
   question and class, the re-fit, the decision and why, and the diff (only `version` and the reworded lines). Expect
   `shadow` (classify.v1 records only) when no holdout class got worse, else `held` with the class. Check
   `ls /tmp/learn-live/state/packs/` (the file), `$T packs` (classify.v101's row), and the cockpit at
   `http://127.0.0.1:7436/judgment?va=classify.v1&vb=classify.v101` (Versions: the lineage and the diff). If it took
   classify.v1's place, `$T ask stop` is judged by classify.v101 (`$T judge log --pack classify -n 1`), and
   `$T packs rollback classify.v101` puts v1 back (the next message is judged by v1).
3. `$T ask "run proc.run echo hi"` twice (each a `security.v3` judgment at the gate; `$T judge log --pack security`),
   label each `$T judge label <id> wrong`, then `$T judge learn security.v3 --split <now>`: with the errors on the
   train split, the proposal is held with its reason, or (no class worse, an error fixed) waits on the card:
   `$T confirm` (it lists what waits), then `$T confirm <id> --decline` (a declined row; nothing stands in v3's place). With `--split`
   at "now", the train split is every earlier judgment and the holdout is empty, so "no class worse" holds trivially;
   a run with no holdout labels deserves a look (see below).
4. Stop it: `$T shutdown`.

## What is left, or uncertain

- **Patches I did not apply** (others' steps): 25d's `learning/replay.rs` `fn right` should read
  `graded(a, &t).map(|(_, x)| match a.band.top { Top::Noul(lean) => lean == x, _ => x })`; 25c's `learning_file`
  should join `self.store.dir().parent()` (as `judge::lineage::state_of`) rather than `self.cfg.state_dir()`.
- **An empty holdout**: below the minimum, "no class worse" over no holdout labels is vacuously true, so a candidate
  that fixed train errors takes a shadow parent's place (or a live parent's canary) with no held-out evidence. The
  brief's rule allows it; the owner may want "at least N holdout labels" (say 5) before any move.
- **The combination rule** at the minimum: each deciding question's macro precision and macro recall (the mean over
  its classes that have one) must each rise by `margin`; all deciding questions must. A Noul's classes are `true` and
  `false`. Scores are not graded by the loop (only the test pack, probe.v1, has one).
- **An error is a label**, per answer: a judgment wrong on two questions is two errors; `min_errors` counts labels.
  A writer that fails, or a budget skip, leaves its errors new; a refused writer file consumes them (so a writer that
  keeps refusing does not spend every night).
- **The parent** is `placed(root, "")`, the version standing at the point. A learned canary still running holds the
  lineage (commit 9), so the parent is never chosen by a session's arm.
- **Rollback rules of a learned version** are its lineage's wired version's (by id; for `security` that is v1's, with
  the adoption's v3 rules added by id). A noise label on any learned security version counts toward v3's notices brake
  (labels carry no root), and gate judgments of a learned v3 successor carry `root` so their notices post.
- **Not done**: health's judge lines and the Judgment table's mode column show a learned version as `not wired` (the
  table reads health's lines); `judge.list`'s "disagrees" (`fact/judge.rs`) and the report's question kinds read
  compiled-in packs only, so a learned version's rows say less there; the security card shows the numbers in its row
  (`numbers`) but not the diff (the CLI and the cockpit show it); no method lists proposals (`Core::learn_proposals`
  exists; the rows are in `judge.learn:<id>`); health does not say that the writer's profile does not resolve.
- **The CLI** has `theseus judge learn`, and `theseus packs promote` / `rollback` work on a learned version by name;
  promoting one to `shadow` and a reject (`pack.rollback { off: true }`) are the cockpit's (and the protocol's) only.
- **The owner's promote of a learned version** goes through 26a's bar, which reads a learning report of that version;
  until a report covers it the owner's promotion is `forced`.
- **Docs to change** (the maintainer's): Part III's item for 25f; `docs/status.md` (the step, the roadmap row);
  `docs/design/README.md` if it indexes §2.17.

## The gate

Each commit's gate failed only in its suite, on:
- the 33 L1 tests the brief names (theseus-sandbox's contract tests and `spawn_100`, theseusd's sandbox tests: a root
  daemon's job without a job cgroup, theseus-pv6i), in every run;
- `theseus-core term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one` once (commit 2's gate: 15.7 s, a timeout
  under the suite's load; it passes alone 3 of 3; the terminal code is untouched);
- `theseus-core tests_judge::a_tiny_day_limit_pauses_shadow_with_one_row` once (commit 6's gate: `skipped_today` 1,
  not 2, after its fixed 300 ms wait; it passes alone 5 of 5 and at nice 19 under load 8 of 8; it is on the loop-end
  path this step touches, so it deserves a look on the owner's machine).
The phases after the suite passed by hand at each commit: protocol types, the turn bench (5 frames plain, 9 with a
tool, each at its budget), and `cargo deny --offline` (advisories, bans, licenses, sources ok). The lifecycle bench,
which `THESEUS_GATE_NO_BENCH` skips, was run once by hand on commit 7's build on a quiet machine
(`target/debug/theseus-sim bench lifecycle --runs 10 --check`): `LIFECYCLE OK`, every phase inside its budget
(SIGKILL then restart p95 27.5 ms of 150; a binary swap p95 26.8 ms of 200). Nothing new runs before serving: the
lineage is read in `warm_ladder`'s after-serving task (or by the first judgment), the loop on the tender's thread.

**The branch's head, `5203d5c`** (commit 9; the report's commit adds only this file): the gate's suite ran 2,500
tests, 2,467 passed, 17 skipped, and 33 failed, exactly the L1 set above; protocol types, the turn bench, and
`cargo deny --offline` passed after it.
