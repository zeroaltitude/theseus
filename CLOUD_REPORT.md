# CLOUD_REPORT: context-meter (theseus-wy6v.2)

Branch `cloud/20261013-context-meter`. Code commit `ad0423ab`.

## What I found
- `context.explain` already had every part and its tokens, but only `theseus why` showed them. No client showed how full the context is.
- The compile's estimate (`Compiled.est_tokens`) is in hand at each loop, so the fill costs nothing extra to carry.
- The compaction trigger is the compiler's request limit (window − output cap − 4,096), and a clearing pass comes at 50 % of that limit. There is no "80 %" in the code. I made 80 % a named constant, `fill::COMPACT_WARN_PCT`, and the warning says **`compacts soon`**, not "compacts at 80%": the owner may revisit the number or the words.

## What I changed (one commit, `ad0423ab`)
- **The fill field.** `TurnSubmitResult` gains `context_tokens` and `context_window` (optional, serde default, absent from the bytes when unset). The frame is `turn.submit`'s result, the same value `turn.ended` pushes. No new frame and no store read. The core sets it from the last loop's compile (`turn/fill_step.rs`, 1 line in `compile_step.rs`, 6 in `turn.rs`, which has 28 lines left of its ceiling). The window is the working window if one is set, else the catalog's.
- **Words.** `theseus_protocol::fill`: `status_words` (`ctx 7%`, `ctx 83% · compacts soon`), `context_short` (the short form beside `why.rs`'s full one, reusing its `num`) and `TurnSubmitResult::context_words`.
- **Chat.** The status line segment (`ctx N%`, a field read, no RPC per redraw). `/context` is one `context.explain` call. `/cost` is one `health` call plus the turns this chat saw end. A resumed chat takes the share from `/context` or from its first turn.
- **Footer.** `status_line` in `render.rs` adds ` · ctx N%` after the cost, so `ask`, `watch` and `watch --interactive` show it, and so does the board TUI's detail pane (it draws the same line). The board TUI's own top header is global, not per session, so it shows nothing.
- **CLI.** `theseus context [SESSION]` (`--json` prints the answer as it came). It adds one match arm to `run()` in main.rs.
- **F10 arm.** `s10` opens the chat, types `/cost` then `/context`, captures, and records the two commands.
- **Also.** The generated TS for `TurnSubmitResult`. `render::day` made `pub` (one word) so `/cost` reuses the day-ceiling words. The `core_output.txt` golden moved only by the two new fields in `turn.ended` and the outbox lines that embed the result.

## How I proved it
- `nextest -p theseus-protocol fill`: 5 pass. They cover the share, the round trip with an old peer, three or more parts with amounts and the free space, and **the short form and `theseus why` agree on every part's tokens** (block by block).
- `nextest -p theseus-tui`: 151 pass, including 3 new tests. The share shows at 80 and 120 columns, with the warn band. `/context` lists the parts and the free space. `/cost` sums to the session's cost, and its pure test covers the 10 listed turns plus the rest in one line.
- `nextest -p theseus-core the_result_carries_the_last_requests_tokens_and_its_window`: passes, with and without a working window.
- **Planted reverts:**
  - The share computed against half the window failed 3 tests: `the_share_is_of_the_window_and_warns_past_the_band`, `the_short_form_names_the_parts…` and `the_pair_round_trips…`.
  - The conversation row dropped from the short form failed `the_short_form_names_the_parts…` and `the_short_form_and_why_agree_on_every_parts_tokens`.
  - The file was restored and touched, and `git status` was clean.
- `scripts/f10/test_f10.py`: OK.
- **Gate** (`THESEUS_GATE_NO_BENCH=1`, `THESEUS_KEEL_BASE=fcdef5e0`): keel, keel tests, fmt, shape, features, clippy, cockpit, test build and the reader rule passed. The suite ran 4,808 tests. Only the known L1 failures remained (theseus-sandbox contract and `spawn_100`, theseusd `sandbox::*`) plus `tests_output::the_cores_output_matches_its_golden`, which I then regenerated (diff read) and committed. **I did not re-run the whole gate after the golden update and did not run the phases after the suite by hand.** The maintainer's gate on the owner's machine must cover that.

## Keel findings expected
- The gate's keel phase failed on commits that came with the branch's base, not mine: `crates/theseus-sim/src/lifecycle.rs:155` ("seed" budget, an unsigned ack) and a renamed store test. This VM's `origin/main` is older than the branch point. I judged my range with `THESEUS_KEEL_BASE=fcdef5e0`, where the keel passes. None of my commits touches a test, budget, cap or ceiling.
- `python3 scripts/keel-guard.py --base fcdef5e0`: no findings.

## Disk (a trap on this VM)
The writable allowance (about 39 GB) filled twice during the debug build and the gate. It broke a link with a bus error and made the first suite run fail 171 tests. I freed it by deleting `target/debug/incremental` and stale duplicate artifacts, then reran. Anyone building here should use `CARGO_INCREMENTAL=0`.

## F10 (debug build, stand-in model, both sizes)
`python3 scripts/f10/driver.py theseus --bin-dir target/debug --out /tmp/f10`. The timing checks (S1b, B1) don't count on a debug build.
- **Match: 28 of 42 at both sizes**, against main's 11 in the 2026-10-12 live run. That run predates the chat and other joined rows, so the rise is not all mine. I did not run main's build through the driver.
- **This row:** S10a (`20,413 of 1,000,000 tokens (2.0%)`), S10b (`this conversation: $0.5681`) and S10c (system, tools, messages, free) all pass, and they failed or lacked evidence before.
- Still no on this build (not this row's): S2d, S5c, S6b, S7a, S7b, S9a to S9c, S11b to S11d, S12a to S12c.
- Per-step table: `/tmp/f10/scorecard-theseus.md` (not committed). Beats B1 to B5 all yes.

## Live check for the maintainer
```
theseusd --config <scratch> --socket /tmp/s.sock --state-dir /tmp/state &
theseus --socket /tmp/s.sock context          # short form: total of window, five rows, free space
theseus --socket /tmp/s.sock chat             # send a message: the status line gains `ctx N%`; /context, /cost
python3 scripts/f10/driver.py theseus --bin-dir target/release-thin --out /tmp/f10 && python3 scripts/f10/scorecard.py /tmp/f10/theseus-*
```

## Left or uncertain
- "The share read from the cache where the turn's usage records it" is not done. Turn usage is summed over loops, so a cache share from it would mislead, and `context.explain` carries no usage. The short form uses the compiler's estimate throughout (the same figures as `why`).
- The cockpit has no session-level place for the fill, so I added nothing there.
- `/cost` lists only turns this chat saw end. Earlier ones are summed into one line, so the lines always add up to the session's cost.
- Docs to update at review: the spec's Part III item, `docs/status.md`, and `crates/theseus-protocol/AGENTS.md` (a `fill.rs` entry), the CLI's AGENTS.md (`theseus context`) and the TUI's AGENTS.md (`/context`, `/cost`, `chat/usage.rs`).
