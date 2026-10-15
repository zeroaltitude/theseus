# CLOUD_REPORT: routine-trust (HZ5, theseus-52tut)

Branch `cloud/20261013-routine-trust`, from the task commit 8f650ed8 (store format 34, as `routines` left it).

## What I found

- `TaskRun.tools` (the wake's "tools it may use without asking") already exists and is stored, so the grant needs **no stored-field change and no
  `MANIFEST_FORMAT` bump**. Everything new in META is a new key (`routine.blocked:<task>`, `routine.ping:<routine>`), which AGENTS.md treats as
  no bump. `RoutineSpec` is unchanged. The wire types gained optional fields only (`RoutineInfo.grant|streak`, `RunRow.blocked`,
  `RoutineCreateParams.grant`, `RoutineUpdateParams.grant|add_grant`, `WorkAnswerResult.detail`).
- The gate never refuses, and the owner's `[policy]` has no "deny" posture (`Posture::parse("deny")` is `None`). So "a grant naming a tool the
  owner's policy denies" is built as: a tool the daemon does not have, or one of `grant::NEVER` (`proc.run`, `term.open|send`, `routine.create`,
  `extend.propose`, `task.update|split|close`, `aws.call`, `aws.hands.run`, and any `mcp:ext-*`), or `channel.post` without its place. **Design
  choice for the owner**: the NEVER list is mine, and is the thing to revisit at the evaluation pass (decision 7: scoped grants want an Opus
  security review; I am Sonnet and did not get one).
- A run's failure report and its cancel report were posted to the DM on every failure; a routine failing every minute would post every minute.
  They are now left out for a routine's run that did not complete (`watch::replaced`, `cancelled_report`), because the routine's own notice
  tells it once a day.

## What I changed

One commit, 57009a88 (`routines: a routine's grant, Try it now, ...`). By step of the brief:

