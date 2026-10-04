# CLOUD REPORT: replay, audit and backfill for the learning ledger, step 25d (theseus-0j2.14)

Branch `cloud/20261004-replay`, built on `main` at `802f913` as cloned (its first commit is the task, `da43cf4`).
Started 18:22 UTC, report at 20:28 UTC (deadline 23:22).

| Step | Commit | What |
|---|---|---|
| 1. Replay | `3ab2a6f` | `judge.replay`, `theseus judge replay`, `learning/replay.rs`, `learning/rebuild.rs`, `theseus_judge::replay`, the fake Jev's per-state scripts |
| 2. Audit | `9307a93` | `judge.audit`, `theseus judge audit`, `learning/audit.rs` |
| 3. Backfill | `41dbb92` | `judge.backfill`, `theseus judge backfill`, `learning/backfill.rs`, `event_at_ms` as a judgment's time |
| 4. The owner's runs | in 1 to 3 | `Act::JudgeRun` through `judge_act`, each method in `OPERATORS`, each run on a `learning` thread at nice 19 |
| Docs | `02ca798` | theseus-core's `AGENTS.md`: the learning ledger's entry names 25d's modules, scopes, keys and tests |

## 1. Replay (`3ab2a6f`)

**Found.** `pack_report` looked its pack up by name (`by_name`), so a candidate the build does not embed could not be
reported; `graded` and `answer` were private to `report.rs`; nothing kept a builder's name and version together with its
`Builder`; the fake Jev scripted answers per question only, so no test could make one wording fix one judgment and break
another; the judge's client was reachable only through the recording sink, which scopes every row `judge:<pack id>`.

**Changed.**
- `theseus_judge::replay` (pure): `what_changes` (a candidate that changes nothing Jev reads, only thresholds or which
  questions decide, is `ThresholdsOnly`; one that changes a question's instructions or criteria, the model, the
  builder or its cap is `Asks`), `stored_state_differs` (builder, builder version, cap), `stored_state` (a blob back to
  a `BuiltState`, its sha256 checked against the judgment's), `reband`, `builder_identity`.
- `theseus_judge::fake`: `FakeJev::script_when(state_has, criteria_has, question, answer)`: a rule per state and per
  criterion text, first match wins, before the plain script.
- `learning/report.rs`: `pack_report_of(Option<&Pack>, …)` beside `pack_report` (unchanged), so a candidate's `Pack`
  gives its questions, kinds and baseline; `graded` and `answer` are `pub(crate)`.
- `learning/replay.rs`: `Core::judge_replay` (public, async: the learning loop can call it in-process). The candidate
  (embedded by name, or `pack_text` through `Pack::parse`; never a path), refused when wired, when its id is not the
  incumbent's, when it is the incumbent, or when any `judge.call` or `judge.replay` row of the pack holds its name under
  another sha256. The set: a report's frozen holdout (labels: the report's frozen ones, `Holdout::labels`), its train
  split (answered, before the window, never the holdout's; today's labels), `--errors` (only the labeled judgments the
  incumbent got wrong), or ids. Each judgment's counting label per question is made absolute (`{"not": c}`, a class, a
  bool, a level), so both sides are graded by one truth. States: stored when builder, version and cap match, else
  rebuilt, else left out with the reason. Calls one at a time through `JudgeService::jev()` (the client and breaker),
  `Urgency::Shadow`, in a `Caller` that reserves each call inside the run's limit and settles at its cost (its
  reservation when usage is unknown). Both sides through `pack_report_of`; per judgment, fixed and broken; agreement
  with the incumbent; per Choice class, `fell`. A `security` candidate also runs eval.rs's `security.v3` planted set,
  each case asked of both versions, met and missed. One frame: each call's `judge.call` (`budget: replay`, context
  `purpose: replay`, `run`, `rejudges`, `state`) and the `judge.replay` row (`rpl_…`, the result whole, the
  candidate's text in a blob), all scoped `judge.replay:<pack id>`.
- `learning/rebuild.rs`: `loop.v1`'s input from the turn's `turn.ended` row (loops, cost, tool calls, stop reason) and
  the session's nodes up to the row's time, through `judge::loop_end::input`; `unrebuildable(Builder)` says why each
  other builder's input can't be.
- Config `[judge] replay_limit_usd = 0.5` (default, template line, validation). Protocol: `judge.replay`,
  `judge_runs.rs`'s types (TypeScript regenerated), `LedgerKind::JudgeReplay`; `fact/judge_runs.rs`; the CLI's
  `judge replay` (`crates/theseus/src/judge_runs.rs`, the side-by-side lines in `render/judge_runs.rs`).
- `scripts/long-files.txt`: theseus-protocol's `lib.rs` ceiling 2678 → 2681 (the method line, its doc, the module
  line), with the reason in the entry.

