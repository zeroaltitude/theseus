# CLOUD_REPORT: provable-memory-a (theseus-ejdk)

Branch `cloud/20261011-provable-memory-a`. One build commit (203c753) and this report.

## What I found
- Recall already froze each item's header and wrote `derived_from` edges; nothing cited, and no surface resolved a source.
- The brief says "`theseus history` takes a session or node today": it takes only a session (`theseus reach` takes a node's end). So `history` now accepts a handle.
- A Discord message's id was **not stored** anywhere (the brief says it is). Core only saw it as `turn.submit`'s `reply_to`.
- Discord uses classic components, not Components V2.
- Imported node ids end `_<index>`, so a handle's characters are letters, digits and `_`, not hex.

## What I changed (203c753)
1. **Handles.** `theseus_protocol::cite` (pure): `handles_for` gives each id the last 6 characters of its id (what `theseus history` prints after the prefix) and grows the whole pack's tail by one character until every distinct id's tail is distinct (tested). The handle is frozen at the front of the item's header at `hold_node` (`[m:a1b2c3] a message from…`), so old recalled headers keep their bytes and are simply not citable. One sentence appended to `turn::ASSEMBLY` (system block, no new breakpoint): "When you state something from recalled testimony, cite its handle." (changes the cached prefix once.)
2. **What a reply cited.** `turn/cite_step.rs`: after the reply, handles are resolved against the turn's own `Recall` items; unknown handles stay text; each given one is a `memory.cited` row (scope `recall:<session>`, with `memory.used`: recall id, handle, node, source session) in the turn's next frame. `Citation`s ride in `TurnSubmitResult.cites`, the reply post's footer, `session.history`'s `cites`, and the new `memory.cites` method. **No store format change**: a new ledger kind and a new META key only (no bump). A Discord message's id is stored as META `msg.origin.<node id>`, in the input node's own frame.
3. **Surfaces.** CLI `ask` (streaming marker holds a split handle) / `--no-stream` / `history`: `[1]` marks + footnote block + `theseus history m:a1b2c3`, which shows the message whole (`node.reach` + `session.history`). TUI: history and the answer of a turn it sent. Discord: `-#` footnotes above the footer, one action row: a link to the original message when the source is a Discord message in a bound channel (or the lane's own DM), else "Show source [n]" whose press answers ephemerally with an excerpt (`runtime/source.rs`). Cockpit: `ReplyMarkdown` with hover cards linking the session. Footnote builders are plain functions (`render::cite`) for term-render.
4. **Shared places.** `citation(item, shared)` returns none for a private or imported source; the core leaves it out of result, post and history, so every surface leaves the handle as text with no button.
5. Existing tests whose headers now start with a handle were rewritten (not deleted): `import::tests` (2), `tests_situation` (2), `tests_rerank_live` and `tests_tiering` normalisers (handles are per-run), `theseus-exam`'s oracle render (it must match the core's note byte for byte: handles added, test updated). `render.rs` (Discord) was at its ceiling, so `reply_parts`/`wake_header` moved to `render/reply.rs` (net -40 lines). `fake-model` rules may say `{handle}`. AGENTS.md of each touched crate updated. Clippy fixes: `rpc/server.rs`'s dispatch at its 100-line limit now routes `memory.recalls`/`memory.cites` through `memory_reads`; the Discord press hooks in `extensions_interaction` (cognitive-complexity limit).

### Choices / divergences the owner should hear about
- **CLI streaming** marks every well-formed handle `[n]` while the text streams (the citations arrive only at the end); a handle that was no source gets a footnote saying so, instead of staying text. `--no-stream`/`history` leave unsourced handles as text exactly.
- A TUI pane marks a reply streamed by **another surface's** turn only when the history is read again.
- `watch` shows raw handles (no cites on its events).
- Only the turn's own (first-loop) Recall node is cited against, as asked; a later turn does not cite an earlier turn's handles.
- Discord DM sources link only when they are the lane's own DM (the DM channel id is not known otherwise); others get "Show source".

## How I proved it
- `cargo nextest run --workspace` (minus L1 tests): 3720 passed; failures only the 33 L1/sandbox tests (known, root VM) and one timing test, `rpc::tests_ordered::reads_and_other_sessions_answer_while_an_input_waits` (passed alone twice).
- New tests: protocol `cite` (3), core `tests_cite` (6: header handle and guidance golden, two-given-one-unknown records two rows, handle not given, `memory.cites` and history `cites`, Discord id kept and cited, shared place cites no private/imported source), CLI `tests/cite.rs` (4: stream goldens, no-stream, history, `history m:…`) and `render::cite` (2), TUI `tests_cite` (2), Discord `render::cite` (3), `runtime::source` (1) and `tests_cite` (2 through the gateway: link button URL, "Show source" ephemeral excerpt, and a reply with no sources unchanged), cockpit `test/cites.test.ts` (3); fake-model `{handle}` (1). Cockpit: `npm test` 314 pass, `npx tsc -b` clean, `npm run lint` only pre-existing warnings, `npm run build` ok.
- **Planted reverts** (each restored and `touch`ed, `git status` clean): handle dropped from header, any handle accepted (`.or(items.first())`), shared filter off, guidance sentence removed → 5 of 6 `tests_cite` fail (header/guidance, two-rows, shared, history/cites, Discord id). `mark_into` made a no-op → CLI and Discord golden tests fail; Discord link set None → `a_source_links…` and the gateway test fail; TUI `cited` made a no-op → both TUI tests fail; cockpit `markCites` no-op → `npm test` 1 fail.
- **FAST**: `theseus-sim bench lifecycle --runs 10 --check` (debug build, this 4-core VM): LIFECYCLE OK; cold start p50 20.3 ms / p95 27.8 (budget 50); shutdown p95 16.1 (100); SIGKILL restart p95 29.6 (150); binary swap p95 26.7 (200). Handles are string work: no Jev call, no frame, no wait.

## Live check (done on this VM)
Scratch daemon (`theseusd --config … --state-dir … --socket …`, stand-in model `theseus-sim fake-model --rules` with text `We chose oak for the new pier [{handle}], not [m:ffffff].`, `[memory] mode = "live"`, env: secrets). `theseus ask "Remember: we chose oak for the new pier."`, wait for the index, then a new session: `theseus ask "What did we decide about the pier wood?"` printed
`We chose oak for the new pier [1], not [2].` + `[1] a message from sock#2 in ses_… (as of @19)` + `theseus history m:a739fb` + `[2] m:ffffff: not a source this reply was given`. `theseus history m:a739fb` showed the message whole; `theseus history <session>` marked `[1]` with the footnote; the ledger had one `memory.cited` row (handle, node, recall id, source session).

### For the maintainer, on the owner's real store with a real model
1. Install the build; on a scratch copy of the store (never the operator's daemon), `[memory] mode = "live"` or a canary of 1.0.
2. `theseus ask "what did we decide about X?"` where X was discussed before: expect `[n]` marks, the footnote lines (who, place, date, `theseus history m:…`), and `theseus ledger -n 400 --json | grep memory.cited` rows. `theseus history m:<handle>` opens the message whole.
3. Discord (a scratch daemon bound to a test channel only): type "what did we decide about X?" in a private channel/DM whose earlier message was a Discord message: expect `-#` footnotes, an `Open [1]` link button to that message, and, for a CLI-origin source, `Show source [2]` answering ephemerally with the excerpt. In a shared channel: handles stay as `[m:…]` text, no footnotes, no buttons.
4. Cockpit: open the session deck: the marks show a hover card and link to the source's session (not exercised in a browser here; unit-tested and builds).

## Left / uncertain
- Cockpit hover card and Discord link/press are verified by tests and stand-ins only, not a real browser or Discord.
- Docs to change at review: spec Part III item, `docs/status.md`, and the killer-features plan's row R9 (which says `theseus history` takes a node, and that a Discord id is stored).

## Gate
`scripts/gate.sh` failed in its **keel** phase on this branch's base range, not on my changes: it reports 5 erosions (TUI `a_finished_task…` renamed, `too_many_arguments` allows, long-files ceilings) from commits already on the base, and `scripts/keel-signers` lists no key at the base so no ack can pass; the 31 "same test moved" lines are core's config tests. **Keel findings expected from my commit: none** (no test deleted or loosened, no allow, no ceiling or budget raised; `render.rs` shrank). Per the brief I ran the remaining phases myself: `cargo fmt --check` ok, `scripts/shape.sh` ok, `cargo clippy --workspace --all-targets -D warnings` clean, cockpit lint/test/build ok, full suite as above, `cargo deny check` ok, lifecycle bench ok (turn/jobs benches were not run: `THESEUS_GATE_NO_BENCH=1`). `keel-guard.py` output pasted above in essence.
Note: the VM's disk filled twice (target/ ~25 GB); I removed `target/debug/incremental` and ran with `CARGO_INCREMENTAL=0`.
