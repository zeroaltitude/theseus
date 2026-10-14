<!-- CLOUD_REPORT.md: the report of cloud/20261013-deferred-tools (theseus-7m7w.2). Dropped at the merge, as CLOUD_TASK.md is. -->

# deferred-tools (theseus-7m7w.2): the report

Branch `cloud/20261013-deferred-tools`, from `70c9b97` on main `e9ea344` (store format 32). One step, four code commits.

| commit | what |
|---|---|
| `2e10af9` | core: families, selector, board, provider, the cache-holding fork, misses, `tool.defer`, health, template, store format 33 |
| `4c77fb0` | cli, cockpit: `theseus tools` families per place, health's `tools:` line, the cockpit's Tool families panel |
| `b6597af` | three defects found in review (a compaction's request, the search's streamed input, `context.explain`), the metric, the restart test, leaner defaults |
| `9849b5e` | `THESEUS_DEFER_DUMP`, the first request of a deferred daemon, for `count_tokens` with a key |

## What I found

- The tool set is about **15,500 tokens** of Theseus's first request (the registry of a test daemon; no AWS, MCP or language
  server, which only add). By family, at 2.4 bytes a token: tasks 5,912 (8 tools), files 3,655 (11), commands 1,721 (4),
  terminals 1,264, channels 856, memory 745, web 683, extensions 672. The tasks family alone is more than a third.
- Estimated first request, whole to as sent (`each_place_kinds_first_request_sends_less_than_the_whole_catalog`, run with
  `--no-capture`; tokens at the Claude rate, system and messages included):

  | place kind | tools whole | tools as sent | first request whole | first request as sent |
  |---|---|---|---|---|
  | cli | 15,508 | 6,478 | 16,273 | 7,250 |
  | headless | 15,508 | 6,997 | | |
  | task | 15,508 | 5,733 | | |
  | dm | 15,508 | 13,929 | 16,273 | 14,697 |
  | channel | 15,508 | 13,929 | 10,571 | 10,571 (a shared place is offered the public tools only, and loads all of them) |

  "As sent" counts a deferred tool as the constant 93 tokens and the search tool 264 (the lane's figures), the loaded
  families by their size. Request *bytes* do not fall: a deferred tool's definition is still in the body. The
  estimator, `ProviderRequest::census`, was counting those bytes as tokens (about +9,000 tokens in every reservation and
  compaction threshold); it now counts the constant (`tools_defer::tools_bytes`, shared with `context.explain`).
- **Three defects in code I was in**, each with a test and a planted revert (below): a compaction's request carried the
  whole catalog undeferred while its manifest named the deferred set (and, the catalog being bigger, the summary often did
  not fit and the ring stood); the provider assembled a streamed `tool_use` input only, so the tool search's own
  `server_tool_use` went back to the API with `input: {}`; `context.explain` compared the whole catalog's digest and said
  `changed`.
- `compiler.rs` had one line left: `compile` forks its spec the way it already forked the ontology's memberships, in
  `tools_defer::forks`. That is the whole of the compiler's change (2,559 of 2,560).

## What it does

- **Families** (`tools_defer/family.rs`): files (fs, git, text, file.read), commands (proc.run, job.*), web, memory,
  tasks (task.*, wake.*), terminals, code (lsp), channels, aws, extensions, self, and one `mcp:<server>` each; a wire name
  the table does not know is `other` and always loads (a test fails a registered tool in it). `Tool::family()` stays the
  name's first part. Each family has a summary line; a new tool's wire prefix joins a family there (writer A's
  `self.propose_change` is already `self`; `owner_ask` and `thread.*` are mapped to `tasks`, **the joiner of the second
  should confirm those two**).
- **Place kinds** (`PlaceKind`): cli, dm, channel, task, headless. Headless is the daemon's own fact (`--stdio`:
  `!failed_turns.driver_retries()`), never a harness's or a benchmark's name; a test reads the feature's files and the
  template's section for such a name.
- **Selection** (`select.rs`, pure): `always_loaded` (files, commands), the kind's committed defaults, then the ledger:
  loaded when used in `load_share` (0.10) of the kind's turns over `window_days` (14) with at least 10 turns; a default
  hides when the kind has been watched for `hide_after_days` (14), has 10 turns in that window and the family was not used.
  A young or empty ledger hides nothing. A search counts as a use. Defaults: cli code + memory; dm and channel memory,
  tasks, web, channels; task code; headless code + terminals. `aws`, extensions, self and every MCP server are deferred
  everywhere until use loads them.
