# CLOUD REPORT: tui-rows (theseus-k2vca)

Branch `cloud/20261013-tui-rows`. Commits: 16202d45 (the feature), ae39b343 (a session's work view survives `gone`).
Client only: `crates/theseus-tui` (new `work.rs`, `tests_work.rs`; edits to `board.rs`, `app.rs`, `ui.rs`, `main.rs`,
`tests.rs`, `AGENTS.md`). No dependency, no protocol type, no store format, no config key.

## What I found
- `work.watch` carries neither `confirm.requested`, `confirm.resolved` nor `wake.noticed` (the bus sends those to
  `executions.watch` and session watchers only). Dropping `executions.watch` would break reminders and whole
  questions with no daemon change, so **the executions stay the board's feed and `work.watch` (interactive: true) is
  an overlay that words the rows**. The brief said "instead of"; this is the divergence the owner should hear about.
- The work board prunes an ended task (`gone`) once its root has had a later event, while the TUI's row stays. The
  overlay therefore keeps a conversation's or task's view on `gone` (ae39b343).
- Live: the daemon's `report.first_line` was empty for a finished task in my run (`reason: "reported"` only); it
  comes from the task record's `report:` evidence (`push/work/views.rs`). So the live capture shows no report line;
  the rig test covers it. Daemon-side, not changed.
- Under 100 columns the list is full width; from 100 the sidebar was 36 columns, too narrow for a doing. With the work
  board it is two fifths (36 to 64) so 120 columns gives 48. Without it the old width stays (four existing tests
  depend on it).

## What changed
- Rows: `◐ Title  3/5 job 2:14 of usual 2:40 · said`, `model 0:31`, `fs.read 0:12`, `asks: <prompt>` (budget: its label),
  `✗ … failed 15:13: reason`, `✓ … done 14:13`; title cut first (to 8 columns), then the doing, the cost goes before
  either. `said` is `progress.said` (the job's `last_line` does not exist in the protocol yet).
- A finished task's first report line under its row, `◆` while done-until-seen (the shared seen file).
- Header `● 1 needs you · ◐ 3 working · ✗ 1 failed · ✓ 2 done`; title `theseus ●N` counting needs-you only
  (a failure is counted apart, in the header), plain `theseus` when none. The same in fallback mode.
- Elapsed repaints on the wall minute: `App::deadline` gains one minute deadline while any session shows a running time.
  The countdown on a card is unchanged (each second, only while the card is on screen). Display is `m:ss` as of the
  last repaint, so between pushes a time can lag up to a minute (the owner's "repaints on the minute").
- Fallback: `work.watch` refused or undecodable leaves `live` false; rows say their labels as before.
- A row's words are used only when the work view's level equals the board's level.

## Proof
- `cargo nextest run -p theseus-tui`: 78 pass (6 new in `tests_work.rs`, 1 unit test in `work.rs`, plus 5 existing
  header/title assertions rewritten, not removed). `-p theseus-tui -p theseus`: 430 pass. `cargo clippy --workspace
  --all-targets -D warnings`, `cargo fmt --check`, `scripts/shape.sh` clean.
- Planted reverts: (1) `App::deadline` minute -> per second: `elapsed_repaints_on_the_minute_and_a_countdown_each_second`
  failed (`left: 1790000001000, right: 1790000040000`). (2) title counting working tasks: existing
  `a_finished_task_is_done_counted_and_told_without_a_bell` failed (and my `…title_only_what_needs_you` asserts no `●4`).
  Both restored, files touched.
- Not done: no 200-row timing run (rows build with one pass over the views plus one jobs map; no per-frame reads).

## Live check (done on this VM)
Scratch daemon (bench profile + fake model, `[policy.tools] "fs.write" = "approve"`), TUI in tmux at 120x30:
```
 theseus · ● 1 needs you · ◐ 1 working                                              19:32 · socket ok
 ● paint t…  asks: fs.write create /tmp/live/ap…│ no session open: ...
 ◐ sound the north channel  job 0:32      $0.03 │
 ○ count the boats  ready                 $0.03 │
```
A task over its carve (failure):
```
 theseus · ● 1 needs you · ◐ 1 working · ✗ 1 failed                                  19:33 · socket ok
 ● paint t…  asks: fs.write create /tmp/live/ap…│
 ○ regatta: tally up the boats in the ha…  ready│
   └ ✗ Tally t…  failed 19:33: over_limit (the …│
 ◐ sound the north channel  job 1:10      $0.03 │
```
A finished task: `└ ✓ Tally the boats in the harbo…  done 19:34` (no report line: `first_line` empty, above).
"of usual" and `3/5` did not appear live (no history of the program; no plan steps); the rig covers them.
To repeat: `theseus-sim fake-model --rules rules.json`, `theseusd --config … --socket S --state-dir D`, `theseus-tui --socket S`.

## Gate
`scripts/gate.sh` fails in its door/keel phase on **main's own history**, not on this change: the keel guard refuses
`Keel: budget-raised crates/theseus-sim/src/lifecycle.rs` (commit 1c2beb2747) because its signature does not verify
here (`%G? is E`: no signer key on this VM), and flags a renamed store test. **Keel findings expected from my commits: none.**
`python3 scripts/keel-guard.py` prints only those two. Later phases I ran by hand: fmt, shape, clippy (workspace),
theseus-tui and theseus tests. I did not run the whole suite, lifecycle/jobs/turn benches, size or deny phases;
the maintainer's gate should. `protocol_types` untouched (no protocol change).

## Left / uncertain
- Docs: `docs/status.md` / the tour's TUI notes should mention the rows, header and title; `crates/theseus-tui/AGENTS.md` is updated.
- `job-follow`'s `last_line` is not shown (field absent). The mark for a done row in work mode is `✓` (the legacy `◆ done`
  stays in fallback); the `◆` moved to the report line.
- A failed task's row stays `✗` until seen like any needs-you; the title does not count it (header does): the owner may want it in the title.
