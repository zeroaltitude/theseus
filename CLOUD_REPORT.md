# Cloud report: self-view (theseus-pw1q.19)

Branch `cloud/20261013-self-view`. Three commits on the task commit: the self view and the Discord words, the
`self.changed` notification, and this report.

## Step 1: the TUI's self view and Discord's `self digest` / `self log` (5b8cac1)

**Found.** `self.log` and `self.digest` already carry everything: rows, the switch's state, the budget view. Nothing
told a surface when a row landed except the events a surface already holds. Discord's `runtime.rs` was at 3,500 of 3,500
lines after the arm, and `control` was at clippy's complexity limit (26 of 25 with one more arm).

**Changed.**
- TUI: `crates/theseus-tui/src/selfview.rs` (+ `Mode::SelfView`, key `l`, `Purpose::SelfLog`, footer `[l] self`, a help
  line, a pane drawn over the body). Top to bottom: the switch's state (`render::self_state_line`); "Self-improvement is
  off. Nothing has run." when the mode is off; the day's line and the newest branch's (`SelfLogResult.budget`, wide
  screens only, absent while off); the newest 20 rows, one line each (time, kind, what, cost, undo); enter shows a row
  whole, esc goes back, then closes. Under 100 columns it folds to the state and the newest five. It reads `self.log` on
  open, on reconnect, on `events.lost`, and on the events the TUI already hears (an execution that changed state, a task,
  a question asked or answered), one read at a time (a second wanted meanwhile is made when the first lands). No timer.
- Discord: `runtime/self_word.rs`. A message of exactly `self digest` or `self log` (any case): in a place the place rule
  calls private it posts `self.digest`'s text through the weekly post's own cut (`rsi::digest_capped`) or the last ten
  rows (`rsi::digest_line`); anywhere else it says "The self log is private." and nothing more. Mode off: both begin
  "Self-improvement is off. Nothing has run." and still give the rows. The halt, resume and word arms share one arm of
  `control` (`self_control`), so `runtime.rs` stays at 3,500 and clippy's complexity holds.
- Core: `digest.rs`'s `capped` and `line` are `pub`, re-exported as `rsi::digest_capped` and `rsi::digest_line` (one new
  `pub use` line in `rsi/mod.rs`).
- AGENTS.md of the TUI and Discord crates.

