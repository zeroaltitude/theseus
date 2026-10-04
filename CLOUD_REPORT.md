# Cloud report: classify.v1 and role.v1 at inbound, step 25a (theseus-0j2.6)

Branch `cloud/20261004-classify-role`, from `main` at 3add3f5 (plus the task commit 0b407c0).
Started 08:46 UTC, report written 10:05 UTC, 2026-10-04.

## The step: one commit, ff5479d

### What I found

- Every turn with input reaches the core through `turn.submit` (`rpc/methods.rs`): the CLI, the web UI, and
  Discord's typed and spoken messages (`runtime.rs`, `runtime/voice.rs`). A task's first turn, a wake's turn and a
  report's turn are all continuations (`TurnRequest.input == None`): `task::create` writes the brief as a node in
  `open_task`'s frame, and the driver takes a continuation. So "a turn whose input node is written" already
  excludes all three, and the point never sees them.
- Discord's own controls (`/stop`, `/cancel`, `/new`, `/status`, `/tasks`, `/wakes`, `/trust`) never become turns.
  But an unknown `/word` from Discord, or any `/word` typed at the CLI, does arrive as turn input. So the core
  tells a slash command by its first word: `/` and then a name (letters, digits, `-`, `_`). A path such as
  `/etc/hosts is empty` is not a slash command.
- `TurnRequest` doesn't carry the connection's surface, and adding a field would touch 38 struct literals across
  the tests. So the place kind comes from the session's place (`outbox.try_target`: `discord:dm:…` gives
  `discord_dm`, `discord:channel:…` gives `discord_channel`). With no place, it comes from the client label the
  turn's author falls back to (`web#n` gives `web`, anything else `cli`). The place rule's class is added:
  `cli (private)`, `discord_channel (shared)`.
- Health's `calls_today`, `failed_today` and `skipped_today` count each settle. A batched point settles once, so
  one call counts as one, matching the fake's connection count.

### What I changed

- **theseus-judge**: `Ask` gains `pub id: Option<String>` (`Ask::new` sets `None`), and `Judgment::pending` uses
  it when set. `judge::new_id()` mints `jdg_<uuid v7>`. Test: `a_minted_id_is_the_judgments_and_none_mints_one`.
- **`judge/inbound.rs`** (new): `CLASSIFY_PACK`, `ROLE_PACK`, `PACKS`, `SEED_ROLES`, `Inbound`, `place_kind`,
  `author_of`, `slash_command`, `JudgeService::at_inbound`, and the spawned `judge_inbound`.
  - `at_inbound` filters by mode and sample, mints one id per pack, marks the trace with one `judge` mark per
  judgment (`pack`, `point: inbound`, `mode: shadow`, `judgment`), and spawns. Everything else happens in the
  spawned task: the session's nodes, the live tasks (`Kernel::tasks(execution)`, non-terminal, at most 20,
  with the brief's text after the harness preamble), the state, its one blob, the reservation, one
  `DecisionPoint` with both asks, and the settle.
  - Each judgment's `context` holds session, execution, turn, node, baseline, place_kind, blob, and
  `on_path_ms: 0`.
- **`judge/mod.rs`**: a `mod` line, and `WIRED` gains `classify.v1` and `role.v1`, both in shadow. 23a's loop
  path is unchanged.
- **`turn/inbound_step.rs`** (new): `TurnRunner::inbound_point`. With the judge off it reads nothing. Otherwise it
  reads the place, builds the `Inbound`, and calls `at_inbound`. **`turn.rs`** gains `mod inbound_step;` and one
  call after `node_written` (3,454 lines; its ceiling is 3,500).
- **The roles**: the state carries the spec's twelve seed rows (§3.4) as compiled-in data (`SEED_ROLES`: id and a
  one-sentence stance with its hints), with `current_role` none. I wrote `operator (infra)`, `thought partner` and
  `security analyst` as the ids `operator`, `thought_partner` and `security_analyst`. Step 26c's versioned table
  replaces all of this.
- **The author in the state**: in a private place it is `operator`; in a shared place it is
  `a person in a shared place`, so no Discord display name is sent to Jev.
- **Config template**: the `[judge]` comment names the three packs and what is not judged. No new key.
- **Tests**:
  - New `tests_inbound.rs` (7 tests).
  - `tests_judge.rs`: the loop tests judge loop.v1 alone, with the inbound packs' lines off. Its rig is shared:
  `rig_on`, `judge_config`, `off`, and `pub(crate)` helpers. Two `h.packs` expectations name the three packs.
  - `tests_task_wakes.rs`: `life_with(dir, tweak)` and `pub(crate)` helpers, for the exclusion test.
  - `theseusd/tests/judge.rs`: its rig turns the inbound packs off, and the packs line expects three.
