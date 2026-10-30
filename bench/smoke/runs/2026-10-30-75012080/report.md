# PASS

Release smoke of `75012080769b14b4cdfa45d97d49107771ce85bc` on 2026-10-30 (candidate 75012080769b, theseusd 1.0.3; tb's static build 2777771ffbc6 (dirty tree)). Not reportable: a release check against the floor in `bench/smoke/floor.json`.

| part | result | number | floor | delta |
|---|---|---|---|---|
| tb | PASS: 8 solved, both controls failed | 0.085 $/solved (n=10) | - | - |
| route | n/a until the flip: the daemon routes with route.v8, the cases are route.v10's; read 39/60 on the model and 9/13 musts at worst, for the record | 39 worst model/60 (n=3) | - | - |
| recall | PASS: 160 tests | 185 s (n=160) | - | - |
| resume | PASS: 3/3 resumed, 6 effects, none repeated | 3 resumed (n=3) | 3 | - |
| dm | PASS: 5 replies, footers | 4.15 slowest s (n=5) | 60 | - |

**Cost: $0.70** (price table `8414028cd0f7`), **wall: 22.3 min** (the parts' own clocks).

Corrected 2026-10-30 at 12:50, on the same install: route is re-judged from the same three replays with the pack the daemon routed with (route.v8), and dm is the corrected design's rerun (`runs/2026-10-30-75012080-125047`): a fresh session working in `~/projects/theseus`, every turn marked as the smoke's, 0 approvals asked. The first design's dm ran from a lane outside the workspace, asked the owner once, and is superseded.

## TB2 slice, per task

| task | passed | $ | wall s | turns | models | exception |
|---|---|---|---|---|---|---|
| fix-git | yes | 0.051 | 17.501 | 6 | claude-sonnet-5-5 6 | - |
| overfull-hbox | yes | 0.126 | 69.745 | 7 | claude-sonnet-5-5 7 | - |
| regex-log | yes | 0.039 | 26.748 | 3 | claude-sonnet-5-5 3 | - |
| db-wal-recovery | yes | 0.029 | 13.999 | 4 | claude-sonnet-5-5 4 | - |
| git-multibranch | yes | 0.047 | 24.999 | 5 | claude-sonnet-5-5 5 | - |
| sparql-university | yes | 0.123 | 52.499 | 10 | claude-sonnet-5-5 10 | - |
| password-recovery | yes | 0.074 | 40.748 | 10 | claude-sonnet-5-5 10 | - |
| bn-fit-modify | yes | 0.061 | 47.749 | 7 | claude-sonnet-5-5 7 | - |
| configure-git-webserver | no | 0.061 | 78.246 | 7 | claude-sonnet-5-5 7 | - |
| raman-fitting | no | 0.07 | 54.748 | 10 | claude-sonnet-5-5 10 | - |
