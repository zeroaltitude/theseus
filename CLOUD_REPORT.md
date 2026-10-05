# CLOUD_REPORT: an earlier process's in-flight call is booked as spent at its unknown mark (theseus-f3wr)

Branch `cloud/20261005-crash-hold`, from `main` at 60b43fb6 (store format 20). Started 20:22 UTC, report at
about 21:35 UTC.

| Commit | Step |
|---|---|
| `ca9c1be5` | kernel: the earlier-process mark books the reservation as spent; the existing marking test updated |
| `b22e0c91` | kernel: four new tests (reset frees it, overdue still holds, a late resolution counts it once, a task's parent counts it once) |
| `f7c76bcb` | sim: a fake-model rule's `hold_ms`, for the live check (a small change outside the brief's list: see step 3) |

No store format bump: no stored field or variant is added (see step 1). No new dependency. No file grows past its
ceiling: `kernel.rs` is 3,017 of 3,030 (it was 3,008).

## Step 1: the kernel books it (`ca9c1be5`)

**What I found.** `mark_earlier_calls_unknown` called `mark_unknown(c, EARLIER_PROCESS)`. That settles the call
through `accept_completion` with `Outcome::Unknown` and `cost_micros: None`, and the `(false, Outcome::Unknown)` arm
calls `hold_reservation_in`, which moves the reservation into `held_unknown_micros`. A reset leaves that alone, so
each crash shrank the session's room for good. Two more places matter for counting it once:

- **The resolve arm** (`was_unknown`, a real outcome arriving later) did `held -= a.reserved; spent += cost or
  reserved`. On a booked call that would take another call's hold and count the cost a second time.
- **The late-after-cancel arm** has the same arithmetic.

No producer can resolve an earlier process's provider call (the process is gone, and `Evidence::in_process` is
`tool == provider.messages` only), but the heartbeat's reconcile re-probes every `OutcomeUnknown` action. So both
arms now go through one helper.

**What I changed.**
- `kernel.rs`: `accept_completion(c)` now calls `accept_completion_as(c, false)`, and `mark_unknown(id, reason)`
  calls `mark_unknown_as(id, reason, false)`. Both new methods are `pub(crate)`. With `book` true and an `Unknown`
  outcome on an action that was not unknown already, the reservation arm calls `book_reservation_in`, which is
  `settle_reservation_in` at the reservation's own amount. So spent goes up by the reservation, reserved goes down by
  it, and held is unchanged. The action gets `detail = {"cost_basis": "reservation"}`. The
  `action.outcome_unknown` row gets `"cost_basis": "reservation"`, with `cost_usd` set to the reservation (it was
  `null`). Both the resolve arm and the late-after-cancel arm now call `earlier::resolve_in`:
  - a held call does exactly what it did before;
  - a booked call keeps the estimate, takes nothing held, and books only a cost above the reservation (a real cost
    is never hidden, and only a reset lowers spend).
- `earlier.rs`: `mark_earlier_calls_unknown` calls `mark_unknown_as(c, EARLIER_PROCESS, true)`. It also gains the
  helpers (`booked`, `mark_booked`, `booked_row`, `book_reservation_in`, `resolve_in`) and a module paragraph on
  why the call is booked.
- `types.rs`: `Action::detail`'s doc comment names the new use.
- A task's parent gets the booked amount through the usual `carry_to_parent`, from the child's spend delta. Its
  carve shrinks by the same amount, so the parent's spent + reserved is unchanged: booked once, nowhere else.

**Store format.** No bump. The marker lives in the action's existing `detail: Option<Value>` field (which outbox
actions already use) and in a ledger row's `data`. Neither adds a stored field or variant. The commit body says so.

