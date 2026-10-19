# CLOUD_REPORT: onto-core (theseus-e3mjc)

Branch `cloud/20261019-onto-core`, base `6fc5884` (main as cloned). Five commits on top of the task commit:

| Commit | What |
|---|---|
| `eb209d0` | the ontology crate: the three origins built, who may write what, kind retire, `examples` / `mined_from`, the eleven seeds, `Ontology::undo` |
| `5b88a92` | core: a bound guild channel makes its guild and takes it as its parent; `given()` yields both |
| `76406f4` | core: a machine write carries its undo in its rows; `ontology_undo` writes the inverse and an `ontology.undo` row in one frame; the ledger kind |
| `5db725c` | store: format 46, and the old layout's sample |
| `1b663c6` | the two rewritten tests keep their names for the keel guard; the seed constructor needs no `allow` |

## 1. The crate (`eb209d0`, `1b663c6`)

**Found.** `Origin::comes_with` refused jev, sweep and dream by name; `GIVEN` held three kinds; no machine origin could
write; nothing could retire a kind; no undo existed. The `check` code had no notion of "the operator wins".

**Changed** (`crates/theseus-ontology`):
- `kind.rs`: `Origin::comes_with` is `None` for every origin; docs say who writes from jev (an inbound judgment), sweep
  (the nightly re-association) and dream (the nightly consolidation); `Origin::is_machine`. `Kind` gains `retired_ms`,
  `examples` (at most 5 valid names, `EXAMPLES_MAX`) and `mined_from` (`MinedFrom`: titles, paths, hosts, capitalised),
  all skipped when empty. `GIVEN` is five. `seeds()` is the design's table of eleven rows, in `given_seeds()` and
  `interpreted_seeds()`; `topic` carries `mined_from: [titles, capitalised]` and `resource` `[paths, hosts]`;
  `thread` and `project` say "(read at compile from R6)". `Transport`'s doc is widened as asked.
- `machine.rs` (new): `check_kind_writer` (the operator writes any row; the sweep adds a new interpreted kind whose
  `assigned_by` names sweep and jev, with `added_by` `sweep:<id>`, and may retire only a kind it added; jev and the
  dream never write a kind), `check_kind_retire` (`InUse` while a category of the kind is live or a stored list holds
  it; a given or seeded kind is refused: `Given` / `Writer`), `check_category_writer` (a retired kind takes no new
  category; a machine origin writes live categories only, with `added_by` `<origin>:<id>`, and never supersedes a
  category the operator wrote), `check_list_writer` (a retired kind takes only an empty list; **operator wins**: a
  machine list keeps every operator and import entry of the current list unchanged, else a `Writer` refusal naming the
  entry, and adds only entries of its own origin), `Ontology::undo` and `Ontology::replaced`.
- `ontology.rs`: `check` calls them; `interprets` / `stored_by` take any origin the row names (the operator always);
  `check_table` ignores retired rows for precedence uniqueness.
- `compose.rs`'s message and the `check_basis` message say "guild, channel, thread, project, and person".
- `Ontology::undo(wrote, before, at_ms)`: gives, lists first, the record that puts each key back (the earlier list
  as it was, an empty list when it was new, a minted category retired at `at_ms`, a minted kind retired), each checked as
  the operator's write against a trial snapshot (so a category the write's own list named is free to go, and one another
  session's list names now is `InUse`). I added the `at_ms` argument to the brief's example signature, since the crate
  has no clock.
- **`Refusal::UnbuiltOrigin` and `Origin::comes_with` are kept**, against item 1's "remove the variant if nothing else
  uses it": the brief's key test plants exactly that (`comes_with` restored to `Some(..)`, "the test fails with
  `UnbuiltOrigin`"), and `Rule::comes_with` is the same reader-rule mechanism. `comes_with` is now a match with no
  `Some` arm. If the owner wants the dead variant gone, it is a deletion of `Origin::built`, `Origin::comes_with`, the
  variant, and the two `built()?` calls; no test but the planted one reads it.
- **A given kind's `assigned_by` check** (`check_basis`) accepts the person row as it was stored before Jev
  (`transport, operator, import`), so a store from before loads with nothing dropped (the test below fails without it).
