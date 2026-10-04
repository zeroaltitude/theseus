# Cloud report: restoring a store from S3, step 16 (theseus-mgw.10)

Branch `cloud/20261004-restore-s3`, from `main` at 4bb5aac. Started 09:22 UTC and finished about 10:50 UTC, inside the
4-hour deadline. Step 16 is one commit, **01e4ed0** (`durable: theseusd restore --from s3://…, …`). This report is the
commit after it.

## What I found

- The local restore (`crates/theseus-core/src/restore.rs`) takes a WAL or store directory. I left it unchanged: the
  S3 path fetches into a directory laid out as a store's, then calls it.
- **Without a seeded cursor, the restored store's next start ships everything again.** On a fresh state dir the
  tender has no cursor. A stale cursor reads as `Rewound`, which resets to `Saved::default()`. In both cases
  `put_once` checks S3 only for an object that is *in flight*, so every segment and blob is sent again. The tender's
  doc comment says "every object is checked against S3 before it is sent", but the code doesn't do that. So step 16
  seeds the cursor rather than relying on the tender's checks.
- **The old store's `blobs.shipped` leaks into the next deployment (a step 15 problem).** `<state>/durability/`
  belongs to whichever store was there before. A restore with `--force`, or a store swapped under it, would have left
  `blobs.shipped` saying blobs were shipped that the new deployment's prefix never got. The restore now moves that
  directory aside. The same thing can happen to a local `theseusd restore --from <dir>`, which I did not change: see
  "Left open".
- **A sealed segment can grow after a restore.** The local restore appends `store.restored` to the last segment when
  it fits, even if S3 had sealed that segment whole. The seed handles this: below.
- The client holds a response whole, capped at 16 MiB, and a segment is up to 64 MiB. So a large object is read in
  ranges.

## What I changed (01e4ed0)

- **`aws/durable/read.rs`** holds the reads.
  - `Reader::get`: one `GetObject` with `ChecksumMode` up to `CHUNK` (8 MiB), S3's whole-object SHA-256 compared when
    it keeps one. Past that, `Range` reads. Every object is then checked against **its row**: length, and SHA-256 (a
    sealed segment's hex, a tail's base64, a blob's name).
  - `Reader::query`: `Query` on `pk = :pk` with `ConsistentRead`, following `LastEvaluatedKey` over every page.
  - `policy()` and `session()`: the restore session. It is the owner role with an inline policy of `s3:GetObject` on
    `bucket/durability/<dep>/*`, `s3:ListBucket` on the bucket with `s3:prefix` `durability/<dep>/*`, and
    `dynamodb:Query` on `theseus-durability` with `dynamodb:LeadingKeys` `<dep>#*`. It is named `theseus-restore`,
    with kind `Tender("restore")`. It is minted with the inline policy for **the URL's deployment**, not the config's,
    and the reads sign with it (`Signer::With`). An account with no `owner_role` is refused; the key never signs.
- **`aws/durable/fetch.rs`** holds the rows and the fetch. The rows decide what is current, never a listing.
  - Each segment comes from its sealed object when one exists; its tails are then not read at all.
  - Otherwise its tails are stitched from byte 0, each starting where the last ended (`tails.get(&at)`). Tails past
    where the join stops are recorded as `unjoined`, said, and left in S3.
  - A segment with no row, or whose first position (read from its first frame) is not the last restored position + 1,
    is a gap. So is a tail-cut segment that a later segment follows. The restore stops at the gap, says so, and never
    fills it.
  - A blob S3 lacks is said (`blobs_missing`). Any object whose bytes differ from its row refuses the whole restore.
- **`aws/durable/restore.rs`** holds the orchestration and the seed.
  - `from_s3` checks in this order, before any request: the URL, the socket (refused while a daemon answers), the
    account (the config's whose `theseus-<id>-<region>` is the URL's bucket, with `durability = true`), and an
    occupied store without `--force`.
  - Then: the session, the rows, a fetch into `<state>/store.from-s3-<ms>/`, and the local restore. The staging copy
    is removed whether it succeeds or fails, since it can be fetched again.
  - After the restore, `<state>/durability/` is moved to `durability.before-restore-<ms>`.
  - **The seed**, only when the config's deployment is the restored one. The cursor goes just past the last frame at
    or before the restored `last_position`, i.e. before `store.restored`, found with the follower. `rows_to` is that
    position, and `blobs.shipped` gets every restored blob.
  - The cursor's segment stays open to tails (`sealed_to = seg - 1`, `tail = (seg, offset)`). The exception is when
    `store.restored` rotated into a new segment and S3 already holds this one whole (`sealed_to = seg`). So a
    "sealed" segment that grew after the restore is shipped again whole when it seals, never lost.
  - The lines say where each segment came from ("segment 3 from its object, segment 4 from 7 tail(s)"), plus
    unjoined tails, the gap, missing blobs, and unread rows. With the same deployment they warn: "a daemon started on
    this store ships into s3://…/". With another deployment they say it ships whole into its own prefix.
- **`theseusd`** (`main.rs`):
  - An `s3://` `--from` resolves the secrets board (`resolve_into`), builds `Aws::from_config`, and calls `from_s3`.
  - `--repair` with an `s3://` URL is refused.
  - The local report's lines are now `restore_lines()`, shared by both paths with the same output as before.
  - The help text names the new form.
- **`aws/s3.rs`**: `bytes_of` became `pub(crate)` so the reads can reuse it.
- **`aws/tests_durable.rs`**: the fake's items are `pub(super)` for the new test file. It gains `GetObject` (ranges
  answered 206 with `Content-Range`, and `x-amz-checksum-sha256` under checksum mode) and a paged `Query` (`Limit`,
  `ExclusiveStartKey`, and `LastEvaluatedKey` whenever a page is full, as DynamoDB does).

