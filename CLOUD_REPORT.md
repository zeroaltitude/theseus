# CLOUD_REPORT: fresh-defaults (theseus-is8t)

Branch `cloud/20261011-fresh-defaults`, from main at fdd14925 (store format 27, unchanged: no stored record changed).
Commits: `c4730a0` (core, health, tests), `e5bfcce` (setup.sh and its tests), then this report.

## Step 1: recall on by default (c4730a0)

**Found.** `[memory] mode` defaulted to `off`; the template's `[memory]` was all comments.

**Changed.** `MemoryMode::default()` is now `Live` (`config/memory.rs`), and `MemoryConfig::default()` follows it, so
the code's default is what counts, and `scripts/setup.sh` needs to write nothing: its sparse cut has no `[memory]`
table. The arm is `baseline` already. The template says so, and its uncommented `mode = "live"` equals the default (the
template test now asserts `Config::example().memory == MemoryConfig::default()`). A config that sets `mode` keeps it
(test over all four values). The place rule is untouched: a shared place draws on its own sessions alone.

**Proved.**
- `config/tests_fresh.rs::a_config_with_nothing_in_memory_recalls_live_on_baseline`, and
  `tests_fresh_recall.rs` (new): a fresh config's turn in a private place (the CLI's) asks `bm25, entity, vector`,
  records `live baseline`, admits the earlier session, `recalled == 1`; the same config in a shared place admits
  nothing of the CLI's session (dropped as `place`), `recalled == 0`; `mode = "off"` asks the index nothing.
- Lifecycle bench, `theseus-sim bench lifecycle --runs 10 --check`, this 4-core VM, debug builds, run once each way:
  before (stash of the change) cold start p50 23.2 / p95 37.2 ms, config-copy start 22.0 / 30.5, clean shutdown 13.1 /
  21.6, SIGKILL restart 26.9 / 92.3, swap 28.7 / 40.5, cancel 11.2 / 15.0: `LIFECYCLE OK`. After: 21.5 / 26.2, 23.2 /
  32.3, 13.2 / 16.9, 27.2 / 45.0, 26.2 / 33.5, 12.7 / 22.0: `LIFECYCLE OK`. Unchanged within noise. The bench's own
  daemon runs recall in shadow; the turn bench (`bench turn --check --runs 5`) holds 5 / 9 / 15 frames.
- A live recall on the stand-in model: see "Live check".
- Planted revert (default back to `Off`): 6 tests fail (`memory_is_live_by_default_and_off_turns_it_off`,
  `example_template_uncommented_still_parses`, `tests_fresh::a_config_with_nothing_in_memory…`,
  `tests_fresh::the_sparse_config_stays_sparse`, and both `tests_fresh_recall` place tests). File restored and touched.

## Step 2: the judge follows its key (c4730a0)

**Found.** `enabled: bool`, default false; the template said `enabled = false`, which a sparse cut keeps, so setup would
have written a config that kept the judge off.

**Representation.** The TOML key `enabled` is `JudgeConfig::enabled_setting: Option<bool>` (skipped when None, so a
config that says nothing stays sparse). `enabled: bool` stays the field everything reads (60-odd sites, tests that set
it), is `serde(skip)`, and is settled by `JudgeConfig::resolve` as the config loads (`Config::from_toml`). `auto_why`
holds the reason. The template's `enabled = false` line is now a comment (an uncommented-template test still parses it).

**Auto means.** `key_secret`'s entry decides: `env:` on when the variable is set and non-empty; `file:` on when it
reads (mode 0600 rule included); `op://` on when it is not a placeholder (`<`..`>`); off with no entry, an unset
variable, an unreadable file, or a placeholder. `enabled = false` is off whatever the key; `true` with no entry still
refuses to load (the old check). Health's `judge` block gains `why`, `theseus health` says `judge: off (<why>)`, and
`theseusd check` and health's first line say which and why.

**A choice to hear.** The vault is never asked at the start (serve first), so an `op://` reference turns the judge on
at load and its key resolves behind the socket, exactly as `enabled = true` always behaved (health's `key:` says
`resolving`/`failed`). Only `env:` and `file:` are tested for resolving at load. If the owner wants "on only once the
vault answered", that needs the judge's `enabled` to become dynamic (60-odd readers): not done.

**Packs.** Unchanged: routing (route.v3) and rerank live, security.v3's notices live (its default, ahead of this task),
every other pack in shadow. The task text says "except routing and rerank"; security.v3 notices already acted before
this row.

