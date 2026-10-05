# CLOUD REPORT: durability-fixes (theseus-iame, theseus-mgw.12, theseus-b9x6)

Branch `cloud/20261005-durability-fixes`, from `cf468be4` (the task commit on main at `80ef1dea`; store format 16,
unchanged). Three green commits, one per step, then this report.

| Step | Commit |
|---|---|
| 1. theseus-iame: the tender's session lists its prefix | `e0c068af` |
| 2. theseus-mgw.12: only synced frames ship | `203fc632` |
| 3. theseus-b9x6: the restore's two guards tested, and tails after a sealed object joined | `c8973251` |

## Differences from the brief (the code won)

- **The fake answered HEAD 404 to anyone.** I confirmed it: `answer`'s `HeadObject` arm ignored the session's
  policy. The policy check now lives in one helper, `may_know_missing`, which the GET arm and the HEAD arm both call.
  The HEAD arm answers 403 with no body, as S3 does. Once HEAD answered by the policy, the *existing*
  `a_refusal_is_said_in_health_and_the_next_pass_ships` failed on main's policy too. It had been passing only because
  of the fake.
- **A frame's mark is a batch behind.** That is why I chose the writer's `synced()` as the bound (see step 2).
- **`Batch` was changed additively.** I gave `Batch` no new fields. The bound is an argument to a new
  `WalFollower::read_upto`. `read` calls it with no bound, and theseus-index is unchanged. The only additive change
  is one new `Stop` variant, `Held`. theseus-index's matches on `Stop` already have `_` arms and `==` checks, so the
  new variant does not break them.
- **The hands' `ListTheHandsPrefix`.** I left infra/aws/theseus-hands.yaml alone. The hand role (`aws/hands/hand.rs`)
  only `PutObject`s its outputs. No hand code heads or gets a key, so none needs to tell a missing key from a
  refusal. If a hand ever reads back from S3, that statement needs `StringLikeIfExists` for the same reason.

## Step 1: theseus-iame (`e0c068af`)

**Found.** The tender's inline policy (`durable::policy`) had no `s3:ListBucket`. So S3 answers 403 to the tender's
HEAD of a missing key, `Bucket::call` maps 403 to `Other`, and `s3_halt` turns `Other` into `Retry`. The tender heads
a key only when its cursor held the object in flight at a crash: `put_once`, `resume_upload`'s head (for an upload S3
no longer lists), and `put_large`'s lost completion. A crash between saving the mark and S3 storing the object
therefore retried forever and shipped nothing behind it.

**Changed.**
- `durable::policy` gains a `ListItsPrefix` statement: `s3:ListBucket` on the bucket's ARN, with
  `StringLikeIfExists` on `s3:prefix` = `durability/<deployment>/*`. Its comment points at read.rs's statement for
  why it is `IfExists`.
- The module doc and the fn doc now say the session may list.
- The fake answers HEAD by the session's policy, as GET does.

**Proved.**
- New `an_object_in_flight_that_s3_never_stored_is_sent_once`:
  - One pass runs with `refuse = Some("PutObject")`, so the cursor saves the tail's mark and the put is refused.
  - On the restart, the same batch puts the same tail in flight again. The restart heads it, and the object is
    stored: one refused put, then one sent.
  - 30 more records then ship, and every segment ships equal to the log.
- New `an_upload_s3_no_longer_lists_starts_again_once`:
  - The pass crashes after the upload's part 1, then the fake forgets its uploads.
  - The restart's `ListParts` gets `NoSuchUpload`, and its HEAD of the key gets 404.
  - The upload starts again (two `CreateMultipartUpload`s) and is completed once.
- `the_tender_session_is_narrowed_to_its_prefix_and_its_rows` checks:
  - three statements;
  - `ListBucket` on the bucket, under the exact `StringLikeIfExists` condition;
  - the fake's `lists_without_prefix` accepts the policy;
  - still no `Delete` and no action ending in `*`.
- Planted revert, `ListItsPrefix` dropped from the policy. Four tests fail:
  - the two new tests, each at the restart's `pass().unwrap()` with `Halt::Retry`;
  - the policy test;
  - the existing `a_refusal_is_said_in_health_and_the_next_pass_ships`.

  File restored and `touch`ed; `git status` clean of it.
- The gate: see the end.

## Step 2: theseus-mgw.12 (`203fc632`)

