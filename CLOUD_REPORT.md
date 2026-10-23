# CLOUD_REPORT: tail7b-hold (theseus-nsqqo, theseus-a9zg9)

Branch `cloud/20261021-tail7b-hold`, based on `ae420a49`. The two code commits are unsigned (the VM has no key).

## Step 1+2: a notify hold tells once; unset `external_text` follows `enforcement` (theseus-nsqqo), commit 01ecdb26

**Found.** `gate()` raised every acting call under a hold to the mode's posture, so notify noticed on every call. `external_text` defaulted to `Ask`.

**Changed.**
- **Where the once-mark lives.** The META key `external.told.<session>` holds the hold's `since_ms`. It is written in the frame that plans the notified call (`told_row`, pushed beside the `ToolNotified` row in `toolrun.rs`), so it is atomic with the notice. I chose META because it needs no new stored field and no format bump.
- **How the gate reads it.** `external::held_untold` reads the hold and, under notify, returns none when the mark equals the hold's `since_ms`. The later calls then pass the gate unraised, with no notice and no `ToolNotified` row. I first added a field to `order::At`, but that forced an edit to `rsi/sealed/tests_reach.rs`, a keel file, so I dropped it. Reading through the existing `held` closure leaves `At` alone. The mode is fixed for a daemon's life, because a config change restarts the daemon, so ask is never filtered.
- **Across a restart, a task, a report and a job.**
  - A restart keeps the mark, since it is in the store. A new hold has a new `since_ms`, so after a trust and another read it tells once more.
  - A hold carried into a task, report or job is a new hold for the child session (`taken` sets `since_ms` to now, and the key is per session). It tells once there. The code treats it this way already, and I found nothing to change.
  - Calls gated before the first one's frame lands, such as parallel calls in one response, may each notify once.
- **The mode in force.** `PolicyConfig.external_text` is now `Option<Mode>`. `PolicyConfig::external_mode()` gives approve → ask and notify or open → notify, and `external_key()` names the key that decided. Open notifies because a hold never goes silent. The readers moved to it are `build_runtime` (the one place the config is read; every other reader goes through `ToolRuntime.external_text`/`external_key`) and `rpc/explain.rs`. `judge/gate.rs` reads only the hold, not the mode.
- **Setting text.** A notice's setting was `[policy] external_text = notify` and is now `[policy] <key> = notify`, with the key `external_text` or `enforcement`. The `why` wording says "notified once after that (notify)" instead of naming the key, since it cannot know which key decided.
- **Template.** The `[policy]` comment now explains the optional key and that notify tells once. I kept `external_text = "ask"` uncommented in the template. Every test rig built from `Config::example()` relies on a template that shows ask, and ten-odd rigs would otherwise need editing. Say if you want the template to show notify, or the key commented.
- **Rig edits to existing tests.** Rigs that built a config with no `external_text` got the old default of ask:
  - `tests_external.rs`: `core_in` now sets `external_text = Some(Ask)`, with a new `core_mode` variant.
  - `tests_wakes.rs`: its `config()` does the same.
  - Their assertions are unchanged.
- **Test renamed.** `config/tests.rs::external_text_is_ask_by_default_and_names_its_two_values` became `an_unset_external_text_follows_enforcement`, because its premise (default ask) is the owner's changed rule. It still checks that bad values are refused.
- **Tests whose expected notices drop.** None: no existing test asserted a notice on a second call.

**Proved.**
- New tests are in `tests_external/once.rs`: `a_notify_hold_tells_the_owner_once` (key), `a_notify_hold_tells_once_across_a_restart` (core rebuilt over the same store), `a_trusted_session_that_reads_again_is_told_again`, `an_ask_hold_still_waits_on_every_call`. The config test above covers the table.
- Planted reverts, each file restored and touched:
  - Notify on every call (the once check forced false): the three notify tests failed (`once`, `across_a_restart`, `told_again`).
  - Mark never persisted (the record written under another key): the same three tests failed. A pure in-memory plant cannot be expressed with a static, because a rebuilt core in one process would share it. The restart test reopens the store, so it fails when the mark is not durable.
  - `unwrap_or(Ask)` in place of following enforcement: `config::tests::an_unset_external_text_follows_enforcement` failed.