The step adds no stored field and no record kind, so `MANIFEST_FORMAT` is unchanged. It adds no dependency, no
protocol type, and no config key. No file passes its line ceiling (`main.rs` is 1,355 lines).

## How I proved it

`aws/tests_restore.rs` has 7 tests. Each ships a store with the real tender (`Shipper::pass`) through the fake, then
restores it with `from_s3`: queries of 3 rows to a page, GETs of 100 bytes to a range.

1. `a_shipped_store_restores_from_s3_equal_to_the_original`: sealed segments come from their objects, read in ranges,
   and the open segment from its tails. The restored WAL's records equal the original's, with `store.restored` after
   them. The last position, the session ids and labels (17), and the blob's bytes are all equal. More than 2 Query
   pages were followed, and the staging copy is gone.
2. `the_restored_stores_next_start_ships_nothing_again`: a tender on the restored store sends **exactly one new
   object** (the `store.restored` frame's tail). No key is sent again, and Put plus UploadPart grows by 1. A second
   restore then holds both `store.restored` rows: the join goes on.
3. `another_deployment_seeds_no_cursor_and_keeps_the_old_one_aside`: nothing is seeded, the old `cursor.json` is kept
   aside byte for byte, and the lines say so.
4. `a_sealed_object_is_preferred_and_a_stale_tail_is_not_stitched`: segment 1 was shipped as 3+ tails, then sealed
   whole. Its tails are never fetched. The open segment then shipped tails a, b (long), and c. Its log was cut back to
   after a (a power loss), then b' (short) was written and shipped. c's stale tail is not stitched and is said. The
   restored segment equals the live one, and holds b' but not c.
5. `a_corrupted_object_is_refused_by_its_checksum`: a blob was replaced in S3 with bytes of **the same length**, and
   S3's own checksum updated to match, so only the row's digest can tell. The restore is refused, naming the key and
   "SHA-256". No store and no staging are left. A sealed segment's byte flipped is refused the same way.
6. `a_missing_blob_and_a_gap_are_said`: the blob's absence is said. Segment 2's row was removed, so the restore stops
   at it: only segment 1 is restored, with the gap said.
7. `the_restore_session_only_reads_and_nothing_is_fetched_while_a_daemon_serves`: the policy's actions are exactly
   `s3:GetObject`, `s3:ListBucket`, and `dynamodb:Query`, with the prefix resource and the leading-keys condition.
   While a listener holds the socket the restore is refused and the fake sees **zero** requests. Once stopped, every
   request is AssumeRole, GetCallerIdentity, GetObject, or Query. One AssumeRole names `theseus-restore`, and its
   body names no PutObject, BatchWriteItem, or Delete.

Runs:
- `cargo nextest run --workspace -E 'test(tests_restore) | test(tests_durable)'`: 15/15 (7 new, 8 step 15).
- **Under load**, with 4 busy loops at nice 0 and the tests at `nice -n 19`: 5 runs plus 1 earlier, each 15/15
  passed. The loops were killed by their pids.

**Planted reverts.** Each file was restored from a copy, `touch`ed, and checked with `git status`. All 7 pass again
afterwards.
- **No SHA-256 check** (`|| !want.matches(&raw)` removed from `Reader::get`):
  `a_corrupted_object_is_refused_by_its_checksum` fails. `from_s3` returned `Ok`, restoring the forged blob. The first
  plant only failed on the length check, because my forged blob was a different size. I made it the same length, so
  the test now proves the digest itself.
- **Tails stitched without the contiguity check** (`tails.get(&at)` changed to `tails.range(at..).next()`):
  `a_sealed_object_is_preferred_and_a_stale_tail_is_not_stitched` fails. With the "said" assert disabled for a second
  plant, it fails on the substance: the restored sessions include **"c, from a log that will be rewound"**. The stale
  frame stitched in passes every frame and position check of the local open. Only the join rule keeps it out.

`cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` are clean (the first gate run caught
one `question_mark` lint, now fixed).

## Live check (the maintainer's, on the owner's account, after the owner's go)

Use a config whose AWS account's table has `owner_role`, `durability = true`, and a `deployment` of its own, say
`deployment = "theseus-restore-probe"`. Call that config `probe.toml`, and name the account's table rather than its
id.

```sh
# 1. Ship a few turns (step 15's live check), then stop.
S=$(mktemp -d); theseusd --config probe.toml --state-dir $S/a --socket $S/a.sock &
theseus --socket $S/a.sock ask "say hi"; theseus --socket $S/a.sock ask "say bye"
theseus --socket $S/a.sock health          # the account's durability line: caught_up
theseus --socket $S/a.sock sessions list   # note the sessions; and `history` for each
theseus --socket $S/a.sock shutdown
# 2. Restore into a fresh state dir.
theseusd --config probe.toml --state-dir $S/b --socket $S/b.sock \
  restore --from s3://theseus-<account>-<region>/durability/theseus-restore-probe/
```

The restore should print:
- `fetched s3://… (account …, in the session theseus-restore): N segment(s), M blob(s), … bytes`, then
  `segment 1 from 1 tail(s)` or similar (a small store has only its open segment);
- the local restore's lines;
- **the warning** "the config's deployment is theseus-restore-probe, the one restored: a daemon started on this store
  ships into s3://… (its cursor is seeded at position P …)", since the probe config is the shipping one.

```sh
# 3. A daemon on the restored state.
theseusd --config probe.toml --state-dir $S/b --socket $S/b.sock &
theseus --socket $S/b.sock sessions list   # the same sessions; `history <id>` the same transcripts
theseus --socket $S/b.sock ledger | tail   # store.restored is there
theseus --socket $S/b.sock health          # durability: caught_up, after shipping one tail (the store.restored frame)
theseus --socket $S/b.sock shutdown
```

Step 3 also proves the seed live: the bucket under the prefix gains exactly one new `wal/<n>.seg.tail/…` object, and
no segment or blob is sent again.

4. In CloudTrail (or `theseus aws trail`), the session `theseus-restore` (source identity `theseus-restore-probe`)
   shows only `AssumeRole` (from the key), `GetObject`, and `Query`, with no write. It costs under a cent: a few GETs,
   a few Query pages, a few MB out.

## Left open, and choices the owner should hear about

- **The residual stale-tail case.** Contiguity is the guard the brief chose. It misses one case: a rewritten frame
  that ends at **exactly** the byte where an old-timeline tail begins. Then the old frame stitches in and its
  positions check, as the planted revert shows. The rows carry no timeline mark. A cheap extra guard would be to
  refuse a tail whose S3 `LastModified` is older than its predecessor's: the tender ships tails in order, and a
  rewind takes a reboot. I did not add it; it needs the fake's objects timed. A stronger one is an epoch in the tail
  rows (the cursor's start, or a nonce per tender start), which changes step 15's rows.
- **A corrupt sealed object refuses the restore**, even when the segment's tails might still tile it. A fallback to
  the tails (when they join to the row's `bytes`) would be small, if wanted.
- **A gap restores the prefix before it**, said in the output, rather than refusing. The `store.restored` row does not
  record the gap, since I kept the local restore unchanged. Its `from` names the staging path,
  `<state>/store.from-s3-<ms>/wal`, not the URL. If the row should carry the URL and the gap, the local restore needs
  an extra-fields argument.
- **The restore session's policy covers the URL's deployment**, which may differ from the config's. Restoring
  production's prefix into a scratch config with its own deployment is the safe way to do this. Its daemon then ships
  into its own prefix, and nothing is seeded.
- **A local `theseusd restore --from <dir>` still leaves `<state>/durability/` in place.** A stale cursor resets on
  `Rewound`, but its `blobs.shipped` still skips blobs. A follow-up should move it aside there too.
- **Docs to update** (the maintainer's):
  - `docs/design/aws-toolset.md` §5 "Step 16": the command is `theseusd restore --from s3://…` (not `theseus
    restore`); it signs in `theseus-restore` (not a tender session); it reads the rows, not a listing.
  - `crates/theseus-core/AGENTS.md`'s AWS bullet should name `durable/read.rs`, `fetch.rs`, and `restore.rs`, plus
    `tests_restore.rs`.
  - The `durable.rs` module comment "every object is checked against S3 before it is sent" overstates `put_once`.
  - `docs/status.md`: step 16 landed.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (second run, after the clippy fix): fmt, shape, features, clippy, cockpit,
test build, and reader rule all passed. **The suite failed with 2014 run, 1980 passed, 34 failed, 17 skipped.** Every
failure is outside this change:

- **33 sandbox tests** fail because the VM runs as root (theseus-pv6i). All 21 failing tests in `theseus-sandbox`
  (`bench spawn_100` and every `contract` clause) and the 12 failing tests in `theseusd::sandbox` (e.g.
  `a_cancel_of_an_l1_job_is_verified_by_its_pid_namespace`, `a_host_beyond_the_list_once_approved_is_outside_text`)
  say "the daemon runs as root, and Linux exempts root from RLIMIT_NPROC".
- **`theseus-core tests_output::the_cores_output_matches_its_golden`** fails on the timezone. A `wake.at` preview's
  UTC offset prints `+00:00` on this UTC VM, and the golden holds `-#:#`. It passes under `TZ=America/Los_Angeles`
  (1/1). This is not the change's, and is worth a ticket: the golden depends on the machine's timezone.

The phases after the suite, run by hand, pass. Protocol types: `cockpit/src/protocol.gen` unchanged. Deny: advisories,
bans, licenses, and sources all ok. The benches were skipped (`NO_BENCH`), as the brief says.
