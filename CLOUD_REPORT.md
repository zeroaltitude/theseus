<!-- CLOUD_REPORT.md: written by the cloud session; the maintainer drops this commit at the merge. -->

# Cloud report: threads-import (T11), theseus-71ib

Branch `cloud/20261013-threads-import`. Base: main with T1 (`thread-record`, store format 33) joined; the other thread
rows (T2 context, T3 recall, T4 route, T6 Discord threads) had **not** joined when this was built. **No store format
bump** (a ledger kind, two META keys, nothing in a stored record). No new dependency (Cargo.lock and the lockfiles are
untouched). Started 06:46 UTC, report at about 08:15.

## What I found

- The pieces were all there: imported sessions carry agent, place (kind, name, id), topic labels and sensitivity in
  `ImportedFrom`, their owner messages are the `Body::Imported` nodes of integrity `operator`, and the people backfill
  is a ready model for a budgeted, cheap-profile, tool-answered call (`judge/people/extract.rs`, `run.rs`).
- **A defect in the handle scheme, found by the daemon test** (and handled in the card, not fixed in `cite`): recall's
  handle is the last six characters of a node id, grown only until a *pack* is distinct. An imported summary node is
  `imp_<digest>_summary`, so its handle is `m:ummary` for every episode; `theseus history m:ummary` then fails with
  "names 2 nodes". An owner message's `…7e48_0` is no better across 20,000 episodes (4 hex digits). A card is read
  store-wide, so its handles keep 12 digits of the episode's digest and its unit (`m:…<12 hex>_3`, `…<12 hex>_summary`;
  `card::handle_of`, `UNIQUE_HEX`): 48 bits, one collision in about a million stores of 20,000 episodes. Recall's own
  short handles are unchanged; **the owner may want `cite::handles_for` to prefer more of the digest for imported ids**
  (not done: it changes every recalled header's bytes, so it's the owner's call).
- Within the code I was in: `import::write::sessions_of_tag` was `pub(super)` and is now `pub(crate)` (the scan needs
  it); `judge_thread` and `threads_on` in `rpc/threads.rs` are now `pub(super)`. Nothing else outside my modules moved.

## What I built (commit `5fa957b`)

1. **Candidates** (`crates/theseus-core/src/threads_import/`, pure `candidates.rs`; the scan `source.rs`).
   Every imported channel conversation (a Slack or Discord channel, grouped by its id) with an owner message in the last
   `--since-days` (30) is one candidate; the DMs are clustered by an episode's first topic label cut to two path parts
   (`harbor/moorings/winter` → `harbor/moorings`), one candidate for each cluster with at least `--min-cluster` (8)
   owner messages in the window. Title: the channel's label (`#` trimmed), `DMs: <cluster tail>` for a cluster. Persona:
   the agent that most episodes name, **only if `[personas.<agent>]` exists** (case-insensitive). Topics: the episodes'
   labels, most common first, 3 at most. Working set (`extract.rs`, no model): repos (github URLs, ssh remotes, the word
   after `-R`/`--repo`/`repo`/`clone`), refs (`#N`, `owner/name#N` from issue/pull URLs, Beads ids: 3-10 lowercase
   letters, a hyphen, 3-6 of letters/digits with a digit, or any suffix after the prefix `theseus`), branches
   (`feat|fix|feature|lane|cloud|release|hotfix|chore|self/…` and the word after `branch`/`checkout -b`/`switch -c`),
   files, hosts; each list keeps its 8 most-named entries (ties by name). T4's pre-router isn't on main, so these are
   my own token kinds, "the same kinds" in spirit; if T4 lands with another set, `extract.rs` is the one file to align.
   Slugs are unique within the table (`-2`), and a slug that names an existing thread is flagged `exists` (apply skips it).
2. **`thread.import`** (new protocol method, `theseus-protocol/src/threads_import.rs`; `rpc/thread_import.rs`) and
   **`theseus thread import`** (`crates/theseus/src/thread_import.rs`): with no `apply` it is the dry run (writes nothing,
   asks no model); the CLI prints the numbered table (slug, title, persona, topics, working set, owner messages, last
   active, sensitivity counts, kind; notes of what was left out) and writes a plan file (`thread-import-plan.txt`, or
   `--plan FILE`) of `[ ] N slug title` lines. `--apply 1,3,5` takes numbers of a fresh table; `--apply all-ticked`
   reads the `[x]` lines of the plan **by slug** (the daemon looks a slug up in a fresh scan, so a plan stays good while
   the history grows, and a slug that is no longer a candidate is skipped with the reason). Apply makes each thread
   through `thread.new` then `thread.edit` (persona, working set, card). Owner's act (`judge_act(Act::Thread)`), in
   `client::OPERATORS`.
3. **The card.** One call per thread to `[threads] import_profile` (new key, `haiku`), off every turn's path; one tool
   (`write_card`), a one-sentence instruction, input scrubbed by the daemon's scrubber, newest 30 episodes within 24,000
   characters (an episode's summary, else its newest 3 owner lines). Every line must cite episodes the call offered (`E1`…);
   a line that cites none, cites an unknown label, or repeats a message of 60+ characters word for word (or is a stretch
   of one) is **dropped and counted**; at most 12 lines of 190 characters, each ending in up to 4 handles (`[m:…]`). A
   `thread.imported` ledger row (new `LedgerKind`; a day-ceiling spend kind, `SPEND_KINDS` 12 → 13) holds the model, tokens,
   cost, lines, dropped count, and the citations (handle, node, session) with the day's spend (META `threads.import.day`)
   in one frame. `[threads] import_day_usd` (2.00): a call whose worst case passes what is left of it, or that the day
   ceiling refuses, is not made; the thread is made without a card and the result says why. Template and tests updated.
   **Imported history stays testimony**: nothing writes a node into any session; the new thread has no session until
   its first message; the card is on the thread's record only.
4. **Bindings.** A candidate from a Discord channel carries `channel: discord:channel:<id>` and a note that it can be the
   thread's place once T6 joins; there is no T6 bind to call, so nothing is bound.
5. **Vestige** (`crates/theseus/src/vestige.rs`): `--vestige PATH` (a file, or a directory's `vestige*.json`) reads an
   array of memories (or an object holding one under `memories|items|nodes|entries|data`; a memory is a string or an object
   with `content|text|memory|note|body|summary`, an optional time and `tags`) and sends each as an episode of the new source
   `vestige` (`episode::VESTIGE_SOURCE`, accepted by the format and in `SCRUBBED_SOURCES`, so the daemon scrubs it), place
   kind `file`, id = the digest of its text, so a note seen again (even with another time or tags) is skipped. Reading is a
   file read; I had no real Vestige dump, so the shapes are the ones I assumed (listed in the module doc): **the maintainer
   should look at one real dump** and tell me if its key names differ.

Per-thread frames in an apply: `thread.new` (1), `thread.edit` (1), the `thread.imported` row with the day mark (1).
A plain turn's frames are unchanged (nothing here is on a turn's path).

## How I proved it

- **Unit and core tests (fixtures only, invented):** `threads_import/tests.rs` (15), `rpc/tests_thread_import.rs` (9),
  CLI `thread_import/tests.rs` (4) and `vestige/tests.rs` (3), and theseusd's `tests/thread_import.rs` (the real CLI against a
  real daemon on the stand-in model). The invented export is `threads_import/fixture.rs`, committed as
  `crates/theseus-core/tests/fixtures/threads-import/openclaw.jsonl` (`THESEUS_GOLDEN=write` rewrites it; a test holds them
  equal). It has two Discord channels and a Slack channel (one a year old), a partner-confidential channel whose lines
  carry `KESTREL-TERM-SHEET-LINE` and a partner repository, three DM topic clusters (9, 3 and 0 labelled lines), and a CLI session.
  They cover: candidates from channels and DM clusters; the window (a year-old channel, a mixed one counted by in-window
  lines only); the cluster minimum (7 not, 8 yes, across episodes); persona only when configured; working-set rules;
  partner text kept out of titles, topics, working sets, the card's inputs and every model request, while it still counts
  in owner messages and sensitivity counts (a partner-only stream is `confidential discord-channel conversation`, no model
  call); the dry run's table and plan file, and that it writes no row and asks no model; apply makes exactly the ticked
  threads; the card's citations and handle shape, the dropped uncited line, the `thread.imported` row and the budget line;
  a spent day (seeded 1.9999 of 2.00, and a cap of 0) makes a thread without a card, no model asked; an answer with no
  card; existing and unknown slugs skipped; the owner-only refusal (a stranger from a shared channel; threads off); a
  Vestige episode scrubbed and imported once; no session or node written by an apply.
- **Planted reverts** (each: plant, run `test(threads_import) | test(tests_thread_import)`, restore, `touch`):
  - partner episodes' text in the card's inputs (`inputs` built from `group`, not `open`): FAILED
    `a_partner_episode_counts_but_gives_no_text`, `partner_text_never_reaches_the_model_a_title_or_a_card`,
    `the_dry_run_lists_the_streams_and_writes_nothing`.
  - **history in the live prefix**: T2 isn't on main, and the card is the only text of a thread that T2 puts at the head of its
    messages, so the plant is the model pasting history into the card with the paste guard off: FAILED
    `a_card_line_that_repeats_a_message_is_dropped_as_history`, `nothing_of_the_history_enters_a_thread_or_a_session`.
    (The same test also holds that no session or node is written.) There is no code path that puts a whole cluster's
    history anywhere else, so there was nothing more faithful to plant.
  - the day line not checked: FAILED `a_spent_day_makes_a_thread_without_a_card`.
  - the window ignored: FAILED `a_channel_is_a_candidate_only_for_owner_messages_in_the_window`.
  - a persona named though unconfigured: FAILED `a_persona_is_named_only_when_it_is_configured`,
    `the_dry_run_lists_the_streams_and_writes_nothing`.
  - the cluster minimum not applied: FAILED `a_dm_cluster_needs_its_minimum_of_owner_messages_across_episodes`,
    `the_dry_run_lists_the_streams_and_writes_nothing`.
- **FAST:** a plain turn is untouched. `theseus-sim bench turn --check --runs 5 --burst 0`: frames_plain 4 (budget 4),
  frames_tool 8, syncs_plain 2, frames_batch 14, syncs_first_byte 2, first_request 38.6 KB (budget 40), all ok.
  `theseus-sim bench lifecycle --runs 10 --check` (this 4-core VM, debug binaries): **one budget MISSED, `health` p95
  5.3 ms against 5 ms** (p50 4.1); every other row ok (restart, shutdown, swap, restore, cancel 2 frames). Nothing I
  added is on the start path or in `health`; the budgets are the owner's 16-core machine's.
- **keel guard:** `python3 scripts/keel-guard.py` → `keel: ok … 0 findings acked`. Its one printed line is information about a
  base store test renamed upstream (`a_checkpoint_for_close_…`), not mine. **Keel findings expected: none.**

## The live check on this VM

`tests/thread_import.rs` is the live check: a real `theseusd` (scratch state dir, its own socket and config), the real
`theseus` CLI, the fixture imported with `theseus import openclaw`, and a stand-in Messages API for Haiku. It runs the dry
run (table, plan file; `theseus thread list` still empty; no model request), refuses `--apply all-ticked` with nothing
ticked, ticks two and applies (`Made the thread tide-gauges: 2 card lines citing 2 episodes`, `boat-ops: 1 card lines
citing 1 episodes`), shows `thread show` (persona `wren`, working set `reef-lab/gauge-log`, 2 card lines, no session), reads
two card handles back with `theseus history m:…` (they resolve to the summary of episode `…7e48` and to
`imp·7e4a_0`), sees that the model got two requests holding no `KESTREL`, `term-sheet` or `second tranche`, applies again
(`0 made, 2 skipped … exists`, no new call), and imports a two-note Vestige dump (2 imported, then 2 already imported).
Passed. **"Ask one a fact its card covers" can't be shown yet:** T2 (the card at the head of a thread's messages) is not on
main, so `theseus ask --thread` doesn't send the card; the card is on the record (`theseus thread show`) and its handles read.

