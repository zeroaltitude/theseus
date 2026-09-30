# A store written by an older binary (theseus-qa0 F4a)

The versioned-reader rule's fixture (P5b): today's binary must serve this store at once and read every
record in it. Never open it in place: tests copy it to a temporary directory first, because an open
writes (the index's checkpoint, the ledger rows of a start).

**Written by** the tree at 460a35b (F1's last commit, 2026-09-29, before F2; manifest format 2, every
record at schema 1), built from `git archive 460a35b` in its own target directory, by its own lifecycle
bench, one run of the shutdown phase:

```
theseus-sim bench lifecycle --theseusd <that build's theseusd> --phases shutdown --runs 1 --dir <dir>
```

The bench started the daemon four times (two warm-ups, a start, and a restart after the measured
shutdown). It opened three sessions and submitted a turn whose stand-in model ran `sleep 300` through
`proc.run`, shut down with the job running, restarted, and cancelled the job's execution. `<dir>/state/store`
is this directory, as the old daemon left it:

- `MANIFEST.json`: format 2, redb;
- `wal/000000001.seg`: 121 records, 59,867 bytes;
- `index.redb`: the old daemon's index, with a checkpoint partway through, so an open replays a tail.

| Kind | Records | Latest by key |
|---|---|---|
| session | 6 | 4 sessions |
| ledger | 76 | |
| execution | 17 | 4 executions: 3 waiting on input, 1 cancelled |
| action | 14 | provider calls and the `proc.run` job |
| completion | 2 | |
| node | 5 | the turn's messages |
| compilation | 1 | |

It has no outbox, wakes, tasks, or hold on external text: those came after 460a35b. Their readers are
the serde defaults this fixture exercises (the fields are absent).
