# theseus-75t4q measurement notes (raw)

Test: `theseus-core tests_onto_names::a_chain_asked_over_two_nights_clusters`
Filter: `cargo nextest run -p theseus-core --lib -E 'test(a_chain_asked_over_two_nights_clusters)'`
(`cargo nextest list` with the same filter shows exactly one test:
`theseus-core tests_onto_names::a_chain_asked_over_two_nights_clusters`.)

## Base 78fcfb6d, VM idle (cpu PSI avg10 ~2), 20 runs
Test time 0.142-0.253 s each, 20/20 pass; wall (incl. nextest start) 1.37-1.92 s.
```
1 rc=0 1.922245423 PASS [   0.172s] 
2 rc=0 1.693726096 PASS [   0.201s] 
3 rc=0 1.555613185 PASS [   0.178s] 
4 rc=0 1.507008167 PASS [   0.192s] 
5 rc=0 1.622411000 PASS [   0.177s] 
6 rc=0 1.685381100 PASS [   0.198s] 
7 rc=0 1.724569935 PASS [   0.220s] 
8 rc=0 1.711910413 PASS [   0.218s] 
9 rc=0 1.611914996 PASS [   0.185s] 
10 rc=0 1.635550379 PASS [   0.253s] 
11 rc=0 1.583122403 PASS [   0.142s] 
12 rc=0 1.501841526 PASS [   0.142s] 
13 rc=0 1.541771684 PASS [   0.177s] 
14 rc=0 1.449485724 PASS [   0.173s] 
15 rc=0 1.536997883 PASS [   0.187s] 
16 rc=0 1.480777007 PASS [   0.170s] 
17 rc=0 1.585314659 PASS [   0.184s] 
18 rc=0 1.374630899 PASS [   0.148s] 
19 rc=0 1.483764563 PASS [   0.196s] 
20 rc=0 1.611570066 PASS [   0.170s] 
finished
```

## Base, 6 busy loops at nice 0, test at nice 19 (cpu PSI avg10 ~50-54)
Run 1: nextest SLOW then TIMEOUT x2 (killed at 120 s); the runner reported 267 s wall
(a gdb attach was paused on it for part of that). Stopped after one run: it is a
hang-to-kill every time the pressure is over the bar, so more runs add nothing.
```
1 rc=100 267.3 SLOW TIMEOUT TIMEOUT
```
Stack of the hung process (gdb, thread apply all bt): no thread holds a lock or is in a
blocking call. Both tokio workers parked; one in epoll_wait with a 949 ms timeout (a
tokio timer); FakeJev's accept thread in accept4; store-writer parked on its channel.
That is a task asleep in `tokio::time::sleep(1 s)` in a loop, i.e.
`theseus_store::pressure::quiet`.