- **Golden**: `tests/golden/seed-kinds.json` changes by the seven new rows (thread, project, event, resource,
  capability, expertise, culture), the two widened `assigned_by` lists (person, topic), and `mined_from` on topic and
  resource (item 5 asks for those seeds). The guild-channel and DM compile goldens are unchanged (they pass untouched).

**Rewrites of tests that asserted a rule this row changes** (none deleted; the first two keep their names, which the
keel guard reads, and say so in a doc comment):

| Test (tests.rs) | Before | After |
|---|---|---|
| `a_row_that_names_an_unbuilt_origin_is_refused_and_says_so` | a topic row naming jev/sweep/dream, written by the operator: `UnbuiltOrigin`, with "M5"/"M6" in the message; a kind written by `Jev`: `UnbuiltOrigin` | each origin's `comes_with()` is `None`; a culture row naming `[operator, origin]` is admitted from the operator; the same row written by the machine origin itself is refused `Writer` |
| `only_the_transports_three_kinds_are_given` | a new `thread` row, given: refused `Given` saying "only guild, channel, and person"; an interpreted kind naming the transport: `Given` | a new `region` row, given: refused `Given` saying "only guild, channel, thread, project, and person"; an interpreted kind naming the transport: `Given` (same) |
| `a_membership_is_of_its_lists_kind_from_an_origin_the_kind_allows` | a `Jev` entry in a topic list: `UnbuiltOrigin` | a `Jev` entry in a topic list is admitted (topic names jev); in a `culture` list (which does not) it is `Writer` |
| `a_compose_skips_what_it_cannot_use_and_uses_the_rest` | a jev membership of `topic:web` skipped as "`jev` may not assign `topic` memberships" | a jev membership of `culture:manners` skipped as "`jev` may not assign `culture` memberships"; the given-kind message names five kinds |
| `the_seed_rows_check_and_come_in_precedence_order`, `every_record_round_trips_through_its_key_and_value`, `the_import_writes_topics_and_lists_while_the_kind_names_it`, `tests_people::the_person_kind_stays_given_and_gains_a_stored_side` | four seed rows; topic names operator and import; 4 + 8 + 1 + 1 records; person names three origins | eleven rows; topic names five origins; 11 + 8 + 1 + 1; person names jev too |
| fixture kinds in 10 tests | `culture`, `project`, `thread` at precedence 50, 60, 35, 45 | renamed `habit`, `program`, `region` and precedences 150, 160, 55 where a new seed holds the old name or number; same assertions |
| `tests/props.rs` | a row naming an origin other than transport, operator, import is never accepted; `culture` is a new kind | every origin is built (`comes_with()` is `None`); `culture` is the seed's version 2; free precedences are 45, 55, 95 |

No assertion of a guard was dropped: a refusal that was asserted is asserted of the same thing under the new rule
(a writer that may not write it, or an origin the row does not name).

**Tests added**: `tests_origins.rs` (the key test `jev_files_a_topic_list_and_mints_its_category`; the culture/jev
refusals; `added_by` prefixes `sweep:x`, bare `jev`, `jev:`, `ada`, `Jev:x` and over-long; operator wins and the import's
entries too; machine writes never supersede or retire the operator's category; undo restores byte for byte; undo of a mint
another session uses is `InUse`; unpaired undo; a sweep's kind undone) and `tests_kinds.rs` (new seeds pass the table
and are the five given; the row without the new fields serialises as today's literal bytes; six examples refused and an
unknown `mined_from` word refused; retire `InUse` for a live category and for a list, then succeeds, then the kind takes
no new category or list; given and seeded kinds are never retired; the sweep retires only its own; a retired kind frees
its precedence; the sweep adds a new interpreted kind and nothing else; a store from before loads whole).

## 2. Bind (`5b88a92`)

`ontology::given(place, snapshot, now)` reads the guild through the channel category's parent; `guild_category`;
`bound_category(target, name, guild)` sets the parent. `bind_categories` makes `guild:<id>` (once, never renamed) and
the channel (made, renamed, or given the parent it lacked) in one frame, guild first; an identical bind writes none.
`BoundPlace` carries no guild name and the Discord binding does not give one to the core, so **the guild is named by its
id**. Adding `guild_name` to `BoundPlace` touches its literals in ~15 tests and the binding: I left it. A DM is unchanged.

