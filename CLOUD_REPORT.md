# CLOUD_REPORT: trim-utils (theseus-rwpbg.2)

Branch `cloud/20261017-trim-utils`, based on c18d8cbe. Three code commits: e0495de6 (dead items), 4c8560b6 (mirrors),
4b9daa5f (clip, now_ms, dates, jsonrpc). `git diff --stat c18d8cbe..4b9daa5f`: 83 files, +531 / -969.

## Steps
1. **Dead pub items (e0495de6, plus serving_daemon and SERVER_NOT_INITIALIZED in 4b9daa5f).** Census: each of the 37 names had exactly
   one `rg -w` hit before. Deleted 35. Kept LSP `INFORMATION` and `HINT`: dropping them leaves half a severity set. Deleting
   `work_root_of_session` left `WorkBoard::root_of_session` dead (rustc warned), so it went too. The doc claim about the
   cockpit went with `learn_proposals`. Fake-Discord helpers were deleted: one hit each, no Discord row calls them.
   `sealed.rs`'s read half and `daemon_in_cmdline` & co. stay.
2. **clip (4b9daa5f).** protocol `term/words.rs` now has `clip` (A), `cut` (C, no mark) and `cut_marked` (B, N chars then the mark).
   Moved: kernel's three `take(200)` -> `cut`; CLI status/task_tree/diffview, core thread_join -> `clip`; core session_find -> `cut`;
   recall/cite, needs_you, ask, watch/step, three card cuts, LSP and MCP -> `cut_marked` (byte-identical to what each did).
   **Bug fixed:** `webui.rs clip` said "at most 200" and returned 201; now `words::clip(.., 200)`; its test asserted 201, now 200.
   Left (own trim_end or width semantics, or Discord/herdr): protocol push/why/work_threads, CLI render/watches/horizon/vestige/harness,
   tui `cut`, judge, files, persona_import, harvest, project/slice, system.rs; Discord's 8 and `herdr_answer`.
   The review's "five core sites" that say "at most N" were not found beyond webui; the others' docs do not promise a bound.
3. **now_ms (4b9daa5f).** Kept the protocol name `now_unix_ms` (renaming its 100+ callers is churn). Replaced copies in kernel job.rs,
   core lsp/node_cache/rpc-ontology/aws-logs, index tender+vectors, CLI status/work/board, TUI main; store `record.rs` re-exports
   protocol's (store already depends on it). Kernel `Clock` untouched. Left: herdr.rs, tests' helpers, Discord/sim.
4. **Dates (4b9daa5f).** Deviation: **not jiff.** A new `theseus_protocol::time` holds Hinnant's `days_from_civil`/`civil_from_days`
   once, total over every input (callers like recall/when pass unchecked dates; jiff is fallible and would need a new dep line in four
   crates). Eleven copies now call it: core wake (re-exports), github, aws/hands/start, import/catalog, threads_import/fixture,
   learning/backfill, protocol why, tools git, exam time, theseusd install. Discord's `horizon_cmds` left. Pins: `time::tests` (leap
   day 2000/2024, year boundaries, day -1, year 0, 4-century round trip), existing `toolrun/hands.rs` pins of `utc`, wake's and exam's tests.
5. **jsonrpc (4b9daa5f).** `theseus_protocol::jsonrpc`: builders, `classify`, `classify_batch`, `id_number`, `Incoming`, base codes; protocol's
   `RpcError` gained `PartialEq` and `#[serde(default)]` on `message` (TS unchanged: `git status cockpit` clean). LSP and MCP files shrink
   to re-exports plus their own codes; their jsonrpc tests are untouched. New dependency edge lsp->protocol, mcp->protocol (Cargo.lock lists
   it, no new package). Pin: `jsonrpc::tests::the_builders_write_the_bytes...` (literal bytes for request, notification, response,
   error, batch). One output change: the "error answer with no id" message clips at 300 with the mark counted as before (B kept), none.
   CLI `CallError` not wrapped.
6. **Mirrors (4c8560b6).** Done: `When` = `RecallWhen` (`holds` moved to protocol), `Said` = `ReplayReply`. Pins in
   `recall::tests_time` and `own_bench::tests` (JSON bytes). Not done, with reasons: `Census` (has `of_messages`/`tokens`; an inherent impl
   on a foreign type is not allowed), `Estimate` (enum method + impls, compiler.rs at its ceiling), `TagCounts` (a *stored* record:
   `None` is stored as `null`, the wire omits it, so the bytes would change).

## Proof nothing was lost
- Planted reverts, each failing its test, then restored and touched: `clip` -> `take(max)` fails `term::words::tests::clip_at_most_n_counts_the_mark`;
  `719_468` -> `719_467` fails `time::tests::civil_dates_are_pinned...`; builder `"jsonrpc"` -> `"2.1"` fails `jsonrpc::tests::the_builders_write...`.
- A mistake caught by the suite: I first imported `cut` (no mark) into CLI `status.rs`; `theseus::status the_long_form_cuts_the_title_to_the_terminal`
  failed, fixed to `clip`; the CLI package then passed.
- No CLI golden changed. `cockpit/src/protocol.gen` unchanged.

## Gate
The full gate cannot run here: its door/keel phases compare with this clone's origin/main (9fa84735, older than the task base) and
fail on unsigned acks from main's own history. I ran its phases by hand instead:
- `cargo fmt --all` clean; `scripts/shape.sh` ok; `cargo clippy --workspace --all-targets -- -D warnings` clean; cockpit `npm run build` ok.
- `python3 scripts/keel-guard.py --base HEAD --head HEAD --quiet`: empty (the specified `--base origin/main` is unusable here as above).
- Suite (`nextest run --workspace`, TZ=America/Phoenix, debug info off to fit the disk): 6344 run, 6302 passed. Failures: 
  L1/sandbox (theseus-sandbox contract+bench, theseusd::sandbox: the known root-daemon refusal, theseus-pv6i);
  6 theseus-discord timeouts (`tests_war_room`, `tests_task_threads`, `tests_task_channels`): **also time out on the unmodified base** (checked by stash);
  `theseus-core tests_standing::the_records_rebuilt_after_a_restart...` fails on first attempt, passes on retry (same-millisecond ordering, a
  timing flake; passes on the base too). The two CLI status tests failed from my mistake above and pass after the fix.
- Not run: benches, `cargo deny` (not run; no deny phase by hand), a live scratch daemon. Live check for the maintainer: start a scratch
  daemon, `theseus status` at `COLUMNS=60` (titles end in `…`), an LSP tool call (`lsp.*`) and an MCP server list through a scratch config.
## Docs to change
`crates/theseus-core/AGENTS.md` (sealed_score line, done). Part III should note `theseus_protocol::{time, jsonrpc}` and that LSP/MCP now depend on protocol.
