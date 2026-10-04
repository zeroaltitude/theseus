# Cloud report: step 37b, tasks set one-shot wakes (theseus-7kg)

Branch `cloud/20261004-task-wakes`, from `main` at `d9b0931`. Started 02:30 UTC, report written 04:00 UTC
(2026-10-04), within the 3-hour deadline.

| Commit | What |
|---|---|
| `e837edc` | wake: a task sets one-shot wakes and parks on them (theseus-7kg) |
| `ce9c585` | sim: a scripted stand-in model for live checks, `fake-model --rules` (theseus-7kg), and the AGENTS.md lines |
| (this one) | cloud report (not for main) |

No new dependencies. `Cargo.lock` and the package-lock files are unchanged. No stored shape changed, so
`MANIFEST_FORMAT` is not bumped: `WakeInfo` is wire-only, and `Execution` and `PendingWake` are untouched.

## What I found

- `wake::set` refused every wake in a task (`TASK_REFUSAL`). At the end of a task's turn, `TurnRunner`'s finish
  turned `Wait { Input }` into `Complete { "reported" }` unless results were unread (`turn.rs`, "A task that has
  nothing left to wait on is done").
- The kernel already did most of the rest:
  - a task's end (`end_turn`'s terminal branch) and its cancel (`Kernel::cancel`) both call `drop_wakes` in their
    own frame;
  - `wakes::free` already treats `Waiting { Input }` as free, so the due scan fires a wake for a parked task;
  - the driver already takes a task's continuation turns.
  So parking needed only the turn's end decision.
- **A gap the naive change would open.** An operator can cancel a parked task's last wake (`theseus cancel <wake>`,
  `/cancel`). The task would then wait on input forever. Nobody gives a task input, so it would never report, and
  `wake_parent` would never fire. The same happens if that cancel lands while the task's turn is ending. I closed
  this in the kernel, in the same frame as the cancel (see below).
- Pre-existing, not mine:
  - a `/stop` on a task leaves it waiting on input with no wake (the stop branch of `end_turn`);
  - the `max_loops` / late-result path of a task wrote `Wait { Input }`, and the core woke it in a second frame, so
    a crash between the two left the task stuck. The new kernel rule now queues that task in the end's own frame.

## What I changed (`e837edc`)

1. **`wake.at` in a task** (`crates/theseus-core/src/wake.rs`):
   - It now refuses only a repeating wake (`every`). The refusal reads: "Refused: this session is a task, and a task
     must end, so it cannot set a repeating wake. Set a one-shot wake instead (`after` or `at`, without `every`):
     you park on it, its turn continues this task, and you report when you have nothing left to wait on. …"
   - The tool's description gains one sentence on what a wake does in a task.
2. **Parking** (`turn.rs`, 3 lines; `task::parks_on_wake`). A task's turn that would wait on input, with a wake
   pending, ends `Wait { Input }` instead of `Complete`. The wake's turn continues the task. A turn that would wait
   with no wake left completes it, and it reports once on DD7's path. `wake_parent` fires there, as before. An
   unreadable execution ends the task, as before 37b.
3. **The kernel's rule: a task never waits on input with nothing to wake it** (`wakes::task_unparked`).
   - `cancel_wake` queues a task left waiting with no wake, in the cancel's frame (`execution.queued`,
     `why: "wake_cancelled"`).
   - `end_turn` does the same when that cancel landed as the turn ended (`why: "task_unparked"`).
   - In both cases the next turn finds nothing new (`nothing_new`), the task ends, and it reports with its last
     message.
   - A conversation is unaffected.
4. **The cap, the carve, and the cancel.** The cap of 5 applies per execution, as for any session. The task's
   turns spend from its carve, and the parent's spend counts them (tested). A cancel of the task drops its wakes in
   the cancel's frame (the existing `drop_wakes`), so a dropped wake never fires.
5. **Surfaces.**
   - `WakeInfo.task`, the task's short id, is set when the wake's execution has a parent (protocol `lib.rs`, +4
     lines; TypeScript regenerated in `web/src/protocol.gen/WakeInfo.ts`).
   - `theseus wakes` prints `task a1b2c3 (waiting)` where it printed `session ses_… (waiting)`.
   - Discord's `/wakes` adds `· task \`a1b2c3\``.
   - The cockpit's Wakes panel (`cockpit/src/views/Actions.tsx`, one line) shows `task a1b2c3` in place of the
     session's title.
   - `task.list` says a parked task waits on `a wake`, not `input`.