**Item 7 (thread and project at compile) is not built; only the rows are.** The walk (`TurnRunner::walk`) can reach the
thread (`thread_topics` finds it with `self.threads.all(..).of_session(..)`), but `thread:<slug>` and `project:<id>` need
categories written at thread and project creation (`rpc/threads.rs`, `project/create.rs`, `rpc/projects.rs`), which are
frames those paths count in their own tests (the thread's one-frame create and the project's), and the channel parent
of a thread is a Discord surface, not on the thread record. That is more than a small local change. What R6 needs: a
category write beside the thread's and the project's create frame (origin `transport`), `Walk::of` taking the thread
slug and the session's `project` (a `SessionRecord` field) into `given`, and the same on `ontology.list`. The seed rows say
"read at compile from R6".

## 3. Undo (`76406f4`)

`LedgerKind::OntologyUndo` (`ontology.undo`), written by `fact::ontology::OntologyUndo` (so the reader rule holds).
`CategorySet` and `MembershipSet` gain `undo: Option<&Undoable>` (the write id, the key, the record as written, the
record as it was or `null`), riding the row's payload as `"undo"`; operators' rows are unchanged (their `undo` is `None`
and absent). `Core::ontology_machine_write(by, write, records)` (jev, sweep or dream only; categories and lists only: a
machine kind or guidance write comes with the sweep's row) writes one frame, scoped `ontology.write:<write>`, and
refuses if the snapshot changed under it. `Core::ontology_undo(write, who, via)` reads the scope back, calls
`Ontology::undo`, and writes the inverse records and one `ontology.undo` row (label `wrong`, the write id) in one frame,
as the operator; a second undo is refused; an `InUse` writes nothing. "The undone frame's id" is the write id (a
judgment's, a sweep's); frames have no id of their own that rows can name before they are written. No RPC or CLI: R3's.

## 4. Store format (`5db725c`)

**Bumped, 45 to 46** (main at my clone is at 45, not 15 or 16 as the preamble says). The brief's design plans no bump,
but the rule (AGENTS.md, "The store's version rule") says a step that adds a field to a stored record bumps it, and
the same record family bumped for `handles` and `merged_into` (format 26). A build from before would read a retired
kind as live and drop what jev wrote. The old layout is a literal in `tests_layouts.rs` (`KIND_BEFORE_ONTO_CORE`, an
`onto:kind:` row; the META reader now tells a kind row from a category). The 13 asserts that name the number, and
theseusd's `versions.rs` (the newer store is 47), follow it. **Others bump too: renumber at the merge** (`store.rs`'s
`MANIFEST_FORMAT` line and comment, those asserts).

## 5. Planted reverts

Each: file, the edit, the test that failed and its failure line. Every file was restored and `touch`ed, and
`git status` was clean after each (the script is `plant.py` in my scratchpad; each plant re-ran the test alone).
Replay by applying the edit and running `cargo nextest run -p <crate> -E 'test(<name>)'`.

