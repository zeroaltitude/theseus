# b7-ec2-hold-not-released — a finished gate box stayed held for twenty minutes

Board line 45 (B7, P3). Recorded 2026-11-13 by the factory-four worker.

**This note is a record, not a product change.** The file it describes — `~/reports/theseus-fleet/ec2.sh` — is
**not tracked in this repository**, and this repository has no home for the fleet and departure operator
scripts: none of `ec2.sh`, `depart.sh`, `cloud-returns.sh` or `arm.sh` is tracked, `scripts/` is build-and-gate
tooling, and `infra/aws/` is the hands/posture/foundation CloudFormation and says nothing about the
`theseus-fleet` stack. Rather than invent a home, the change is recorded here in full so it is recoverable from
the repository even though the script is not in it.

## What the board said, and what was already built

> `ec2.sh launch` still does not reserve a box for its caller

That sentence was already stale: `launch [n] [--for LANE]` reserves the first box it starts by tagging it with
the lane at `run-instances` time, `rm LANE` releases a claim and keeps the box, and `release-done [LANE|all]`
releases holds whose work has finished — which `send` runs itself before it reports no free box. The reservation
exists.

**The part that was missing is the end of the hold, not its beginning.** A hold was released only by a
twenty-minute idleness *guess* (`RELEASE_GRACE`) or by hand. So for up to twenty minutes after its gate exited,
a box that was idle, healthy and warm was invisible to a departure that needed a second host —
`release-done`'s own comment records that this happened twice on 2026-11-11.

## The change

A hold now ends on **knowledge** rather than on a guess, by giving an *observed* finish its own short grace.

- `lane_finish LANE BUCKET` → `done | running | unknown`, read from the lane's own result objects in S3
  (`fleet/results/<lane>/<kind>.exit`), which are what `wait` and `result` already read. `done` means at least
  one kind is recorded and none of them says `running`.
- `release_done` picks the grace from that answer: `RELEASE_GRACE_DONE` (**new, default 2 minutes**) when the
  finish is observed, `RELEASE_GRACE` (**default 20, unchanged**) when it is only inferred.
- `wait`, on the exit it has just observed, calls `release_done LANE` — the completion path releasing the hold
  in the place that knows the unit finished. `HOLD=keep` opts out.

Three invariants were deliberately preserved:

1. The on-box probe still runs and `units=0` is still required, so nothing is released while a unit is live.
2. **An answer it cannot parse still keeps the hold.** That timidity is the reason the mechanism is trusted.
3. The box is **kept**, never terminated. The grace timer remains the belt to this brace, and the 30-minute
   reaper still sits outside both.

`MAXBOXES` is untouched at 10 and `RELEASE_GRACE`'s default is untouched at 20, as the line required.

### Why two minutes and not zero

A caller may run units back to back on one lane — `precut-fleet.sh` runs `gate` and then `test` on the same
lane, seconds apart. Releasing at zero would hand its warm `target/` to another caller between the two. Two
minutes keeps the box its own across that gap while cutting the starvation window from twenty minutes to two.

## How it was proved, with no compile, no AWS call and no box

`~/reports/theseus-fleet/test-release-done.sh` (also untracked). `ec2.sh` is a case-dispatch script and cannot
be sourced, so the harness copies everything above the dispatch line into a scratch file, sources that, and
replaces only the four things that touch the world — `W` (the EC2 API), `WM` (S3), `SSM` (the on-box probe) and
`boxes` (the fleet listing) — with fixtures. The function bodies under test are the real ones.

**12 cases, 12 passed.** The matrix:

| case | probe | S3 exits | before | now |
| --- | --- | --- | --- | --- |
| the defect | `units=0 idle=3` | `gate=0` | kept 17 more min | **released** |
| back-to-back caller | `units=0 idle=1` | `gate=0` | kept | kept |
| a unit is live | `units=1 idle=99` | `gate=0` | kept | kept |
| finish not observed | `units=0 idle=5` | `gate=0,test=running` | kept | kept |
| finish not observed | `units=0 idle=25` | `gate=0,test=running` | released | released |
| no results at all | `units=0 idle=5` | — | kept | kept |
| no results at all | `units=0 idle=25` | — | released | released |
| unparseable answer | `connection refused` | `gate=0` | kept | kept |
| empty answer | — | `gate=0` | kept | kept |
| an unheld box | — | — | untouched | untouched |
| a `pending` box | — | `gate=0` | kept | kept |
| `release_done LANE` selectivity | another lane | — | ignored | ignored |

One harness bug is worth recording because it is the house's recurring shape: `release_done` was first run
inside `$( )`, so its assignment to the fixture's `DELETED` was lost to the command-substitution subshell and
three passing cases reported as failures while printing the correct `released lane …` line. That is the same
family as the `| head` exit-status trap in `FLEET.md` section 3.

## What was deliberately left

- **No AWS box was started and no live fleet call was made.** The line is about how `ec2.sh` behaves.
- `reap-idle`, `terminate`, `rm` and `launch` are unchanged.
- Nothing was landed, queued, ticked, installed or tagged.