6. **Tests.**
   - `crates/theseus-core/src/tests_task_wakes.rs` (new, 7 tests).
   - The kernel's `tests_tasks::a_task_left_waiting_with_no_wake_is_queued_in_that_frame`.
   - The daemon's `tests/tasks.rs::a_kill_while_a_task_is_parked_on_its_wake_then_a_restart_reports_once`.
   - New cases in the CLI's and Discord's renderer tests.

`ce9c585` adds `theseus-sim fake-model --rules <file>` (`src/fake_model.rs`, with a unit test), which the live check
below needs: none of the simulator's stand-in models could script `task_create` or `wake_at`. The same commit adds
the AGENTS.md lines (kernel: the never-waits rule; theseus-sim: the subcommand).

Other changes' areas I touched, each kept small:
- the kernel (`end_turn`'s wait branch, about 8 lines; `wakes.rs`'s `cancel_wake`);
- `turn.rs` (3 lines);
- protocol `lib.rs` (one field);
- the cockpit (one line);
- theseusd's `tests/tasks.rs`: a new test, and I renamed its rig's `_model` field to `model` so the test can read
  the stand-in's requests.

## How I proved it

All runs below are on this VM: 4 cores, running as root, UTC.

### The new tests: all pass

| Test | What it proves |
|---|---|
| `a_task_sets_a_wake_parks_wakes_and_then_reports_once` | After its first turn the task is parked: `Waiting { Input }`, one wake, no report. `wake.list` names it (`task`), target the parent's place, also in the place's `/wakes` listing. ≥ 1.9 s later the wake's turn continues it; it completes after 2 turns with `reported`, one report post, the parent reading the wake turn's words. `wake_parent` fires once (`task.report_wake` ×1, parent at 2 turns, still 2 a second later); `wake.fired` ×1. The wake turn's spend shows in the parent's. |
| `a_cancelled_tasks_wakes_never_fire_and_the_cancel_frees_their_slots` | The task asks for six wakes: 5 set, the sixth refused with the cap's message. The cancel drops all 5 in its frame (5 `wake.cancelled` rows, `why: the execution was cancelled`). `wake.list` is empty. Nothing fires, the task takes no further turn, one report (`cancelled`), no `task.report_wake`. |
| `a_cancelled_tasks_wake_never_fires_past_its_time` | A 2 s wake, the task cancelled, then 3 s of waiting: no `wake.fired`, no wake node. |
| `a_repeating_wake_in_a_task_is_refused_and_a_one_shot_one_is_not` | In one turn: the series is refused with the readable text; the one-shot wake is set (one `wake.set`, no `repeat`); the task parks. |
| `a_task_with_no_wake_reports_at_once` | One turn, one report, no `wake.set`: as before 37b. |
| `a_restart_while_a_task_is_parked_fires_its_wake_once_and_reports_once` | The core is stopped while the task is parked, and a new core is built on the same store: the wake fires once, the task reports once, `wake_parent` fires once. |
| `a_cancel_of_a_parked_tasks_last_wake_ends_it_and_it_reports_once` | The gap above: the operator cancels the only wake; the task is queued (`wake_cancelled`), ends, reports once with its first turn's words, and `wake_parent` fires once. |
| Kernel `a_task_left_waiting_with_no_wake_is_queued_in_that_frame` | The cancel frame queues the task. `end_turn` queues a task left with no wake (`task_unparked`). A conversation's cancel leaves it waiting. |
| Daemon `a_kill_while_a_task_is_parked_on_its_wake_then_a_restart_reports_once` | With the real `theseusd`, the fake Discord, and the stand-in model: `task.list` says `waiting_on: "a wake"`, and `wake.list` names the task. SIGKILL, then restart: the task completes, one report on the fake Discord (`📋 **Task \`…\` finished**`, `-# 2 turns`), no wake left, one model request for the wake's turn. |
| `fake_model::tests::a_ruled_stand_in_asks_for_the_first_matching_rules_calls` | The rules match in order, multiple calls stream, and unknown keys are refused. |