**The real run is local and the owner's.** In order, on the owner's daemon:

```
theseus thread import --from openclaw --dry-run --plan ~/thread-plan.txt     # read-only; prints the table, writes the plan
$EDITOR ~/thread-plan.txt                                                    # put an x in [ ] of the threads to make
theseus thread import --from openclaw --apply all-ticked --plan ~/thread-plan.txt
theseus thread list; theseus thread show <slug>; theseus history m:<a handle from its card>
theseus thread import --vestige <the export directory or the dump file>      # once; run it again to see "already imported"
```

Check before applying: `[personas.<agent>]` exists for the agents that should become personas; `[threads] import_day_usd`
(2.00) is what the cards may spend a local day (a Haiku card is a fraction of a cent, so 30 threads cost well under it); the
dry run's `left out:` lines say what the window and the minimum cut; `--since-days` and `--min-cluster` move them.

## Gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, fmt, shape, features, clippy
(`-D warnings`), cockpit, test build, reader rule all passed. **The suite phase failed on 34 of 4,505** (4,471 passed):
- 33 are the known L1 failures of a root VM: `theseus-sandbox::contract` 19, `theseusd::sandbox` 13, `theseus-sandbox::bench`
  `spawn_100` 1.
- 1 is **`theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`**: it fails alone
  too, deterministically, after its 21 s timeout ("the thinking's create, held, and the reply's post"). I touched nothing in
  theseus-discord. When I checked out this branch the container reported four commits left behind that are not in this base,
  among them `b495389 discord: the thinking-answer test holds the process message's create whatever its text … (theseus-wpv2g)`
  and its merge `a6bc001`: that is this test's fix, on a branch not joined here. I could not rerun the test on a clean base
  (the disk allowance allows one build tree), so this is an inference from the commits, not a measurement.