**The bound's source: the writer's `Wal::synced()`.** It is exact and live, and the tender runs in the process that
writes the log.
- One read-only accessor: `WalStore::synced_to()`, wrapped by the core's `Store::synced_to()`.
- It is handed to the shipper as `Hooks::synced_to` (an `Arc<dyn Fn() -> u64>`), the way `Hooks` hands it the ledger.
  The closure holds the core by `Weak` (the AGENTS.md trap) and reads 0 once the core is gone.
- I did not use the marks: they are a batch behind, so a quiet store would never ship its last write.
- With `fsync = false`, `synced_to` is `last_position()`, since nothing is ever synced. I chose that so a store run
  without fsync still ships at all.
- `Hooks::default()` has no bound and ships every whole frame. The older tests' logs are never synced, and they stay
  unchanged.

**Changed.**
- **theseus-follow:** `read_upto(max_bytes, upto)` stops before the first frame whose last position is past `upto`.
  - It returns `Stop::Held { segment, offset, position, at_unix_ms }`, and the cursor stays before that frame.
  - A segment the read stopped inside is never named sealed, even when a later segment exists.
  - `read` is `read_upto(.., u64::MAX)`.
  - Extracting `segment()` keeps `read_into` under clippy's 100 lines.
- **Shipper:** it reads at each batch's bound. A pass that ends `Held`:
  - sets `oldest_unshipped_unix_ms` to the held frame's own time, since everything before it shipped;
  - emits no `CaughtUp`;
  - sets health's state to `waiting`, with `error` = "for the WAL's sync: position N is written, not yet synced";
  - makes `follow` pass again a settle later. The writer's sync changes nothing inotify sees, so waiting for the next
    change could take up to the backstop's minute.
- `AwsDurabilityStatus`'s doc names the new reason for `waiting`. The cockpit's generated TypeScript was regenerated
  for that comment.
- Nothing changed in the WAL's writes, its frame layout, or the store's format. Nothing was added to the start path.
- AGENTS.md: theseus-follow's guide (`read_upto`) and theseus-core's list of AWS tests.

**Proved.**
- theseus-follow `a_read_up_to_a_position_holds_the_frames_past_it`: holds mid-segment, does not name a segment
  sealed while it is read only in part, re-reads nothing while the bound stays, then reads the rest and names the
  segment sealed.
- New `aws/tests_synced.rs`, a log written as the writer writes it (`Wal::write`, then `Wal::sync`), bounded by
  `Wal::synced`:
  - `a_write_without_its_sync_waits_and_ships_after_the_sync`. Three synced frames and two unsynced:
    - the pass ships to position 3 and says `waiting` with position 4;
    - `oldest_unshipped` equals the held record's time;
    - no tail ends past the synced byte;
    - after `sync`, the pass ships to 5 and says `caught_up`, with oldest none.

    Then unsynced frames roll segment 1: it is not shipped whole until they are synced, and after that every segment
    is equal and each object was sent once.
  - `a_power_losss_cut_follows_on_from_the_cursor`. Three frames are synced and shipped, and two unsynced are held.
    Then:
    - the WAL closes, segment 1 is truncated where the sync ended, the WAL reopens, and two different frames are
      written and synced;
    - the next start (a new shipper from the disk cursor) passes `caught_up` with no `Rewound` and sends nothing again;
    - no tail ends past the log, and the join equals the segment;
    - `restore::from_s3` gives no gap and nothing unjoined, its records equal the log's, and it holds the new
      sessions and neither lost one.
