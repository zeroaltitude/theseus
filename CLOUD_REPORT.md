# CLOUD_REPORT: retry-dismiss (theseus-7zgsm)

Branch `cloud/20261013-retry-dismiss`. Work commit `dffb8faf`. Base: the task commit on main as cloned (main had
**store format 33**, not 27 as the brief says; this branch makes it **34**).

## Step: a failed task stays flagged until dismissed; retry opens a linked task

### What I found
- `attention()`/`attention_of()` kept a failed, blocked or budget-exhausted execution at "needs you" for good; nothing in
  the record could say it was seen. The work board's rollup (`below.failed`), the short line and the TUI all key on
  that level, so one new fact on the execution makes every surface agree.
- `Kernel::open_task_at` needs a parent's `TurnGuard`; a retry has none, so it got its own kernel transition.
- A conversation's lasting provider failure (a 400) parks the execution on input rather than failing it, so "a
  conversation's failed turn" is dismissable only when its execution is failed/blocked/budget-exhausted (kernel check
  `ExecState::is_failure`). Not exercised end to end for conversations; see "uncertain".

### What I changed (commit `dffb8faf`)
- **Store format 34**: `Execution.dismissed: Option<Dismissal{at_ms,turns}>` and `Execution.retry_of`, both left out of the
  bytes when unset. Layout sample `EXECUTION_AT_33` and a format-33 store test in `tests_layouts.rs`; the four
  "moved to this build's format" asserts, `store.rs`'s one, and `theseusd/tests/versions.rs` now say 34 (35 for the
  "newer" case). `MANIFEST_FORMAT` doc line added.
- **Kernel** `retry.rs`: `dismiss_execution` (one frame: the record + `execution.dismissed` row; again writes nothing) and
  `open_retry` (one frame under failed/parent/new locks: new queued task with the failed one's `reports_to` and
  `wake_parent`, carve `min(failed limit, parent left)`, `retry_of`, the failed one dismissed, the core's records, rows
  `execution.opened`, `budget.carved`, `execution.queued`, `execution.dismissed`). `RetryError` (a separate type: kernel.rs
  is at its line ceiling). A dismissal ends when the execution takes another turn (`Dismissal.turns`).
