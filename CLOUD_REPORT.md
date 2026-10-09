# Cloud report: people (theseus-wy7y)

Branch `cloud/20261009-people`, cut from `78b58b8` (store format 25). Started 18:23 UTC, report at 19:55 UTC.
Every time here is from `date` on the VM (UTC).

Three commits on top of the task's own:

| Commit | Subject |
|---|---|
| `ff86dc5` | ontology: people beyond Discord DMs, with handles, merges and the import's people |
| `85e0740` | ontology: the bulk accept reads a new topic's proposal as a topic's, with its test |
| `225eb4f` | cockpit: people in the ontology's pulldowns and view, with proposals' select-all accept |

What the task asked, and what is here:

| Part | State |
|---|---|
| 1. Person categories from the transport, the import and the operator; memberships from each; the place rule first | built |
| 2. One person, many handles; merge by the operator (reversible from its row) and by an exact handle | built |
| 3. `theseus import people [TAG] [--dry-run]`, deterministic | built |
| 4. Jev proposing people (`categorize.v1` or a sibling), bulk answers, `import people TAG --propose` | **bulk answers built; the judgment and `--propose` are not** (why below) |
| 5. The cockpit: people in the pulldowns and the view, a person's page, proposals with select-all | built |
| 6. Role facts, never evaluations, in the CLI's help | built (Jev's instructions belong with part 4) |

## Step 1: people in the ontology (`ff86dc5`)

**Found.** `person` was given, `assigned_by = [transport]`, and `Ontology::check` refused any stored list of a given
kind and any category of one not from the transport. `place_category` made a person only from `discord:dm:<user>`.
The import's records hold people two ways: a message's `author` (`person:<name>` in the episode format: a person who is
not the owner; the owner is a bare name, agents are `agent:<name>`, plus `tool` and `outside`), and the episode's
`place` (`kind`, `name`, `id`). No message author carries an id, and a DM's place does not say whether it was Slack or
Discord.

**The kind's design (the owner should hear this).** `person` stays **given**, and gains a **stored side** rather than
becoming interpreted:

- Given, because its transport side is a fact read from the place at compile and never stored, and the place rule's
  shared-place walk is "the transport's memberships only" (`Walk::compose`); an interpreted person would have put a
  DM's own person through the stored path or needed a second kind.
- Its seed row now names `[transport, operator, import]` (`Kind::stores()` is true for a given kind whose row names an
  origin beside the transport; guild and channel stay `[transport]`). The operator and the import may declare a person
  and keep a session's stored `person` list; a stored list never holds a `transport` entry (refused), and the
  transport writes no list. A shared place's walk still reads only the place's given memberships: `Walk::of` adds no
  stored ones there, and `Walk::compose` filters to `transport` (both proven below).
- `per_session` stays `many` (a given fact; a channel's listed people). The import keeps at most 12 a session
  (`import::people::PER_SESSION`), the DM's party first, then by messages.
- A stored row of the old person kind (the owner editing its precedence or description) no longer checks against the
  new seed's `assigned_by` and is dropped at load with a warning, the seed serving. None is expected on the owner's
  daemon; if one is there, the warning says so.
- `compose` (`theseus-ontology/src/compose.rs`) now takes a stored person membership as usable; a test caught it
  skipping them ("`person` memberships come from the session's place, not from `import`").

**Handles and merges.**
- `Category.handles` (`discord:<id>`, `slack:<id>`, `email:<addr>` in lowercase, `name:<display name>`), checked in
  `theseus-ontology/src/person.rs`; only a person carries them. A DM's person (`person:<digits>`) holds its
  `discord:<id>` without storing it (`handles_of`).
- No two held people share a handle but a name: a write that would is refused, saying to merge. The automatic merge
  of an exact handle happens at two places: `ontology.category.add` of a person whose handle another holds adds the
  new handles and name to the held person instead of making a second one; and a DM's first bind whose `discord:<id>`
  another person holds (the import's, say) writes, in one frame as the operator, that person without the handle, the
  DM's person, and the merge of the other into it (`Core::bind_person_merging`). A display name alone never merges.
- `ontology.person.merge` (`theseus ontology person merge A B`): B keeps its id and takes A's handles (and A's name as
  a `name:` handle), memberships and guidance; A is written with `merged_into: B` and is no category, as a retired one
  is; a load leaves a merged person's old guidance unread without a warning. A DM's person (added by the transport)
  is never absorbed: `merge DM X` is refused, saying to merge X into the DM. Both having guidance is refused (person
  guidance is one line, `intent_line`). The `ontology.merged` row, scoped `ontology.merged:<absorbed>`, holds A's
  record, B's handles before, the moved sessions and their lists before; `--undo` writes those back (A held again, B's
  handles as they were, each list as before the merge).

