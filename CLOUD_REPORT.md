# CLOUD REPORT: approvals-batch (theseus-6i0, theseus-w6uh)

Branch `cloud/20261005-approvals-batch`, cloned at `4a44946` (store format 22, unchanged here). Started 03:06 UTC and
finished 04:30 UTC on 2026-10-06. Two steps, each one commit, each gated and pushed.

| step | commit | what |
|---|---|---|
| 1 | `b009fa0` | toolrun: a declined call ends its batch's waits (theseus-6i0) |
| 2 | `995ad50` | toolrun: a call is found by its own response, whatever an earlier one's ids (theseus-w6uh) |

No new dependency, no store format bump, no protocol change (`cockpit/src/protocol.gen` untouched), and no new
config key.

## Where the brief and the code differ

- **"A write outside the roots waits" is not true at the template's postures.** Since theseus-ewi (policy.rs,
  `a_write_outside_the_roots_takes_the_posture_whichever_tool_makes_it`), a write outside the roots takes the tool's
  posture: under the template's `enforcement = "notify"` it runs with a notice. So the tests make writes wait through
  `[tools] approve_paths`: one guarded directory outside the roots. The live check below needs the same, or
  `[policy] enforcement = "approve"`.
- **The two lookups by bare id disagreed with each other.** resume.rs's map kept the *last* ToolCall node with an
  id, and late.rs's cancel lookup (`find`) took the *first*. Both were wrong for a repeated id.
- **The issue's own case already passed on main as cloned** (the stubs step's `unanswered` reads only the results
  after the last assistant message, and the map's "last node" happened to be the right one). The case main missed
  is a call that was never planned (it comes after a waiting call) and whose id repeats an earlier response's
  answered call. The cancel path is wrong in the same case.
- **theseus-sim's stand-in no longer numbers per response.** `calls_turn` makes each id unique across turns and
  restarts (`toolu_fake_{ms}{count}_{i}`, since 39b). Live check 2 cannot reproduce repeated ids with the stand-in as
  cloned; see that section. I left fake_model.rs alone, as the brief says.

## Step 1: a decline ends the batch's waits (theseus-6i0), `b009fa0`

**Found.** `run_calls` stops admitting at the first call that asks. The continuation answered a declined call
(`Pending::Cancelled`, then `answer_cancelled`, giving `Declined`) and pushed the rest, all `NeverPlanned`, through
`run_fresh` and `run_calls`, which parked on the next waiting call's card. So N waiting calls meant N cards in
sequence, and the model heard the decline only after the last one.

**Changed.**
- `run_calls` moved whole to a new module, `toolrun/calls.rs` (toolrun.rs went from 2,494 to 2,418 lines). It is
  now a wrapper, `run_batch(.., declined: false)`. With `declined` set, an `Admitted::Asks` call is not asked. It
  goes to `not_asked`, and the gating continues, so calls that need no answer run as before. Only the call sites
  changed in toolrun.rs, plus one unused import.
- `ToolRuntime::resume` sets `declined` once any call of the batch is answered `Done { Declined }`, and passes it
  to `run_fresh` and then `run_batch`. What sets it:
  - an operator's decline;
  - an expired card (`expired_answer` answers `Declined`);
  - `run_confirmed`'s "confirmation is no longer valid", which is also `Declined`.

  A supersede also answers `Declined`, but with input every `NeverPlanned` call is already answered not-run.
- **The not-asked answer.** The text is `Not run: an earlier call in this batch was declined.`, with status
  `Cancelled`, because `Declined` would say the operator declined it, and nobody was asked. Meta is
  `{"not_run": "an earlier call in this batch was declined"}`. No new `ResultStatus`, so no store format bump.
