# CLOUD_REPORT: job-wait-status (theseus-5d204)

Branch `cloud/20261021-job-wait-status`. Code commit `9043c50`. Store format unchanged (47). No new dependency
(Cargo.lock untouched). No ceiling raised; no file of this row is in `scripts/long-files.txt`.

## What I found
The beat (`jobs_tools.rs`, `wait`) returns its facts only to the model. `wait_facts::Clock::beat` already holds
everything a status line needs; `waits::LOOK` is 1 s and nothing pinned it.

## What changed (9043c50)
- `toolrun/jobs_tools/wait_status.rs` (new): `post(core, f, clock, silence)` builds one line and posts it with
  `outbox.post_for(.., {"kind":"notice","text":..})`. The pure `line(..)` builder: job (`Found::called()`, cut to 80
  chars), run time and quiet time through `narrative::duration`, `about N.N CPU` only with a cgroup reading and run
  time above 0 s, last line in quotes (cut to 160) from `peek::last_line` or `no output since it started`. It is
  whitespace/control-collapsed to one line, scrubbed through `core.tools.scrubber`, and a post error is a
  `tracing::warn!` and never changes the wait. Off when `[tools] job_wait_status` is false.
- `toolrun/jobs_tools.rs`: `mod wait_status;` and one call in the beat arm's `Some(out)` branch only (held output,
  the still-running result at the cap, the steer return and a job's end post nothing).
- `toolrun/jobs_tools/wait_facts.rs`: `Clock::ran_ms()` accessor (`read` now uses it); `text` and the beat's result
  and meta are untouched.
- `config.rs`, `config/limits.rs`, `config/theseus.example.toml`: `[tools] job_wait_status` (bool, default true,
  `default_job_wait_status`); one `pub use` item. No config golden listed the `[tools]` keys; the template tests
  (`example_template_uncommented_still_parses` among them) pass in the suite.
- `toolrun/waits.rs`: `the_waiters_look_is_a_minute_or_less` (LOOK stays 1 s).
- `AGENTS.md` (core): one sentence after the beat's paragraph.
- `tests_wait_beat.rs`: new tests appended; existing ones untouched.

Status line for the key test's job (as asserted by substrings): `Job j1 (true) is still running: 10 min 0 s so far,
no new output for 10 min 0 s. Last line: "round 2"` (the job's argv in the rig is `true`).

## Tests and planted reverts
Each plant was applied, its test run (fails), the file restored and `touch`ed, `git status` checked clean.
| Test | Plant | Result |
|---|---|---|
| `a_silent_jobs_beat_posts_one_status_line_to_its_place` (key) | remove the call in the beat arm | FAIL |
| `no_status_line_without_a_beat` (prints keeps wait; beat 0; switch off with byte-equal beat text) | ignore the switch | FAIL |
| `each_beat_posts_its_own_line` | post twice per beat | FAIL |
| `the_status_line_is_scrubbed` | skip the scrub | FAIL |
| `the_waiters_look_is_a_minute_or_less` | `LOOK = 61 s` | FAIL |
Also: `a_held_job_posts_nothing` (cheap: `job_output_max_bytes = 1 MiB`, print the head's size), `an_unbound_session_posts_nothing`,
and `wait_status::tests` (CPU only with reading and run time > 0; no output wording; ellipsis cuts; no newline,
`\r` rewrites give the last one). The pure formatter has no plant beyond its asserts. Note: a second beat's "run
time" in the paused-clock rig reads the wall clock for the job's start, so `each_beat_posts_its_own_line` asserts
count and wording, not "20 min".

## Commands and results
- `cargo nextest run -p theseus-core -E 'test(/wait_beat|wait_cost|waits::|wait_status|job_wait|beat::|first_request|example_template/)'`: 51 passed.
- `cargo clippy -p theseus-core --all-targets -- -D warnings`: clean. `cargo fmt`: clean.
- `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 CARGO_INCREMENTAL=0 scripts/gate.sh`: door, keel, fmt, shape, features,
  clippy, clippy-probe, cockpit, test build, reader rule all pass. Suite: 7363 run, 7328 passed, 35 failed:
  34 are the known L1 failures (`theseus-sandbox::contract` x 19, `theseusd::sandbox` x 14, root VM, theseus-pv6i);
  1 is `theseusd::gone_jobs a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`, which passes alone
  on rerun (load flake, not on the flaky list). Phases after the suite: protocol types (no generated change),
  `cargo deny --offline check` ok (advisories, bans, licenses, sources) run by hand; the benches were not run
  (`THESEUS_GATE_NO_BENCH=1`) and the first gate run died on disk space (target/ incremental, 18 GB), cleared
  and rerun.
- Keel guard: `python3 scripts/keel-guard.py --base origin/integ/harness --head HEAD --quiet`: empty output.

## Live check for the maintainer (owner's machine, delegated unit)
`theseus ask -s <session in a bound DM> "run sleep 660 in the background, then job_wait it for 900 s"`. Expected:
the DM gets exactly one notice at about 600 s: `Job j1 (sleep 660) is still running: 10 min 0 s so far, no output
since it started, about 0.0 CPU.` and the model's wait returns the same facts as before. With `[tools]
job_wait_status = false`, no notice. Check the notice renders as one plain line in Discord (the courier's
`notice` arm renders it as is; not checked here).

## Uncertain / follow-ups
- A `sleep` job reads about 0.0 CPU; a busy one reads about its cores.
- The run time uses the wall clock; the quiet time uses tokio's clock (as the beat's own facts do).
- Docs: `docs/status.md` (recently landed; `[tools] job_wait_status`) and the spec's Part III item for
  theseus-53ujg.6's beat should mention the status line and that `waits::LOOK` is pinned at 60 s or less.
