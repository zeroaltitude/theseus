# Cloud report: `theseus import openclaw` (theseus-0lrr.6)

Branch `cloud/20261005-soul-import`, on `c4f79e9f` (store format 22 as cloned). Started 2026-10-06 00:15 UTC.

Commits on top of the task commit:
- `4be682e4` import: the operator's past history as imported sessions, idempotent and erasable by tag; store
  format 23 (theseus-0lrr.6). Steps 1 to 4 together (below says why).
- `f306c939` import: history and node listings show an erased import's tombstones, and the erase says what the
  index's forget found (theseus-0lrr.6).
- the report (this file).

**Why steps 1 to 4 share one commit.** Step 1 alone (the session flag, the bodies, the origin, the format bump) has
no writer: nothing would write an imported session, so the reader rule's spirit and the tests have nothing to hold.
Steps 2 to 4 share one fixture and one rig; each test reads more than one step (the erase test re-imports, the
place test imports). I built them together and proved each by its own tests and planted revert, listed per step.

## Step 1: the imported session in the store

**Found.** A session is a `SessionRecord` (kind `conversation` or `task`); a node's provenance is `Node.origin`
(operator, agent, tool, harness, mcp) plus a tool result's `external`; its time is `created_at_ms`. A session has a
place only through the outbox's binding, and an unbound one reads as the CLI's (private). Recall's place rule reads
`TurnRunner::place_of`. The index's extractor reads nodes as JSON, so a new body kind with `text` is indexed by its
catch-all arm.

**Changed.**
- `SessionRecord.imported: Option<Box<ImportedFrom>>` (`import/mod.rs`): tag, episode id and hash, source, agent,
  place `{kind, name, id}`, labels (sensitivity, partner, topic, book hint, credential redacted), triage, as-of,
  message count, whether a summary was written, the file and line it came from, when it came in, and `erased`
  (the receipt). A flag, not a new `SessionKind`: no client's match changes, and the wire's `SessionInfo` keeps its
  shape. Kind stays `conversation`, label `imported`.