**The import's people** (`crates/theseus-core/src/import/people.rs`, `import.people`, `theseus import people [TAG]
[--dry-run]`): reads the tag's live sessions and each one's nodes (`Store::session_nodes`), so it decodes every
imported node once: about 115,000 on the owner's daemon, on the blocking pool, the owner's act, never at a start.

- A DM's party is a person by its handle: the place's id, `discord:` when all digits (a snowflake), else `slack:`.
  This is a guess from the id's shape, since the episode format does not name the transport of a DM: worth checking
  against the owner's export (a Slack DM channel id such as `D…` would be taken as the party's handle).
- An author `person:<name>` is a person by name. A name that was the only person author in the DMs of exactly one
  party is that party everywhere (`linked`), so the DM's "pell" and the channel's "Pell" are one. Any other name is a
  person of its own, keyed by its folded name; across the ontology it is found again only among the people this tag's
  run made (idempotence), never among people someone else made.
- A party found by an exact handle among the held people uses that person (a DM's person the binding made included),
  never a second.
- Each session's list: the party, then its person authors by message count, at most 12; an operator's own entries
  kept and counted first; a list holding the same people from the same origins is not written again.
- Frames through `Board::write`, cut past 4,000 records, the machine's quiet waited for between frames, the stop
  looked for there (the topics' `write_frames`, made `pub(super)` and given the people's row, `import.people`).
- The erase: `topics::unassign` now empties every stored kind's lists (`Kind::stores()`, not `!is_given()`), and
  `people::unassign` retires each person an import made that nothing uses. A person the import made that the operator
  since edited (an added handle, say) but gave no guidance and no list is retired too: the erase cannot tell an edit
  from the import's own record. Guidance or a list keeps it.

**Bulk answers.** `ontology.proposal.accept_all` (`theseus ontology accept --kind topic|person --min-confidence X`,
and the cockpit's selection by `judgments`) accepts each matching unanswered proposal as the single accept does; a
proposal of a new category is left, with why.

**The store's format: 25 to 26.** `Category` gained `handles` and `merged_into` (serde defaults, absent when empty);
stored `person` lists are new. `tests_layouts.rs` has the format-24/25 category's layout as a literal
(`CATEGORY_BEFORE_PEOPLE`, read as META's ontology category) and keeps every byte. The store's own format test and
`theseusd/tests/versions.rs` moved to 26 (27 for "newer").

**Shared files touched** (each named, each small): `crates/theseus-protocol/src/lib.rs` (three method names appended
to two existing lines, no net line), `ts.rs` (types appended to two existing lines), `ledger.rs` (two kinds),
`crates/theseus/src/main.rs` (the `person` subcommand and the bulk accept's flags: a shared file, 45 lines),
`crates/theseus/src/client.rs` (`OPERATORS`, 27 to 30), and `rpc/import.rs`'s `prefixed`, which now routes
`ontology.person.merge` and `ontology.proposal.accept_all` to the ontology's arm (`rpc/people.rs`'s `ROUTE`), so
`rpc/server.rs`'s `dispatch` stays under clippy's length. No long file grew past its ceiling; no new config key.

**Proved.**
- theseus-ontology: 48 unit tests (6 new in `tests_people.rs`: the kind's stored side; the operator and the import
  declare and assign; a stored list never holds the transport and guild/channel keep none; handles' rules; no two
  people share a handle but a name, a DM's implicit `discord:` included; a merge moves memberships, handles and
  guidance, a load agrees, and the undo restores; a merge's refusals), plus the golden of the seed rows
  (`tests/golden/seed-kinds.json` rewritten for the person row) and the property tests. All pass.
- theseus-core `import::tests_people` (5): the fixture of three invented people across a Slack DM, a Discord DM and a
  channel gives three people with their handles and each session's memberships; a dry run counts and writes nothing;
  a second run writes nothing; a shared place's walk reads none and composes nothing while a private one composes a
  person's guidance line; a DM's person holding the id is used and the erase takes the rest back (and a reload agrees);
  a merge and its undo through the RPC; a person added with a held handle joins that person, a name alone makes a
  new one; `import.people` from no private place is refused. `tests_people_proposals` (1, in `85e0740`): bulk accept
  by kind and confidence, by selection, and a shared place's refusal.
- Planted reverts (each restored and `touch`ed, `git status` clean after):
  - `Walk::compose`'s shared-place filter made to pass everything: `a_tags_people_are_found_once_each…` failed at "the
    place rule comes first".
  - `check_category`'s handle-twin refusal switched off: `tests_people::two_people_never_share_a_handle_but_a_name`
    failed.
  - `topics::unassign` back to `!k.is_given()`: `a_dms_person_holding_the_id_is_used_and_the_erase_takes_the_rest_back`
    failed with `(0, 0)` emptied and retired.
  - The bulk accept's kind filter before `85e0740` (a new topic's proposal counted under `person`) failed
    `proposals_are_accepted_in_bulk_by_kind_confidence_or_selection` (`accepted: [], left: [the new topic's]`).

## Step 2: the bulk accept's kind (`85e0740`)

Found by its test: `--kind person` listed a new topic's proposal among the people's. A new topic's proposal is a
topic's now. The core's guide (`crates/theseus-core/AGENTS.md`, the import's paragraph) names the people's modules.

## Step 3: the cockpit (`225eb4f`)

- A session's memberships panel (`src/components/Memberships.tsx`): the pulldown groups topics and people
  (`<optgroup>`), a person shown by name and handles, with a search box over names, ids and handles; an `import`
  membership can be taken out as an operator's can.
- The Ontology view: a people panel (searchable by handle, by session count), a person's page (handles, the sessions
  that hold them from one read of every stored membership while the page is open, and "merge into this one",
  confirmed), "add a person" (handles; its placeholder says a role, never an evaluation), and a proposals panel: topics
  and people side by side, a kind filter, checkboxes, select all, Accept (`ontology.proposal.accept_all` with the
  picked judgments). New file `src/components/People.tsx`; Ontology.tsx gains 9 lines.
- Pure parts in `src/lib/people.ts`; `test/people.test.ts` (5 tests: the pulldown's groups and order, search by
  handle, a person's line, the people's order, select all by kind and confidence). The cockpit's test runner is
  `node --test` with no DOM, so the pulldown is tested through its pure grouping, not rendered. `npm run lint` (0
  errors), `npm test` (305 pass), `npm run build` pass; the gate built the bundle.

## Not built: Jev proposing people, and `import people --propose`

The judgment framework answers only choices, Nouls and scores (`theseus-judge`'s packs: 15 choice, 40 noul, 1 score
questions), and a choice's options must be ones offered: a test that scripted the fake Jev to answer a person's id
under `categorize.v1` got a topic back. "A new person (name, and a short role line from the text)" needs an answer
type that returns text, which Jev (`jev-1.13.0`) does not give today; that is a protocol change on Jev's side, the
owner's call. What would work without it, as a next step:

- A sibling pack, `people.v1`, at the same exchange-end point and mark as `categorize.v1`: a `person` choice over the
  held people (up to 50, each by name, handles and description, `options_from = "people"`, a new `Source::People`),
  with `none`; and per-person Nouls, "do these messages mention {item}?". It would propose existing people only; a new
  person needs the text answer above (or a deterministic name extractor, which this repository's rule on hand-written
  detection argues against).
- Its proposals in `ontology.proposals` beside the topics' (the read already scans by scope; it would scan
  `judge:people` too), accepted through the same path: `Proposed::category` must then find people
  (`k.stores() && assigned_by` names the operator, not `!k.is_given()`), a one-line change left out here since nothing
  can exercise it yet.