**Proved.** TUI `tests_self.rs` (7 tests, then 8 with step 2): the pane at 120 and 70 columns against a fixed log (snapshot
strings), the fold, off, enter/esc, the live append, an older daemon (`self.log` unknown, said in the pane), and
`nothing_reads_the_log_on_a_timer_and_a_burst_is_two_reads` (tokio's paused clock, the loop stepped through ten minutes of
deadlines). Discord `tests_gateway/self_word.rs` (4) + a unit test: the DM gets the weekly post's text exactly
(`digest_capped(self_digest().text)`), `self log` gives the newest ten in order, a shared channel gets the refusal alone
(two words, both refused, no row text), mode off. `cargo nextest run -p theseus-tui`: 78 passed.
**Planted reverts.** (a) a 5 s deadline in `App::deadline` plus a read in `tick` while the pane is open: the first draft
of the no-timer test did NOT catch it (the rig's `settle` cuts every step at 50 ms, so a 5 s deadline never fired); I
rewrote the test to step the loop through the deadlines, and it then failed with "no timer reads the log"; file restored,
`touch`ed. (b) the place-class check switched off (`if false && …`): `a_shared_channel_is_told_the_log_is_private…` timed
out (the refusal never came; the digest was posted); restored and `touch`ed.

## Step 2: `self.changed` (e93c9a4)

**Found (my own addition, not in the brief).** A halt or resume from another surface writes no execution, so with only
the events the TUI held, the pane showed a stale switch until the next execution moved. I saw it in the live check of
step 1 before this step.

**Changed.** A new notification `self.changed { kind, at_ms }` (`notify::SELF_CHANGED`, `Event::SelfChanged`,
`rsi::SelfChanged`, the cockpit's generated `SelfChanged.ts`), sent once after the switch's own frame is written
(`Core::self_changed`, in `rsi/switch.rs`, two calls), to the connections that watch every session only
(`Bus::publish_watchers`, in the bus's `wide` set). No frame, no ledger row, no store format change. The TUI's
`moves` includes it. Other self rows (a night's exam pass, sealed puts) still show at the next event or on reopening: I
did not add a call at each of their sites. If the owner wants those live, each site is one line (`self.self_changed(kind)`
beside its `announce`), but `self_changed` is private to `rsi/switch.rs` today.

**Proved.** Core `rpc::tests_self::the_switchs_move_is_told_to_the_overview_watchers_once` (the overview watcher hears one
`self.changed` per move, a session's watcher none, a halt that changes nothing none); TUI
`a_move_of_the_switch_from_another_surface_shows_without_a_key`. `tests_registry::every_notification_has_its_event_and_a_sender`
passes; protocol tests 87 passed (the generated TS is staged). **Planted reverts:** the halt's `self_changed` call
removed: the core test FAILS; `Event::SelfChanged` out of `moves`: the TUI test times out ("timed out waiting for 2
self.log request(s)"); both restored and `touch`ed.

## Live check

Done on this VM with a scratch `theseusd` (own config, socket and state dir, `[self] mode = "act"`, a stand-in key file):
rows planted through the real fact path (`theseus self halt "…"` and `self resume`, which write `self.halted` and
`self.resumed`), then `theseus-tui --socket /tmp/live/sock` in tmux at 120x24: `l` showed the state, the budget line and the
rows; a `theseus self halt` from the CLI with the pane open updated the pane within 1.5 s with no key pressed; enter showed
the row whole (what, why, undo, position). The 70-column fold is held by the unit test; my tmux resize sequence did not
reopen the pane, so it is not a live observation. The Discord half is checked against the fake Discord (the gateway tests
above: DM, shared channel, off); no live bot was used.

Commands for the maintainer: start a scratch daemon, plant rows with `theseus --socket S self halt why; self resume`, run
`theseus-tui --socket S`, press `l`; from another shell `theseus --socket S self halt again`: the top row appears at once.
In a bound DM type `self digest` and `self log`; in a shared channel the same words get "The self log is private."

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the final tree: door, keel, keel tests, fmt, shape, features,
clippy, cockpit, test build: ok. Suite: 4,487 run, 4,453 passed, 34 failed: the 33 known L1 failures (theseus-sandbox's
contract tests and `spawn_100`, theseusd's `sandbox` tests: root daemon, theseus-pv6i) and one more: **theseus-discord
`tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`**, a timing test of the thinking-answer
scenario (it needs its process message's create to be held at the fake before the reply's post). Same code, run alone:
PASS, PASS (stashed tree), FAIL x3, then PASS, FAIL, PASS; the whole `tests_process` file passed. It is host-speed
dependent and not in the known list: **name it for the owner** (it joined in b495389). Later phases run by hand:
protocol types ok (the generated TS staged), `cargo deny --offline check` ok, `theseus-sim bench turn --check --runs 5
--burst 0` ok (frames and syncs within budget), `theseus-sim bench lifecycle --runs 10 --check`: every start, shutdown,
SIGKILL, swap and seed budget ok; only "health, after the seed" missed (p50 5.8 ms against 5 ms, a 4-core VM; I touch no
start path). The jobs bench is the L1 one and skipped. `python3 scripts/keel-guard.py`: ok, 0 findings (its renamed-test
note is from main, not this branch).

## Left, uncertain, and for the owner

- No `/self digest` slash command: `runtime.rs` has no room (3,500 of 3,500). It would go in a module of its own with
  the command list moved out first.
- A shared place's `self digest`/`self log` is intercepted as a control, not sent to the model (as `halt self` is). Exactly
  two words, so "self log is broken" is an ordinary message.
- `self log`'s rows carry `numbers` as raw JSON in Discord (the digest's `line`); the TUI shows only the cost.
- The `self.changed` notification is a wire addition (protocol docs updated in `AGENTS.md`); no store change.
- A disk allowance of about 40 GB was spent by debug builds on this VM; I freed stale duplicate artifacts in
  `target/debug/deps` and built with `CARGO_INCREMENTAL=0` for the rest. Nothing outside the repo and /tmp was touched.

**Keel findings expected:** none.