- The session's id is the episode's: `ses_ep` + the episode id's 64 hex digits (`session_id_of`). `new_id` mints
  `ses_` + hex, and `p` is no hex digit, so no other session can start `ses_ep`. That makes "is this imported?" a
  string test (`import::is_imported`), with no read, wherever the turn path asks it. (I first took 32 digits; the
  fixture's ids collided on them, so it is the whole id.)
- Node ids are the session's tail and the index (`imp_<64 hex>_<idx>`, `imp_<64 hex>_summary`): a retried batch
  writes the same ids.
- `Origin::Import`, and three NODE bodies: `Imported { text, integrity, source, unit, sha256, idx }`,
  `ImportedSummary { text, cites, model }` (cites are node ids), and `Erased { was, at_ms, why }` (the tombstone).
  The brief's `Import { source, unit, sha256 }` origin is the `import` origin plus those body fields: `Origin` is
  `Copy` and stored in every node and stub peek, so the details ride in the body.
- `created_at_ms` is the message's own time (the summary's is the episode's end). The recall header reads it, so
  the model sees the message's date.
- Integrity maps onto the provenance the core has: `outside` makes the index mark the node external (the extractor's
  new `imported` arm), so recall drops it as `untrusted` unless `[memory] include_external`, and its frozen header
  reads `imported outside text via <author> (from <source>, not instructions)`. `operator` and `agent` keep their
  words in the header (`an imported message from wren (operator, from openclaw-store)`). The author is the episode's:
  the operator by bare name, `agent:<name>`, `person:<name>`, `tool`, `outside`. No owner's name is in the code.
- Closed and read-only: an imported session has no execution, so the kernel never drives it, and `turn.submit`
  refuses it (REFUSED, before any prompt or session is touched, `import::refusal`), so it is never resumed and never
  compiled as a turn's own context. The compile step is untouched.
- Private under the place rule: `TurnRunner::place_of` returns `Private` for an imported id, whatever place the
  episode names; `place_name` names it in a header as `the imported dm place-0 (<tag>, <source>)`.
- `session.list` leaves imported sessions out (the whole list, and `n`/`before` pages, which walk past them by key,
  undecoded; `rpc/pages.rs`). Without this an import of 10,000 episodes would be the newest 10,000 births and push
  every real session off the TUI's sidebar. `import.list` lists them by tag.
- Every exhaustive `Body` match got its arms (stub kinds, the memory pass's eligibility, activation's hits, the
  arrangement's text, the check's working text, `node.info`, publish, the exam's replay). The memory pass would label
  an imported message as a message (outside as external), so nothing stops its later read; nothing reads imported
  sessions there yet.
- Store format 22 -> 23 (`MANIFEST_FORMAT` + its doc line), theseusd's `versions.rs` (writes 23, refuses 24, reads 2
  to 23), core store.rs's pin, and a layout sample: a routed session at format 21 to 22, by hand, which reads and
  round-trips. The goldens print no store format, and none of their lines moved.

**Proved.**
- `import::tests::an_episode_file_imports_once_and_a_second_run_skips_every_episode`: every source and place kind,
  origin `import`, the message's time, the summary's cites, the credential marker and labels kept, no execution.
- `import::tests::an_imported_session_takes_no_turn`, `the_session_list_leaves_imported_sessions_out`.
- `tests_layouts::every_old_layout_on_disk_still_reads`, theseusd `versions` (in the gate's suite).
- Planted: the as-of set to the import time (`now_unix_ms()` for `m.at_ms` in `records_of`): the import test fails
  (`left: 1791249426795, right: 1777730591000`), and the place test fails (`… 2026-10-06 01:17 UTC (as of @8)`).

## Step 2: the import, `import.episodes` and `theseus import openclaw <file>...`

**Changed.**
- Protocol: `theseus-protocol/src/import.rs` (params, results, the tag listing), three methods on one line of the
  table (`import.episodes`, `import.erase`, `import.list`), two ledger kinds (`import.batch`, `import.erased`). The
  TypeScript is regenerated (8 files). `lib.rs`'s ceiling goes 2,727 -> 2,730 (`scripts/long-files.txt`, with why).
- `import/episode.rs`: reads a line, checks `format` 1 and every field against the format's lists (sources, place
  kinds, sensitivities, `partner-candidate:<codename>`, books, integrity, author shapes, 64-hex digests, RFC 3339
  times, the summary's cites naming messages it has, idx once), and the hash: sha256 of the canonical JSON without
  `hash`, as Python writes it (`sort_keys`, no spaces), accepting both `ensure_ascii` forms, with floats in Python's
  repr (`1e-05`, `1e+16`). Unknown fields are tolerated (the hash covers them).
- `import/write.rs::import_batch`: one batch in one frame: each episode's nodes, then its session record (scoped
  `import:<tag>`), the tags' counts (META `import.tag.<tag>`) and an `import.batch` row (scoped
  `import.ledger:<tag>`). A batch past 4,000 records or 8 MiB is cut into another frame, with a wait on
  `pressure::quiet_blocking` between. One import or erase at a time (a static mutex), on the blocking pool.
  Idempotent: an id imported with the same hash is skipped; another hash is rejected and named (`imported before
  with another hash (…), not overwritten`); an id twice in one batch is the same; an erased episode is not imported
  again (my choice; see below). A malformed line is rejected with its line number and the batch goes on.
- `rpc/import.rs`: the dispatch (`rpc_prefixed`, which also routes `pack.*`, so `dispatch` stays within clippy's 100
  lines), `judge_act(Act::Import)` for the import and the erase (the owner from a private place), the forget.
- CLI `crates/theseus/src/import.rs`: `import openclaw <file>...` reads each file a line at a time
  (`read_until`, so it never holds the file), sends batches of 256 lines or 4 MiB, one in flight, 20 ms apart, sums
  the counts, and prints per file `read, imported, skipped, rejected (nodes in frames, ms)` and each rejected line
  `line N (ep_…): why`. A line that is not UTF-8 is rejected locally. `--json` prints the summed result. Both
  writes are in `OPERATORS` (refused inside a job).

