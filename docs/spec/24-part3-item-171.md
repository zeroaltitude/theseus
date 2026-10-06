# The Ship of Theseus, chapter 24: Part III, A4's Items 171 to 180 ([index](README.md))
### Item 171. Durability fixes: only synced frames ship, the tender's session may list its prefix, and a restore joins the tails after a sealed object (theseus-iame, theseus-mgw.12 and theseus-b9x6; the seventh cloud batch's durability-fixes session, fired 2026-10-05 01:35 from 80ef1dea, Opus 5.5; e0c068af, 203fc632 and c8973251; reviewed 05:03 to 06:00 by local reviewer R9, stack A, and accepted with the stack at 07:23; joined 08:17 at 38bb2924, a signed merge onto 3dba509d, by the stack-A joiner; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** Three gaps in the durability tender (step 15, Item 108) and the S3 restore (step 16,
Item 123), each found after its join:
- **theseus-iame (P1).** The AWS live checks of 2026-10-04 (check 3, the restore, 23:00) ran a session carrying
  exactly the tender's inline policy (`durable::policy`: put, get, list parts and abort under its prefix, no
  `s3:ListBucket`). There a HEAD of a missing key answered 403, not 404. `Bucket::call` maps only a 404 or
  `NoSuchKey`/`NoSuchUpload` to `Missing`, so a 403 was `Other`, which `s3_halt` turns into `Retry`. The tender heads a
  key exactly when its cursor held the object in flight at a crash (`put_once`, `resume_upload`'s head, `put_large`'s
  lost completion), so a crash between saving the mark and S3 storing the object retried forever and shipped nothing
  behind it. The restore's own session had been given the list statement at step 16; the tender never was.
- **theseus-mgw.12.** The follower reads whole frames from the page cache, so a tail could hold a frame the machine
  had never synced. After a power loss the local log is cut and written again differently: the next start meets a
  rewind and ships again, but the stale tails past the new log stay in S3. Since Item 92 each frame carries the
  synced mark, so the tender can know what is durable.
- **theseus-b9x6.** R1's review of restore-s3 found two guards right in the code that no test caught (a segment
  whose first frame does not follow the last restored position is a gap; a restore whose own row rolled into a new
  segment seeds the cursor so the sealed one is not shipped again), and a narrow window: after a restore whose last
  segment came from its sealed object, the tender ships that segment's later frames as tails, and a second restore
  preferred the old sealed object and never read them, saying nothing.

**What landed** (`theseus-core`'s `aws/durable*`, `theseus-follow`, one accessor in `theseus-store`; the merge 15
files, +917 −58, without the cloud files; no new package, no store format change, nothing on the start path).
- **The tender lists its prefix** (e0c068af). `durable::policy` gains `ListItsPrefix`: `s3:ListBucket` on the bucket,
  under `StringLikeIfExists` on `s3:prefix` = `durability/<deployment>/*`, as the restore's `read::policy` has it. A
  HEAD carries no `s3:prefix`, so only `IfExists` lets it through; with it a missing key heads 404, and an object in
  flight at a crash is sent once. The policy is now three statements, still with no delete and no action ending in
  `*`. The test fake's HEAD arm answered 404 to anyone; it now answers by the session's policy through one helper,
  `may_know_missing`, which the GET arm shares, and a 403 with no body, as S3 does. _(theseus-bfk9, Part III Item 198: "a HEAD carries no `s3:prefix`" is false on real S3: a HEAD's implied list carries the key as `s3:prefix`, so the statement is now `StringLike`, which refuses a list with no prefix and still lets a missing key head 404.)_
- **Only synced frames ship** (203fc632). The bound is the writer's own `Wal::synced()`: exact, live, and in the
  process that writes the log. `WalStore::synced_to()` (theseus-store) is wrapped by the core's `Store::synced_to()`
  and handed to the shipper as `Hooks::synced_to`, a closure that holds the core by `Weak` and reads 0 once the core
  is gone. With fsync off, `synced_to` is `last_position()`, so a store that never syncs still ships.
  - theseus-follow gains `read_upto(max_bytes, upto)`, which stops before the first whole frame whose last position
    is past `upto` and answers `Stop::Held { segment, offset, position, at_unix_ms }`, its cursor before that frame. A
    segment the read stopped inside is never named sealed, even when a later one exists. `read` is
    `read_upto(.., u64::MAX)`, so theseus-index, which calls `read`, is unchanged; `Batch` gained no field, and the
    one new `Stop` variant breaks no match on main (each is an `==` check or has a `_` arm).
  - The shipper reads at each batch's bound. A pass that ends held sets `oldest_unshipped_unix_ms` to the held
    frame's time, emits no `CaughtUp`, says `waiting` in health with the reason "for the WAL's sync: position N is
    written, not yet synced", and passes again a settle (5 s) later, since a sync changes nothing inotify sees.
    `AwsDurabilityStatus`'s doc names the new reason (protocol.gen's comment only).
  - Nothing changed in the WAL's writes, its frame layout or the store's format.
- **The restore joins tails after a sealed object** (c8973251). `fetch` follows a sealed object with the tails that
  start at its end (`after_object`), joined only when the first begins at the object's last position + 1; the
  segment's source is then `Source::ObjectAndTails { count }` ("from its object and N tail(s) after it"). A tail past
  the end that does not join is said in `unjoined` and never stitched; tails inside the object, shipped while it was
  open, are still never read; and a sealed segment's `unjoined` no longer cuts the restore short, since its object is
  whole and the next segment's position check still guards the join. `seed` already ships such a segment whole when
  it rolls (only `Source::Object` counts as whole), and a comment now says so. The session chose the stitch over a
  warning because those tails are the restored history itself, the `store.restored` row and everything after it.
- **AGENTS.md:** theseus-follow's guide (`read_upto`) and theseus-core's list of AWS tests.

**How it is proven.**
- **The session's tests.** `an_object_in_flight_that_s3_never_stored_is_sent_once` (one pass with `PutObject` refused
  saves the tail's mark; the restart heads it, then one put; 30 more records ship and every segment equals the log)
  and `an_upload_s3_no_longer_lists_starts_again_once` (`ListParts` gets `NoSuchUpload`, the HEAD 404; two
  `CreateMultipartUpload`s, one completion). The policy test checks three statements and the exact condition. The
  follower's `a_read_up_to_a_position_holds_the_frames_past_it`. A new `aws/tests_synced.rs` writes the log as the
  writer does (`Wal::write`, then `Wal::sync`): `a_write_without_its_sync_waits_and_ships_after_the_sync` (three
  synced frames ship and two wait, `oldest_unshipped` the held record's time, no tail past the synced byte; after the
  sync it ships to 5 and says `caught_up`; an unsynced roll does not ship segment 1 whole until synced) and
  `a_power_losss_cut_follows_on_from_the_cursor` (two unsynced frames held; segment 1 truncated where the sync ended,
  two other frames written and synced; the next start passes `caught_up` with no rewind and sends nothing again, and
  `restore::from_s3` equals the log with no gap). The restore's guards: a segment given the next one's bytes reads as
  a gap at its first position, and only what precedes it restores; a restore whose row rolls a sealed 64 MiB segment
  ships only the new tail (about 6 s); and `tails_after_a_sealed_object_are_joined_by_the_next_restore` (a second
  restore takes `ObjectAndTails { count: 1 }` and holds both `store.restored` rows). tests_durable and tests_synced
  ran 5 times under load, 12 of 12 each. The session's own planted reverts (the statement dropped: 4 fail; the bound
  ignored: both tests_synced fail; each guard off; the stitch off) each failed as it said.
- **The review** (R9, on main ef37f325; clean merge, no join fix): fmt, the test build, clippy, theseus-protocol's 31
  tests, the cockpit's 65 tests and build, and shape clean; **319 of 319** selected tests (theseus-follow, -store,
  -index and -protocol whole; the core's `aws::` and `store::`). **3 of 3 planted reverts caught**, each a fresh angle
  on the session's: the bound ignored (the power-loss test and its sibling fail); the list statement as plain
  `StringLike` (a HEAD carries no prefix, so 403 again: 4 fail); the stitch off (`left: Object, right:
  ObjectAndTails { count: 1 }`). R9 also checked that theseus-index still reads every follower outcome, that the
  bound is read once per batch and only grows, and that with fsync on a held pass is only the window between a write
  and its sync.
- **Live:** none. Every live check reaches AWS, and the overnight rules allowed no AWS call; the fake's `IfExists`
  model (the review's plant 2 shows it) stands in. Two checks wait for Eddie (Known gaps).

**What the session found.** The fake had hidden the bug: once its HEAD answered by the policy, the existing
`a_refusal_is_said_in_health_and_the_next_pass_ships` failed on main's policy too, and had passed only because of the
fake. A frame's synced mark is a batch behind, so a bound read from the marks would never ship a quiet store's last
write; hence the writer's `synced()`. The hands' policy needs no list statement: a hand only puts its outputs, and
needs `StringLikeIfExists` too if it ever reads back. _(Since theseus-bfk9, Part III Item 198: `ListBucket` under `StringLike` on its prefix.)_

**The join** (stack A's first; R9's dry runs clean on b07150d6, 1a08a40e and, by the joiner at 07:25, 3dba509d).
Lock `cloud-durability-fixes-join` 07:27:49; the merge at 07:29:09 on sim2's 3dba509d was clean (git placed the
branch's one accessor each in the core's `AGENTS.md` and `store.rs` and theseus-store's `store.rs` beside main's
changes; no resolve.py, nothing for rerere), 15 files staged, its tree equal to `merge-tree`'s less the two cloud
files. No join fix. The joiner checked by hand what `merge-tree` cannot: every `Stop` use on main is an `==` check or a
built value, and every `Hooks {` literal is in a file only this branch changed. The warm took 34 minutes (a test build
of 25 m 54 s), because the stack-K reviewer's 16 busy loops ran beside it; then rustfmt, theseus-protocol's 31 tests
(protocol.gen as staged) and R9's selection, 319 of 319. The signed merge **38bb2924** (3dba509d and 7af7c71c), 08:05.
Its gate (08:05:50 to 08:16:45, first take): **2,703 of 2,703** (1 slow, the floor-wake test, 19 skipped), the reader
rule's 9; lifecycle in every budget (cold start p50 31.8 / p95 37.5 ms; SIGKILL then restart 35.1 / 37.6; clean
shutdown with a job 35.7 / 53.0; binary swap 57.4 / 61.6; the daemon's own clock kernel 9.83 / 10.94, serving 24.39 /
30.82); L1 start p50 10.01 ms; turn frames 5 and 9 (plain p50 86.4 ms, tool call 196.4), fdatasync p50 7.7 ms. Pushed
08:17; done line 08:17:15. theseus-iame, theseus-mgw.12 and theseus-b9x6 closed with the hash, each with a note naming
the live check still owed. The store stays at format 19.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). Nothing changes while durability is off, as it is on Eddie's daemon. Once it is on:
only synced frames ship; a missing key heads 404 and an in-flight object is sent once; health's durability line may
read `waiting` "for the WAL's sync" for a moment; a restore stitches the tails after a sealed object. _(Health's text printed no durability line until theseus-9ai1, Part III Item 198: the status was in `--json health` alone. Durability went on for Eddie's daemon at install #6, 2026-10-06.)_ Turning it on
waits for the live checks. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** The bound is an argument to a new `read_upto`, not a field of `Batch`. The fsync-off case ships what
the page cache holds, the old behaviour; R9 recommended keeping it, since fsync off is a test or bench store only and
no daemon setting has it. `Hooks::default()` ships with no bound; only tests use it. The window b9x6 named was closed
by stitching, not by a message.

**Known gaps.** Two AWS live checks wait for Eddie: a HEAD and a GET of a missing key in the tender's session (404 and
`NoSuchKey`, 403 with the statement removed), which proves `IfExists` on real S3; and a scratch daemon with
`durability = true` through two turns, a restore, a start that ships only the `store.restored` row, and a second
restore that says "from its object and 1 tail(s) after it", then the teardown (under a cent). theseus-nysv (P3): two
restore messages in `durable/fetch.rs` carry an 18-space run, a lost line continuation, shown to the operator.

### Item 172. Push once: each notification serialized once and shared by every queue, and every operator act's `by` from `Conn::actor` (theseus-celu.36, with theseus-cny7; Review 2's S6, v1.1's lane push2 and step V8; the seventh cloud batch's push-once session, fired 2026-10-05 01:35 from 80ef1dea, Opus 5.5; 90a5121b, 3c13516d and 2cc6a3be; reviewed 04:59 to 06:56 by local reviewer R10, stack P, and accepted with the stack at 07:28; joined 08:32 at 0a94e361, a signed merge onto 38bb2924, by the stack-P joiner; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** The push (Review 2's S6) had added the backlog cap and `events.lost`, but `SessionBus::publish` still cloned
each message once per watcher, `EventSink::send` cloned it again for the requester, and each connection's writer
serialized its own copy: N deep copies and N serializations per notification. And operator acts other than approvals
still wrote the connection's label as `by` (`web#35`, seen on a recompile by the cockpit's controls check on
2026-10-01, theseus-cny7), where since spec v0.71 an approval, a trust, a press, an undo and a cancel name the person
or the surface through `Conn::actor`. Roadmap v1.1 ordered V8 after push2, since both sit near `rpc/server.rs`, so one
session did both.

**What landed** (`theseus-core` only; the merge 9 files, +543 −50; no stored record, protocol type or package
changes; the store stays at format 19).
- **The measurement** (90a5121b). `tests_push_once::measure_serializations_per_notification`, ignored by default,
  publishes 200 rounds of a `model.delta`, a padded 64 KiB `node.written` and a 48 KiB `turn.ended` through a turn's
  `EventSink` to 1, 10 and 100 watchers on capped queues, writes every queue out as a writer does, and prints
  serializations and copies per notification and the time per watcher, from `outbound::counts` (thread-local, test
  builds only). Run with `cargo nextest run -p theseus-core --run-ignored only -E 'test(measure_serializations)'
  --no-capture`. (`node.written` really carries only ids and a kind; the padded row stands for a large notification,
  and `turn.ended`, which carries the reply, is the largest a turn sends.)
- **Serialize once** (3c13516d). A queue holds an `Item`: a `Message` its writer serializes (responses, and anything
  sent to one connection alone), or a `Line(Arc<str>)`, a notification's NDJSON line with its newline, written as it
  is. `outbound::Note` borrows a notification and serializes it lazily (`OnceCell`) at the first queue that takes it;
  a note that is dropped at the cap, or reaches only raw test channels, is never serialized. `bus.rs`'s `publish`,
  `publish_all` and `EventSink::send` (the requester, then `deliver` to every watcher, the all-session watchers
  included), `narrative.rs` and `profile.use`'s notification each build one `Note`, so every queue holds the same
  `Arc`. `rpc/server.rs`'s `write_item` writes a line as it is; `write_one` still sends `events.lost` after whichever
  item drains a queue. The cap is untouched apart from the item type: a line counts as one queued message, a
  notification past `BACKLOG_CAP` is dropped and counted with its stream, responses always go, and S6's latest-wins
  queues and the slow client's disconnect are as they were. The wire is byte for byte the same
  (`serde_json::to_string(&msg)` plus `\n`); the serialization now runs once on the publisher's thread, under the
  bus's `subs` lock where the deep clones ran before. Every surface goes through `serve_connection` (the socket,
  `--stdio`, the web UI's WebSocket, Discord's in-process client), so all share the one line.
