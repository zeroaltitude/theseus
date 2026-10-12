# CLOUD_REPORT.md: tamper-ledger (theseus-bmkxk)

Branch `cloud/20261011-tamper-ledger`. Commits: `2d0ef75d` (the chain, the walk, the surfaces, the tests) and
`01f4665e` (a broken row still names its kind). This file is the last commit.

## What I found

- The brief's two goldens: **`core_output.txt` changed** (every ledger row gains `"chain":"#.<sha#>"`, 399 lines);
  **`kernel_frames.txt` did not change** (`THESEUS_GOLDEN=write` over theseus-kernel left it byte-identical).
- main was at **store format 29** when I cloned (the standing row), so the bump is **29 -> 30**.
- The bump is for a new field on a stored record. The field is added to the row's *bytes* by the writer, not to the
  `LedgerRow` struct (four test files build `LedgerRow` literals, and nothing but the writer may set a link), so
  `tests_layouts` holds two samples (a format-29 row with no `chain`, which reads and round-trips; a format-30 row
  with one, `Kept::AllBut(["/chain"])`) and a test opens a format-29 store holding old rows, writes, checks the chain
  starts and the manifest moves to 30.
- Several existing tests counted positions or kinds in a store whose first ledger write now carries the chain's
  start row; I adjusted their numbers and kind lists. Three store tests whose own arithmetic is about positions use a
  `cfg(test)` `WalStore::without_chain()` (never a production knob). No test was deleted.

## What I changed

