# CLOUD_REPORT: gw-drive (theseus-batvg.3)

Branch `cloud/20261021-gw-drive`, base `1d092ded`. One step, one code commit: `7a6dee4`.

## Found

G1 was in the base (`theseus-google`, `google::{Google, call, finish}`, scope map, `docs` family, template markers).
Nothing of Drive existed. `Plan.authority` is what makes the gate ask at every posture, and `posture_only` false is
what keeps earned grants and routine grants from loosening a call, so `drive.share` uses it for every call.

## Changed (`7a6dee4`)

- `crates/theseus-core/src/google/drive.rs` (new): the four tools, one `DriveTool` with an `Op`.
  - **find**: typed fields to `q` (raw `q` parenthesised, `fullText contains`, `name contains`, `'id' in parents`, `mimeType =`,
    always `trashed = false`; `\` and `'` escaped; a folder must look like a Drive id, so it can't smuggle a clause).
  - **read**: Doc via `files.export` as `text/markdown`; Sheet via Sheets `get` (tabs) and `values:batchGet` (first tab
    `'Tab'!1:200`, or `range`) rendered as a markdown table; Slides via `presentations.get` with a field mask, per slide
    `## Slide N (objectId, layout NAME)` and `[objectId] text`; anything else downloads (`alt=media`). `save_to` is a
    Write resource of the plan; a file over 1 MB, or non-text, needs it.
  - **write**: create (multipart upload, `mimeType` set for conversion, type inferred from the extension: `.md` Doc, `.csv`
    Sheet, else raw; `slides` only from a `.pptx`), replace content (media upload), move (`addParents`/`removeParents`
    after reading the current parents), rename. `from_path` is a Read resource, capped at 5 MB.
  - **share**: grant (`user` or `anyone`, `reader|commenter|writer`, `notify` default false), remove (lists the permissions,
    deletes the match), trash and restore. Plan always has `authority`.
  - Results go through `finish`: `Source::File {owners all me, shared}` for a read; a listing is `Source::None` only when
    every row is the owner's own and unshared, else outside text `drive:find`; writes and shares are `Source::None`.
- `google/mod.rs`: `pub mod drive`, the test module, `NAMES` (5 entries), and the `all.extend(drive::tools(self))` line
  (replacing the `gw-drive` marker comment in `tools()`).
- Template: the four `drive.*` lines, commented, after the `# gw-drive:` marker (kept: `tests_google` asserts the markers).
- `crates/theseus-google/src/fake.rs`: `pub mod drive;`, `*` as a one-segment wildcard in routes (`path_matches`), and
  `Reply::raw` for non-JSON bodies (exports, downloads). `fake/drive.rs` (new): a stateful Drive (files, permissions,
  multipart parsing, Doc export, Sheet values, Slides read) installed with `Drive::install(&fake)`.
- `config/tests.rs`: `cfg.policy.tools.len()` 60 to 64 (four new template lines) and an assertion that `drive.share` is approve.

## Proved

- `cargo nextest run -p theseus-core google::`: 48 passed (10 in `tests_drive`).
- Tests (all in `google/tests_drive.rs`): `share_and_trash_always_ask_whatever_the_posture` (posture open in config and
  on every drive tool: plan has authority, decision is Approve, `posture_only` false, so no earned or routine grant
  covers it; whole turns for a share and a trash wait with nothing sent; the write and read plans are posture-only and
  open), `markdown_round_trips_through_a_doc`, `a_shared_owned_file_is_external`, `an_owned_unshared_file_is_not_external`,
  `find_compiles_typed_fields_to_drive_q`, `a_large_binary_needs_save_to`, `every_drive_tool_is_deferred_and_small`
  (family `docs`, each at most 220 tokens by the AWS test's estimate), plus a sheet/slides read, a share/remove/trash,
  and a write-validation test.
- Planted reverts (each restored and touched, `git status` clean after):
  - share's `authority` set to `None`: `share_and_trash_always_ask_whatever_the_posture` failed.
  - `quote` without escaping: `find_compiles_typed_fields_to_drive_q` failed.
  - the over-1-MB `save_to` check removed: `a_large_binary_needs_save_to` failed.
- `cargo clippy -p theseus-core -p theseus-google --all-targets -- -D warnings`: clean. `cargo fmt`: clean.
- Keel guard: `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing (exit 0).

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, fmt, shape, features, clippy, clippy-probe, cockpit
and test build passed; reader rule passed. The suite ran 7,259 tests: 7,224 passed and 35 failed:
- 33 are the known L1 failures (theseus-sandbox contract tests and `spawn_100`, theseusd's sandbox tests).
- `config::tests::example_template_uncommented_still_parses`: mine (the tool count); fixed in the commit and passing (153 config tests).
- `tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused`: failed in the full
  run, passed alone. Not mine (a horizon test); not on the flaky list.

Not run: the gate's phases after the suite (protocol types: no protocol change and `protocol.gen` is clean; lifecycle,
jobs, turn and size benches skipped by `THESEUS_GATE_NO_BENCH`). `cargo deny --offline check`: bans, licences and sources
ok; **advisories FAILED** on existing dependencies (no `Cargo.lock` change from this branch; the advisory database
fetched on this VM may be newer than the owner's). A re-run of the gate after the config-count fix was not made.

## Live check for the maintainer (needs the owner's Google account)

1. `theseus google auth`, then `theseus google status`: `drive.*` among tools_on.
2. In a session: `drive_find {"text":"<a word in a known Doc>","type":"doc"}` shows the row; `drive_read {"id": ...}` returns markdown
   (check headings and lists survive Drive's export), and for a Sheet the tabs plus a table, for a Deck the slide text.
3. `drive_write {"name":"gw-drive-check","content":"# T\n\n- a\n- b\n"}` makes a Doc; read it back. Then
   `drive_write {"id": <that>, "content":"# T2\n"}`: **this replaces a native Doc's content through a media update with
   `text/markdown`, which I could not verify against real Drive; if Drive refuses, replacement needs a different request**.
4. `drive_share` (any form) and `trash:true`: each must put a card to the owner at every posture, even with `[policy.tools] "drive.share" = "open"`.

## Left, uncertain, for the owner

- One `google.called` row per tool result: tools that make several calls (read of a Sheet, a move, a revoke) record only
  the content or last call's row. `Google::call` and `finish` take one row; widening it is G1's.
- `[policy.tools]` template: the marker comment stays and my lines follow it (the existing `tests_google` test requires the
  markers). `google::NAMES` grew to five entries on its own line: the other Google rows edit the same line, so expect a join conflict there and in `tools()`.
- `fake.rs` was touched beyond the one mod line and one route line (wildcard routes, `Reply::raw`), small and additive.
  Other rows adding `raw` too would conflict.
- Doc to change: the design's §3.1 should say `drive.read` of a Sheet needs the Sheets API enabled in the project (the Drive
  scope covers it, but not the API); `AGENTS.md` of theseus-core could list `google/drive.rs` and the fake's Drive stub.
- A disk note: this VM's writable disk is about 40 GB; a debug workspace test build overflows it. I deleted `target/` and
  built with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`.