- **V8** (2cc6a3be). `profile.use` passes `conn.actor(None)` to `switch_profile` (so `profile.changed`'s row, the
  answer and its notification carry it), and `session.recompile` passes it to `request_recompile`
  (`context.recompile_requested`'s row). The session listed every method that takes a `Conn`: the others already
  named the actor (through `conn.actor` or `conn.answerer`), or use the label as the connection's own key for its
  subscriptions, which is right, or read only the surface.
- **The output golden boxed.** `tests_output::the_cores_output_matches_its_golden` had under 48 KiB of room on the
  test thread's 2 MiB stack on main, and push-once's debug frames tipped it into an overflow (gdb put the deepest point
  in serde's decoding of an `Execution` under `ToolRuntime::complete`, not in this change). The test boxes its two
  scenarios, moving their state to the heap: about 150 KiB of room. Retention's join on main had made the same boxing
  (Item 157), so the merge kept main's comment.

**How it is proven.**
- **The measurement**, the session's, debug build: serializations per notification fell from N to 1 and deep copies
  from N to 0; at 100 watchers the time per watcher fell from 6.2 to 1.6 µs for a delta and from 1.29 ms to 13.7 µs
  for the 64 KiB notification. At one watcher it is the same work, on the publisher's thread. The review's A/B (main
  with the measurement commit against the whole stack, debug, frozen, A B B A in one review-step hold, 06:44:45 to
  06:49:25): N and N on main, 1 and 0 on the stack; per watcher at 100 watchers, a delta 5.3 and 4.9 µs against 0.84
  and 0.80, a 64 KiB `node.written` 1.44 and 1.03 ms against 11.5 and 12.7 µs, a 48 KiB `turn.ended` 0.84 and 0.83
  ms against 8.7 and 9.6 µs (about 6 times cheaper per watcher for a delta, 80 to 125 times for a large note).
- **Tests.** `the_requester_and_two_watchers_write_one_line_byte_for_byte` (one serialization, no copy, the same
  `Arc`; written through `write_item`, the bytes equal the old `to_string + "\n"`),
  `a_notification_no_connection_queues_is_never_serialized` (no watcher, a raw channel, a full queue) and
  `a_large_notification_to_a_hundred_watchers_is_one_copy` (`Arc::strong_count` 101); `tests_actor.rs` drives
  `profile.use` and `session.recompile` through `serve_connection` as `sock#7` on the CLI, `web#35` on the web UI and
  an unnamed `tide#3`, and reads `the CLI`, `the web UI` and `tide#3`. The cap's, bus's, narrative's and `tests_push`'s
  tests assert exactly what they did. The review ran 1,364 tests on the merged tree (all of theseus-core and
  theseus-discord; theseusd's push, web UI, headless, stops and closed-pipe binaries; the CLI's golden): 1,363 passed,
  the one failure a judge timing test under load ~18 that passed alone and on the stack (theseus-vbju, P3); every
  push-once test passed again on the whole stack, among 1,515. The golden passes at the default stack, and on the
  stack still at 1,900 KiB, overflowing at 1,800.
- **Planted reverts**, 3 of 3 caught at exit 100 on the whole stack: each queue serializing its own line again (the
  two tests read 3 and 101 serializations, and the measurement sees N per notification and main's time per watcher);
  `events.lost` sent only after a message (the lag test waits out its 30 s); the recompile's `by` back to the label
  (`sock#7` against `the CLI`). The session's own three (a copy per watcher, the cap skipping shared items,
  `events.lost` after a shared item) each failed as it said.
- **Live** (the review's, on a scratch daemon of the merged build, the stand-in model, Discord and the index off, the
  web UI on loopback): one long turn (a 98,762-byte `turn.ended`) with two `--json watch`es, two raw socket taps and a
  raw WebSocket tap on the web UI's own `/ws`: the watches identical, the two taps' 11 notification lines equal byte
  for byte, the web UI's 11 frames the same lines. The cap: a watch and a web tap SIGSTOPped while 800 turns ran
  (324 s), then continued: the watch printed exactly one "lost 4577 notifications while this terminal was behind",
  the web tap got exactly one `events.lost {dropped: 3959}`, and health's push line went from `lost 0` to `lost
  8536`, their sum. `by`: `profile.changed` from the CLI and the web UI's socket read `the CLI` and `the web UI`, and so
  did the two recompiles; no `sock#` or `web#` anywhere.
- **FAST.** Nothing on the start path. The turn (8 runs, A B B A A B B A): plain 5 frames on both, median p50 100.9
  against 101.0 ms; tool call 9 frames, 195.1 against 205.3, one outlier B run (329.6), by pairs +10, +133, −12 and
  −26 ms: no cost. The lifecycle (4 runs): B's medians equal or better in every phase but cold start (+2.2 ms, its p95
  47.6 to 40.7), both arms missing budgets under the neighbours' IO.

**What the session found.** The golden's stack edge (above): any change that grows a turn's futures could tip it, so
other branches might meet it, and the boxing fixes it. The lag test (`tests_push`'s
`a_client_that_stops_reading_hears_what_it_lost_and_catches_up`) hits nextest's 120 s kill at nice 19 behind four busy
loops on four cores, on main's code too; it passed in every review run at load 5 to 18. `turn.submit`'s author still
falls back to the connection's label (`p.author.unwrap_or(conn.client)`, so a CLI message is authored `sock#7`); the
session left it, since authors are in the compiled transcript the model reads.

**The join** (stack P's first; R10's dry runs on b07150d6 and e4d09068, the joiner's on 3dba509d and 38bb2924). Lock
`cloud-push-once-join` 07:40:42, queued behind stack A's durability-fixes; merged at 08:17:18 on 38bb2924. Two
conflicts, R10's: theseus-core's `lib.rs` (activation's test modules beside V8's `tests_actor`, both kept in order)
and `tests_output.rs` (both sides box the golden's halves; main's comment kept, the code the same). rerere replayed
R10's resolution and `push-once/resolve.py` found nothing left and its checks held. No join fix: every caller of
`Outbound::notify` and `Drain` on main is in the four files the branch changes; task-board's `TASK_CHANGED` in
`bus.rs`'s `wide()` is covered, since `deliver` reads `wide(note.message())`. The staged tree equals the dry run's, and
against R10's review merge differs by exactly main's own changes since (177 files). The warm (test build 3 m 38 s,
clippy) clean; before the gate push-once's tests, the golden at the default 2 MiB stack and theseusd's push, web UI
and closed-pipe binaries, 33 of 33. The signed merge **0a94e361** (38bb2924 and 3ec37e96). Its gate (08:23:45 to
08:32:38): **2,708 of 2,708** (1 slow, 20 skipped); lifecycle in every budget (cold start p50 22.4 / p95 27.4 ms;
SIGKILL then restart 26.0 / 30.4; clean shutdown 32.2 / 51.1; binary swap 47.5 / 53.1); L1 start p50 7.70 ms; turn
plain 5 frames p50 78.4 ms, tool call 9 frames p50 142.5 (sim2's gate before it: 74.2 and 149.2), fdatasync p50 6.6
ms. Pushed; done line 08:32:56. theseus-celu.36 and theseus-cny7 closed with the hash.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No key and no format change. Every watcher shares one serialized line per
notification, and a slow watcher still gets one `events.lost`. The ledger's `by` for `profile.use` and
`session.recompile` names the surface (`the CLI`, `the web UI`), not a connection's label. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** The cap stayed exactly as it was, in `outbound.rs`; only the queue's item type changed. With one
watcher the serialization moved from the writer's thread to the publisher's. `tests_actor.rs` slipped into step 1's
commit (nothing compiles it until step 3's `mod` line); history was not rewritten, and the join did not squash it.

**Known gaps.** For Eddie, both recommended yes by R10: `turn.submit`'s author following `actor`
(`conn.actor(p.author.as_deref())`), as its own step with the golden's diff read, since it changes what the model
reads; and the golden run on a thread with an explicit 8 MiB stack, since the boxing leaves about 150 KiB. theseus-vbju
(P3): `tests_judge::a_failing_jev_is_recorded_by_its_class_and_changes_no_turn` overran its 3 s turn bound once under
suite load. Review 2's S6 is done for serialization; latest-wins and the slow-client disconnect remain.

### Item 173. Kernel fixes: a deadline's stop verified for the whole tree, an earlier process's provider calls unknown at the first tick, one late-after-cancel row, and nested kernel locks caught (theseus-g11i, theseus-jnnj, theseus-m9iy and theseus-oqxw; the seventh cloud batch's kernel-fixes session, fired 2026-10-05 01:35 from 80ef1dea, Opus 5.5; 2d4c3afc, 389ed841, 1f1dacdf and ba6455a5; reviewed 05:51 to 06:05 by local reviewer R8, stack K, held on one slow cancel, and in R8's follow-up 06:26 to 07:12; accepted at 07:51 by the batch-7 harvest wake 3; joined 08:45 at fb90df75, a signed merge onto 0a94e361, by the stack-K joiner; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** Four kernel issues, each found after its step had joined:
- **theseus-g11i.** At batch 5's smalls-tools join gate (2026-10-04 18:37 to 18:43, Item 147),
  `the_deadline_stops_the_whole_tree_too` found a job's two sleepers still running just after its completion could be
  read. Three readings were open: (a) a SIGKILLed process not yet gone when the kill's wait ended; (b) a phase ending
  on an empty scan while the wrapper still had a child; (c) the test's own scan, which looked over the whole machine's
  `/proc` for a fixed marker (`300.1802`) another run of the same test could share.
- **theseus-jnnj.** Tier 7.1 (Item 88) gave a spooled completion two takers, the drain and the turn waiting on its job.
  `completion_with` let the second taker write nothing only for a `Succeeded` or `Failed` action, so a cancelled job's
  late completion taken by both wrote `completion.late_after_cancel` twice (nothing was booked twice).
- **theseus-m9iy.** Found by the spine step S2's live check (2026-10-02): a provider call in flight when the daemon was
  killed stayed `dispatched` after the restart until its deadline (`[model.timeouts] total_secs`, 600 s), its session
  "waiting on 1 call" with the call's reservation, until the heartbeat marked it `overdue_no_evidence` (in the live
  check, killed 09:38:27, settled 09:48:30).
- **theseus-oqxw.** A transaction (`Kernel::frame`) holds its executions' locks while its closure runs. A transition
  called on the kernel itself for an execution the transaction did not name takes that lock while holding others, out
  of id order, and two such threads could hang; nothing caught it.

**What landed** (`theseus-kernel`, two lines in `theseus-core`; the merge 14 files, +701 −62; no store format change,
no protocol type, no config key, no package; `tests_frames.rs`'s golden byte-identical; kernel.rs 3,008 of its 3,030
ceiling).
- **The tree stop (g11i, 2d4c3afc).** In `tests/tree.rs` each case's marker is `300.180<n><this run's pid>`
  (`marker_of`), so a case scans for its own processes alone, and a failure prints the stop's verdict and, for each
  process found, its state, start time, parent and signal masks. In `tree.rs`, defensively for (a) and (b):
  `tree::stop`'s reap now answers `tree::Left` (`job::reap_all`: `None` on ECHILD, `Some` when `waitpid` returns 0,
  `Unknown` otherwise). Phase 1's early return, the freeze's end and the kill's end each need an empty scan *and* a
  reap that does not say `Some`; a child the reap counts and no scan finds is reported as a survivor
  (`Stopped::unseen`, "a child no scan found"), never verified. The kill's wait keeps each killed process's pidfd
  (checked against its start time, as the signal's was) and ends only when every one polls as exited (`wait_exit`).
  `KILL_WAIT` went from 500 ms to 2 s, and the daemon's `job::ANSWER_WAIT` from 2.5 to 3 s, so the freeze (0.5 s) and
  the kill (2 s) still fit before the daemon kills the wrapper's group. The cgroup stop keeps `populated 0` as its
  emptiness test and ignores the reap's answer; the tree stop after it uses it. (Since theseus-ypqg a wrapper's only
  children are its command and the orphans it inherits as a subreaper, so ECHILD means its job has no process left.)
- **One late row (jnnj, 389ed841).** A `Cancelled` action with `completions_seen >= 1` reads as `Accepted::Taken`.
- **An earlier process's calls (m9iy, 1f1dacdf).** `Evidence::in_process`, a default trait method, is
  `action.tool == PROVIDER_TOOL`, which tells an in-process call from a job with no format change; it is scoped to
  provider calls, since a dispatched tool call is answered by its turn's resume. Startup's scan
  (`reconcile_with(.., false)`, called once per process) notes each dispatched, not-overdue in-process call in
  `Kernel::earlier`, in memory, and writes nothing. `Kernel::mark_earlier_calls_unknown` (new `earlier.rs`) marks them
  all `outcome_unknown` in one frame, reason `in_process_before_restart` (producer
  `reconciler:in_process_before_restart`), skipping any settled since. The driver calls it once before its loop
  (`Core::mark_earlier_calls` in `rpc/driver.rs`, one line in `harness::drive`, which stays at 99 of clippy's 100
  lines); the heartbeat's reconcile is the backstop, and a crash before the tick loses nothing, since the next start
  scans again. Marking a call takes it out of the execution's `outstanding` and moves its reservation from reserved to
  held-unknown. An overdue in-process call at startup is still marked by startup's own reconcile, as before.
- **Nested locks (oqxw, ba6455a5).** `ExecLocks::lock_all` checks a thread-local count of the `ExecLock`s the thread
  holds, of any `ExecLocks`; above 0 it panics before taking anything ("a lock taken while this thread holds another:
  name it in the one Kernel::frame"), keeping the old "locked twice on one thread" message when the id is the same.
  One thread-local read per transition. The session found nothing that nests on purpose: every multi-lock transition
  takes its locks in one `lock_all`.
- The kernel's guide: tree.rs, `tests/tree.rs`, the job wait, `earlier.rs`, and the lock invariant.

**How it is proven.**
- **Reading (c), found by the session.** On the old stop, the deadline case failed 0 of 400 runs sequentially at
  nice 19 beside four busy loops, 0 of 200 with four runners each in its own pid namespace, and **94 of 100 with four
  runners sharing `/proc`**: 27 with a verified verdict (`survivors 0`) while the scan found sleepers whose parent was
  another run's wrapper, 67 because another run's scan had killed theirs. No run on any load showed (a) or (b). On the
  new code: 0 of 300 sequential and 0 of 100 in the shared four-runner setup; `tests/tree.rs` whole 20 of 20 under
  load.
- **The session's tests:** two load-free tree tests (an injected scan that misses a live child three times, with a
  reap that says it is left: killed 1, left 0; a reap that always says `Some` with an empty scan: unverified, "1
  process outlived the kill: a child no scan found"); `a_cancelled_jobs_late_completion_taken_twice_writes_one_row`;
  `tests_earlier` (a provider call in a turn that dies, a job parked on, a session parked on its own call; startup
  writes the same 2 frames as without them and marks nothing; the first tick marks both calls in one frame, the job
  stays dispatched; a second tick writes nothing; the heartbeat backstop; a call this process dispatched is left); the
  two nested-lock tests (the two-thread half, with the check removed, hangs into `Timeout` after 5.17 s: the deadlock
  is real on the old code). (a) has no load-free test: nothing keeps a SIGKILLed process off the CPU without load.
  kernel-sim at 40 seeds held every invariant at both race rates.
- **The review** (R8, merged onto sim2's review commit over main ef37f325; clean, no resolve.py, no join fix). The
  whole workspace suite on the stack, `--retries 0`, at load 13 to 16: **2,685 of 2,686**. The one red,
  `job_approval`'s `a_cancel_kills_the_jobs_whole_tree_a_setsid_descendant_too` ("a verified kill is quick:
  2.077 s" against its 1.5 s bound, with a right verdict; the whole test took 10.5 s there, about 7 times its time
  alone), held the review for an A/B. One copy of the test, built once, against main's daemon and the stack's,
  alternating, about 50 timed runs per arm: the cancel's median (max) was 89 (114) against 91 (425) ms quiet, 93.5
  against 87.5 beside 16 busy loops, 114.5 against 114.5 beside 64, and 129.5 against 119 with 16 copies at once,
  g11i's stop 10 to 32 ms in every run; 0 of 20 plain reruns failed on either build; and the same bound had failed on
  main's code at 1.507 s on 10-01. So it was the bound under load, filed as theseus-cs71 (P2). **4 of 4 planted
  reverts caught** at exit 100: g11i (b), m9iy's startup note, oqxw's check, jnnj's arm.
- **The kernel simulator over this kernel** (the stack's release build, 200 seeds × 1,000 steps): at `--p-race 0`
  and 0.3 alike **200,400 invariant checks, all held**, with every sim2 count above 0 and the counts kernel-fixes
  moves present (completions 25,604 with 2,118 duplicates and 561 late-after-cancel, 5,382 unknown → 3,898 resolved, at
  rate 0; 12,046, 825, 416 and 1,970 → 1,440 at 0.3). The sim test binary at nice 19 beside 16 busy loops: 10 of 10
  (cut from 20 to give the machine back to the join queue).
- **Live** (R8, a scratch daemon of the stack's build): after a `kill -9`, session B's provider call (to a listener
  that never answers) was `outcome_unknown` **15 ms after `driver.started`**, with one `in_process_before_restart`
  and no `overdue_no_evidence`, and B held $1.3065, the old call's reservation; session A's job crossed the restart
  ("the harness restarted meanwhile") and landed as `tool.late_result`, "proc.run ok (late) · exit 0 · 20002 ms ·
  job-done", never marked unknown. The deadline case under load, 100 runs per build at nice 19 beside 8 loops: 0 of
  100 on both. Reading (c), two runners × 30 sharing `/proc` beside 8 loops: **the stack failed 0 of 60, main's build
  36 of 60** (28 of them a scan finding the other runner's processes).
- **FAST** (A B B A A B B A, one hold): plain turn A 144.8 against B 131.8 ms, tool call 303.5 against 300.9; the
  lifecycle's B − A within each arm's spread (cold −14.8 ms, restore +36.3 as noise). No cost.

**What the session found.** Reading (c) is what failed on the gate: a lane running the kernel's tests outside a gate,
beside one, gives exactly that failure. m9iy's brief had the requeued turn waiting on the stale call, but no turn code
waits on it (`park` leaves provider calls out of `Wake::Actions`, and a requeued turn plans a new call): what the call
held was the execution's `outstanding` entry and its reservation, which the mark settles. And `reset_budget` frees
nothing of such a call: the reservation is held unknown for good, $1.31 per crash in the live check (theseus-f3wr,
below). The terminal test `sh_runs_a_command_and_ctrl_c_interrupts_one` failed on main's code too, 3 of 6 alone
(theseus-ynia).

**The join** (stack K's second, after sim2; R8's dry runs onto 1a08a40e with sim2's join simulated and onto sim2's
join commit 3dba509d itself, both clean, not even the cloud files). Lock `cloud-kernel-fixes-join` 07:59:53, queued
behind durability-fixes and push-once; one guarded call checked sim20's failures (none), fetched, dry-ran onto
0a94e361 and merged at 08:32:58. `harness.rs` and `rpc/driver.rs` auto-merged: 14 files, +701 −62, 13 of them
byte-identical to R8's review merge and the 14th differing only in restart-notify's lines above `drive` (theseus-74lt,
Item 166); its bind heartbeat's reconcile is m9iy's backstop, and either order writes one
`outcome_unknown` per call. No join fix. The warm (test build 3 m 32 s, clippy) clean. The signed merge **fb90df75**
(0a94e361 and 5a92b32e). Its gate (08:38:31 to 08:44:42, first run): **2,716 of 2,716** (push-once's 2,708 plus 8 new;
1 slow, 20 skipped, no retry); lifecycle in every budget (cold start p50 21.9 / p95 25.4 ms; SIGKILL then restart
26.2 / 30.9; binary swap 50.8 / 61.0; the kernel startup's reconcile p50 0.13 ms, as before); L1 start p50 6.00 ms;
turn frames 5 and 9 (plain p50 72.4 ms, tool call 149.3); both kernel-sim tests with no retry (10.48 and 11.66 s);
cs71's cancel test passed at 1.648 s. Pushed about 08:45; done line 08:45:45. theseus-g11i, jnnj, m9iy and oqxw
closed with the hash (m9iy's naming theseus-f3wr); theseus-cs71 open. R8's tree, target and scratch removed. No store
format change.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No key and no format change. A job's stop now waits up to 2 s for a verified kill;
after a crash, an earlier process's in-flight provider calls go "outcome unknown" at the first tick instead of after
their 10 minutes. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** g11i's (a) and (b) were fixed though neither showed, since the cancel's stop and the cgroup's tail
share the code; (c) was the test's. m9iy marks at the driver's first tick, not at startup's reconcile, which keeps
startup writing nothing new. `in_process` is scoped to provider calls.

**Known gaps.** **theseus-f3wr** (P2, waiting for Eddie): an earlier process's call's reservation stays held unknown
for good; R8 recommends booking it as spent at the mark (the request reached the provider and may have been charged),
and his reset then clears it. **theseus-cs71** (P2): the cancel test's 1.5 s wall bound fails under a loaded suite;
R8 recommends bounding the verdict's own `ms` and keeping g11i's longer waits, which cost a normal cancel nothing. (a)
has no load-free test. `harness::drive` is at 99 of clippy's 100 lines, so its next line needs a split. Two kernel
comments (`wakes.rs`'s `drop_wakes`, kernel.rs's `cancel`) still say `/stop` drops an execution's wakes, though
`stop_execution` keeps them, as sim2 found and its simulator now checks (Item 170); kernel-fixes, which owned
the kernel that night, left them.

### Item 174. AWS fixes: runaway mode ends on a raised line, `[policy.aws]` keys checked against the catalog after serving, a blackhole route tested, and the hand image's build script fixed (theseus-6hkx, theseus-snhr, theseus-rx7m and theseus-i7bz; the seventh cloud batch's aws-fixes session, fired 2026-10-05 01:35 from 80ef1dea, Sonnet 5.5; 9bb3df98, ea31d70f, f3169c92 and e73ecb3e; reviewed 06:00 to 06:22 by local reviewer R9, stack A, and accepted with the stack at 07:23; joined 08:56 at 60d2d6c3, a signed merge onto fb90df75, by the stack-A joiner; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** Four findings against the AWS work of 2026-10-04:
- **theseus-6hkx.** Found while preparing the runaway brake's live check (the AWS live checks, check 2, which could
  not run: the account has no hands stack). Runaway mode's refusal says "to change it, the operator raises
  hourly_alert_usd or runaway_factor under [aws.accounts.<id>] and restarts the daemon", but `runaway::current()`
  returned the mark while its period held without comparing it with the config, so after a raise and a restart the same
  group stayed refused until the hour or day ended.
- **theseus-snhr** (P2). The loader checks each `[policy.aws]` key's form (`read`, `service`, `service:Operation`) but
  not its names, since the catalog stays undecoded on the start path (the AWS design's §3.10). A typo
  (`"ec2:TerminateInstance" = "approve"`, `"cloudformaton" = "notify"`) loaded, never matched a call, and the call fell
  to its class's line or `enforcement`: a looser posture than the operator wrote, and with C2's writes one that could
  let a write run that was meant to wait.
- **theseus-rx7m.** R2's review of hands-network (Item 134) planted the removal of the blackhole
  check in subnet discovery and nothing caught it: the stateful fake's route tables had no blackhole route.
- **theseus-i7bz.** The paid live checks of 2026-10-04 (hands-cancel's Lambda part) found that
  `infra/aws/hand/build.sh` failed both its own checks on a good image: `file` says "static-pie linked" for Rust's musl
  target, which the `statically linked` grep rejected, and under `pipefail` the image run's pipeline answered with
  docker's status, not the grep's.

**What landed** (`theseus-core`'s `aws/`, the protocol's `AwsStatus`, the CLI's `render/aws.rs`, theseusd's
`aws.check` phase, `infra/aws/`; the merge 15 files, +565 −16; no new package, the catalog reached through the
existing `theseus_aws::catalog`; no store format change, no config key).
- **Runaway yields to a raised line** (6hkx, 9bb3df98). `runaway::current(store, &Account, now)` ignores a mark when
  the config no longer gives its line as little room: the line or the factor raised (line × factor past the mark's),
  or the line removed. `reached` then decides again, and if the figure is still past the new line a fresh mark is
  written; a lowered line keeps the mark. The rule is read at admission and by `observe` (health), never at the start,
  and needs no row. The session chose this over dropping the mark at the start, which would put a write on the start
  path.
- **The policy-key check** (snhr, ea31d70f). `aws/policy_keys.rs` checks each `[policy.aws]` key against the catalog:
  a misspelt service, an operation the service lacks, an alias (`states`, `monitoring`) or a case-loose operation
  (`s3:listbuckets`) is named, with why. A call matches by the catalog's canonical names, so an alias or a loose
  operation loads and never matches, which is why they are named too; an ambiguous name reads as no service.
  `Aws::check_policy_keys` runs on a `spawn_blocking` thread at the top of theseusd's `aws.check` phase task, after
  serving and before `check_all`. It keeps the unknown keys for `AwsStatus.unknown_policy_keys` (absent on the wire
  when empty; optional in protocol.gen's `AwsStatus.ts`), warns once per key in the log, and adds `policy_check_ms` and
  the keys to the phase's detail. The start never fails on one. Health's line, in `render/aws.rs`: `aws:
  [policy.aws] "a", "b" name no service or operation, so their lines never apply; the call falls to its class's line
  or 'enforcement'`. The session measured the check at 20.5 ms and RSS 17.2 to 22.4 MB (debug, 5 keys, three services
  decoded).
- **A blackhole route** (rx7m, f3169c92). The fake's `route_tables(unrouted, blackhole)` and `State.blackhole`, and
  `a_blackhole_route_is_no_way_out_and_the_subnet_is_refused`: the subnet is refused and named, no `RunTask`, one
  route read.
- **build.sh** (i7bz, e73ecb3e). `grep -qE 'statically linked|static-pie linked'`, and `out=$(docker run … || true)`
  before the grep. New `infra/aws/test/test_hand_build.py` (5 tests, stdlib only, a scratch git repo, stubbed
  `scripts/build.sh`, `file` and `docker`, never `--push`).
- theseus-protocol's `lib.rs` ceiling in `scripts/long-files.txt`: 2,727 at the join (below).

**How it is proven.**
- **The session's tests** (each step's alone, then one full gate over the tree; the four commits were made from it,
  not each gated): `hands/tests_runaway.rs`, 4 tests over accounts rebuilt from an edited config on the same store (a
  raised hourly line admits the same group; a raised factor admits it and `observe` finds none; a lowered line still
  refuses, with no second row; the day's line removed ends its mark); the policy-key tests (a typo'd operation and a
  misspelt service named and right ones not; an alias and a loose operation named; the status lists the keys and is
  absent when empty; health's line); the blackhole test; and the infra Python tests, 41 run, OK, 1 skipped. Its plants
  (runaway blind to the config: the three "ends the mark" tests fail and the lowered-line test passes; operations
  unchecked: 3 of 5 fail; the blackhole check dropped; the old grep: 3 fail; the old pipeline: 2 fail) each failed as
  it said.
- **The review** (R9, stacked on durability-fixes' review commit over main ef37f325; since the commits were not each
  gated, the merged tree was built and tested whole): fmt, the test build, clippy, protocol's 31 tests, the cockpit's
  65 tests and build, and shape clean; **481 of 481** selected tests (durability-fixes' selection, the CLI package,
  theseusd's `aws`, `versions` and `default_config` tests); the infra Python tests 41, OK, 1 skipped. **5 of 5 planted
  reverts caught**: runaway blind to the config (the three "ends the mark" tests fail, the lowered-line test passes);
  an alias taken as the service's name (`if false && entry.name != service`); a blackhole route counted as a way out;
  build.sh's old grep; the old pipeline under pipefail.
- **Live, with no AWS call possible** (R9, 06:06). A scratch daemon of the stack's debug build inside `unshare -rn` (a
  network namespace with only `lo` and no route), the account bound to the config's stand-in `endpoint` on a closed
  loopback port with invented keys, the stand-in model, Discord and the web off, and the report's three keys
  (`"ec2:TerminateInstance" = "approve"`, `"cloudformaton" = "notify"`, `"s3:ListBuckets" = "notify"`). The daemon
  served in 0.3 s; `--json health` listed `aws.unknown_policy_keys = ["cloudformaton", "ec2:TerminateInstance"]`;
  health had one `aws:` line naming those two and not s3; the log had one warning each (`no AWS service is named
  "cloudformaton"`, `ec2 has no operation "TerminateInstance"`); shutdown exit 0. The keys come in the config map's
  sorted order, not the file's.
- **FAST.** Nothing on the start path: the runaway rule reads the config at admission and in health, and the key
  check runs after serving, so the account check starts about 20 ms later. The lifecycle bench binds no AWS account,
  so the check never runs there; the stack's A/B is with durability-fixes' and wal-sync's (Item 176).

**The join** (stack A's second; R9's dry runs on b07150d6 and 1a08a40e, the joiner's on 3dba509d and, while queued, on
0a94e361 with kernel-fixes chained). Lock `cloud-aws-fixes-join` 08:17:25, ten seconds after durability-fixes' done
line, queued behind push-once and kernel-fixes, whose locks were older; the merge at 08:45:52 on kernel-fixes'
fb90df75. One conflict, `scripts/long-files.txt`'s theseus-protocol line: both sides had raised the `lib.rs` ceiling
from 2,709, main to 2,723 at task-board's join and the branch to 2,713. rerere replayed R9's resolution and
`aws-fixes/resolve.py` checked it: the line keeps main's reasons, takes the merged file's own count, **2,727**, and gains
one clause ("to 2,727 at cloud aws-fixes (theseus-snhr: AwsStatus's unknown_policy_keys)"). git merged the core's
`aws/mod.rs`, the protocol's `lib.rs`, the CLI's `render.rs` and theseusd's `main.rs` beside main's changes. The staged
files equal a fresh `merge-tree` plus resolve.py, byte for byte. No join fix. The warm (test build 2 m 20 s, clippy)
clean; then protocol's 31 tests (protocol.gen as staged), R9's selection 481 of 481, and the infra Python tests 41, OK.
The signed merge **60d2d6c3** (fb90df75 and 91efb51a), 08:50. Its gate (08:50:48 to 08:56:30, first take): **2,727 of
2,727** (1 slow, 20 skipped); lifecycle in every budget (cold start p50 25.4 / p95 35.6 ms; SIGKILL then restart 26.7 /
28.2; clean shutdown with a job 32.1 / 48.0; binary swap 47.6 / 51.2; the daemon's clock kernel 8.06 / 13.09, serving
20.67 / 27.10); L1 start p50 6.50 ms; turn frames 5 and 9 (plain p50 77.2 ms, tool call 151.4), fdatasync p50 6.6 ms.
Pushed 08:56; done line 08:56:51. theseus-6hkx, snhr, rx7m and i7bz closed with the hash, with notes naming the live
checks still owed. The store stays at format 19.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No key and no format change. Runaway mode now ends at a restart onto a raised
`hourly_alert_usd`, `daily_budget_usd` or `runaway_factor` within the same period, as the refusal's words promise; a
lowered line keeps the mark, and nothing changes unless a mark exists. The key check runs once after serving: a key
that names no AWS service or operation shows in health's `aws:` line and as one log warning each, with the looser line
the call falls to; if every key is right, nothing shows. The joiner did not read Eddie's config; after install #4
the journal held no warning, so no `[policy.aws]` key of his was unknown. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** The runaway rule is read at admission, not applied at the start. An unknown key is warned of, not
refused at the start: refusing would decode the catalog on the start path (about 20 ms and 5 MB); R9 recommended
keeping it, with a refusal, if wanted, as a config check before a restart. Ambiguity is not reported on its own; it
reads as no service. `unknown_policy_keys` is not `ts(optional)` on the Rust side (ts-rs rejects it on a `Vec`); it is
skipped when empty, like the other such lists, and optional in the generated type. No cockpit code reads it yet.

**Known gaps.** The AWS live checks wait for Eddie: runaway with a deployed hands stack and his go (a line low enough
that one small group enters runaway mode, a raised `hourly_alert_usd`, a restart within the hour, and the same group
runs; with theseus-ongv), and, optionally, the health line on the real account. `build.sh` was not run for real (a cold
`release-thin` musl build and a Docker Hub base image); the stubbed Python tests and two plants stand in, and a real
run is in R9's follow-up list.

### Item 175. `proc.run` takes steps: up to 16 programs judged as the strictest and run in turn to the first failure, with one result, a batch's steps in one class; and `fs.patch` recounts its hunk headers (theseus-7gir.3 and theseus-inw, with theseus-nrvq; the benchmark program's tool answer to batching; the seventh cloud batch's proc-steps session, fired 2026-10-05 01:35 from 80ef1dea, Opus 5.5; e92f5d04, 0be84690 and 32335372; reviewed 05:55 to 06:56 by local reviewer R10, stack P, and accepted with the stack at 07:28, theseus-nrvq's option 1 decided then as a join fix; joined 09:13 at d8ac9b54, a signed merge onto 60d2d6c3, by the stack-P joiner; store format 19 to 20; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** The benchmark program's rule (Eddie, 2026-10-04 15:52) is that a fix changes what the agent can do or see,
never instructions in the prompt, which retired B6's "batch shell steps" paragraph (theseus-n88g.6). Where round trips
cost Theseus, the tool's answer is a `steps` array on `proc.run`, described in its own schema (theseus-7gir.3). And
the dogfood pilot found `fs.patch` refusing a hunk whose header counts were off by one, a common slip in a model's
patch (theseus-inw).

**What landed** (`theseus-tools`, `theseus-core`'s `toolrun/`, the store's format, theseusd's tests; the merge 25
files, +1,921 −147; no new package; no system-prompt text added or changed).
- **`fs.patch` recounts** (inw, e92f5d04; theseus-tools' new `recount.rs`). Before diffy parses a file section, each
  `@@ -a,b +c,d @@` gets its lengths from its body (context on both sides, `-` on the old, `+` on the new, `\ No
  newline` on neither); the starts and the section heading after `@@` are kept, so diffy still checks the context and
  the removed lines against the file. A section whose headers all match comes back byte for byte; one with any
  recounted header adds a line to the result, `recounted 2 hunk headers in src/a.rs`. An empty line inside a hunk is a
  blank context line whose space was dropped; the empty lines that end a hunk are its end unless the header counts
  them. fs.rs grew 4 lines (2,297 at the merge, unlisted).
- **`steps` and the gate** (7gir.3, 0be84690). `proc.run` takes `steps: [{argv, cwd?, timeout_secs?}]` or `argv`,
  exactly one of the two, at most **16** (`proc::MAX_STEPS`, also the schema's `maxItems`; "steps holds 17 steps, more
  than 16: split the batch"). A step has no `env` or `sandbox`: those are the call's, for every step, and the call's
  `cwd` and `timeout_secs` are each step's default. The plan's summary lists every step whole ("run 3 steps in turn,
  stopping at the first that fails: 1. `printf one` in /w; 2. …"), its resources each distinct directory, and a new
  optional stored field, `Plan.steps`, holds each step's argv. The tool contract gains two defaulted methods,
  `Tool::steps` (each step as the call it would be alone) and `Tool::jobs` (each step's job, every directory checked
  before the first starts). The gate's `ToolRuntime::judge` (`toolrun/batch.rs`) runs each step through the place's
  refusal and the whole order, unchanged: a refusal of any step refuses the batch ("step 2 of 3 (`x`): …"), else the
  batch takes the strictest by (floor, posture), the floor over approve over notify over open, a tie keeping the
  earlier step, its reason and notice prefixed with the step, the grants joined. One approval binds the batch, since
  the proposal is the whole input, and a batch that differs in any step asks again. `external::Listed::of` reads each
  step's argv. The kernel's deadline is the steps' timeouts summed, plus 30 s. `policy.explain`, which explains a tool
  and not a call, carries a `steps` condition saying how a batch is judged, and `tests_explain`'s helper runs the same
  `judge`, so the test that holds explain to the gate holds the batch rule too.
- **The run** (7gir.3, 32335372; `toolrun/steps.rs`, beside `run_job`, whose launch is now its own function,
  `launch`, shared by one job and each step). Every step runs as a job under the call's **one** action and correlation
  id: a `/stop` or a cancel finds the running step's pid under that id, and `launch`'s check of the action starts no
  more steps once one came; the turn holds its job wait from before the first launch to the batch's end, so the drain
  never settles the call between steps, and the action stays `dispatched` across them. A step that exits 0 before the
  last is taken off the spool by the turn, and the next starts once the finished step's wrapper is gone (a race the
  session found and closed: the wrapper writes its completion before removing its pid file; the wait is at most 2 s,
  polled every 5 ms on tokio's timer). The step that ends the batch (the last; one that fails or times out; one a stop
  killed) settles the call as one job does, in one frame with the result. **One result:** the steps before as blocks
  (`[step 1 of 3: …]`, `[exit code 0, 1 ms]`, the end of the output), the ending step's block as one job's result is,
  then each step not run (`[step 3 of 3: …: not run]`); `meta.steps` holds a row per step. The steps before share at
  most a quarter of `result_max_chars`; the whole is then capped as one job's result. Past `proc_sync_secs` the running
  step goes on in the background as one job does, and the answer says the steps after it will not run. A restart
  mid-batch settles the call from the running step's own report; the steps after it never start. A batch counts once
  in the shell-fallback ratio.
- **One class per batch** (theseus-nrvq, join fix 2; below).
- **The store's format, 19 → 20**: `MANIFEST_FORMAT` 20, "/// 20 = a `proc.run` batch's `steps` on a tool call's
  plan" (the branch's own number was 18; main had reached 19). A format-17 `proc.run` tool call is the old layout's
  sample in `tests_layouts.rs`, labelled "at format 17 (unchanged from 3 through 19) … (20, theseus-7gir.3)".
  protocol.gen's `Plan.ts` gains `steps?`.

**How it is proven.**
- **The session's tests:** theseus-tools' 42 (with `recount::tests`, 4, and `tests_patch`, 7: counts off by one
  either way apply and say "recounted"; two hunks named once; right counts read as before; an empty line inside a hunk
  is context; a recounted hunk whose context does not match still fails and writes nothing; all-or-nothing across
  files); `tests_steps`' gate part (a batch takes its strictest step's posture; `op whoami` as step 2 makes the batch
  wait at the floor; the deadline covers every step) and run part (8 tests on the real wrapper: the brief's batch
  `printf one`, `false`, `touch never` gives one result, error, the marker absent, two `tool.job_started` rows under one
  correlation id; one approval runs every step and a changed batch asks again; a step past the wait goes to the
  background and the rest are not run; a stop during a step starts no more); theseusd's `tests/steps.rs` (a stop
  during step 2 kills it and starts no third; a `kill -9` of the daemon during step 2 settles the call `succeeded` from
  step 2's own report). Under load at nice 19, 5 runs, 13 of 13 each. Its plants (recount off: 7 fail; the context
  check skipped: 8; the gate judging only the first step: 3; a failed step not stopping the batch) each failed.
- **The review** (R10, stacked on push-once's review commit over main ef37f325): the build clean, the cockpit
  included; **1,513 of 1,515** on the whole stack, the two failures the header test (join fix 1, below; then 196 of
  196 in the confirming run) and a terminal test that passed 3 of 3 alone (theseus-ynia's family). **3 of 4 planted
  reverts caught** at exit 100 (the step after a failure runs; a batch judged by its first step; the recount off); the
  fourth, the floor no longer outranking a list's approve, was not caught: the code is right and the test missing
  (theseus-grms, P3).
- **Live** (R10, a scratch daemon of the stack's build, the stand-in model): at notify the brief's batch gave one
  error result naming each step, `never` absent, the plan's three argvs and `meta.steps`' three rows; at approve it
  parked with one card holding the whole input, and `theseus confirm` ran steps 1 and 2 and stopped, with one
  `tool.confirm_requested` row and two `tool.job_started` rows under one correlation id; `fs.patch` applied a
  short-counted hunk ("recounted 1 hunk header in notes.txt") and refused a wrong context, the file unchanged. R10's
  own probe found theseus-nrvq: `proc.run {"argv": ["true"]}` with `true` on `[sandbox] l1_argv` binds L1, but the batch
  `[true, printf two]`, with `printf two` on `approve_argv`, waited at approve with class L0, so the operator's yes
  would have run the L1 program at L0. R10 declined it.
- **FAST** (the whole stack against main, one hold, palindrome order): turn frames 5 and 9 on both, plain median p50
  100.9 against 101.0 ms, tool call 195.1 against 205.3 with one outlier run (by pairs +10, +133, −12, −26); the
  lifecycle's B − A within the noise (cold start +2.2 ms, the rest equal or lower). Nothing on the start path but one
  constant; the gate's `judge` adds one `steps` check for a call that is not a batch.

**What the session found.** The race between a finished step's wrapper and the next step's pid file (above). A
removed line `--- x` beside an added line `+++ y` still reads to `split_patch` as a file header and cuts the section;
the patch then fails and writes nothing (a test holds it; not widened). Jev's security input sees an empty `argv` for
a batch, its `steps` riding in the input's rest, and Discord's summary falls through to the input's JSON.

**The decision made for Eddie** (the DM thread, 07:28; morning notes 47; reversible). The goal is theseus-core's
written invariant, "Nothing falls back from L1 to L0". As built, a batch bound one job class, its strictest step's, so
an L0 step at approve or the floor carried an `l1_argv` step out of L1 on the operator's yes, and the card did not say
so. Of R10's three options, option 1 went in as join fix 2: a batch whose steps differ in class is invalid input,
refused before any card. It fails closed, needs no format change, and loses nothing that worked before proc-steps.
Option 2 (a class per step: a stored field and another format bump) and option 3 (L1 for the whole batch, which fails
an L0-only step) stay his for later.

**The join** (stack P's second; R10's dry runs on b07150d6 and e4d09068, the joiner's on 3dba509d, 38bb2924, fb90df75
and 60d2d6c3). Lock `cloud-proc-steps-join` 08:33:20, queued behind kernel-fixes and aws-fixes; merged at 08:57:03 on
aws-fixes' 60d2d6c3. Five conflicts, R10's: theseus-core's `lib.rs` (task-board's `tests_task_claims` beside
`tests_steps`), theseus-tools' `AGENTS.md` (lane files' PDF bullet, then the branch's two), and the format in
theseus-store's `store.rs`, the core's `store.rs` pin and theseusd's `tests/versions.rs`. rerere replayed R10's
resolution, already at 20, and `proc-steps/resolve.py` (which reads main's format from HEAD and renumbers to main's
plus one) found nothing left: the const and its doc line, the core's pin `assert_eq!(m["format"], 20, …)`, versions.rs's
four strings (writes 20, refuses 21, "is format 21", "formats 2 to 20") and the sample's label. The tool path
(`toolrun.rs`, `job.rs`, `late.rs`) merged clean beside tiering, task-board and lane files. **Join fix 1** (R10's
`joinfix.py`): `tests_m3::a_header_under_the_models_cache_minimum_gets_no_breakpoint` gave Haiku a cache minimum "the
prefix can never reach", 16,384 tokens, and with the `steps` schema on `proc.run` the header passed it on the merge, so
the test failed every run; the code is right and the premise moved, so the minimum went to 65,536 tokens with its doc
comment saying why. **Join fix 2** (theseus-nrvq, option 1; checked first in R10's finished tree): `judge` returns why
it turns a call away as `Turned::Place` (the place's refusal, as before) or `Turned::Invalid`, which the gate reports as
invalid input; each step's class is read from the `Bound` its order gave, with what chose L1, and `one_class` refuses a
batch whose steps differ: "Invalid input: step 1 of 2 (`true`) runs in L1 ([sandbox] l1_argv names `true`) and step 2
of 2 (`printf two`) at L0: a batch runs in one class; split it". A step's own plan error and an empty `steps`, which the
whole call's plan already refuses first, became invalid input too. `policy.explain`'s "steps" condition says the rule.
Its test, `a_batch_whose_steps_differ_in_class_is_refused_and_one_class_runs`, gets that result with no card, no
`tool.confirm_requested` row and nothing launched, then runs a batch whose steps share L1, each in L1; its planted
revert (`one_class` always passing) failed it in main's tree (the mixed batch waited at `awaiting_confirm`). The staged
tree equals the dry run's with all three scripts. The warm (test build 2 m 48 s, clippy) clean; before the gate 317 of
317 targeted tests, with `a_restart_with_a_step_running_settles_the_call` beside restart-notify's harness bind. The
signed merge **d8ac9b54** (60d2d6c3 and 8ab34f73). Its gate (09:05:10 to 09:13:36): **2,751 of 2,751** (1 slow, 20
skipped); lifecycle in every budget (cold start p50 22.6 / p95 29.9 ms; SIGKILL then restart 26.6 / 29.9; binary swap
52.3 / 95.4); L1 start p50 6.07 ms; turn frames 5 and 9, plain p50 81.4 ms and tool call 160.8 (inside the last six
gates' spread of 72.4 to 86.4 and 142.5 to 196.4, at the same fdatasync). Pushed; done line 09:13:59. theseus-7gir.3,
inw and nrvq closed with the hash; theseus-grms and vbju open. **Main's store format: 20.**

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). Eddie's store moved from format 16 to 20 at the new build's first write (20 is this
join's), one way, after the install's backup. No config key. A model can send `proc.run` with `steps`: up to 16 programs in turn, stopping
at the first failure, with one result and one card for the whole batch; a batch that mixes an `l1_argv` program with
an L0 step is refused as invalid input, and the model is told to split it. `fs.patch` recounts miscounted hunk
headers. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** The brief's schema step and gate step are one commit. `policy.explain` explains the batch rule as a
condition, not call by call. `Plan.steps` carries argv only; each step's directory is in the summary (a field would
need a new protocol type, and `ts.rs`'s list is at its line limit). The class rule came at the join, not in the
session.

**Known gaps.** R10's "For Eddie", each recommended as built unless he says otherwise: the steps after a background
step never run (the answer says so; a durable continuation would need each step's spec in the record and a launch from
the drain); a restart mid-batch settles from the running step alone (a `tool.step_finished` row only if an operator
needs the earlier steps); the approval card's reason names the strictest step, and Discord's card and the cockpit
should show the plan's summary beside it (Discord's `render.rs` is at its ceiling); the cockpit's call view should list
`plan.steps` and render `meta.steps`. The narrative's subject for a batch is its directory. A cancel landing in the
milliseconds between two steps would be marked `outcome_unknown`, not cancelled (narrow, untested). theseus-grms (P3).
`tests_m3`'s doc still calls the header "about 13 KB".

### Item 176. WAL sync: a failed sync's frames are cut back and never come back, followers meet a cut as a rewind, and a reopened log syncs its found segment's name once (theseus-ljgm and theseus-c67g; the seventh cloud batch's second part, its wal-sync session, fired 2026-10-05 02:35 from faaa9df6, Opus 5.5; 65e7d449, 9a086d12, e1a4eddf and 6ba717aa; reviewed 06:22 to 07:00 by local reviewer R9, stack A, with one FAST finding, and accepted with the stack at 07:23; joined 09:24 at d50e6f2f, a signed merge onto d8ac9b54, by the stack-A joiner; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** Two durability holes in the WAL (§6), each older than the step that found it:
- **theseus-ljgm**, found by the spine step S2 (2026-10-02). When the writer's `fdatasync` failed, every frame of its
  batch was answered with the error, but the frames stayed in the file, unindexed: later batches wrote after them and
  synced, and the next open replayed the failed frames as committed, so a caller told "failed" saw its record appear
  after a restart. And a later `fdatasync` can report success after the kernel has dropped the failed pages' dirty
  state (PostgreSQL's "fsyncgate", 2018), so even the frames after them might not be on disk.
- **theseus-c67g**, found while proving the WAL's synced mark (Item 92). theseus-xprd's rule is that a segment's name
  is as durable as its frames: the sync that makes a new segment's first frame durable syncs the log's directory too.
  That held while one process lived. If the process that created a segment died after writing to it and before that
  sync, the next open found the segment and appended to it, and `append_segment` returned no directory to sync, so the
  new process's syncs never synced the name until its next roll. On a filesystem that does not order a file's sync
  after its directory entry, a power loss in that window could lose the segment's name, and every frame the new
  process acknowledged there.

**What landed** (`theseus-store`'s `wal.rs` and its tests, `theseus-follow`; the merge 6 files, +759 −92; the frame
layout and the marks unchanged, so no store format change; no new package).
- **The found segment's name** (c67g, 65e7d449). `append_segment` returns the log's directory for a found last segment
  too, so the first sync after an open syncs it, once. The open itself still syncs nothing. That first sync is the
  kernel's startup frame's, before serving, on every start (the found segment is the usual case): one more directory
  fsync on the start path, which the session measured at 0.18 ms median on its VM's disk (0.59 p90, 4.1 max), and which
  cannot wait until after serving without either the startup frame not being reported durable before serving or being
  reported over a name a power loss could drop. theseus-store's AGENTS.md invariant and wal.rs's module docs say so.
- **A failed sync is cut back** (ljgm, e1a4eddf). When a sync fails, it cuts the last segment back to `Durable`, the
  end of the last frame a sync that returned Ok covered (segment, length, total, next position), then `set_len` and
  `sync_data` on the cut, and rolls the writer's length, total and next position back with it, so the next frame takes
  the first cut position, gapless. `synced` is untouched. **After a successful cut the log takes frames again:** the
  cut was synced, so the segment on disk is exactly what earlier good syncs made durable (where PostgreSQL panics
  because it cannot tell what it lost, here it is everything past the frontier, and gone); a disk whose errors persist
  fails each batch in turn, each answered failed and each cut, and never writes past a gap. A roll's failed `sync_all`
  of the segment it leaves is cut the same way, and no segment is created (unsynced frames are only ever in the last
  segment). The log goes **broken**, with the sync's error and the remedy in its message, in three cases: the cut or
  its sync fails; the cut is not in the segment written last (checked, though it cannot happen); or the open found
  frames past the last position known synced and no sync of this log has covered them yet (`found_unsynced`: their
  writer may have answered them Ok, so they cannot be cut, and no later sync may claim them; the first good sync
  clears it). The existing `broken` state for a torn write is reused. Two concurrency holes the brief did not name are
  closed: syncs run one at a time, holding `Wal::durable` from capture to answer (a writeback error is reported to one
  fsync per open file description, so two concurrent syncs on dup'd handles could see one Err and one Ok over the same
  lost pages), and a roll holds it too, the lock order `durable` before `w`; and `Writer::cuts` records each cut's
  first position, so a multi-caller `append` whose frame another caller's failed sync cut fails. The store's single
  writer thread never contends either, and a good sync pays one uncontended mutex and a copy of four numbers.
- **Followers meet a cut as a rewind** (9a086d12). A running follower checked nothing behind its cursor: when a cut's
  replacement frames were the cut ones' size, its offset could land on a frame boundary holding exactly the next
  position it expected, and it read on, silently missing the records that now held positions it had already read.
  After each read, `still_read` preads the 12-byte header of the frame the read began after and checks its length and
  crc against the cursor's mark; a cut before or during the read is `Rewound` (the index tender rebuilds, the
  durability tender ships again), whatever the read made of the bytes, a would-be `Corrupt` included. The cost is one
  open and one pread per read call. Positions are reused after a cut, so the index tender, which follows unbounded,
  depends on this check.
- **The writer's view** (6ba717aa): the test through the store's writer thread; the writer's code is untouched.
- Test hooks `fail_next_sync` and `fail_next_cut`, cfg(test), grouped with `cut_next_write` as `Writer::planted`.

**How it is proven.**
- **The session's tests** (`wal/tests/sync.rs`; the first five failed on main's code with only the hook added):
  `a_failed_syncs_frames_never_come_back` (on main the segment kept the batch; fixed, the next frame takes position 2
  and a reopen reads 1, 2, 3 with nothing cut); `a_later_good_sync_never_claims_a_failed_batch`;
  `a_failed_sync_at_a_roll_cuts_the_segment_it_leaves`; `a_failed_cut_leaves_the_log_broken`;
  `a_failed_sync_over_frames_the_open_found_unsynced_breaks_the_log`; `an_append_whose_frame_another_failed_sync_cut_fails`;
  the store's `a_batch_whose_sync_fails_is_answered_failed_and_never_comes_back` (three appends in one batch whose
  sync fails: each answered "a sync that failed", none indexed; the next answered `[2]`; a reopen holds exactly
  `(1, before), (2, after)`); c67g's `an_open_that_finds_its_last_segment_syncs_its_name_with_the_first_frame` and two
  updated directory-sync tests; the follower's `a_follower_that_read_frames_since_cut_meets_a_rewind` (on main the read
  returned Ok where a rewind was due). theseus-store's 74 tests passed 5 of 5 under load. The crash test
  (`theseus-sim crash-test --restarts 8 --writers 4`): 20 iterations and 50 iterations, OK, zero committed records
  lost. The session's plants (the cut skipped; `synced` advanced past a failed batch, where the reopen itself refuses,
  reading a later mark's claim on a cut position as rot; a failed cut not marking the log broken; the directory left
  out) each failed as it said.
- **The review** (R9, stacked on aws-fixes' and durability-fixes' review commits over main ef37f325): the build clean;
  **492 of 492** selected tests, both branches under it among them, durability-fixes' power-loss test beside the cut
  tests, and R9's two seam probes (below). **4 of 4 planted reverts caught**: the cut skipped (6 fail); the follower's
  check dropped; the same check dropped as the durability tender sees it (the seam: caught by probe 2 alone); c67g
  reverted (3 fail).
- **The seam with durability-fixes** (Item 171). The durability tender never ships a frame a
  later cut removes, by construction: a cut takes only frames past `Durable`; `sync` sets `Durable` and raises
  `synced` from the same capture under the same lock; the open sets `Durable` at the end it found, at or past
  `synced`; `synced` never moves back; and the tender ships only frames at or before `synced`. Probe 1 (a cut while the
  same shipper is held) follows on with no rewind and nothing sent again, S3's copy and a restore equal to the log.
  Probe 2 (a cut below the held position, written so the cursor lands on a boundary holding the position it expects:
  what a replaced log makes, not a failed sync) fails the pass with "the WAL was replaced", starts again from the log's
  start, catches up, and a restore equals the log.
- **Live** (R9). The crash test with the stack's frozen binary: **50 iterations × 8 restarts, 4 writers, tear on,
  96.9 s, zero committed records lost** (131,950 torn bytes removed, 2,286 records durable but unreported, 3 indexes
  moved aside). strace at a restart after SIGKILL, on scratch daemons: main's build syncs the log's directory **0**
  times in the second start; the stack's **exactly once**, right after the startup frame's `fdatasync` and before any
  other segment's.
- **FAST: one finding** (R9's A/B, main ef37f325 against the whole stack, debug, frozen, A B B A in one hold, on a
  busy disk: IO PSI some 26 to 35 %, fdatasync p50 14 to 20 ms). Turns: 5 and 9 frames, B equal or faster in each
  pair. The start: the daemon's own clock put B's kernel phase **+12.5 ms in both pairs** (A 19.4 and 16.1, B 31.9 and
  28.6) and time to serving +6.5 and +12.9: c67g's one directory sync, a second sync wait before serving, about 12 ms at
  that latency and 0.18 ms on a quiet disk. Filed as **theseus-3q29** (P2), with the session's cheaper rule.

**What the session found.** The two concurrency holes and the follower's silent miss (above). And one half it left:
a failed index write after a good sync still fails every frame of its batch though they are durable, and they come
back at the next open (theseus-lu5n, P3: answer them Ok once indexed, with a bounded retry, then broken until a
restart; cutting synced frames would move `synced` back, which the marks forbid).

**The join** (stack A's third, after durability-fixes, which it conflicts with; R9's dry runs on b07150d6 and
1a08a40e, the joiner's on 3dba509d, 0a94e361 and d8ac9b54). Lock `cloud-wal-sync-join` 08:56:51, queued behind
proc-steps; merged at 09:14:04 on proc-steps' d8ac9b54. One conflict, theseus-follow's `lib.rs`, two blocks,
keep-both: durability-fixes had moved `read`'s body into `read_upto(max, upto)` and split out `segment()`, and this
branch had wrapped `read`'s body with `still_read`. rerere replayed R9's resolution and `wal-sync/resolve.py` checked
its placement: `read_upto`'s match reads `read_into(max_bytes, upto, ..)` and then `still_read(&began)`, so `read`
(the index tender) and `read_upto` (the durability tender) both make the rewind check. git merged theseus-follow's
`tests.rs`. The staged files equal a fresh `merge-tree` plus resolve.py, byte for byte. No join fix. The warm (test
build 2 m 40 s, clippy) clean; R9's selection with theseusd's `mcp_server` binary added, **494 of 494**. The signed merge
**d50e6f2f** (d8ac9b54 and 612c05f7), 09:18. Its gate (09:19:01 to 09:24:34, first take, IO PSI some avg10 3.56 at its
start): **2,760 of 2,760** (1 slow, 20 skipped), wal-sync's tests, the follower's rewind test and durability-fixes'
bounded read among them; lifecycle in every budget, **cold start and SIGKILL-restart included**, the brief's watch on
c67g (cold start p50 22.8 / p95 25.0 ms against 50 + 7; SIGKILL then restart 30.3 / 31.0 against 150 + 25; clean
shutdown with a job 35.4 / 53.8; binary swap 53.2 / 56.0); the daemon's kernel phase p50 8.76 / p95 10.03 ms, against
8.06 / 13.09 at aws-fixes' gate and 9.83 / 10.94 at durability-fixes', at fdatasync p50 6.4, 6.6 and 7.7 ms, so at that
latency the one directory sync did not show above the gate-to-gate noise; serving p50 20.03 ms; L1 start p50 6.56 ms;
turn frames 5 and 9 (plain p50 80.3 ms, tool call 154.1). Pushed 09:24; done line 09:24:53. theseus-ljgm and c67g
closed with the hash. Main's store format stays 20 (proc-steps'). After the join, one more A/B for theseus-3q29, on a
disk that turned busy within a minute: the kernel phase B − A +13.9 and +6.1 ms, to serving +19.4 and +4.4; with R9's,
two busy-disk pairs put c67g's cost at +6 to +14 ms, and a quiet number is still unmeasured.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No key and no format change. Every start syncs the log's directory once, with the
startup frame's sync: one more sync wait before serving, larger while the disk is busy. A failed `fdatasync` (a disk
error) now cuts its batch back off the log, and its callers' "failed" stays true after a restart; if the cut itself
fails, the log goes broken until a restart. Nothing changes on a healthy disk. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** The brief's design, with the log taking frames again after a successful cut rather than staying
broken until a restart (one line in `cut_back` if Eddie prefers the other). The two concurrency fixes were not in the
brief. The follower's check wraps `read` and touched no line of durability-fixes' bounded read; the join put it inside
`read_upto`.

**Known gaps.** **theseus-3q29** (P2, FAST): skip c67g's directory sync when the open already knows a position in the
found segment was synced (the index's checkpoint, or a mark, at or past its first position), which makes a clean
restart free and leaves only a start after a crash to pay; its one hole is a start after an upgrade from a build that
synced frames without the name, which ext4's ordered journal covers anyway. R9 recommended building it. _(Built 2026-10-06, Part III Item 196, with the close for an upgraded store; its one hole, a manifest moved before the first frame's sync, is theseus-xva3.)_ **theseus-lu5n**
(P3), above. R9's other recommendations, keeping the code as built: the log taking frames after a cut, plus a health
count of cut batches ("the WAL cut N failed batches since start", not filed; Eddie's call); and the strict
`found_unsynced` rule. fsync off (tests and benches only) still syncs at a roll, so a failed roll's sync there cuts
frames answered without a sync; no daemon path reaches it.

### Item 177. Health words: a `web:` line, the 1-hour cache write priced and counted, the cockpit's binary card, and one row and one notice when free space crosses a line (theseus-jxau, theseus-4v1z, theseus-od13's binary half (V6) and theseus-f337 (V10); the seventh cloud batch's health-words session, fired 2026-10-05 01:35 from 80ef1dea, Sonnet 5.5, finished 02:37; de5e779f, 80affc3e, 56509918 and 7b36f314; reviewed 07:59 to 08:48 by local reviewer R13, stack H, and accepted with the stack at 09:36; its join held for Eddie's account switch and the disk compaction; joined 11:47 at 319b95b0, a signed merge onto d50e6f2f, by the stack-H joiner; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** Four small gaps between what the daemon knew and what the operator was told:
- **theseus-jxau.** Health's web section (the refusals by kind, the dev origin) was JSON only since the hardening
  lanes of 2026-10-01; neither the CLI's text nor the web UI showed it.
- **theseus-4v1z.** The cache2 lane had added a fifth price, `cache_write_1h_per_mtok`, and a 1-hour count,
  `cache_creation_1h_input_tokens`, but `theseus catalog` still showed four prices and health's usage line no 1-hour
  writes.
- **theseus-od13.** Lane hardening2 had added two health facts, `approval.open` and `binary` (whether the daemon's jobs
  can write the binary it runs, Review 2's consideration 3); the CLI printed both, but the web apps showed neither.
  The approval half went with theseus-zmgb; the binary half is v1.1's V6.
- **theseus-f337** (v1.1's V10). Health's `disk:` line (theseus-102) warned below `[server] disk_warn_mb` and refused
  jobs below `disk_floor_mb`, but a crossing told no one: nothing was written and no one was told when free space fell
  or came back.

**What landed** (the CLI's `render/`, the cockpit, `theseus-core`'s disk watch, three ledger kinds, theseus-discord's
courier; the merge 28 files, +762 −33; no new package, no protocol type beyond the three kinds' strings, no config key,
no store format change). The branch's `CLOUD_REPORT.md` is empty: the session's temp filesystem was full when it wrote
it, and it committed the empty file; its final message survived, and R13 reconstructed the rest from the commits.
- **`web:`** (jxau, de5e779f; `render/web.rs`'s `web_line`). Health always has one `web:` line: `web: ok` when nothing
  was refused and the dev origin is off; otherwise `refused host H, origin O, peer P` and `dev origin X (served N)`;
  and `OWNER CHECK OFF (why): a local process of any user is served` goes first, under the Bad tag. The three health
  goldens gain `web: ok`.
- **The 1-hour cache write** (4v1z, 80affc3e). `theseus catalog`'s table moves from `cmd.rs` into `render/catalog.rs`
  and gains a `$c.1h` column (`-` where a model has none). Health's tokens line ends `cache-write N (1h M)`, the
  `(1h M)` only when M > 0. The three catalog goldens change.
- **The binary in the cockpit** (od13, V6, 56509918). `lib/binary.ts` (the tone, and the CLI's `binary:` words) and
  `BinaryCard.tsx`: a fault card at the top of Systems only when jobs can write the binary, a pill in the Daemon card,
  and a `binary` dot in the header between `web` and `disk`, with a test of all three states.
- **The disk's crossings** (f337, V10, 7b36f314). `Disk::crossing` holds the last state in a mutex: falling takes
  effect at once; rising counts only past a margin, `max(line / 20, 32 MB)`; an unreadable disk is no crossing; a
  first read that is ok says nothing, and a first read that is not ok is a crossing from nothing (`left: null`). Each
  crossing of either line, or the way back, writes one `disk.low`, `disk.below_floor` or `disk.ok` row (free and total
  MB, both lines, the state it left; `fact/disk.rs`), one operator notice (kind `disk`, which theseus-discord's courier
  words and posts on the operator's channel: an owner's bound DM, else the notice's place) and one WARN line
  (`rpc/disk_watch.rs`'s `watch_disk`). The heartbeat calls it on the `"timer"` beat only: one `statvfs` and a mutex
  every 60 s, after serving. The courier's `plan` passed clippy's 100 lines, so its disk arm is a method of its own,
  `disk_post`.

**How it is proven.**
- **The session's** gate (fmt, shape, features, clippy, cockpit, test build, reader rule; the suite failing only the
  33 known L1 tests on the cloud's root VM) and three planted reverts, each caught. It ran nothing under load and did
  not drive the courier through a fake Discord.
- **The review** (R13, on main 3dba509d, review commit 341b9848; clean merge, no join fix): fmt, the test build,
  clippy, protocol's 31 (protocol.gen unchanged), the cockpit's lint, 86 tests and build, and shape clean; **333 of
  333** tests (the branch's own, the whole CLI crate with its goldens, theseus-discord's 122, theseus-protocol's, and
  the core's disk, registry, heartbeat, harness, driver and outbox tests). **6 of 8 planted reverts caught**: the margin
  removed (`left [low, ok, low, ok, low, ok]` against `[low]`), a row and a notice on every beat that is not ok, the
  watch on a wrapper's notify beats too, `web:` reading ok with the owner check off, the `$c.1h` prices dropped, and the
  cockpit reading a writable binary as ok. Not caught, test gaps with the code right: health's tokens line without its
  `(1h M)` (theseus-xiaz, P2) and the courier's disk arm gone (theseus-8phq, P2).
- **Live** (R13; a scratch daemon of the merged build inside `unshare -rm`, its state dir a 512 MB tmpfs, the Discord
  rig's fake Discord and invented secrets, a stand-in Messages API whose usage splits its cache writes, heartbeat 1 s,
  `disk_warn_mb = 300` and `disk_floor_mb = 150`, each margin 32 MB). Health read `web: ok`, then `web: refused host 1,
  origin 0, peer 0` after one GET with a foreign Host (answered 403), and `cache-write 1200 (1h 300)` after one turn;
  the loud owner-check line cannot show on Linux (the web UI sets `peer_unchecked` only off Linux), so the plant and the
  unit test hold it. `theseus catalog` showed the `$c.1h` column. The disk, each row within one 1 s beat of the move:
  512 MB, nothing; 280, `disk.low` (left ok); 315, 290 and 325, inside the margin, nothing; 120, `disk.below_floor`
  (left low); 160, 140 and 175, nothing; 200, `disk.low` (left below_floor); 345, `disk.ok` (left low); steady,
  nothing: exactly four rows, and four posts on the fake Discord, one per crossing ("Free space under the state dir is
  low: 279 MB free of 512 MB (health warns below 300 MB).", "… is below the floor: … New jobs are refused, and running
  ones are stopped.", "… is back over the floor, but still low: … Jobs run again.", "… is back to normal: 344 MB free of
  512 MB."). The cockpit, in headless Chrome: the header's `binary` dot, the fault card "Binary · jobs can write it", the
  Daemon card's pill, no console errors.
- **FAST** (R13, both of stack H against 3dba509d, debug, one hold, a quiet machine): turn frames 5 and 9, plain p50
  76.4 against 76.8 ms, tool call 160.5 against 159.5; every lifecycle phase equal or quicker on the stack (cold start
  22.5 against 20.4 ms, SIGKILL restart 26.4 against 24.8), no budget missed. Level.

**The join** (stack H's first; R13's dry runs on 38bb2924, 60d2d6c3 and d50e6f2f clean; held from 09:36 by Eddie's
08:25 ask, finish what is in flight and start nothing until his account switch and the disk's compaction, which ended
at 11:20; the joiner spawned at 11:29). Lock `cloud-health-words-join` 11:31:06; the merge at 11:31:14 on wal-sync's
d50e6f2f: no conflict, no script, no join fix. By hand: courier.rs (lane speed's outbox, the disk arm in `plan`'s
match returning speed's `Plan`), Shell.tsx (cockpit-tabs') and the six CLI goldens are byte-identical to R13's
reviewed merge; of the 28 files only four differ from it, by main's own later edits (the core's `lib.rs` and
`rpc/mod.rs`, driver.rs's `mark_earlier_calls` from kernel-fixes, a render.rs test's `..Default::default()` from
aws-fixes). The disk watch runs on the `"timer"` beat only, and the harness's first tick is restart-notify's
`heartbeat("bind")`, so the first check still comes one period after serving. The warm (test build 2 m 07 s, clippy)
clean; then protocol's 31, R13's selection 334 of 334, shape ok, every ceiling held (the CLI's render.rs 3,098 of
3,100). The signed merge **319b95b0** (d50e6f2f and 0428d910). Its gate (11:36:16 to 11:46:49, first take, 247 s of it
waiting for the shared lock behind another reviewer's cold build): **2,768 of 2,768** (1 slow, 20 skipped); lifecycle in
every budget (cold start p50 23.0 / p95 31.2 ms; SIGKILL then restart 26.5 / 34.8; clean shutdown with a job 30.8 /
48.3; binary swap 46.4 / 49.3; the kernel phase p50 8.09 ms at fdatasync 6.5), level with wal-sync's gate; L1 start
p50 6.32 ms; turn frames 5 and 9 (plain p50 75.0 ms, tool call 150.8), resident 68.9 MB after the start and 96.3 MB
after 30 turns. Pushed; done line 11:47:16. theseus-jxau, 4v1z and f337 closed with the hash; theseus-od13 closed in
part, its binary half here and its approval half with theseus-zmgb. The store stays at format 20.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No config key: the disk lines' keys exist, and a crossing now writes a row and tells
the operator. Health gains its `web:` line and the `(1h M)` count; `theseus catalog` its `$c.1h` column; the cockpit's
Systems view its binary card, pill and dot. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** A switched-off web UI reads `web: ok`, since health has no field for it. The cockpit shows the
binary quietly unless jobs can write it. `disk_post` split out of the courier's `plan` for clippy. The session split
render.rs's change across commits, so de5e779f or 80affc3e alone may not build; the join is one merge.

**Known gaps.** **theseus-83y0** (P3), the one Eddie should know first (morning notes 50): under WSL the disk line and
the new crossings read the virtual disk (about 650 GB free on this machine), not the Windows drive, which fills first
(24 to 43 GB free that morning), so the notice cannot fire for the failure this machine actually has; R13 recommended
it before relying on the notice here. A switched-off web UI should read `web: off`, through an `enabled` field on
`WebStatus` the next time health's protocol changes. The cockpit's Ledger and Activity show a disk row by its first
fields in the idle tone; a `summary.ts` case and the fault tone for `disk.below_floor` go with its next change.
theseus-xiaz and theseus-8phq (P2), the two test gaps. The disk lines' defaults (warn 5,120 MB, floor 1,024, margins
256 and 51 MB) stay, as R13 recommended.

### Item 178. Telemetry of calls: a failed provider call's time and span carry `error.type`, and AWS requests are counted and timed by service, operation and outcome (theseus-lmhp and theseus-ku5f; AWS design §3.8; the seventh cloud batch's second part, its telemetry-calls session, fired 2026-10-05 02:35 from faaa9df6, Sonnet 5.5; 251fdb37 and 20ac831f; reviewed 06:57 to 08:40 by local reviewer R12, stack Y, and accepted with the stack at 08:52; its join held for Eddie's account switch and the disk compaction; joined 12:02 at 4d7cd561, a signed merge onto 319b95b0, by the stack-Y joiner; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** Two holes in the metrics (§3.20, the GenAI names):
- **theseus-lmhp**, from the telemetry lane. `theseus.provider.call.duration_ms` mixed failed calls with answered
  ones: a call closed by `ModelCallFailed` carried its class on its span's `error`, but its time went into the answered
  calls' series, so a burst of fast failures read as a faster model.
- **theseus-ku5f.** The AWS design's §3.8 asked for calls counted and latency measured by service, operation and
  outcome; C1 (row 29) left it, and AWS requests had spans (`kind = "aws"`, with `rpc.service`, `rpc.method` and
  `status`) but no metric.

**What landed** (`theseus-core`'s `telemetry/`; the merge 7 files, +632 −9; no protocol type, config key, package or
store format change).
- **`error.type` on a failed call** (lmhp, 251fdb37). `spans.rs` gains `semconv::ERROR_TYPE`, `error_type()` (the
  provider span's `error` text, the class `ModelCallFailed` closed it with) and `ProviderCall.error_type`; `gen_ai`
  adds `error.type` to a failed call's span, and `metrics.rs`'s `provider_calls` adds it to
  `theseus.provider.call.duration_ms`, so a failed call is its own series per class beside the answered ones.
  `failed()`'s ERROR status and the flattened `error` attribute are unchanged. Three rules: a call a `/stop` cut has
  class `stopped`, its own `error.type`, since its time is cut at the stop and among the answers it would pull their
  percentiles down; a refusal closes through `ModelAnswered` (`stop_reason` `refusal`) with no `error`, so it is an
  answer and gets none, and its fallback is a second provider span on the fallback's model; and
  `theseus.provider.first_token_ms` carries no `error.type` (recorded before it is added), since a first token that
  arrived before a mid-stream failure is a real time to first token, and the model keeps one series.
- **The AWS metrics** (ku5f, 20ac831f). `theseus.aws.calls` (an int sum) and `theseus.aws.duration_ms` (a ms
  histogram) join `INSTRUMENTS`. `aws_requests` walks the whole trace for `kind = "aws"` spans, as `lsp_requests` does,
  from both `turn` and `failure`, with the attributes `rpc.service`, `rpc.method` and `theseus.outcome` (the span's
  `status`: `ok`, `unbound` or `error`; `unknown` when absent; the same name `lsp.request` and `file.read` use). The
  AWS error code is not an attribute: it is the service's own text, unbounded, so it stays on the span and the
  `aws.called` row, and `theseus.outcome = error` counts the failures. Service and operation are bounded because
  `aws.call`'s plan checks them against the catalog before any request (the session read that path, not every other
  `Request` constructor).
- **The tests' pictures.** The old exporter's golden (`testdata/old-exporter.json`) had one duration point (count 3,
  one model); it is now two, count 2 with no `error.type` and count 1 with `rate_limited`, the point the task meant to
  move. The old exporter never wrote `error.type` on a span, so `tests.rs` compares the span picture without that one
  attribute (`without_error_type`). The core's output golden pins no metric and is unchanged.

**How it is proven.**
- **The session's tests:** `telemetry/tests_calls.rs`, 5 (two answered calls and one `rate_limited` give two series,
  the failure's with `error.type`, a count of 1 and 150 ms; a refusal and its fallback carry none, on the metrics and
  the spans, with no ERROR status; a `stopped` call is its own series; a failed turn's calls split the same way; the
  span carries it) and `telemetry/tests_aws.rs`, 3 (two `DescribeStacks`, one `AccessDenied`, and an unbound
  `GetCallerIdentity` give three series, each counted once and timed; a failed turn's AWS calls count the same; a
  request under a continuation's span is counted). The telemetry suite 43 of 43; its plants (the duration's
  `error.type` dropped: 4 fail; a refusal given one; a walk that does not recurse: 3 fail; `unbound` counted as `ok`)
  each failed.
- **The review** (R12, on main e4d09068, review commit c49379d9): fmt, the test build, clippy, protocol's 31 tests
  (protocol.gen unchanged) and shape clean; every theseus-core test, `--retries 0` at load about 23, **1,194 of 1,194**;
  on the whole stack with telemetry-resumed, the workspace suite **2,706 of 2,706** at load about 30. **5 of 5 planted
  reverts caught** at exit 100: `error.type` off the duration; an `unbound` request counted as `ok`; the failure
  path's AWS walk removed; the span's `error.type` dropped; the first token given `error.type` too.
- **Live** (R12, a scratch daemon of the stack's build as a transient user unit, the stand-in model, a loopback OTLP
  sink, `metrics_interval_secs = 2`): a turn the stand-in answered, then, with the stand-in stopped, a turn whose call
  was refused ("turn failed (network)", exit 1, in 0.1 s). `theseus.provider.call.duration_ms` held two points for the
  model, `{error.type: network}` count 1 (3.8 ms) and one with no `error.type`, count 6; `first_token_ms` one series
  with none; `theseus.provider.errors` `network` 1; and the failed `provider.call` span in `/v1/traces` carried
  `error.type = network` with ERROR status. The AWS half stayed offline (no AWS call, no account bound); tests_aws and
  plants 2 and 3 stand in.
- **FAST** (the whole stack against e4d09068, one hold, palindrome order): turn frames 5 and 9 on both, plain median
  p50 103.3 against 87.5 ms, tool call 225.5 against 216.2; every lifecycle phase's B median equal or better. One more
  trace walk per turn's end (beside the eight already there) and one attribute per failed call; nothing on the start
  path and nothing a turn waits on.

**What the session found.** `telemetry::tests::a_failed_continuation_is_counted_as_a_failed_turn_is` failed under
load (4 traces for 3) on its base too, not this branch's: already theseus-qjd6 (P3), filed by lane speed at 01:39 (the
exporter's 2 s post timeout and one retry under load), with this session's three failures appended to it.

**The join** (stack Y's first; R12's dry runs on e4d09068, 3dba509d and 38bb2924, the joiner's at 11:29 on d50e6f2f
and at 11:41 on health-words' merge). Lock `cloud-telemetry-calls-join` 11:31:57, queued 51 s behind health-words';
merged at 11:47:33 on 319b95b0. One conflict, `telemetry/metrics.rs`'s `INSTRUMENTS`, two blocks, keep-both: main's
`ACTIVATE`, `ACTIVATED`, `NODE_CACHE_BYTES` and `NODE_CACHE_READS` (batch 6's), then the branch's `AWS_CALLS` and
`AWS_DURATION`. rerere replayed R12's resolution, and `telemetry-calls/resolve.py` found it resolved: it sets the
array's declared length to its entries' count, **34**, whether or not the line conflicts (the `[T; N]` trap, where two
sides raise the length by the same edit and the line merges clean but under-declared). `self.aws_requests(t)` sits on
the turn path beside activation's `activations(t)` and on the failure path. All 7 staged files equal the dry run's
tree. No join fix. The warm (test build 2 m 57 s, clippy) clean; then the core's `telemetry::` and `aws::` tests and
the output golden at TZ=America/Phoenix, 180 of 180. The signed merge **4d7cd561** (319b95b0 and 64fe9b36), 11:53. Its
gate (11:56:33 to 12:01:59): **2,776 of 2,776** (1 slow, 20 skipped); lifecycle in every budget on the first run
(cold start p50 23.7 / p95 40.7 ms; SIGKILL then restart 28.1 / 38.9; clean shutdown 35.3 / 54.9; binary swap 49.6 /
66.2); L1 start p50 7.46 ms; turn plain 5 frames p50 81.3 ms, tool call 9 frames p50 155.8 (health-words' gate before
it: 75.0 and 150.8, within the run-to-run spread), fdatasync p50 6.6 ms. Pushed; done line 12:02:23. theseus-lmhp and
ku5f closed with the hash, ku5f's naming the live AWS check still owed. The store stays at format 20.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No key and no format change. An OTLP collector sees failed provider calls as their
own `theseus.provider.call.duration_ms` series per `error.type`, and two new instruments for AWS requests, once an
account is bound. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** `stopped` is an `error.type` of its own rather than left among the answers; R12 recommended keeping
it and naming it in §3.20 as a cancellation, so an error-rate panel leaves it out (`theseus.provider.errors` is a
separate counter, its rate unchanged). The first token carries none. The AWS code is kept off the metric; a per-code
count, if ever wanted, would map codes to a few bounded classes (throttled, denied, not found, other).

**Known gaps.** The live AWS check waits for Eddie: with an account bound, a turn that calls `aws_whoami` should give
one `theseus.aws.calls` point (`rpc.service = sts`, `theseus.outcome = ok`) and its duration; an unbound
`GetCallerIdentity` gives `unbound`, and a refused `aws_call` `error`. theseus-qjd6 (P3), the load-flaky test above.

### Item 179. `theseus judge prove`: the prove's records built from the daemon's ledger, one per finished task, the generator run over them, and classify.v1 held against the model's own `task_create` (theseus-0j2.18; roadmap-v2 row 50, L3's join, M5 §2.9's prove; the seventh cloud batch's prove-wire-in session, fired 2026-10-05 01:35 from 80ef1dea, Opus 5.5; e83d87b0, b133c40c, c59813e0, c92b7bea and 9d54a5ab; reviewed 08:27 to 09:31 by local reviewer R13, stack H, and accepted with the stack at 09:36; its join held for Eddie's account switch and the disk compaction; joined 12:34 at 19a768a3, a signed merge onto 4d7cd561, by the stack-H joiner and, after the account's weekly limit stopped it, the DM thread; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** The prove's generator joined with judge-prove (Item 101): theseus-judge's `prove.rs` and
the `theseus-judge prove` binary, over JSONL records of finished tasks. The one-command wire-in did not: nothing built
those records from the store, and the CLI had no `theseus judge prove`. Row 50 builds the records from the ledger as M5
§2.9 defines them (each arm's tasks, success, false completion, spend with its judge calls, turns, nudges, stops) and
runs the generator from one command. Until 26b's canary has run, the report honestly says "insufficient", with its
counts.

**What landed** (`theseus-core`'s `learning/prove.rs` and `rpc/judge_prove.rs`, the protocol's `judge_runs.rs`, the
CLI; the merge 18 files, +1,909 −4; no new package, no store format change, and the method writes nothing; theseus-
protocol's `lib.rs` gains no line, `JUDGE_PROVE` sharing `JUDGE_REPLAY`'s line).
- **The records** (e83d87b0; `build` is pure over the rows it is handed). One record per `task.ended` row, an
  execution opened by `task.create`; a second row naming the same task counts once. `task.closed` makes and changes no
  record (its report close repeats the same end). A task opens no tasks, so `turns` (its session's `turn.ended` rows)
  and the spend are its own. **Who pays:** every judgment today, canary included, is paid from the judge's own day
  budget (the sink writes `budget: "shadow"`), so `spend_micros` is the execution's `spent_micros` plus the judge
  calls whose row says `shadow`, and `judge_micros` is the `cost_micros` of every `judge.call` naming the session; if
  26b moves canary judgments onto the execution (§2.6), the record already skips non-`shadow` rows, so nothing counts
  twice. **The arm** is the `pack_arm` of the session's `loop.v1` judgments; a task is left out, counted by reason, as
  `never_judged`, `no_arm` (only `all`, outside a canary or before 26a), `both_arms`, `cancelled` (a cancel or `/stop`
  ended it) or `unreadable`. **Stops:** one per judgment in the task's arm, control's the baseline's (`no_tool_calls`
  stops), the canary's Jev's by §2.8a's rule, else the baseline's; until 26b builds the nudge the turn ends either way.
  `should_stop` is the resolved `work_state` label (`complete` true; another class or `not:complete` false; a label
  that only rules out another class, `not:progressing`, `null`). `false_completion` is read at the last judgment where
  something called the task complete and the acting decision was stop. **Success** is false on a `failed` or
  `budget_exhausted` execution or when the last resolved `work_state` says not complete (the system's near-identical
  rule, a continuation re-ask, an audit, an operator's label, resolved heaviest first, so an operator's `complete`
  outweighs the system's rule); true once a learning run has read the 24 hours after the task's end
  (`learning.last_run.at_unix_ms >= ended + FALSE_COMPLETION_MS`), since the system label is written only by that run;
  `null` until then. `nudges` and `unnecessary_nudges` are 0 until 26b, and the method says so in a note.
- **The method** (b133c40c). `judge.prove { since?, until?, min_tasks?, min_labeled?, records? }` builds on the
  blocking pool and runs `theseus_judge::prove::prove`, answering the generator's JSON `report`, its `markdown`, the
  verdict, arms, left-out counts, tasks, notes, the window and its time, and the JSONL `records` when asked. It reads
  `task.ended` by kind tag within the window, a page at a time, and per task its execution, its session's `judge.call`
  and `turn.ended` rows by kind and session, `judge:loop`'s scope and `learning.last_run`. The default window starts
  at loop.v1's latest non-declined move to canary, read straight from `pack:loop`'s scope: the first version took it
  from the ladder, whose first load on a daemon with the judge off writes the adoption table's three `pack.mode` rows,
  so a read wrote; now it writes nothing. `--until` is the whole local day, DST-safe through `local_midnight`.
  `dispatch` was at clippy's 100 lines, so one arm serves both learning-ledger reads, `m @ (LEARNING_REPORT |
  JUDGE_PROVE) => self.rpc_ledger(…)`. 10,000 tasks read in 1,837 ms (debug, an ignored test).
- **The CLI** (c59813e0). `theseus judge prove [--since D] [--until D] [--min-tasks N] [--min-labeled N] [--records
  <path|->] [--json]`: stdout is exactly the generator's Markdown, so `theseus judge prove > a.md` equals
  `theseus-judge prove <file> --markdown -`; stderr says what the daemon read (the window, records by arm, left out by
  reason, the classification line, the note, the time). It is a read, so it is not in `OPERATORS`.
- **Classification** (c92b7bea). `classify_quality` holds classify.v1's lean on `should_promote` against its
  baseline, the model's own `task.create` in that turn, read from the system's `task_create` label; the truth is
  operator and audit labels only (the system label is the baseline itself); the verdict is McNemar's test on the
  discordant pairs at 95 % (`jev_better`, `baseline_better`, `no_difference`) or `insufficient (compared n of min)`.
  `kind` is not compared: its baseline makes no class.
- theseus-core's AGENTS.md names the prove (9d54a5ab). Protocol types `JudgeProveParams`, `JudgeProveResult` and
  `ClassifyQuality`, with three generated files.

**How it is proven.**
- **The session's tests** (`tests_prove.rs`, 8, over a store seeded with ten invented tasks): every field of six
  records in both arms and the four left-out reasons; an open window is no outcome yet; the records file gives the
  same report; known cohorts give exact rates (40 + 40 tasks: completion 0.9 and 0.6, completions per USD 2.25 and
  1.2, `canary_better`; a control arm of 12 is "insufficient", "labeled tasks: 12 of 30"); judge spend counts in the
  whole; the method is the generator over its records, byte for byte, with `last_position` unchanged; the default
  window is the canary's; classification against the model's own `task_create`. Its plants (judge calls left out of
  the spend: 4 fail; an open window read as success: 3; the arm read from another pack: 2; system labels as truth: 1)
  each failed. Live on a scratch daemon of its build: "## Verdict: insufficient" on a fresh store, `cmp` identical
  against the binary, and the window line after a forced canary move.
- **The review** (R13, on health-words' review commit over main 3dba509d): the build clean (protocol.gen equal to the
  resolved `index.ts`); **440 of 440** (the prove's 8, every learning, judge, ladder, registry and dispatch test of the
  core, the CLI's, theseus-judge's and theseus-protocol's); **the whole workspace on the stack, 2,712 of 2,712**
  (`--retries 0`). **4 of 6 planted reverts caught** (a second `task.ended` row counted again; a judgment the execution
  paid added again; the method's Markdown no longer the generator's; the canary's decision the baseline's); not caught,
  test gaps with the code right: the CLI's stdout no longer byte for byte (`println!` for `print!`; theseus-w38g, P2)
  and a declined move opening the default window (theseus-u4t3, P2).
- **Live** (R13; the stack's frozen build, the Discord rig's layout with Discord and the index off, `[judge] enabled =
  true` with Jev's `api_base` on a loopback port nothing listens on, so no key spent, the stand-in model). A fresh
  store: "## Verdict: insufficient", canary 0 and control 0, each "labeled tasks: 0 of 30", stderr "every task that
  ended: loop.v1 has not moved to canary", the ledger's 12 rows before and after. `packs promote loop.v1 --canary 0.5`
  ("forced short of the bar") and three sessions that each made a task (all ended within 3 s): "since loop.v1's move to
  canary 0.5 on 2026-10-05", "3 finished tasks read; records by arm: canary 2, control 1", insufficient with 0
  labeled, `success` null, one stop per task by the baseline. `--records FILE > a.md` against `theseus-judge prove
  FILE --markdown -`: `cmp` silent (2,079 bytes, 3 records). A prove wrote no row (249 before and after).
- **FAST** (with health-words, Item 177: level). Nothing on the start or turn path; the method is a
  read on demand on the blocking pool.

**What the review found.** Not as the report said: a task whose `loop.v1` judgments all failed or were skipped (Jev
down, or no key) is counted in its arm with the baseline's decisions, not left out as `never_judged`. R13 recommended
keeping that, intent to treat (leaving out the tasks Jev failed on could bias the canary arm), and saying it, in a note
and in §2.9. And a semantic meeting point with learn-loop (Item 164), which builds and passes but will
mislead later: since learn-loop the loop point judges with the newest learned version the ladder placed, while the
prove reads only `loop.v1` judgments, so once a learned loop version is placed its tasks will read as `never_judged`,
and loop.v1's canary will in fact have been displaced in those sessions (theseus-ag0t, P2; no learned loop version
exists yet).

**The join** (stack H's second; R13's dry runs on 38bb2924, 60d2d6c3 and d50e6f2f, the joiner's on d50e6f2f and on
4d7cd561, the merge's base). Lock `cloud-prove-wire-in-join` 11:36:50, queued behind health-words' and
telemetry-calls'; merged at 12:02:33 on telemetry-calls' 4d7cd561, 10 s after its done line. Seven conflicts, one
block each, all where learn-loop had added beside the branch, all keep-both: protocol.gen's `index.ts` (learn-loop's
`JudgeProposal`, then the prove's two types, the generator's order), `learning/mod.rs` (`propose`, then `prove`), the
protocol's `judge_runs.rs` (learn-loop's types, then the prove's three, the shared `}` split), its `lib.rs` (one method
line with both `JUDGE_LEARN` and `JUDGE_PROVE`, no line added), `ts.rs`, and the CLI's `cmd.rs` and `main.rs` (`Learn`,
then `Prove`). rerere replayed R13's resolution and `prove-wire-in/resolve.py` found all seven resolved and checked
each by content; the `[T; N]` lists learn-loop's joiner met hold, `RUNS` declared 5 with 5 entries and `OPERATORS` 21
with 21 (the prove is in neither). No join fix; learn-loop's helpers the branch calls are unchanged. Every ceiling
holds (the protocol's `lib.rs` 2,727 of 2,727, the CLI's `render.rs` 3,098 of 3,100; `tests_prove.rs` 827 lines). The
warm met the machine: started at nice 19 at 12:06:32 beside another reviewer's 16-loop load rig (so as not to disturb
that measurement), then starved by two reviewers' cold builds at nice 0 once the rig ended; the joiner stopped its own
warm at 12:15:49 and restarted it at plain priority at 12:15:54 (the chain log's 12:21 line), and it ran to 12:21:26
(test build 1 m 04 s, clippy clean). After it: protocol's 31 (the regenerated protocol.gen equal to the staged merge),
R13's selection 442 of 442, shape ok. The signed merge **19a768a3** (4d7cd561 and c602add1), 12:23. Its gate (12:23:08
to 12:31:45, ok): **2,785 of 2,785** (1 slow, 21 skipped); lifecycle inside every budget (cold start p50 24.9 / p95
26.7 ms; from the config copy 24.1 / 45.9; SIGKILL then restart 28.6 / 32.6); turn frames 5 and 9, at budget. The
joiner stopped at 12:31:51, six seconds after the gate ended, when the account hit its weekly usage limit; the DM
thread finished the join (12:34 to 12:36): pushed (origin/main 19a768a3 at 12:34), deleted the branch, wrote the done
line (12:34:59), closed theseus-0j2.18 with the hash, appended install #4's list, and removed R13's tree. The store
stays at format 20.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No key and no format change; one new command, `theseus judge prove`. With no canary
moved yet it reads "insufficient", with its counts. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** Who pays is defined for today (the judge's day budget) and for 26b's §2.6 rule both. Success waits
for a learning run, not the clock. `should_stop` from a `not:<other class>` label is `null`. The window is read from
`pack:loop`'s scope, not the ladder, so the read writes nothing. A task whose judgments all failed counts in its arm.

**Known gaps.** R13's "For Eddie", each recommended: 26b writes one row per nudge naming its judgment and turn (to fill
`nudges` and `unnecessary_nudges`) and follows §2.6 for who pays an acting canary judgment; keep success waiting for
the learning run, and `should_stop`'s `null`; the cockpit's Judgment view is a later cockpit step (every field is in
`JudgeProveResult`); keep intent to treat, and say it. theseus-ag0t (P2), the learned loop version, fixed before the
first one is placed (a reason of its own, and a window line). theseus-w38g and theseus-u4t3 (P2), the two test gaps.
If a canary is rolled back and promoted again, the default window restarts at the new move, and earlier canary tasks
need `--since`.

### Item 180. Telemetry resumed: a confirmed call's run and a background job's end traced in the turn that answers them, each tool call counted once at its answer, and an approved AWS call's requests under its call (theseus-8pei and theseus-0zm4; telemetry2's last step; the seventh cloud batch's second part, its telemetry-resumed session, fired 2026-10-05 02:35 from faaa9df6, Opus 5.5; 82a2cade, 081994e8 and 7d6ce7bd; reviewed 07:43 to 08:45 by local reviewer R12, stack Y, and accepted with the stack at 08:52; its join held for Eddie's account switch and the disk compaction; joined 13:00 at 60b43fb6, a signed merge onto 19a768a3, by the stack-Y joiner, relaunched after the account's weekly limit stopped it; installed 2026-10-05 13:07 at 60b43fb6, install #4)

**Why.** Two calls the telemetry never saw, both found by the telemetry lane and C1:
- **theseus-8pei.** `theseus.tool.calls` and `theseus.tool.duration_ms` read the turn's `tool <name>` spans, and a
  call was traced once, in the turn whose model proposed it, with what became of it there: `awaiting_confirm` for a
  call that waits for the operator, `background` for a `proc.run` that outlived `proc_sync_secs`. The confirmed call's
  run (in the continuation, through `ToolRuntime::resume`) and the background job's end (a late result) made no span,
  so their outcome and time were never in the tool metrics, and a duration histogram of `proc.run` missed every job
  that ran in the background, the longest ones. Since C2 a fact's `span` writes into its turn's trace, and nothing in
  `resume.rs` or `late.rs` could make one.
- **theseus-0zm4.** C1 hands an AWS call's requests to the turn's trace through `Aws::bind` and `Aws::spans`, read
  by `trace_calls` for the calls of the batch the turn ran. A call that waited for approval ran later, in the
  confirm's continuation, where no `trace_calls` runs: its `aws.called` rows were written, but its spans stayed in
  `Aws`'s bounded list (256 calls) until they fell off. With 14b's writes, which ask, that would be common.

**What landed** (`theseus-core`'s `toolrun/`, `turn/` and `telemetry/`; no protocol type, config key, package or store
format change).
- **The calls come back to the turn** (82a2cade, the brief's steps 1, 2 and 4). `ResumeOutcome` gains `calls:
  Vec<ToolUse>` and `ran: Vec<Ran>` (a `Ran`'s index points into `calls`), filled for each call the loop answers, timed
  from its start, and by `ResumeOutcome::ran_batch` for `run_fresh`'s calls (the ones after the call that asked, never
  gated in their turn), their indices and groups shifted after those already there, so a group that ran together
  still sits under one `tools` span. `run_confirmed`, `run_authorized`, `check_dispatched` and `answer_settled` return
  the call's `CallOutcome`, and `answer_cancelled` its `ResultStatus` (R12 checked each status against the result node
  it writes: a superseded call is `declined`; `not_run` and `answer_never_asked` are `cancelled`; restart's `unknown`
  and a job's own status pass through). `absorb` returns `Vec<LateCall>` in place of a count. `fact::turn::CaughtUp`
  pushes the continuation's span with the calls' spans as its children. `trace_calls` and `call_result` move to a new
  `turn/calls.rs`, with `call_spans`, `late_spans` and `TurnRunner::take_late`. **A gap the issue did not name:**
  `finish` absorbs too, taking late results that settled while the turn ran; they are traced as well, at the trace's
  top level, since that turn has no continuation span for them. The late call's wire name comes from the registry by
  canonical name, with `theseus_tools::wire_name` as the fallback. An approved AWS call's requests are spans under its
  call in the continuation, with the row's correlation id, inside the call's bounds.
- **A late result's time.** Each is a `tool <wire>` span: a point at its absorption, with `late: true` and `run_ms`,
  the action's `settled_at_ms − dispatched_at_ms` (else the node's `duration_ms`); `spans::tool_calls` takes `run_ms`,
  when present, as the call's time. A late span has `result`, what the metrics and `failed()` read, and no `outcome`
  attribute, since no `CallOutcome` exists for it.
- **Counted once, at the answer** (081994e8, step 3: the rule "moved", as recommended). `Metrics::tool_calls` skips
  spans whose `result` is `awaiting_confirm` or `background`, so each call counts once, by its result: in its own turn
  when it ran there; in the continuation when it waited for the operator (approved, declined, superseded, unknown
  after a restart); at its late result for a background job, timed by the job's run. The proposing turn's span keeps
  `awaiting_confirm` or `background`, so the trace still says what happened then; a confirmed call that goes to the
  background inside its continuation is traced `background` there and counted at its late result.
- theseus-core's AGENTS.md says where a call's span is built and when it counts (7d6ce7bd). turn.rs went from 3,499 to
  3,439 lines on the branch.

**How it is proven.**
- **The session's tests:** `telemetry/tests_resumed.rs` (a confirmed call's run is traced in its continuation, `ok`,
  timed by its 0.4 s run, the proposing turn's span still `awaiting_confirm`; a late result is traced once with its
  job's run, `late: true`, `run_ms` ≥ 1,600, zero length; a confirmed 0.4 s call and a confirmed 1.6 s call that goes
  to the background are counted once each by their runs, one `proc.run`/`ok` series of 2) and `aws/tests_resumed.rs`
  (an approved `DescribeStacks`'s span under its call in the continuation). Two existing tests move by the rule (the
  shell-fallback ratio (6, 10) → (4, 8): the synthetic turn's 60 s `background` and 1 ms `awaiting_confirm` points are
  gone; 3 points, not 4), and the output golden moved four lines, each a new child span under a continuation (a late
  `proc_run` with `run_ms`, an approved `fs_write`, a declined one, a cancelled one). Under load at nice 19, 5 runs, 4
  of 4. Its plants (continuation spans dropped: 3 fail; a late result timed by its absorption; a moved call also
  counted in its proposing turn: 3 fail; AWS spans not taken: 2 fail) each failed.
- **The review** (R12, stacked on telemetry-calls' review commit over main e4d09068; one conflict, telemetry.rs's
  test module lines, keep-both; no join fix: tiering's and task-board's lines in `toolrun/` and the turn merged and
  compiled as they are; `Ran`, `ResumeOutcome` and `CallOutcome` unchanged on main since the base): the build clean,
  the golden passing as merged; **the whole workspace suite on the stack, 2,706 of 2,706** (`--retries 0`, load about
  30). **3 of 5 planted reverts caught** (the `awaiting_confirm` skip removed, a confirmed call counted twice; the late
  span's `run_ms` dropped; the `background` skip removed). Not caught, test gaps with the code right: `finish`'s late
  results untraced (`take_late` taking them and pushing no span; under the "moved" rule that is the only place such a
  job counts, so a regression would drop it silently; theseus-kxyc, P2), and `ran_batch`'s index shift dropped (the
  calls after the one that asked would be named for the wrong calls, one counted twice and another never;
  theseus-6xwq, P2). R12 grepped the cockpit, theseusd, every crate, the configs and the docs for the two series: no
  consumer in the repo expected the old counts.
- **Live** (R12, a scratch daemon of the stack's build as a transient user unit, `enforcement = "approve"`,
  `proc_sync_secs = 2`, the stand-in model, a loopback OTLP sink). A confirmed `sleep 1; echo tide`: the proposing
  turn's trace `loop 0` > `tool proc_run` `awaiting_confirm`, the continuation's `continuation` > `tool proc_run`
  `ok`, 1,621 ms. A confirmed `sleep 8`: `background` in the continuation (2,029 ms), then the late result's turn,
  which the daemon ran on its own, `continuation` > `tool proc_run` `ok`, `late: true`, `run_ms: 8035`, a point. The
  sink's last metrics post: `theseus.tool.calls` only `proc.run`/`ok`, **2**, and `theseus.tool.duration_ms` count 2,
  sum 9,656.3 ms (1,621.3 + 8,035), with no `awaiting_confirm` or `background` series; an earlier post, before the late
  turn, held 1. The approved AWS call stayed offline (the test stands in).
- **FAST** (the whole stack against e4d09068, one hold, palindrome order): turn frames 5 and 9 on both, plain median
  p50 103.3 against 87.5 ms, tool call 225.5 against 216.2, fdatasync the same; every lifecycle phase's B median equal
  or better (cold start 29.3 → 22.7 ms). Spans are built from what the calls returned; nothing waits on them or reads
  the store for them.

**The join** (stack Y's second; R12's dry runs on e4d09068, 3dba509d and 38bb2924, with `resolve.py` extended at
08:28 for durability-fixes' new test module; the DM thread's at 11:28 on d50e6f2f, which found two hunks new since
R12's review, both from proc-steps, and wrote `b7-joins-y/resolve-main.py` for them; the joiner's again at 11:41 and
12:03). Lock `cloud-telemetry-resumed-join` 12:02:30, queued behind prove-wire-in's. The joiner stopped at 12:31 on the
account's weekly limit with nothing merged; relaunched at 12:37, it took the lock over at 12:38:03 and merged at
12:38:37 on prove-wire-in's 19a768a3. Four conflicts, all keep-both: by `telemetry-resumed/resolve.py`, the test
module lines in `telemetry.rs` (main's `tests_recall`, then the branch's `tests_resumed`; rerere replayed R12's
resolution) and in `aws/mod.rs` (`tests_restore`, the branch's `tests_resumed`, durability-fixes' `tests_synced`); by
`resolve-main.py`, which refuses any other shape, theseus-core's `AGENTS.md` "Tool calls" paragraph (main's
batch-steps clause, then the branch's continuation and span sentences) and `toolrun.rs`'s use lines (main's
`pub(crate) use batch::Turned;`, then the branch's `pub use late::LateCall;`). The 15 staged files equal a dry run of
the same two commits plus both scripts, byte for byte. `INSTRUMENTS` stays 34 of 34 (the branch adds no instrument),
and the other fixed-length lists in the files both sides changed did not move. No join fix: proc-steps' batched
`proc.run` is one call with one result, and this branch counts each call once at its answer; `tests_steps` and the
telemetry tests pass together. The warm (test build, clippy) clean; then rustfmt and theseus-core's telemetry and aws
tests, `tests_steps` and the output golden at TZ=America/Phoenix, **193 of 193**; shape ok (toolrun.rs 2,494 and
telemetry/tests.rs 2,484 of the 2,500 limit; turn.rs 3,452 of its 3,523). The signed merge **60b43fb6** (19a768a3 and
382c5a59), 12:51. Its gate (from 12:51:56, 125 s of it waiting for the shared lock; exit 0 at 12:59:43): **2,789 of
2,789** (1 slow, 21 skipped); lifecycle in every budget (cold start p50 25.0 / p95 29.3 ms; from the config copy 27.3 /
34.4; clean shutdown with a job 36.2 / 56.5; SIGKILL then restart 28.4 / 34.8; binary swap 50.3 / 59.1; the daemon's
clock kernel 8.66 / 15.20, serving 21.37 / 29.23); L1 start p50 9.24 ms; turn plain 5 frames p50 76.8 ms, tool call 9
frames p50 164.6, fdatasync p50 6.8 ms. Pushed; done line 13:00:09. The store stays at format 20.

**The install** (2026-10-05 13:07 at 60b43fb6, install #4). No key and no format change. An OTLP collector's `theseus.tool.calls` and
`theseus.tool.duration_ms` count each call once, at its answer; a background job counts once, timed by its run; an
outside dashboard's `awaiting_confirm` and `background` series stop. A confirmed call's span sits under its
continuation, and an approved AWS call's requests under its call. Health after the restart (13:07:43): `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving at 29.1 ms with builds beside it, `cgroup: delegated`, `route.v1`, `rerank.v1` and `security.v3` live, memory live on the `baseline` arm, Discord ready, and no error or warning in the journal; the store from format 16 to 20 at its first write, after the install's backup.

**Divergences.** The brief's steps 1, 2 and 4 are one commit (their code shares `turn/calls.rs` and `catch_up`'s one
record). `finish`'s late results are traced at the trace's top level, not under a continuation (theseus-core's
AGENTS.md says the continuation for both, a nit R12 noted). The counting rule moved the two series' points rather than
keeping the proposing turn's placeholders.

**Known gaps.** R12's "For Eddie", each recommended: accept "moved" and say it in §3.23 and the status page (if "how
often is the operator asked" is wanted as a metric, add a counter for it, not the double count); the calls that never
count, a waiting call answered by `answer_after_cancel` and a background job whose late result no turn takes, accepted
now with a P3 follow-up that counts them where the cancel answers them; `theseus.task.changes` now counting a layer-1
task edit approved in a continuation, accepted; `TurnSubmitResult.tool_calls` kept as the turn's own loop, its doc
comment to say the metrics are the full count. theseus-kxyc and theseus-6xwq (P2), the two test gaps, soon after the
join. The live AWS check waits for Eddie. turn.rs's long-files ceiling can drop to about 3,460; toolrun.rs and
telemetry/tests.rs sit just under 2,500, so their next additions need a split or an entry.