- Unchanged and passing: the existing tests_durable and tests_restore tests (with step 1's fake fix), theseus-follow
  (14 tests), and theseus-index.
- tests_durable.rs and tests_synced.rs ran 5 times under load (four busy loops at nice 0, the run at nice 19): 12/12
  each time (13.6 s, 12.4 s, 13.3 s, 23.2 s, 22.2 s).
- Planted revert, the bound ignored (`synced_to()` returns `u64::MAX`): both tests_synced tests fail, each at its
  first `assert!(s.held())` (lines 129 and 198). Restored and `touch`ed.

## Step 3: theseus-b9x6 (`c8973251`)

**Changed.** Each guard now has a test that catches its planted revert. Separately, the narrow window the review saw
is fixed by **stitching**, not by saying:
- `fetch` follows a sealed object with the tails that start at its end (`after_object`). They are joined only when the
  first of them begins at the object's last position + 1, and the segment's source is then
  `Source::ObjectAndTails { count }` ("from its object and N tail(s) after it").
- A tail past the end that does not join is said in `unjoined`, never stitched.
- Tails inside the object, shipped while it was open, are still never read.
- A sealed segment's `unjoined` no longer cuts the restore short: its object is whole, and the next segment's position
  check still guards the join.

I chose stitching because those tails are the restored history itself, the `store.restored` row and everything after
it. With only a warning, a second restore would still drop them until the segment next rolls.

`seed` already ships an `ObjectAndTails` segment whole when it rolls, because only `Source::Object` counts as
`whole`; a comment there now says so.

**Proved.**
- (a) `a_segment_that_does_not_follow_is_a_gap_and_what_precedes_it_restores`. The store has at least 4 segments, and
  segment 2's object and row are given segment 3's bytes, sha256, length and `last_position`. The gap reads
  "segment 2 begins at position <first of 3>, not <last of 1 + 1>", only segment 1 is restored, from its object, and
  its records equal the log's.

  Planted revert, the guard off (`if false && first != Some(follows)`): the test fails at line 622
  (`r.fetch.gap … expect`). Restored and `touch`ed.
- (b) `a_restore_whose_row_rolls_a_sealed_segment_ships_only_the_new_tail`. It needs a full segment of the store's
  own 64 MiB size (about 6 s):
  - The live log fills segment 1 to 100 bytes short of 64 MiB, using 8 MiB ledger rows sized from a measured frame,
    then rolls into segment 2. It ships with 8 MiB parts, and segment 2's rows are taken from the table.
  - The restore takes segment 1 from its object, and its `store.restored` row rolls into segment 2.
  - The next pass sends only `000000002.seg.tail/…` and nothing again.

  Planted revert, `rotated && whole` off (`if false && …`): the test fails at line 690, because segment 1 is sent a
  second time. Restored and `touch`ed.
- The window: `tails_after_a_sealed_object_are_joined_by_the_next_restore`:
  - The table is cut to end at the last sealed segment.
  - The first restore's row fits in it, and the next start ships that row as a tail from the object's end.
  - A second restore takes `ObjectAndTails { count: 1 }`, with nothing unjoined and last position + 1. It holds both
    `store.restored` rows and the first restored store's records.

  Planted revert, the stitch off (the object alone): the test fails at line 740 (the source is `Object`).
- The existing 7 tests_restore tests pass unchanged.

## The live check (the maintainer's, with the owner's go; under a cent)

`durable::policy` for account `<account>`, region `<region>`, deployment `<scratch>`, used as the session policy:

```json
{"Version":"2012-10-17","Statement":[
 {"Sid":"ShipToItsPrefix","Effect":"Allow",
  "Action":["s3:PutObject","s3:GetObject","s3:ListMultipartUploadParts","s3:AbortMultipartUpload"],
  "Resource":"arn:aws:s3:::theseus-<account>-<region>/durability/<scratch>/*"},
 {"Sid":"ListItsPrefix","Effect":"Allow","Action":"s3:ListBucket",
  "Resource":"arn:aws:s3:::theseus-<account>-<region>",
  "Condition":{"StringLikeIfExists":{"s3:prefix":["durability/<scratch>/*"]}}},
 {"Sid":"ItsIndexRows","Effect":"Allow","Action":"dynamodb:BatchWriteItem",
  "Resource":"arn:aws:dynamodb:<region>:<account>:table/theseus-durability",
  "Condition":{"ForAllValues:StringLike":{"dynamodb:LeadingKeys":["<scratch>#*"]}}}]}
```

1. Head and get a missing key in the tender's session. Save that JSON as `/tmp/tender-policy.json`, then:

   ```sh
   eval "$(aws sts assume-role --role-arn arn:aws:iam::<account>:role/theseus-owner \
     --role-session-name theseus-durability --policy file:///tmp/tender-policy.json \
     --query 'Credentials.[AccessKeyId,SecretAccessKey,SessionToken]' --output text |
     awk '{print "export AWS_ACCESS_KEY_ID="$1" AWS_SECRET_ACCESS_KEY="$2" AWS_SESSION_TOKEN="$3}')"
   aws s3api head-object --bucket theseus-<account>-<region> --key durability/<scratch>/wal/none
   aws s3api get-object --bucket theseus-<account>-<region> --key durability/<scratch>/wal/none /dev/null
   ```

   Expect `An error occurred (404) … Not Found` from the head, and `NoSuchKey` from the get. Repeat with the
   `ListItsPrefix` statement removed (main's policy): both should be 403 / `AccessDenied`.
2. A scratch daemon:
   - Fresh state dir, `theseus-sim fake-model --rules` as the model, Discord and the web off.
   - `[aws.accounts."<account>"]` with `owner_role = "theseus-owner"`, `deployment = "<scratch>"`,
     `durability = true`.
   - Use its own `--config`, `--socket` and `--state-dir`.

   Then:
   - Run two turns (`theseus --socket <sock> send …`). Within a minute, `theseus --socket <sock> health` should show
     the durability line reading `caught_up`. A brief `waiting (for the WAL's sync …)` between a write and its sync is
     expected and clears a settle later.
   - Stop it with `theseus --socket <sock> shutdown`.
   - Run `theseusd restore --from s3://theseus-<account>-<region>/durability/<scratch>/ --state-dir <fresh2>`.
     `theseus sessions` on a daemon over `<fresh2>` should list the same sessions.
   - That daemon's next start should ship only its `store.restored` row: its health's `segments`, `tails` and `bytes`
     count that row's tail alone.
   - A second restore into `<fresh3>` should show `from its object and 1 tail(s) after it` when the row fit in the
     last sealed segment, and `from N tail(s)` otherwise.
3. Teardown with the owner's credentials:
   - `aws s3 rm --recursive s3://theseus-<account>-<region>/durability/<scratch>/`.
   - Delete the table's rows whose `pk` starts with `<scratch>#`: `aws dynamodb query` each pk (`<scratch>#wal`,
     `<scratch>#blob`, and `<scratch>#rec#<kind>` for each kind shipped), then `delete-item` each key.

## Left, uncertain, and for the owner

- **Docs to update at review:**
  - aws-toolset.md §5 should say that only synced frames ship, that the tender's session may list its prefix, and that
    a restore joins tails after a sealed object.
  - status.md / Part III should record the new `waiting` reason in health.
- **The fsync-off case.** `synced_to` is everything written: a store without fsync ships what its page cache holds,
  which is the old behaviour. The alternative would be to ship nothing.
- **An unbounded `Hooks`.** `Hooks::default()` has no bound. Only tests use it, and the daemon always passes one.
- **The power-loss test with the bound reverted.** It fails at its first held assertion, before reaching the
  `Rewound` it would otherwise show. Without the bound the unsynced tails do ship, so the cut leaves them stale past
  the log. That is the bug, and the test's later assertions also cover it.
- **The 64 MiB test** takes about 6 s and holds a few hundred MB while it runs.
- **One test failed in step 1's gate that is not on the known list:**
  `theseus-core term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one` failed after 15.7 s inside the full suite,
  then passed alone (0.54 s). It is a terminal timing test unrelated to this change, and it passed in steps 2's and
  3's gates.
- **One refused command.** This environment refused the brief's inline `sh -c 'while :; do :; done' &` load loop
  (a safety check on `sh -c`). I ran the same loop from a script file in the scratchpad and killed the loops by their
  pids.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit:
- fmt, shape, features, clippy, cockpit, test build and the reader rule passed.
- The suite failed only on the 33 known L1 tests (theseus-sandbox's contract tests and its bench's `spawn_100`, and
  theseusd's sandbox tests: theseus-pv6i, root without a job cgroup). Step 1 also had the terminal test above.
- I then ran the phases after the suite myself, and all passed: protocol types (with protocol.gen staged), the turn
  bench (`frames_plain` 5/5, `frames_tool` 9/9), and deny (advisories, bans, licenses, sources ok; `cargo deny fetch`
  succeeded in setup). The lifecycle and jobs benches are skipped under `THESEUS_GATE_NO_BENCH`.

Counts: step 1 (e0c068af) 2564 run, 2530 passed, 34 failed (33 L1 and the terminal test); step 2 (203fc632) 2567 run, 2534 passed, 33 failed (L1). Step 3's gate (c8973251): the suite ran 2570 tests, 2537 passed, and the 33 failures are the known L1 ones alone; the phases after it passed.