**Every other unknown mark is unchanged.** `mark_unknown`'s callers (`overdue_no_evidence` in the reconciler,
`wrapper_lost` in `rpc/driver.rs`, `interrupted_by_restart` in `toolrun/resume.rs`, and the AWS hands' overdue and
poller marks) all reach `accept_completion_as(.., false)`, so they hold exactly as before. The golden of the
kernel's frames (`tests_frames`) is byte-identical.

**FAST.** The booking runs only inside `mark_earlier_calls_unknown`, which runs at the driver's first tick after
serving (`rpc/driver.rs::mark_earlier_calls`), or in the heartbeat's reconcile (`reconcile_with(.., due = true)`).
Startup's reconcile (`due = false`) only notes calls (`note_earlier`) and writes nothing, as before. The booking is
a few additions inside the frame the mark already wrote: no new frame and no new read. On the turn path,
`accept_completion` gains one `bool` that is false. The turn bench still counts 5 frames for a plain turn and 9 for
a tool-call turn.

## Step 2: tests (`ca9c1be5` for the existing one, `b22e0c91` for the rest)

- `tests_earlier::an_earlier_processs_provider_call_is_unknown_at_the_first_tick_and_a_job_is_not` now asserts
  `(spent, reserved, held) == (100, 0, 0)`, no reservations left, and the row's `cost_basis` and `cost_usd`
  (0.0001). It checks that the action's `detail` holds the marker and its state is still `OutcomeUnknown`. A second
  tick, then the heartbeat's reconcile, write nothing and book nothing more.
- `tests_earlier::a_reset_frees_what_an_earlier_processs_call_was_booked`: after the mark, spent is 40,000, held is
  0, and `available_after_reset()` is the whole 100,000. An approved `reset_budget` returns 40,000 as the spend
  before, and leaves `(0, 0, 0)` with `available() == 100_000`. tests_m3's
  `a_reset_that_cannot_free_what_is_held_unknown_asks_once_and_does_not_loop` passes unchanged in the gate's suite.
- `tests_earlier::an_overdue_mark_still_holds_its_reservation_as_unknown`: a provider call of this process, past its
  deadline with no evidence, is `overdue_no_evidence` with `(0, 0, 300)`. Its action has no `detail`, and its row
  has no `cost_basis` and a `null` `cost_usd`.
- `tests_earlier::a_booked_calls_late_resolution_counts_it_once`: two booked calls (500 and 300) and one held overdue
  call (700) start at `(800, 0, 700)`. A resolution at 200 keeps `(800, 0, 700)`; one at 450 gives `(950, 0, 700)`;
  the held call's resolution at 100 gives `(1050, 0, 0)`, as before.
- `tests_tasks::a_tasks_earlier_call_is_booked_once_in_its_parent_too`: the task has spent 4,000 already and has a
  10,000 call in flight at the crash. After the mark, the task is `(14000, 0, 0)` and the parent's spend is 14,000.
  The carve goes from 26,000 to 16,000 and the parent's `available()` is unchanged. A second tick and the heartbeat
  change nothing.

**Proof.**
- `cargo nextest run --workspace -E 'package(theseus-kernel)'`: 162 passed, 1 skipped (158 on main, plus 4 new).
- theseus-core's budget, task, reset and held tests (`-E 'package(theseus-core) & (test(/budget/) | test(/task/) |
  test(/held_unknown/) | test(/reset/))'`, with `TZ=America/Phoenix`): 93 passed.
