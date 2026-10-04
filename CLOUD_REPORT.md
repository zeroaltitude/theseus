# Cloud report: the durability tender, step 15 (theseus-mgw.7)

Branch `cloud/20261004-durability-tender`, from `main` at `d9b0931`. Not for `main`: the maintainer drops this commit at the merge.

## Step 15: the durability tender

### What I found

- `theseus-follow` was already built with this tender in mind: its `Batch` names the byte spans read (`spans`) and the
  segments a read sealed (`sealed`). It did not say which span holds which record. The index rows need that.
- The segments are 64 MiB, so shipping only sealed segments would leave hours of exposure, not 5–60 s. The tender
  also ships the open segment's new whole frames as byte-range "tails".
- The foundation stack makes the bucket (`theseus-<account>-<region>`, versioned, SSE-S3) and the table
  (`theseus-durability`, keys `pk`/`sk`, on demand). It defines no tender role: a tender's narrowing is the inline
  session policy on `theseus-owner`, as C2's budget tender does.
- `theseusd` stops with `rt.shutdown_timeout(500 ms)`, which waits for the blocking pool. An inotify wait in
  `spawn_blocking` would have held a clean stop for up to 500 ms. I caught this in review and moved the watch to a
  thread of its own (below).
- The C1 fake (`aws/tests.rs`) keeps request bodies as lossy UTF-8, so it can't check binary bodies. The tests use a
  stateful fake of their own in `aws/tests_durable.rs`, built the same way: S3 objects and multipart uploads, each
  checksum checked as S3 checks it; DynamoDB items, with unprocessed items when a test asks for them; and STS.

### Where it runs, and why

It runs in the daemon, as a task the socket daemon starts after serving
(`Core::tend_durability_after_serving`, called beside the index tender in `after_serving`). It reads the WAL through
the index lane's follower (`theseus-follow`). It does not run inside `theseus-index`.

I read the design's "on the index lane's WAL follower" as naming the follower crate, not the index tender's process.
The tender signs in a role session, and §3.5 keeps AWS credentials in the core: no child or program holds them. To
run inside `theseus-index`, the core would have to hand that process session credentials, and a second path would
then exist for them to leak. The follower is a read-only library, so it serves both tenders. The tender holds the
core by `Weak` and ends once the core is gone.

### What it does

- **Objects**, under `s3://theseus-<account>-<region>/durability/<deployment>/`:
  - `wal/<n:09>.seg`: a sealed segment, whole. One `PutObject` up to 8 MiB; past that, a multipart upload in
    8 MiB parts, each part with its SHA-256 and the upload created with `ChecksumAlgorithm=SHA256`.
  - `wal/<n:09>.seg.tail/<from:012>-<to:012>`: the open segment's new whole frames, a byte range of the file. The
    tails tile the segment. A segment is never sent as tails once it is sealed, or once a later segment exists
    (backfill).
  - `blobs/<sha256>`: each new blob. A file whose bytes don't hash to its name is left and logged.
  - Every object carries `ChecksumSHA256`, which S3 verifies. Bytes go exactly as the WAL holds them, read only
    through the follower, so the frame-format change in flight ships unchanged.
- **Index rows** in `theseus-durability`. Every partition key starts with the deployment:
  - `<dep>#wal` / `<n:09>`: a sealed segment's key, bytes, sha256 (hex), last position, and parts.
  - `<dep>#wal` / `<n:09>.tail.<from:012>`: a tail's key, its range, last position, and sha256.
  - `<dep>#blob` / `<sha256>`: a blob's key and bytes.
  - `<dep>#rec#<kind>` / `<key>`: each keyed record's latest position, `at_unix_ms`, segment, and scope. This is the
    redb index's "latest by key" as rows: a restore finds a node's segment from it.

  Rows go in `BatchWriteItem`s of at most 25. A batch never names one key twice: the latest stands. Unprocessed
  items are sent again with a doubling backoff (50 ms first, 8 tries). Past the tries the pass fails, keeps its
  rows, and the next pass writes them.
