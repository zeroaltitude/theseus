# NOT LANDABLE

(As a whole branch: the tools-crate work is green, but the wiring that makes the stale-edit check bite was taken out, and four core goldens need an owner call. See below.)

**Row:** B6 `theseus-3i6d1`, item `theseus-w2cs` (file tools: CRLF, stale edit, hidden search).
**Branch:** `cloud/p5-w2cs-file-tools`. **Base:** `a2b44fd1`. Head: the commit before this report (`81d7637`). Files: `crates/theseus-tools/src/{fs.rs,lib.rs,tests_crlf.rs,tests_hidden.rs,tests_stale.rs}`; 4 fix commits (+ one that backs out the core wiring, `22a70a9` then `81d7637`).

## 1. CRLF (`d4809ba`)
- Confirmed: `fs.read` uses `str::lines()`, which drops `\r`, so a model quotes bare `\n`; a multi-line `old_string` never matched a CRLF file. `fs.patch` failed the same way. A single-line edit already worked.
- Fix: `fs.edit` retries the match with CRLF endings only when the text as given is absent from a file containing CRLF, and writes the replacement as CRLF. `fs.patch` applies to an all-CRLF file as the read showed it and writes CRLF back. Normalising is at the match and at the write of the edited text only, so other lines' endings (including a mixed file) are untouched. Rejected: normalising the whole file (a silent whole-file rewrite).
- Proof: red, before the fix: `old_string was not found in /tmp/.tmptvfixs/a.txt...` (multi-line edit) and the patch case failed. After: `cargo test -p theseus-tools --lib`: 90 passed.

## 2. Hidden files in search (`5d22cdf`)
- Confirmed: `walker()` hid them and `fs.grep`/`fs.glob` took no flag. Both now take `all` (default false; hidden and ignored included, as `fs.list`); `.git` is still never walked. The default scope text is unchanged (existing tests pin it); `all=true` says "hidden and .gitignore'd paths included; .git is never searched".
- Proof: red: `unknown field 'all'` for both. After: tests assert exact file lists (default: 2 files `a.txt`, `sub/b.txt`; all: 5 for grep incl. the ignored `skipped.log`, 4 for glob; `.git/config.txt` never present). 92 passed.

## 3. Stale edit (`2beed8f`, wiring `22a70a9` then backed out in `81d7637`)
- Confirmed: no staleness check existed. Added `ReadRecord`/`ReadScope` in `lib.rs` (a length+hash witness per session and path, no new dependency, no global): `fs.read` notes, `fs.write`/`edit`/`patch` note what they wrote, `fs.edit`/`fs.patch` refuse an unread file or one changed since, with a message saying to re-read. A ctx with no scope checks nothing.
- Proof: red: edit of an unread file returned `Replaced 1 occurrence...` (4 of 7 tests failed). After: 99 passed. Planted revert (`check` always `Ok`): the same 4 tests went red again; restored and touched.
- **NOT live:** wiring it in `toolrun.rs`/`session_dir.rs` (one record in `ToolRuntime`, a scope per session in `ctx_in`) made 27 core tests fail, all tests that edit a file the session never read (own_bench::whole, rpc::tests_rewind, tests_lsp_edits, tests_end_note, tests_gate_layers, tests_tool_words, rsi::tests_pr, tests_m3::parallel). Fixing them means rewriting many tests; I left that to the owner. So today the check exists and is tested but the runtime does not enable it. To enable: restore those two core edits (see `22a70a9`) and give those tests a read.

## Gate / core results (the gate script was NOT run)
I did not run `scripts/gate.sh`, install node 24, or `npm ci`. What I ran: `cargo test -p theseus-tools --lib` (99 pass) and `cargo nextest run -p theseus-core` on variants. Keel guard: `0 findings` on the range.
Final core state (tools changes, no wiring): the 4 `google::tests_morning::*` cases fail on the base too (not mine). **4 are mine and need an owner call:** `tests_tasks_wake::a_persons_turn_loads_what_it_always_did`, `tests_private_view::a_private_sessions_request_is_byte_identical`, `tests_tiering::a_recall_source_from_before_a_floor_renders_the_same_bytes`, `tests_tiering::every_request_is_the_same_with_the_cache_off`: they pin exact token counts of the tool table (e.g. `DM_PERSON_TOKENS`), which the new `all` schema property moves. I did not touch the constants (the guard would count it as erosion and I cannot ack). Another 3 context-size tests were fixed by trimming the description text. The other failures I did not rerun individually at the end; the full core run before the trim had 11 failures, 4 google + these 7.

## Left / uncertain
- Enable the stale check in core (above), and update the four token constants (owner).
- The brief's pointers were right on `fs.rs` lines (drifted a few). `docs`: `crates/theseus-tools/AGENTS.md` should mention the read record and `all`.