Existing tests are unchanged and pass. That includes all of `tests_tasks` and `tests_wakes` (core and kernel),
`tests_output`'s golden (under a negative-offset TZ; see the gate), and the frame budget.

### Planted reverts (each restored with `touch`, and `git status` checked clean after)

1. **A parked task ends anyway**: `parks_on_wake` returns `false`.
   - 5 of the 7 core tests fail ("no the task parked on its wake in 10 s"). Only `…_with_no_wake_reports_at_once`
     passes, as it should.
   - The daemon kill test also fails: "no the task parked on its wake in 40 s".
2. **A cancelled task's wakes survive**: `Kernel::cancel` no longer calls `drop_wakes`.
   - `a_cancelled_tasks_wakes_never_fire_and_the_cancel_frees_their_slots` fails at `assert!(e.wakes.is_empty())`.
   - The kernel's own `tests_wakes::a_cancel_clears_the_wake_and_nothing_fires` also fails.
   - `…_never_fires_past_its_time` still passes under this revert. That is correct, not a gap: a cancelled
     execution is never `free`, so its leftover wake cannot fire. It just stays on the record.
3. **The operator's cancel leaves the parked task waiting**: `task_unparked` is skipped in `cancel_wake`.
   - The kernel test fails (`left: (Waiting, false)`, `right: (Queued, true)`).
   - The core test fails ("no the task's report in 10 s").

### Under load (`nice -n 19`, beside four nice-0 busy loops; pids killed by number)

- **The set.** 5 rounds of `tests_task_wakes` (7), the daemon kill test, and core `tests_wakes::` and
  `tests_tasks::`: 29 tests a round.
- **Round 1.** 29 of 29 pass.
- **Rounds 2–5.** 28 of 29 each. All 37b tests pass every round. The failure each time is
  `tests_tasks::a_report_that_starts_a_turn_at_the_parents_limit_asks` ("no the parent waits on its budget in 20 s").
- **That failure is pre-existing.** I built `main` (`d9b0931`) in a separate worktree and target dir and ran that
  test under the same load twice: it failed both times, at the same line. Its stand-in answer is 145,000 words, and
  in a debug build at nice 19 that outlasts the test's 20 s wait. It passes unloaded, here and in both gates.
- **Kernel-sim.** `theseus-sim kernel-sim --seeds 300`: all invariants held across 300 seeds (1615 crashes, 4655
  wakes set, 481 cancelled).
- **Kernel-sim gap.** It has no invariant for the new rule. A blanket one ("no task waits on input with no wake")
  would be wrong, because a `/stop` legitimately leaves a task so. Its random operations already cancel wakes and
  end tasks' turns, and 300 seeds raised nothing.

### A live check I ran here (the same script as below)

| Time | Seen |
|---|---|
| +3 s | `theseus tasks`: `56ec13 ○ ready · wake 03:55 … 1 turn`. `theseus wakes`: `58f7a8 2026-10-04 03:55:25 +00:00 (in 56s) once task 56ec13 (waiting) Look at the build once more, then report.` |
| +68 s | The task: `· complete … 2 turns … reported`. `theseus wakes`: `no pending wakes`. One report node in the parent, carrying `Checked again: the build is green. Done.`. One `wake.set`, one `wake.fired` (`late_ms` 262), and one `task.report_wake` row. The parent's report turn was answered. |

## The live check for the maintainer

This uses a scratch daemon on a fresh state dir, the simulator's new scripted stand-in model at 127.0.0.1:9448, and a
stand-in `op`. Nothing reaches a model, Discord, or the vault. Build the branch first: `cargo build -p theseusd -p
theseus -p theseus-sim`, or the install build. Save this as `live-37b.sh` and run it as `sh live-37b.sh
target/debug`. It takes about 75 s.

