# CLOUD_REPORT: a Resume button on the self-build halt note (theseus-6s7ll)

Branch `cloud/20261027-p3x-self-halt-resume`. Code commit `f910fc38`. The report is the last commit.

## Step 0: what landed, and the citations

- At my start `origin/main` was still `5342a01c`. By the time I ran the keel guard it had moved to `a20350c4`: the
  p3w-rsi integration, fanout-cue, jrffh, and not-this-docs. `git merge-tree HEAD origin/main` merges clean. The
  later main touches `rsi/pr.rs` (+31 lines) and `rsi/tree.rs` (+66 lines), so my hunks there may need a look at join.
- Citations. All were right unless listed:
  - `rsi/pr.rs:165-171` is the doc comment at 165, `fn notify` at 166, and `"quiet": true` at 171. Correct.
  - `self_note.rs:21` (docstring), `:233-240` (parse tests) and `:236` (`parse("snote:halt")`). Correct.
  - `tests_gateway/self_note.rs:110-113`, `:132` and `:267`. Correct.
  - **Wrong or loose, one:** `:132` (`ids == ["snote:halt"]`) is the **`self_join` note's** test, not the halt-build note. It has no
    reason to change, because the self-build halt note is a different post. I extended nothing there and weakened
    nothing. The new halt note has its own test.
  - **Loose, one:** the brief says the halt notice "reuses the sink" and is a "quiet notice". True, but it is the
    plain `kind: "notice"`. See the finding below.
- Where `resume self` lives: `crates/theseus-discord/src/runtime/halt.rs` (`resume_self`, the `SELF_RESUME` rpc). The core side
  is `rpc/rsi.rs` (`method::SELF_RESUME`) into `self_resume` and `judge_act`'s `Act::SelfResume` (`rpc/confirms.rs:723-735`).
  **Reused:** I moved the rpc call out of `Place::resume_self` into `Shared::resume_self_words` (halt.rs). The typed words and
  the button's press both call it, so there is one implementation.

## Finding (not fixed, owner should hear it)

A plain `kind: "notice"` staged with `stage_to_operator(None, …)` is **not delivered** in the gateway rig. The courier's `"notice"`
arm uses `place_channel()`, and the operator lane has no `dm_user`, so it never lands in the owner's DM. My first guard test staged
a quiet `notice` and timed out waiting for it. Whether production's operator lane differs, I could not tell. If it does not, the
existing halt and may-build notices were never delivered on Discord. The new `self_halt` kind goes through `jev_post` and
`operator_channel`, which works (tested end to end). So the new kind fixes delivery as well as ping and button.

## What I changed (`f910fc38`)

