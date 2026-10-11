# CLOUD REPORT: ontology-polish (theseus-7mow)

Branch `cloud/20261010-ontology-polish`. Commits: seagull `8fa7833`, categories groups `96c5f41`, accept-by-name `9d10614`.
No store format change, no protocol change, no new dependency.

## 1. The seagull (cockpit) — 8fa7833
- **Found:** the sea is made in `src/ship/audio.ts` and `surf.ts` (Web Audio, seeded noise, no asset); `useShipSound.ts` mounts it
  for every page via `useSurf`, with `hearing = on && !calm && swell`.
- **Changed:** new `src/ship/gull.ts` (pure: the 150–420 s random gap, a seeded call of 2 or 3 falling notes, its pan, the
  scheduler `startGulls`), `src/ship/gullsound.ts` (the graph: sawtooth + wobble through a formant band-pass, an envelope
  fade in/out, a slow stereo pan across), `ShipAudio.gull/gullStop/gullsHeard` in `audio.ts`, and an effect in `useSurf`
  that starts the scheduler while the sea is heard and stops it plus every gull on cleanup. Peak gain 0.07 against the idle
  roll's 0.15. `cockpit/AGENTS.md` names the new files.
- **Proved:** `npm test` 321 pass (`test/gull.test.ts`: 6 tests: seeded gaps in bounds and never on a beat; scheduler with
  sound off schedules and plays nothing, on: first visit a gap away, stop cancels; call shape and gain under the waves; the
  node graph's sources stopped and gain disconnected on `gullStop` and on `dispose`; no context means no gull; the hook's
  wiring). `npx tsc -b`, `npm run lint` (no new warnings; the existing ones are in OntologyParts/Memberships/Actions/Ledger).
  Planted reverts, each caught: scheduler ignoring `on` (scheduler test), `gullStop` not stopping (graph test), a fixed gap
  (gaps test).
- **Live check (owner's browser):** open `/ship`, turn Sound on, wait; every few minutes a faint falling two/three-note cry
  should pass from one ear to the other. Dev console: nothing to read it, so to hear it sooner, temporarily set
  `GULL_GAP_MIN_S`/`GULL_GAP_MAX_S` in `gull.ts` to 5/10 in a dev server. Calm or `?swell=0` should keep it silent.
- **Uncertain:** the timbre is synthesized by ear-less reasoning (formant band-pass on a sawtooth); the owner should listen and
  tune `GULL_GAIN` and the note pitches (1.9–2.2 kHz start, falling to about 0.6x). I did not add a Ship "what's new" stop
  (`news.ts`), which would move `COCKPIT_VERSION`; add one if wanted.

## 2. The categories pane as closed groups (cockpit) — 96c5f41
- **Found:** the Ontology view's category table is one tree where every person is a root row. "Groups" in the owner's words are
  the kinds, so I group by kind.
- **Changed:** `lib/ontology.ts` `groupsOf` / `groupKeyOf` / `UNKNOWN_GROUP`; `views/Ontology.tsx` shows one header row per
  group (chevron, name, count), all closed on load, and the group's tree (same chevron folds as before) when open. The `person`
  group holds every person row as before; a kind the kinds table does not name (or an empty kind) goes in `<unknown>`, last,
  omitted when empty. Open all / fold all take the groups; a category selected by `?category=` opens its group. Branches and
  "in all" counts are per group. Nothing removed: the separate People widget is untouched.
- **Proved:** `test/ontology.test.ts` 13 pass: grouping and order and counts, people group holds every person and every
  category is in exactly one group, `<unknown>` group and its omission, and the view's source holds closed-by-default and the
  gated rendering. Planted reverts caught: person group opened at start (view test), people sent to `<unknown>` (3 tests).
- **Live check:** `/ontology`: categories pane shows only headers (`channel · n`, `person · n`, `topic · n`, ...), closed;
  opening `person` lists the person rows; select a person in the People widget and the person group opens in the pane.
- **Note:** the daemon cannot make an unknown-kind category through normal paths, so `<unknown>` rarely shows.

## 3. Accept-by-name joins the handle's holder (core) — 9d10614
- **Found:** `held_for` tried `person_by_handle("name:<name>")`, which returns `None` for any name handle by design, so only
  the display name ever matched: the owner's duplicates. `person_named` (the `as_person` path) likewise ignored name handles.
- **Changed:** `rpc/proposals.rs`: `name_holders` (live, unmerged, unretired people with a folded-equal `name:` handle),
  `held_checked` (exact handle, then name handle, then display name; two name-handle holders is an error naming both ids),
  `held_for` as its listing form (ambiguous means no holder), `topic_of` uses the checked form, `person_named` counts name
  handles (two holders: its existing "N people are named … name one by its id" refusal lists the ids). The grouped accept
  (`accept_all`, by judgments or row, `as_person`) goes through `accept_open`, so it follows. `AGENTS.md` of theseus-core
  updated. `tests_groups.rs` exports `Plant`/`plant` for the new test.
- **Proved:** `judge/people/tests_handles.rs`, 2 tests: a person with three name handles takes proposals under each (and the
  display name) singly, plus `accept_all` with `as_person` a handle, no second person made; two live holders refuse the single
  accept, the grouped accept and `as_person` with both ids, and declare/join nothing. Existing people tests (26 selected) pass.
  `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo fmt` clean. Planted reverts caught: handle lookup removed
  (both tests fail), first-holder pick (the two-holder test fails).
- **Live check (scratch daemon, fresh state dir):** `theseus ontology person add "Ada Quill" --handle "name:Addie"`, plant or
  wait for a people.v1 proposal of "Addie", then `theseus ontology proposals` and accept it: it should join `person:ada-quill`
  and no second person appear in `theseus ontology categories`. Add a second person holding `name:Addie`: the accept should
  refuse naming both ids.

## The gate
`scripts/gate.sh` stops in `keel` (5 s) on findings that are not in this branch's diff but in its range back to merge-base
`a9ad950` (the guard sees no `keel-signers` at the base): tui `tests.rs` assert-removed, `theseus-index/src/vectors.rs` and
`theseus-protocol/src/tests_work_join.rs` allow-added, and two `long-files.txt` ceilings (protocol lib.rs 2726→2733,
`theseus/tests/golden.rs`). **Keel findings expected:** none from my commits (they touch none of those files); the above come with the base.
So I ran the later phases by hand, all on the final tree: `cargo fmt --check` ok; `scripts/shape.sh` ok; `clippy --workspace
--all-targets -D warnings` ok; cockpit `npm run lint` (warnings only, pre-existing), `npm test` 321/321, `npm run build` ok;
registry test ok; `cargo nextest run --workspace --no-fail-fast`: 3733 run, 3700 passed, 33 failed, 44 skipped, the 33 being
exactly the known L1 root-daemon failures (theseus-sandbox contract and `spawn_100`, theseusd `sandbox` tests); `cargo deny
--offline check` ok (after `cargo deny fetch`). Not run: the benches (`THESEUS_GATE_NO_BENCH=1`), and the gate was run once on the
combined tree, so each of the three commits is green as a subset of it, not gated alone.
Env note: the VM's disk filled during the first suite run (`target/debug/incremental`, 17 GB, deleted; the suite was then run with
`CARGO_INCREMENTAL=0`).