- `theseus import people TAG --propose`: the judgment over imported sessions, under a spend cap (default $5), paced by
  the machine's quiet, resumable by a META mark per tag; proposals only, never memberships. Its instructions say what
  a person record may hold: role facts, never evaluations.

## Live check (done here)

A scratch daemon (`theseusd --config scratch.toml --socket … --state-dir …`; `[discord]`, `[web]` and `[index]` off, a
`file:` stand-in key), the debug build of `ff86dc5`'s tree before its last clippy-only refactor (the same code paths),
and a three-episode fixture made by a Python script (`gulls.jsonl`, tag `gull-2026-04`, invented people: a Slack DM
with Marlo Quill `U0GULL01`, a Discord DM with "pell" `500000000000000007`, and a Slack channel where Marlo, Tamsin
Reef and Pell speak):

```
$ theseus ontology categories
No categories yet: `theseus ontology topic add NAME` declares a topic, and a bound place's channel or person is made when the binding starts.
$ theseus import openclaw gulls.jsonl
gulls.jsonl: read 3, imported 3, skipped 0, rejected 0 (11 nodes in 1 frame, 4 ms)
$ theseus import people --dry-run
people of gull-2026-04 (dry run: nothing written): 3 sessions read, 3 people found (0 held already, 3 to declare); 3 sessions to join, 5 memberships of origin import; 0 frames (1 ms)
$ theseus import people
people of gull-2026-04: 3 sessions read, 3 people found (0 held already, 3 declared); 3 sessions joined, 5 memberships of origin import; 1 frame (2 ms)
$ theseus import people   # again
people of gull-2026-04: 3 sessions read, 3 people found (3 held already, 0 declared); 0 sessions joined, 5 memberships of origin import; 0 frames (1 ms)
$ theseus ontology categories
Marlo Quill (person:marlo-quill)  2 sessions
  handles: slack:U0GULL01, name:Marlo Quill
Tamsin Reef (person:tamsin-reef)  1 session
  handles: name:Tamsin Reef
pell (person:pell)  2 sessions
  handles: discord:500000000000000007, name:pell
$ theseus ontology member ses_ep000000000000000000000000000000000000000000000000000000009a110002
person:tamsin-reef           person   import as of 1791572719612
person:marlo-quill           person   import as of 1791572719612
person:pell                  person   import as of 1791572719612
$ theseus ontology person add "M. Quill" --handle slack:U0GULL01 --handle email:mq@harbour.example --desc "Runs the gull survey counts."
person Marlo Quill (person:marlo-quill)
  handles: slack:U0GULL01, name:Marlo Quill, email:mq@harbour.example, name:M. Quill
$ theseus ontology person merge person:tamsin-reef "Marlo Quill"
merged person:tamsin-reef into Marlo Quill (person:marlo-quill): 1 sessions moved.
  handles: slack:U0GULL01, name:Marlo Quill, email:mq@harbour.example, name:M. Quill, name:Tamsin Reef
$ theseus ontology categories
Marlo Quill (person:marlo-quill)  2 sessions
  handles: slack:U0GULL01, name:Marlo Quill, email:mq@harbour.example, name:M. Quill, name:Tamsin Reef
pell (person:pell)  2 sessions
  handles: discord:500000000000000007, name:pell
$ theseus ontology person merge person:tamsin-reef --undo
undid the merge of person:tamsin-reef into person:marlo-quill: 1 sessions' lists as they were.
  handles: slack:U0GULL01, name:Marlo Quill, email:mq@harbour.example, name:M. Quill
$ theseus ontology categories
Marlo Quill (person:marlo-quill)  2 sessions
  handles: slack:U0GULL01, name:Marlo Quill, email:mq@harbour.example, name:M. Quill
Tamsin Reef (person:tamsin-reef)  1 session
  handles: name:Tamsin Reef
pell (person:pell)  2 sessions
  handles: discord:500000000000000007, name:pell
$ theseus import erase --tag gull-2026-04
erased gull-2026-04: 3 sessions and 11 nodes tombstoned in 1 frame (2 ms); index: not asked (no tender runs): its follower drops them as it reads their tombstones; topics: the memberships of 3 sessions emptied, 0 topics taken away; 3 people taken away
$ theseus ontology categories
No categories yet: `theseus ontology topic add NAME` declares a topic, and a bound place's channel or person is made when the binding starts.
```