- Under load (four `yes > /dev/null` at nice 0, the tests at `nice -n 19`), the 12 kernel tests matching
  earlier, booked, overdue or reset, three runs: 12 of 12 passed each time. (The brief's `sh -c 'while :; …'` loop
  was refused by this environment's command checker, so I used `yes` as the busy loop.)
- `cargo clippy -p theseus-kernel -p theseus-sim --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.

**Planted reverts.** Each was restored from a saved copy, `touch`ed, and checked with `cmp` and `git status`.
- *The mark settles with no cost* (`mark_unknown_as(c, EARLIER_PROCESS, false)` in `earlier.rs`): 4 fail. These are
  `an_earlier_processs_provider_call_is_unknown…` (the row's `cost_basis` is `Null`), `a_reset_frees…`
  (`(0, 40000)` against `(40000, 0)`), `a_booked_calls_late_resolution…` (`(0, 0, 1500)` against `(800, 0, 700)`),
  and `tests_tasks::a_tasks_earlier_call_is_booked_once…`.
- *Every unknown mark books* (`mark_unknown` passes `true`): 4 fail. These are
  `an_overdue_mark_still_holds_its_reservation_as_unknown` (`(300, 0, 0)` against `(0, 0, 300)`),
  `a_booked_calls_late_resolution…`, the existing
  `tests::unknown_then_genuine_success_resolves_and_budget_moves_held_to_spent`, and
  `tests_frames::the_kernels_frames_match_their_golden`.
- *The parent books the child's call a second time* (after `carry_to_parent`, add the reservation to the parent
  again when booking): `a_tasks_earlier_call_is_booked_once_in_its_parent_too` fails with a parent spend of
  `(24000, 0)` against `(14000, 0)`. My first try at this plant (passing `spent_before − reserved` to the carry)
  was a no-op, because the child had no spend before the mark. That is why the test now gives the task 4,000 of
  spend before the crash.

One process note: while rewriting the late-resolution test, a script of mine cut the file's last two existing
tests (`the_heartbeat_marks_…` and `a_call_this_process_dispatched_…`). The first gate run and the plants ran
without them. I put them back, unchanged from `main`, before either commit. Both pass at `ca9c1be5` and at the
head, and the second gate ran on the final tree.

## Step 3: the fake model's hold, for the live check (`f7c76bcb`)

**What I found.** The brief's live check needs "a rule that holds its answer for 60 s", but `fake-model --rules`
had no such rule, and it answered one connection at a time.

**What I changed.** `Rule` takes `hold_ms` (default 0, so every existing rules file reads as before). A rules
stand-in answers each connection on its own thread, so one held call holds nothing else; the bench's job and mixed
stand-ins keep their one thread. The `fake-model --help` text and theseus-sim's AGENTS.md say so. The new test is
`fake_model::tests::a_held_rule_answers_late_and_holds_no_other_call`: a held rule's answer comes after 1.5 s, and
a quick rule asked after it answers first. theseus-sim is outside the brief's list of files. The change is small
and only for the stand-in.

**One thing I learned running it.** A hold of 60 s meets the daemon's first-byte timeout ([model.timeouts], 60 s),
so the call fails as `timeout` (phase `first_byte`) at the 60 s mark and the driver retries. Use 45 s.

## The live check

I ran it myself on this VM with debug builds, a fresh state dir, Discord, the web and the index off, and the
stand-in model. The results are below. For the maintainer, here are the exact commands on an install build. The
config helper is the 35-line Python I used (`/tmp/lc/setup.py` here). It takes `theseusd example-config`, points the
model and every provider at the stand-in, makes every secret `env:LC_KEY`, turns off `[discord]`, `[web]` and
`[index]`, and sets `projects_dir` and `[kernel] spend_limit_usd`. Any equivalent config does the same job.

```sh
D=/tmp/f3wr; mkdir -p $D; export LC_KEY=lc-scratch-key-1
python3 setup.py $D 18555 ~/.local/bin/theseusd 100     # writes $D/config.toml
echo '[{"when": "hold the line", "text": "Held, then answered.", "hold_ms": 45000}]' > $D/rules.json
theseus-sim fake-model --addr 127.0.0.1:18555 --rules $D/rules.json &  FAKE=$!
theseusd --config $D/config.toml --socket $D/sock --state-dir $D/state > $D/d1.log 2>&1 &  DAEMON=$!
theseus --socket $D/sock ask "please hold the line" > $D/ask.log 2>&1 &
sleep 3; theseus --socket $D/sock budgets                    # A
kill -9 $DAEMON
theseusd --config $D/config.toml --socket $D/sock --state-dir $D/state > $D/d2.log 2>&1 &  DAEMON=$!
sleep 3; theseus --socket $D/sock budgets                    # B
theseus --socket $D/sock ledger -n 3 -k action.outcome_unknown   # C
```

- **A**: the session is running, with spent $0.0000 and reserved $1.3071 (Sonnet 5.5's worst case for that
  turn), and held $0.0000.
- **d2.log**: `an earlier process's in-process calls are unknown calls=["act_…"]` at the first tick.
- **B**: spent $1.3071 and held $0.0000. Reserved may again read $1.3071: the driver queued the session with the
  unknown result, and its next turn sends the held prompt again, so a new call is in flight.
- **C**: the row has `"cost_basis":"reservation","cost_usd":1.307098` and producer
  `reconciler:in_process_before_restart`.

Then the reset. Stop the held answers and lower the limit, so the next call asks:

```sh
theseus --socket $D/sock shutdown; kill $FAKE
echo '[{"when": "hold the line", "text": "Answered at once."}]' > $D/rules.json
sed -i 's/^"spend_limit_usd" = 100.0/"spend_limit_usd" = 2.0/' $D/config.toml
theseus-sim fake-model --addr 127.0.0.1:18555 --rules $D/rules.json &  FAKE=$!
theseusd --config $D/config.toml --socket $D/sock --state-dir $D/state > $D/d3.log 2>&1 &
sleep 3; theseus --socket $D/sock budgets; theseus --socket $D/sock confirm      # D
theseus --socket $D/sock confirm --approve <the act_ id it names>               # E
theseus --socket $D/sock budgets                                                 # F
theseus --socket $D/sock shutdown; kill $FAKE
```

- **D** on this VM: spent $2.6142 of $2.00, held $0, and "waits at its limit: its next call needs $1.3071". The
  spend is two bookings. One is the crash's call. The other is the retry's call that was in flight at the clean
  `shutdown`, which is also an earlier process's call at this start. Both `action.outcome_unknown` rows carry the
  marker.
- **E**: "approved … by the CLI", then the turn answers "Answered at once." for $0.0002.
- **F**: spent $0.0002, reserved $0, held $0, available $1.9998, resets 1. The reset freed everything that was
  booked.

## Left open, and for the owner

1. **`interrupted_by_restart` (`toolrun/resume.rs::check_dispatched`) is the same case in substance, and it holds
   no money today.** It runs in the continuation after a restart, for a `tool_use` whose action is still
   `Dispatched`. It is not a harness tool, and not a job whose wrapper lives or whose spool holds a result. So the
   process that ran it no longer exists (or, for a job, neither its wrapper nor its result does): the same case as
   `EARLIER_PROCESS`. But every such action is planned by `ToolRuntime::plan_call` with `reserve_micros` 0, and
   `plan_action` makes no reservation when the reserve is 0 (`kernel.rs`, `if reserve_micros > 0`). So
   `reservation_id` is `None`, and hold or book changes nothing. Provider calls (the ones that reserve) never take
   that path: they are not `tool_use`s. AWS hands reserve, but their marks are the hands' own. I left it unchanged.
   If tools ever reserve, it should book as `EARLIER_PROCESS` does.
2. **A gap in the same case: an earlier process's call already past its deadline at the start is still held.**
   Startup's reconcile probes a dispatched action whose deadline has passed instead of noting it (`note_earlier`
   only takes calls not yet overdue). With no evidence it is marked `overdue_no_evidence` at the start, before
   serving (this is what main does), and so it is held. A provider call's deadline is `[model.timeouts]
   total_secs` (600 s). So a daemon down longer than that after a crash still holds the call for good. It is the
   same money problem, outside this brief's scope ("the earlier-process reason only"). One fix: at startup,
   `reconcile_with(.., due = false)` could note an overdue in-process call too, and leave it to the first tick to
   book. That also takes a write off the start path.
3. **Lifetime cost does not include the estimate.** `theseus budgets` showed lifetime $0.0000 while spent was
   $1.3071: lifetime is the session's recorded cost, from its `provider.call` rows, and a booked call has none. So
   "spent" and "lifetime" now differ by the estimate. That is honest (lifetime is what the provider reported), but
   the owner may want the budget page to say so, or lifetime to add the booked estimates.
4. **A clean `shutdown` with a provider call in flight is booked too**, as D shows: the call is an earlier
   process's at the next start. That follows the decision (the request may have been charged).

## Docs, for the maintainer to fold in

**Spec, `docs/spec/04-part1-s3.10.md`, the "A reset leaves held what it can't free" bullet.** Append:

> An earlier process's in-process call is not held (theseus-f3wr, owner's decision 2026-10-05): the request reached
> the provider before the crash and may have been charged, and no completion can bring its cost, so the first tick
> after the restart books its reservation as spent, an estimate that reads true or slightly high, and a reset clears
> it like any other spend. Its action and completion stay `outcome_unknown`; the action's `detail` and its
> `action.outcome_unknown` row say `"cost_basis": "reservation"`, the row with the reservation as `cost_usd`. A
> resolution that came later would book only a cost above the estimate. Every other unknown mark (overdue with no
> evidence, a lost wrapper, a restart's interrupted tool call, the AWS hands') holds as before: there the call may
> still run, or its cost may still arrive.

**theseus-kernel's AGENTS.md, the `earlier.rs` bullet.** Replace with:

> - `earlier.rs` (theseus-m9iy): an earlier process's in-process calls. Startup's reconcile notes each dispatched
>   provider call (`Evidence::in_process`, by its tool) in memory and writes nothing; the driver's first tick after
>   serving (or the heartbeat) marks them all `outcome_unknown` in one frame, as `in_process_before_restart`, each
>   reservation booked as spent, never held (theseus-f3wr): `mark_unknown_as(.., book: true)`, the action's
>   `detail` and row `"cost_basis": "reservation"`. Every other `mark_unknown` holds. A resolution of either goes
>   through `earlier::resolve_in`, so a booked call is never counted twice.

Under the AGENTS.md's Invariants, after "An attempt that may have run is `OutcomeUnknown`", add: "Its money is
held, unless its process is gone (an earlier process's in-process call), when it is booked at its reservation
(theseus-f3wr)."

## The gate

Run on the final tree (`f7c76bcb` plus this report): `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`.

**Suite:** 2,794 tests ran: 2,761 passed and 33 failed, all 33 of them the known L1 failures this VM causes (a
root daemon's job without a job cgroup, theseus-pv6i):
- theseus-sandbox's 19 contract tests and its bench's `spawn_100`;
- theseusd's 13 `sandbox` tests.

Nothing else failed and nothing was retried. The core's golden (`tests_output`) passed under
`TZ=America/Phoenix`.

**Phases before the suite:** fmt, shape (no file over 2,500 lines but the 10 listed), features, clippy, the cockpit,
and the test build all passed. The gate noted one crate (`theseusd`) compiled under the lock. The first gate run
noted none.

**Phases after the suite, run by hand:**
- `protocol types`: no change in `cockpit/src/protocol.gen` (no protocol type was touched).
- The benches are skipped under `THESEUS_GATE_NO_BENCH`.
- `turn` (`theseus-sim bench turn --check --runs 5 --burst 0`): frames_plain 5 of budget 5, frames_tool 9 of 9.
- `deny` (`cargo deny --offline --log-level error check`): advisories, bans, licences and sources ok. Its database
  was fetched at setup.

The first gate run, on the tree before the two restored tests and the doc lines, gave the same result: 2,792 ran,
with the same 33 failing. The commit is green by the brief's rule.
