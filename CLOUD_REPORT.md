# LANDABLE

**Row:** p5-b7-gate-truth, bucket row `theseus-4vsrz` (1.0.5 B7), four checklist items.
**Branch:** `cloud/p5-b7-gate-truth`. **Base:** `4683adb8a24e182c6123f1207f164a9486c1a1fa`. Head: see the last pushed commit.
**Not done:** the full gate was NOT run (no node 24 install, no workspace build). Every change is shell, Markdown, or a
draft file, no Rust. What ran instead: `bash -n scripts/gate.sh scripts/deny-daily.sh`; `python3 -m unittest
scripts/test_gate_tree.py` (12 tests, OK); `python3 scripts/keel-guard.py --base 4683adb8…` → `keel: ok (… 7 files changed,
0 findings acked)`. No request can grow (no Rust), so `dm_flow` was not run. `origin/main` was not merged in.
The first two commits carry the session trailer too; the brief's `Tabitha/Claude` trailer is on the later ones.

## Item 1: cargo deny decoupling: DONE (commit 5525d36)
- **Defect, confirmed:** `scripts/gate.sh:556-569` (`deny_check`) reads `~/.cargo/advisory-dbs`, and `scripts/deny-daily.sh`
  fetched into that same directory, so each daily fetch changed what the next gate judged.
- **Fix:** the daily job writes a generated config (the tree's `deny.toml` plus `db-path`) and fetches and checks in its
  own database (`THESEUS_DENY_DB`, default `~/.cache/theseus/deny-daily-db`). The gate's database is now a pin: only the
  owner's `cargo deny fetch db` moves it. Gate wording and `scripts/AGENTS.md` updated. **No `ignore` added.** Rejected:
  vendoring a database copy in the repo (a stale copy hides real advisories, large, owner's call).
- **Proof:** a live run of `deny-daily.sh` here fetched into a scratch db and reported `advisories FAILED` (exit 1, would file
  an issue), while `~/.cargo/advisory-dbs` still did not exist. Not done: a test for the script's `db-path` insertion, and the
  gate's phase against a pinned db (no gate run).
- **What the other half would need:** nothing blocks merges any more; the owner decides when to move the pin.
- **Mapping confirmed from the fetched database:** `crates/zip/RUSTSEC-2026-0429.md` (`package = "zip"`) and
  `crates/pin-utils/RUSTSEC-2026-0441.md` (`package = "pin-utils"`, `informational = "unmaintained"`).
- **Owner questions** (ignore, upgrade, or vendor around each):
  1. **RUSTSEC-2026-0429, zip 8.6.0**: reached only through `candle-core 0.11.0` → `theseus-index` (candle-core's manifest
     requires `zip = "8.6.0"`, so no `cargo update -p zip` can reach the patched >=9.0.0; theseus-files already uses 9.0.5).
     Options: ignore with reason (the index never calls `ZipArchive::extract`; verify before writing that), wait for a
     candle release on zip 9, or `[patch]` candle-core. I did not try the patch or measure a compile.
  2. **RUSTSEC-2026-0441, pin-utils 0.1.1**: unmaintained, no patch. `cargo tree -i pin-utils` shows it as a normal dependency
     of `aws-smithy-http` → `aws-sigv4` → `theseus-aws` → `theseusd`, so it ships. Options: ignore (as for the other
     unmaintained crates in deny.toml) or wait for aws-smithy to drop it.

## Item 2: stale Known gaps line: DONE (commit 3814653)
Premise verified: `docs/spec/21-part3-item-139.md:285` (Item 141) records "the labels test first (61ded074, theseus-mm4a)".
Only the one sentence at `docs/spec/19-part3-item-118.md:1274` changed (2+/2−). Line 1229 and 1263 of that chapter also name mm4a
as history and were left alone.

## Item 3: batch un-ack: PARTIAL, guarded (commits e16b7ef repro, 07f8178 KEEL-PATCH.md)
- **Mechanism, proved:** `scripts/keel-batch-repro.sh` (throwaway repos). Branch `a` alone: `routing/test_effort.py:1:
  test-removed`. `a` merged with `b` (adds an unrelated test to `voice/test_mute.py`): `voice/test_mute.py:4: assert-removed …
  (3 assertions) -> 1 assertions`. The ack commit stays in range (`acks` reads `base..HEAD`); the finding's rule and path
  change (`rewritten`, `scripts/keel-guard.py:685`, falls through to other files), so the exact-match `acked` stops covering it.
- **Patch** (in `KEEL-PATCH.md`, not applied): pair a rewrite within its own file only. With it the batched run reports
  `test-removed` in the real file again. The guard's suite (185 tests) behaves identically with and without it in a scratch
  copy (2 errors both ways, missing `docs/` files in the copy), so it is not proven against the suite in the tree.
  Trailer: `Keel: keel-file scripts/keel-guard.py — <THE OWNER'S WORDS GO HERE>`.

## Item 4: guard output and docs: PARTIAL (docs DONE in the last fix commit; guard patch drafted)
- `scripts/AGENTS.md` was not in `KEEL_FILES`; the where-versus-what text landed as an ordinary commit, with the
  no-ack-across-a-batch rule and the `--base $(git merge-base …)` check.
- Guard patch (`KEEL-PATCH.md`, second part): each finding is tagged `[where]` or `[what]` with a legend; `BASE DRIFT?` prints when
  the base is not an ancestor of HEAD, naming the cheap check. Shown working on a scratch repo for both outputs.

## Brief corrections
Fact 4 and the checklist were not contradicted. Item 1's brief implied the gate fetched; it already ran `--offline`, and the
real coupling was the shared database directory. The Item 3 description of an ack "leaving the range" is not what happens: the
ack stays, the finding moves.