**Proved.**
- `a_changed_hash_is_rejected_and_a_bad_line_is_named_by_its_number`: a changed episode under its id (rejected,
  the stored text unchanged), a truncated line (line 3, `not JSON`), a blank line (not counted), `format 2` (line 5),
  a forged text under the old hash (line 6, `hash does not match`).
- `an_import_from_no_private_place_is_refused`: an unnamed connection's import and erase are REFUSED, nothing
  written.
- `episode::tests::lines_a_python_pipeline_wrote_read_with_their_hashes`: two lines written by `json.dumps` in
  Python 3 (one hashed with `ensure_ascii` on, one off with non-ASCII text, the credential marker and `1e-05`), and a
  byte changed in either is refused. `floats_are_written_as_python_writes_them`, and
  `the_canonical_form_is_pythons_in_both_string_forms` (Python's own outputs, checked with `python3`).
- The CLI's `a_files_counts_and_its_rejected_lines_by_number`.
- Planted: idempotency off (the lookup of the stored episode replaced by `None`): the import test fails, the second
  run imports all 12 again (`left: (12, 0, 1), right: (0, 12, 0)`), and the changed-hash and erase tests fail too.
- **Rate**, on a scratch daemon of this build (debug binaries, this 4-core VM), a synthetic file of 10,000 episodes
  (15.8 MB, 3 messages and a summary each, written by a Python generator with real hashes): imported in **8.5 s wall
  (about 1,170 episodes a second; 6.6 s in the daemon, about 1,500 a second)**, 40,000 nodes in 40 frames. `health`
  asked every 250 ms beside it answered in p50 11 ms, max 19 ms. The same file again: all 10,000 skipped, 0 frames,
  4.0 s wall. The index tender caught up (40,200 nodes, 0 behind) seconds later.

## Step 3: erase by tag, and `import list`

**Found: the store's erasure semantics today.** Spec §5.6 specifies payload erasure with preserved structure
(the record keeps its id, kind, origin, times and edges; the payload becomes an erasure marker; a receipt). What is
built: (a) the tender's `index.forget` (the index drops nodes now; a rebuild from the WAL brings them back), and (b)
the tender's follower dropping a node written again with nothing to index ("an erased payload"). No `Redaction` or
`Suppression` record exists, and nothing rewrites a WAL frame.

**Used.** (b) as the durable part, with (a) for immediacy: `import.erase` writes, in batched frames, each node of
the tag again under its id, origin, author and time with `Body::Erased { was, at_ms, why }`, each session's record
with its `erased` receipt (who, when, why), the tag's counts, and an `import.erased` row; then asks a running tender
to `index.forget` the nodes. The follower drops each tombstoned node as it reads it, so the index is right even with
no tender running, and a rebuild leaves them out.

**Found and fixed in the tender.** A rebuild meets a node and its tombstone in one batch. The skip path asked
`engine.holds`, which reads only what is committed, so the node stayed indexed after a rebuild. `ingest` now keeps
the nodes it indexed in the batch (`fresh`). theseus-index's AGENTS.md says so.

**Also fixed (C2).** `session.history` and `node.list` scan a session's scope, which holds the original records
beside their tombstones. `import::shown` shows an imported node by its newest record, once; a listing with no
imported node costs one pass.

**Proved.**
- `import::tests::an_erased_tag_leaves_nothing_to_recall`: 12 sessions and every node tombstoned (each node's
  newest record is `Erased` with its time kept), the receipts, the history and listing show only tombstones and no
  text, a private turn's recall finds only the other tag's (the stand-in index reads as the follower does),
  `import.list` counts 12 erased, a re-import is refused as erased, a second erase finds nothing.
- theseus-index `tests_import::an_erased_import_leaves_the_index_and_a_rebuild_brings_none_back`: real tender, real
  WAL: indexed (outside text external, origin `import`), tombstones appended, none held or found, the live node still
  found; a fresh index over the same WAL holds only the live node.
- Planted: the erase writes no tombstones (`frame.push(n.erased(…))` dropped): the erase test fails (the newest
  record is the imported message). The tender's `fresh` taken out: the index test fails (`imp_0a_0 is not rebuilt`).
  `shown` taken out of the history: the erase test fails (the history lists the three imported texts beside their
  tombstones).
- Live: erasing 10,000 sessions and 40,000 nodes took 4.2 s in 13 frames; the tender had already dropped every node
  at its tombstone when the forget was asked (the reply now says so), and the index held only the live session's 2
  nodes. Erasing 50 took 31 ms.

## Step 4: recall sees it

**Proved.**
- `a_shared_place_recalls_no_import_and_a_private_place_recalls_it_with_its_time`: a turn in a shared channel
  recalls none (each imported session dropped for `place`), though two episodes name that very channel; a CLI turn
  (private) recalls the decision, its `Recall` node's frozen header `an imported message from wren (operator, from
  openclaw-store) in the imported dm place-0 (reef-2026-05, openclaw-store), 2026-05-02 14:03 UTC (as of @…)`, and
  the model's request holds the fact and that date.
- `outside_text_is_never_placed_as_instruction`: by default the outside message is dropped `untrusted` and never in
  the request; with `include_external` it is admitted after the testimony preamble, its header naming it outside
  text and not instructions, and nothing of it is in the system block.
- Planted: the place taken from the episode (a `discord-channel` episode marked `Shared(discord:channel:<id>)`):
  the place test fails, the shared channel recalling its two episodes' six nodes. **The brief expected the
  private-place test to fail here; it cannot**: a private place may draw on every place, shared ones included
  (`Place::may_draw_on`), so a shared mark only shows on the shared side. Taking the explicit `Private` rule out
  altogether fails nothing either: an imported session has no binding, and an unbound session already reads as the
  CLI's, private. The rule guards a future binding; the tests guard the outcome.
- Live (below): a CLI turn recalled 6 imported summaries from the BM25 index, headers with the episodes' dates and
  tags; after the erase, none.

## FAST

The import is an operator command: nothing on the start path, and on the turn path only `place_of`'s string test
and, for an admitted imported item, one session read for its header's place name.

A (main, `c4f79e9f`) against B (this branch), debug builds, each driven by its own `theseus-sim`, A B A B:

| | A | B | A | B |
|---|---|---|---|---|
| cold start, p50 / p95 ms | 16.3 / 21.1 | 16.9 / 19.7 | 17.0 / 29.9 | 15.6 / 22.5 |
| clean shutdown | 5.7 / 9.4 | 6.0 / 7.4 | 6.1 / 7.5 | 6.2 / 11.4 |
| SIGKILL, restart | 17.5 / 25.9 | 16.1 / 22.5 | 17.2 / 20.8 | 17.6 / 25.4 |
| binary swap | 19.0 / 21.9 | 17.3 / 22.8 | 21.0 / 24.3 | 17.1 / 33.0 |
| restore (no budget) | 30.2 / 38.5 | 27.6 / 30.6 | 27.1 / 34.3 | 27.1 / 230.4 |
| turn bench, plain: frames, wall p50 | 5, 40.3 | 5, 38.5 | 5, 37.0 | 5, 42.2 |
| tool-call: frames, wall p50 | 9, 96.1 | 9, 94.5 | 9, 87.3 | 9, 90.8 |

Every lifecycle budget passed in all four. The one restore p95 of 230 ms is one run of ten (its p50 27.1 ms); restore
has no budget and does not read the import's code. Turn bench: 10 runs each and a burst of 30 turns. The logs are in
the session's scratchpad (`ab-lifecycle.log`, `ab-turn.log`). (B's `theseus-sim` driving A's daemon failed at the
bench's first start, `theseusd exited (exit status: 1) before answering`; I did not chase why, and ran each build
with its own driver.)

## Live check for the maintainer (a scratch daemon of this build)

```sh
D=$(mktemp -d); mkdir -p $D/projects
cat > $D/config.toml <<EOF
[model]
api_base = "http://127.0.0.1:47731"
[providers.zai]
api_base = "http://127.0.0.1:47731"
api_key_secret = "zai_api_key"
[secrets]
anthropic_api_key = "env:THESEUS_FAKE_KEY"
zai_api_key = "env:THESEUS_FAKE_KEY"
[discord]
enabled = false
[web]
enabled = false
[tools]
projects_dir = "$D/projects"
[memory]
mode = "live"
EOF
echo '[{"when": "tide log", "text": "From what was recalled: the tide log is kept in the boathouse ledger."}, {"when": "", "text": "Noted."}]' > $D/rules.json
theseus-sim fake-model --addr 127.0.0.1:47731 --rules $D/rules.json & FAKE=$!   # kill $FAKE at the end
THESEUS_FAKE_KEY=sk-fake theseusd --config $D/config.toml --socket $D/sock --state-dir $D/state &
T="theseus --socket $D/sock"
# The fixture: the two lines a Python generator wrote, in the tree (or a small file of the pipeline's).
cat crates/theseus-core/src/import/python-ascii.jsonl crates/theseus-core/src/import/python-unicode.jsonl > $D/eps.jsonl
$T import openclaw $D/eps.jsonl        # read 2, imported 2, skipped 0, rejected 0 (8 nodes in 1 frame, … ms)
$T import openclaw $D/eps.jsonl        # read 2, imported 0, skipped 2, rejected 0 (0 nodes in 0 frames, … ms)
$T import list                          # reef-synthetic: 1 session, 4 nodes, 0 erased; openclaw-snapshot 1
                                        # reef-unicode: 1 session, 4 nodes, 0 erased; openclaw-store 1