- **The durable cursor**, `<state>/durability/cursor.json`, written whole, synced, renamed, and its directory
  synced. It holds:
  - the follower's cursor that everything before it is shipped to;
  - the last sealed segment shipped (`sealed_to`) and how far the open one's tails reach;
  - the last position whose rows are written;
  - the objects in flight, keyed by object key: each is saved before its first request, and a multipart upload's
    id and parts are saved as S3 takes each part;
  - the rows of shipped objects not yet written.

  A restart that meets an in-flight object asks S3 before sending: `HeadObject` with `ChecksumMode=ENABLED` for a
  put; `ListParts` for an upload, adopting the parts whose checksums match the file. An upload S3 completed before
  the cursor knew is recognised by its composite checksum. Blobs shipped are listed in `blobs.shipped`. A WAL the
  cursor no longer matches (`FollowError::Rewound`, after a restore) starts again from the log's start, and S3's
  checksums say what is there.
- **The session**: `theseus-durability`, `Kind::Tender("durability")`, with this inline policy (`durable::policy`):
  - `s3:PutObject`, `s3:GetObject`, `s3:ListMultipartUploadParts`, and `s3:AbortMultipartUpload` on
    `arn:aws:s3:::<bucket>/durability/<deployment>/*`;
  - `dynamodb:BatchWriteItem` on the table, with the condition `ForAllValues:StringLike dynamodb:LeadingKeys
    ["<deployment>#*"]`.

  It allows no delete.
- **When it runs**: 2 s after serving (`START_AFTER`), then after the account's check settles. After that:
  - each WAL change wakes it (inotify through `theseus_follow::Waker`, on its own `durability-watch` thread, so a stop
    never waits for it);
  - it waits 5 s (`SETTLE`) so the writes around the change ride along, then ships;
  - a pass also runs every 60 s as a backstop;
  - a failed pass is tried again after a backoff that doubles from 5 s to 60 s.

  Disk reads go through `theseus_store::blocking`.
- **The config**: `[aws.accounts.<id>] durability = true`, false by default. The loader refuses it without
  `owner_role`, or on two accounts. The template has the line.
- **Health**: `aws.accounts[i].durability` (`AwsDurabilityStatus`, in `theseus-protocol/src/aws.rs`) holds:
  - `state`: `waiting`, `shipping`, `caught_up`, `failing`, or `stopped`;
  - `bucket`, `prefix`, and `table`;
  - `oldest_unshipped_unix_ms` (the oldest unshipped committed record's commit time, from the batch's first record
    or the wake) and `lag_ms` (its age when health is asked);
  - `shipped_to_position` and `last_shipped_unix_ms`;
  - counts since the start: `segments`, `tails`, `blobs`, `rows`, and `bytes`;
  - `error`.
- **The ledger**: one `durability.shipped` row per sealed segment (fact `fact::durability::SegmentShipped`), with the
  segment, key, bytes, sha256, last position, parts, and `took_ms`. Tails, blobs, and rows are counts in health and
  telemetry instead of rows: a row each would be a frame each, every few seconds, and each frame would be shipped
  again. A segment is 64 MiB, so its rows are few.
- **Telemetry**: two new instruments.
  - `theseus.durability.shipped` (IntSum): bytes, or rows, by `theseus.durability.object`, one of `segment`, `tail`,
    `blob`, or `rows`.
  - `theseus.durability.lag_ms` (Histogram): the oldest unshipped record's age each time the tender catches up. Its
    `max` per interval is the alarm's input.

  There is no gauge kind in the in-house OTLP encoder. The live `oldest_unshipped` is health's.

### Commits

- `d5b7340` aws: the durability tender ships the store to S3 and DynamoDB (theseus-mgw.7)
- `04573f6` aws: a tail shipped before a restart is not shipped again in part (theseus-mgw.7)

