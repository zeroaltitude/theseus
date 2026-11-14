# b7-forty-nine-local-lanes — the adjudication, re-derived

Board line 41 (B7, P3). Recorded 2026-11-13 by the factory-four worker.

**Nothing was deleted.** No worktree, no lane, no branch, no ref. This note is an adjudication and a record; it
is not a cleanup and it authorises none.

The line's own terms were honoured: the commits are **HELD, not lost**, and the count **was re-derived before
anything was said about it**. The old figure of forty-nine is stale by construction, and the re-derived figure
is **larger**, not smaller.

## Method

Every worktree of the Theseus repository — `git worktree list` plus everything under `~/lanes/` and
`~/lanes-local/` that it names — was examined read-only, with `GIT_OPTIONAL_LOCKS=0` on every call so no lane's
index was refreshed or locked and nothing was pruned.

For each lane: its branch and HEAD; whether the tree has **uncommitted tracked** edits (an untracked build
artefact is not held work); how many commits in its history **no `origin/*` ref reaches at all**; and how many
**unique non-merge** commits it carries against `origin/main`.

Reachability is `git rev-list --count <head> --not --remotes=origin` — one history walk per lane rather than one
`merge-base --is-ancestor` per origin ref. The first version of the script did the latter and was still running
after ten minutes; this one finishes in **13.6 seconds**. It is also the stricter and more honest test: it asks
whether origin reaches the head's *whole history*, not merely whether `origin/main` is an ancestor.

## The re-derived count

| quantity | value |
| --- | --- |
| worktrees enumerated | **112** |
| lanes holding ≥1 commit **no origin ref reaches** | **57** (old count: 49) |
| those unreached commits, summed | **347** |
| of which **unique non-merge** | **272** |
| lanes with uncommitted tracked edits | **7** |

Verdicts: **40 hold real work** · **15 hold nothing** (unreached, but every unreached commit is a merge) ·
**50 hold nothing** (origin reaches their whole history) · **7 need a human**.

Cross-tabulated:

| | holds real work | holds nothing | needs a human |
| --- | --- | --- | --- |
| origin reaches the whole history | 0 | 50 | 5 |
| ≥1 commit unreached by origin | 40 | 15 | 2 |

### What "holds real work" does and does not mean

It is a literal statement: these lanes carry commits that exist **nowhere on origin**. It is **not** a claim
that the work is wanted. Dating the newest commit in each of the 40:

| newest commit | lanes |
| --- | --- |
| October 2026 | **28** |
| November 2026 | 12 (one of which is this worker's own `factory-four` lane, dated today) |

So the bulk of the 40 is 1.0.3/1.0.4-era material from 24–29 October, predating the 1.0.5 cut. Whether any of
it should be recovered is a judgement for a human with the release history in hand, which is why the line says
adjudicate rather than act.

### The fifteen that hold nothing but look like they hold something

Fifteen lanes have unreached commits of which **zero** are unique non-merge commits — their only unique content
is superseded integration merges. This is exactly the distinction the 08:47 spare drew for `integ/p6-1` and
`integ/p6-2`, now drawn for every lane:

`b5-courier-finish-cRTAoA`, `b5recut-merge`, `b6lingering-finish`, `fix-b4cell`, `rv-b5outbox`,
`rv2-b5outbox`, `rv2census`, `rv2prev`, `rv2shapes`, `rvwc-merge`, `wadusixreds`, `waduverify`,
`/tmp/b5c1c2-merge`, `/tmp/b5secmerge2`, and `~/lanes/p4-w4a` (`integ/p4-w4a`, 4 unreached commits, all merges).

The ten `integ/p6-*` lanes went further still: origin reaches their histories **entirely**, so they hold
nothing at all.

### The seven that need a human

| lane | branch | HEAD | why |
| --- | --- | --- | --- |
| `~/lanes-local/p0b-held-loop` | `p6/p0b-held-loop-repeat-cap` | `5cc7fbac` | 4 tracked edits, commits all on origin — **the conductor's live critical-path builder; in flight, not stale** |
| `~/lanes-local/rv2newmain` | (detached) | `13b7776e` | 9 tracked edits, commits all on origin |
| `~/lanes/gauntlet` | `bench/gauntlet-1025` | `68d07a62` | 6 tracked edits **and** 9 commits no origin ref reaches |
| `~/lanes/p3t-cn-provenance` | `p3t/cn-provenance` | `7f297c98` | 7 tracked edits **and** 1 unreached commit |
| `~/lanes/p4-w3` | `p5/live-learning-self-fact` | `50de5054` | 16 tracked edits, commits all on origin |
| `~/projects/theseus-wt/b7-route-cliff-measure-3239541` | (detached) | `3a2e1b57` | 1 tracked edit |
| `/tmp/b6cpolicy-merge` | (detached) | `3a2e1b57` | 2 tracked edits |

Any lane with uncommitted work belongs to someone until proven otherwise. None was touched.

## The two named never-touch entries, checked

**`p5/b3-voice-leadin-trace`** (`~/lanes-local/b3voicetrace`, HEAD `3f278b74`) — kept as the record of an
adjudication, neither queued nor deleted. Measured: clean, 2 unique non-merge commits against `origin/main`,
and **`origin` reaches its whole history** — `git branch -r --contains 3f278b74` returns
`origin/p5/b3-voice-leadin-trace`. The record is preserved on origin, so keeping the local lane costs nothing
and losing it would lose nothing. Untouched, and the board's disposition is confirmed safe.

**`p4w1` is DIRTY — and it is not `~/lanes/p4-w1`.** This matters and a prune driven off the local table alone
would get it wrong. The ARM `p4w1` is a lane **on the M1 Max (arm-host.example)**, not a worktree on this machine.
Read-only from `arm.sh status`:

```
lane p4w1 9d0d7473 target 58G  idle DIRTY:  M cockpit/src/protocol.gen/index.ts
     last send: refused 2026-11-12 18:58:36 lane tree not clean
```

That is the 18:58 send the board names, still refused, still dirty, still belonging to nobody. **Held from any
prune, as every shift has held it.** Its commits are not at risk — `9d0d7473` is also the head of
`~/lanes-local/integ-p6-2` (`integ/p6-2`), whose whole history origin reaches — so the only thing the lane
uniquely holds is that one uncommitted generated file.

Meanwhile the *local* worktree `~/lanes/p4-w1` is a different object entirely: clean, HEAD `1e58f318`,
contained in `origin/main`. Anyone reconciling "p4w1 is dirty" against a local table will find it clean and may
conclude the fact is stale. It is not; they are looking at the wrong lane.

(`arm.sh status` also showed `p4w2`, `p4w3` and `p4w4` each running a gate at the time of reading, consistent
with the shift's note that all three ARM cargo units were busy. **No `cargo`, `rustc` or gate was run by this
worker on any host.**)

## The full table

112 rows, sorted by verdict then lane. `dirty` counts uncommitted **tracked** edits; `unreached` counts commits
in the lane's history that no `origin/*` ref reaches; `unique non-merge` counts non-merge commits in
`origin/main..HEAD`.

| lane | branch | HEAD | dirty | unreached | unique non-merge | verdict |
| --- | --- | --- | --- | --- | --- | --- |
| `~/lanes-local/b4live` | `p5/b4-standing-ack-live` | `b4905b5a` | 0 | 1 | 1 | HOLDS REAL WORK (1 unique non-merge commits, 1 unreached by origin) |
| `~/lanes-local/importfix2` | `p5/import-prefix-diverged-quiet-take` | `bb671d0e` | 0 | 1 | 1 | HOLDS REAL WORK (1 unique non-merge commits, 1 unreached by origin) |
| `~/lanes/h66v48-local` | `p4/h66v48-flash` | `4128bb3a` | 0 | 1 | 1 | HOLDS REAL WORK (1 unique non-merge commits, 1 unreached by origin) |
| `~/lanes/ouqru-v2-live` | `p3t-cn-prov-fix` | `7f297c98` | 0 | 1 | 1 | HOLDS REAL WORK (1 unique non-merge commits, 1 unreached by origin) |
| `~/lanes/p3s-cn-screen` | `local/20261026-p3s-cn-screen` | `5337ed02` | 0 | 1 | 1 | HOLDS REAL WORK (1 unique non-merge commits, 1 unreached by origin) |
| `~/lanes/p3s-owner-oks` | `local/20261026-p3s-owner-oks` | `56b5f9f8` | 0 | 1 | 1 | HOLDS REAL WORK (1 unique non-merge commits, 1 unreached by origin) |
| `~/projects/worktrees/theseus/omnibus-1025` | `docs/omnibus-1025` | `a34f53cb` | 0 | 1 | 1 | HOLDS REAL WORK (1 unique non-merge commits, 1 unreached by origin) |
| `~/projects/worktrees/theseus/p3-m4` | `lane/p3-m4` | `64651b4b` | 0 | 1 | 1 | HOLDS REAL WORK (1 unique non-merge commits, 1 unreached by origin) |
| `~/lanes/p5kln` | `p5/p5kln-undo-classifier` | `bb3e3242` | 0 | 2 | 1 | HOLDS REAL WORK (1 unique non-merge commits, 2 unreached by origin) |
| `/tmp/b7merge` | `(detached)` | `a87223b3` | 0 | 2 | 1 | HOLDS REAL WORK (1 unique non-merge commits, 2 unreached by origin) |
| `~/lanes/cn-bench` | `bench/cn-benchbase-8qgwo` | `0f0d5a6f` | 0 | 11 | 11 | HOLDS REAL WORK (11 unique non-merge commits, 11 unreached by origin) |
| `~/lanes/p4tq` | `p4/tqomi-route-v9` | `2ce08918` | 0 | 13 | 11 | HOLDS REAL WORK (11 unique non-merge commits, 13 unreached by origin) |
| `~/lanes/p4-turn` | `integ/p4-turnwork` | `406d47e1` | 0 | 16 | 14 | HOLDS REAL WORK (14 unique non-merge commits, 16 unreached by origin) |
| `~/lanes/p4tw2` | `integ/p4-turnwork2` | `9e10b2c9` | 0 | 16 | 14 | HOLDS REAL WORK (14 unique non-merge commits, 16 unreached by origin) |
| `~/lanes/p4integ` | `integ/p4-dry` | `fea3b940` | 0 | 11 | 2 | HOLDS REAL WORK (2 unique non-merge commits, 11 unreached by origin) |
| `~/lanes/8xzcu` | `p4/8xzcu-benchslices-label` | `7d97c2e0` | 0 | 2 | 2 | HOLDS REAL WORK (2 unique non-merge commits, 2 unreached by origin) |
| `~/lanes/p3a-threadbench` | `local/20261026-p3a-threadbench` | `4336dd92` | 0 | 2 | 2 | HOLDS REAL WORK (2 unique non-merge commits, 2 unreached by origin) |
| `~/lanes/p3t-jrffh` | `local/jrffh-namescrub` | `c0a074d7` | 0 | 2 | 2 | HOLDS REAL WORK (2 unique non-merge commits, 2 unreached by origin) |
| `~/lanes/p4twf` | `p4/turnwork-f1f2` | `63197cf3` | 0 | 22 | 20 | HOLDS REAL WORK (20 unique non-merge commits, 22 unreached by origin) |
| `~/lanes/p4twf-regate` | `integ/p4-turnwork-f1f2-regate` | `1c2a73e5` | 0 | 23 | 20 | HOLDS REAL WORK (20 unique non-merge commits, 23 unreached by origin) |
| `~/lanes/p4-routing2` | `p4/routing-last-two` | `2ffdf884` | 0 | 28 | 26 | HOLDS REAL WORK (26 unique non-merge commits, 28 unreached by origin) |
| `~/lanes/p4-join` | `integ/p4-join` | `61d8e7e6` | 0 | 42 | 26 | HOLDS REAL WORK (26 unique non-merge commits, 42 unreached by origin) |
| `~/lanes/p4-w3b` | `integ/p4-w3b` | `9d4ed93e` | 0 | 34 | 28 | HOLDS REAL WORK (28 unique non-merge commits, 34 unreached by origin) |
| `~/lanes-local/b6keepclose` | `p5/b6-keeping-close-leaves-the-group` | `dbb60f68` | 0 | 3 | 3 | HOLDS REAL WORK (3 unique non-merge commits, 3 unreached by origin) |
| `~/lanes-local/capplant` | `scratch/aold` | `8d245b13` | 0 | 3 | 3 | HOLDS REAL WORK (3 unique non-merge commits, 3 unreached by origin) |
| `~/lanes-local/factory-four` | `p6/factory-four` | `83f5994b` | 0 | 3 | 3 | HOLDS REAL WORK (3 unique non-merge commits, 3 unreached by origin) |
| `~/lanes/p4-g8bxk` | `p4/g8bxk-task-card-home` | `07079291` | 0 | 3 | 3 | HOLDS REAL WORK (3 unique non-merge commits, 3 unreached by origin) |
| `~/lanes/p4-gezlj` | `p4/gezlj-route-v9` | `dcf8f840` | 0 | 3 | 3 | HOLDS REAL WORK (3 unique non-merge commits, 3 unreached by origin) |
| `~/lanes/p4-quest` | `p4/questions-lifecycle` | `a0479bd6` | 0 | 3 | 3 | HOLDS REAL WORK (3 unique non-merge commits, 3 unreached by origin) |
| `~/projects/worktrees/theseus/release-1.0.3` | `release/1.0.3` | `aa1e0950` | 0 | 4 | 3 | HOLDS REAL WORK (3 unique non-merge commits, 4 unreached by origin) |
| `~/lanes/p4-hyg` | `integ/p4-hygiene` | `be0864ca` | 0 | 9 | 3 | HOLDS REAL WORK (3 unique non-merge commits, 9 unreached by origin) |
| `~/lanes/h66v47-bar` | `p4/h66v47-bar` | `c11454e0` | 0 | 4 | 4 | HOLDS REAL WORK (4 unique non-merge commits, 4 unreached by origin) |
| `~/lanes/p4-0d40a` | `p4/0d40a-probe` | `de271783` | 0 | 4 | 4 | HOLDS REAL WORK (4 unique non-merge commits, 4 unreached by origin) |
| `~/lanes/h66v45-trim` | `p4/h66v45-trim` | `9831ba9f` | 0 | 5 | 5 | HOLDS REAL WORK (5 unique non-merge commits, 5 unreached by origin) |
| `~/lanes/p4-ui8dx` | `p4/ui8dx-86-proofs` | `f4861800` | 0 | 5 | 5 | HOLDS REAL WORK (5 unique non-merge commits, 5 unreached by origin) |
| `~/lanes/p3-tecj5` | `local/20261026-p3-tecj5` | `147caf87` | 0 | 6 | 5 | HOLDS REAL WORK (5 unique non-merge commits, 6 unreached by origin) |
| `~/lanes/o366s` | `probe/o366s` | `1dfb85cd` | 0 | 6 | 6 | HOLDS REAL WORK (6 unique non-merge commits, 6 unreached by origin) |
| `/tmp/b5secmerge` | `(detached)` | `343de585` | 0 | 7 | 6 | HOLDS REAL WORK (6 unique non-merge commits, 7 unreached by origin) |
| `~/lanes/wf482-bench` | `bench/wf482-benchbase` | `d5a3f1ef` | 0 | 7 | 7 | HOLDS REAL WORK (7 unique non-merge commits, 7 unreached by origin) |
| `~/lanes/p4-qc5r6` | `p4/qc5r6-unpaced-siblings` | `e7d05577` | 0 | 9 | 8 | HOLDS REAL WORK (8 unique non-merge commits, 9 unreached by origin) |
| `~/lanes/gauntlet` | `bench/gauntlet-1025` | `68d07a62` | 6 | 9 | 9 | NEEDS A HUMAN (dirty: 6 tracked edits, and 9 commits no origin ref reaches) |
| `~/lanes/p3t-cn-provenance` | `p3t/cn-provenance` | `7f297c98` | 7 | 1 | 1 | NEEDS A HUMAN (dirty: 7 tracked edits, and 1 commits no origin ref reaches) |
| `~/projects/theseus-wt/b7-route-cliff-measure-3239541` | `(detached)` | `3a2e1b57` | 1 | 0 | 0 | NEEDS A HUMAN (origin has the commits, but 1 tracked edits are uncommitted) |
| `~/lanes/p4-w3` | `p5/live-learning-self-fact` | `50de5054` | 16 | 0 | 0 | NEEDS A HUMAN (origin has the commits, but 16 tracked edits are uncommitted) |
| `/tmp/b6cpolicy-merge` | `(detached)` | `3a2e1b57` | 2 | 0 | 0 | NEEDS A HUMAN (origin has the commits, but 2 tracked edits are uncommitted) |
| `~/lanes-local/p0b-held-loop` | `p6/p0b-held-loop-repeat-cap` | `5cc7fbac` | 4 | 0 | 0 | NEEDS A HUMAN (origin has the commits, but 4 tracked edits are uncommitted) |
| `~/lanes-local/rv2newmain` | `(detached)` | `13b7776e` | 9 | 0 | 0 | NEEDS A HUMAN (origin has the commits, but 9 tracked edits are uncommitted) |
| `~/lanes-local/b5-courier-finish-cRTAoA` | `(detached)` | `515dc410` | 0 | 1 | 0 | holds nothing (1 unreached commits, all merges; no unique content) |
| `~/lanes-local/b5recut-merge` | `(detached)` | `491b6507` | 0 | 1 | 0 | holds nothing (1 unreached commits, all merges; no unique content) |
| `~/lanes-local/b6lingering-finish` | `(detached)` | `58a3cae4` | 0 | 1 | 0 | holds nothing (1 unreached commits, all merges; no unique content) |
| `~/lanes-local/fix-b4cell` | `(detached)` | `f15d364b` | 0 | 1 | 0 | holds nothing (1 unreached commits, all merges; no unique content) |
| `~/lanes-local/rv-b5outbox` | `(detached)` | `ed564047` | 0 | 1 | 0 | holds nothing (1 unreached commits, all merges; no unique content) |
| `~/lanes-local/rv2-b5outbox` | `(detached)` | `61d87b42` | 0 | 1 | 0 | holds nothing (1 unreached commits, all merges; no unique content) |
| `~/lanes-local/rv2prev` | `(detached)` | `9b37b0ee` | 0 | 1 | 0 | holds nothing (1 unreached commits, all merges; no unique content) |
| `~/lanes-local/rv2shapes` | `(detached)` | `80f44d86` | 0 | 1 | 0 | holds nothing (1 unreached commits, all merges; no unique content) |
| `~/lanes-local/rvwc-merge` | `(detached)` | `d31b9ad1` | 0 | 1 | 0 | holds nothing (1 unreached commits, all merges; no unique content) |
| `~/lanes-local/wadusixreds` | `(detached)` | `88444022` | 0 | 1 | 0 | holds nothing (1 unreached commits, all merges; no unique content) |
| `~/lanes-local/waduverify` | `(detached)` | `14e70f69` | 0 | 1 | 0 | holds nothing (1 unreached commits, all merges; no unique content) |
| `/tmp/b5c1c2-merge` | `(detached)` | `890adb16` | 0 | 1 | 0 | holds nothing (1 unreached commits, all merges; no unique content) |
| `/tmp/b5secmerge2` | `(detached)` | `a34e8bf0` | 0 | 1 | 0 | holds nothing (1 unreached commits, all merges; no unique content) |
| `~/lanes-local/rv2census` | `(detached)` | `b326caa6` | 0 | 2 | 0 | holds nothing (2 unreached commits, all merges; no unique content) |
| `~/lanes/p4-w4a` | `integ/p4-w4a` | `2978f5be` | 0 | 4 | 0 | holds nothing (4 unreached commits, all merges; no unique content) |
| `~/lanes-local/20f-audit` | `p6/20f-widened-audit` | `fbf14ebb` | 0 | 0 | 1 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/b3voicetrace` | `p5/b3-voice-leadin-trace` | `3f278b74` | 0 | 0 | 2 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/b4syncdl` | `p5/b4-sync-rpc-deadline` | `b4f0fe04` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/b5withhold` | `p5/b5-self-build-withhold-real` | `1fc15dc4` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/b6-imported-memory-keep-6d712471` | `p5/b6-imported-mcp-memory-keep` | `df709670` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/b6-memory-round2-ca79e0c8` | `(detached)` | `09d41c3b` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/b6cpolicy` | `p5/b6c-policy-one-word-and-overhold` | `76382ff0` | 0 | 0 | 1 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/cond-p64s` | `integ/p6-4s` | `eaaad022` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/depart` | `(detached)` | `5cc7fbac` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/flaketruth` | `p5/chain-test-override-retired` | `1cb82fd9` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/flaketruth2` | `p5/calls-time-stall-in-the-turn-poll` | `73f8ed24` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/harvest-p6` | `integ/p6-9` | `0caaed78` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/harvest-w3` | `(detached)` | `31d1ede0` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/ievo3` | `p6/ievo3-sync-on-demand` | `381acc7e` | 0 | 0 | 1 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/integ-p6-1` | `integ/p6-1` | `0eb5d245` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/integ-p6-2` | `integ/p6-2` | `9d0d7473` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/integ-p6-3` | `integ/p6-3` | `9cd5e90b` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/integ-p6-4` | `integ/p6-4` | `0d530f8e` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/integ-p6-5` | `integ/p6-5` | `64817e2a` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/integ-p6-6` | `integ/p6-6` | `32e22eb5` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/integ-p6-7` | `integ/p6-7` | `d707a56a` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/integ-p6-8` | `integ/p6-8` | `e28c3ca1` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/precut` | `(detached)` | `5cc7fbac` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/rev-b4syncdl` | `(detached)` | `b4f0fe04` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/rework-w14` | `p6/w14-thinking-on-switch-r2` | `6ee9502f` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/rework-w17` | `p6/w17-store-format-r2` | `32344eaf` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/ri5up-onto` | `p6/ri5up-onto-report` | `5cc7fbac` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/s22-2-selfsign` | `p6/self-loop-signs-its-content-commit` | `405ebff3` | 0 | 0 | 2 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/s55d-http2` | `p6/s55d-http2` | `fb7dcc29` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/s7-b4fix` | `p5/b4-sync-rpc-deadline-armfix` | `d7f078b8` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/s7-keelprobe` | `(detached)` | `9c6434c1` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/s7-lockexec` | `p5/b7c-gate-tests-lock-exec-seconds` | `05fdffe4` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/sju2s` | `p6/sju2s-tool-line-not-subtext` | `f320e5a1` | 0 | 0 | 1 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/w11-r2` | `p6/w11-discord-lines-r2` | `46f90ded` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/w22-finish` | `(detached)` | `4199301c` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/w23-pubtp08` | `p6/w23-privacy-label` | `62e885a3` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/w30-memory-pressure-2-r2` | `p6/w30-memory-pressure-2-r2` | `a60080c7` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/w31-aws-2-r2` | `p6/w31-aws-2-r2` | `d19983ee` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/w33-store-format-2-r2` | `p6/w33-store-format-2-r2` | `cfa5d660` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/w34-r2` | `p6/w34-trust-from-chat-r2` | `7ae28564` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/w35-reversibility-2-r2` | `p6/w35-reversibility-2-r2` | `fcd92fc9` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/w36-models-2-r2` | `p6/w36-models-2-r2` | `3bda927e` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes-local/wadus8` | `p6/wadu-s8-after-a-grant` | `0d21b49a` | 0 | 0 | 3 | holds nothing (origin reaches its whole history) |
| `~/lanes/p4-w1` | `p5/cn-shared-const` | `1e58f318` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/lanes/p4-w2` | `p5/cn-predicate-keel-file` | `7d0d9019` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/projects/theseus` | `main` | `7c4c0d69` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/projects/worktrees/theseus/release-size` | `(detached)` | `5cc7fbac` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `~/worktrees/b4-anchor-reasons-surface` | `p5/b4-anchor-reasons-surface` | `9ee902de` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `/tmp/b5rev` | `(detached)` | `fb7586db` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |
| `/tmp/b7-judge-budget-doc-row-gate.KpQaIA` | `(detached)` | `19c0676e` | 0 | 0 | 0 | holds nothing (origin reaches its whole history) |

## What was deliberately left

- **No worktree, lane, branch or ref was removed, and none was created except this worker's own
  `~/lanes-local/factory-four`.**
- `~/lanes-local/p0b-held-loop`, `~/lanes-local/20f-audit` and the ARM lanes were read and nothing else.
- No recommendation to prune anything is made here. The 15 merge-only lanes and the 50 fully-reached lanes are
  the obvious candidates **if** someone decides to act, and that decision is not this worker's.