The "add" with a held Slack id joined Marlo (the automatic merge of an exact handle) rather than making "M. Quill".
`theseus ledger -k ontology.merged` showed the merge's row and its undo's, each holding the absorbed record.
(`ontology member`'s "as of" prints raw milliseconds: that renderer is unchanged by this branch.)

## Live check for the maintainer (the install build, a copy of the owner's store)

On a scratch daemon over a copy of the owner's store (format 25: the first write moves it to 26, and an older build
then refuses it):

1. `theseus ontology kinds`: the person row reads `given`, assigned by `transport, operator, import`, `many`.
2. `theseus ontology categories`: before, one person (the owner's DM), now with `handles: discord:<id>`.
3. `theseus import people openclaw-2026-10 --dry-run`: counts people, sessions to join and memberships; writes
   nothing (`theseus ontology categories` unchanged). Expect it to take seconds to a minute: it decodes every
   imported node once.
4. `theseus import people openclaw-2026-10`: the same counts, "declared"; then again: "0 declared … 0 sessions
   joined … 0 frames".
5. `theseus ontology categories`: the people with their handles and session counts; check whether any Slack DM's
   party handle reads `slack:D…` (a DM channel id, not a user's: see step 1's caveat). If the owner's own DM party is
   in the import with his Discord id, it is the DM's person (held), not a second one.
6. Merge two handles of one person: `theseus ontology person merge <a person:…> <the DM's person:…>`, then
   `theseus ontology person merge <a person:…> --undo`.
7. The cockpit's Ontology view: the people panel, a person's page with sessions, the proposals panel; a session's
   Context tab pulldown has "people" under "topics".
8. Not on the owner's store: `theseus import erase --tag <a scratch tag>` takes back that tag's people (the fixture
   above shows it).

After the install, the owner runs `theseus import people openclaw-2026-10 --dry-run`, then without `--dry-run`. There
is no `--propose` yet (above).

## Docs the maintainer may want to change

- `docs/status.md` and the spec's Part III: the person kind's stored side, `import.people`, the merge and its undo,
  the bulk accept, store format 26.
- The spec's §4.1a (the kinds table): `person` is given with a stored side; its seed row names the operator and the
  import.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, twice:

- Before `ff86dc5`: fmt, shape, features, clippy, cockpit and the test build passed; the suite ran 3,595 tests,
  3,560 passed, 35 failed: the 33 known L1 tests (theseus-sandbox's contract tests and `spawn_100`, theseusd's
  sandbox tests: the VM runs as root, theseus-pv6i), plus two:
  - `theseus-core store::tests::a_write_moves_an_older_store_to_this_builds_format`: it read the manifest's format as
    a literal 25; fixed to 26 in `ff86dc5`, and passes.
  - `theseusd::gone_jobs a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran` (not on the known
    list): `the live wrapper's job runs on: {"dispatched":2,"outcome_unknown":3,"succeeded":11}` at
    gone_jobs.rs:358, under the full suite's load. It passed alone at once. It is a count of dispatched actions read
    right after a wait for that count to be 1 (a late result's turn dispatches its model call "for a moment"), a race
    between the wait and the assert in the test, in code this branch does not touch (job wrappers and a restart). Its
    output is kept in the session's scratchpad; worth an issue.
  The protocol types and `cargo deny --offline check` (after `cargo deny fetch`, which the setup's failed build had
  skipped) passed. The benches are off in a lane's gate.
- Before `85e0740` and `225eb4f` (one run over both): fmt, shape, features, clippy, cockpit (lint, 305 tests, build)
  and the test build passed; the suite ran 3,596 tests, 3,563 passed, 33 failed, all of them the known L1 tests. The
  protocol types and `cargo deny --offline check` passed. So the gate is green but for the L1 tests the brief names.

Runs under load (the recipe: the tests at nice 19 beside four busy loops at nice 0, killed by their pids after):
the new tests (the ontology crate's `tests_people`, the core's `import::tests_people` and `tests_people_proposals`)
with the gone_jobs test, 14 tests a run: two runs, 14 of 14 passed each (79 s and 81 s). A first run was cut by its
own 900 s timeout while its test binaries relinked at nice 19 under the loops, before any test ran.