- No new dependency. `Cargo.lock`, the protocol, its TypeScript, and the store format are unchanged: judgments
  are ledger rows.

### How I proved it

- `tests_inbound`:
  - `a_persons_message_is_judged_by_both_packs_in_one_request`: the fake counts 1 connection.
  `classify.v1/kind` and `role.v1/role` are both in that one request, and role's criteria are 13 (12 seeds and
  `other`). With no tasks, `addressed_task` is not asked. There are two rows, `judge:classify` and `judge:role`,
  each keyed by its own id, with the same state sha256 and blob and the same call id, `packs: 2`. Questions are
  5 and 1; the costs satisfy `batch::shares(cc+cr, [5,1]) == [cc, cr]`, with cc > cr > 0. The state says
  message, `operator`, `cli (private)`, `current_role: none`. The two trace marks are zero-length and carry the
  rows' ids. Health shows one call, its spend equal to the two costs, and the packs line
  `loop.v1: off, classify.v1: shadow, role.v1: shadow`.
  - `the_next_messages_state_reads_the_previous_message_and_the_reply`: the previous message, the last reply, and
  minutes 0.
  - `a_slash_command_is_not_judged`: `/status` makes no mark and no call; the next message makes exactly one.
  - `a_tasks_first_turn_a_wakes_turn_and_a_reports_turn_are_not_judged`: a parent's `START CHILD AGAIN 3s`
  runs the task's first turn, its wake's turn, and the parent's report turn. Exactly one classify row and one
  role row, both the parent's, and one connection.
  - `a_judged_turn_keeps_its_frame_budget`: with all three packs on, a second plain turn has 2 marks, its trace's
  own `frames` is exactly 5, and the WAL grew by 5 or fewer during it.
  - `a_failing_jev_is_recorded_by_its_class_and_changes_no_turn`: down, slow (10 s against a 5 s total),
  429 and malformed each leave both rows failed with `network`, `timeout`, `rate_limited` and `malformed`.
  There is one connection, health shows `(1, 1)`, and the turn takes under 3 s. The provider request is
  byte-equal (as JSON) to the judge-off daemon's, and output, stop reason, loops, usage and cost are equal.
  - `the_inbound_packs_off_call_nothing`.
- Unit tests in `judge/inbound.rs`: slash commands, the place kind, and the seed roles.
- The focused run (`package(theseus-core) & (tests_inbound | tests_judge | tests_task_wakes | judge::)` plus
  `package(theseus-judge)`): **146 passed**. `theseusd::judge`: **3 passed**.
- **Planted reverts** (each restored from a copy and `touch`ed, with `git status` clean of it after):
  1. Two decision points (one `judge()` per ask) in `judge_inbound`. **4 failed**, among them
     `a_persons_message_is_judged_by_both_packs_in_one_request` at `jev.connections() == 1`, plus the slash,
     the failing-Jev, and the task tests, all on their one-call counts.
  2. Judge continuations: `turn.rs` calls `inbound_point` on the transcript's last node when `input.is_none()`.
     `a_tasks_first_turn_a_wakes_turn_and_a_reports_turn_are_not_judged` failed with `left: (4, 4), right: (1, 1)`:
     the parent's message, plus the task's first turn, the wake's, and the report's.
- **Under load**: AGENTS.md's recipe, `nice -n 19` with four busy loops at nice 0, over `tests_inbound` and
  `tests_judge` (16 tests), three runs: **16/16 passed each time**, no retries.
  - The first load run caught my exclusion test using a 1 s wake: under load the wake fired before
  `parked()` saw the task parked. It now uses 3 s, as the original test uses 2 s, with a 30 s wait for the end.
  - An earlier attempt that rebuilt the tests under nice 19 starved and hit the 30-minute limit with no output.
  It is not counted.

### Live check (the maintainer's, with the real key)

On a scratch daemon with a fresh state dir. In its config, `[judge] enabled = true` and `[model] live = "glm"`
(or pass `-P glm` to each ask), with `[secrets] jev_api_key` and `zai_api_key` as on the owner's machine:

```sh
S=$(mktemp -d); theseusd example-config > "$S/config.toml"   # then edit: [judge] enabled = true, live = "glm", real op:// refs
theseusd --config "$S/config.toml" --state-dir "$S/state" --socket "$S/sock" &
T="theseus --socket $S/sock"
SID=$($T ask --json -P glm "Write a shell one-liner that counts the lines in every .rs file under crates/" | jq -r .session_id)
$T ask -s "$SID" -P glm "and the tests too"
$T ask -s "$SID" -P glm "stop"
$T ask -s "$SID" -P glm "/status"          # a slash command: no judgment
sleep 5; $T judge log -n 20
$T --json judge log -n 20 | jq -r '.rows[] | [.data.pack, .data.call.id, .data.cost_micros, ((.data.answers // [])[] | select(.question=="kind" or .question=="role") | .answer.choice)] | @tsv'
$T health | grep judge
$T shutdown
```

What each should show:

- `judge log` lists **three `classify.v1`** judgments with their `kind`. I expect `new_ask`, then `follow_up`
  with `fragment` likely true, then `control`. It also lists **three `role.v1`** judgments with the role guessed
  (probably `coder`), and three `loop.v1` judgments, one at each turn's end.
- The jq line shows that each classify/role pair shares one `call.id`, with classify's cost about 5/6 of the
  pair's.
- Health's judge line shows `classify.v1: shadow` and `role.v1: shadow`, and its calls count one call per message
  plus one per turn end.
- The `/status` turn has no inbound judgment (its turn may still get a `loop.v1` one).

### What is left, or uncertain

- **The live check is not run here** (no key).
- **The place kind's surface is inferred from the place, or the client label**, not from the connection's
  `Surface`. A protocol client that sends its own `author` with no bound place (neither the CLI nor the web UI does) reads as `cli`. Discord voice
  reads as `discord_dm` or `discord_channel`, like typed text. If the owner wants `voice` named, `TurnRequest`
  needs a field (38 literals) or the surface needs another route.
- **A message from inside a job** (a `theseus ask` from a job's shell, with `opened_from`) is judged as a person's
  message: the turn can't tell it apart. Say if it should be excluded; the TurnRequest doesn't carry
  `opened_from` today.
- **Frames.** The turn's own frames never change: it writes no judge frame, and its trace counts 5. But the
  point's reservation can write the shadow budget's frame (the day's first block) and the sink's frame (2 s
  after the judgment), and either can land inside a long turn's window in a WAL-wide count. That is the
  theseus-0j2.3 shape. The turn bench runs with the judge off, so it doesn't see it. theseus-0j2.8's judge-on
  bench would measure it, and now covers an inbound judgment early in each turn as well as loop.v1's late one.
- **Budget accounting**: a batched point settles once, so one call counts as one call in health. Per-pack counts
  live in the rows.
- **Docs** for the maintainer to write: Part III's 25a item; `docs/status.md`; and theseus-core's AGENTS.md
  ("What's here" could gain a Judge bullet naming `judge/inbound.rs`, `turn/inbound_step.rs` and
  `tests_inbound.rs`; there is none for 23a yet either). Design §3's 25a entry could note that the seed roles
  are compiled in until 26c.
- **Merging with the other judge sessions**: `WIRED` gains two lines; `judge/mod.rs` gains `pub mod inbound;` and
  two doc lines; `Ask.id` follows the shared convention. Others adding `Ask { … }` literals need `id`.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: fmt, shape, features, clippy, cockpit, test build and reader rule
passed. **The suite failed only on cases not this step's:**

- 21 `theseus-sandbox::contract`/`bench` tests and 8 `theseusd::sandbox` tests: the VM runs as root, and L1
  refuses a root daemon (theseus-pv6i, known).
- `theseus-core tests_output::the_cores_output_matches_its_golden`: a time-zone sign in a `wake.at` preview
  (`+#:#` here, `-#:#` in the golden). This VM runs in UTC. It **passes with `TZ=America/Los_Angeles`** and fails
  without it. This is environmental and predates this step (a golden whose shape depends on the machine's zone
  is worth an issue).
- `theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults`: failed once, passed on its retry (it is on
  the flaky list).
- `theseusd::judge` (3 tests) failed in that run because they expected loop.v1 alone. **Fixed in this step**
  (their rig turns the inbound packs off) and rerun: 3/3 passed.

After the fix I ran the later phases myself:

- fmt `--check`: ok.
- `scripts/shape.sh`: ok.
- `cargo clippy --workspace --all-targets -D warnings`: clean.
- The protocol-types check: `cockpit/src/protocol.gen` is untouched.
- `cargo deny --offline check`: advisories, bans, licences and sources all ok. `cargo deny fetch` ran during
  setup.

The benches were off (`NO_BENCH`), as instructed.