```
PLANT 1 KEY: Jev is unbuilt again | exit 100
  file: crates/theseus-ontology/src/kind.rs
  before:             | Origin::Jev\n            | Origin::Sweep\n            | Origin::Dream => None,
  after:              | Origin::Sweep\n            | Origin::Dream => None,\n            Origin::Jev => Some("M5, with Jev's categorize.v1"),
  FAIL: ['FAIL [   0.123s] (1/1) theseus-ontology tests_origins::jev_files_a_topic_list_and_mints_its_category']
  panic: ["thread 'tests_origins::jev_files_a_topic_list_and_mints_its_category' (9408) panicked at crates/theseus-ontology/src/tests_origins.rs:52:42:", 'called `Result::unwrap()` on an `Err` value: UnbuiltOrigin { origin: Jev, comes_with: "M5, with Jev\'s categorize.v1" }', 'stack backtrace:']
PLANT 2 operator wins: a machine list may drop the operator's entry | exit 100
  file: crates/theseus-ontology/src/machine.rs
  before:             if !l.members.contains(old) {
  after:              if false && !l.members.contains(old) {
  FAIL: ['FAIL [   0.098s] (1/1) theseus-ontology tests_origins::the_operator_wins_on_conflict']
  panic: ["thread 'tests_origins::the_operator_wins_on_conflict' (10123) panicked at crates/theseus-ontology/src/tests_origins.rs:44:21:", 'refused: ()', 'stack backtrace:']
PLANT 3 added_by prefix unchecked | exit 100
  file: crates/theseus-ontology/src/machine.rs
  before:         if !prefixed(&c.added_by, by) {
  after:          if false && !prefixed(&c.added_by, by) {
  FAIL: ['FAIL [   0.086s] (1/1) theseus-ontology tests_origins::a_machine_category_names_its_writer_in_its_added_by']
  panic: ["thread 'tests_origins::a_machine_category_names_its_writer_in_its_added_by' (10838) panicked at crates/theseus-ontology/src/tests_origins.rs:44:21:", 'refused: ()', 'stack backtrace:']
PLANT 4 retire ignores live categories | exit 100
  file: crates/theseus-ontology/src/machine.rs
  before:         if live > 0 {
  after:          if false && live > 0 {
  FAIL: ['FAIL [   0.085s] (1/1) theseus-ontology tests_kinds::a_kind_is_retired_once_nothing_uses_it_and_then_takes_nothing_new']
  panic: ["thread 'tests_kinds::a_kind_is_retired_once_nothing_uses_it_and_then_takes_nothing_new' (11553) panicked at crates/theseus-ontology/src/tests_kinds.rs:108:54:", 'called `Result::unwrap_err()` on an `Ok` value: ()', 'stack backtrace:']
PLANT 5 a machine origin may write any kind | exit 100
  file: crates/theseus-ontology/src/machine.rs
  before:             _ => return Err(sweep_only("write a kind")),
  after:              _ => {}
  FAIL: ['FAIL [   0.093s] (1/1) theseus-ontology tests_kinds::the_sweep_adds_a_new_interpreted_kind_and_changes_nothing_else']
  panic: ["thread 'tests_kinds::the_sweep_adds_a_new_interpreted_kind_and_changes_nothing_else' (12268) panicked at crates/theseus-ontology/src/tests_kinds.rs:233:14:", 'called `Result::unwrap_err()` on an `Ok` value: ()', 'stack backtrace:']
PLANT 6 undo does not check the inverse (InUse) | exit 100
  file: crates/theseus-ontology/src/machine.rs
  before:             trial.check(r, Origin::Operator)?;
  after:              let _ = &r;
  FAIL: ['FAIL [   0.079s] (1/1) theseus-ontology tests_origins::an_undo_of_a_mint_another_session_now_uses_is_refused']
  panic: ["thread 'tests_origins::an_undo_of_a_mint_another_session_now_uses_is_refused' (12983) panicked at crates/theseus-ontology/src/tests_origins.rs:239:44:", 'called `Result::unwrap_err()` on an `Ok` value: [Members(MemberList { session: "s1", kind: "topic", members: [] }), Category(Category { id: CategoryId("topic:billing"), name: "billing", parent: None, description: "", added_by: "jev:j-0002", retired_ms: Some(9000), handles: [], merged_into: None })]', 'stack backtrace:']
PLANT 7 six examples admitted | exit 100
  file: crates/theseus-ontology/src/kind.rs
  before:         if self.examples.len() > EXAMPLES_MAX {
  after:          if self.examples.len() > EXAMPLES_MAX + 1 {
  FAIL: ['FAIL [   0.083s] (1/1) theseus-ontology tests_kinds::examples_are_at_most_five_names_and_mined_from_is_a_closed_set']
  panic: ["thread 'tests_kinds::examples_are_at_most_five_names_and_mined_from_is_a_closed_set' (13698) panicked at crates/theseus-ontology/src/tests_kinds.rs:92:10:", 'called `Result::unwrap_err()` on an `Ok` value: ()', 'stack backtrace:']
PLANT 8 an old person row is dropped at load | exit 100
  file: crates/theseus-ontology/src/kind.rs
  before: self.assigned_by == seed.assigned_by || self.before_jev(),
  after:  self.assigned_by == seed.assigned_by,
  FAIL: ['FAIL [   0.087s] (1/1) theseus-ontology tests_kinds::a_store_from_before_loads_whole_and_serves_the_same_memberships']
  panic: ["thread 'tests_kinds::a_store_from_before_loads_whole_and_serves_the_same_memberships' (14413) panicked at crates/theseus-ontology/src/tests_kinds.rs:262:5:", 'assertion `left == right` failed', 'left: [("onto:kind:person", Given { kind: "person", why: "`person` is given by the transport, and its assigned_by is the transport\'s fact: it stays as it is (transport, operator, import, jev). Its precedence, rule, and description may change" })]']
PLANT 9 a seeded kind may be retired | exit 100
  file: crates/theseus-ontology/src/machine.rs
  before:         if seeds().iter().any(|s| s.name == k.name) {
  after:          if false && seeds().iter().any(|s| s.name == k.name) {
  FAIL: ['FAIL [   0.108s] (1/1) theseus-ontology tests_kinds::a_given_or_seeded_kind_is_never_retired']
  panic: ["thread 'tests_kinds::a_given_or_seeded_kind_is_never_retired' (15128) panicked at crates/theseus-ontology/src/tests_kinds.rs:156:14:", 'called `Result::unwrap_err()` on an `Ok` value: ()', 'stack backtrace:']
PLANT 10 a retired kind keeps its precedence | exit 100
  file: crates/theseus-ontology/src/ontology.rs
  before: .find(|o| o.retired_ms.is_none() && o.name != k.name && o.precedence == k.precedence)
  after:  .find(|o| o.name != k.name && o.precedence == k.precedence)
  FAIL: ['FAIL [   0.091s] (1/1) theseus-ontology tests_kinds::a_retired_kind_frees_its_precedence']
  panic: ["thread 'tests_kinds::a_retired_kind_frees_its_precedence' (15844) panicked at crates/theseus-ontology/src/tests_kinds.rs:191:5:", 'assertion `left == right` failed', 'left: Err(Precedence { why: "`pipeline` and `workflow` both have precedence 55: each kind needs its own, so that clashes resolve the same way every time" })']
PLANT 11 the channel has no parent at bind | exit 100
  file: crates/theseus-core/src/ontology.rs
  before:         id,\n        parent,\n
  after:          id,\n        parent: None,\n
  FAIL: ['FAIL [   0.364s] (1/1) theseus-core tests_ontology_bind::a_guild_channels_bind_makes_its_guild_and_gives_the_channel_its_parent']
  panic: ["thread 'tests_ontology_bind::a_guild_channels_bind_makes_its_guild_and_gives_the_channel_its_parent' (16710) panicked at crates/theseus-core/src/tests_ontology_bind.rs:59:5:", 'assertion `left == right` failed', 'left: None']
PLANT 12 a rebind always writes | exit 100
  file: crates/theseus-core/src/rpc/ontology.rs
  before:                 if stale && !made.iter().any(|m| m.id == c.id) {
  after:                  if !made.iter().any(|m| m.id == c.id) {
  FAIL: ['FAIL [   0.348s] (1/1) theseus-core tests_ontology_bind::a_guild_channels_bind_makes_its_guild_and_gives_the_channel_its_parent']
  panic: ["thread 'tests_ontology_bind::a_guild_channels_bind_makes_its_guild_and_gives_the_channel_its_parent' (17379) panicked at crates/theseus-core/src/tests_ontology_bind.rs:72:5:", 'assertion `left == right` failed', 'left: 1']
PLANT 13 a store from before never gets its parent | exit 100
  file: crates/theseus-core/src/rpc/ontology.rs
  before:                     old.parent != c.parent || (c.kind() != "guild" && old.name != c.name)
  after:                      c.kind() != "guild" && old.name != c.name
  FAIL: ['FAIL [   0.382s] (1/1) theseus-core tests_ontology_bind::a_store_from_before_gets_the_channels_parent_at_its_next_bind']
  panic: ["thread 'tests_ontology_bind::a_store_from_before_gets_the_channels_parent_at_its_next_bind' (18050) panicked at crates/theseus-core/src/tests_ontology_bind.rs:100:5:", 'assertion `left == right` failed', 'left: None']
PLANT 14 the undo row forgets what the list was | exit 100
  file: crates/theseus-core/src/rpc/ontology_machine.rs
  before: before: b.as_ref().map_or(serde_json::Value::Null, Record::to_value),
  after:  before: serde_json::Value::Null,
  FAIL: ['FAIL [   0.211s] (1/1) theseus-core tests_ontology_undo::a_machine_write_is_undone_byte_for_byte_in_one_frame']
  panic: ["thread 'tests_ontology_undo::a_machine_write_is_undone_byte_for_byte_in_one_frame' (18723) panicked at crates/theseus-core/src/tests_ontology_undo.rs:129:5:", 'assertion `left == right` failed: the earlier list, byte for byte', 'left: [123, 34, 107, 105, 110, 100, 34, 58, 34, 116, 111, 112, 105, 99, 34, 44, 34, 109, 101, 109, 98, 101, 114, 115, 34, 58, 91, 93, 44, 34, 115, 101, 115, 115, 105, 111, 110, 34, 58, 34, 115, 49, 34, 125]']
```