- **Core, `rsi/pr.rs`:** `notice_body(halt, text)` is a new pure function. `halt=false` gives the old quiet notice. `halt=true`
  gives `{"kind":"self_halt","text":…}` with no `quiet`.
  - The exact hunk is the body of `Board::notify`. `notify()` builds `stage(halt, text)` and gives `Notify::new(..)` the
    informational path (quiet, unchanged) and `.with_halt(..)` the halt path.
  - **How the exception is scoped:** `Notify` (`rsi/tree.rs`) gets an optional second sink, `with_halt`, and `send_halt`.
    Only `infra.rs::tell` calls `send_halt`. The two other callers (the door's fetch notices, which use `n.0` directly)
    are untouched and stay quiet. A `Notify` with no halt sink (all test sinks) sends the halt through its plain sink,
    so existing tests are unchanged. I removed `Notify::send`, whose only caller was `tell`.
- **Discord:**
  - `courier.rs`: a `"self_halt"` arm in the post dispatch (to `jev_post`), and a button arm giving `Buttons::SelfResume`
    at both component sites.
  - `render.rs`: the `Buttons::SelfResume` variant, 2 lines.
  - `runtime/self_note.rs`: `Press::Resume`, the `snote:resume` parse arm, `resume_buttons()` (one row, one Success button,
    label "Resume self-improvement"), and the press arm. The press calls `resume_self_words`.
  - The ping needs no code. `jev_post` writes with `pings(Event::Jev)`, so a post with no `quiet` pings by default. I
    asserted it (`!m.silent()`).
- The button is on the `self_halt` note only. The digest and join notes still get Drop and Halt, with no Resume.
- **Second press:** it calls the same `self.resume`. The core answers `changed:false`, and the presser sees "▶️ Self-improvement was
  not halted." Nothing errors, nothing double-resumes, and state is unchanged. A guest's press is refused by the core
  (owner only, `judge_act`) and tells the presser alone "🛑 …", with no state change. The same holds for typed "resume self".
- Protocol lines added: **0** (`theseus-protocol` untouched). `render.rs` went from 2986 to 2988 (ceiling 3009).
  `courier.rs` is 2268. I kept `too_many_lines` fulfilled by giving `self_halt` its own arm. A rustfmt reflow of the or-list had
  dropped the fn under 100 lines and tripped the unfulfilled `expect`.
- `dm_flow.txt`: `git diff --stat` empty. Untouched.

## Red-first

I wrote each test against **stubs** (the variant and functions existing, behaviour absent) so it fails for the right reason, not by
compile error. Base plus stubs gave:

- `rsi::tests_pr::infra::the_halt_notice_is_its_own_kind_and_does_not_ask_for_quiet`: FAIL, `left: "notice" right: "self_halt"`.
- `runtime::self_note::tests::items_come_out_of_the_started_lines_and_ids_parse_back`: FAIL, `left: None right: Some(Resume)`.
- `runtime::self_note::tests_resume::the_resume_note_has_one_button_on_the_id_that_parses_back`: FAIL, `left: 0 right: 1`.
- `tests_gateway::self_note::the_halt_note_pings_and_carries_one_resume_button`: FAIL, `left: [] right: [("Resume self-improvement","snote:resume")]`.
- `tests_gateway::self_note::a_resume_press_resumes_once_and_a_second_press_is_harmless`: FAIL, timed out at "ben is refused" (the press is not parsed).
- **Guards, not red-first (pass at base):**
  - `an_informational_self_notice_stays_quiet` (core: `kind:"notice"`, `quiet:true`).
  - `no_other_self_note_gets_a_resume` (digest and join carry no `snote:resume`).
  - `the_halt_takes_the_halt_sink_and_a_plain_sink_takes_it_too` (new API, so it has no base).

After the fix, 39 of 39 selected tests passed.

`cargo nextest list -p theseus-core -p theseus-discord -E '…'` selected all of the above, for example
`theseus-core rsi::tests_pr::infra::the_halt_notice_is_its_own_kind_and_does_not_ask_for_quiet` and
`theseus-discord tests_gateway::self_note::a_resume_press_resumes_once_and_a_second_press_is_harmless`. Four more are listed in the
gate's suite.

## Planted reverts

All files were restored from copies, then `touch`ed, and `git status` showed only my edits.

- (a) Delete the `"resume"` parse arm: 3 FAIL: `items_come_out_of…`, `the_resume_note_has_one_button…`, `a_resume_press_resumes_once…`.
- (b) `resume_buttons()` returns `vec![]`: 3 FAIL: `the_resume_note_has_one_button…`, `the_halt_note_pings_and_carries_one_resume_button`, `a_resume_press…`.
- (c) The halt branch of `notice_body` goes back to the quiet `notice`: FAIL on `the_halt_notice_is_its_own_kind…`. The guard
  `an_informational_self_notice_stays_quiet` stays PASS.
- Not planted: the `self_halt` courier arm. Without it the gateway tests would fail ("a post of kind … is not one this binding knows").

## Keel

`python3 scripts/keel-guard.py` gave `keel: ok (… 10 files changed, 0 findings acked)`. The first run, before I fetched `origin/main`, printed noise
from a stale base. **Keel findings expected: none.** I raised no cap or ceiling, deleted no test and added no `#[allow]`.

## Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`)

- door, keel, fmt, shape, recall tests, features and clippy passed. This took two reruns. The first clippy run caught my dead-code
  `send` method, and the second caught the unfulfilled `too_many_lines` expect.
- The suite phase: 8771 run, 8736 passed, 35 failed.
  - 34 are the known L1 set (theseus-sandbox and `theseusd::sandbox`, theseus-pv6i).
  - One was `theseus-core tests_route::a_switched_turns_compilation_names_its_recall_drops`. It passes alone (`1 passed`) and touches
    routing and recall, not this change, so I count it a load-ordering failure. It is outside the lists in the brief, and it did not reproduce.
- **Not run:** the phases after the suite (protocol types, lifecycle, jobs, turn and size benches). Benches are off. I changed no
  protocol type. `cargo deny` is not installed and `cargo deny fetch` failed, so the deny phase was skipped.
- Because the suite shows a failure outside the brief's lists, the commit is not strictly "green" under the rule in the brief. The
  maintainer should rerun it on the owner's machine.

## Live check for the maintainer

On a scratch daemon with `[self] mode = "act"` and a Discord rig, stage a `self_halt` post with
`theseus self halt`, then trigger two exit-127 self builds. Expect a DM that pings, carries the notice's words, and has one
"Resume self-improvement" button. Press it as the owner and expect "▶️ Self-improvement resumed." and
`theseus self status` showing not halted. Press it again and expect "was not halted.".

## Integrator notes

- **For main:** `f910fc38`.
- **Not for main:** the start commit (empty), `CLOUD_TASK.md`, and this report's commit.
- The hunks likely to conflict with the p3w-rsi merge are in `rsi/pr.rs` (the body of `notify` plus the new `notice_body`) and
  `rsi/tree.rs` (the `Notify` struct and impl). `merge-tree` was clean against `a20350c4`.
- No `OWNER CALL` is open. The delivery finding above is worth the owner's attention.

CLOUD REPORT COMPLETE