- **What a not-asked call leaves behind:**
  - its `ToolCall` node with the gate's record (`gate: needs_confirm`) and `correlation_id: None`, as an invalid
    input's node has;
  - its result, written in one append with the node;
  - a narrative line from a new fact, `fact::tool::CallNotAsked` (in `FACTS`; narrative only, no ledger row);
  - the in-memory per-tool counter (`count`), which ticks at admission as for every admitted call.

  No action is planned, so nothing waits in the kernel, no `tool.confirm_requested` row is written, and no card is
  posted.
- **Counted and traced once, at its answer.** Its `Ran` goes through `ran_batch`, so each call is one span under
  the continuation (the test checks spans by `tool_use_id` and `result`).
- **Ordering detail.** The not-asked result is written at admission, as an invalid input's is, so it is stored
  *before* a later-indexed read's result, and its span comes first. The model's request is still in call order
  (the compiler orders results by the call). The tests assert the request's order exactly, and sort the stored
  results and spans by call.

**Proved.**
- New tests in `tests_approvals.rs` (a file of its own, `mod` line in lib.rs):
  - `a_declined_call_ends_its_batchs_waits` runs one response of [write to guarded/a.txt, read of note.txt inside
    the root, write to guarded/b.txt] and declines the first with the note "wrong place". It checks:
    - the continuation's output is the next scripted reply, and `awaiting_confirm` is None;
    - one `tool.confirm_requested` row in all, `kernel.pending_confirms()` and the session's pending confirms are
      empty, and neither file exists;
    - the model's next request holds three `tool_result`s in order `w1, r1, w2`: the decline with "wrong place",
      the note's content, and the exact not-run line;
    - the statuses are `Declined, Ok, Cancelled`;
    - w2's node has no correlation id, and its gate record is `needs_confirm`;
    - there are three `tool *` spans, each under `continuation`, with results `declined/ok/cancelled`.
  - `an_approved_call_leaves_the_next_to_ask`: the first write is approved, the second still asks (a second, distinct
    card; 2 rows), is approved, and runs; the request holds `w1, r1, w2`.
- **Planted revert**: the rule switched off (`Admitted::Asks(_, g) if false && declined`). The first test failed as
  the bug does: `assertion left == right failed: left: "" right: "Understood: neither file."` (the continuation parked
  on a second card). The second test still passed. The file was restored and `touch`ed, and `git status` was clean
  of it.
- Related suites: `tests_m3 | tests_tasks | tests_glide | tests_resumed | confirm | resume | late | tests_approvals |
  tests_output | tests_registry | tests_continuations | tests_cancel`: 204 passed, 0 failed. The output golden was
  unchanged by step 1.
- Under load (4 busy loops at nice 0, the tests at nice 19): 5 of 5 runs green.
- Gate: see "The gate" below.