### How I proved it

Tests (`crates/theseus-core/src/aws/tests_durable.rs`, unless named otherwise):

- `a_sealed_segment_is_shipped_once_with_its_checksum`: 30 records in 400-byte segments (5 segments). It checks
  that:
  - every sealed segment's object equals its file;
  - the open segment's tails join to its file;
  - every body went with its matching `x-amz-checksum-sha256`;
  - segments over 150 bytes went in 128-byte parts;
  - every key was sent exactly once;
  - each segment's row names its SHA-256, and a key's row has its latest position and segment;
  - there is one `durability.shipped` row per sealed segment;
  - a second pass sends no request at all;
  - more writes ship without resending anything.
- `a_restart_mid_upload_resumes_without_a_duplicate_or_a_gap`: a crash after S3 took part 2, before the cursor
  saved it. The restart does one `ListParts`, sends no part twice, and completes the upload once, and every
  segment matches its file. A second case is a crash just after a whole put: the restart's `HeadObject` finds the
  object by its checksum and sends nothing again.
- `a_tail_shipped_before_a_crash_is_not_shipped_again_in_part` (the second commit): a crash after a tail's put and
  save, before its batch's rows, with more frames written before the restart. The next tail starts where the
  shipped one ended, with no overlap, and the pending rows are written.
- `a_blob_is_shipped_once`: a blob, then a second one, then a restart. Each was put once, with its row. A file not
  named by its hash is left.
- `index_rows_batch_and_retry_their_unprocessed_items`: 60 keyed records, with the fake leaving 2 items unprocessed
  twice. Every row is written, no request carries over 25 puts, and the unprocessed items are re-sent. When the
  fake keeps refusing, the pass fails with "unprocessed" and the state goes to `failing`; the next pass writes the
  rows.
- `a_refusal_is_said_in_health_and_the_next_pass_ships`: S3 refusing `PutObject` (403) gives health's line `failing`
  with `AccessDenied` and an `oldest_unshipped`. Once S3 answers, the line reads `caught_up` with lag 0.
- `the_tender_session_is_narrowed_to_its_prefix_and_its_rows`: the policy's resources, its condition, and no delete.
- `the_tender_starts_after_its_wait_and_ships_what_the_wal_gains`: `run` on the real clock. No request reaches the
  fake in the first 500 ms (health says `waiting`), then the store ships, and a later write is woken and shipped.
- `durable::cursor::tests`, `durable::rows::tests`: the cursor's round trip and layout check; record rows (the latest
  per key, its segment, nothing at or before `rows_to`).
- `config::aws::tests`: `durability` defaults off, loads with `owner_role`, and is refused without it.
- `theseus-follow`'s tests: `ends` (one per span, rising, the last one the batch's last position).
- `crates/theseusd/tests/aws.rs::the_durability_tender_waits_for_serving_and_says_so`: a real daemon with the account
  at a fake endpoint, `owner_role`, and `durability = true`. The tender's line says `waiting`, and no request with
  the `theseus-durability` session name reaches AWS before the socket answers; its first request (AssumeRole, with
  the `ShipToItsPrefix` policy) comes after.
- The lifecycle bench's config (`theseus-sim/src/lifecycle.rs`) now names `owner_role` and `durability = true` for its
  unreachable account, so the bench's start, stop, and swap shapes include the tender. I ran it once
  here for the shape only, after the second commit: `theseus-sim bench lifecycle --runs 3` gave `LIFECYCLE OK`, with
  a cold start p95 of 12.8 ms, a clean shutdown of 7.5 ms, a restart of 13.7 ms, and a swap of 15.4 ms. Those
  numbers come from this 4-core VM; the maintainer measures the budgets.

Runs:

- Planted revert 1, **the durable cursor dropped**: `cursor::save` became a no-op. The restart test failed at its
  `CreateMultipartUpload` count (`left: 2, right: 1`: the upload started over). I restored the file, touched it,
  checked `git status`, and the test passed again.
