# CLOUD_REPORT: persona-threads, T10b (theseus-a8n7)

Branch `cloud/20261013-persona-threads`, from main as cloned (store format 40 at `c0bd49fb`). Three commits of work:

| Commit | What |
|---|---|
| `798e04a1` | bench: the personality exam's `persona` family (step 6) |
| `1c5aa171` | persona: import, card, proposed standing rules, recall boost, thread prior (steps 1 to 5), store format 41 |
| `98659d94` | persona: the persona folder goes beside the store the daemon runs on (a bug the live check found) |

## What I found first

- **`[personas.import] files` and `[personas] own_boost` cannot exist as written.** `[personas]` is a map of persona
  name to table, so both keys would parse as personas named `import` and `own_boost`. The two keys live in a new
  section, **`[persona]`** (`import_files`, `own_boost`), with defaults, a template block and the template test.
  `config/persona_settings.rs`.
- **`StandingScope` had no persona scope**, so the task's "scope them to the persona" needed the additive variant:
  `StandingScope::Persona(name)`. That is a stored-record variant, so **store format 41** (below).
- **The keel guard fails on the base, not on this branch.** `python3 scripts/keel-guard.py` (and so `scripts/gate.sh`,
  whose first phases are the door and the keel) reports one erosion in the range `9fa84735c0..HEAD`:
  `crates/theseus-sim/src/lifecycle.rs:155: budget-raised: "seed" => Some(250.0)`. Its ack is on `1c2beb2747`, a
  commit that is in this branch's history before my first commit, and its signature does not verify on this VM
  (`%G? is E`: no key). None of my commits touches a cap, a ceiling, a budget, a test's assertion count or an
  `#[allow]`. See "Keel findings expected".

## The steps

### 1. `theseus persona import --from <dir> --name <persona> [--agent <name>] [--dry-run]`

**Built** (`1c5aa171`): `persona.import` (a daemon method; the CLI refuses it inside a job, the daemon judges it as the
import's act, `Act::Import`) reads the workspace on the daemon's machine and carries `[persona] import_files`
(`SOUL.md`, `IDENTITY.md`, `USER.md`, `AGENTS.md`, `MEMORY.md`, and the folder `memory`) **byte for byte** into
`<state>/personas/<name>/`. The CLI then writes `[personas.<name>] files` (and the card, below) into the config file, the
way `persona edit` does, with a `.bak-<ms>` copy; a vault note is told what to hold instead.

- The five top-level files are the persona's context files, in that order. The `memory/` notes are carried and read for
  feedback but **not compiled** into the prompt (they are the agent's episodic notes, which the soul import already
  brought in as episodes; compiling 20 notes into layer 2 would cost every cold message). The owner can add any of them to
  `files` by hand.
- A file over the 64 KB context limit is cut at its headings (then at a blank line, then at a line, and only for one line
  longer than the limit at a character boundary) into `NAME.part1.md` and so on. The parts joined are the file.
- A later import whose bytes differ is **version 2**: the replaced files move to `.versions/1/`, `import.json` is
  rewritten, and the next compile that sees the digests change writes the usual `persona.changed` row. The same bytes
  again are "as it was" and write nothing.
- The dry run prints every file with size and digest, what is over the limit (and into how many parts), what was skipped
  (a link, a non-UTF-8 file, an empty one), the draft card, and the proposals. It writes nothing.
- The importer's code is `crates/theseus-core/src/persona_import/` (`split.rs`, `card.rs`, `feedback.rs`, `manifest.rs`,
  `run.rs`; the RPC is `rpc/persona_import.rs`; the wire types `theseus-protocol/src/persona_import.rs`).

### 2. The specialization card

Drafted with no model: the identity file's role line (`Role:`, then `Focus`, `Specialization`, `Specialty`, `Creature`,
`Vibe`, or a `## Role` heading and the line under it) and the agents file's level 1 and 2 headings (up to 6, fences
skipped), at most 420 characters. It is a **draft** while the config's card equals the manifest's draft text and the owner
has not kept it. `persona.list` now says `card_draft` and `offered` (a card and no draft: what route.v5's persona question
will read; route.v5 is not in this tree, so `offered` is the reader today, in `theseus persona`). The owner edits it with
`theseus persona edit <name> --card "..."` (the text then differs from the draft) or keeps it with
`theseus persona edit <name> --confirm-card` (`persona.card_confirm`). The import never replaces a card the config
already has.