```sh
#!/bin/sh
# 37b's live check: a scratch daemon on a fresh state dir and the simulator's scripted
# stand-in model; nothing reaches a model, Discord, or the vault.
# Usage: live-37b.sh <dir of the build's binaries> [work dir]
set -eu
BIN=$1
W=${2:-$(mktemp -d)}
mkdir -p "$W/bin" "$W/projects" "$W/state"
# A stand-in `op`: every reference reads as tv-<its item>.
cat > "$W/bin/op" <<'OP'
#!/bin/sh
case "$1" in
  inject) sed -E 's#\{\{ op://Test/([^/]+)/[^}]* \}\}#tv-\1#g' ;;
  read) for a; do ref="$a"; done; printf 'tv-%s' "$(echo "$ref" | cut -d/ -f4)" ;;
  *) exit 1 ;;
esac
OP
chmod +x "$W/bin/op"
# The template, made safe: the stand-in model at 127.0.0.1:9448, secrets in the vault `Test`
# (no GitHub token), Discord, the web UI, and the index tender off.
"$BIN/theseusd" example-config --plain | awk -v proj="$W/projects" '
  /^\[/ { sec = $0 }
  /^api_base = "https:\/\/api\./ { print "api_base = \"http://127.0.0.1:9448\""; next }
  sec == "[secrets]" && /^github_token = / { next }
  sec == "[secrets]" && /^[a-z_]+ = "op:\/\// { print $1 " = \"op://Test/" $1 "/credential\""; next }
  (sec == "[discord]" || sec == "[web]" || sec == "[index]") && /^enabled = true/ { print "enabled = false"; next }
  sec == "[tools]" && /^projects_dir = / { print "projects_dir = \"" proj "\""; next }
  { print }' > "$W/config.toml"
cat > "$W/rules.json" <<'RULES'
[
  {"when": "⏰ wake", "text": "Checked again: the build is green. Done."},
  {"when": "Watch the build", "calls": [{"name": "task_create", "input": {"brief": "Check the build now, and again in a minute, then report.", "wake_parent": true}}]},
  {"when": "a background task started by session", "calls": [{"name": "wake_at", "input": {"after": "1m", "note": "Look at the build once more, then report."}}]}
]
RULES
"$BIN/theseus-sim" fake-model --addr 127.0.0.1:9448 --rules "$W/rules.json" > "$W/model.log" 2>&1 &
MODEL=$!
PATH="$W/bin:$PATH" OP_SERVICE_ACCOUNT_TOKEN=test-not-a-token \
  "$BIN/theseusd" --config "$W/config.toml" --state-dir "$W/state" --socket "$W/sock" > "$W/theseusd.log" 2>&1 &
DAEMON=$!
T="$BIN/theseus --socket $W/sock"
until $T health > /dev/null 2>&1; do sleep 0.2; done
echo "== the turn starts a task"
$T ask "Watch the build"
sleep 3
echo "== theseus tasks (the task waits on a wake)"; $T tasks
echo "== theseus wakes (names the task)"; $T wakes
echo "== waiting 65 s for the wake"
sleep 65
echo "== theseus tasks (complete)"; $T tasks
echo "== theseus wakes (none)"; $T wakes
echo "== the report in the parent, once"; $T history | grep -c "Report from task" || true
$T history | grep -A2 "Report from task" || true
echo "== the ledger: the wake, the report's wake"
$T ledger -n 200 -k wake.set; $T ledger -n 200 -k wake.fired; $T ledger -n 200 -k task.report_wake
$T shutdown || true
kill "$MODEL" || true
wait "$DAEMON" 2>/dev/null || true
echo "work dir: $W"
```

What each step should show:
- **The turn.** `task.create` is notified and runs. The reply is `Done.`.
- **`theseus tasks` at +3 s.** The task (`<short>`) shows `○ ready · wake HH:MM`, `1 turn`, `wakes its parent`.
- **`theseus wakes` at +3 s.** One wake: `… once  task <short> (waiting)  Look at the build once more, then
  report.`. This is the line that names the task.
- **After 65 s, `theseus tasks`.** The task is `· complete … 2 turns … reported`.
- **After 65 s, `theseus wakes`.** `no pending wakes`.
- **The report count.** `1`. The report node reads `[Report from task <short> (…): finished after 2 turns, …]`
  followed by `Checked again: the build is green. Done.`, then the parent's answer to it (the report's turn, from
  `wake_parent`).
