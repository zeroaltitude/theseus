# Cloud report: the user unit's crash loop, and two install gaps

Branch `cloud/20261004-install-smalls`. Commits: f331a39 (0v8s), c2e065c (4xyj), 375f747 (a7gx).

## theseus-0v8s: crash loop
- **Found:** `SERVICE_COMMON` had `RestartSec=5` and no start limit.
- **Changed (f331a39):** `RestartSec=1`; `StartLimitIntervalSec=300` and `StartLimitBurst=10` in `[Unit]`
  (`START_LIMIT` in install/layout.rs). I applied it to **both** the `--user` and `--separate` system units, since
  they share `SERVICE_COMMON` and the same loop; the job host's unit is unchanged (its `RestartSec=5` stays).
  Goldens `user.plan`, `separate.plan`, `separate-migrate.plan` updated; the "line 8 is" check became line 10.
- **Proof:** test `the_daemon_units_bound_a_crash_loop`. `cargo test -p theseusd --bin theseusd install::`: 34 pass.
  Planted revert (`RestartSec=5`, `StartLimitBurst=5`): 4 fail (that test and the three goldens). Restored, touched.
- **Live check:** `theseusd install --user | grep -E 'Restart|StartLimit'` shows `StartLimitIntervalSec=300`,
  `StartLimitBurst=10`, `RestartSec=1`. With a daemon that panics at start, `systemctl --user show theseusd -p
  NRestarts,ActiveState` should stop at 10 starts (`failed`, start-limit-hit) with `crashes/` holding 10 files.

## theseus-4xyj: unnamed token file
- **Found:** a plan noted it; `--apply` still wrote the unit.
- **Changed (c2e065c):** new flag `--token-from-drop-in` (requires `--user`). `--user --apply` with no
  `--op-token-file` / `THESEUS_OP_TOKEN_FILE` bails before writing anything, naming both ways out. Plan, `--check`
  and `--remove` unchanged; the plan's note mentions the refusal. Check is in install/mod.rs (the Mode::Apply
  branch), not modes.rs, since only mod.rs knows the mode. AGENTS.md of theseusd got one sentence.
  The hint test passed `apply` only to fake argv, so it now stays a plan.
- **Proof:** test `a_user_apply_without_a_token_file_refuses_unless_a_drop_in_supplies_it` (tree unchanged on
  refusal, plan and check pass, flag lets apply through, remove needs no token). Planted revert (condition
  inverted): 4 tests fail. Restored, touched.
- **Live check:** `env -u THESEUS_OP_TOKEN_FILE theseusd install --user --apply` exits 1, "nothing was changed";
  adding `--token-from-drop-in` writes the unit.
- **For the owner:** `scripts/user-service.sh install` always passes a token file, so unaffected.
  docs/user-service.md should mention `--token-from-drop-in` (not edited, per the rules).

## theseus-a7gx: check follows the unit's config
- **Changed (375f747):** `resolve_from_unit` now also reads the unit's `--config` into `UNIT_CONFIG`, only for the
  `check` command (`FOLLOW_UNIT_CONFIG=1`) and only when the shell has no `THESEUS_CONFIG`. `install` still checks
  the config the unit would get. The OK line says "THESEUS_CONFIG is not set here, so this is the installed unit's
  config". With no unit installed, the plan-based path is unchanged.
- **Proof:** test `check_follows_the_installed_unit_for_the_config_when_the_shell_has_none` (also: a variable still
  decides). These script tests **skip as root**, so I ran the test binary as `nobody`:
  `su -s /bin/bash nobody -c 'cd /home/user/theseus && CARGO_MANIFEST_DIR=... ./user_service_script'`: 21 pass.
  Planted revert (follow switched off): the test fails with the reported symptom, `FAIL config: .../.theseus/theseus.toml
  is not a readable file`. Restored, touched.
- **Live check:** in a shell with `THESEUS_CONFIG` unset, after an install naming a different config,
  `scripts/user-service.sh check` should show `ok config: <the unit's config> (...installed unit's config)`.

## Docs to change (not edited)
- docs/user-service.md line 14 and 244: `RestartSec=5` is now 1, plus the start limit; add `--token-from-drop-in`.

## Gate
`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: fmt, shape, clippy, builds passed; **suite failed**: 1721 of 1754 pass.
- 32 sandbox tests (`theseus-sandbox::contract`, `::bench spawn_100`, `theseusd::sandbox`): "the daemon runs as root,
  and Linux exempts root from RLIMIT_NPROC" (theseus-pv6i, known).
- `theseus-core tests_output::the_cores_output_matches_its_golden`: a wake's time prints `+00:00` where the golden
  has `-#:#` (the golden was written in a zone west of UTC); the VM is UTC. Nothing in this change touches it; not
  fixed here.
Then the post-suite phases by hand: protocol types clean, `cargo deny` bans/licenses/sources ok (advisories not
run), web and cockpit lint/build/test ok (warnings only), web dist unchanged. Benches skipped (NO_BENCH).
