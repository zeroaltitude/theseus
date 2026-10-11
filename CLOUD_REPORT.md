# CLOUD REPORT: mcp-memory (theseus-02i1h)

Branch `cloud/20261011-mcp-memory`. Code commit `8cff180`.

## What I found
- `memory.lookup` (the model's tool) renders text only, and a private asker's recall draws on **shared places'
  sessions too** (`Place::may_draw_on`). So a plain reuse would have returned shared-place hits to an MCP client:
  the MCP path needs its own filter (below).
- The MCP server already writes `mcp_server.call` per tool call (tool, client, latency). `memory.read` is a second,
  richer row for memory reads only.
- `provable-memory-a` had not joined at my clone (fdd1492), so the citation is the **node id + WAL position**. Where
  its handle format would replace it: `MemoryCitation` (theseus-protocol `memory.rs`), built in
  `memory_lookup/mcp.rs` (`hit_of_item`, `hit_of_node`).

## What I changed (commit 8cff180)
- theseus-protocol: method `memory.lookup`; types `MemoryLookupParams`, `MemoryLookupResult`, `MemoryHit`,
  `MemoryCitation` (in `memory.rs`, no new module/include line); ledger kind `memory.read`; TS regenerated.
- theseus-core: `memory_lookup/mcp.rs` (search, get by id with neighbours, get a session page, all reusing
  `by_words` and the `read` helpers; `by_words` gained `session: Option<_>` and a `private_only` flag that drops
  every hit whose session `place_of` does not read as private); `rpc/memory_read.rs` (handler: only `Surface::Mcp`,
  only while `[mcp_server] memory`, answers "memory is off for MCP clients (`[mcp_server] memory`)", writes the
  `memory.read` row: principal `mcp`, tool, client, a 16-hex digest of the words/id/session, hits, ok);
  `mcp_server::ALLOWED` +`memory.lookup`; `[mcp_server] memory` (default false, in the template);
  `fact::memory::MemoryRead`. A shared place's node, by id or by session, answers as "not found" (never says it
  exists). Veiled (personal, partner-confidential, labels unread) text is never in a hit.
- theseus-mcp: `memory_tools.rs` (the two tools, two-sentence descriptions with "quote the citation"), `CoreClient::memory`,
  FakeCore. theseusd: `McpCore::memory` calls the method.
- `dispatch` (rpc/server.rs) was at clippy's 100-line/complexity limit: `memory.search` and `memory.lookup` now share
  one arm and helper (`Core::mem`); I removed one stale comment line there.
- No store format change. No new dependency. lib.rs +2 lines.

## Proof
- `tests_mcp_memory.rs`, 8 tests: native + imported hits with citations checked against the store; a shared place's
  node never returned (search, get by id, get by session); veiled hits veiled everywhere; get by id with
  neighbours; paging by cursor; memory off refusal + other surfaces refused; `memory.label`, `memory.consolidate`,
  `memory.search`, `memory.recalls`, `import.sessions`, `index.query` refused for Mcp and every `JUDGED` act errors;
  one `memory.read` row per call, digest only. All pass (48 with the lookup and surface suites).
- theseus-mcp: tools list names, a golden (`tests/golden/memory_tools.json`), param mapping test. 52 pass.
  theseus-protocol 65 pass. theseusd `mcp_server` binary 4 pass (its tools-list assertion updated).
- **Planted reverts** (each file restored and touched): `private_only` off -> `a_shared_places_node_is_never_returned`
  fails; veil off -> `a_veiled_hits_text_is_veiled` fails; key ignored -> `memory_off_answers_the_refusal_...` fails.
- A first version made `total` depend on the page size (`k` from the page); `paging_follows_the_cursor` caught it
  (12 vs 9); `k` is now fixed at 100.
- Suite (`cargo nextest run --workspace`): 3755 run, 3721 pass, 34 fail: 33 are the known L1 set
  (theseus-sandbox contract + spawn_100, theseusd sandbox). The 34th was `theseusd::mcp_server`'s tool-list
  assertion, which I fixed (passes now). Two other mcp_server tests failed once while the disk was full
  (`/` hit 98%, job tests need the disk); they pass after I freed build artifacts.
- `cargo fmt --check` ok, `cargo clippy --workspace --all-targets -D warnings` ok, `scripts/shape.sh` ok.

## Gate
`scripts/gate.sh` **fails in the keel phase**, on findings that are not mine: `origin/main` has moved past my base
(a9ad950), so the range includes others' changes: assert-removed in theseus-tui tests, `#[allow]`/`#[expect]` in
theseus-index and theseus-protocol tests_work_join, ceilings in long-files.txt (theseus-protocol lib.rs 2733 is
existing; mine is 2720). I touched none of those files. Phases after keel I ran by hand: fmt, shape, clippy, full suite
(above). I did not run `features`, `cockpit`, `deny`, or the benches. **Keel findings expected: none from this branch.**

## Live check (done here, scratch daemon on /tmp/live, torn down)
Scratch daemon: `[mcp_server] enabled, port 17434, memory = true`, key from `env:`; two imported fixture episodes
(one `public`, one `personal`); the index tender built and ready (bm25_only).
- `claude mcp add --transport http theseus http://127.0.0.1:17434/mcp --header "Authorization: Bearer ..."` with a
  scratch `CLAUDE_CONFIG_DIR`; `claude mcp list` -> `theseus ... √ Connected`.
- Raw MCP calls (curl): `memory_search {words:"ferry timetable", since:"March 2026"}` returned the public message with
  `node_id`, `position`, `session_id`, `as_of`, `harness claude-cli`; the personal ones had `veiled:true` and the
  placeholder text. `memory_get {session}` the same; `memory_get {id:"nod_x"}` -> error "no remembered node or session...".
  `theseus ledger --kind memory.read` showed one row per call (digest, hits, ok).
- **Not done**: `claude -p "what did I decide about X? use theseus memory"` (no API key here). A native-session hit on a
  live daemon (needs a model turn); covered by the core test instead.

### For the maintainer, on the owner's machine
1. Set `memory = true` under `[mcp_server]` in the owner's config overlay (it is `false` in the public template).
2. `claude mcp add --transport http theseus http://127.0.0.1:7434/mcp --header "Authorization: Bearer <key>"`, then
   `claude mcp list` (shows Connected; the server lists `memory_search` and `memory_get`).
3. `claude -p "what did I decide about <topic>? use theseus memory"`: the answer should quote a citation (node_id,
   position, session_id, as_of). Then `theseus ledger --kind memory.read`: one row per call, no words in it.
4. In a bound shared channel's session, say something unique, then search for it over MCP: it must not appear.

## Left / uncertain
- `memory.read` is written with `Rec::record` on the blocking pool, outside any turn (one small frame per call).
  Failed calls write a row too (`ok:false`); a call refused because memory is off does not.
- `memory_search` asks the index for up to 100 hits each call so the private-only filter cannot starve a page; it can
  cost a little more than `memory.lookup`'s own k. A deep cursor past 100 filtered hits answers nothing.
- A search with a `book`/`topic` narrows to the catalog's first 500 episodes, as `memory.lookup` does.
- Docs to change at review: spec (new method `memory.lookup` + `memory.read` row; `[mcp_server] memory`), status.md,
  M7 Q18 answered; the citation to move to the provable-memory handle when that joins.
- I deleted large stale test binaries under `target/` (in the repo, ignored) when the disk filled.
