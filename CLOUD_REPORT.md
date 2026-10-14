# CLOUD_REPORT: px-exam, the personality exam (theseus-slvl.1)

Branch `cloud/20261013-px-exam`. Code commit `ea1f29a` (one commit: `bench/personality/`, one line in `bench/README.md`). No Rust, no store change (MANIFEST_FORMAT untouched), no config key, nothing added to a prompt header (0 tokens). Standard library only; no package added.

## What I built
`bench/personality/` (README.md there is the guide): probes in TOML (identity, owner, correction, candour, register, initiative; preferences generated: 5 types x 6 contexts; surfaces and routed profiles as repeats), parametrized by a persona TOML (name, owner's name, time zone and its names, card facts, preference words; optional `persona_file`/`card_file` for the owner's own text; a neutral built-in default "Quill"/"Rowan" for none). `xchecks.py`: the recall check language plus prefix, last_line, word/sentence/token bounds, no_table, no_bullets, tz_used, and `ledger persona_digest | wake_created | no_wake` (read from the daemon's `context.compiled` and `wake.set` rows). Arms: theseus, theseus-persona, claude-code (bare), claude-code-fair (persona+card in `$CLAUDE_CONFIG_DIR/CLAUDE.md`). Reuses `bench/recall/drive.py`'s Theseus and ClaudeCode classes. Distractor sessions, filler sized to cross a scratch `context_window` (Claude Code: `/compact` at marks), daemon restart before the "later" context. Surfaces driven: CLI, cockpit JSON-RPC, a DM in `theseus-sim fake-discord` (footer split from the reply; its dollars counted). MCP not driven: its server serves memory reads, not turns. `judge.py` + `rubric.md` (direct API call, default Sonnet, blind to arm); `blind.py` (20 pairs, key apart, tally with Wilson and sign test); `report.py` (md, JSON, CSV, light/dark SVGs via `bench/report`, `--force` guard); `exam.py plan --estimate`.

## Found along the way (fixed, with tests)
- My stand-in lost the question behind a tool call: after a tool result the harness's pending-wakes block is the newest user text. Caught by `test_e2e` (initiative.promise failed); fixed in `exam_standin.py`.
- A regex `\b` after a stem (`congrat\b`) rejected "Congratulations": fixed in `register.toml`.
- Window 30k is too small: Theseus's first request is ~16k tokens (22.7k upper bound) and the output cap takes a quarter; default window is now 60k.
- Planted-revert gap: the mid-plan test for "probed before stated" did not catch its revert (a different error fired first); rewrote it, then it did.

## Proof
- `python3 -m unittest discover -s bench/personality`: 75 tests OK (xchecks 14, plan 24, judge/blind 12, score/report 22, e2e 3 on real daemons + fake Discord).
- Stand-in (`exam.py standin`, smoke size, plan bbc06efc, all four arms, $0 on the stand-in): oracle: 21/21 reply-checked probes pass on every arm; wrong: 0/21 on every arm. `identity.manifest` (judges the daemon, not the reply): fails on `theseus` (no persona), passes on `theseus-persona` in both modes, as designed. Compactions forced at steps 40/53 (Theseus) and by `/compact` (Claude Code, which runs on the stand-in too). Judge stand-in: 5 replies/arm scored 5 vs 1. A full-size Theseus-arm stand-in run (146 steps, 3 compactions) completed; I deleted its output to free disk before scoring it, and the full-size Claude Code arms did not finish: not claimed.
- `test_every_kind_of_check...`: each check kind used in the plan passes on an oracle and fails on a wrong answer.
- Planted reverts (restored, `touch`ed, `git status` clean): prefix startswith->endswith (test_prefix FAIL); unreached scored as miss (test FAIL); blind tally orientation flipped (FAIL); tz_used with no names returning False (FAIL); validator's stated-before-asked check off (FAIL, after the rewrite); stand-in tool-result lookup off (e2e FAIL); persona digest always True (e2e FAIL). **Not caught:** counting `wake.set` rows of every session instead of the step's session (`TheseusArm.wakes`): the per-step delta hides it; it is defence in depth.
- Keel: `python3 scripts/keel-guard.py` ok, 0 findings acked, nothing expected.
- Speed: no Rust changed; `bench lifecycle --check` not run (gate run with THESEUS_GATE_NO_BENCH=1; unaffected).

## The gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`)
door, keel, fmt, shape, features, clippy, cockpit, test build, reader rule: pass. Suite: 34 failures: 33 known L1 (theseus-sandbox contract x19, bench x1, theseusd sandbox x13: root VM, theseus-pv6i) and **1 not on the known list**: `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer` (panics at `tests_gateway.rs:161`), fails alone on rerun (21 s). This commit touches no Rust, so it fails on the base; the thinking-answer tests were just changed on main by theseus-wpv2g. I did not run the later machine_checks phases (protocol types, lifecycle, jobs, turn): nothing they read changed. A first gate run died at clippy on "No space left on device" (disk allowance); I freed `target/debug/incremental` and reran.
Also: `discover -s bench/recall` has 1 error, `test_drive.TheseusDriver.test_mains_sizing_fails_the_mark_on_the_counting_stand_in_as_it_did_live` ("no context.compiled row after the first turn"), failing alone too and on the untouched base (its 35k window vs today's ~16k-token first request). `discover -s bench/report`: 60 OK.

## Live check for the maintainer
```
cargo build --release -p theseusd -p theseus -p theseus-index -p theseus-sim   # or scripts/build.sh --profile release-thin
python3 bench/personality/exam.py plan --size full --profiles <name>=<model> --estimate 4     # ~$1.9 an arm, ~$7.5 for four, +~$0.4 judge
python3 bench/personality/exam.py smoke --arms theseus,theseus-persona,claude-code,claude-code-fair --out /tmp/px-smoke --bin-dir target/release --claude-version <pinned>   # one probe per arm, cents; expect each reply names the persona on the persona arms
python3 bench/personality/exam.py run --arms theseus,theseus-persona,claude-code,claude-code-fair --size full --persona <the owner's persona.toml> --bin-dir target/release --budget 10 --out /tmp/px
python3 bench/personality/exam.py judge --run /tmp/px && python3 bench/personality/exam.py blind --run /tmp/px   # answer /tmp/px/blind/answers.txt, then --answers
python3 bench/personality/exam.py score --run /tmp/px && python3 bench/personality/exam.py report --run /tmp/px --date <day> --slug first-run
```
I ran no real-model smoke probe (no key here). Run Claude Code live only in a throwaway container (it has Bash/Write tools).

## Uncertain / for the owner
- Cost is a model (`plan.estimate_cost`), not a measure; a cold cache or tool calls raise it. Budget is split evenly over the remaining arms.
- "A week later" = restart + later sessions. Persona identity per persona = one run per persona file. Profiles for `--profiles` must be on the configured provider.
- Deterministic candour/register checks are blunt regexes; the judge is uncalibrated; 20 blind pairs is a signal, not proof.
- Chart colours: the house palette has no slots for the four exam arms, so `report.py` borrows `theseus-batching`/`openclaw` slots and names them in the legend; maintainers may want real arm keys in `charts.ARMS`.
- Docs to update at review: `docs/benchmarks/` report on first run; spec Part III item and `docs/status.md` for PX.
