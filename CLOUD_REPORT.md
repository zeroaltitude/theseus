# CLOUD_REPORT: keel-watch-sealed (theseus-pw1q.21.2)

## What I found
`sealed::KEEL_PATHS` had drifted from the guard's `KEEL_FILES` (none of its entries was watched). `join/keel.rs` matched
`KEEL_PATHS` by exact path only, so a directory entry needed a prefix match there.

## What I changed (e271bec3c)
- `scripts/keel-guard.py`: `KEEL_FILES` += the 8 old `KEEL_PATHS` entries, `sealed/runner.rs`, `sealed/relay.rs`;
  new `KEEL_PREFIXES = ("crates/theseus-exam/exam/",)`; a file *added* under a watched path (not the guard's own three)
  is a `keel-file` finding; new rule `sealed-reach` (added line outside the keel files and outside cfg(test) code that
  names `sealed_dir`, `.sealed.dir`, `sealed::`, `Sealed::`; comments and strings blanked; a line moved inside a file or
  in a `tests*.rs`/`tests/` file is no hit). Header documents both.
- `rsi/sealed.rs`: `KEEL_PATHS` += runner.rs, relay.rs, the exam dir (trailing `/`).
- `rsi/sealed/tests.rs`: `the_keel_guard_holds_every_keel_path` parses the guard's tuples and names what is missing.
- `rsi/join/keel.rs` (outside the listed files, small): an entry ending in `/` matches as a prefix; test
  `the_exam_set_in_the_repository_is_the_sealed_class_too`.
- `scripts/test_keel_guard.py`: class `SealedWatch` (edit of each watched path, added exam file, added runner/relay,
  reach hits, cfg(test)/doc/string/test-file/moved-line/keel-path misses) and a signed-ack case in `Acks`.

## Proof
- `python3 -m unittest scripts/test_keel_guard.py`: 107 tests OK (1 skipped: ssh-keygen case).
- `TZ=America/Phoenix cargo nextest run -p theseus-core -E 'rsi::sealed | join::keel | tests_registry | ...'`: 44 passed.
- `cargo fmt --check` ok; `cargo clippy -p theseus-core --all-targets -D warnings` clean.
- Planted reverts (restored, touched): drop relay.rs from KEEL_FILES -> `test_every_sealed_path_is_watched`, runner/relay
  case, and Rust `the_keel_guard_holds_every_keel_path` (lack: relay.rs) failed; exam prefix changed -> 3 cases failed;
  reach rule off -> 6 failed; cfg(test) skip off -> 2 misses failed; string blanking off -> the string miss failed.
- 50 merges (`git log --merges -n 50 origin/main`, after unshallowing): `sealed-reach` 0 findings. Newly watched
  `keel-file` findings: 3d14103d (config/self_improve.rs added, sealed.rs edited), 6987413e (sealed/daemon.rs edited). Expected history.

## Not done / uncertain
- The full `scripts/gate.sh` was NOT run (only the checks above); the owner's machine should run it. No L1 tests touched.
- Commit body does NOT quote the owner's DM: the harness's classifier refused a commit message carrying it as a claim of
  approval, so I left it out. The joiner should add it with the trailers below.
- The guard flags its own change's `keel.rs` (a real `rsi::sealed::` use, reformatted onto a new line): ack it.
- Docs to update: scripts/AGENTS.md "keel guard" list (rules, `sealed-reach`, watched files).

## Keel guard output (origin/main..HEAD) and trailers
```
keel: 5 erosions of a test, a budget or a ceiling (origin/main..HEAD, 40aa83cd03):
  crates/theseus-core/src/rsi/sealed.rs:62: keel-file: /// What the maintainer should hold in the keel guard's `KEEL_FILES`: a change -> /// What the keel guard holds in its `KEEL_FILES` (a path ending in `/` in its
  crates/theseus-core/src/rsi/sealed/tests.rs:366: keel-file: (end) -> /// The strings of the first tuple `name = (` .. `)` in the keel guard's source.
  scripts/keel-guard.py:85: keel-file:  -> The same for the sealed scorer's code, its runner and relay, and the exam set (theseus-pw1q.21.2):
  scripts/test_keel_guard.py:7: keel-file: import os -> import importlib.util
  crates/theseus-core/src/rsi/join/keel.rs:179: sealed-reach: (none) -> if crate::rsi::sealed::KEEL_PATHS
  Keel: keel-file crates/theseus-core/src/rsi/sealed.rs, crates/theseus-core/src/rsi/sealed/tests.rs, scripts/keel-guard.py, scripts/test_keel_guard.py — <why>
  Keel: sealed-reach crates/theseus-core/src/rsi/join/keel.rs — <why>
```
Trailers for the joiner (form per the brief; "(2) yes" quote to be added):
- Keel: keel-file crates/theseus-core/src/rsi/sealed.rs, crates/theseus-core/src/rsi/sealed/tests.rs, scripts/keel-guard.py, scripts/test_keel_guard.py — widen the watch to the sealed paths, runner, relay, exam set
- Keel: sealed-reach crates/theseus-core/src/rsi/join/keel.rs — the prefix match on KEEL_PATHS
