# CLOUD REPORT: threads-home-claim (theseus-98bz.6)

Branch `cloud/20261021-threads-home-claim`, from `integ/threads` at `6d4e0dd9` (plus the task commit `35bb7d8`).
One step, one commit: `4ab5791` (threads: a home turn that served one thread becomes that thread's memory).
No line ceiling is passed: `crates/theseus-protocol/src/lib.rs` stays at 2,730 (the method went on the existing
`THREAD_*` line), and `crates/theseus-discord/src/runtime.rs` is untouched.

## What I found

- `thread_routed_turn` (`rpc/thread_flow.rs`) runs after every routed turn and knows whether it ran at home
  (`r.slug == None`). The turn's result already carries what it wrote (`TurnSubmitResult.files`: `changed`, `left`,
  each capped at 8 with `more`, scrubbed, and the turn's `dir`), so no new bookkeeping was needed.
- The lens already outranks a node's session with `moved.of.<node>` (`ThreadScene::moved_to`), and the home's compile
  reads only `Body::Moved` nodes, so writing the key alone changes recall and nothing else. A claim is therefore a
  `MovedTo` whose `split` id begins `claim_`.
- Readers of the key: `marked_moved` (history), `refuse_moved` (split), the unsplit's null write, recall's header
  (`recall_step.rs` ~529, which credits the thread: right for a claim), `thread_recall.rs` (not edited). The cockpit's
  `moved_to` (progressLine.ts) is a work view's, a different field.

## What I changed (`4ab5791`)

- `rpc/home_claim.rs` (new): `Core::claim_home_turn` (called once, on the line after `let Some(res) = res else { return };`
  in `thread_routed_turn`; nothing else in that file changed). It returns before any store read when the turn ran in a
  thread, the switch is off, the result has no files, either list has `more > 0`, or there is no live thread with
  neither `sealed` nor `seal_pending`. `thread_of` (pure, unit-tested) matches each path: a thread's `files` entry equal
  to it, or a `roots` entry it lies under by components; absolute and `~/` entries as written (`misroute::normal`,
  `fold`, now `pub(crate)`), relative entries (a bare `tarnlib`) under the turn's directory (`files.dir`, the home
  session's directory). Unmatched paths are ignored; a path two threads hold, or paths of two threads, claim nothing.
  The turn's nodes are paged back from the home session's newest (64 a page through the index's `s:<session>` tag,
  newer turns' nodes passed over, stop at the first older turn's node, at most 1,024 read; past that nothing is
  claimed). A node whose key already holds a value is left. One `store.append`: each node's key, the record
  `thread.claim.<id>` (`ClaimRecord`: thread, session, turn, nodes, paths, at, undone) and the `thread.claimed` row;
  its narrative line is announced after. Nothing is posted.
- `thread.unclaim` (`THREAD_UNCLAIM` on the existing method line; `ThreadUnclaimParams`/`Result` in
  `theseus-protocol/src/threads.rs`, added to an existing ts.rs line; routed in `rpc_threads`; judged as
  `judge_thread`, the owner from a private place). Writes null only where the key still equals this claim's
  `MovedTo`, counts the rest as `kept`, marks the record undone, writes `thread.unclaimed`. An unknown claim is
  NOT_FOUND and an undone one invalid, both naming `thread.unclaim`'s `claim`.
- Ledger kinds `ThreadClaimed`, `ThreadUnclaimed` after `ThreadUnmerged`; facts beside `ThreadUnsplit` in
  `fact/thread.rs`, listed in `fact/mod.rs`.
- Readers: `marked_moved` gives a claim `claimed_by` + `claim` (never `moved_to`); `refuse_moved` lets a split take a
  claimed node (its `MovedTo` replaces the claim's; the unclaim then keeps it). `moved.rs` gains `CLAIM` and
  `MovedTo::is_claim`.
- CLI: `theseus thread unclaim ID` (`thread/split.rs`, beside `unsplit`; in `client::OPERATORS`, refused in a job);
  history prints `[claimed by «pr-round», claim_…]`.
- Config: `[threads] claim_home_turns = true` after `link_proposals` (field, `Default`, module doc, template line).
- `cockpit/src/protocol.gen/` regenerated (two new types and index.ts). AGENTS.md: one new paragraph in
  theseus-core's guide after "Linked threads".

**Store format: unchanged at 48.** The claim writes the existing `MovedTo` value under the existing `moved.of.*` key
family and one new META key family plus two ledger kinds; the split (format 47) and links bumped only for the new
stored fields (`Body::Moved`, `ThreadInfo.links`), and the store guide says a new record kind needs no bump. No stored
record gained a field.

**Speed.** A claim on a test turn (4 nodes: one page read, 4 key reads, one frame) took 1.86, 1.71 and 1.59 ms
(debug build, timed around `try_claim` in a throwaway run, not committed). It runs after `runner.run` returns, so after
the reply has streamed, but before `turn.submit`'s answer goes back. A turn in a thread, an unrouted place, a turn with
no files, or no live thread costs a field test or two. `tests_m3::a_plain_turn_stays_within_its_frame_budget`, the
first-request tests and the turn bench pass unchanged (bench: plain p50 28.9 ms; first requests cli 7,332, dm 9,411).

## How I proved it

Tests (`rpc/tests_home_claim.rs`, whole cores with the scripted provider and the fake Jev; `home_claim::tests`; the CLI's
`render::history` and `thread::split` tests):

| test | what |
|---|---|
| `a_home_turn_that_served_a_thread_is_claimed` (key) | ask, answered `home` over a second connection; the turn writes `tarnlib/tarn/retry.py`; every node keyed to pr-round with one `claim_` id; one row (thread, session, node count, path); the record; lens `Pass` for pr-round, not `Pass` for sec-fix |
| `a_home_turn_claims_nothing_that_is_not_one_live_threads` | a scratch file; one file under each root; nine files (`more` = 1); pr-round sealed |
| `a_node_a_split_moved_keeps_its_splits_value` | a planted split value survives; the rest claimed |
| `an_unclaim_restores_the_home_turn_and_leaves_a_later_split` | a real split over the claimed turn succeeds, unsplit; unclaim restores null, keeps a later split's node (`kept` 1), lens verdict equals the place's own note's (the base distance), second undo and unknown claim refused |
| `a_claimed_nodes_history_says_claimed_by` | `session.history` detail `claimed_by` + `claim`, no `moved_to` |
| `with_the_switch_off_nothing_is_claimed` | off: no key, no record, no row; template line; the key loads |
| `a_path_is_the_one_thread_whose_root_or_file_holds_it` | the pure matcher (bare root, components, `~/`, files, `..`, two threads) |

`a_split_over_a_claimed_node` is covered inside the unclaim test (`split_over_the_claim`).
`tests_thread_mix` and `tests_thread_link` pass unchanged (in the gate's suite and the load runs).

Planted reverts (each: plant, `touch`, run, restore, `touch`, `git status` clean but for the step's own changes):

1. never claim (`if true || …` at the head of `claim_home_turn`) → **key test fails** (`tests_home_claim.rs:335`, `[]`).
2. `thread_of` ignores a second matching thread → `a_path_is_the_one_thread…` and `a_home_turn_claims_nothing…` fail.
3. `more > 0` check off → `a_home_turn_claims_nothing…` fails ("unseen paths").
4. sealed filter removed → `a_home_turn_claims_nothing…` fails (the sealed case).
5. existing key not respected → `a_node_a_split_moved_keeps_its_splits_value` fails.
6. unclaim writes null unconditionally → `an_unclaim_restores…` fails (kept).
7. undone check off → `an_unclaim_restores…` fails (second undo answered Ok).
8. history always `moved_to` → `a_claimed_nodes_history_says_claimed_by` fails.
9. `refuse_moved` refuses claims → `an_unclaim_restores…` fails ("a split over a claim: the part chosen holds …").
10. switch ignored → `with_the_switch_off_nothing_is_claimed` fails.

Load: the claim tests with `tests_thread_mix` and `tests_thread_link` (22 tests) at `nice -n 19` beside four busy loops
at nice 0, three runs: 22/22 each time.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (with `CARGO_INCREMENTAL=0`, see below): door, keel, fmt,
shape, features, clippy, clippy-probe, cockpit, test build and the reader rule pass. The suite: 7,352 run, 7,319 passed,
**33 failed, all the known L1/root ones** (theseus-pv6i): theseus-sandbox's `bench spawn_100` and its 19 contract tests,
and 13 of theseusd's `sandbox` tests. No other failure, no retry. Then `machine_checks`' later phases by hand: protocol
types clean (after `git add`), lifecycle and jobs benches skipped (no-bench), `theseus-sim bench turn --check --runs 5
--burst 0` ok, size skipped (no release build), and `cargo deny --offline check`: advisories, bans, licenses, sources
ok (the database fetched at setup).

Keel guard, `python3 scripts/keel-guard.py --base origin/integ/threads --head HEAD --quiet`: empty output, exit 0.

VM note: the disk allowance filled once (a second build of every crate after switching incremental off); I removed
`target/` and rebuilt with `CARGO_INCREMENTAL=0`, which the gate runs fine under.

## Live check for the maintainer (a scratch daemon, never the owner's)

On a scratch daemon of this build with threads on and a fresh state dir, in a private place (the CLI is one):

```
theseus thread new pr-round --title "pr round"
theseus thread edit pr-round --root tarnlib        # or a whole path; a bare name is under the session's directory
theseus thread new sec-fix --title "security fix"
theseus thread edit sec-fix --root secsvc
cd <a scratch dir holding tarnlib/ and secsvc/>
theseus ask "make the retry in tarnlib/tarn/retry.py back off twice as long"
```

When asked which thread, answer `h` (home). Then:

- `theseus history <home session>` shows the turn's lines ending `[claimed by «pr-round», claim_…]`;
  `theseus ledger window` (or the session's rows) shows one `thread.claimed` with the path.
- `theseus ask --thread pr-round "what did we change in the retry?"`: its recall admits the home turn's notes
  (the recall row's items include them, headed `[from thread «pr round» …]` when read from elsewhere); the same
  question in sec-fix finds them another thread's.
- `theseus thread unclaim claim_…`: prints `Undid claim_…: N nodes of ses_… belong to no thread again …`; history loses
  the mark; a second run says it was undone already.

(`thread edit --root` and `--file` replace those lists, as `theseus thread edit --help` says.)

## Left, uncertain, for the owner

- **Unseen paths claim nothing.** A turn that wrote more than 8 files (or left more than 8) is never claimed, since the
  ninth could be another thread's. Raising `files_left::MAX_PATHS` or carrying the whole list would widen it.
- **Claims are rare until 98bz.5 joins**: working sets fill today only by the owner, an import or a split, so most
  threads have no roots or files to match. Expected.
- **Race**: the claim reads the keys and writes in one `store.append`, not under the home execution's lock; a split
  landing in the millisecond between would have its keys overwritten by the claim. A split refuses while a turn runs in
  its source, and the claim runs right after the turn, so the window is small; `kernel.frame` over the home execution
  would close it at the cost of that lock.
- **An unsplit after a split over a claim** writes null for its nodes, so the claim does not come back (the claim
  record then reports those nodes as `kept`). I think that is right: the owner moved them explicitly.
- A turn whose nodes number more than 1,024 is not claimed (bounded read).
- Follow-ups: Jev's post-turn project answer as a second trigger (plan row 7); a turn that only read a thread's files
  (no write) is not claimed; a Discord `/thread unclaim` and a cockpit button.
- **Docs** (the maintainer's): the spec's Part III item for 98bz.6; `docs/status.md`; the threads design (home split's
  section) should say a home turn can be claimed, that `history` shows "claimed by", and name `claim_home_turns` and
  `theseus thread unclaim`.

<!-- REPORT COMPLETE -->
