# CLOUD_REPORT: thread-merge-split (T8, theseus-vt9k)

Branch `cloud/20261013-thread-merge-split`, cloned from main at `3f1f5248` (store format 33). No store-format bump: the
fields (`merged_into`, `split_from`) were T1's, and the one new stored thing is a META key. T3 (seals, provenance
headers) and T4 (router) had not joined; the cockpit has no thread view yet.

## What was built

All four items. Commits: `75facc71` (the feature), `1ca30891` (follow-ups from the full run), `4a0be407` (fmt),
`e180cb80`, `353c2070` (clippy's line limit).

1. **`thread.merge {from, into}`** (`theseus thread merge a b`, Discord `/thread merge`). One frame holds B's record,
   A's record, the META `thread.merge.<a>` (what the undo needs) and the `thread.merged` row.
   - B's card gets A's card quoted under `merged from «a», <date>:` (at most 6 lines while B has lines of its own, always
     within 12). A's whole card is in the row and stays in A's record.
   - Working sets are unioned, topics too (3 at most, the rest returned in `topics_left_out` for the owner to pick).
   - A seal, or a pending seal, on either thread ends up on B.
   - A is retired: `state: done`, `merged_into: B`, surfaces cleared. The surfaces it had are named in the row's
     `archived_surfaces`, because T6 (Discord threads) has not joined and nothing writes `surfaces` yet.
   - A's session then gets a note at its tail and is retired, a frame each. B's session is untouched: its card snapshot
     is T2's, so B shows the merge at its next recompile, never mid-warm (tested).
   - `Board::live_of` follows `merged_into` to the live thread (the distance rule's reader; the merge guard uses it).
     `Board::links_of` feeds `ThreadRow.merged_from` and `split_to`.
2. **`thread.split {from, slug, range | positions}`** (`theseus thread split a new --first @A --last @B` or `--at @N`).
   - New thread C: its session is opened first (`opened_from` A, so it holds what A holds), then a brief and one
     `Body::Arrangement` of the chosen messages (verbatim, role `context`, an edge to each quoted node). Then the thread
     record, its place record and the `thread.split` row go in one frame with the session already bound
     (`Threads::write_bound`). A then gets one note node at its tail, "Split to «c» … Nothing here was moved or removed".
   - C inherits topics, persona and seal, and has `split_from = A`. Its working set is read from what the chosen turns
     did (paths, URL hosts, `#` refs), not guessed from prose.
   - **The model's tool** `thread.split {slug, title?, goal?, from, to?}` is offered only in a thread's session. `from`
     (and `to`) are quotes, resolved as an arrangement's are, because the model has no WAL positions. It stops before the
     request that asked for it. It waits for the owner's tap at every posture (`Plan::authority`), and its result is the
     note A keeps.
3. **Undo.** `thread.unmerge {slug}` is one frame: A's state and surfaces come back, B gets a card line, A's session is
   reopened. B keeps what it took, a seal included. A split is undone by `thread.done` on C (nothing of A moved). Both
   are in the CLI help.
4. **History.** `theseus thread show` prints `merged into`, `merged from`, `split from`, `split to`. The TypeScript is
   regenerated (`ThreadMerge*`, `ThreadSplit*`, `ThreadRow`'s two lists). **No cockpit view changed: it has no thread
   view yet.**

Changes in code I did not own:
- `compiler/situation.rs`: a new `Class::Quoted` (an arrangement authored `thread:<source>`) that every situation admits.
  Without it a conversation's first compile fails `context_unadmitted`. A task's arrangement is still refused at
  conversation start, so `a_piece_its_situation_does_not_admit_fails_naming_it` is untouched.
- `task_graph/tools.rs`: `authority_for` cleared any tool's `Plan::authority` unless the task rule said it waits. It now
  applies only to task edits, so `thread.split` always asks. Behaviour for task tools is unchanged.
- `rpc/explain.rs`: the probe plan carries the tool's authority, so `policy.explain` says `approve`.
- `Threads` holds a `Weak<Core>` (`attach`, wired in `rpc/mod.rs`) so the async tool can open a thread.

## Proof

- **Tests added (about 60):** `rpc/tests_thread_join.rs` (17), `thread_split::tests` (7), `thread_join::tests` (4), the
  CLI's 3, Discord's 2.
  - Merge: union and retire in exactly one frame, the board and a restart agree.
  - Refusals: self, missing, done target, already merged, target merged away.
  - Nothing deleted: every node of A is the same, plus one note, and A's session is retired.
  - Recall from B finds A's fact with A's session as provenance.
  - Seal inherited both ways, and by a split.
  - B's request bytes unchanged until a recompile.
  - Unmerge restores state, session and surfaces, and notes B.
  - Split quotes exactly the chosen turns, in order, by id.
  - A's earlier request is a byte prefix of its next one.
  - A hold is inherited, and an orphan session is retired.
  - The tool is offered only in a thread, waits for the tap, and splits by quotes.
- **Planted reverts** (restored with `git checkout` and `touch`, tree clean afterwards):

  | Bug planted | Caught by |
  |---|---|
  | merge also moves A's session to B | `a_merge_deletes_nothing_of_the_retired_thread` |
  | split range loses its last message | 6 tests (`a_range_quotes_exactly…`, `a_split_quotes_exactly…`, …) |
  | split also retires the source (moves) | `a_split_of_a_holding_thread_holds_and_an_unused_session_is_retired` |
  | unmerge leaves A done | `an_unmerge_restores_the_retired_thread…` |
  | merge drops the seal | `a_merge_with_a_sealed_thread_is_sealed…` |
  | split drops the seal | same test |
  | `authority_for` reverted | `the_tool_waits_for_the_owners_tap…` |

  Weak spot: the retire-the-source plant is caught only by the holding test, not the main split test; I did not strengthen it.
- **Runs:** targeted core tests 64/64 pass. Full workspace `cargo nextest run --workspace`: 4639 run, 4606 passed,
  **33 failed, all the known L1 root failures** (theseus-sandbox contract and `spawn_100`, theseusd `sandbox`). `cargo
  clippy --workspace --all-targets -D warnings`, `cargo fmt --check`, `scripts/shape.sh` clean. Cockpit lint (existing
  warnings only), tests and build pass.
- **Not run under load**, and not needed: no timing or race claim is made. Not run: `cargo deny`, the speed benches
  beyond the lifecycle one.
- **FAST:** a plain turn's path gets no new work (one `retain` over two names, which it already did for one). The
  frame-budget and turn tests pass in the suite.
  `theseus-sim bench lifecycle --runs 10 --check` on this VM (a debug build): start p95 25.8 ms (budget 50), shutdown 28.4
  (100), SIGKILL restart 26.0 (150), swap 29.1 (200) all ok. **Two rows missed by a hair: the push seed p95 2.1 vs 2 ms and
  health p95 5.1 vs 5 ms.** Nothing in this change touches either path; budgets are the owner's machine's, so I left them.
  Please rerun there.
- **A bug found in code I was in:** `authority_for` (above) silently dropped any non-task tool's authority.

## Keel findings expected

None from my commits. `python3 scripts/keel-guard.py` reports one finding **already on the branch from main**:
`crates/theseus-sim/src/lifecycle.rs:155` budget-raised (`"seed" => Some(250.0)`), whose ack commit `1c2beb2747`'s
signature does not verify (`%G? is E`). This made the full gate fail at `keel`, so I ran the later phases by hand (fmt,
shape, clippy, cockpit, suite, lifecycle). The maintainer should see whether that is a clone artefact.

## Live check (done here, on a scratch daemon, with `theseus-sim fake-model --rules --dump`)

Run in `/tmp/live`; it is gone with the VM. To repeat:
```
theseus-sim fake-model --addr 127.0.0.1:19777 --rules rules.json --dump dump &   # rules answer by keyword
ANTHROPIC_API_KEY=x theseusd --config cfg.toml --socket d.sock --state-dir state & # cfg = bench/theseus-bench.toml with [model] api_base set
theseus --socket d.sock thread new harbour
for q in "tide …" ×5, "disk …" ×5; do theseus --socket d.sock ask --thread harbour --no-stream "$q"; done
theseus --socket d.sock --json history SES | …                  # positions: 171 and 255 were the last four disk turns
theseus --socket d.sock thread split harbour disk --first @171 --last @255
theseus --socket d.sock ask --thread disk --no-stream "what was the biggest file again"
theseus --socket d.sock thread show harbour   # shows `split to: disk`
```
Result: "8 messages quoted, @171 to @255". C's first request (in `dump/`) held the four disk turns verbatim and the brief,
and none of the tide turns. A's history ends with the "Split to «disk»" note. Then `thread merge weir pump`, an ask in
`pump`, `thread show` (`merged from: weir`; `weir (done) … merged into: pump`), an ask in `weir` refused with "is done",
`thread unmerge weir` ("active again"), `thread done disk`.

**Limits of that check:** the stand-in answers by keyword, so "C answers from them" was proved by what the request
carried, not by model reasoning. In the merge check, A's facts reached B's request only through the quoted card (A had no
card), since the index tender was off. Facts reaching B by recall are covered by the unit test, not the live one.

## Uncertain, and for the owner

- **A merge is quoted, not copied.** B's card gets at most 6 quoted lines of A's. The rest of A reaches B through recall
  (A's nodes stay A's), which needs memory on. Say if the merge should instead write A's card into B's session as a note.
- **The split arrangement's preamble** is the task wording ("quoted … that define this work"), followed by a brief that
  says what it really is. A `preface` field on the arrangement would fix it but is a store-format bump; I did not.
- **Chosen turns are capped** at 400 messages / 400,000 characters (constants in `thread_split.rs`, not config).
- **Archiving a Discord surface** is only the row's `archived_surfaces`, until T6's helper exists.
- **Unmerge leaves a seal B inherited.** A seal only loosens by the owner.
- **Split is four frames, not one:** open, nodes, thread, note. A crash between them leaves at worst an unused retired
  session or a thread without its note.
- **A split while A has a turn running** cannot append the note (reported as `noted: false`); the tool path is
  unaffected, its result is the note.
- A pre-existing lifecycle bench miss may be noise on this VM (above).

## Docs to change (I did not touch them)

Spec Part III: a T8 item (merge/split/undo, the META key `thread.merge.<slug>`, the `Quoted` class, the three ledger
kinds `thread.merged|unmerged|split`, the three methods). `docs/status.md`: the thread row. `crates/theseus-core/AGENTS.md`
is updated here.