**Not built: one card for a whole batch** (the owner's later choice). It would need:
- a card shape that lists every waiting call of a response (tool, input, reason each), with Discord's render and
  the cockpit's;
- a confirm protocol whose answer names a batch, or a list of correlation ids, with one answer bound to several
  actions: each action's proposal digest bound in one kernel frame, all or none;
- a way to decline one call and approve the others (or not);
- the calls after the first planned at the first ask instead of left unadmitted;
- expiry and supersede applied to the set;
- the place rule, the floor, and T1's hold still decided per call, so a batch card could not approve a floor call
  by riding with others.

## Step 2: a call is found by its own response (theseus-w6uh), `995ad50`

**Found.** I wrote the tests first and ran them on step 1's tree, before the fix:
- the issue's case (two responses' first calls share `toolu_fake_0`, the second's write waits, and is approved)
  **passed**;
- [a waiting write `toolu_fake_0`, then a read `toolu_fake_1`, the id of the first response's answered read]
  **failed**. After approval the read was answered with the old call's settled action, never run:
  `("toolu_fake_1", "The call settled but its output was lost in a restart. Check the current state before relying
  on it.")`;
- the cancel of that execution **failed**. Both new calls were answered `Unknown` from the first response's actions:
  `"The execution was cancelled by operator. This call settled as succeeded and its result was never recorded…"`.

**Changed.**
- `toolrun/resume.rs::calls_of(nodes, assistant_id)` builds a map by `tool_use_id` of the ToolCall nodes whose
  `assistant_node` is the last response's. It reads only the nodes after that response (its calls are written after
  it), so it decodes less than before.
- resume's map and late.rs's `cancelled_results` lookup both use it. No stored field changes: every ToolCall node
  already carries `assistant_node`.
- **Why key rather than refuse.** Refusing a repeated id as the response arrives would break the stand-in as it
  was, and any compatible proxy that numbers per response, for no gain. An id is unique within its response, which
  is all a turn needs. The provider's API accepts results matched to the immediately preceding message.
- **Golden.** 37 lines of `tests/golden/core_output.txt` moved, all of them `context.compiled` (ledger, notify,
  compile span). A script compared each pair: each only gains `"stubs":#`, with 0 other differences. The
  continuation used to decode every ToolCall node of the session, so later compiles found them decoded. They now
  find stubs.

**Proved.**
- New tests in `tests_approvals.rs`:
  - `an_approved_call_whose_id_an_earlier_response_used_runs` (the issue's case: the file is written, and the next
    request holds one result, its own);
  - `a_never_planned_call_whose_id_an_earlier_response_used_runs_anew` (the note is rewritten between turns, and
    the read's result is the new content; all four results `Ok`);
  - `a_cancel_answers_the_last_responses_calls_whatever_their_ids` (both new calls `Cancelled` with "Not run: …";
    the write's result names its own correlation id, and the read's names none; the write's action is `Cancelled`).
- **Planted revert**: `calls_of` keyed by bare id across the whole session again (the guard became `|| true`, the
  slice started at 0). The never-planned test failed with
  `[("toolu_fake_0", "Created …/guarded/c.txt (5 bytes)."), ("toolu_fake_1", "The call settled but its output was
  lost in a restart. …")]`, and the cancel test with `left: Unknown right: Cancelled`. The issue's own case passed
  under the plant, as on main. The file was restored and `touch`ed, and `git status` was clean of it.
- The same related suites: 191 passed and 1 failed, the golden, before I rewrote it (above). Afterwards it passed
  four times in a row.
- Under load: 5 of 5 runs of the 5 tests green.

**judge/gate.rs (report only, not changed).** It pairs by bare id too, and needs the same kind of fix:
- `last_calls` leaves out *every* earlier call that shares the current call's id (`tool_use_id != this`), and gives
  each listed call the outcome of the *newest* result with its id (`rev().find_map`), which can be a later call's;
- `recent_reads` takes the input (path or URL) of the *first* ToolCall with a result's id, which can be an earlier
  call's.

It is shadow input to security.v1/v3 only, so the effect is misattributed context for Jev, never a gate decision.
Pairing a result with its call by `correlation_id` (both nodes carry it when planned) or by position after the
assistant node would fix it.

## The live check (the maintainer's)

Use a scratch daemon on a fresh state dir: Discord and the web off, `[tools] roots` a scratch dir, `[tools]
approve_paths = ["<scratch>/guarded"]` (or `[policy] enforcement = "approve"`; see the first difference above), and
the stand-in model as its `api_base`, `theseus-sim fake-model --rules rules.json`.

1. **A decline ends the batch.** rules.json:
   ```json
   [{"when": "WRITE-BOTH", "calls": [
       {"name": "fs_write", "input": {"path": "<scratch>/guarded/a.txt", "content": "a\n"}},
       {"name": "fs_read",  "input": {"path": "<scratch>/roots/note.txt"}},
       {"name": "fs_write", "input": {"path": "<scratch>/guarded/b.txt", "content": "b\n"}}]}]
   ```
   - Run `theseus --socket <sock> ask "WRITE-BOTH"`. It parks on one card.
   - `theseus --socket <sock> confirm` (no id) lists one entry.
   - Run `theseus --socket <sock> confirm <id> --decline --note "wrong place"`, then `confirm` (no id) again: it is
     empty, with **no second card**.
   - `theseus --socket <sock> history <session>` shows the decline with "wrong place", the read's content, and
     `Not run: an earlier call in this batch was declined.`, then the stand-in's reply (`Done.`).
   - Neither file exists.
   - The narrative has `fs.write: not asked, since an earlier call in its batch was declined; it did not run.`
2. **Repeated ids.** The stand-in as cloned makes every id unique (since 39b), so it cannot show this. To see it
   live, run a stand-in built with `calls_turn`'s id as `toolu_fake_{i}` (a local edit, not for main; crash-hold
   owns fake_model.rs). Then, in one session:
   - a turn whose call runs (`fs_read`);
   - a turn whose `fs_write` to `guarded/` waits, then `confirm <id>`: the file is written, and history shows the
     call's own `Created …` result;
   - optionally, a turn of [`fs_write` guarded, `fs_read`] whose read repeats `toolu_fake_1`: after the confirm,
     the read's result is the file's current content.

   Without that build, the core tests above are the proof.

## Left open, and choices the owner should hear

- **Trigger.** "Declined" here means any call answered `Declined` in the continuation, which includes an expired card
  and a confirmation that no longer holds. If the owner wants only the operator's own no to end the batch, the
  trigger narrows to `a.declined_note().is_some()`.
- **Discord history.** A not-asked call's ToolCall node has a `needs_confirm` gate record and no action.
  Discord's and the cockpit's history renders of a tool_call node read the gate. Worth a glance in the live check:
  it should read as a call followed by its not-run result, never as a pending card.
- **Docs to update** (the maintainer's):
  - theseus-core/AGENTS.md's "Tool calls" bullet should name `toolrun/calls.rs` (`run_calls`/`run_batch`, the
    decline rule), `resume::calls_of`'s keying by `assistant_node`, and `tests_approvals.rs`;
  - Part III's item for this step;
  - status.md.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run on each commit's tree.

**Step 1** (`/tmp/gate1.log`): fmt, shape, features, clippy, cockpit, test build and reader rule passed. The suite:
2,832 run, 2,798 passed, 34 failed.
- 33 failures are the known L1 set (theseus-sandbox contract and `spawn_100`, theseusd sandbox), root without a job
  cgroup (theseus-pv6i).
- The 34th is **not on the known list**: `theseus-kernel::children
  a_sweep_reaps_wrappers_and_orphans_and_never_an_owned_child`, FAIL + LEAK, at `tests/children.rs:226`:
  `left: Relearned { wrappers: 1, tenders: 0, orphans: 2, zombies: 0 }`,
  `right: Relearned { wrappers: 1, tenders: 1, orphans: 1, zombies: 0 }`. Under the gate's load, the sweep classed
  a tender as an orphan.
  - It passed alone 3 times out of 3. Nothing here touches theseus-kernel.
  - Since it is a classification count (a tender taken for an orphan: one the sweep would reap), I report it as a
    **finding**, not a flake.

**Step 2:**
- A first run failed about 100 job tests, all with "the disk under the state dir has 771 MB free, below the floor
  of 1,024 MB, so the job was not started". The VM's disk allowance was spent, 17 GB of it by
  `target/debug/incremental`. I deleted that directory (in-repo build output) and reran with `CARGO_INCREMENTAL=0`.
- The rerun (`/tmp/gate3.log`): every phase before the suite passed. Suite: 2,835 run, **2,802 passed, 33 failed:
  exactly the known L1 set**, nothing else (the kernel test above passed this time).
- The phases after the suite, run by hand: protocol types clean, no "compiled under the lock" note, and
  `cargo deny --offline check`: advisories, bans, licenses and sources ok.
- The benches were skipped (`THESEUS_GATE_NO_BENCH=1`), as the brief says.
