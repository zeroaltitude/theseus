# theseus-75t4q measurement notes (raw)

Test: `theseus-core tests_onto_names::a_chain_asked_over_two_nights_clusters`
Filter: `cargo nextest run -p theseus-core --lib -E 'test(a_chain_asked_over_two_nights_clusters)'`
(`cargo nextest list` with the same filter shows exactly one test:
`theseus-core tests_onto_names::a_chain_asked_over_two_nights_clusters`.)

## Base 78fcfb6d, VM idle (cpu PSI avg10 ~2), 20 runs
Test time 0.142-0.253 s each, 20/20 pass; wall (incl. nextest start) 1.37-1.92 s.
```
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