1. **The grant**: `routine/grant.rs` (parse, `check`, `Grant::covers`, the gate step), `Book::grant_for` (`routine.rs`), `Layer::Routine` in
   `toolrun/order.rs` after the glide's layer and before T1's hold, `At.routine`, set in `toolrun.rs`'s `gate()` from `tc.task` only. The model's
   `routine.create` takes `grant` (checked in `plan`, and against the registry when the owner's yes lets it run); the card says
   `runs use without asking: ...`. FAST: a non-task session pays one `Option` test; a task whose parent is not a routine one map lookup.
2. **Try it now**: `routine.try {id}` (`rpc/routines.rs`, `routine::plan_trial`): occurrence 0, `RunMark.run` 0, not counted in `runs`, the
   series untouched; `theseus routine try`. **Not built: the creation card's `[Try it now]` button.** The card is a tool-call confirm; a button
   needs an `approve_try` answer remembered until the tool runs after the resume (the routine does not exist at the yes), then a trial from
   `routine_tool::run`. The verb it would call is done.
3. **Bounded asks**: `[routines] run_deadline_secs` (900, in the template). `ask_ttl_ms` gives a routine run's card, `confirm.list` and the
   expiry scan the run deadline. Past it `expire_questions` queues the question and the harness tick (`fire_notices_and_routines` ->
   `routines_tick`, `rpc/routines_watch.rs`) writes `routine.blocked:<task>` `{tool, reason}` and cancels the run; `RunRow` says `blocked`. The
   work board's and `pending_by_execution`'s expiry times still use the global lifetime (equal by default).
4. **The ping rule's entries** (data and pure functions in `theseus-protocol/src/notices.rs`): `ROUTINE_PURPOSES`, `routine_urgency`,
   `routine_pings` (once per routine per local day), `routine_line` (streak from the second), `routine_answers`. Sent as `wake.noticed`
   (`fact::wake::RoutineNoticed`, so the TUI, the CLI and the cockpit already read it) and posted to the routine's target as a
   `reminder`-shaped post with its answers (what `horizon-discord` draws). The ping book is META `routine.ping:<routine>`. Skipped and
   missed-while-down occurrences inform (event only, no DM post). A run that reports keeps the existing task report (H-4).
5. **Answers**: `work.answer` on the routine's wake id: `run_now`, `pause`, `details`, and `add_to_grant` after a block
   (`answer_routine`; `WorkAnswerResult.detail` carries Details). `answer.rs` got one arm.
6. **Reader**: `theseus routine add --grant a,b`, `routine update --grant|--add-grant`, `routine try <id>`, and the grant and the streak in
   `routine list`.

Files: new `routine/grant.rs`, `routine/grant/tests.rs`, `routine/watch.rs`, `rpc/routines_watch.rs`, `tests_routine_trust.rs`; the rest are
small edits (list in the commit). Generated: `cockpit/src/protocol.gen/` (RoutineBlocked.ts new, five changed). AGENTS.md of theseus-core and
theseus-protocol say what is here.

## How I proved it

- `cargo nextest run -p theseus-core -E 'test(tests_routine_trust) | test(routine::grant)'`: 10 passed.
  - `a_granted_tool_runs_unasked_after_the_run_reads_a_page`: a routine with `grant: [fs.write, fs.edit]`, tried: the run fetches a page (its
    session holds external text), writes and edits, no question, the notice says `ran under the routine's grant: fs.write`; run 0, `runs` stays 0.
  - `without_a_grant_the_same_call_asks_and_so_does_any_other_session`: the control (the same run asks for `fs.write`), and an ordinary
    conversation with the same hold asks for `fs.write` although a routine with that grant exists.
  - `an_ungranted_tool_asks_once_and_the_run_ends_blocked_naming_it`: `run_deadline_secs = 1`; one `tool.confirm_requested`, no
    `question.expired`, the run `blocked` with `{tool: fs.edit, reason: "fs.edit needs approval (not in its grant)"}`, one notice
    `✗ Tide check blocked: fs.edit needs approval (not in its grant)` with `[add_to_grant, run_now, pause, details]`, no cancel report; then
    `work.answer add_to_grant` on the routine's wake grants it, `details` answers in words, and the next run completes.
  - `a_grant_naming_a_denied_tool_is_refused_at_creation` (`proc.run`, an unknown tool, a bare `channel.post`; and `routine.update --add-grant`).
  - `the_grant_applies_to_the_routines_own_runs_and_nothing_else`: a task a run starts, another conversation's task, a forged execution: no grant.
  - `a_failing_routine_pings_once_a_day_with_its_streak`: failure 1 pings; failure 2 the same day tells nothing and the streak is 2; after the ping
    book's day is rewound to a past date ("midnight"; the core's clock is the real one, there is no paused clock to cross midnight with), failure 3
    pings `(3 in a row)`; no failure report of the runs' own is posted.
  - Protocol `tests_notices.rs`: three new tests (the entries' urgencies, once per day, the line, the answers, the notice).
- **Planted reverts** (each restored, `touch`ed, `git status` clean after):
  1. The grant honoured outside the routine's tasks: (a) `Book::grant_for` looking up any routine instead of the task's parent: caught by
     `the_grant_applies_to_the_routines_own_runs_and_nothing_else` (task of another conversation got the grant). (b) The gate giving a
     conversation session the first granted routine's grant: caught by `without_a_grant_the_same_call_asks_and_so_does_any_other_session`
     (`asked[0].tool == "fs.write"` failed) after I strengthened it; the first version of that assertion passed under the plant (the model's
     next call, `fs.edit`, asked), which is why the assertion names the tool.
  2. A blocked run expiring silently (`queue_if_routine` returning false, so the generic expiry declines it): `an_ungranted_tool_asks_once...`
     failed after 30 s with "no ... in 30 s" (no blocked notice).
  3. A ping every time (`routine_pings` always true): `a_failing_routine_pings_once_a_day_with_its_streak` failed (never saw the "told today" row).
- `cargo nextest run -p theseus-protocol -p theseus`: 537 passed, 1 failed once (`theseus::markdown rendering_costs_little_per_hundred_lines`, a
  timing test under load; it passed alone), none new from this change.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets`: clean. Line ceilings: no listed file grew past its ceiling.

Second commit (see `git log`): shortens the model tool's text (its longer description and the grant's schema text made every request
bigger and broke `tests_route_keep::a_kept_first_compile_holds_the_recall_drops_in_its_loop_alone`, a window-sized test; it passed on the base,
failed on my first commit, passes now) and adds `the_creation_card_shows_the_grant` and the CLI's `the_list_shows_the_grant_and_the_streak`.
Consequence: the model sees `grant` in the schema with no description; the owner may want a sentence in the tools note instead.

## Gate

- `scripts/gate.sh` as given fails in its **keel** phase here, on commits of the base (a `Keel:` ack whose signature can't verify in this
  clone, `crates/theseus-sim/src/lifecycle.rs`; origin/main in this clone is older than the branch's base). Not mine. I ran it with
  `THESEUS_KEEL_BASE=8f650ed8` (keel: ok, 0 findings for this branch; `python3 scripts/keel-guard.py --base 8f650ed8`).
- With that: door, keel, fmt, shape, features, clippy, cockpit build all passed on the first commit. The **test build** then died with "No space
  left on device" (the VM's disk allowance; 28 GB of target). I cleared target/debug/incremental and target/release and **did not rerun the
  whole gate** (it needs ~15 min twice through the door); deny, the daemon's tests, the smoke and the benches were not run.
- Instead, on the final tree: `cargo clippy --workspace --all-targets` clean, `cargo fmt` clean, and
  `cargo nextest run -p theseus-core -p theseus-protocol -p theseus`: **2839 passed, 0 failed**. Before the second commit, the run over
  core, protocol, theseus, theseus-tui and theseus-kernel was 3219/3222: `tests_standing::the_records_rebuilt_after_a_restart...` and
  `tests_work::the_substrate_scenario...` failed in the full run and passed alone (ordering/timing, not touched by this change), and
  `a_kept_first_compile...` was mine (fixed above). theseus-tui and theseus-kernel passed in that run; theseusd's tests were not run.
- Keel findings expected: none.

## Live check (not done here; for the maintainer)

I ran out of time for the scratch daemon. The in-process tests drive the same paths with a real core and a stand-in model and web server. To
run it: `theseus-sim fake-model --rules rules.json` with one rule `{"when":"Tide check","steps":[http_fetch of a local page, fs_write, fs_edit, text]}`,
a scratch daemon with `[routines] run_deadline_secs = 20` and `private_addresses = "open"`, then
`theseus routine add "every 1m" "Tide check: read the page and keep a note" --title "Tide check" --grant fs.write` and
`theseus routine try tide`: expect the run to write the note unasked after the fetch, ask for `fs.edit` (`theseus confirm`), and 20 s later
end `blocked` (`theseus routine runs tide` shows `blocked: fs.edit needs approval (not in its grant)`) with one
`✗ Tide check blocked: ...` notice; `theseus answer <routine wake> add_to_grant` (or `routine update tide --add-grant fs.edit`), then
`theseus routine run tide` completes.

## Left or uncertain

- The creation card's `[Try it now]` button (above). Skipped/missed Inform events are emitted but have no test. Run `catch_up`'s "missed while down,
  inform once at start" is one Inform per skipped occurrence, not one aggregate.
- `ask_ttl_ms` is applied to the run's cards, `confirm.list` and the expiry scan; `push` board and `pending_by_execution` still show the global
  lifetime (same by default, 900 s).
- The config key is `run_deadline_secs` (the brief says `run_deadline`); the repo's keys carry their unit.
- The grant's `NEVER` list, and L1: a granted call that is an L1 call is not loosened (L1's own asks stay), including the hold. Owner decision.
- The "midnight" in the streak test is a rewound ping book, not a paused clock (the core's tests use the real clock).
- Docs to update at review: the spec's Part III item and `docs/status.md`; `docs/design` horizon row HZ5.
