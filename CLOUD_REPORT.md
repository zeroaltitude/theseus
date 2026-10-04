# Cloud report: step 28a, independent check tasks (theseus-vug.3)

Branch `cloud/20261004-independence`, from `main` at 78d749c (39a merged). Started 15:25 UTC, report 16:55 UTC.
One code commit: **20dadc1** `core: check tasks: check_of, the exclusion, the overlap flag, and the basis (theseus-vug.3)`.

## What I found

- `task.create` (39a's shape) opens a task session through `open_task`'s one frame: brief node, `Arrangement`
  node (27), edges, outbox record, and the 39a `TASK` record. `compile()` reads only the child's own nodes, so
  what a check sees is whatever that frame writes into its session, plus the task graph's view (39a).
- A task's report is its last assistant message in its own session (`task::load_report`). The parent's next turn
  relays it as a `[Report from task …]` node with a `derived_from` edge (via `report`) to that message.
- `TurnCtx` has no config, so `profile` could not be resolved where `task.create` runs. I added
  `ToolRuntime::profiles` (name → provider and model, built from `Config::all_profiles`) and passed it to
  `task::create`. A check's session gets that `last_target`, and the driver's `target_for_session` runs its turns
  on it.
- **The task graph's view (39a) reaches every task, checks included.** It shows each task record's title,
  objective and acceptance: the parent's words, not the checked session's nodes. But if the checked task edits
  its own record while it works (`task.update` notes or evidence), that text would reach the check through the
  view, outside the exclusion. I left 39a alone, as the brief asks. **The owner should decide** whether a check's
  view should leave out the checked task's record, or show only its title.

## What I changed (20dadc1)

- **`check_of` and `profile`** on `task.create`. The changes to its `Input`, schema and description only add
  things; the logic is in `crates/theseus-core/src/check.rs`.
  - `check_of` resolves only among the tasks this conversation started (`kernel.tasks(Some(execution))`, then
    `task::resolve`).
  - A task with no report is refused, saying why: still running or waiting ("is waiting and has not reported"),
    or failed or cancelled ("cancelled (…), and has no report to check").
  - **`profile` is open only to checks.** Any other task that names one is refused as invalid input, and an
    unknown profile is refused with the configured names. A check without `profile` runs on its parent's target.
  - **27's rule: `check_of` meets it.** The checked task's standing `objective` and `acceptance` pieces stand for
    the check's own, so a check needs no arrangement. It may add pieces, which resolve as 27's do. If the checked
    task has no objective piece (it predates 27, or it has only a `design` piece), the check must bring an
    objective or design piece of its own.
  - **The fidelity check does not apply to a check.** A check's brief is short by nature, and its work is the
    claim, not the parent's discussion.
- **The admission.** The check's `Arrangement` node gets a new optional `claim` (`check::Claim`: the task, its
  report node, when it was written, and the report text capped at 16,000 characters). `check::render` renders the
  pieces, then a short framing paragraph, then `--- Claim: claimed by task a1b2c3, as of <UTC> --- <report>`. The
  compiler, recall's render and `node_info` use it.
  - The node gets a `derived_from` edge to the report (`graph::VIA_CLAIM = "claim"`), as well as the edge to each
    piece's node.