- Planted revert 2, **the restart's ListParts adoption dropped** (`listed.into_iter().take(0)`): the test failed with
  "a part sent twice: [("up1","1"), ("up1","2"), ("up1","3"), ("up1","4")] left: 4, right: 5". Restored and touched;
  passes.
- Planted revert 3, **the tail clamp missing**: I ran the new test against the first commit's `durable.rs`. It failed
  with "a gap or an overlap at …/000000000000-000000000312, left: 0, right: 156". The fix restored it; passes.
- Under load, with four `while :; do :; done` loops at nice 0 and the tests at `nice -n 19`:
  - the 10 durability tests in theseus-core passed 10 of 10 runs (100 of 100), each run 7 to 14 s;
  - `theseusd --test aws` passed 3 of 3.
- The gate: below.

### The live check (the maintainer's, on the owner's account)

Use a scratch daemon with its own state dir, socket, and config, never the operator's. Give it a `deployment` of its
own so its prefix and rows can't touch the operator's. Below, `<id>` is the bound account, `<region>` the foundation's
region, and `$S` the scratch socket.

1. In the scratch config's account table (beside its `credentials`, `region`, and `owner_role = "theseus-owner"`):

   ```toml
   deployment = "theseus-scratch-15"
   durability = true
   ```

2. Start it: `theseusd --config <scratch.toml> --state-dir <scratch-state> --socket $S &`. Then, within 10 s:
   `theseus --socket $S --json health | jq '.aws.accounts[0].durability'`.
   This should show `state` going from `waiting` to `caught_up`, `prefix` "durability/theseus-scratch-15/", `bucket`
   "theseus-<id>-<region>", and `segments`/`tails` counting.
3. Write something: `theseus --socket $S ask "say ok"`. Within 5 to 60 s, health shows
   `oldest_unshipped_unix_ms` set and then cleared, `lag_ms` back to 0, and `tails` up by one or more.
