# CLOUD_REPORT: l1-self-build (theseus-pw1q.21.1)

Branch `cloud/20261017-l1-self-build`, work commit `376e5cb`. Written by a continuation session after the first hit its
time cap. That first session left no notes. This one re-verified the commit and made no code change.

## What was built (376e5cb)
- `L1.rw: Vec<PathBuf>` (serde default, skipped when empty), bound read-write at the same path by theseus-sandbox
  (`view.rs`, `spec.rs`, `job_l1.rs`, `job.rs`). Only `Sandbox::self_build_view` sets it (`sandbox/self_build.rs`).
- Self-build view: `rw` is the tree and its target only. `~/.cargo`, `~/.npm`, `~/.cache/theseus` and the repo are
  overlays. `~/.rustup` and main's gate copy are read-only. No egress, `CARGO_NET_OFFLINE`, 8192 pids (`BUILD_PIDS`
  const, no config key), floor and sealed dir hidden. A red log that needs the network gets the supply-chain reason
  (`sandbox::supply_chain`).
- `rsi/jail.rs`: the gate, planted-revert runs, merge gate with benches, auto-revert's gate and the install's build
  go through `Jail`. The install's L0 half only copies and restarts (`install::script`).
- `pr.rs`: refuses only when L1 can't run (`Board::l1_unavailable`: health's refusal, else `Sandbox::self_test`).
  `sealed_dir` is still read by its current name. R3's joiner renames that one call site.
- Docs in code: the AGENTS.md of core, kernel and sandbox.

## Proof run in this session (Linux VM, root, no job cgroup)
- `cargo test -p theseus-core --lib -- rsi::tests_jail rsi::tests_pr tests_sandbox rsi::tests_tree rsi::join`:
  60 passed, 0 failed. This covers `no_model_input_reaches_rw`, the `tests_pr` cases (a sealed set doesn't stop a
  build that runs in L1, a host without L1 builds nothing, a halted switch doesn't probe L1), the jail's view and
  environment tests, the supply-chain test and `the_l0_half_only_copies_and_restarts`.
- `cargo fmt --check`: clean. `cargo clippy -p theseus-core -p theseus-kernel -p theseus-sandbox -p theseusd
  --all-targets -D warnings`: clean.
- `cargo test -p theseusd --test self_build`: 4 passed, 1 ignored, **but the four skip as root** ("L1 refuses a root
  daemon's jobs (theseus-pv6i)"), so they ran no job. **The spike's part (a), a planted `build.rs` that can't see the
  sealed dir or store, is NOT proven here.** Part (b), the real gate in the view, was not run either.
- **Not done this session:** planted reverts (none re-run, so none to list); the full `scripts/gate.sh` (neither
  nextest nor cargo-deny was installed, and the budget was spent on builds and tests); runs under load.
  Treat the first four tests below as unproven for the revert half.

## Maintainer's live checks (ordinary user, with a job cgroup)
1. `TZ=America/Phoenix cargo test -p theseusd --test self_build -- --nocapture` should show 4 passed, none skipped:
   rw persists and nothing else does, the planted `build.rs` can't see the sealed dir or store, a new crate fails
   offline, and a tiny crate's gate passes.
2. `cargo test -p theseusd --test self_build -- --ignored a_real_checkouts_command_runs_in_the_view`: the real tree's
   command in the view. Time a cold `scripts/gate.sh` in the view against the L0 gate.
3. Planted reverts to run: bind the tree read-only (`an_rw_bind_persists...`); copy a `proc.run` field into `rw`
   (`no_model_input_reaches_rw`); drop the sealed dir from `hidden` (`a_planted_build_rs...`); drop `--offline`
   (`a_new_crate_fails_offline`); restore the old `sealed_dir.is_some()` refusal (`tests_pr`).
4. One real self branch gate with a scratch sealed dir set.

## Keel
`python3 scripts/keel-guard.py --base 40aa83cd --head HEAD --quiet` (the merge-base; `origin/main` has moved on and
shows unrelated findings from other work):
```
crates/theseus-core/src/rsi/join/install.rs:264: assert-removed: the_install_is_a_transient_user_unit_that_restarts_last (7 -> 5 assertions)
crates/theseus-core/src/rsi/pr.rs:104: allow-added: no clippy::unused_async on fn l1_unavailable
```
The install test lost 3 assertions that named the old L0 `cd` and `scripts/build.sh` lines (the build moved into L1) and
gained 1. The new `the_l0_half_only_copies_and_restarts` asserts the replacement. Trailers for the joiner:

`Keel: assert-removed crates/theseus-core/src/rsi/join/install.rs — the owner, Discord DM 2026-10-17 23:34:35 MST: "(2) yes" to "one Keel yes covering R1, R2, R3 and R4's keel changes: the keel guard watching the sealed and exam paths, self builds inside L1, the scratch-daemon Runner and model relay, the sealed check in self-join" (the install's build moved into L1; its L0 half is checked by a new test)`

`Keel: allow-added crates/theseus-core/src/rsi/pr.rs — the owner, Discord DM 2026-10-17 23:34:35 MST: "(2) yes" to "one Keel yes covering R1, R2, R3 and R4's keel changes: the keel guard watching the sealed and exam paths, self builds inside L1, the scratch-daemon Runner and model relay, the sealed check in self-join" (L1 probe helper kept async to match the daemon's version)`

The owner may prefer to drop the `allow` by making `l1_unavailable` non-async instead.

## Uncertain / for the owner
- Whether the commit body of 376e5cb carries the keel quote: it doesn't. The joiner should rely on the trailers above.
- Docs to change at review: spec Part III item, `docs/status.md` (self builds now run in L1; the sealed-dir refusal is
  gone), `docs/design` RSI loop 1 §4.2.