$T sessions                             # prints nothing: no imported session is listed
$T health | grep '^index'               # within seconds: ready · bm25_only · 8 nodes in 8 chunks · … 0 B behind
$T ask --json "Where is tide log 2 kept, per the reef survey?" > $D/ask.json; jq '{recalled, output}' $D/ask.json
#   recalled 6; output "From what was recalled: the tide log is kept in the boathouse ledger."
SID=$(jq -r .session_id $D/ask.json)
$T memory recalled $SID                 # private · ran · 8 candidates · admitted 6 (imported_summary first) ·
                                        # dropped 2 for untrusted: the two outside messages (imp_…_2)
$T rpc node.list "{\"session_id\": \"$SID\"}" | jq -r '.nodes[] | select(.kind=="recall") | .detail.items[].header'
#   an imported summary of 2 messages in the imported discord-channel place-2 (reef-synthetic, openclaw-snapshot),
#   2026-05-03 12:40 UTC (as of @14)
#   an imported message from wren (operator, from openclaw-snapshot) in the imported discord-channel place-2 (…),
#   2026-05-03 10:03 UTC (as of @11)   … and four more, each with its episode's own date
$T ask -s ses_ep1e5ce1dd5da6434b5495f93e0fc68026ccaa6c37f8e58d7f570774e8fca8f56e "go on"
#   theseus: ses_ep1e5c… is an imported session: closed and read-only, it takes no turn. … (code -32005); exit 1
$T import erase --tag reef-unicode --why "live check"
#   erased reef-unicode: 1 session and 4 nodes tombstoned in 1 frame (… ms); index: nothing left to forget: its
#   follower had dropped them at their tombstones
$T import erase --tag reef-synthetic --why "live check"
$T import list                          # each tag: 1 erased
$T history ses_ep1e5ce1dd5da6434b5495f93e0fc68026ccaa6c37f8e58d7f570774e8fca8f56e   # four lines, each "erased"
$T ask --json "Where is tide log 2 kept, per the reef survey?" > $D/ask2.json
$T memory recalled $(jq -r .session_id $D/ask2.json)   # 2 candidates, both the first ask's own: no imported item
$T health | grep '^index'               # 4 nodes: the two asks' messages and replies only
$T shutdown; kill $FAKE
```

I ran these commands as written on this build (02:02 UTC); the outputs above are theirs. The ids are the fixture's:
an episode's session is `ses_ep` and its episode id's hex.

With the pipeline's real files, the same commands; `--json` on `import openclaw` gives the summed counts and every
rejected line. Run the first import on a copy of the store (a copied state dir), since a store moves to format 23 at this
build's first write and an older build then refuses it.

## What is left, uncertain, or the owner's to decide

- **The payloads stay in the WAL.** An erase tombstones every reader's view (the newest record, the index, recall,
  history, `memory.recalls`), but the original frames still hold the text, and a backup or durability tender that
  shipped them keeps them. §5.6's in-place payload erasure, its `Redaction` record, and the backup accounting are
  not built. If something imported must later not exist at all, that is the step it needs.
- **A `Recall` node written before an erase** (a turn that recalled an imported node) renders its source by WAL
  position in that session's later requests, so the erased text can still reach that session's model. Fixing it is
  in the compile path (the render reads sources by position for the cache's sake), which this task left alone.
  §5.6's "invalidates cached contexts that included the node" is the rule it would need.
- **An erased episode is not imported again** (rejected, naming the erase). Re-importing a corrected tag after an
  erase would need new node ids or a generation in them; a new tag name works today, with new episode ids.
- **The hash is checked strictly** (both of Python's `ensure_ascii` forms; Python's float repr). If the pipeline
  hashes some other way, every line is rejected with `hash does not match the episode's canonical JSON`; the fix is
  in `episode::canonical`. Two lines a Python generator wrote are in the tests.