**Proved.** `config/tests_fresh.rs`: `the_judge_is_on_exactly_when_its_key_resolves` (env set, unset, placeholder, no
entry, vault ref, file present and removed), `enabled_false_wins_and_true_with_no_key_still_refuses`,
`an_auto_judge_keeps_each_packs_own_mode`, `the_sparse_config_stays_sparse` (a config that says nothing: no `[judge]`,
no `[memory]`; a deliberate `false` and `true` stay in the note; the template's cut has neither table).
Planted reverts: the config's word ignored (4 fail), auto never on (4 fail), a placeholder counted as a key (2 fail).

## Step 4: health's first line (c4730a0)

`HealthResult.features` (`theseus-protocol/src/features.rs`: `line`, `memory`, `jev`, `scrub`, `budgets`), built by
`theseus_core::features::of` from the config and the judge's key state; `theseus health` prints `features.line`
first, `--json` carries the field, `theseusd check` prints the line too. Example, a fresh install with no key:

    features: memory live (baseline) · Jev off (no jev_api_key entry under [secrets]) · secret scrub always on · budgets: daily ceiling $200

With the key: `Jev on, routing live` (or `routing shadow`, or `key failed: …`). Generated TypeScript regenerated
(`Features.ts`, `HealthResult.ts`, `JudgeHealth.ts`, `index.ts`). One health line, no frame, no protocol method.
Tests: `features::tests` (every state), `render/features.rs` (first line, older daemon, JSON field), `theseusd`
`default_config.rs` (check's line). Planted revert of the render call: its test fails.

## Step 3: setup asks for the key (e5bfcce)

`scripts/setup.sh` asks once, when it writes a new config, for the Jev key's reference: `op://<vault>/<item>/<field>`
or `env:NAME`; anything else (a key typed by mistake) is refused and asked again, three times, then taken as a skip. It
writes the reference as `jev_api_key`; an empty answer deletes the entry (judge off, said, and setup is not stopped for
that placeholder). Asked only on a terminal without `--yes`; otherwise the placeholder stays and the run says the judge
stays off until it is filled in. New options: `--yes`, and `--jev-key REF|skip` (a reference only; a value exits 2 with
nothing written). Step 7 shows the features line. `docs/setup.md`, `scripts/AGENTS.md` updated.

**Proof.** The script refuses root and needs cgroup v2; this VM is root without cgroup v2, so `setup_script.rs` skips
here. I ran the compiled test binary as `nobody` (`setpriv`) with the preflight's cgroup line bypassed in a scratch
copy of the script (never in the tree): 6 of 6 pass, the three old ones and three new ones (`…never_asks_for_the_jev_key`
with no terminal and under `--yes`, `jev_key_names_a_reference_or_skips_and_never_takes_a_value`,
`on_a_terminal_the_jev_key_is_asked_once…`, which drives a real pty from python3: answer, skip, `--yes` on a terminal,
and a typed key refused three times and never in the config). Planted reverts: asking without the terminal test (2 fail),
accepting any reference (2 fail), ignoring `--yes` (1 fails). The maintainer can run them on a normal machine.

## Live check (this VM, scratch daemons, nothing else touched)

setup.sh itself (as `nobody`, stand-in systemctl, bypassed cgroup line) wrote two configs: `--jev-key skip` and
`--jev-key env:JEV_LIVE_KEY`. Both are sparse (no `[memory]`, no `[judge]`). Scratch daemons on them, stand-in model
(`theseus-sim fake-model`), a python stand-in Jev that answers 500:
- No key: `theseus health` line 1 = `features: memory live (baseline) · Jev off (no jev_api_key entry under [secrets]) · …`;
  `judge: off (no jev_api_key entry under [secrets])`.
- `JEV_LIVE_KEY` set: `features: memory live (baseline) · Jev on, routing live · …`; `judge:` lists route.v3 live,
  rerank.v1 live, security.v3 live, the rest shadow.
- Recall on the no-key daemon, three seeded sessions then "Where does the red kite roost?": index `ready · bm25_only ·
  9 nodes`, `recalled: 6`, `memory recalled` shows `live · baseline · private · ran`, the kite note first.
  The key daemon recalled 1 after one seed.

**For the maintainer, on the owner's machine** (never the operator's daemon; a copy of the config and a scratch state):

    theseusd --config <owner's note> config --sparse > /tmp/owner.sparse
    grep -nE '^\[(memory|judge)\]|^(mode|enabled|arm) ' /tmp/owner.sparse     # a line here is the owner's own word and wins
    theseusd --config <note> --socket /tmp/s.sock --state-dir /tmp/s-state &   # a fresh state dir
    theseus --socket /tmp/s.sock health | head -1                              # features: …

An owner overlay that pasted the whole template keeps `enabled = false` under `[judge]` (the old template's line) and
`mode = "shadow"`/`off` under `[memory]` if it had them: those stay as written. Remove them to take the new defaults.

## Defaults changed (every config that omits the key)

1. `[memory] mode`: `off` → `live` (arm `baseline`, unchanged). Consequences beyond recall in front of the model, all
   already the design for memory on: the memory pass after each turn (reads the transcript, and with the judge on
   calls memory.v1/attribution.v1 in shadow), recall rows (`recall.ran`, `memory.arm`) in every turn's existing frames
   (frame count unchanged), and **the nightly consolidation** (`[memory] consolidate_hour`, `synth_limit_usd_per_day`
   0.50): it calls the `synth_profile` model on clusters, but only `arm = "+synthesis"` reads its output, so on
   `baseline` it is spend with no reader. I did not change it; the owner may want it gated on the arm.
2. `[judge] enabled`: unset was `false`; now auto (on when the key resolves). With the judge on, every judgment sends
   the session's state (messages, final text, trimmed tool calls, scrubbed of secrets) to Jev, and shadow packs spend
   up to `shadow_limit_usd_per_day` ($1) and the learning loop up to its own limits. A fresh install with a key
   therefore starts sending to Jev without a further yes: the owner decided it ("the judge on whenever a Jev key
   resolves"), but it deserves saying.
3. The template: `[judge] enabled = false` is now a comment; `[memory]`'s example line is `mode = "live"`.

## Test changes that moved (not deleted; keel guard: 0 findings, the two renames pair as the same test)

`memory_is_off_by_default_…` → `memory_is_live_by_default_and_off_turns_it_off`; `the_judge_is_off_by_default_…` →
`the_judge_is_off_without_a_key_…`; judge `cfg()` helper now settles the table; `tests_m3`'s frame-budget row list gains
`recall.ran` (frames still ≤ 5, and `bench turn` 5); judge-counting tests and the transcript-read, tool-time, output
golden and categorize rigs set `memory.mode = off` (recall brings its own judgments, frames and reads), so no golden
moved; `theseusd`'s `safe_note` sets `[judge] enabled = false` (a stand-in `op://Test/…` key would otherwise turn the
judge on, pointed at the real Jev endpoint in every daemon test).

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=fdd1492 CARGO_INCREMENTAL=0 scripts/gate.sh`: door, keel,
keel tests, fmt, shape, features, clippy, cockpit, test build, reader rule: ok. Suite: 3758 run, 3723 passed, 35 failed:
- 33 known L1 (22 theseus-sandbox contract/bench, 11 theseusd sandbox: root without a job cgroup, theseus-pv6i);
- `tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn` (named flaky, theseus-b38m): it failed
  alone too, and passed with recall off. Recall's awaits on the test's one thread let its stall land inside a read's
  run. That is the test's single-thread artefact, not a product finding, so its rig sets memory off (comment says why).
- `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer` (21 s under load; passes
  alone in 1.2 s).
Phases after the suite, run by hand: protocol types (no diff after the protocol suite), turn bench (5/9/15 frames: ok),
`cargo deny --offline check` (advisories, bans, licenses, sources ok). The lifecycle and jobs benches are skipped under
`THESEUS_GATE_NO_BENCH`; the lifecycle bench was run alone (above). A final rerun of theseus-core, theseus-discord, theseus,
theseus-protocol and theseusd after the last edit: 2508 run, only the L1 sandbox failures.

Disk note: this VM's 39 GB allowance filled twice from incremental/stale test binaries; it needed `CARGO_INCREMENTAL=0`
and pruning old `target/debug/deps` generations.

**Keel findings expected.** None. `scripts/keel-guard.py` against fdd1492: `keel: ok (… 37 files changed, 0 findings
acked)`; against the stale `origin/main` in this clone (a9ad950) it lists findings from rows joined since, which are
not this branch's.

## Where docs should change (not edited here)

Spec Part III item and chapter on M6/recall (live by default), the version line, `docs/status.md` (recently landed, the
roadmap's R1 row), `docs/technical-overview.md` and `docs/design/m6-memory.md` where they say memory is off by default,
and the first-ten-minutes note. Say the judge now follows its key.

## Left open

- `op://` auto-on is by reference, not by an answer (above).
- Nightly consolidation spends on `baseline` with no reader (above).
- The cockpit shows nothing of `features` yet (the type is generated; a header chip would read it).