- **The exclusion on the check's own pieces.**
  - A piece whose node copies the report (an edge into the report: the parent's relayed node) does not become a
    piece. It is recorded as admitted with role `claim`, from `own`.
  - A piece whose node derives, by `derived_from` edges followed back from any other node of the checked session
    (at most 20,000 nodes visited), is refused, class `excluded`, naming the node and its source.
- **The overlap flag** (`check::overlaps`, `OVERLAP_WORDS = 12`).
  - **What is compared:** the brief and each of the check's own admitted pieces, against every node of the checked
    session except its report, its brief and its arrangement. The default the brief proposed: those are admitted,
    or the parent's own words.
  - **How words are counted:** a word is a maximal run of letters and digits (`char::is_alphanumeric`), lowercased.
    Punctuation, case and spacing neither make nor break a match.
  - A node's words are its message text, a reply's text, its thinking and its tool_use inputs, a call's input
    strings, or a result's content.
  - Each maximal run of shared 12-word windows is one flag: its source (`brief` or `piece N`), the node holding
    its first window, its length in words, and the span as the source wrote it (cut at 200 characters).
- **The basis.**
  - `theseus_protocol::TaskCheck` (new module `crates/theseus-protocol/src/check.rs`) is stored on the check's
    session record as `TaskOf.check`. It holds the checked task, its report node and time, the excluded sessions,
    the admitted pieces (each with `from`: `checked`, `own` or `claim`), the profile, provider and model, the
    overlaps, and `at_ms`.
  - The ledger rows are `task.check_opened` (the whole basis) and `task.check_refused` (class, `check_of`, reason).
    Their facts are in `fact/check.rs`.
  - The one wording, `TaskCheck::line()`, is `🔍 check of task a1b2c3 · independent (excluded ses_…a1b2c3, <model>)`,
    with `· overlap: N spans` added when anything is flagged. It shows on:
    - `task.list` (`TaskInfo.check`);
    - `theseus tasks` (a line under the task, and one line for each flag);
    - the report's outbox post (`check`), which Discord shows under the report's head;
    - Discord's `/tasks` line (`· 🔍 \`a1b2c3\``);
    - the report node the parent reads (the line after its head);
    - the `task.create` result, which also lists each flagged span and puts the basis in its meta;
    - the cockpit's task view (`cockpit/src/lib/check.ts` mirrors `line()`, and each flag shows on hover).
- **Store format 12 → 13** (`TaskOf.check`, `Body::Arrangement.claim`; both are absent when unset, so older records
  keep their bytes). `tests_layouts` gets two literal samples: a format-12 task session with its arrangement, and
  a format-12 arrangement node with a trusted piece, a superseded piece and the fidelity ack.
  `theseusd/tests/versions.rs` moves to 13 (and 14 for its "newer" store).
- **TypeScript.** I regenerated `cockpit/src/protocol.gen/` (TaskCheck, CheckPiece, CheckOverlap, TaskInfo, index).
- **Line ceiling.** `scripts/long-files.txt` raises protocol `lib.rs` from 2654 to 2661, with the reason in the file.
- **Directory guides.** The `AGENTS.md` of theseus-core and theseus-protocol each get an entry.
- **Tests I had to touch.**
  - `tests_arrangement.rs`: one `let Body::Arrangement { pieces, fidelity_ack, .. }` pattern gains `..`, because
    the body has a new field. No assertion changed.
  - `theseusd/tests/versions.rs`: the format numbers.
  - No other existing test changed. 27's refusal and fidelity tests, DD7's `tests_tasks`, and `tests_task_wakes`
    pass unchanged.
- **Dependencies.** No new dependency; `Cargo.lock` and the package-lock files are unchanged.

## How I proved it

- **New tests.**
  - `tests_check.rs` (through the whole core):
    - `a_checks_compilation_holds_nothing_of_the_checked_session_but_its_claim`: the check's first request is the
      brief, then the objective, the acceptance and the claim, in order. The maker's brief, its call, and the words
      its tool read do not appear. The check session has exactly one edge into the maker's session, to the report,
      via `claim`.
    - `the_basis_is_recorded_and_shown`: the session record, the row, the call's result and meta, `task.list`,
      and the report's post body and node text.
    - `a_copied_span_of_twelve_words_raises_the_flag_and_eleven_does_not`: the span, its node and its length.
    - `a_check_of_a_task_with_no_report_is_refused`: a waiting task, a cancelled one, an unknown name, and
      `profile` without `check_of`.
    - `the_exclusion_holds_on_the_checks_own_pieces`: the relayed report becomes the claim, and a named profile is
      what the check runs on. A parent node with a `derived_from` edge into the maker's tool result is refused as
      `excluded`, and so is an unknown profile.
  - `check::tests` (12 against 11 words, case and punctuation, run lengths; word splitting; the claim's render).
  - `theseus-protocol` `check::tests` (the line).
  - The CLI's `render::tasks::tests::a_checks_basis_is_shown_under_it`.
  - The Discord report test (the line beside the report).
  - The cockpit's `test/check.test.ts`.
- **Focused run.** `cargo nextest run -p theseus-core -p theseus-protocol -p theseus -p theseus-discord -E 'test(check) |
  test(layout) | test(arrangement) | test(tests_tasks) | test(task_wakes) | test(generated) | test(registry) |
  test(a_tasks_report)'`: 72 tests, all passed (this includes `every_old_layout_on_disk_still_reads` and
  `tests_registry`).
- **Under load.** Four busy loops at nice 0, and the test at `nice -n 19` (test binary prebuilt).
  `-E 'test(tests_check) | test(tests_arrangement) | test(check::)'` ran five times, 14/14 each run (about 40 s
  each).
- **Planted revert 1 (the exclusion).** In `check::prepare`, I appended the checked session's last tool result to
  the claim's text. `a_checks_compilation_holds_nothing_of_the_checked_session_but_its_claim` failed with
  `"forty two herring gulls" reached the check`. The other 4 passed. I restored the file, ran `touch`, and checked
  `git status`.
- **Planted revert 2 (the flag).** I set `OVERLAP_WORDS = 13`. Both `check::tests::a_span_of_twelve_words_is_flagged_and_eleven_is_not`
  and `tests_check::a_copied_span_of_twelve_words_raises_the_flag_and_eleven_does_not` failed (`left: 0`, no flag).
  I restored the file, ran `touch`, and confirmed the constant is 12 again.
- **Cockpit.** `npm run lint` shows the existing warnings only; `npm test` 38 passed; `tsc -b` is clean.

## The live check (the maintainer's)

On a scratch daemon with a fresh state dir. Its config, from `theseusd example-config`, has `[discord]` and
`[web]` off, a GLM profile `glm`, and, if available, a second profile (`glm-b` below, any other model).

```sh
S=$(mktemp -d /tmp/theseus-28a.XXXX); mkdir -p $S/state $S/work
theseusd example-config > $S/theseus.toml     # edit: [discord]/[web] off, profiles glm (+ glm-b), projects_dir = $S/work
printf 'the keeper logs the tide at dawn and dusk every day\n' > $S/work/words.txt   # 11 words
theseusd --config $S/theseus.toml --socket $S/sock --state-dir $S/state & echo $! > $S/pid
T="theseus --socket $S/sock"
# 1. The maker.
$T ask -P glm "Count the words in $S/work/words.txt and report the number. Do it as a background task, quoting this message as its objective."
SID=<the session id the ask printed>
$T tasks                                  # wait until the task is complete
$T ask -s $SID "What did the task find?"  # the parent reads the report
# 2. The check (on the second profile, if configured).
$T ask -s $SID "Start a check of that task (check_of its id), with profile glm-b, briefed only with the goal and its claim."
$T tasks                                  # the check's id: CHK
$T rpc compilation.list '{"session_id": "<CHK>"}'
```

- **Step 2.** `compilation.list` should show, in the first user message: the brief, then the maker's objective
  piece, then `[Check: …]` and `--- Claim: claimed by task <short>, as of … UTC ---` with the maker's report.
  Nothing else of the maker's session should be there: no `fs.read` call or its result, and not the maker's brief.
  The task graph's view block also appears (see "What I found").
- **Step 3.** `theseus tasks` should show the line
  `🔍 check of task <maker short> · independent (excluded ses_…<maker short>, <glm-b's model>)` under the check.
  Once the check finishes, `$T ask -s $SID "And the check?"` shows its report node with the same 🔍 line after
  its head. `$T rpc ledger.tail '{"n": 50}'` should hold a `task.check_opened` row with the basis.
- **Step 4.** Ask for a task that waits (for example, "start a task that sets a wake in an hour, then counts"),
  then at once "start a check of it". The call's result should read `Not started: task <short> is <running|waiting>
  and has not reported…`, and the ledger should hold a `task.check_refused` row with class `no_report`.
- **Stop.** `$T shutdown`, then wait for the pid in `$S/pid` to exit.

## What is left, or uncertain

- **The task graph's view** reaches a check (see "What I found"). It is not part of the exclusion as built.
- **What the exclusion follows.** It walks `derived_from` edges only. A parent reply that paraphrases the maker's
  working without an edge is caught only by the overlap flag, and only when it copies 12 words or more. This is
  as the design intends; it is said here so the owner hears it.
- **Spans that cross text boundaries.** `working_text` joins a node's text parts with newlines, so a 12-word window
  can span two JSON strings of one tool input. It can flag a little more, never less.
- **The cockpit's time machine** does not fold the check basis for a past moment; the basis shows for the present
  only. No `timemachine.ts` step was added.
- **Docs for the maintainer to write:**
  - the spec's Part III item for 28a;
  - `docs/status.md`'s row;
  - `docs/design/m5-judgment.md` §2.11 and §2.14. The basis is on `TaskOf.check` at format 13, not a SESSION
    schema 3 → 4; the claim lives on the `Arrangement` node; `profile` is a check's alone; the fidelity check is
    skipped for checks; the Observatory is now the cockpit's task view.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on 20dadc1's tree:

- **Phases that passed:** fmt, shape, features, clippy, cockpit, test build, and the reader rule.
- **Suite:** 2240 run, 2205 passed, 35 failed, 17 skipped. Every failure is environmental:
  - **34 sandbox tests:** `theseus-sandbox::contract` clauses and `theseusd::sandbox`. They are the root-VM L1 case
    (theseus-pv6i).
  - **`theseus-core tests_output::the_cores_output_matches_its_golden`:** the golden carries a negative UTC offset
    (`-#:#`) in a wake's time, and this VM runs at `+00:00`. The test passes with `TZ=America/Los_Angeles`.
    Nothing in this step touches it.
- **An earlier gate run** also failed about 25 `theseusd` job, task and continuation tests. The disk had filled
  (ENOSPC). After I cleared `target/debug/incremental`, those binaries (tasks, continuations, push, stops,
  outbox, reaping, job_approval, mcp_server, versions) passed alone: 36 of 36.
- **The phases after the suite**, which I ran myself: protocol types ok (`protocol.gen` staged), and deny ok
  (advisories, bans, licenses, sources; `cargo deny fetch` succeeded). Benches are off under `THESEUS_GATE_NO_BENCH`.

**I count the commit green.**