- **The index's `EXTRACTOR_VERSION` is not bumped**: what it indexes of every node already stored is unchanged (no
  store has an `imported` or `erased` node yet), and a bump would rebuild every index.
- **Health's session count includes imported sessions** (10,050 on the live check). `session.list` hides them;
  `health` was left as it counts keys.
- **No edges.** The summary's citations are in its body (node ids), not `derived_from` edges, so activation's
  adjacency does not walk them; the erase has no edges to tombstone.
- **The memory pass** would treat an imported message as a message when it reads an imported session; it does not
  read them yet (none takes a turn).
- **The cockpit** would need: a view of `import.list`; a way to browse a tag's sessions (`session.list` hides them,
  so a paged `import.sessions { tag, before, n }` read over the `import:<tag>` scope); the history view's three new
  node kinds (`imported`, `imported_summary`, `erased`; `node.info`'s `detail` carries integrity, source, unit,
  sha256, cites, the receipt); and the erase as the owner's act with its receipt. The TypeScript types are generated.
- **Docs the maintainer should write**: the spec's Part III item for theseus-0lrr.6; `docs/status.md`; and
  `docs/design/m6-memory.md` §2.15 (imported history: private, testimony, outside text excluded by default, erasure
  by tombstone) and §2.8 (store shapes: format 23's session field and bodies).

## The gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, on the tree of each commit:
fmt, shape, features, clippy, cockpit, the test build and the reader rule passed; the suite ran 2,834 tests: 2,801
passed, 33 failed, all of them the known L1-as-root tests (theseus-sandbox's contract tests and `spawn_100`,
theseusd's `sandbox` tests; theseus-pv6i). The phases after the suite I ran myself: the protocol types (staged,
clean), the turn bench (5 and 9 frames, ok), and `cargo deny --offline check` (advisories, bans, licences, sources
ok). The lifecycle and jobs benches are skipped under `THESEUS_GATE_NO_BENCH`; the lifecycle bench ran A against B
above. In the first per-crate suite run, `term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one` failed once
(theseus-ynia, known); it passed in the gate's suite.

The gate ran three times to completion: on `4be682e4`'s tree, and twice on `f306c939`'s (the first of those stopped
in the test build: building main's binaries for the A/B bench in a worktree sharing this target dir replaced the
workspace crates' artifacts, the trap AGENTS.md names; touching the sources and running again built them from this
tree, and the suite then gave the same 2,801 and 33). On `f306c939`: the turn bench 5 and 9 frames, deny ok, the
protocol types clean.

The VM's disk filled during the first gate (the gate builds with the workspace's features, a second copy of every
crate beside my per-crate runs, and 17 GB of incremental caches); I removed `target/debug/incremental` and stale
test binaries and ran with `CARGO_INCREMENTAL=0`.