**1. The chain** (`theseus-store/src/chain.rs`, `store/chain_writer.rs`, `store.rs`'s `commit`).
- A link is `<seq>.<sha256 hex>`, the row's last JSON field: `,"chain":"<seq>.<64 hex>"}` spliced onto the *bytes the
  caller handed the store*. Digest = SHA-256(previous digest (32 bytes) || seq (8 bytes, LE) || canonical bytes).
  **The canonical bytes are the stored payload with the suffix taken off, by bytes, never re-serialized**, so no serde
  version, key order or float format can change what was hashed. (`sha2` 0.10 was already in Cargo.lock; the store
  gains `sha2` and `theseus-protocol` as dependencies, no new package. The protocol dependency names the two new
  ledger kinds, `LedgerKind::LedgerChainStarted` and `LedgerHistoryPinned`.)
- Computed only in the writer, in WAL order, frame by frame, on a *clone* of the head that is committed to the shared
  state only once the batch is durable and indexed: a frame the log refuses, or a sync that fails, gives its links
  back. A payload that is not a JSON object is stored as it came, unchained (a walk calls it broken once the chain has
  started; nothing in production writes one).
- The head is loaded lazily (first ledger write) from the newest chained ledger row: an index read, never a walk.

**2. Its start on an existing store.** The first ledger write of a chain-aware build adds a `ledger.chain_started` row
*in the same frame, ahead of the caller's first row* (`Job::hidden`: its position is no one's answer, and the frame
count is unchanged). It counts the unchained rows (index count) and says where they end. **Design choice for the
owner:** the brief asks for "a digest over them all" in that row. Reading every old row at the first write would put
history-sized work on the start path (FAST: nothing before serving grows with history), so the digest is a *second*
row, `ledger.history_pinned`, written once after serving by a paced `ledger-pin` thread
(`theseus-core/src/ledger_pin.rs`: 2 s after serving, `SCHED_IDLE`, waits while the machine is busy, holds the core by
`Weak`; the META key `ledger.history_pinned` in the same frame marks it done). The walk checks the start row's count
and the pin's digest against the old rows. Until the pin lands the old rows are pinned by count only. A fresh store
pins nothing. (A first version of the thread held the store's `Arc` right after serving and broke `early_signal`'s
"a stop leaves nothing to replay"; the sleep before it first touches the store is the fix, written into core's AGENTS.md.)

**3. `theseus ledger verify [--since POS] [--anchor SEQ:DIGEST] [--json]`**, `theseus ledger head`, and the read-only
protocol method `ledger.verify` (`theseus-protocol/src/ledger_verify.rs`; 8 lines of lib.rs; no row data in the answer).
The walk (`chain_walk::verify`) streams a page of 2,000 rows at a time. Output:
`ledger ok: 88 rows chained since 2026-10-12 10:07:56, head 2ffbf7d7… (seq 87, position 157), in 2 ms`, or
`ledger BROKEN at position 56: turn.started (seq 33), 2026-10-12 10:08:04: edited` with a detail line; exit 0 / 1.
Faults: `edited`, `removed` (N rows), `reordered`, `out_of_order`, `unchained`, `truncated` and `rewritten` (against an
anchor), `history` (the old rows no longer match the count or the pin). Edited / removed / reordered are told apart by
the seq in the link plus one row of lookahead (a swap shows as a seq jump whose next row is the missing one).
`--since` trusts the first row it sees and checks everything after it. `ledger.verify` is routed through
`rpc_prefixed` (rpc/import.rs) because `dispatch` is at clippy's 100-line limit.

**4. The head, anchored.** `theseus ledger head` prints it; `--anchor SEQ:DIGEST` (8+ hex characters) checks a walk
against a head recorded earlier; health's `ledger_head` shows `2ffbf7d7…#87`; the durability tender puts `SEQ:DIGEST`
(`Hooks::ledger_head`) on every segment's and tail's DynamoDB index row (`ledger_head`) and in the
`durability.shipped` row.
**What the chain catches:** an edit of a row, a removed row, two rows swapped, a row with its link stripped, an edit of
the pre-chain history (after the pin), and, against a head recorded earlier, a truncated tail or a chain rewritten
from some row onward. **What it cannot:** a forger who rewrites every later row with Theseus's own code and has no
earlier head to compare against. (Said in `docs/technical-overview.md`, the keel section.)

**5. The cost, stated.**
- Bytes added per row: `,"chain":"<seq>.` + 64 hex + `"` = **77 bytes at a one-digit seq, 83 at seven digits**. On 400
  single-row frames (release) the WAL grew 79 B/row: **+20.4 % on a 333-byte row, +7.0 % on a 1,083-byte row** (a real
  store holds many other records, so its growth is less). One extra ~200-byte row, once.
- Writer: `chain::seal` costs **4.7 us/row at 333 B, 8.4 us/row at 1,083 B** (release, allocation included). Against one
  fdatasync per frame (~0.5 ms here) it is lost in the noise: 400 single-row frames took 561 us/frame unchained and
  477 us chained (the run order decided that, not the chain). No frame is added.
- `ledger verify` on **1,000,001 rows (402 MB of WAL): 5.5 s** (release; a warm second run 5.3 s), constant memory
  (`tests/chain_stream.rs`: 30 MB of rows walked, peak heap growth under 3 pages). Writing the same million took 24 s.
- `theseus-sim bench lifecycle --runs 10 --check` (debug binaries, this 4-core VM, p95 ms). After, three full runs: cold
  48.7 / 61.5 / 61.2 (budget 50 + 7), shutdown 35.5 / 26.8 / 48.5, kill+restart 41.6 / 45.7 / 58.5, swap 53 / 42 / 38.
  Before (the parent commit, built in a worktree), two runs: cold 39.2 / 46.3, shutdown 39.5 / 36.1, kill+restart
  59.0 / 66.3. Both builds miss `health, after the seed` (VM: p95 8.4 to 15.5 against 5 ms), and the "after" runs miss
  cold start twice by a few ms and once missed shutdown (1,908 ms, one sample of ten, not seen in thirteen later
  runs). I judge these VM noise: the chain adds one hash per row, no frame, and the start frame one record. **The budgets
  are the owner's machine's to judge: run the bench there.**
- Frame budgets, `theseus-sim bench turn --runs 5 --burst 0 --check`: frames_plain 4/4, frames_tool 8/8, syncs_plain 2/2,
  frames_batch 14/14, all ok (this tree's budgets are 4 and 8, not the brief's 5 and 9). One lifecycle run showed a
  cancel at 3 frames once under load; twelve later runs: 2 every time.

## How I proved it

- `theseus-store` `store/tests_chain.rs` (18 tests): a fresh store starts and verifies; a stored row is the caller's bytes
  plus one field (77 B); a restart continues from the stored head with one start row; a store from before the chain starts
  one pinning its count; **8 appenders x 25 frames make one valid chain of 201**; several ledger rows in one frame; a
  non-object row is left alone; a forged chain verifies; **edited** (position 8, "changed after it was written"),
  **removed** (the row after it, `missing: 1`), **swapped** (`reordered`, "positions 8 and 9"), **truncated tail** (against
  an anchor: "2 row(s) are missing from the tail"), a chain **rewritten** by a forger re-sealing rows 4..10 (the walk
  alone passes it; the earlier head catches it), a **stripped link**, a **missing first row**, `--since`, the pinned
  history (an old row edited; an old row removed), and an edit that broke the JSON still names its kind.
  `tests/chain_stream.rs` (memory bound, counting allocator). `ledger_pin::tests` (3), `rpc/tests_ledger_verify.rs` (3),
  `tests_layouts` (2 samples and the 29 -> 30 upgrade), `aws/tests_durable.rs::the_ledger_head_rides_each_segments_row`,
  and the CLI's words (`ledger_verify.rs`, 4).
- **Planted reverts** (each restored and `touch`ed; `git status` clean after): (a) the hash skips the row's bytes
  (`h.update(canonical)` removed) -> `an_edited_row_is_found...`, `a_chain_rewritten...`, `since_starts_at...` FAILED;
  (b) the chain restarts at a restart (`read_chain_head` reads nothing) -> `a_restart_continues_the_chain_from_the_stored_head`
  FAILED; (c) verify always ok (a break is never recorded) -> 9 of 17 tests FAILED (removed, rewritten, stripped,
  first-missing, truncated, edited, history, since, swapped).
- The whole suite on this tree (`cargo nextest run --workspace`): 4251 tests, 37 failed: the 33 known L1/sandbox ones
  and four that were effects of this VM's disk filling and passed alone: `theseus-judge
  builders::tests::every_builder_stays_under_its_cap_on_huge_inputs` (a 532 ms timing bound), `theseus-discord
  tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer` (it failed once in an earlier run too,
  and passes alone), `theseus-core tests_kept::*` (a `proc.run` that could not write), and `tests_standing::the_records_rebuilt_...`
  (an ordering of random ids; it passed on rerun). One store test (`an_older_stores_log_goes_on_marked...`) was mine to
  fix and is fixed. After that: `nextest -p theseus-store -p theseus-protocol -p theseus-core -E
  'chain|ledger|registry|layouts|wire|ts'` -> 2168 passed.

## The live check on this VM, and for the maintainer

A scratch daemon on the stand-in model, three turns, verify ok; stopped; one row's bytes edited in a copy (with the
frame's crc recomputed); a daemon on the copy; verify named that row:

```
ledger ok: 363 rows chained since 2026-10-12 10:07:56, head 5539a6d7… (seq 362, position 653), in 9 ms
ledger BROKEN at position 56: turn.started (seq 33), 2026-10-12 10:08:04: edited
  the row does not match its own digest, and the next row still links to that digest: its bytes were changed after it was written
  33 rows before it verified                                                   (exit 1)
```

Commands (repo root, after `cargo build -p theseusd -p theseus -p theseus-sim`; `L` is a scratch directory):

```bash
L=/tmp/live; mkdir -p $L
echo '[{"when":"hello","text":"Hello from the stand-in."}]' > $L/rules.json
cat > $L/config.toml <<'TOML'
[server]
state_dir = "/tmp/live/state"
socket = "/tmp/live/sock"
[secrets]
anthropic_api_key = "env:FAKE_ANTHROPIC_KEY"
[model]
api_base = "http://127.0.0.1:9448"
[discord]
enabled = false
[web]
enabled = false
[index]
enabled = false
[judge]
enabled = false
[memory]
mode = "off"
TOML
export FAKE_ANTHROPIC_KEY=sk-test
target/debug/theseus-sim fake-model --addr 127.0.0.1:9448 --rules $L/rules.json & M=$!
target/debug/theseusd --config $L/config.toml & D=$!; sleep 3
S="target/debug/theseus --socket $L/sock"
$S ask "hello a"; $S ask "hello b"
$S ledger verify; echo "exit $?"      # ledger ok: N rows chained since ..., head ...
$S ledger head                         # the head to keep where the log's editor can't reach
$S shutdown; wait $D
cp -r $L/state/store $L/copy-store && python3 tamper.py $L/copy-store turn.started 3     # tamper.py below
mkdir -p $L/state2 && mv $L/copy-store $L/state2/store
sed 's#/tmp/live/state#/tmp/live/state2#; s#/tmp/live/sock#/tmp/live/sock2#' $L/config.toml > $L/config2.toml
target/debug/theseusd --config $L/config2.toml & D2=$!; sleep 3
target/debug/theseus --socket $L/sock2 ledger verify; echo "exit $?"
#   ledger BROKEN at position P: turn.started (seq N), <time>: edited      exit 1
target/debug/theseus --socket $L/sock2 shutdown; kill $M
# against a head recorded earlier (truncation, or a chain rewritten from a row on):  $S ledger verify --anchor SEQ:DIGEST
```

`tamper.py` (finds the Nth ledger row of a kind in the WAL, changes one byte of its data, fixes the frame's crc; frames
are `magic u32 LE | len | crc | body`, a marked frame's crc covers the magic):

```python
import struct, sys, zlib, glob, os
store, kind = sys.argv[1], sys.argv[2]
nth = int(sys.argv[3]) if len(sys.argv) > 3 else 3
seen = 0
for seg in sorted(glob.glob(os.path.join(store, "wal", "*.seg"))):
    b = bytearray(open(seg, "rb").read())
    off = 0
    while off + 12 <= len(b):
        magic = bytes(b[off:off + 4]); blen = struct.unpack_from("<I", b, off + 4)[0]
        body0 = off + 12; end = body0 + blen
        marked = magic == b"MWHT"  # the u32 0x5448574D, little endian
        i = body0 + (8 if marked else 0)
        count = struct.unpack_from("<I", b, i)[0]; i += 4
        for _ in range(count):
            pos, k, _s, _at, kl, sl, pl = struct.unpack_from("<QHHQHHI", b, i)
            p0 = i + 28 + kl + sl
            payload = bytes(b[p0:p0 + pl])
            if k == 2 and (b'"kind":"%s"' % kind.encode()) in payload:
                seen += 1
                if seen == nth:
                    j = payload.index(b'"data":') + 8
                    new = bytearray(payload)
                    new[j] = ord('Z') if new[j] != ord('Z') else ord('Y')
                    b[p0:p0 + pl] = new
                    crc = zlib.crc32((magic if marked else b"") + bytes(b[body0:end])) & 0xffffffff
                    struct.pack_into("<I", b, off + 8, crc)
                    open(seg, "wb").write(b)
                    print(f"edited position {pos} (ledger row {seen} of kind {kind}) in {seg}")
                    sys.exit(0)
            i = p0 + pl
        off = end
print("no such row", file=sys.stderr); sys.exit(1)
```

## What is left or uncertain

- **Keel findings expected: none of mine.** `scripts/keel-guard.py` and the gate's keel phase report six erosions in
  `merge-base(HEAD, origin/main)..HEAD`, none in a file I touched, all from the base branch's own history (this VM's
  `origin/main` is behind the clone): `theseus-tui/src/tests.rs` assert-removed; `theseus-index/src/vectors.rs` and
  `theseus-protocol/src/tests_work_join.rs` allow-added; `theseus.example.toml` daily_spend_ceiling_mode cap-raised;
  `scripts/long-files.txt` ceiling-raised twice (protocol lib.rs 2726 -> 2733 and `crates/theseus/tests/golden.rs`). Their
  acks are signed commits this VM cannot verify (`%G? is E`; `keel-signers` is empty at the base), so the gate fails in
  the keel phase here before it reaches anything of mine. I raised no ceiling, removed no test, added no `#[allow]`.
- **The gate**: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` fails in **keel** (above). I ran the later
  phases by hand: `cargo fmt --check` ok; `scripts/shape.sh` ok; `cargo clippy --workspace --all-targets -D warnings`
  clean; cockpit lint (warnings only), test and build ok; `cargo deny --offline check` ok; `protocol.gen` regenerated and
  committed; the registry and the whole suite as above. Not run: the `features` phase and `scripts/build.sh` installs.
  The lifecycle, turn and cancel benches were run by hand (above), not the jobs bench.
- **This VM's disk allowance ran out several times** (stale test binaries, test cores in /tmp); that, not the chain,
  produced the odd failures above. I deleted only target/ artefacts and /tmp test directories.
- A mistake of mine: one shell block with an unquoted heredoc created an empty `/state2` directory at the root of this
  VM's filesystem (a variable was empty). It holds nothing and is not in the repository.
- `core_output.txt` is rewritten with `THESEUS_GOLDEN=write`; read its diff (every ledger line gains its chain field).
- Not done: `ledger verify` needs the daemon (an offline walk of a store directory would be a small `theseusd`
  subcommand over `chain_walk::verify`); the health head is the writer's claim, never a walk; the `ledger-pin` pass reads
  the old history once, paced, on a big store (the owner's store is the first big case: its time is not measured here).
- Docs to change at review (I did not touch the spec, status, README or design docs): the spec's Part III item (what it
  built, the 29 -> 30 bump, the two ledger kinds, `ledger.verify`, the cost numbers, the history-pin divergence from
  "a digest in the start row"), `docs/status.md`, and the killer-features plan's R25 row.
