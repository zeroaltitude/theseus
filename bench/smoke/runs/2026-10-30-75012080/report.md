# FAIL (route, dm)

Release smoke of `75012080769b14b4cdfa45d97d49107771ce85bc` on 2026-10-30 (candidate 75012080769b, theseusd 1.0.3; tb's static build 2777771ffbc6 (dirty tree)). Not reportable: a release check against the floor in `bench/smoke/floor.json`.

| part | result | number | floor | delta |
|---|---|---|---|---|
| tb | PASS: 8 solved, both controls failed | 0.085 $/solved (n=10) | - | - |
| route | FAIL: run 1: 39/60 on the model < 54; run 1: 9/13 musts; run 2: 39/60 on the model < 54; run 2: 9/13 musts; run 3: 39/60 on the model < 54; run 3: 9/13 musts; the daemon's whole bar met 0/3 | 39 worst model/60 (n=3) | - | - |
| recall | PASS: 160 tests | 185 s (n=160) | - | - |
| resume | PASS: 3/3 resumed, 6 effects, none repeated | 3 resumed (n=3) | 3 | - |
| dm | FAIL: unexpected reply: turns [2, 4] | 7.12 slowest s (n=5) | 60 | - |

**Cost: $0.82** (price table `8414028cd0f7`), **wall: 22.3 min** (the parts' own clocks).

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