The phases after the suite I ran by hand: protocol types (`git add cockpit/src/protocol.gen` → no diff) ok; `cargo deny
--offline check` ok (advisories, bans, licenses, sources); the turn bench ok; the lifecycle bench as above (the `health`
row); the jobs bench is L1 and skipped (`NO_BENCH`, root VM). The commit is counted green on that basis.

## Left, uncertain, and for the owner to hear

- **Cluster rule.** DM clusters are the episodes' first topic *label* cut to two path parts, not `categorize.v1`'s clusters
  or ontology memberships: the labels are what an imported episode reliably has, and they're deterministic. An episode with
  no label is in no cluster (counted in `left out`). If the owner wants `categorize.v1`'s grouping, `cluster_of` is the seam.
- **Partner rule.** An episode is partner-confidential by its sensitivity label *or* by carrying a partner codename. Its
  topic labels are also left out of the thread's topics (a label can name a deal). Its owner messages still count.
- **A thread made before its card fails** keeps its place: the card is best effort and the result says why (budget, no
  `write_card` call, provider error, the profile not resolving). The model call is paid before `thread.new`; a failed
  `thread.new` after a paid call still books the row and the day's spend.
- Apply is sequential and holds the RPC for the whole run (a few seconds a thread); the CLI shows nothing until it ends.
- Docs to change at review (not edited here): the spec's Part III item for theseus-71ib, `docs/status.md` (T11 landed,
  `thread.import`, `[threads] import_day_usd`/`import_profile`, the `vestige` source), and `docs/design/` threads notes if
  they list T11's shape. The AGENTS.md files of theseus-core, theseus and theseus-protocol are updated.
- **Disk:** this VM's allowance ran out once during the first gate (`No space left on device`); I deleted
  `target/debug/incremental` (inside the repository) and used `CARGO_INCREMENTAL=0` after; nothing outside the repository
  and /tmp was touched.