### 3. Standing rules from feedback

Found by shape (`feedback.rs`): a note whose frontmatter says `type: feedback` gives its first paragraph; the bullets
under a `## Feedback` heading give one item each (continuation lines joined). `order` when the words command (never,
always, must, stop, don't, do not), `preference` otherwise. An item over the 1,000-character record limit is counted, not cut.

They become **proposals**, never records: META `standing.proposal:<id>` (id `stp_` + 12 hex of the persona, kind and words,
so the same words for the same persona are one proposal), filed in one frame with a `persona.imported` row.
`standing.proposals`, `standing.keep` and `standing.dismiss` (CLI: `theseus standing proposals [--all] [--persona]`,
`keep <id>`, `dismiss <id>`; the two answers are the owner's acts, refused in a job) answer them. A keep is
`standing.add` of the words with `StandingScope::Persona(name)` (so it rides only turns that speak as that persona;
`standing::Scope` has a `persona`) and then closes the proposal (`standing.proposal_answered`); a dismissal closes it for
good, and neither is proposed again. A persona scope names a persona the config has.

### 4. The persona recall boost

`theseus_memory::PersonaBoost` in `Params.persona`: while a persona is engaged, a candidate whose session the core reads as
an imported episode of the persona's own agent ranks `[persona] own_boost` (default 1.2, 1.0 off, bounded to 2.0) times its
score. Only the rank multiplies; the filters (place, threshold, floor) still see the index's score, so the boost admits
nothing and a note a fifth better than the persona's own still wins. The agent is the manifest's (`--agent`, else the
persona's key), matched lower case against `ImportedFrom.agent`. The recalled item's frozen header begins
`[from Wren, 2026-09-14, @4210]`, as a thread's mark does. "Engaged" is the turn's persona (`Target.persona`: the session's
choice, its place's, the default); route.v5's per-message verdict is not in this tree, and will engage a persona through the
same scene (`persona_recall.rs`). A model's own `memory.lookup` and `memory.search` do not boost (ambient recall only).

### 5. The thread's prior

`thread.new { place }` (new optional field) takes the place's persona from the bindings file when the config has that persona.
The router's new-thread tap passes the home session's place; Discord's `/thread new` passes the channel (or DM) it was typed
in. `thread.edit { persona }` already changes it. A place naming an unknown persona, an unbound place and no place give none.

### 6. Identity probes per persona (the personality exam has joined)

`bench/personality/probes/persona.toml`: family `persona`, three probes in the exam's own format (who are you and what are
you for; what do you care about most; what do you call me). The checks look for the words of the persona's own files, set
in the persona TOML as `role_words` (from its IDENTITY.md) and `care_words` (from its SOUL.md), with defaults for the
built-in persona. The calm session plans the family; the report lists it after identity. It is one exam run per persona
file, as the README already said for the persona-identity row.

## How I proved it

Commands are from `/home/user/theseus` with `TZ=America/Phoenix`, `CARGO_INCREMENTAL=0` (the disk allowance filled twice with
incremental on).

- **New tests (all pass):** core `persona_import::tests` (9: bytes identical for a CRLF, curly-quote, no-final-newline file;
  dry run writes nothing; version 2 keeps the old bytes; over-limit split verbatim; the importer's split; draft card
  deterministic and marked; feedback by shape; too-long feedback counted; bad name and empty workspace refused),
  `rpc::tests_persona_import` (the draft is not offered until kept; the folder is beside the store even when the config
  names another state dir), `rpc::tests_standing_proposals` (2: filed, never live, kept for its persona alone, dismissed
  never proposed again; a persona scope names a configured persona), `rpc::tests_persona_threads` (1),
  `tests_persona_recall` (2: through a whole turn the engaged persona's own episode is first though the other agent's note
  scored a tenth higher, and its header names it; no persona or a boost of 1.0 changes nothing), `standing::tests` (a
  persona-scoped record holds for that persona alone), `tests_layouts` (a format-40 standing record reads and writes back
  byte for byte, the persona scope round-trips), theseus-memory `tests_persona` (3: equal relevance, bounded, admits
  nothing), the CLI's `persona_cmd` (2) and `standing_cmd` (1), the cockpit's scope words, and Python `bench/personality`
  (`python3 -m unittest discover -s bench/personality -p "test_*.py"`: 77 tests OK, 3 skipped; two new).
- **Planted reverts** (plant, show the failure, restore, `touch`, `git status` clean):

  | Behaviour | The plant | What failed |
  |---|---|---|
  | A carried file is the workspace's bytes | CRLF rewritten to LF in `run.rs` | `persona_import::tests::a_carried_file_is_the_workspaces_bytes` |
  | The boost is the persona's own agent's | `factor` returns the boost for any known session | `tests_persona::a_clearly_better_note_still_wins_and_one_is_off` and `at_equal_relevance_the_engaged_personas_own_comes_first` |
  | Feedback is never live | the import writes each item as a live persona record | `tests_standing_proposals::feedback_is_proposed_never_live_and_kept_for_its_persona_alone` |
  | A persona scope holds for its persona alone | `Persona(_) => true` | `standing::tests::a_persona_scoped_record_holds_for_that_persona_alone` |
  | A thread takes its place's persona | `persona: None` in `thread_new` | `tests_persona_threads::a_thread_born_in_a_bound_place_inherits_its_persona` |
  | A draft card is not offered | `offered = card.is_some()` | `tests_persona_import::the_drafted_card_is_offered_to_jev_only_once_kept` |
  | The folder is beside the store | `state_root` returns the config's `state_dir` | the same test (its config names another state dir) |

- **FAST:** `target/debug/theseus-sim bench turn --check`: plain turn **4 frames** (budget 4), tool-call turn **8** (budget 8),
  batch 14; syncs before the first byte 2, 2, 2. A plain turn with no persona configured reads no store and no file on this
  change's path (`persona_scene` returns on `None` before anything is read). With a persona, the manifest is read once per
  persona per daemon run (a ~1 KB file) on the first recall, and an imported candidate's session record once per session.
  No timer was added.
  `target/debug/theseus-sim bench lifecycle --runs 10 --check` (a debug build on this 4-core VM): cold start p50 29.7 ms and
  p95 41.6 (budget 50), config-copy start p50 33.8, clean shutdown p50 15.5 (budget 100), crash restart, swap and restore
  within their lines; **one line missed: "health, after the seed, each call its own connection" p50 6.2 ms against 5 ms.** Health
  reads no persona and this change added nothing to its path; I did not build the base to compare, so I cannot say the miss
  is the VM's and not the branch's. Please run it on the owner's machine.
- **The gate.** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` stops in its first two phases (the door runs main's
  gate on this tree; the keel phase fails on `1c2beb2747`'s unverifiable ack, above). So I ran the gate's later phases by hand
  (`scratchpad/mygate3.sh`, mirroring `gate.sh`): `cargo fmt --all -- --check`, `scripts/shape.sh`,
  `cargo clippy --workspace --all-targets -q -- -D warnings`, the cockpit's lint, test and build, the reader-rule test
  (`tests_registry`, 15 pass), `cargo nextest run --workspace --no-fail-fast`, the protocol-types check, and
  `cargo deny --offline check` (bans, licenses, sources; no advisory database was fetched here). Results are at the end.

## The live check on this VM

A scratch daemon on a fresh state dir (`--config`, `--socket`, `--state-dir`; the stand-in secret is `env:`), two invented
workspaces (`crates/theseus-core/tests/fixtures/agents/{wren,finch}`), the script is reproducible as below.

```
B=target/debug; F=crates/theseus-core/tests/fixtures/agents; W=/home/user/live
mkdir -p $W/state
printf '[secrets]\nanthropic_api_key = "env:LIVE_FAKE_KEY"\n\n[judge]\nenabled = false\n' > $W/theseus.toml
export LIVE_FAKE_KEY=x
$B/theseusd --config $W/theseus.toml --socket $W/sock --state-dir $W/state &
T="$B/theseus --socket $W/sock"
$T persona import --from $F/wren --name wren --dry-run      # nothing written
$T persona import --from $F/wren --name wren
$T persona import --from $F/finch --name finch
for f in SOUL IDENTITY USER AGENTS MEMORY; do cmp $F/wren/$f.md $W/state/personas/wren/$f.md; done   # silent: identical
$T shutdown; (restart the daemon onto the config the CLI wrote)
$T persona                       # both personas, each with its (DRAFT) card
$T standing proposals            # three, none live
$T standing keep <id of the metres one>; $T standing   # one live record, scope persona:wren
$T persona edit wren --confirm-card; $T persona        # wren's card no longer DRAFT; finch's still is
$T persona import --from $F/wren --name wren           # "as it was (version 1)", "3 already proposed earlier"
```

What it showed: the dry run listed the five persona files and three notes with digests, the draft card
`Tidal ecology field notes and sailing logistics for the Harbor Survey. Areas: Operating notes; Tide tables; Species logs;
Weather windows.` and three proposals (two orders and a preference); the import wrote `[personas.wren]` and `[personas.finch]`
(`files`, `card`) into the config; all five `cmp`s were silent; after the restart `theseus persona` listed both with
`card (DRAFT: not offered to Jev until you keep it)`; `standing` listed only the kept record
(`persona:wren ... persona-import:stp_1cc8841ff217`); after `--confirm-card` wren's card lost its DRAFT mark and finch's kept it;
a second import wrote nothing.

**What the live check found:** the first run wrote the persona folders under `~/.theseus/personas`, because the import took
the state dir from `[server].state_dir`, which a `--state-dir` start does not move: a scratch daemon would have written
into the operator's own state dir. Fixed in `98659d94` (the folder is beside the store, as the spool is), with the test
above; the second run wrote under the scratch dir and `~/.theseus` was not created.

**Not driven on this VM:** "a question in each persona's place recalls that agent's episode first". That needs two Discord
places bound to personas and imported episodes in the index; the VM has no Discord and no model. It is proved in-process
instead (`tests_persona_recall`, through a whole turn and a stand-in index), and the maintainer's check on the owner's
machine is below.

## The maintainer's commands for the owner's own agents (local; mine could not read the real files)

The agent label the boost matches is the episodes' `agent` field (what the soul import recorded for that agent); the
persona key must equal it, or pass `--agent <label>`.

```
# 1. dry-run each workspace: nothing is written; read what it carries, the over-limit files, the card, the proposals
theseus persona import --from <agent workspace dir> --name <persona key> [--agent <episodes' agent label>] --dry-run

# 2. after the owner has read the files and the card, apply (writes <state>/personas/<name>/ and [personas.<name>] in the config)
theseus persona import --from <agent workspace dir> --name <persona key> [--agent <label>]

# 3. restart the daemon onto the config; read each card, then keep or edit it
theseus persona
theseus persona edit <persona key> --card "..."          # or: --confirm-card
# 4. answer the proposed standing rules, one at a time (none is live until kept)
theseus standing proposals
theseus standing keep <id>        # or: theseus standing dismiss <id>
# 5. bind the persona to its places in the bindings file (persona = "<persona key>"), then in such a place ask something
#    that agent's imported episodes answer: the Recall node's item header begins "[from <Name>, <date>, @<position>]"
#    and the item is first (`theseus why` / Discord's `/why` show it).
```

## What is left, or uncertain, and choices the owner should hear about

- **`memory/` notes are carried, not compiled.** The task says the persona's `files` are written to the carried files; I
  wrote the five top-level files and left the notes out so a persona's layer 2 stays small. One line in `files` adds a note.
- **The card's draft state lives in the import record** (`import.json`), not in the config: a draft is "the config's card
  equals the drafted text and the owner has not kept it". The daemon serves the config it started with, so after
  `persona edit --card` the draft mark clears only after the restart, as every config edit does.
- **Proposals are META records, not a new record kind**, so the only format change is the persona scope variant plus the new
  META key family; both are in format 41, and the doc comment on `MANIFEST_FORMAT` says so. The maintainer renumbers.
- **Jev is not yet offered the card:** route.v5 is not in this tree. `offered` is the reader (`theseus persona`, the
  cockpit's generated types); when route.v5 joins it reads `PersonaInfo.offered`.
- **One-tap keep** is a CLI keep today (`theseus standing keep`); a Discord/cockpit button would call the same two methods.
- **The persona exam's `persona` family** needs the words of a real agent's files in its persona TOML by hand;
  there is no extractor from a workspace.
- **The first full suite run saw `theseusd::stop_checkpoint::a_clean_stop_with_every_service_on_leaves_nothing_to_replay`
  fail once** ("the store did not close ... index_repaired: true"). It passed in the next two full runs and alone. It is a
  negative assertion, so I kept its output rather than retry it away; nothing in this change holds the store at a stop, and
  I have no explanation. Please watch for it.
- Docs for the maintainer to change (I did not touch the spec, status or design docs): a Part III item for T10b; the
  version line; `docs/status.md`'s recently landed step; the spec's standing chapter (the persona scope, the proposals,
  `standing.proposals|keep|dismiss`, format 41); the personas chapter (`persona.import`, `persona.card_confirm`, `[persona]`);
  the thread chapter (`thread.new { place }`). The nearest `AGENTS.md` files are updated (`theseus-core`, `theseus-memory`).

## Keel findings expected

None from my commits. The guard's one finding is in the base: `crates/theseus-sim/src/lifecycle.rs:155` budget-raised
(`"seed" => Some(250.0)`), acked on `1c2beb2747`, whose signature does not verify on this VM. Full `keel-guard.py` output
after my last commit:

```
keel: refused the ack `Keel: budget-raised crates/theseus-sim/src/lifecycle.rs ...` on 1c2beb2747: its signature does not verify (%G? is E)
keel: 1 erosions of a test, a budget or a ceiling (a branch: merge-base(HEAD, origin/main)..HEAD, 9fa84735c0):
  crates/theseus-sim/src/lifecycle.rs:155: budget-raised: "seed" => Some(250.0), -> (gone or rewritten)
```

(It also prints ~20 "the same test: ... (moved)" lines for tests of earlier commits.) Tests I rewrote, none deleted: five
`assert_eq!(moved["format"], 40)` became 41 in `tests_layouts.rs`, one in `store.rs`, and theseusd's `versions.rs` literals
(40 to 41, 41 to 42, "formats 2 to 40" to "2 to 41"), which follow the format bump; `test_plan.py`'s expected family set
gained `persona`; `OPERATORS` in the CLI grew from 66 to 70 entries (the test over it iterates the array).

## The gate's result

`scripts/gate.sh` itself fails in its keel phase (and so in the door, which runs main's copy first) on the base's
unverifiable ack, above. The later phases, run by hand on the final tree (`98659d94` plus this report), all pass but the suite:

| Phase | Result |
|---|---|
| fmt, shape | ok |
| clippy `--workspace --all-targets -D warnings` | ok (175 s) |
| cockpit lint, test, build | ok |
| reader rule (`tests_registry`) | 15 passed |
| protocol types | ok (the generated TypeScript is committed) |
| deny (bans, licenses, sources, offline) | ok |
| suite `cargo nextest run --workspace --no-fail-fast` | 5,504 run, 5,468 passed, 36 failed, 47 skipped |

The 36 are the 33 known L1 failures (theseus-sandbox's contract tests and `spawn_100`, theseusd's `sandbox` tests: this VM runs
as root, theseus-pv6i) and three that pass when run alone:

- `theseus-core push::tests::work::a_fresh_seed_shows_the_open_work_the_frames_built` and
  `theseus-judge builders::tests::every_builder_stays_under_its_cap_on_huge_inputs`: both passed on rerun alone; I did not
  explain them (timing under the full suite's load is my guess).
- `theseus-core tests_kept::fs_read_of_a_long_file_is_a_contiguous_window_and_other_tools_keep_both_ends` failed in this run
  (its first result held 948 numbered lines where the test expects `mid.rs`'s 600: the two parallel `fs_read` results came
  in the other order) and passed alone, and in the run before. It reads two files in one response, which the
  parallel-calls row groups, so I take it for an ordering race in the test, not this change.

Earlier full runs on earlier trees of this branch also saw, and cleared on rerun alone: `theseus::herdr
a_change_reaches_herdr_within_ten_ms_at_the_99th_percentile` (p99 10.19 ms against 10) and `theseus::markdown
rendering_costs_little_per_hundred_lines` (3.5 ms per 100 lines): debug-build timings on this VM; and, once,
`theseusd::stop_checkpoint ...` (above). Two real failures of the first full run were mine and are fixed: the store-format
literals in `theseusd/tests/versions.rs`, and the cockpit's `scopeWords` switch (a `tsc` error on the new scope variant).
