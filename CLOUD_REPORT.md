# Cloud report: the ladder, step 26a (theseus-0j2.15)

Branch `cloud/20261004-ladder`, from `main` at `802f913` (plus the task commit `130d665`). Started 18:22 UTC,
report written about 20:35 UTC.

## Commits

| Commit | Subject | Steps |
|---|---|---|
| `05604b8` | judge: route's pins and rerank's breaker opens as rollback rules | 5 (learn.rs's part) |
| `f4680bf` | judge: the ladder: pack modes in the store, arms, promotion, rollback and adoption, step 26a | 1 to 5, and 6's health line, `pack.list` and `theseus packs` |
| `06e6786` | cockpit: the Judgment section's ladder, with promote and roll-back buttons, step 26a | 6 (the cockpit) |
| `7d199fe` | judge: an answered promotion's card is read back at once, and a real daemon's ladder test | a bug the daemon test found in 3 |

**Why steps 1 to 5 share one commit.** The brief asks for one green commit per step. I built the ladder as
one module (`crates/theseus-core/src/judge/ladder/`) whose parts call each other: the first read writes the
missing adoptions (5), the standing reads today's brakes (4, 5), and the promotion and rollback tests drive
the mode in the store (1) and the arms (2) through the RPC (3). Splitting it would have meant committing
stubs that the next commit replaces, which is half a step each time. The learn.rs rules are their own commit,
and the cockpit is its own. Each of the four commits ran the gate on its own tree (the others stashed).

## Step by step

### 1. The mode in the store

**Found.** `PackMode` is off, shadow, canary, live; every point asked `self.cfg.mode_of(pack, Shadow)`;
`WIRED` had every pack in shadow. `pack.mode` is already in the design's row table (§2.5) with no layout
change: ledger rows, no `MANIFEST_FORMAT` bump.

**Changed** (`f4680bf`):
- `LedgerKind::PackMode` (`pack.mode`) and `LedgerKind::PackEvent` (`pack.event`), each written by a fact
  (`fact/ladder.rs`).
- `theseus-protocol/src/packs.rs`: `PackModeRow` (pack version, mode, from, share, who, by, via, why, report,
  holdout bounds, forced, numbers, rule, words, until, question, declined), `PackInfo`, `PackListResult`,
  `PackPromoteParams/Result`, `PackRollbackParams`, `HoldoutBounds`. TypeScript regenerated.
- `judge/ladder/mod.rs`: `Rung` (the four modes and `rolled_back`, which acts as shadow; I kept it out of
  `PackMode` so the config can never name `rolled_back`), `fold` (a version's rows onto its wired line;
  declined rows change nothing; a brake whose `until` passed leaves what it stood on before), and `Ladder`:
  rows scoped `pack:<id>`, read at the first read and kept, with the WIRED line when there is no row.
- `JudgeService::mode_for(pack, session)` is the one answer; `ask_mode` gives a point the judgment's mode
  and writes its arm. Every point now asks it: loop_end, gate, inbound, compile, categorize, rerank.
  `mode_of` is unchanged; a ceiling at shadow or below answers without reading the ladder.
- `Core::warm_ladder` (theseusd's `main.rs`, beside `warm_labels`): the first read after serving, on the
  blocking pool, judge on only. Health never loads the ladder itself.

### 2. Arms

**Changed** (`f4680bf`): `ladder::given_of`. A canary in its canary arm (`learn::arm(session, pack, share)`)
is `Live` with arm `canary` (its judgment's mode `canary`); the control is `Shadow` with arm `control`;
live and shadow are arm `all`. Nothing is stored on the session.

**Differs from the brief.** The arm is written into the judgment's context as **`pack_arm`**, not `arm`:
rerank.v1's context already uses `arm` for memory's arm (`"+rerank"`), and the two would collide. The
maintainer may want one name across the design doc.

**Memory's canary** (config/memory.rs) is untouched: its `bucket` is SHA-256(`"{experiment}\n{session}"`)'s
first 8 bytes shifted to 53 bits, while `learn::arm_fraction` is SHA-256(session, NUL, pack) over 64 bits,
so moving memory onto these arms would move a running experiment's sessions between arms. What it takes:
either a `learn::arm` variant that takes memory's hash (one function), or switching at a new `experiment`
name, which starts fresh arms on purpose; then `[memory] mode`/`canary_fraction` become a ladder row for a
pack id such as `memory:<experiment>`, under the config's ceiling as every pack is.

### 3. Promotion

**Changed** (`f4680bf`, `7d199fe`):
- `pack.promote`, `pack.rollback`, `pack.list` (`rpc/packs.rs`; one dispatch arm, `pack.*`, to keep
  `dispatch` within clippy's 100 lines). `theseus packs [list]`, `theseus packs promote <pack> --canary
  <share> | --live [--report <id>]`, `theseus packs rollback <pack> [--why …]` (`crates/theseus/src/packs.rs`).
  Both moves are in `OPERATORS`, and both are `Act::Ladder` through `judge_act`.
- The bar (`ladder/promote.rs`): a `judge.report` row of the version (cited by `--report`, else the latest of
  the last 14 days, by key, never a scan), whose holdout passes `learn::sufficient` (200 per deciding
  question, 30 per acting class), in which the version beats its baseline; a rolled-back version's report
  must be written after the rollback. `Core::promote_automatic` refuses one short with the numbers
  (`loop.v1 to live refused: short of the bar: work_state: labeled 37 of 200`); the owner's row says
  `forced` with the same numbers.
- **"Beats its baseline"** is not in the report's numbers, so I read it as the code can settle it: every
  acting class (`learning::report::ACTING`, today only `loop.v1`'s `work_state: progressing`) has a labeled
  precision above 0.50 on the holdout. A pack with no acting class listed has only the sample minimum as its
  bar. **This is a question for the owner**: a sharper comparison wants the baseline's own outcomes on the
  same holdout, which 26b's canary outcomes will give.
- **A security pack's promotion is a card**, the owner's or automatic. **Where its question lives**: a
  question is a planned action on an execution, and a promotion has none (the CLI and the cockpit open no
  session). It is planned on the ladder's own session (`the ladder: promotions waiting on the owner`, META
  `ladder.session`, opened at the first card): each card takes one short turn on that execution
  (`admit_input`, `plan_confirm_with`, `end_turn(Wait Input)`) so it parks again, and the question is listed
  by `confirm.list` and expires as any other. It never runs a model turn: nothing sends it input, and its
  answer wakes nothing (as an extension's ack). A session of its own keeps the cards off every conversation's
  transcript and spend. Approval writes the row in the answer's frame (`answer_promotion`); a decline or an
  expiry writes a `declined` row and no mode.
- **Not done: Discord's buttons for the card.** The ladder's session has no place, so no card post goes to the
  outbox; the card shows in `theseus confirm` and in the cockpit's questions (both read `confirm.list`). To
  put it on Discord, `ask_promotion` needs one `outbox.stage` to the owner's DM place (as `extend::ask` stages
  a card to its session's target), which needs a way to name that place from the core. I left it rather than
  guess at the bindings' owner DM.

### 4. Rollback

**Changed** (`f4680bf`): `ladder/rules.rs`.
- Every event a rule counts lands through `JudgeService::land(pack, event)`: a `pack.event` row scoped
  `pack.event:<id>:<day>`, so the first read after a restart reads the day's events back. Then every version
  of that pack id that acts (canary or live) runs `learn::check_all` on today's events after its latest row
  (so a promotion starts the count again). A pack in shadow is never rolled back: there is nothing to undo.
- `judge.label` (`rpc/learning.rs`, one call) lands a label in words (`noise`, `useful`, `wrong role`).
  The event row is a second frame after the label's; a crash between them loses that one count.
- The nightly run (`learning/tender.rs`, one line): after the report, `Ladder::recheck` reads the ladder again
  from the store and checks every pack.
- A rollback is a `pack.mode` row (`rolled_back`, `who: system`, `via: ladder`, the rule, its words), its
  sentence in the narrative's session part (as `judge.paused` is said), and health's line. `pack.rollback` by
  hand is the owner's row; it stands until a promotion.

**Found.** The judge's other notices (`judge.paused`, `judge.resumed`) are narrative lines only, so the
rollback notice is too; no Discord post. If the owner wants rollbacks in the learning channel, that is the
digest's code (25c), not this step's.

### 5. Adoption

**Found.** None of the three live packs is on `main` as cloned: `route.v1` has no pack file, and `rerank.v1`
and `security.v3` are wired in shadow with no live action.

**Changed** (`05604b8`, `f4680bf`):
- `learn.rs`: `RollbackRule::PinsPerDay { count }` and `OpensPerDay { count }`, events `Pinned { day }` and
  `BreakerOpened { day }`, the loader's check (count at least 1), and `CanaryEvent::day`.
- `ladder/adopt.rs`: `ADOPTED` (route.v1, rerank.v1, security.v3) and `rules(id)` keyed by pack id (route: 3
  pins a day; rerank: 2 opens a day; security: more than 30 notices, or 3 `noise` labels, a day). Read beside
  each file's own rules. A rollback by one of these rules is a day's brake: `until` the next local midnight.
- **A pack is adopted when this build wires it live** (`WIRED`'s line `Live`), not merely when its pack file
  is embedded: a row saying `live` for a pack with no live action would claim what nothing does. On this
  branch, therefore, nothing is adopted. Each live pack's session needs, at the merge:
  - **route.v1**: its `WIRED` line at `PackMode::Live`, and at each pin (the owner names another profile for
    a message within 10 minutes after a routed turn in that session) one call:
    `judge.land("route.v1", CanaryEvent::Pinned { day: judge.today() })`.
  - **rerank.v1**: its `WIRED` line at `Live`, and where its own breaker opens (where it writes the
    `judge.circuit` row that names it): `judge.land("rerank.v1", CanaryEvent::BreakerOpened { day: judge.today() })`.
  - **security.v3's notices**: its `WIRED` line at `Live`, and at each notice posted:
    `judge.land("security.v3", CanaryEvent::Notice { day: judge.today() })`. Its `noise` labels already land
    through `judge.label`. Its own brake reads as a rollback when its `judge.paused` row is **scoped `judge`**
    (design §2.5) with `what: "notices"` and `day`; on `main` today `judge.paused` rows are unscoped, so the
    notices step must scope its row for the ladder to read it (`rules::brakes_today`).
  - Each live point should also take its mode from `mode_for` rather than acting on its own switch, so the
    ladder and the config's ceiling decide.
- security.v3's file names no rules (as the brief says); the adoption gives them. Keyed by pack id, they
  apply to `security.v1` too (whose file already names the same two): a rollback of security.v1 by them is
  then a day's brake rather than a standing one. Say if security.v1 should keep the standing kind.

### 6. Surfaces

**Changed** (`f4680bf`, `06e6786`):
- Health's judge line per pack: a pack with no row keeps its old line (`loop.v1: shadow`), so existing tests
  and the cockpit's parse are unchanged; with a row, `route.v1: live (owner: decision of 2026-10-04)`,
  `loop.v1: canary 1.0 (owner: forced by the owner)`, `security.v3: rolled back until 00:00
  (notices_per_day)`; under a lower ceiling, `loop.v1: shadow (the config's ceiling; on the ladder: canary
  1.0 (…))`.
- `theseus packs`: each version's mode and why, its rules, its last five rows.
- The cockpit's Judgment section: a Ladder panel (`components/PackLadder.tsx`, over `pack.list`) with each
  pack's mode, why, rules and rows, and promote (a prompt for a share or `live`, then a confirm) and roll-back
  buttons; off while the time machine is set. `lib/packs.ts` reads health's lines and words a row;
  `test/packs.test.ts`.

## Proof

- **Unit tests**: `theseus-judge learn::tests::the_adopted_rules_fire_on_a_days_pins_and_opens_and_not_on_a_near_miss`;
  `theseus-core judge::ladder::tests` (4: rows folded, a brake lapsing, arms sticky and monotone, the next
  midnight), `judge::ladder::promote::tests` (the bar's numbers), `judge::ladder::adopt::tests` (the table's
  rules pass the loader's checks); `theseus packs::tests`; `theseus client::tests::a_jobs_process_cannot_promote_or_roll_back_a_pack`
  (a job's process refused for both moves; `pack.list` goes); cockpit `npm test` (packs.test.ts, 3).
- **`theseus-core tests_ladder.rs`** (11, whole cores on their own stores, the fake Jev for the arm):
  the config lowers and never raises; short of the bar the system is refused with the numbers and the owner
  forces; a promotion cites its report and its holdout's bounds; a security promotion is the owner's card
  (a shared place's answer refused, approval writes, a decline and an expiry write declined rows and no
  mode); a rule's trigger rolls a pack back (and a pack in shadow is not); the adoption written once after
  serving; each adopted rule fires on its scripted events (3 pins, 2 opens, 31 notices) and not one short; a
  restart keeps the day's count (2 `noise` labels, a restart, the third rolls back); a brake lapses at
  midnight on tokio's paused clock (live again a millisecond after, nothing written, the new day's count
  empty); `max_mode = "shadow"` caps every pack after a restart; a canary judgment records its arm.
- **theseusd `tests/judge.rs::the_ladder_moves_over_the_socket_and_max_mode_caps_it_after_a_restart`**: a
  real daemon, the protocol as the CLI sends it, and a restart with `max_mode = "shadow"` (it found the bug
  `7d199fe` fixes).
- **Planted reverts**, each restored and `touch`ed, `git status` clean after each:
  1. The config raising a mode (the ladder's mode used without `mode_of`, the fast path off): fails
     `the_config_lowers_the_ladders_mode_and_never_raises_it` and `max_mode_shadow_caps_every_pack_after_a_restart`.
  2. A security promotion without its card (`needs_card` never true): fails
     `a_security_promotion_is_the_owners_card` ("no mode before the answer").
  3. The adoption written at every start (the "done" check off): fails
     `the_adoption_is_written_once_after_serving` ("a restart adds none": 2 rows) and also
     `a_restart_keeps_the_days_count` (the second adoption row restarts the count).
  4. The day's count reset at a restart (`load` skipping `read_day`): fails `a_restart_keeps_the_days_count`.
  5. `forget` in place of `reload` after a card's answer: fails the theseusd ladder test on the approved
     pack's health line.
- **Under load** (AGENTS.md's recipe: `nice -n 19`, four busy loops at nice 0, killed by pid): five runs of
  `tests_ladder | ladder:: | tests_judge | tests_learning`, 38 tests each, 38 passed every run.
- **Benches**: the turn bench (`theseus-sim bench turn --check --runs 5 --burst 0`) after each commit:
  frames_plain 5 of 5, frames_tool 9 of 9. The lifecycle bench once (`--runs 10 --check`, debug build, this
  VM): `LIFECYCLE OK`; restart to the first answer p95 43.7 ms of 150, a binary swap p95 26.2 ms of 200.
  Nothing new runs before serving (the ladder's read is `warm_ladder`, after it).

## The live check (the maintainer's)

A scratch daemon on this build: its own config (a GLM profile; `[judge] enabled = true`; `[secrets]`
`jev_api_key` and the GLM key; Discord off; the cockpit on a free port), `--socket /tmp/ladder/sock`, a fresh
`--state-dir /tmp/ladder/state`. `T="theseus --socket /tmp/ladder/sock"`.

1. `$T packs`: on this branch **no pack is adopted** (none is wired live; see step 5), so every pack shows
   its wired line and `$T --json ledger -k pack.mode` is empty. After the live packs merge with their `WIRED`
   lines at `Live`: those packs show `live (owner: decision of 2026-10-04)`, one `pack.mode` row each, and a
   restart (`$T shutdown`, start again) adds none.
2. `$T packs promote loop.v1 --canary 1.0`: says `loop.v1 is canary 1.0, forced short of the bar (no learning
   report of loop.v1 in the last 14 days)`; `$T health` shows `loop.v1: canary 1.0 (owner: forced by the
   owner)`. An `ask` turn that ends with no tool calls: `$T judge log -n 1 --pack loop.v1` shows mode
   `canary`, and `$T judge show <jdg>` shows `context.pack_arm: canary`. `$T packs rollback loop.v1`: a
   `rolled_back` row, `who: owner`, and the narrative's sentence (`$T watch` or the cockpit's narrative).
3. `$T packs promote security.v1 --live`: a card id and no row (`$T packs` still shows security.v1 in
   shadow); `$T confirm` lists it; `$T confirm <id> --approve` writes it (`security.v1: live (owner: forced by
   the owner (approved))`). `THESEUS_SESSION=ses_any $T packs promote security.v1 --live` is refused before
   anything is sent (`theseus packs promote refused: … Run it from your own shell.`).
4. On this branch security.v3 is in shadow, so first `$T packs promote security.v3 --live` and approve its
   card. Then an `ask` turn that runs `proc.run echo hi`; `$T judge log --pack security.v3`; and
   `$T judge label <jdg> noise` on three of its judgments (or one, three times over three judgments): after
   the third, `$T health` shows `security.v3: rolled back until 00:00 (labels_per_day)` and the narrative
   says it. Two labels do nothing.
5. `[judge] max_mode = "shadow"` and a restart: `$T health` shows every pack in shadow, a promoted one as
   `… shadow (the config's ceiling; on the ladder: …)`; `$T --json ledger -k pack.mode` has no new row.

## Left, and uncertain

- Discord's buttons for a security card (step 3); the live packs' calls at the merge (step 5); the
  `judge.paused` brake's scope (step 5).
- "Beats its baseline" as the acting classes' precision above one half (step 3).
- The label's `pack.event` row is a frame after the label's (step 4).
- `pack_arm`, not `arm`, in a judgment's context (step 2).
- Health's line format changed only for a pack with a row; the cockpit's pack table reads either.
- `scripts/long-files.txt`: theseus-protocol's `lib.rs` ceiling raised from 2,678 to 2,681 for the three
  method names (their types are in `packs.rs`), in `f4680bf`, with the reason on its line.
- Docs for the maintainer: Part III's 26a item; `docs/status.md`; `docs/design/m5-judgment.md` §2.5's
  `pack.mode` row (add until, question, declined, numbers, `pack.event`), §2.7 (the adoption, the brakes,
  where a security card's question lives), §2.13 (`theseus packs`, the Ladder panel). I updated
  `crates/theseus-core/AGENTS.md` (the ladder's bullet under the judge) in `f4680bf`.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on each commit's tree: fmt, shape, features, clippy, the cockpit's
lint, tests and build, the test build and the reader rule pass. The suite (2,329 tests on the last commit):
2,295 passed, 34 failed, 17 skipped, every failure one of:
- **33 L1 sandbox tests** (`theseus-sandbox::contract`, `::bench spawn_100`, `theseusd::sandbox`): this VM runs
  everything as root, and L1 refuses a root daemon's jobs (theseus-pv6i). Known.
- **`theseus-core tests_output::the_cores_output_matches_its_golden`**: one line differs, a wake's time printed
  with `+00:00` where the golden has a negative offset; the VM's timezone is UTC. It passes with
  `TZ=America/Los_Angeles` on this branch. Not this step's.
- `theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults` failed once and passed on its retry
  in one run (it is on nextest's flaky list).

The phases after the suite, run by hand on each commit: the generated TypeScript is committed (clean), the
turn bench passes (5 and 9 frames), and `cargo deny --offline check` passes (advisories, bans, licences,
sources) after `cargo deny fetch`, which the setup had not reached.