- Runs after the final code:
  - Gate suite: 7963 tests, 7929 passed, 34 failed. All 34 are the known L1 failures (`theseus-sandbox::contract`, `bench spawn_100`, `theseusd::sandbox`). Nothing else failed, and `theseus-judge` and `theseusd default_config` passed.
  - Earlier `cargo nextest run -p theseus-core -p theseus-judge`: 4337 passed.

## Step 3: template placeholders (theseus-a9zg9), commit 4ca46132

**Changed.**
- **Template `[privacy]` block.** Before, the example line listed four real names, and the prose examples used the same names plus `BigCo`. Now the line is `# sensitive_names = ["examplecorp", "examplecorpinc", "example-repo"]`, and the prose examples use those placeholders. Every rule is kept: case-insensitive, whole words, a multi-word name as a run, a hyphenated name as a unit, each spelling listed.
- **`the_pack_text_names_no_organisation`.** It still reads the line from the template and asserts the placeholder list. It also checks that no embedded pack text hits a name and that a planted text does. A new check hashes (SHA-256, using `sha2`, already a dependency of `theseus-judge`) every word of the template, with and without hyphen splitting, and asserts none matches the old line's four digests.

**Proved.** Plant: put an old name into the template's comment prose (scratch only, restored and touched). The test failed at `pack.rs:1784` with "the template names an old name". The test passes at HEAD.

**Other grep hits for the old names** (`file:line` only; none changed, as they are outside this row; the first four files may be unrelated word matches, which I did not inspect):
- `crates/theseus-tui/src/threads.rs:69`
- `crates/theseus-tui/src/tests_threads.rs:38,73,84,101,120,172,176`
- `crates/theseus/src/current.rs:155`
- `crates/theseus/src/interactive/thread.rs:282`
- `crates/theseus/src/render/thread.rs:59,171,184,189,191,192,193,198,208,210`
- `crates/theseus/tests/thread_switch.rs:41,44,85,89,161,175`
- `crates/theseus/tests/thread_interactive.rs:71,75,192,198`
- `crates/theseus-discord/src/render/reply.rs:48`
- `crates/theseus-judge/src/names.rs:80,111-115` (the matcher's own tests)

The maintainer should scrub these.

## Gate and checks
- `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: the keel, fmt, shape, features, clippy (workspace, `-D warnings`), clippy-probe, cockpit, test-build and reader-rule phases passed. The suite phase failed only on the 34 known L1 sandbox tests above. I did not run the later `machine_checks` phases (protocol types, benches) separately. `cargo fmt --check` is clean.
- `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing, exit 0.
- **Line ceilings.** `toolrun.rs` is 2498 lines (the cap for an unlisted file is 2500). I split `order()` so clippy's 100-line limit holds.
- **Disk.** The VM ran out of disk twice. I pruned duplicate artifacts in `target/` and rebuilt it cold once. No source was lost.

## Live check for the maintainer
On a scratch daemon with `[policy] enforcement = "open"` and `external_text = "notify"`, ask a session to fetch a page and then run three `proc.run` calls. Expect exactly one `tool.notified` row for `proc.run`, with setting `[policy] external_text = notify`. Restart the daemon and run another call: still no notice. Then `theseus policy trust <session>`, fetch again and run: one more notice. With `external_text` unset and `enforcement = "approve"`, expect the calls to ask.

## Store format
Unchanged (49). A META key and a serde-default config field need no bump.

## For `docs/status.md`
"A notify hold now tells the owner once per hold (the META mark `external.told.<session>`, theseus-nsqqo); an unset `[policy] external_text` follows `enforcement`; the public template's `sensitive_names` example uses placeholders (theseus-a9zg9)."

## Follow-ups
- Scrub the other hits listed above.
- Consider the parallel-gating window, where several acting calls in one response may each notify once.
- Decide whether the template should show notify rather than ask.
