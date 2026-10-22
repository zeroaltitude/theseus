# CLOUD_REPORT: tail2-google (theseus-urpt2, theseus-2j9dr, theseus-nbxwz, theseus-d3ysk)

Branch `cloud/20261021-tail2-google`, base `6918e3c1`. One code commit, `8fa70a6` (all four ids in its subject; I
did not split it per id). Store format unchanged (48); no dependency added; no file near a ceiling
(`drive.rs` 1,549 lines).

## First section: deviations and things to know

- **`into_shared` fits, at exactly 220 tokens.** `every_drive_tool_is_deferred_and_small` measures
  `len*10/24` and passes at `MAX_TOKENS = 220`. To fit I had to shorten `drive.write`'s description: it lost its
  opening "Write to Drive." and the `as` mention (the schema's enum still names it). It now reads: "No `id`: create
  `name` from `content` or `from_path` (.md Doc, .csv Sheet). With `id`: replace/`move_to`/`rename`. `into_shared`
  asks owner." The schema entry is a bare `{"type": "boolean"}`. No room is left in the write tool for more words.
- **An existing test needed its fixture changed (not weakened).** `markdown_round_trips_through_a_doc` moved a file to
  `folder9`, which the fake never had. A move now reads the target folder first, so the test adds
  `File::folder("folder9", "Archive")` before the move. Its assertions are untouched.
- **A target folder that cannot be read fails the write** (its `files.get` failure carries the row). I chose
  fail-closed over "write anyway". With `into_shared: true` the read is skipped (the owner was asked).
- A target is "shared" when `shared` is true or `driveId` is non-empty. I did not use `mimeType` from the
  `fields=shared,driveId,mimeType` read to refuse a non-folder target (Drive will refuse that itself).
- The full gate did not complete here. See "Gate" below (disk).

## theseus-2j9dr: every Drive call keeps its row

Found: `Reading::run`, `sheet`, `slides`, `Writing::run/change`, `Sharing::run` kept only the last call's row, and
several errors after a good call carried none.

Changed: `drive.rs` threads a `rows: &mut Vec<Value>` through every call (`call()` pushes the row, a failed call
returns `sheets::failed(f, rows, "")`); drive's own `failed` is gone. New `sheets::refused(message, rows)` builds the
failure for an error after a call (folder, other native type, no tabs, over 1 MB, not text, save_to write, no such
access, "nothing to change", bad permission id). A result's meta uses `with_rows`. `compile_q` refuses a `q` whose
parentheses or single quotes do not pair (honours `\'`, ignores parentheses inside quotes); a balanced `q` compiles
exactly as before. `Reading::run` was split (`shown`) to stay under clippy's 100-line limit.

Calendar check: each calendar tool makes one `g.call` per run (`events` near `listed`, `put` near the change path),
so no rows change there.

## theseus-urpt2: a write into a shared folder asks first

`drive.write` reads the target (`files.get`, `fields=shared,driveId,mimeType`, row kept) before a create with
`folder` and before a move with `move_to` (before the content PATCH too). A shared target is refused with
"`<id>` is shared: a file put there is shared with its members. `into_shared: true` asks the owner first, or
`drive.share` shares the file itself." `into_shared: true` makes the plan `class: Write` plus an `authority`
sentence, so the gate asks at every posture. `from_path` is unchanged.

## theseus-nbxwz

- (1) `slides.rs`: `Run.created`, set in `Run::create`, replaces `rows.len() > 0`.
- (5) `sheets.rs`: a create answer with no `spreadsheetId` is `refused("the create answer named no spreadsheet id",
  rows)` with the create's row; no values call is made.
- (4) `three_rows_make_three_ledger_rows` (pins existing code, no plant).
- (6) Convention: **one row per call** (`google_called_also`). Drive, Sheets and Slides use it; calendar makes one
  call per run. Mail's `merged()` is untouched (not mine).
- Items (2) and (3) of the issue (Slides `deleteText` on an empty placeholder; the layout to placeholder map) are
  live checks, below.

## theseus-d3ysk

`calendar.rs` `own_text(e, account)`: `organized_by` and `eventType != "fromGmail"` and a creator that is absent, or
`creator.self`, or `creator.email` equal to the account (ignoring case). `listed`'s `stranger` and the change
path's `organizer_is_account` use it; `render`'s "(you)" note stays on `organized_by`.

## Tests and plants

All plants were applied with a script, each run alone, then the file restored and `touch`ed (`git status` clean
after).

| Test | Plant | Failure |
|---|---|---|
| `a_sheet_read_keeps_every_calls_row` (key) and `a_move_keeps_the_get_and_the_patch` | `call()` stops pushing rows | both fail: `tests_drive.rs:573` (the methods list) and `:622` |
| `an_error_after_a_call_keeps_its_row` | `refused` drops its rows | `tests_drive.rs:602` |
| `a_write_into_a_shared_folder_asks_first` | skip the folder read | `tests_drive.rs:552` (the refusal case) |
| `a_raw_q_must_balance` | disable the balance check | `tests_drive.rs:710` |
| `a_failed_notes_batch_on_an_existing_deck_does_not_claim_it_made_it` | back to `rows.len() > 0` | `tests_sheets_slides.rs:407` |
| `a_create_answer_with_no_id_is_refused` | `unwrap_or("")` | `tests_sheets_slides.rs:436` |
| `a_gmail_added_event_is_outside_text` | `own_text` back to `organized_by` | `tests_calendar.rs:639` |
| `three_rows_make_three_ledger_rows` | none (pins existing code) | |

The shared-folder test also covers a shared drive (`driveId` route), an unshared folder writing as before, the
plan's `authority`/`class`/`Posture::Approve`/`!posture_only` with `into_shared`, and the run writing. A new fake
helper `File::folder(id, name)` is in `theseus-google/src/fake/drive.rs`.

Runs (with `TZ=America/Phoenix`, `CARGO_INCREMENTAL=0`):
- `cargo nextest run -p theseus-core -E 'test(google::) | test(fact::) | test(config::) | test(tests_first_request) | test(tests_registry)'`: 286 passed, 0 failed.
- `cargo nextest run -p theseus-google`: 26 passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --all --check`: clean.
- `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`: empty output, exit 0.
- No L1 test was run or needed. I did not run the load recipe (no timing change).

## Gate

`scripts/gate.sh` was **not completed**: it recompiles the whole workspace in its own configuration and the VM's disk
allowance (about 35 GB used by `target/`) ran down to 3.8 GB, so I stopped it by pid. Run on the owner's machine:
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`. The checks above (fmt, clippy, the Google/fact/config
suites, keel guard) passed. The shape phase should pass (no file near a ceiling), but I did not see it run.

## Live checks for the owner

1. A real `eventType: "fromGmail"` event (a flight or reservation): `theseus` calendar read of it should be outside
   text (holds the session). Does Google really mark it `organizer.self`? The code treats it as outside either way.
2. Slides `deleteText` on an empty placeholder (the `replace` path): does Slides accept it or answer 400?
3. The layout to placeholder map (`placeholders()`) against a real deck.
4. `drive.write` with `folder` set to a real shared folder, no `into_shared`: refused, nothing created. With
   `into_shared: true`: a card at every posture, and after yes the file is created.

## Docs lines for the maintainer

- Google design, Drive section: "`drive.write` into a shared folder (or a shared drive) is a permission change by
  inheritance: refused unless `into_shared: true`, which makes the plan the owner's authority at every posture."
- Google design, ledger: "One `google.called` row per call; a tool that makes several keeps the last in
  `google_called` and the earlier ones in `google_called_also`, on success and on failure (`sheets::failed`,
  `refused`, `with_rows`)."
- Calendar: "An event is the owner's text only if the account organizes and created it and Gmail did not add it
  (`own_text`)."

## Follow-ups

- Mail should move to one row per call: drop `merged()` in `google/mail.rs`; the test that asserts `calls == 2` is
  `tests_mail.rs:846` (`out.meta["google_called"]["calls"]`).
- If more words are ever needed on `drive.write`, `MAX_TOKENS` is already at its limit: split the tool instead.