## 6. Checks

Run here, all green: `cargo fmt --all --check`; `bash scripts/shape.sh` (ok); `cargo clippy -p theseus-ontology -p
theseus-core -p theseus-protocol --all-targets -- -D warnings` (clean); `cargo nextest run -p theseus-ontology` (77
passed) and `-p theseus-protocol` (183 passed with ontology: 260 run, 1 skipped); `TZ=America/Phoenix cargo nextest run
-p theseus-core -E 'test(/ontology|bind|place|person|topic|layout|golden|people|output|registry|first_request/)'` (385
passed before the undo commit, 424 with it and the protocol tests); store and layout tests (187 passed); theseusd's
`versions` test (10 passed); `python3 scripts/keel-guard.py --base 6fc5884 --head HEAD --quiet` empty (exit 0).
A wider `cargo nextest run --workspace -E 'test(/ontology|person|topic|guild/)'` ran 164 tests: 163 passed, 1 failed:
`theseus-discord tests_task_threads::a_task_in_a_guild_channel_gets_a_thread_where_its_stream_lands_and_one_rename_at_the_end`
(times out at 100 s waiting for the task's tool line in its thread). It fails the same way alone, and again with my guild
category write switched off, so I take it as this VM's (it is not on the flaky list; I did not run it on the base). Not run here: the full gate, the benches, cargo-deny.

## 7. Live check for the maintainer

After the install of this build, on a scratch daemon or the operator's own, with the Discord binding started on a
bindings file that binds a guild channel (a fake-discord daemon works: `theseus-sim fake-discord --gateway --guild`
with the daemon's `[discord]` pointed at it):

```
theseus ontology kinds            # eleven rows: guild channel thread project person topic event resource capability expertise culture
theseus ontology categories       # a guild:<id> category, named by its id, with the channel under it
theseus ontology categories --json | jq '.categories[] | select(.id|startswith("channel:")) | .parent'
```

The first lists the stored `topic` and `person` rows as stored if the operator ever edited them (a row from before
keeps its old `assigned_by`, so Jev may not file topics until the operator writes the row again: `theseus ontology kinds`
shows it). The third must print the guild's id after the binding's first start with this build; run it before and after
a restart: the first start fills the parent (one frame), the second writes nothing (`theseus ledger tail` shows no new
`ontology.category` row). The store's manifest moves to format 46 on the first write.

## 8. Left, and uncertain

- **Item 7 reader (thread, project): not built**, above.
- **The guild's name**: the id, above.
- **The old stored `topic` and `person` rows**: a store from before has `topic` stored with `assigned_by` of operator
  and import (and `person` with three), which supersede the new seeds, so jev may not write a topic until the
  operator writes the row again (the CLI has no verb for a kind's row: `theseus ontology kinds` only reads). This is
  how a stored row always superseded its seed; the maintainer may want a one-time migration at R3, or to let the seed
  win for a row whose `added_by` is `seed`. Not done: it would make a stored row change under the operator.
- `Refusal::UnbuiltOrigin` kept, above.
- `Core::ontology_machine_write` refuses kinds and guidance; the sweep's kind add and the dream's culture writes at
  R5 and R7 will need their rows.
- Ledger rows for a machine write carry the whole prior list and the record as written (a few hundred bytes a row).
- Not run here: the full gate, the benches, cargo-deny, the cockpit's tests (no TypeScript changed: `LedgerKind` is not
  generated).
- Disk: the VM's allowance filled once during the workspace build; I removed `target/debug/incremental` (inside the
  repository) and built the rest with `CARGO_INCREMENTAL=0`.