- **Protocol** `retry.rs`: `execution.dismiss`, `task.retry` params/results, `failure_options()` (Retry for a task, Dismiss,
  Details), `is_failure`, `needs_dismissal`. `ExecutionView`/`WorkView` gain `dismissed` and `retry_of`; both attention
  rules give idle `failed: <reason> · dismissed`. Ledger kinds `execution.dismissed`, `task.retried`. TypeScript regenerated.
  `answer_options()` itself is unchanged (a failure has no `Question`; `failure_options` is the failed task's three).
- **Core**: `rpc/retry.rs` (`Core::dismiss_failure`, `Core::retry_failed`, both `judge_act(Act::Session)`), `fact/retry.rs`,
  the work board (`dismissed` leaves the parent's `failed`/`needs_you`, a dismissed failed task is prunable, `same_work`
  compares the new fields). The retry's brief is the failed brief's words plus one line saying it is a retry, why the last
  try ended, and `--note`; a retry of a retry strips the previous line.
- **CLI** `theseus dismiss ID`, `theseus retry ID [--note]` (`retry.rs`; short ids by `resolve::pick`), both in
  `client::OPERATORS` (refused in a job). To keep `run()` under clippy's 100 lines, `Dismiss`/`Retry` are one arm and
  `Resume`/`Done` were folded into one arm (`remind::settle`, behaviour unchanged).
- **TUI**: failure card `[r] retry [d] dismiss [o] open` (`card::failure_lines`), `r`/`d`/`o` act on a failed row under the
  cursor; anywhere else `r`/`d` still filter ready/done (a conflict with the existing filter keys: the owner may want other
  keys).
- AGENTS.md of protocol, kernel, core, tui, theseus updated.

### How I proved it
- Unit/integration (all pass): kernel `tests_retry.rs` (7: one frame, survives a crash, once only, idempotent, ends with
  the turn, carve cap, retry of a retry, refusals), core `tests_retry.rs` (5: dismissal leaves needs-you on the execution
  board, the work board, the parent's rollup and the short line, one frame, then a restart over a copy of the store and its
  seed still say dismissed; retry links and runs to completion, same brief and pieces, one frame; a retry of a retry links
  back; shared place refused and ledgered, parent ended refused with "has ended"; only failures), protocol (`attention`
  join test over dismissed, `failure_options`), TUI `tests_failure.rs` (4), CLI `retry::tests`, and the existing
  `the_operators_methods_are_refused_inside_a_job` covers the two new `OPERATORS` entries.
- Full suite `cargo nextest run --workspace --no-fail-fast`: 4539 run, 34 failed: 33 are the known L1/sandbox failures
  (theseus-sandbox contract/bench, `theseusd::sandbox`), and `theseus-core tests_kept::fs_read_of_a_long_file_is_a_contiguous_window_and_other_tools_keep_both_ends`,
  which **passes alone** (load flake; not in the brief's list: unexplained, unrelated files).
- Planted reverts: (a) dismissal written only as a row, not on the record: 4 tests fail (kernel
  `a_dismissal_is_one_frame_on_the_record_and_survives_a_crash`, `a_dismissal_ends_with_the_turn_it_dismissed`; core
  `only_a_failure_is_dismissed_and_only_a_failed_task_retried`, `a_dismissal_moves_a_failure_out_of_needs_you_everywhere_and_survives_a_restart`).
  (b) retry carving the parent's whole remainder: kernel `a_retry_carves_no_more_than_the_failed_task_had_and_dismisses_it_in_one_frame`
  fails; the core test does not (the core rig's limit notifies, which takes the other branch).
  Both files restored and touched; `git status` clean.
- Not run by me: the gate's `features` phase (its helpers need the whole script), the `lifecycle`/`turn` benches (no
  bench here; nothing on the turn path changed, the frame count of a plain turn is untouched).

### Gate result
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` **failed in the keel (and the door)** on findings that are not
in my diff: `crates/theseus-sim/src/lifecycle.rs:155: budget-raised` and a renamed store test, from main's own commits, whose
owner ack (`Keel: budget-raised ...`) does not verify here (`%G? is E`: no signing key on this VM).
Run by hand after it: `cargo fmt --check`, `scripts/shape.sh`, `cargo clippy --workspace --all-targets -- -D warnings`
all clean; cockpit `lint`, `test`, `build` pass; full suite as above; generated TypeScript committed (no diff after the
tests); `cargo deny --offline check`: advisories FAILED (database fetched here, no Cargo.lock change from me), bans,
licenses, sources ok.
**Keel findings expected (mine): none.**

### Live check for the maintainer (not done here: no stand-in model daemon was set up on this VM)
```
theseusd --config <scratch.toml> --socket /tmp/s/sock --state-dir /tmp/s/state &   # stand-in model; task whose rule fails
theseus --socket /tmp/s/sock tasks                                  # the failed task
theseus --socket /tmp/s/sock status                                 # counts it as failed
theseus --socket /tmp/s/sock tui                                    # its card shows; `d` dismisses
theseus --socket /tmp/s/sock status                                 # no longer counts it
theseus --socket /tmp/s/sock retry <4 chars of a second failed task's id> --note "try again"
theseus --socket /tmp/s/sock tasks                                  # a new task, retry_of the failed one, runs to done
theseus --socket /tmp/s/sock work                                   # the dismissed task shows `· dismissed`
```
Expect `retry` to print the new short id and a limit no larger than the failed one's.

### Left / uncertain
- The retry's new brief has no `derived_from` edge to the parent's reply (the original's has).
- Two `retry` calls on one failed task open two tasks (the new id is fresh each time; the failed one is dismissed after the first).
- `r`/`d` on a failed TUI row shadow the Ready/Done filter keys there.
- Not touched, per the brief: Discord buttons (`discord-commands`), `cli-tasks`' hint line under a failed row, the cockpit UI.
- Doc changes for the maintainer: spec Part III item (store format 34; the `failure_options` choice instead of a new `QuestionKind`),
  `docs/status.md`.