4. In the bucket (from the operator's own shell, read-only):
   `aws s3 ls --recursive s3://theseus-<id>-<region>/durability/theseus-scratch-15/`.
   It should list `wal/000000001.seg.tail/…` objects (and `blobs/…` after an image). Their `LastModified` times
   should be within a minute of each write.
   `aws s3api head-object --bucket theseus-<id>-<region> --key <a tail key> --checksum-mode ENABLED` shows its
   `ChecksumSHA256`.
5. In the table: `aws dynamodb query --table-name theseus-durability --key-condition-expression 'pk = :p'
   --expression-attribute-values '{":p":{"S":"theseus-scratch-15#wal"}}'` lists one item per tail or segment.
   `":p":{"S":"theseus-scratch-15#rec#session"}` lists the sessions' latest positions.
6. In CloudTrail, the session is `assumed-role/theseus-owner/theseus-durability`. A denial would name the session
   policy.
7. Telemetry, if the scratch daemon exports it: `theseus.durability.shipped` and `theseus.durability.lag_ms`.
8. Stop with `theseus --socket $S shutdown`. Restart, and check that health's `segments`, `tails`, and `blobs` stay
   0 until something new is written (nothing is shipped again).
9. Optional: sealing a segment takes 64 MiB of WAL, so to see a sealed segment ship and its `durability.shipped` row
   (`theseus --socket $S ledger -k durability.shipped`), use a scratch store copied from a store that has more than
   one segment.

### What is left, or uncertain, and design choices for the owner

- **Tails of frames not yet synced.** The follower reads whole frames from the page cache, so a tail may hold a frame
  the machine never synced. If power is lost, the local log is cut there and rewritten differently; the next start
  sees `Rewound` and ships again from the start, but the stale tail objects (and their rows) beyond the new log stay
  in S3. Step 16's restore must verify frames (they carry crcs) and prefer a sealed segment's object. The in-flight
  change that puts the last synced position in each frame would let the tender ship only synced frames. That's a
  small follow-up once it lands.
- **Tails are kept after their segment is sealed.** The session has no delete, on purpose. A bucket lifecycle rule
  on `durability/*/wal/*.seg.tail/` (say 7 days) in the foundation stack would clean them up; that's
  `infra/aws/`'s, not this step's.
- **First start on an existing store** ships the whole history as backfill: every segment, every blob, and a row for
  every keyed record (one `BatchWriteItem` per 25). On-demand writes cost about $1.25 per million.
- **No gauge** in the in-house OTLP encoder. If the alarm wants a true gauge of `oldest_unshipped`, add a gauge kind
  to `telemetry/otlp.rs` and `metrics.rs`.
- **Health's CLI and UI lines are not drawn** (UIs were out of scope): the field is in the health JSON and the
  generated TypeScript (`AwsDurabilityStatus.ts`).
- Shared files I touched, each in a small way:
  - `crates/theseusd/src/main.rs`: one call in `after_serving`.
  - `crates/theseus-protocol/src/lib.rs`: one field on `AwsAccountStatus` (2,566 to 2,570 lines; ceiling 2,613).
  - `crates/theseus-protocol/src/ledger.rs`: one kind.
  - `crates/theseus-core/src/aws/mod.rs`: `Tended.durability`, the inline policy's lookup, and the `mod` lines.
  - `config/aws.rs`, plus `durability: false` in two test literals.
  - `fact/mod.rs`, `telemetry.rs`, `telemetry/metrics.rs`.
  - `crates/theseus-sim/src/lifecycle.rs` (the bench's config, which the benchmarks change also edits).
  - `web/src/protocol.gen/`: regenerated. The cockpit change moves it, so regenerate it at the merge.
  - `theseus-follow`'s `Batch.ends`: index lane's crate, three lines.
- No store format change: the cursor lives outside the store, and a new ledger kind is not a new field. No new
  package: `Cargo.lock` gains only theseus-core's edge to theseus-follow.
- **Docs to write at review**: the spec's §6 "Tenders" paragraph (the durability tender, as built), Part III's item,
  `docs/status.md`, and theseus-core's `AGENTS.md` (`aws/durable.rs` and `tests_durable.rs` under the AWS bullet).
  `theseus-follow/AGENTS.md` already names the durability tender as a reader.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` ran on each commit's tree. Both runs ended `FAILED in suite`, with the
same 33 failures and nothing else: 1,729 of 1,762 tests passed and 10 were skipped. The 33:

- **31 sandbox tests** (`theseus-sandbox::contract` clauses 01 to 12, its egress, scratch, signal, and exit cases, and
  14 of `theseusd::sandbox`), plus **`theseus-sandbox::bench spawn_100`**. Each says "the daemon runs as root, and
  Linux exempts root from RLIMIT_NPROC" (theseus-pv6i: this VM runs everything as root).
- **`theseus-core tests_output::the_cores_output_matches_its_golden`**: a wake's line prints the local UTC offset,
  `+00:00` on this UTC VM, where the golden has `-07:00`. With `TZ=America/Los_Angeles` it passes. The difference is
  the VM's timezone, not this change.

The phases after the suite I ran myself, on each commit's tree, and all passed:
- the reader rule, which passed inside the gate;
- the protocol types: `web/src/protocol.gen` matches the commit;
- the turn bench (`--runs 5 --burst 0`): `frames_plain` 5 at the p95, budget 5;
- `cargo deny --offline check`: advisories, bans, licences, and sources all ok (`cargo deny fetch` succeeded at
  setup);
- the web app's lint and build, and the cockpit's lint, test, and build;
- the web dist: unchanged.

fmt, shape, clippy (`-D warnings`), the bench build, and the test build passed inside the gate. The lifecycle and jobs
benches were skipped, as `THESEUS_GATE_NO_BENCH` does.
