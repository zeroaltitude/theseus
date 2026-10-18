# CLOUD_REPORT: pane-answers (theseus-ypnxt.3)

Branch `cloud/20261017-pane-answers`. Commits: `e0f26550` (A and B), `78643f42` (C). No store change, no dependency, no protocol type.

## What changed
- **A.** `crates/theseus/src/interactive/ask.rs` (new): `prompt_lines`, `keys_line`, `said`. `interactive.rs`: `say_prompt` prints an `owner_ask` numbered then `choose? [1-N, or your own words]`; `on_line` hands it to `on_ask_line`, which sends `work.answer` (`choice:N`, author `theseus watch`); words of the owner's own go as `choice:other` with the note; `y`/`n`/`t`/empty/number out of range send nothing and reprint the keys. A plain approval takes the old `action.confirm` path unchanged.
- **B.** `interactive.rs`: `Purpose::Turn` keeps its text; a `turn.submit` refusal that `task_is_over` (NOT_FOUND or class `execution_*`, as the chat and Discord read it) on a task pane (`of_task`: parent and title from `session.list`) goes to `redirect`, which sends `turn.submit` to the parent behind `task_redirect_line(title, short, "the task is over")`, prints that line, and prints the reply as `main › …`. A bare answer word keeps the old "nothing waits" line.
- **C.** `herdr_setup.rs` KEYS gains `prefix+p` / `theseus.pause`; `herdr-plugin.toml` action `pause`; hidden `theseus herdr pause --pane P` (`herdr_keys.rs`: `hold_for`, `pause`) reads the pane's task tokens and the board's `paused` (`work.watch`), sends `task.pause` or `task.resume` and prints one line. `herdr_main.rs` header lists `prefix+p pause`. `docs/herdr.md` and `crates/theseus/AGENTS.md` updated.

## Tests and plants
| test | plant | failure |
|---|---|---|
| `tests/pane_answers.rs` (4 tests), `interactive::ask::tests` (4) | `choice_id(n - 1)` -> `choice_id(n)` | `a_number_picks_the_option_at_its_place`, `an_owner_ask_is_answered_by_its_number` |
| same | owner_ask bypassed to the `action.confirm` path | `an_owner_ask_is_answered_by_its_number`, `other_words_answer_with_the_owners_own` time out waiting for `work.answer` |
| `a_message_after_the_task_ended_goes_to_main` | redirect guard off (old "not sent" line) | that test times out waiting for the second `turn.submit` |
| `herdr_keys_tests` (2 new), `herdr_setup_tests` (1 new) | `if v.paused` -> `if false` (always pause) | `pause_toggles_by_what_the_board_says_of_the_panes_task` |

Plain approval pinned by `a_plain_approval_still_goes_out_as_action_confirm` and the existing `tests/herdr.rs` (params `{correlation_id, approve, trust, note, author}` unchanged). The one changed existing assertion: the task header in `tests/herdr.rs:844` and `herdr_main.rs` tests now ends `· prefix+i inspect · prefix+p pause ──` (old: `… inspect ──`).
`cargo nextest run -p theseus`: 664 passed, 0 failed. `cargo clippy -p theseus --all-targets -D warnings` clean; `cargo fmt` clean; `scripts/shape.sh` ok.

Transcripts (from `--no-capture`):
```
# owner_ask answered by 2 (daemon saw work.answer {question_id: act_00reef, option: choice:1, author: theseus watch})
  ? Which quay first?
      1. North quay
      2. East quay — the tide is in
      3. Skip both
  choose? [1-3, or your own words]
# message after the end ("hello", then "y")
  ↪ [Written to the task “Sound the north channel” (…9f2c1d), and the task is over] → main
  main › Noted: the north channel waits.
  nothing waits for an answer now (it was answered or withdrawn): `y` was not sent
```

## Budgets
Not run: no wire, prompt or turn-path change (CLI only). The turn bench and owner-config fixture were not run here.

## Overlaps
- `herdr_sync.rs`: the hidden `Pause` variant and its dispatch arm (not in the row's file list; small).
- `herdr_main.rs` header and `tests/herdr.rs:844` (the header text). `docs/herdr.md`.
- No name that a landing lane renames. `theseus answer` is not printed anywhere new (the card's existing line is untouched). `task.get` not used.

## Proof nothing was lost
Whole package suite green (664). Plain approval params pinned. Bare answer words after the end unchanged.

## For the joiner
- **Gate:** `scripts/gate.sh` fails in the door here: this clone's `origin/main` is `9fa84735`, older than the base `40aa83cd`, so the keel guard reports 13 erosions that belong to the trim commits, not to this branch. `keel-guard.py --base 40aa83cd --head HEAD --quiet` is empty (my commits). Phases run by hand: fmt, clippy (theseus), the package suite, shape. Not run: cargo-deny, the workspace suite, the cockpit build, benches.
- **Close theseus-0f933** (the pane's redirect after the end was its last gap).
- **prefix+p:** the plugin-contract snapshot lists no herdr default keys, so I could not check that herdr does not bind `p`; the owner's config binds it explicitly via `theseus herdr setup`, and `lacks` flags a different binding.
- **After task-history (.1) joins**, `herdr pause` may call the CLI's `task pause|resume` instead of the RPCs.
- **Design choices:** an `owner_ask` has no decline option on the daemon (`choice:N` and `other` only), so the pane has no `n` for it. Words not starting with a number are `other`, as the pane already declines an approval with the words as the note. At start the question and its options print twice (the event card, then the prompt).
- **Environment:** the disk filled once during the plants (build artifacts); `target/debug/incremental` was removed.
