# CLOUD_REPORT — theseus-yh48a

**Outcome: STOP-with-pins.** No behaviour changed. Both `benchtasks.ts` sites stay on `family()`, each now carries a
comment saying why, and two tests pin the choice. `benchframing.ts` got a comment only.

Base sha: `ee725a2d` (the branch's parent; not rebased).

## Step 0: citations
All confirmed at `ee725a2d`: `bench.ts` :346, :354, :360, :363, :366, the `showsName` comment (it spans :370-380, the
code is :381-382), `benchtasks.ts` :47/:48/:60, `benchframing.ts` :26-27. **One off by one:** `rank` is at `bench.ts:921`
for the `const` and `:922` for the sort; the `sameArm` use is on :921, so it is right. The prompt's `:376` for the
`showsName` marker is the middle of its doc comment.

## The real two-arm slice
`docs/benchmarks/2026-10-03-harbor-worth-spike` (theseus, theseus-batching, claude-code, 1 trial each per task). This is
the only per-task dataset that holds both `theseus` and a listed ablation (`2026-10-06-harbor-efficiency-checks` has them
too, but not in task-by-arm form). `taskRows` output at the base:

```
fix-git               | theseus 1/1 ; claude-code 1/1 ; theseus-batching 1/1 | theseus=theseus | best=claude-code | too few attempts to say
nginx-request-logging | theseus 1/1 ; claude-code 1/1 ; theseus-batching 1/1 | theseus=theseus | best=claude-code | too few attempts to say
prove-plus-comm       | theseus 1/1 ; claude-code 1/1 ; theseus-batching 1/1 | theseus=theseus | best=claude-code | too few attempts to say
psf__requests-1921    | theseus 1/1 ; claude-code 1/1                        | theseus=theseus | best=claude-code | too few attempts to say
```
Under `family` (today): the ablation is already **its own side/cell**, listed after the rivals, and is not a rival.
Under `sameArm`: the table would be identical in sides, but `best` could become the ablation whenever it ties or beats the
rivals, and the verdict would read "against Theseus + paragraph". The slice is all-pass, so it does not separate the rules
by itself; the tests use a slice that does (see below).

## Choice: leave both on `family()`
- The table already shows the ablation as its own column, so "one side or two" is already answered: two sides. The only
  question is whether it is a **rival**. A per-task table exists to put Theseus beside other harnesses; the ablation vs
  Theseus comparison is the arms chart's job.
- `sameArm` at `pickTheseus` is worse: a set with only `theseus-batching` and a rival would have **no Theseus side** (the
  second test pins this).
- Both sites change together (they would have to), so I changed neither.

## Sweep: `git grep -n "family(" cockpit/src`
- `bench.ts:346` definition; `:360` `sameArm` itself uses it.
- `bench.ts:382` `showsName`: deliberately on `family`, untouched.
- `benchtasks.ts:48`, `:60`: kept, commented, tested.
- `benchframing.ts:27`: comment only, names `theseus-ieq1w` as owner.
- `views/Benchmarks.tsx:535`: **not a membership test** (`family(h) !== h` decides whether a tip appends the raw key); no change.
There is no fourth membership test.

## Red-first evidence
No behaviour changed, so there is no failing-at-base run; the planted revert is the swap to `sameArm`:
- Pinned (green): `benchtasks.test.ts` 6 pass, 0 fail.
- Planted revert (both predicates -> `sameArm`):
  - test 5: `expected: 'claude-code'`, `actual: 'theseus-batching'` (best rival became the ablation)
  - test 6: `expected: 'theseus-batching'`, `actual: undefined` (no Theseus side)
  - 4 pass, 2 fail. File restored and `touch`ed.

## Checks
- `npm test`: 925 pass, 0 fail. `tsc -b`: clean. `npm run build`: built. `oxlint`: warnings only, none in files I touched.
  (`npm ci` printed an audit notice and exited nonzero at first, but `node_modules` was present and every check ran.)
- `python3 scripts/keel-guard.py`: `keel: ok (a branch: merge-base(HEAD, origin/main)..HEAD, ee725a2dc9; 4 files changed, 0 findings acked)`.
- Rust, `docs/benchmarks/`, `manifest.py`, `docs/design/`: untouched. The full gate was not run (cockpit-only, comments and a test).

## Untouched
`benchframing.ts`: one comment line, no code change. `showsName`: not edited.

## Integrator notes
Both commits on this branch are for `main`: the code commit (comments + tests). `CLOUD_TASK.md` and this report are not
for `main`; drop them at the merge. Recommendation for the owner: nothing further on `showsName`.

CLOUD REPORT COMPLETE
