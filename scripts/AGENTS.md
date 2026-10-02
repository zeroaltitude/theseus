# scripts

- `gate.sh`: the commit gate. Every check must pass, and the first failure stops it.
- `smoke.sh`: an end-to-end check of a built daemon, with real secrets and real model calls.

`gate.sh` is a shared file: it changes only at a join, one change at a time.

## The gate

Run it before every commit, in the tree you will commit, under the shared lock, and chain the commit with `&&`
(a `;` once let an unformatted commit through):

```bash
flock -o ~/.cache/theseus-gate.lock scripts/gate.sh && git commit -S -F <message file>
```

It runs, in order:

1. `cargo fmt --all -- --check`, then `cargo clippy --workspace --all-targets -- -D warnings`.
2. The reader rule's registry test alone (`tests_registry` in theseus-core), so a miss stops the gate in seconds
   and names every fix at once.
3. The whole suite, `cargo nextest run --workspace` (about 1,300 tests).
4. The generated TypeScript: it fails when `web/src/protocol.gen/` differs from the commit.
5. The lifecycle bench, on debug builds of `theseusd` and `theseus-sim`, ten runs a phase against §9's budgets. It
   first flushes dirty pages and waits, up to 5 minutes, until IO and CPU pressure are low and the load is under the
   core count. A miss reruns once, and only a second miss fails. Every run goes to the bench history
   (`$THESEUS_BENCH_HISTORY`, by default `~/.cache/theseus/bench-history.csv`). With `THESEUS_GATE_NO_BENCH=1` (a
   lane's gate, which the lane recipe sets), this step is skipped: the gate that joins the lane to `main` runs it.
   Then the turn bench (`theseus-sim bench turn --check`): a plain turn's frames, counted from the daemon's WAL,
   against §9's per-turn overhead restated as frames (5; the floor is 2). A count needs no quiet machine, so it runs
   in a lane's gate too, with five runs of each kind and no burst (about 5 s); at the join it runs ten runs and a
   burst of 30 turns and records the row (about 11 s). A miss reruns once.
6. `cargo deny check`: licences, advisories, bans, and sources.
7. The web apps' lint and build, each when its `node_modules` exists, and then a check that the Observatory's
   committed build is current.

It ends with `gate: ok`.

### How long it takes

On a warm target, a few minutes: the suite is about 90 s and the bench about 7 s of it, plus any wait for a quiet
machine. A lane's niced gate on a warm target took 135 s once it had the lock (2026-10-01: the suite 105 s at 4
threads, a 15 s wait to settle, the bench 7 s). A fresh worktree's first gate also compiles the whole workspace,
dependencies at opt-level 2 included, which takes far longer: about 22 minutes at nice 19 beside busy neighbours. Waiting for the lock behind another gate, and for a quiet machine, can each add minutes, so
give the gate's command a timeout of 30 minutes or more (a `proc.run` call can ask for up to its
`proc_timeout_max_secs`).

### Running it beside other work

- **The lock** keeps one gate's tests out of another's timing bench. `flock -o` closes the lock's fd in the gate, so
  nothing it starts can keep the lock after it. Never take it with `exec N>lock; flock N` in a shell that starts
  anything long-lived.
- **A lane's gate runs niced**, with its own `CARGO_TARGET_DIR`, and `CARGO_BUILD_JOBS=4` and
  `NEXTEST_TEST_THREADS=4` exported (one or the other, never also nextest's `--test-threads`, which it then refuses
  as given twice): `nice -n 19 ionice -c3 flock -o ~/.cache/theseus-gate.lock scripts/gate.sh`. The bench runs
  `target/debug/theseus-sim` by a relative path, so a worktree needs a `target` symlink to its target dir.
- **A lane's gate skips the bench** (`THESEUS_GATE_NO_BENCH=1`). The bench's settle step waits for a quiet machine
  while holding the shared lock, so every other agent's gate queued behind it; the join's gate on `main` benches
  instead. A lane whose work touches the start path runs `target/debug/theseus-sim bench lifecycle --runs 10
  --check` alone, at normal priority, once before its join, and quotes it.
- **A niced gate whose only miss is the bench**, beside busier neighbours, counts as green when the bench rerun alone
  at normal priority passes. Quote both runs.
- **A gate that sits at 0% CPU** is waiting on a lock: this one (held by another gate, or by an orphan that
  inherited it: scan `/proc/*/fd` for it, since `/proc/locks` hides a dead owner), or cargo's package cache. Find the
  holder before waiting longer, and never kill another agent's process.

## smoke.sh

It needs the vault's service-account token (`OP_SERVICE_ACCOUNT_TOKEN`, or `THESEUS_OP_TOKEN_FILE`) and a config
(`THESEUS_CONFIG`), and spends a little on real model calls. It starts its own daemon on a temporary socket and
state dir, and kills it at the end.

- **Its web check probes `WEB_PORT`, by default 7433: the operator's daemon.** Run it only with a scratch config
  whose `[web]` is off or on a port of its own, and `WEB_PORT` set to match.
