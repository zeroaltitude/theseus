# b7-reharvest-cloud-twin-orphan — the hygiene matched the slug, so a re-cut landing orphaned its cloud twin

Board line 35 (B7, P3). Recorded 2026-11-13 by the factory-four worker.

**This note is a record, not a product change**, for the same reason as the line-45 note: the files it
describes live in `~/reports/theseus-105-kickoff/logistics/` and are **not tracked in this repository**, which
has no home for the departure operator scripts.

## What the line offered, and which half was already done

The line named two acceptable fixes:

1. make the hygiene step match the **landed work** rather than the slug;
2. have the detector treat `refs/archive/p5/<slug>` as a twin.

**Fix 2 was already landed, twice.** `logistics/cloud-returns.sh` was taught on 2026-11-12 to suppress a row on
proof that the work is on main, looking for a twin under `<slug>` *and* `<slug>-*` across
`refs/remotes/origin/p5/`, `refs/heads/p5/` and `refs/archive/p5/`; and the p6 namespace's own detector,
`~/reports/theseus-106-kickoff/cloud/p6-returns.sh`, carries the identical block. So the false `harvest` offer
is already suppressed in both namespaces.

**Fix 1 — the one the line calls the real one — was not done, and I implemented it.**

## The measurement, before

Counted 2026-11-13 10:0x MST against `origin/main = 5cc7fbac`:

| namespace | cloud branches on origin | back (carry `CLOUD_REPORT.md`) | landed but still on origin | legitimately pending |
| --- | --- | --- | --- | --- |
| `cloud/p5-*` | **0** | 0 | 0 | 0 |
| `cloud/p6-*` | **31** | 31 | **28** | 3 |

- The **p5 namespace is drained**: there are no `cloud/p5-*` branches left on origin at all, so the p5 detector
  currently reports zero false positives because it has nothing to report. The defect is not observable there.
- The **p6 namespace carries 28 orphans**. The 3 legitimately pending are `w04-specbench`,
  `w07-ledger-durable` and `w38-ledger-durable-r2`.
- **7 of the 28 landed under a variant slug** — `w11-discord-lines`, `w30-memory-pressure-2`, `w31-aws-2`,
  `w33-store-format-2`, `w34-trust-from-chat`, `w35-reversibility-2`, `w36-models-2`, each landed as `…-r2`.
  These are exactly the shape the line describes, and an exact-slug hygiene can never clean them.
- **All 28 twins are `refs/heads/p6/…` — local refs. Not one is an origin head.** That is the fragility the
  line records: the detectors' suppression works, but only on a machine whose local refs happen to carry the
  twin. The branch itself stays on origin for ever, so the row is "not pending" on exactly one machine.

Two causes, both in `depart.sh`'s hygiene step, which computed one name per landed branch —
`cb="cloud/p5-${br#p5/}"`:

1. **matched on the slug**, so a landing under a re-cut slug named a branch that never existed;
2. **hardcoded to `cloud/p5-`**, so once the queue moved to p6 it stopped matching anything at all. This is why
   21 of the 28 are plain exact-slug rows that the old rule would have cleaned had it been looking.

## The change

A new script, `logistics/cloud-twin-hygiene.sh`, sweeps every `origin/cloud/p{5,6}-<slug>` and archives then
deletes those **whose work has landed**, using character-for-character the predicate the two detectors use. That
identity is the point: cleanup and suppression now agree by construction and cannot drift apart again, which is
the class of bug this line is.

- It acts only on proof that the work is an ancestor of main. A twin that exists but has not landed changes
  nothing; a cloud branch with no twin changes nothing.
- It archives to `cloud-reports/<slug>/` **and** to `refs/archive/cloud/<slug>`, and verifies the archive ref
  exists, **before** deleting on origin — the branch is the only copy of its report.
- `--max N` (default 12) bounds how many branches one run may delete.
- `--dry-run` prints a verdict per branch and changes nothing.

`depart.sh` calls it after its existing hygiene step, **dry by default**: live only when
`logistics/cloud-twin-hygiene.armed` exists. The old slug-matched block is left exactly as it was, so arming
the flag is the only change in behaviour; once armed, the old block is redundant (the sweep is a strict
superset) and can be deleted.

## How it was proved, with no compile

`logistics/test-cloud-twin-hygiene.sh` builds a throwaway repository with its own throwaway bare "origin" under
`/tmp`, so nothing it does can reach a real branch. The fixture carries every shape the line names: exact slug
with the twin on origin; exact slug with a local twin; a `-six` variant with a local twin; a `-take` variant
whose only twin is an archive ref; the p6 `-r2` shape; a row that is back with nothing landed; and a row whose
twin exists but is *not* an ancestor of main.

**25 assertions, 25 passed**, covering: the six verdicts in dry run; that the dry run writes no archive; that
the detector and the hygiene agree branch for branch over the same fixture (every landed row suppressed, every
pending row still offered); that a real run deletes exactly the six landed branches and leaves both pending
ones; that each report was archived to a directory and to `refs/archive/cloud/` before deletion; that a second
run is idempotent (`0 landed`); and that `--max 2` deletes two and caps the third.

A dry run of the wired call against the **real** repository at `MAIN=5cc7fbac` then reported
`28 landed, 3 pending, 0 deleted`, with the 7 variant rows each annotated `VARIANT SLUG …-r2` — matching the
independent count above exactly.

## What was deliberately left

- **No branch was deleted.** Not one, in any namespace. The sweep is wired dry and the arming flag does not
  exist; the first live run will want to archive and delete 28 branches (12 at a time under the default cap),
  and arming it is the conductor's call, not mine. A one-off cleanup is not the fix and was not mine to do.
- The detectors were not changed; their half of the line was already done.
- Nothing was landed, queued, ticked, installed or tagged.