- **The cache holds** (`tools_defer::forks`, `Manifest.loaded`): an append takes the loaded set its current compilation
  recorded; a recompile, a clearing pass, a ring, a compaction take the fresh one. The request's tool bytes are identical
  turn after turn (`the_loaded_set_holds_across_a_conversation_and_changes_where_it_recompiles`; the daemon test adds a
  restart). A compilation from before format 33 appends with every tool. Selection runs once a turn from counts in memory
  (a few map reads); nothing per call.
- **Provider**: catalog `tool_search` (true for the Claude models but Haiku 4.5 and 5.5, false for GLM). A supporting
  model gets `defer_loading: true` on a hidden family's tools and `{"type": "tool_search_tool_bm25_20251119", "name":
  "tool_search_tool_bm25"}` first in `tools`, no beta header (the API reference in this session's skill); never all
  tools deferred (the API refuses it; the guard hides nothing then). Others get the hidden tools left out and `tools_find
  {query}`, whose answer names the matching families, their tools and that they load at the next compaction or a new
  conversation; `session.recompile` also takes effect.
- **Misses** (rows `tools.searched`, `tools.missed`, `tools.used`; metric `theseus.tools.events` by event, place and
  family): a call to a hidden tool no search of the conversation surfaced is not run and is answered `Not run: <tool> is
  not loaded in this conversation … Search for it first: call tool_search_tool_bm25 …`; a search records its query, what
  it surfaced, and whether a call followed in its turn; a turn's row lists the families loaded, called, searched and
  loaded-but-unused. `theseus tools` (families per place, `-v` the tools), health's `tools:` line (warns on misses), the
  cockpit's Actions panel, `tool.defer`.
- **Seam**: `Board::select(.., extra)` takes the per-message set for Jev's branches.
- `[tools.defer]` (`enabled`, `always_loaded`, `load_share`, `window_days`, `hide_after_days`) in the template with its
  test; `Parts::for_tests` and the daemon tests' `safe_note` turn it off so the other ~4,400 tests keep their meaning (a stand-in model
  that calls `term_open` in a CLI session would now be answered with the search's words); `tests_defer.rs` and
  `theseusd/tests/defer.rs` turn it on.

## How I proved it

Commands are `cargo nextest run`; `CARGO_INCREMENTAL=0` throughout (incremental builds filled this VM's disk twice).

- Unit: `-p theseus-core -E 'test(tools_defer) | test(config::defer)'`: 47 tests with the whole-turn ones (families, selection
  by days and share, the board, `tools_find`, `resolve`, the scan of a response, the listing).
- Whole turns, `-E 'test(tests_defer)'` (15): defer_loading and the search tool first; no mechanism: `tools_find`, left out;
  `tools_find` then a recompile loads the family, another conversation lacks it; the set holds across turns with the
  ledger changing under it and changes at a recompile; a family unused for 30 days hides; headless and DM defaults; a
  call to a hidden tool is answered with the search and counted; a search lets a call through and its blocks go back to the
  provider; the ledger's searches promote a family; off means everything loads; the rows rebuild the counts (the board
  built from the ledger equals the live one); `context.explain`; the census counts deferred tools as a constant; first
  request per kind.
- Others: `tests_compaction::a_compaction_sends_the_tools_its_manifest_names`,
  `provider::tests::a_server_tools_streamed_input_lands_in_its_block`, `telemetry::tests_tools`, `tests_layouts` (the
  format-32 compilation sample), `theseusd/tests/defer.rs` (3: a real daemon's first request, the miss, a restart).
  Cockpit `npm test` 376 tests, lint, build.
- **Planted reverts** (each restored from a copy, `touch`ed, `git status` clean; the failing tests):

  | plant | what failed |
  |---|---|
  | an append takes the fresh set (the set recomputed every turn) | `tests_defer::the_loaded_set_holds_across_a_conversation_and_changes_where_it_recompiles` |
  | `defer_unloaded` lets every call through (a deferred tool called unsurfaced runs) | `a_call_to_a_tool_nothing_found_is_answered_with_the_search_and_counted`, `the_ledgers_rows_rebuild_the_counts_after_a_restart` |
  | `resolve` omits the search tool | `tests_defer::a_hidden_familys_tools_carry_defer_loading_and_the_search_tool_is_there`, `tools_defer::tests::…search_tool_comes_first` |
  | `compact` renders from the raw spec | `a_compaction_sends_the_tools_its_manifest_names` (the strategy fell back to `ring`) |
  | no `server_tool_use` assembly | `a_server_tools_streamed_input_lands_in_its_block` (query `null`) |
  | `context.explain` reads the raw catalog | `context_explain_reads_the_tools_a_request_sent_…` (`changed`, not `same`) |
  | a benchmark's name in the feature's files | `the_defaults_name_real_families_and_no_benchmark` |
- **Under load**: no race was fixed, so no load run; the set is read once a turn under a mutex held for a few map operations.
- **The gate**, `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the tree of `b6597af` (run 7): keel ok (0
  findings), fmt, shape, features, clippy, cockpit lint/test/build, test build, reader rule, then the suite: 4,439 tests, 4,405
  passed, **34 failed**: the 33 known L1 tests (19 `theseus-sandbox::contract`, `bench::spawn_100`, 13 `theseusd::sandbox`:
  root, theseus-pv6i) and `theseus-judge builders::tests::every_builder_stays_under_its_cap_on_huge_inputs`, a 25 ms-class
  timing bound that passed alone in 1.2 s. Not a negative assertion. Run by hand after the suite: protocol types
  (`git status` clean after the protocol test), `theseus-sim bench turn --check` (every count ok; "turn_batch_p50 within 10% of its
  limit" is a warning on this VM), `cargo deny --offline check` ok.
- **The gate on the tip** (`9849b5e`, run 8; the report commit adds only this file): keel, fmt, shape, features, clippy,
  cockpit, test build and the reader rule ok; the suite 4,439 tests, 4,404 passed, **35 failed**: the same 33 L1 tests, and two
  that run 7 passed:
  `theseus-core tests_rerank::a_reranked_turn_keeps_its_frame_budget` ("a plain turn wrote 5", once; it counts the whole
  store's frames in the turn's window, passed 3 of 3 alone, and the deferral is off in its core) and
  `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`, **a flake of its own**:
  alone it passes 3 of 6 with the feature on and 1 of 6 with the test's core switched to `[tools.defer] enabled = false`
  (that core is built without `for_tests`, so deferral is on in it by default; the edit was reverted), so it is not the
  feature's. Neither is a negative assertion. The earlier run's `judge builders` and `gone_jobs` also passed alone.
- Earlier gate runs failed in shape (compiler.rs 2,562 > 2,560: fixed by a helper), cockpit lint (a function named like a
  hook), the suite (stand-in models calling deferred tools: the `for_tests`/`safe_note` decision above), clippy
  (`dispatch` at 101 lines: `tool.defer` is routed through `rpc_prefixed`; `system_parts` at 102: the tools part is its own function), and the
  disk (twice).

**Keel findings expected: none.** No test deleted or loosened, no `#[allow]`, no cap or ceiling raised. Four tests assert the
store format and moved from 32 to 33 (`store::tests`, three in `tests_layouts`, `theseusd/tests/versions.rs`, whose "newer" case is now 34).
`cache_header` (sessions and a task share one header) is **unchanged in meaning**: its daemons run with deferral off, because
with it on a task's tools differ from its parent's (below).

## The live check to run

Needs the real Messages API. A scratch daemon on a fresh state dir and its own socket (never the owner's):

```sh
# the config: your usual note plus nothing for [tools.defer] (on by default)
theseusd --config $C --socket /tmp/d/sock --state-dir /tmp/d/state &
theseus --socket /tmp/d/sock tools            # tail: "tool families per place", one block per kind of place;
                                              #   cli: files, commands loaded (always), memory, code (default), the rest deferred
theseus --socket /tmp/d/sock tools -v         # names each family's tools
theseus --socket /tmp/d/sock health | grep '^tools:'
```

1. **Tokens, before and after** (a key; `ANTHROPIC_API_KEY`). `THESEUS_DEFER_DUMP=/tmp/rep cargo nextest run -p theseusd
   --test defer` leaves the first request a real `theseusd` sent (`/tmp/rep/first-request-cli.json`, Sonnet 5.5, 34 tools,
   17 deferred). Then:

   ```sh
   jq '{model, system, messages, tools, thinking}' first-request-cli.json > deferred.json
   jq 'del(.tools[]?.defer_loading) | .tools |= map(select(.name != "tool_search_tool_bm25"))' deferred.json > whole.json
   curl -s https://api.anthropic.com/v1/messages/count_tokens -H "x-api-key: $ANTHROPIC_API_KEY" \
     -H anthropic-version:2023-06-01 -H content-type:application/json -d @whole.json     # input_tokens: the whole catalog's
   jq '.tools |= map(select(.name != "tool_search_tool_bm25"))' deferred.json | curl -s .../count_tokens -d @-   # deferred, no search tool
   jq '. + {max_tokens: 1}' deferred.json | curl -s https://api.anthropic.com/v1/messages ... -d @-
        # count_tokens rejects the search tool: its usage.input_tokens (+ cache fields) is the deferred request's, search tool included
   ```

   Expect `whole` ≈ 14,000 to 16,000 and the probe ≈ 6,500 to 8,000, a fall of 7,000 to 9,000 (this build's estimate: 16,273 →
   7,250); if `count_tokens` differs from the estimate by more than 20%, the catalog's `bytes_per_token.json` (2.4) is what to refit.
2. **A search, live.** In a CLI session on Sonnet 5.5 and on Opus 5.5: "open a terminal running python3 -q and print 2+2".
   Expect the model to search (`tools.searched` with `surfaced: ["term_open", …]`, `called: true`), `term_open` to run, and no
   `tools.missed`. `theseus ledger tail -k tools.searched -n 5`, `-k tools.missed`, `-k tools.used`. **Watch for**: a
   `pause_turn` stop reason after a search (the turn ends as `no_tool_calls`; not handled); the API's 400 on a deferred tool's
   field (`eager_input_streaming` is set on some tools and is sent on deferred ones too); a model that calls a deferred tool
   without searching (a `tools.missed` row: the answer names the search, so the next loop should search).
3. **Coding smoke and the Terminal-Bench night set, Sonnet 5.5 and Opus 5.5, before and after.** "Before" is the parent
   commit `e9ea344` (or this build with `[tools.defer] enabled = false` in the bench profile's copy); "after" is this
   build, defaults. Headless trials start a fresh state dir, so they run on the place kind's defaults (files, commands, code,
   terminals) and learn nothing. Record per arm: pass rate, dollars a trial, first request's tokens (`provider.call`
   `usage.input_tokens + cache_*` on loop 0), searches and misses (`tools.searched`, `tools.missed` per trial dir), and
   whether the misses cluster in a family (health's `tools:` line names the most missed). The row's hypothesis is
   $0.120 → under $0.10 a trial with no pass-rate loss; a family many trials search for is the evidence to add to
   `PlaceKind::Headless.defaults()`.
4. **The fallback**, on a profile whose model has no `tool_search` (GLM, Haiku): the first request lacks the hidden tools and
   carries `tools_find`. In a headless one-turn run a family `tools_find` asks for **never loads** (no boundary); see below.

## What is left, and what the owner should hear

- **Task sessions no longer share their tools with their parent** (a different kind of place). Two sessions of one kind still
  share the whole prefix. The header test therefore runs with deferral off; a task's first request is a cache miss for its
  tools and system header where a parent's tools used to be shared.
- **`tools_find` in a one-turn run.** The owner's spec loads what it finds "from the next compilation boundary"; a headless
  run on a non-supporting model has none. A `tools_find` could ask for a recompile at the next turn (one cache miss, only when
  the model asked); not built, since the spec says boundary.
- **Boundaries are the compile's**: a recompile (any trigger, `session.recompile`), a clearing pass, a ring, a compaction. A cold
  rewrite of attachments (`cold_now`) is not a compile boundary and does not change the set.
- **Memory only**: a conversation's surfaced tools and wanted families (256 conversations kept). After a restart a previously
  surfaced tool called without a search is refused once, with the search's words. The counts are rebuilt from the ledger after serving
  (`Core::warm_tools_use`, a paged read by kind; until it ends, defaults).
- **Defaults are guesses, learned away.** Cli does not load `tasks` (5,900 tokens); the first CLI session that wants `task_create` searches.
  Listed with their sizes in `theseus tools`.
- **A loaded family unused in a conversation** is counted per turn (`unused` on `tools.used`, `theseus.tools.events{unused}`), not as an end-of-conversation row.
- **`called` on a search** is within its own turn; a call in a later turn still promotes the family through `tools.used`.
- `tools.find` is in the registry, hidden from the catalog (`tools_defer::listed`), and a public tool (offered in shared places).
  A ceiling cannot hide it.
- **Docs for the maintainer to write** (I did not touch them): the spec's Part III item for 7m7w.2 (store format 33; the
  compile's fork; `tools.*` rows; `theseus.tools.events`), `docs/status.md`'s row, the version line; `docs/design/` the
  request-census row (`tools_bytes` now owns the tool bytes the census counts; its loaded/deferred column can read
  `Board`/`tool.defer`); the context-meter row (`context.explain`'s tools part is already the sent one).
- `kernel_frames.txt` and `core_output.txt` did not move: no frame is added (rows ride the turn's frames), and a daemon with deferral
  off writes none of the new rows.