- **The ledger.** One `wake.set`, one `wake.fired` (on the task's execution), and one `task.report_wake`.

Port 9448 is the default of both `discord model` and `fake-model`. If another stand-in is on that port, change
`--addr` and the awk line together.

## Left open, uncertain, or for the owner

- **The kernel rule's reach.** `wakes::task_unparked` in `end_turn` also catches a task's `max_loops` /
  late-result path, which the core then wakes again (`kernel.wake(…, "late_result")`). Such a turn now writes two
  `execution.queued` rows (`task_unparked`, then `late_result`) where it wrote one. It is harmless, and it closes
  the old crash window between those two frames. I left the core's second wake as it was, to stay out of the
  turn's rewake path.
- **A `/stop` on a task** still leaves it waiting on input with no wake (pre-existing, W1's stop branch). Nothing
  continues it but a turn submitted to the task's session. Worth its own issue: a stopped task should probably
  report, or the stop should say how to go on.
- **The pill.** A parked task's attention pill is `○ ready · wake HH:MM` (the push's `waiting()` for input with a
  wake). That reads as "ready for you", but no one gives a task input. "parked · wake HH:MM" at Working would be
  truer. That is a change in `theseus-protocol/src/push.rs`'s `attention()`, and needs to know the execution is a
  task (`ExecutionView` has its kind). I left it alone: it is the board's wording, read by every surface.
- **A parked task's wake target** is the parent's place (`outbox.target` of a task), so `/wakes` in the parent's
  place lists it, and `wake.list` with its `target` finds it. The wake's turn posts nothing, as task turns never do; the
  report does.
- **External text.** A one-shot wake in a task keeps T1b's exemption, unchanged, and a repeating one is refused
  before the gate matters.
- **The Observatory** (`web/`, being deleted) is not updated. The cockpit's panel is.
- **`tests_output`'s golden depends on the machine's zone** (pre-existing, not mine). It pins a negative UTC
  offset (`-#:#`), so it fails in UTC, here and on `main` alike. It passes under `TZ=America/Los_Angeles`.
  Masking the sign in `tests_output`'s normaliser, or pinning a zone in the test, would make it hermetic.

## Docs to write at review (I edited none)

- **The spec, Part III.** A 37b item: what was built (above), the kernel rule, the proofs, and the `/stop` and pill
  notes as open items. Bump the version line.
- **The spec, §3.3 / §3.15 and DD8's text.** Wherever it says "a task cannot set a wake", say instead that a task
  sets one-shot wakes and parks on them; that a repeating wake is refused in a task; and that a task never waits on
  input with nothing to wake it.
- **`docs/design/m7-surface.md`.** In §2.2's 37b, mark it built. Note the extra case (the operator's cancel of a
  parked task's last wake ends it, and it reports) and `fake-model` for the live check. In the 37b test entry, its
  "kill -9 while parked" is the daemon test above.
- **`docs/status.md`.** The Updated line, the recently landed step, and row 64's 37b.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run twice.

1. **Before `e837edc`** (in the VM's UTC zone): suite 1760 run, 1727 passed, 33 failed, 10 skipped. The 33 were:
   - 32 sandbox cases (19 `theseus-sandbox::contract`, 1 `theseus-sandbox::bench spawn_100`, 12 `theseusd::sandbox`),
     all because the VM runs as root ("Linux exempts root from RLIMIT_NPROC", theseus-pv6i);
   - `tests_output::the_cores_output_matches_its_golden`, from the zone, as above. It fails identically on `main`
     here, and passes with this change under `TZ=America/Los_Angeles`.
2. **Before `ce9c585`** (under `TZ=America/Los_Angeles`): suite 1761 run, 1729 passed, 32 failed, 10 skipped. The
   32 are the same sandbox cases.

After each, I ran the remaining phases by hand, and each passed:
- the reader rule, 9 of 9;
- protocol types;
- the turn bench, `frames_plain: 5 frame(s) at the p95, budget 5: ok`;
- `cargo deny --offline check`, with advisories, bans, licences, and sources all ok (`cargo deny fetch` worked
  here);
- the web apps' lint and build;
- the cockpit's lint, test, and build;
- the web dist check.

fmt, shape, and clippy passed in both gates. The benches were skipped (`NO_BENCH`), as the preamble says.