**Proved.**
- `cargo nextest run --workspace -E 'test(/tests_replay|replay::tests/)'`: theseus-core's 5 `tests_replay` and
  theseus-judge's 3 `replay::tests` pass:
  - `a_replay_over_a_frozen_holdout`: four seeded `loop.v1` judgments, three labeled, a report run; `loop.v2` (the
    `progressing` option's `means` reworded) against a fake scripted per state: 4 called with stored states, labels
    `frozen`, the incumbent's `questions` equal to the stored report's `holdout.questions`, heron fixed and wren
    broken (`fixed 1, broken 1`), every request carrying the new criterion; 4 call rows and 1 run row in
    `judge.replay:loop`, still 4 in `judge:loop`, a later report counting `loop.v1` 4 and no `loop.v2`; `--errors`
    replays heron alone.
  - `a_thresholds_only_candidate_makes_no_call`: `act` 0.90 → 0.75 on `work_state`: `thresholds_only`, 0 calls, the
    fake saw nothing, act-band count 0 → 4.
  - `a_state_is_rebuilt_when_the_builder_changed_and_left_out_when_it_cant_be`: a judgment with builder version 0 is
    rebuilt from a real turn (the request carries the turn's ask and reply); `security.v2` over a `security.v1`
    judgment leaves it out ("built by security, the candidate's by security2, and it can't be rebuilt: a gate state's
    input …") and runs the planted set (2 calls a case).
  - `a_replay_past_its_limit_is_refused_with_the_numbers`, `a_replay_is_the_owners_and_a_name_means_one_text` (a
    shared place's refusal is ledgered as `approval.refused` with `act: judge.replay`; a wired version, another pack's,
    a pack file under `loop.v1`, and `loop.v2` under a second text are refused).
- **Planted revert** (replay rows written into `judge:<pack id>`: `write_replay`'s scope set to
  `rpc::judge::scope_of`): `a_replay_over_a_frozen_holdout` fails at `tests_replay.rs:311` (`left: 0, right: 4`: no
  call rows in `judge.replay:loop`). File restored and touched; `git status` clean.

## 2. Audit (`9307a93`)

**Found.** Nothing called a model outside a session; compaction's summary call (30c is on main) reserves through a kernel
action under its turn's execution, which an audit, outside any session, does not have.

**Changed.** `learning/audit.rs`: `Core::judge_audit`. The pack by name (`loop.v1`) or id (its wired version); the
profile resolved with `TurnRunner::resolve_target`, its model priced in the catalog or refused. The eligible set:
answered judgments of that version with no `source: audit` label; the sample: sha256(seed, id) order, the first n
(the seed defaults to the run id's hash and is in the result). One request per state: the blob's JSON, and each whole
question with options of its own as Jev reads it (instructions; a Choice's ids and `means`; a Noul's `when_true` and
`when_false`; a Score's numbered levels), answered as one JSON object. A value outside its options, of the wrong type,
or for a question the pack does not ask is counted (`dropped`) and dropped. Each request is reserved at the catalog's
prices (`compiler::estimate` and `CatalogEntry::reserve_micros`), in memory against the run's cap, and settled at
`cost_micros(usage)` (the reservation when it failed); the run stops before a request's reservation would pass
`[judge] audit_limit_usd` (new, 5.0), saying so. Labels: `judge.label`, `source: audit`, weight 0.5, `rule` the run id,
keyed `labels::system_key(judgment, question, run)`, scoped `judge:<pack id>`, in one frame with the `judge.audit` row
(`aud_…`); nothing written when nothing was asked. An audit label says no sentence of its own (as a system label); the
run says one. Protocol `judge.audit`, `LedgerKind::JudgeAudit`, the CLI's `judge audit`.

**Proved.** `tests_audit` (4): `an_audit_labels_a_seeded_sample_once` (sample 3 of 5: 3 requests, 6 labels, 6 dropped,
each request carrying the state and the criteria; the report counts 6 audit labels; the next run draws the 2 left; a
third finds none and writes nothing), `a_seed_draws_one_sample`, `an_audit_stops_at_its_per_run_cap` (a cap of one
request's reservation: 1 asked, `stopped` names the cap, cost inside it), `an_audit_from_a_shared_place_is_refused`.
**Planted revert** (the cap check skipped): `an_audit_stops_at_its_per_run_cap` fails at `tests_audit.rs:186`
(`left: 5, right: 1`). Restored and touched; `git status` clean.

## 3. Backfill (`41dbb92`)

**Found.** The holdout split read the row's write time (`Seen::at_ms = row.at_unix_ms`), so a judgment written today of
last week's event would land after any frozen window. The live `loop.v1` point reads the session's nodes when its
spawned task runs, after the turn, so a next message that lands first becomes the live state's `ask` (a race in the
live point, not in backfill, which reads nodes up to the event; see "Left").

**Changed.** `learning/backfill.rs`: `Core::judge_backfill`. Refused, after `judge_act`, without `[judge]
backfill_consent = true` (new, false), naming the line; refused for a pack whose input can't be rebuilt, with
`unrebuildable`'s reason (rerank's and CONTINUE's included). Events: `turn.ended` rows since the local day's midnight
(`day_start`, through the ledger index's kind tag), `stop_reason == "no_tool_calls"`, in the pack's shadow sample
(`judge::sampled`, now `pub(crate)`). Each state through `rebuild::loop_input` with the event's time as `now`; context:
the live point's fields (session, turn, loops, baseline, decision, class, blob, `on_path_ms`) plus `purpose: backfill`,
`run`, `event_at_ms`. Keyed by `event_id` (sha256 of pack, session, turn): skipped when a live judgment of that turn
exists or a row holds the key. Spend through replay's `Caller` against `replay_limit_usd`, estimate first. One frame:
the `judge.call` rows (`budget: backfill`) scoped `judge:<pack id>`, and the `judge.backfill` row (`bkf_…`, `consent`
= sha256 of the running config's JSON); nothing written when nothing was judged. `learning::read_scope` takes
`context.event_at_ms` as a judgment's time when present. Protocol `judge.backfill`, `LedgerKind::JudgeBackfill`, the
CLI's `judge backfill`. The three methods share one dispatch arm (`rpc/judge_runs.rs::RUNS`), keeping `dispatch`
inside clippy's 100 lines; lib.rs's ceiling 2681 → 2682.

**Proved.** `tests_backfill` (4): `backfills_states_equal_the_live_builders_byte_for_byte` (three turns with `loop.v1`
live against the fake; each live blob equals `backfill_state`'s JSON byte for byte; a backfill then finds all three
judged and writes nothing), `a_backfill_judges_each_event_once_under_consent` (2 events judged, keyed by event, the
live context and `purpose: backfill`, the consent digest, `Seen::at_ms == event_at_ms`; a second run: 2 already, no
new rows, no new call), `a_backfill_needs_consent_and_a_rebuildable_pack` (the consent line named and nothing sent;
`rerank.v1` and `continue.v1` refused with their reasons; a shared place's backfill and audit refused and ledgered),
`since_is_a_local_day`.
**Planted reverts:**
- The consent check skipped (`if false && !…backfill_consent`): `a_backfill_needs_consent_and_a_rebuildable_pack`
  fails at `tests_backfill.rs:175` (`unwrap_err()` on an `Ok`: the run judged 1 event without consent).
- Keyed by run, not by event (`event_id` over the pack's name and the run id):
  `a_backfill_judges_each_event_once_under_consent` fails at `tests_backfill.rs:135` (the row's key is not the
  event's).
- Each file restored from its copy and touched; `git status` clean after each.

## 4. The owner's runs (in `3ab2a6f`, `9307a93`, `41dbb92`)

**Changed.** `rpc::Act::JudgeRun { method, what }`, judged by `judge_act` as every approval-like act (the owner, from a
private place; a refusal is an `approval.refused` row with `act` the method, and a sentence); `judge.replay`,
`judge.audit`, `judge.backfill` in the CLI's `OPERATORS`, so `client::refuse_in_a_job` refuses each inside a job
before anything is sent. Each run is `learning::tender::on_low_thread` (a `learning` thread at nice 19), holding the
core by `Weak` until it starts, entering the runtime only to drive its own calls; nothing runs at the start or on a
turn's path. The CLI prints a replay's two versions in two columns (`render/judge_runs.rs`).

**Proved.** The shared-place refusals in `a_replay_is_the_owners_and_a_name_means_one_text`,
`an_audit_from_a_shared_place_is_refused`, and `a_backfill_needs_consent_and_a_rebuildable_pack` (each ledgered, with
nothing sent); the CLI's `client::tests::the_operators_methods_are_refused_inside_a_job` iterates `OPERATORS`, so it
covers the three new methods. Not run here: the job-shell refusal against a live daemon (live check 5).

## Runs under load

`scratchpad/load.py`: four `sh -c 'while :; do :; done'` at nice 0 (killed by their pids), the tests at `nice -n 19`
on a prebuilt test binary: `cargo nextest run --workspace -E 'test(/tests_backfill|tests_audit|tests_replay/)'`, three
runs, 13 of 13 each time (6.3 s alone, 17 to 19 s under load).

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run with `TZ=America/New_York` for steps 2 and 3:
- Every phase passes but the suite, which fails only on the root VM's known L1 tests (theseus-pv6i): 33 of 2,324,
  all `theseus-sandbox::contract` (19), `theseus-sandbox::bench spawn_100`, and `theseusd::sandbox` (13), each
  saying "the daemon runs as root, and Linux exempts root from RLIMIT_NPROC". Gate 1 (step 1) also had
  `theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults` fail once and pass on its retry (it is on
  `.config/nextest.toml`'s flaky list).
- `theseus-core tests_output::the_cores_output_matches_its_golden` fails on this VM under its own `TZ` (UTC: a
  wake's offset in the golden reads `-#:#`, written west of UTC; here it is `+#:#`) and passes with
  `TZ=America/New_York`, so steps 2 to 4 ran the gate under that zone. Not this branch's: the line is a wake's.
- The phases after the suite, run by hand each time (`machine_checks`): the protocol types (`cockpit/src/protocol.gen`
  matches the commit), the turn bench (`theseus-sim bench turn --check --runs 5 --burst 0`: plain 5 frames, tool 9,
  both within budget, unchanged), and `cargo deny --offline check` (advisories, bans, licences, sources ok;
  `cargo deny fetch` succeeded at setup). The lifecycle and jobs benches are the joining gate's
  (`THESEUS_GATE_NO_BENCH=1`); nothing on the start path changed: every run is built on its first use, after
  serving.
- No dependency was added: `Cargo.lock` and the package-lock files are unchanged.

## The live check (the maintainer's)

A scratch daemon with `[secrets] jev_api_key` and `zai_api_key`. `/tmp/t25d/config.toml`:

```toml
[server]
state_dir = "/tmp/t25d/state"
socket = "/tmp/t25d/theseus.sock"
[model]
live = "glm"
[discord]
enabled = false
[web]
enabled = false
[judge]
enabled = true
[secrets]
jev_api_key = "op://…"
zai_api_key = "op://…"
```

```bash
theseusd --config /tmp/t25d/config.toml --socket /tmp/t25d/theseus.sock --state-dir /tmp/t25d/state &
T="theseus --socket /tmp/t25d/theseus.sock"
# 1. Four asks, three labels, a report.
for q in "What is 2+2?" "Name a prime over 10." "Say hello." "List three colors."; do $T ask "$q"; done
$T judge log --pack loop.v1                       # four loop.v1 judgments
$T judge label <jdg_1> right; $T judge label <jdg_2> wrong; $T judge label <jdg_3> right
$T judge report                                   # writes rpt_<today>_loop.v1
# 2. An audit: audit labels on the sampled judgments.
$T judge audit loop.v1 --sample 5 --profile glm
$T --json ledger -n 200 -k judge.label | grep -o '"source":"audit"' | wc -l
# 3. A replay of a reworded candidate.
sed -e 's/^version = 1$/version = 2/' -e 's/Work toward the ask remains/Plainly, work toward the ask remains/' \
  crates/theseus-judge/packs/loop.v1.toml > /tmp/loop.v2.toml
$T judge replay --pack-file /tmp/loop.v2.toml --report rpt_<today>_loop.v1     # both versions side by side
$T judge replay --pack-file /tmp/loop.v2.toml --report rpt_<today>_loop.v1 --split train --errors
$T judge replay --pack-file /tmp/loop.v2.toml --judgments <jdg_1>,<jdg_2>,<jdg_3>,<jdg_4>   # today's, by id
# 4. A backfill is refused without consent, naming the line (never add the line in a live check).
$T judge backfill loop.v1 --since <yesterday>     # "… `backfill_consent = true` under [judge] … Nothing was sent."
# 5. Inside a job's shell, refused before anything is sent.
THESEUS_SESSION=ses_0000aa1b2c3 $T judge replay --pack-file /tmp/loop.v2.toml --report rpt_<today>_loop.v1
$T shutdown
```

What each should show:
1. `judge log` four `loop.v1` judgments; `judge report` prints the `loop.v1` block with 3 labeled and its holdout,
   and `rpt_<today>_loop.v1` is its key.
2. `… 4 of 4 eligible judgments sampled, 4 asked, 0 failed; N audit labels, M answers dropped · $… (limit $5.00)`:
   four asks make four judgments, so `--sample 5` takes all four (run five asks for five). Each state gives one label
   per `loop.v1` question GLM answers inside its options (six questions), so the count printed is N, between 4 and 24;
   each row says `"source":"audit"`, `"weight":0.5`, and `"rule":"aud_…"`. A second identical command prints `0 of 0
   eligible` and writes nothing.
3. A header naming `loop.v2` (its sha256) beside `loop.v1` over the holdout (labels `frozen`), `asked again: 4 called
   (4 stored states, 0 rebuilt) …`, then two columns per question (labeled, bands, Brier, ECE, each class's precision
   and recall), `agreement with the incumbent`, any fixed and broken judgments, and the cost under the $0.50 limit.
   With every judgment made today the holdout's window (the 14 days before today's midnight) is empty: the replay
   says `0 judgments` until the report's window holds some (a backfill with consent, or judgments a day old), and so
   does the train split (`train`, labels `today`). The run by ids replays the four (`judgments`, labels `today`):
   both columns, and `fixed`/`broken` against the three labels.
4. "… a backfill sends your recorded history to Jev, so it runs only under your consent: `backfill_consent = true`
   under [judge] in your config note, which agents can't write. Nothing was sent."
5. `theseus judge replay refused: it is the operator's to run, and this shell is a Theseus job's
   (THESEUS_SESSION=ses_0000aa1b2c3 is set) …` before anything is sent.

## Left, uncertain, and for the owner

- **Only `loop.v1`'s input is rebuilt.** Security's needs the gate's decision and the hold at the call; inbound's the
  live tasks and roles table at the message; CONTINUE's the compile's tail and cache reads; categorize's the ontology
  as it stood; rerank's recall's candidates. Each refuses with that reason. Recording each point's builder input (or
  its node positions, as §2.5 says) in the judgment's context would make replay and backfill possible for all.
- **Packs whose questions draw on dynamic items** (classify's tasks, role's roles, categorize's topics and
  memberships, rerank's notes) can't be replayed from a stored state either: the record keeps the state's JSON, not
  the builder's `Dynamic` items. Storing them in the judgment's context (they are small) would close it.
- **Replay's and backfill's spend** is a per-run cap in memory (`replay_limit_usd`), never `ShadowBudget`: I read "a
  replay never pauses shadow judging" as "it does not draw on the day budget". Design §2.6 says replay, backfill and
  audit are paid by the judge's own budget; if the owner wants them to count against the day's spend without pausing
  it, `spend.rs` needs a draw that never trips the pause. Each run's cost is in its row.
- **The audit's reservation** is held in memory against the run's cap, not as a kernel action: with no session there
  is no execution to plan it under. A crash mid-run loses at most one request's cost from the record (the run's row is
  written at its end).
- **The live `loop.v1` point's race**: `judge_loop` reads the session's nodes when its task runs; a next turn's
  message that lands first becomes the judged `ask`. Backfill reads nodes up to the event's time, so for such a turn
  the two states differ (the byte-for-byte test waits for each judgment before the next turn). Worth a fix in
  `JudgeService::loop_state`: read nodes up to the turn's end.
- `backfill_consent` is read from the running config; the digest recorded is sha256 of the config serialized as JSON
  (`config_copy::sha256`), not the note's own bytes. If the owner wants the vault note's digest (the one the config
  copy records), that is a one-line change once that value is reachable from `Core`.
- The planted-injection set exists only for `security.v3`'s input; it is reused for any security candidate.
- The `--errors` filter applies after the set is chosen, so `--split train --errors` is the train split's errors.

## The paragraph §2.9's "Replay, backfill, and audit" should gain

> **As built (25d).** `theseus judge replay <candidate>` asks a candidate (an embedded version the build does not wire,
> or `--pack-file`, parsed with every loader rule; a name means one text) the incumbent's questions over a report's
> frozen holdout (its frozen labels), its train split, the incumbent's labeled errors (`--errors`), or ids. A state goes
> as it was sent when the candidate's builder, version and cap equal the judgment's, else it is rebuilt from the
> record (today `loop.v1`'s, from the turn's `turn.ended` row and its nodes, through the live point's own input
> function), else it is left out with the reason; a thresholds-only candidate makes no call and re-bands the stored
> answers. Its calls are `judge.call` rows scoped `judge.replay:<pack id>`, which the report never reads, and the run a
> `judge.replay` row; both sides are the report's numbers on the same judgments and labels, with each judgment the
> candidate fixed or broke and each class whose precision or recall fell, and a security candidate's planted-injection
> set beside the incumbent's. Its estimate is checked first against `[judge] replay_limit_usd` ($0.50 a run).
> `theseus judge audit <pack> --sample <n> --profile <p>` sends one request per state, outside any session, with Jev's
> instructions and criteria, and writes each answer inside its options as an audit label (weight 0.5, keyed by
> judgment, question and run), stopping before `[judge] audit_limit_usd`. `theseus judge backfill <pack> --since
> <date>` rebuilds the pack's judged points from the record at each event's time, judges each once in shadow (keyed by
> its event) with the live point's context and `event_at_ms`, which the holdout's split reads, and runs only under
> `[judge] backfill_consent = true`, a line of the owner's note, whose digest each run records. Each run is the owner's,
> from a private place, on a thread at nice 19.

## Docs to change (the maintainer's)

- `docs/design/m5-judgment.md` §2.9: the paragraph above; §2.15's config block gains `replay_limit_usd = 0.5` and
  `backfill_consent = false`; §2.6's table: replay and backfill have a per-run cap and do not draw on the shadow day
  budget (as built).
- `crates/theseus-core/AGENTS.md`, "The learning ledger" entry: add 25d's `learning/replay.rs`, `rebuild.rs`,
  `audit.rs`, `backfill.rs`, `rpc/judge_runs.rs`, the `judge.replay:<pack id>` scope, `event_at_ms`, and the tests
  `tests_replay.rs`, `tests_audit.rs`, `tests_backfill.rs`.
- `docs/status.md`: 25d landed.
