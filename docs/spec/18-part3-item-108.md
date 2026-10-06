# The Ship of Theseus, chapter 18: Part III, A4's Items 108 to 117 ([index](README.md))
### Item 108. Step 15: the durability tender ships the store to the foundation's bucket and table, in a session narrowed to its prefix and rows, from a durable cursor (theseus-mgw.7, roadmap row 32, with theseus-celu.29; the cloud batch 4c's durability-tender session, fired 2026-10-03 20:00 from d9b0931, Opus 5.5; d5b7340 and 04573f6; reviewed 2026-10-04 01:44 to 01:56 by the harvest wake b86bd6c2, with a live check on the account; joined 02:13 at 4bb5aac9, a signed merge onto ab54f037 with one join fix (two test literals), by the same wake; installed 14:09 at bddfd407, install #1)

**Why.** §6's durability target: what the store has committed is off the machine within 5 to 60 s, measured as the
age of the oldest unshipped committed record. The foundation stack (C2, Items 78 and 79) made the bucket
(`theseus-<account>-<region>`, versioned, SSE-S3) and the table (`theseus-durability`, keys `pk` and `sk`, on demand)
and defined no tender role: a tender's narrowing is an inline session policy on `theseus-owner`, as C2's budget tender
does. `theseus-follow`, the index lane's WAL follower (Item 16), was built with this tender in mind. Step 15 (row 32)
is the tender; restoring from what it ships is step 16 (Item 123).

**What the session found.**
- The follower's `Batch` named the byte spans read and the segments a read sealed, not which span holds which record;
  the index rows need that, so `Batch` gains `ends` (three lines in the index lane's crate).
- Segments are 64 MiB, so shipping only sealed segments would leave hours of exposure, not 5 to 60 s: the tender also
  ships the open segment's new whole frames as byte-range **tails**.
- **Where it runs.** The design said "on the index lane's WAL follower"; the session read that as the follower crate,
  not the index tender's process. The tender signs in a role session, and §3.5 keeps AWS credentials in the core: to
  run inside `theseus-index` the core would have to hand that process session credentials, a second path for them to
  leak. So it runs in the daemon, as a task started after serving, reading the WAL through the read-only follower; it
  holds the core by `Weak` and ends once the core is gone.
- `theseusd` stops with `rt.shutdown_timeout(500 ms)`, which waits for the blocking pool, so an inotify wait in
  `spawn_blocking` would have held a clean stop up to 500 ms: the watch moved to a thread of its own.
- C1's fake keeps bodies as lossy UTF-8, so the tests use a stateful fake of their own (`aws/tests_durable.rs`): S3
  objects and multipart uploads with each checksum checked as S3 checks it, DynamoDB items with unprocessed items on
  request, and STS.

**What landed** (30 files, +2,959 −15 over ab54f037 at the join; `aws/durable.rs` with `durable/cursor.rs`, `rows.rs`
and `s3.rs`).
- **Objects**, under `s3://theseus-<account>-<region>/durability/<deployment>/`: `wal/<n:09>.seg`, a sealed segment
  whole (one `PutObject` up to 8 MiB; past that, a multipart upload in 8 MiB parts, each with its SHA-256, the upload
  made with `ChecksumAlgorithm=SHA256`); `wal/<n:09>.seg.tail/<from:012>-<to:012>`, the open segment's new whole
  frames, a byte range of the file, the tails tiling the segment (a segment is never sent as tails once it is sealed or
  a later one exists); `blobs/<sha256>`, each new blob (a file whose bytes do not hash to its name is left and logged).
  Every object carries `ChecksumSHA256`, which S3 verifies, and goes exactly as the WAL holds it, read only through
  the follower.
- **Index rows** in `theseus-durability`, every partition key starting with the deployment: `<dep>#wal` / `<n:09>` (a
  sealed segment's key, bytes, sha256, last position, parts); `<dep>#wal` / `<n:09>.tail.<from:012>` (a tail's key,
  range, last position, sha256); `<dep>#blob` / `<sha256>`; and `<dep>#rec#<kind>` / `<key>`, each keyed record's
  latest position, `at_unix_ms`, segment and scope: the redb index's "latest by key" as rows, so a restore finds a
  node's segment. `BatchWriteItem`s of at most 25, never naming one key twice (the latest stands); unprocessed items
  sent again with a doubling backoff (50 ms first, 8 tries), and past that the pass fails and keeps its rows for the
  next.
- **The durable cursor**, `<state>/durability/cursor.json`, written whole, synced, renamed, its directory synced: the
  follower's cursor everything before is shipped to, the last sealed segment and how far the open one's tails reach,
  the last position whose rows are written, the objects in flight (each saved before its first request; a multipart
  upload's id and parts as S3 takes each), and the rows of shipped objects not yet written. A restart that meets an
  object in flight asks S3 first: `HeadObject` with `ChecksumMode=ENABLED` for a put, `ListParts` for an upload
  (adopting the parts whose checksums match the file); an upload S3 completed before the cursor knew is recognised by
  its composite checksum. A WAL the cursor no longer matches (`FollowError::Rewound`, after a restore) starts again from
  the log's start, and S3's checksums say what is there.
- **The session** `theseus-durability` (`Kind::Tender("durability")`), its inline policy (`durable::policy`):
  `s3:PutObject`, `s3:GetObject`, `s3:ListMultipartUploadParts` and `s3:AbortMultipartUpload` on
  `<bucket>/durability/<deployment>/*`, and `dynamodb:BatchWriteItem` on the table with `ForAllValues:StringLike
  dynamodb:LeadingKeys ["<deployment>#*"]`. **No delete.** The prefix's trailing slash and the `#` keep one
  deployment's objects and rows from another's.
- **When it runs:** 2 s after serving (`START_AFTER`), then after the account's check settles; each WAL change wakes it
  (inotify through `theseus_follow::Waker`, on its own `durability-watch` thread, so a stop never waits for it); it
  waits 5 s (`SETTLE`) so the writes around the change ride along, then ships; a pass also runs every 60 s as a
  backstop; a failed pass is tried again after a backoff doubling from 5 s to 60 s. Disk reads go through
  `theseus_store::blocking`.
- **The config:** `[aws.accounts.<id>] durability = true`, false by default, refused without `owner_role` or on two
  accounts; the template has the line.
- **Health** (`aws.accounts[i].durability`, `AwsDurabilityStatus`): `state` (`waiting`, `shipping`, `caught_up`,
  `failing`, `stopped`), the bucket, prefix and table, `oldest_unshipped_unix_ms` and `lag_ms`, `shipped_to_position`
  and `last_shipped_unix_ms`, counts since the start (segments, tails, blobs, rows, bytes), and `error`.
- **The ledger:** one `durability.shipped` row per sealed segment (segment, key, bytes, sha256, last position, parts,
  `took_ms`). Tails, blobs and rows are counts in health and telemetry instead: a row each would be a frame each,
  every few seconds, each shipped again.
- **Telemetry:** `theseus.durability.shipped` (bytes, or rows, by object: segment, tail, blob or rows) and
  `theseus.durability.lag_ms` (a histogram of the oldest unshipped record's age at each catch-up; its per-interval max
  is the alarm's input). The in-house OTLP encoder has no gauge kind, so the live `oldest_unshipped` is health's.
- The lifecycle bench's config gives its unreachable account `owner_role` and `durability = true`, so the bench's
  start, stop and swap include the tender. Cargo.lock gains one edge (theseus-core to theseus-follow). **No store
  format change:** the cursor lives outside the store, and a new ledger kind is no new field.

**How it is proven.**
- **Tests** (`aws/tests_durable.rs` unless named): a sealed segment shipped once with its checksum (30 records in
  400-byte segments: every sealed object equal to its file, the open segment's tails joining to its file, every body
  with its matching `x-amz-checksum-sha256`, segments over 150 bytes in 128-byte parts, every key sent once, one
  `durability.shipped` row per sealed segment, a second pass sending no request at all, more writes shipped without
  resending); a restart mid-upload (a crash after S3 took part 2, before the cursor saved it: one `ListParts`, no part
  sent twice, the upload completed once; and a crash just after a whole put, found by `HeadObject` and not sent again);
  a tail shipped before a crash not shipped again in part (the second commit: the next tail starts where the shipped
  one ended, no overlap, the pending rows written); a blob shipped once; index rows batched, with unprocessed items
  re-sent and a pass that keeps failing going `failing`; a refusal said in health (`AccessDenied`) and the next pass
  shipping; the session narrowed (resources, condition, no delete); the tender starting after its wait (no request in
  the first 500 ms) and shipping what the WAL gains. Also the cursor's and rows' units, the config's, theseus-follow's
  `ends`, and theseusd's `the_durability_tender_waits_for_serving_and_says_so` (a real daemon with the account at a fake
  endpoint: no request in the `theseus-durability` session before the socket answers; its first, AssumeRole with the
  `ShipToItsPrefix` policy, after). At the review, on the merged tree, 26 of 26.
- **Planted reverts:** the durable cursor dropped (`cursor::save` a no-op) fails the restart test (`left: 2, right: 1`
  uploads created); the restart's ListParts adoption dropped fails it ("a part sent twice", 4 against 5); the tail clamp
  missing (the first commit's code against the new test) failed with "a gap or an overlap at
  …/000000000000-000000000312". The review re-ran the first two on the amended merge, each baseline building and
  passing first. Its first run's plants were void: their baselines did not compile (the join fix's E0063), so plant.py's
  "FAILS as it should" with exit 101 proved nothing; plant.py reports a plant's exit but not that its baseline passed.
- **Under load:** the 10 tests, 10 runs, 100 of 100 (7 to 14 s a run); theseusd's aws tests 3 of 3. The lifecycle bench
  OK for the shape in the cloud.
- **Live, at the review, on the account** (01:49 to 01:51; a scratch daemon of the prepared merge, a fresh state dir,
  the account bound with `owner_role`, `deployment = "theseus-scratch-15"`, `durability = true`, no budget line, the
  simulator's stand-in model): at once `state waiting` ("for the start to settle"), the prefix
  `durability/theseus-scratch-15/`, the table and the foundation's bucket; **caught up 4.0 s after the start** (1 tail,
  1 row, 2,976 bytes, to position 9). A turn ("say ok") **shipped 11.2 s later** (the 5 s settle and the pass): tails 2,
  rows 9, 22,771 bytes, to position 41, lag 0. In the bucket, `wal/000000001.seg.tail/000000000000-000000002976` (2,976
  B) and `…/000000002976-000000022771` (19,795 B), each modified within seconds of its write; after the restart one more,
  from 22,771 to 25,849: **the tails tile the segment, no gap and no overlap.** Each tail's `ChecksumSHA256` equals its
  row's `sha256` (all three), SSE AES256. In the table, one item per tail, and the session's latest position (37, in
  segment 1). After a clean stop and start, health counted 1 tail of 3,078 bytes (the restart's own frames, positions 41
  to 51): **nothing shipped before was shipped again.** Then every version under the scratch prefix (3) and every row
  whose key starts `theseus-scratch-15#` (10) deleted: 0 left. The first cleanup misfired: it parsed a helper's masked
  output, whose mask replaces every 12-digit number, the tails' zero-padded offsets too, so it named keys that do not
  exist; the second ran unmasked, printing only masked text. Not seen: a sealed segment (64 MiB of WAL), and CloudTrail's
  view of the session (its delivery lags).

**The join** (the harvest wake b86bd6c2, 02:01 to 02:13). Prepared on aws-hands' merge: six conflicts, `aws/mod.rs`'s
module docs (aws-curated's secret-handle sentence kept, the tender added; mods sorted), theseus-core's `AGENTS.md` (the
tender's sentence, aws-hands' bullet kept), the protocol's `aws.rs` doc (the tender's line and `confirm_alerts`),
Cargo.lock (theseus-follow beside theseus-judge), and two TypeScript files to `cockpit/src/protocol.gen/`. **The join
fix:** the merged tree failed to compile theseus-core's tests (E0063, field `durability`): the `AwsAccountConfig`
literals in aws-l1's `tests_l1.rs` (Item 94) and aws-hands' `tests_hands.rs` (Item 107) postdate this
branch's base, and take `durability: false`. Re-prepared on aws-hands' amended merge (rerere replayed the six), main
fast-forwarded to 4bb5aac9, warm exit 0. **The gate:** every phase before the benches green, the suite 2,007 of 2,007;
the lifecycle bench missed twice with IO pressure at 9 to 12 % from B5's Terminal-Bench containers (run 1 on cold-start
outliers, p95 630.7 and 983.9 ms with p50s 25.3 and 31.3; its rerun on the clean shutdown, p50 76.0 and p95 107.4
against 100 + 4). Since the bench config is this branch's (the tender on for an account out of reach), the reviewer
checked first that in it the tender waits on the account's check, which fails closed, and ships nothing. Then
**finish-benches.sh exit 0 at 02:12:56:** lifecycle OK on its first run under the strict budgets (cold start p50 23.1,
p95 30.7; from the copy 23.8 and 34.8; clean shutdown 38.7 and 61.1; kill 27.6 and 31.9; swap 53.8 and 56.2 ms; the
settle measured with the busy allowance at IO 12 %), the jobs phase's L1 start p95 9.18 ms, frames 5 and 9, deny ok.
Pushed 02:13. Closed theseus-mgw.7 and theseus-celu.29; filed theseus-mgw.12 (P2): ship only synced frames, now that
each frame carries the synced mark (Item 92). Batch 5's restore-s3 launched on its done line.

**The install** (install #1, 2026-10-04 14:09, at bddfd407). The tender is in the operator's daemon; it ships only
for an account with `durability = true`, and install #1's sources do not record it turned on.

**Divergences.** It runs in the daemon, not in `theseus-index` (above). It wakes on WAL changes with a 5 s settle and a
60 s backstop, where Part II scheduled it "in released turn-lock time by staleness". Tails ship the open segment, which
the design's "WAL segments" did not name. No alarm and no gauge: the lag is a histogram and health's live line. Tails
are kept after their segment seals: the session has no delete, on purpose.

**Known gaps.** **Tails of frames not yet synced:** the follower reads whole frames from the page cache, so a tail may
hold a frame the machine never synced; after a power loss the next start sees `Rewound` and ships again from the
start, and the stale tails stay in S3. Step 16's restore must verify frames (they carry crcs) and prefer a sealed
segment's object; theseus-mgw.12 (ship only synced frames) was fixed in batch 7's stack A (Item 171).
A bucket lifecycle rule on `durability/*/wal/*.seg.tail/` (say 7 days) in the foundation stack would clean up old
tails (`infra/aws/`'s). A first start on an existing store ships its whole history as backfill (about $1.25 per
million row writes). Health's CLI and cockpit lines were not drawn (the field is in the health JSON and the generated
TypeScript).

### Item 109. B4: terminals as tools: `term.open`, `term.send`, `term.read` and `term.close`, a pty per terminal with a model of its screen, closed at a session's end, a cancel, a `/stop` and the daemon's stop (theseus-n88g.4, the worth plan's B4 and the owner's D7, with theseus-celu.30; the cloud batch 4c's terminal-tools session, fired 2026-10-03 20:00 from d9b0931, Opus 5.5; 7d330f7; reviewed 2026-10-04 00:21 to 00:40 by the batch-4 harvest wake 7379d0cc; joined 02:19 at 60ab73ca, a signed merge onto 4bb5aac9, by the harvest wake b86bd6c2; installed 14:09 at bddfd407, install #1)

**Why.** `proc.run` is a job with a typed argv, no pty, and stdin from `/dev/null`, so nothing interactive could be
driven: no REPL, editor, debugger or prompt that asks. The worth plan's tool list (Item 93) named a terminal toolset,
a pty per session, and the owner said yes at 17:14 on 2026-10-03 (D7). §3.24's selected set had reserved the need as
`proc.session.open/send/close`.

**What the session found.** A toollet's `ToolCtx` does not know its call's session, and a terminal belongs to one. So
the `Tool` trait and `ToolCtx` stay as they are, and toolrun's async path runs a `term.*` call through `Terms::run(tool,
session, input, ctx)` instead of `Tool::run_async` (one `match` in `run_inproc`); the tools remain ordinary tools for
planning, the catalog and the gate. The gate already judges any plan that carries an `argv` as it judges `proc.run`
(the floor, the approve and allow lists, the path arguments), so nothing in `policy.rs` changed. `ToolRuntime::brokered`
would have named a broker grant for a terminal's program that never reaches it, so it returns early for the family: a
terminal's program gets no grant, since a secret sent into a pty whose screen the model reads is a design question the
session answered no. And sessions never end as such; their executions do. "The session's end" is its execution
reaching a terminal state at a turn's end (in practice a task that reported, or a failure); a conversation's terminals
live until `term.close`, a cancel, a `/stop` or the daemon's stop.

**What landed** (27 files, +3,621 −5 over 4bb5aac9 at the join; no new dependency, `libc` already there).
- **`theseus-core/src/term/`.** `Terms`, the registry: at most 4 terminals a session; open, send, close one, close a
  session's, close all (`Core::close_terminals` at the daemon's stop); the read's snapshot and its diff. `pty.rs`:
  `posix_openpt`, `grantpt`, `unlockpt`, `ptsname_r` and `TIOCSWINSZ`, both ends opened `O_CLOEXEC | O_NOCTTY`; the
  child through `children::spawn(Kind::Owned)` with `setsid`, `TIOCSCTTY` and the operator's umask (all a `pre_exec`
  does, async-signal-safe), the job's environment (`proc_env`) plus `THESEUS_SESSION` and `TERM=xterm`; the slave dup'd
  into the child's stdio and the parent's copies dropped, so the master sees EOF when the program ends; a reader thread
  per terminal feeds the screen and writes the screen's own answers (`6n`, `c`) back. `vt.rs`, the VT model, and
  `keys.rs`.
- **The four tools** (`term/tools.rs`):
  - `term.open {argv, cwd?, rows?, cols?, quiet_ms?}`, class Run, async: the id (`t1`, `t2`, … counted across the
    daemon) and the first screen, after up to `quiet_ms` (300) of quiet, at most 3 s in all. It plans its argv and
    cwd, so it meets the floor and the lists as `proc.run` does.
  - `term.send {terminal, text?, keys?}`, class Run: the text first (each `\n` is Enter, CR), then each named key
    (Enter, Tab, Escape, Backspace, Delete, Insert, Space, the arrows, Home, End, PageUp, PageDown, F1 to F12, and
    `Ctrl-x`, `C-x` or `^x`; an unknown key is invalid input). It answers once the keys are sent, with no screen, and
    plans its terminal's program's argv and cwd.
  - `term.read {terminal, quiet_ms?, until?, timeout_ms?}`, class Read: the numbered rows (blank trailing rows elided),
    the cursor, whether full-screen, the program's state (running, exited N, signal N), the lines that scrolled off the
    top since the last read (a scrollback of 2,000 lines), and which rows changed. `until` is plain text that may span
    rows and takes precedence over `quiet_ms`; the wait is bounded by `timeout_ms` (10 s, at most 60 s) and ends early
    when the program ends.
  - `term.close {terminal}`, class Read: SIGHUP and SIGTERM to the process group, every descendant, and every process
    holding the pty's slave open (found in `/proc/*/fd`); 500 ms; then SIGKILL to what is left, and the program
    reaped.
- **Outside text.** A program on `[policy] external_programs` (`Listed::in_text`) marks its terminal's screens as
  outside text, so its first screen holds the session, as a listed program's output does (Item 74).
- **The closes.** A task's terminals close at its execution's terminal end (`turn.rs`, 8 lines); a cancel and a
  `/stop` close the session's (`rpc/driver.rs`); `finish_stop` closes all of them first (`outbox.rs`, 2 lines), at once
  when none is open, otherwise on the blocking pool with each `term.closed` row written in its own session before the
  stop's checkpoints.
- **Rows and surfaces:** `term.opened` and `term.closed` (`by`) with narrative lines (`fact/term.rs`); health's
  `terminals` (`TerminalInfo`) and a line per terminal; the CLI's `⌨ term.send t1 "print(1)⏎" Ctrl-C` line and
  Discord's summary naming the terminal and keys (`theseus-protocol/src/term.rs`). The template's `[policy.tools]`
  lines: `term.read` and `term.close` open, like the other reads; `term.open` and `term.send` commented, so they
  inherit `enforcement`. theseus-core's `AGENTS.md` gains a Terminals bullet.
- **No store format change:** two new ledger kinds, strings in the existing record.

**How it is proven.**
- **Tests** (21 in theseus-core, 1 in the protocol). `term::tests` (13): golden screens from recorded escape sequences
  (a shell with a backspace; tabs, the deferred wrap, scrolling into the scrollback; cursor moves and every erase;
  insert and delete of characters and lines; a scroll region with reverse index; the alternate screen entered and
  restored, from a recorded `vim` start and quit and a recorded `less` session; escapes dropped whole and UTF-8 split
  across feeds; the answers to `6n`, `c`, `>c` and `5n`), the named keys, and real programs on ptys: `sh` (`echo
  $((6*7))` on its row, a read's changed rows, a quiet read's "Nothing changed", Ctrl-C ending `sleep 4242; echo after`
  with `after` never printed, another session unable to reach the terminal, the close leaving the shell dead); `python3
  -q` (`sum(range(101)) * 3` giving `15150`, Ctrl-C's `KeyboardInterrupt`, Ctrl-D's end reported); a listed `cat`
  whose every screen is outside text, and a shell marked once it is sent `gh issue view 1`; the 4-terminal limit; a
  close leaving no child (a `sleep` that ignores HUP and TERM, and a `setsid sleep` that left the tree, both gone).
  `tests_term` (7, through the whole core): the gate's decisions (`term.open python3` waits on the approve list, `cat`
  runs on the allow list, `op …` is the floor, `sh` is at `notify`; `term.send` judged by its terminal's program); a
  listed program's first screen holding its session (the next `term.send` waits, `term.read` does not); an unlisted
  program holding nothing; and a terminal closed, with its `term.closed` row and its program dead, by a cancel, by a
  `/stop`, by the daemon's stop (two sessions), and by a task's session end (the task opens `sleep 4747` and reports).
  At the review, 22 of 22; the session's under load, 5 runs of the 21, all green.
- **Planted reverts.** A task's terminal left open at its session's end (`if session_ended && false`) fails
  `a_tasks_terminal_closes_at_its_sessions_end` after its 20 s wait, the other six passing; a listed program's screen
  not marked (`let external = None`) fails two tests ("its open's screen is outside text"; "the send waits after the
  screen"). The review re-ran both and added a third: the daemon's stop without `close_terminals()` fails
  `the_daemons_stop_closes_every_terminal`, since the `Drop` backstop kills the programs but writes no `term.closed`
  row, so the record is the stop's own close.
- **The session's live check** (a scratch daemon of the branch, a scripted stand-in Messages API, a fake `op`): one
  ask drove `term.open python3 -q`, `term.send "print(6 * 7 * 1000 + 1)\n"`, `term.read until "42001"` and
  `term.close`; then `vim -u NONE -N note.txt`, `ihello from vim`, Escape, `:wq`, and a read saying "its program exited
  (0)", the file holding `hello from vim`; then `term.open sleep 4949`. The CLI showed the `⌨` lines and health a
  `terminal t3 (sleep) · ses_… · pid … · 24x80 · running` line; `theseus stop <session>` closed both terminals
  (`by: its conversation was stopped`) and the sleep was gone; after three more and a `theseus shutdown`, nothing was
  left, and the next start's ledger held three `term.closed` rows `by: the daemon stopped`.

**The join** (the harvest wake b86bd6c2, 01:24 to 02:19; alone, since it touches the stop path and the turn path).
Prepared four times as main moved (on dd94bc6b, fbc1d2a0, e457555d, then 4bb5aac9), rerere replaying each time:
toolrun.rs keeps Tier 7.1's `job_waits` and 36b's `mcp` beside `terms` (terms after mcp in the constructor);
`fact/mod.rs` keeps the ontology's, the judge's, the MCP board's and the hands' facts beside the two terminal facts;
health keeps the judge's line beside `terminals`; `config.rs`'s template count 35 (main's 31 plus the four terminal
lines); `TerminalInfo.ts` to `cockpit/src/protocol.gen/`. **The merged `finish_stop` closes terminals first, before
81kk's late-rows close and the last checkpoint** (Item 104). Main fast-forwarded to 60ab73ca; the protocol
tests 25 of 25, the TypeScript unchanged. **Gate exit 0 at 02:19:23:** 2,029 of 2,029 (17 skipped, 1 slow); lifecycle
OK on the first run (cold start p50 22.7, p95 29.4; clean shutdown 34.6 and 67.6; kill 26.5 and 32.8; swap 47.2 and
55.5 ms); the jobs phase's L1 start p95 7.89 ms; frames 5 and 9; pushed 02:19. Closed theseus-n88g.4 and
theseus-celu.30.

**The install** (install #1, 2026-10-04 14:09, at bddfd407). The four tools are in the operator's daemon, offered with
the rest; a terminal open at a restart is closed and recorded by the stop.

**Divergences.**
- The interactive process is the `term` family (four tools, one verb each), not §3.24's `proc.session.open/send/close`;
  the screen is read by its own tool, not returned by every send.
- The session's end is its execution's terminal end, since sessions never end as such.
- No broker grant reaches a terminal's program; a program that needs its token (`gh`) runs through `proc.run`. Were a
  terminal ever given one, its screen would need the job wrapper's `redact` too (theseus-rnx).
- `term.close` is class Read, so it never waits on the outside-text hold: it only stops what the session started.
- Terminals live in memory: they do not survive a restart or an exec (their ptys die with the daemon, and the kernel
  hangs up each program's session); the rows are the record. A restart onto a changed note goes through `finish_stop`,
  so it closes and records them first.

**Known gaps.** An ended program keeps its slot until `term.close` or its session's end, so its last screen can be
read; the zombie is reaped at the next read, at health, or at the close. Ids are daemon-wide counters: a `term.send`
plan by id can name another session's terminal's argv in its own gate record before the run refuses it (random ids per
session would close that). Text typed into an open shell is not judged against the floor (`op` typed into `sh`), the
same as an approved `sh -c "…"`: the gate judges a program, never what a command does. The VT model leaves out colours
and attributes, wide characters (a CJK character or an emoji takes one cell), combining marks, insert and origin modes,
program-set tab stops, character sets (line drawing shows as ASCII letters) and the mouse; `until` is plain text, not
a pattern (no regex crate in the core); `vt100` would cover these were a crate allowed. The cockpit's terminal view is
left for a cockpit step (each open terminal from health's `terminals`, its last screen as a monospace block, the
`term.opened` and `term.closed` rows in the session's timeline, and a close button that is the operator's `/stop`).

### Item 110. 41b: Theseus's own MCP server on loopback, its conversations held to a posture floor, never an approval surface (theseus-ext.2, roadmap row 72, with theseus-celu.27; the cloud batch 4c's mcp-server session, fired 2026-10-03 20:00 from d9b0931, Opus 5.5; 5fbd647; reviewed 22:42 (notes) and 2026-10-04 00:22 to 00:36 by the batch-4 harvest wakes, with a live check at 01:38 to 01:40 on a scratch daemon of the prepared merge; joined 02:35 at 24a9fef6, a signed merge onto 60ab73ca, by the harvest wake b86bd6c2; installed 14:09 at bddfd407, install #1, off by default)

**Why.** §3.8 gives Theseus both MCP roles. The client came first (36b, Item 106); 41b is the server: another
agent on the operator's machine (Claude Code, an editor) opens a Theseus conversation, sends it text and reads the
reply, under the same gate as every other surface. M7's design (`docs/design/m7-surface.md` §2.5) asked for a
loopback listener with one static key, a surface of its own that can never answer an approval, a principal `mcp` held
to a posture floor, and a job's hold passed on when a job's own process is the client. `theseus-mcp`'s `server` module
(41a) had merged ahead of its reader with the listener, the Host, Origin, key and rate checks, the five tools, and a
`CoreClient` trait with a fake core.

**What the session found.**
- 41a's server had no peer (uid) check, which the web UI has (theseus-3qf).
- The design's "peer traced through `/proc/net/tcp`" no longer exists as such: theseus-zmgb retired the process walk,
  and `peer.rs` reads only the client socket's uid. A job's hold passes on through `THESEUS_SESSION`, which only the
  CLI sends (`opened_from`); an HTTP client sends nothing like it, so the job had to be found from the connection.
- `Authority`, stored with every execution, already had `principal` and a free `ceilings` map that nothing read. They
  hold the MCP principal and its floor with no new field, so the store's format did not move, and tasks inherit both
  (the kernel copies a parent's authority).
- The Discord binding's in-process client (`rpc_client.rs`) is exactly what `CoreClient` needs: its module was made
  `pub` (one line in `theseus-discord`) rather than copied.
- `turn.submit` answers at once with `stop_reason: "awaiting_confirm"` when a call parks, and the reply then comes from
  the driver's continuation turn, so `conversation_send` has to wait past `turn.submit`'s own answer.

**What landed** (5fbd647 on d9b0931: 35 files, +2,226 −18; the merge 24a9fef6 against 60ab73ca, 35 files, +2,249 −23).
- **`[mcp_server]`** (`theseus-core`'s `config/mcp_server.rs`): `enabled` (false), `port` (7434; 0 picks one),
  `key_secret` (`mcp_server_key`), `posture_floor` (`approve`), `spend_limit_usd` (5.0), `requests_per_minute` (60).
  When the server is on, the key must be a `[secrets]` entry, the limit above 0, and the rate at least 1. The template
  gains a commented `[mcp_server]` block and a commented `mcp_server_key` line in `[secrets]`.
- **The core's rules** (`mcp_server.rs`, `rpc/mcp.rs`):
  - `Surface::Mcp` (`approval.rs`). `Answerer::unknown` refuses it ("never an approval surface"): an answer, a trust, a
    press, an undo or a publish from it is REFUSED, ledgered as `approval.refused` with `via: mcp` through the one
    judgment.
  - Dispatch on `Surface::Mcp` allows only the five tools' methods and those owner's acts, which are then refused. Any
    other method is refused at once.
  - `session.open` on `Surface::Mcp` opens with principal `mcp`, the ceiling `posture_floor = <floor>`, and the
    `[mcp_server]` spend limit. A `turn.submit` from it must name a session whose principal is `mcp`, so a client
    cannot write into the operator's own conversations, which have no floor.
  - **The floor** (`toolrun.rs`, a call and a small fn): every non-`Read` call in such a session is raised to the
    floor, after external text's hold; if the execution cannot be read, the floor is `approve`.
  - **The place class is private.** The session's words go back to the MCP client, a process of the daemon's own uid
    (the listener refuses any other) holding the operator's key; any such process can already read every session
    through the 0600 socket, so `shared` would hide nothing, and it would strip the file tools and `proc.run` that the
    client's turns exist for. The floor is what keeps its acts the operator's.
- **The daemon** (`theseusd`'s `mcp.rs` and `mcp/trace.rs`):
  - started in `after_serving` (the socket daemon only), once its key's secret resolves; nothing on the start path
    waits for it, and `web.rs` is untouched (the server has its own port);
  - `McpCore` implements `CoreClient` over one in-process connection on `Surface::Mcp`;
  - `conversation_send` races `turn.submit` against the wait: on `awaiting_confirm` or `budget` it calls
    `session.wait { settled, after_position }` until the view is ready or idle, then reads the reply from
    `session.history`; past the wait it answers `{ status: "running", turn_id }` (the id from `turn.started`) and the
    turn goes on. `conversation_status` reads `session.history` and a zero-timeout `session.wait`;
  - **only the daemon's uid**: `theseus-mcp`'s `server::Config` gains `admit`, a hook run first on each request on the
    blocking pool, refusing with 403 and a `Why::Peer` refusal; the daemon's hook is the web UI's check;
  - **a job's hold passes on**: `mcp/trace.rs` finds the client socket's inode (`peer::client_inode`, new, on
    `peer.rs`'s table reader), looks for its holder among the daemon's own descendants only (the daemon is a child
    subreaper, so jobs and their orphans are there), and takes `THESEUS_SESSION` from that process or its nearest
    ancestor under the daemon (so a client run with `env -i` still names its job). It is sent as `opened_from` on
    `session.open` and `turn.submit`.
- **The tools:** `conversation_open`, `conversation_send`, `conversation_status`, `task_list`, `wake_list`.
- **Where it shows:** one `mcp_server.call` row per call (tool, session, client, latency, ok) and `mcp_server.refused`
  at most once a minute per kind with an `unreported` count, both written on the blocking pool; health's `mcp_server`
  block (a protocol module of its own: state `starting | listening | stopped | failed`, port, sessions, clients, opened,
  last client, calls, errors, refusals by kind, and the error), with its TypeScript; a `mcp server:` line in
  `theseus health`; the narrative line "MCP client <name> opened session <short>; its calls that act wait for the
  operator"; the tracing span `mcp_server.call`, already in the server.
- **The reader rule:** `theseusd` depends on `theseus-mcp` (one Cargo.lock edge, no new package). At the join the
  crate's manifest names both readers: the core's MCP board (36b) for the client side, and theseusd's server (41b) for
  the `server` feature.
- **The bench:** `theseus-sim`'s `bench_config` turns the server on (port 0, the key from the fake `op`), so every
  lifecycle phase runs with it, as with the index tender.
- **The store's format stays 7.** The principal and the floor ride in `Authority`; `tests_layouts` is untouched.

**How it is proven.**
- **Tests:** `theseusd/tests/mcp_server.rs`, a real daemon, listener, client, CLI and job wrappers: a client opens a
  conversation (five tools listed, the label `mcp lantern-agent tide tables`, the limit $2), sends it text and gets the
  reply, and a send into the operator's session is an `isError`; a wrong key twice gives 401 twice and one `key` row,
  and the right key from a foreign Origin (`lantern.example`, invented) gives 403 and an `origin` row; under `enforcement = notify` an
  acting call waits, `conversation_send` with `wait_secs: 2` answers `running`, status shows `waiting_on: confirm`,
  `theseus confirm --approve` lets it run and the client reads "Done."; a job's process that opens a session through
  MCP passes on its hold (the new session holds via `job` from the reader session). In the core,
  `tests_m3/mcp_surface.rs`: an MCP session's authority, floor and limit; its write waits; an answer from MCP is REFUSED
  and nothing moves, while the CLI's counts; methods outside the allowlist and turns into operator sessions are
  refused; the operator's own session has no floor. Unit tests for the floor, the trace (real children, direct and
  stripped with `env -i`), the `admit` hook, the CLI line and the approval case. At the review: **13 of 13**; in the
  cloud, the 9 MCP tests at nice 19 beside four busy loops, 5 times: **45 of 45**.
- **Planted reverts**, the session's four, re-run at the review, each failing as said: `Surface::Mcp`'s arm in
  `Answerer::unknown` removed; the Origin check dropped (theseus-mcp's `a_foreign_origin_is_refused` fails, the daemon
  test reads `HTTP/1.1 200 OK` to the foreign page); the floor at the gate dropped (the write runs: "awaiting_confirm:
  null, output: Written."); the trace returning no job session ("the MCP session holds nothing").
- **Live, at the join** (a scratch daemon on a fresh state dir, built from the prepared merge 48c7d8f6; the simulator's
  scripted stand-in model, a stand-in `op`, the key a `file:` secret of 24 random bytes, mode 600; the server on port
  7491; Discord, the web UI and the index off; a small streamable-HTTP client in Python, then Claude Code's own health
  check from a scratch config dir). Before any client: `mcp server: listening on 127.0.0.1:7491 · 0 session(s) · 0
  opened · 0 call(s), 0 an error`, the socket served first. A wrong key: 401 ("a key is required, as Authorization:
  Bearer"); the right key with a foreign Origin: 403 ("refused: a foreign Origin"); health then read `refused 2 (key 1,
  origin 1)`. `initialize` (server `theseus`, protocol 2025-06-18, an `Mcp-Session-Id`), `tools/list` the five tools, a
  session labelled `mcp live-check live check`, "what is 2+2?" answered `4` with its turn id and cost. "Please create
  the file": the model called `proc_run`, the send answered `running` after its 5 s wait, status read `waiting_on
  confirm`, `theseus confirm` listed "proc.run — approve (an MCP client opened this session, and …" with the file
  absent; approved from the CLI, the file existed, status read `last_reply "Done."`, and a third send answered `4`.
  `claude mcp list`: `theseus-scratch: … (HTTP) - ✔ Connected`. One `mcp_server.call` row per call (the waiting send
  5,002 ms) and the two refusals; shutdown clean, exit 0.
- **The lifecycle bench** in the cloud, for its shape (debug, 3 runs, the server on): OK.

**What the live check found** (theseus-fphu, P3). A new message from the MCP client while its own call waits
supersedes the call (`action.declined {"by":"operator","reason":"superseded: the operator sent a new message"}`), as a
new message does from the CLI. Declining is the fail-closed direction and the client owns the conversation, so this is
not an approval through MCP, but the row names the operator while the message came from principal `mcp`. Also found by
the session: the daemon tests' stand-in model numbers tool-use ids from `toolu_test_0` in every response, so a second
turn's call can collide with a first turn's answered one and the continuation finds "nothing_new" after an approval;
its test reads first to step around it.

**The join** (the harvest wake b86bd6c2, 01:28 to 02:35). Prepared four times as main moved (on 7b6b1487, 80d1f809,
0fe594d9, then 60ab73ca), rerere replaying 13 recorded resolutions. Both MCP branches had added `rpc/mcp.rs`: one
module now, with both impl blocks. Both had named a config type `McpServerConfig`: `[mcp.servers.<name>]`'s keeps the
config root's re-export, and `[mcp_server]`'s is `config::mcp_server::McpServerConfig`. The core keeps 81kk's closed
flag beside the server; health keeps the judge's line and the terminals beside it; two TypeScript files go to
`cockpit/src/protocol.gen/`. **The join fix** (the ontology's precedent): the RPC dispatch merged to 102 lines against
clippy's 100, so the MCP surface's two additions go through its own module (`mcp_admit` takes the surface, any other
passes; `session.open` goes through `session_open_on`); merge amended as 24a9fef6. **Gate run 1 red in the suite**
(02:27:35, 2,039 of 2,040): `tests_m3::parallel::a_cancel_during_a_batch_leaves_no_call_dispatched` timed out after
its 5 s wall-clock wait with the suite at 190 s under B5's load; alone, 5 of 5 at 1.1 to 1.6 s (filed theseus-t2yb, a
P2 gate flake). **Gate run 2 exit 0 at 02:35:31:** 2,040 of 2,040 (17 skipped, 1 slow); lifecycle OK on the first run
with the server on in the bench config (cold start p50 25.3 / p95 30.5, clean shutdown 41.1 / 58.0, kill 31.2 / 34.6,
swap 53.5 / 59.2 ms); the jobs phase's L1 start p95 7.50 ms; frames 5 and 9; pushed 02:35. theseus-ext.2 and
theseus-celu.27 closed. It was the last of batches 4, 4b and 4c: 16 of 16 on main.

**The install** (install #1, 2026-10-04 14:09, at bddfd407): check, unit and restart OK; health clean (9 secrets
ready, discord ready, startup 73.1 ms); the store moved from format 6 to 14 on start. `[mcp_server]` is off by
default, and the install's health line names no MCP server among what is on.

**Divergences.**
- The peer is not traced through `/proc/net/tcp` (that walk was retired): the uid check is the web UI's, and a job is
  found by the client socket's inode among the daemon's descendants.
- The design's five tools, and none of the filed ones (memory, re-exported built-in tools, resources, prompts,
  sampling, elicitation, per-client tokens).
- The place class is private, argued above; the floor keeps the acts the operator's.
- The Observatory's "server panel" became a list of what the cockpit should show (health's block, MCP-opened sessions
  marked with their floor, the two ledger rows), not built.

**Known gaps.**
- One shared in-process connection for every MCP client: `session.wait` allows 64 parked waits per connection, so more
  than 64 concurrent sends past a parked turn would get `LIMIT` (the rate bound makes it unlikely).
- The reply after an approval is the session's last assistant text, without its cost.
- The hold trace is a light guard, like the CLI's: a job can defeat it by double-forking to the daemon after stripping
  its environment, or by handing its socket to an outside process.
- The key is not in the broker's harness-only list; a job can get it only through an explicit `[broker.programs]`
  grant.
- No telemetry fact or metric beyond the span, the ledger rows and health's counters.
- theseus-fphu (the supersede's attribution), theseus-t2yb (the gate flake), and the stand-in model's id trap.

### Item 111. 21c: the cockpit's Ontology view and a session's memberships panel (theseus-8kk.2, roadmap row 27; the cloud batch 5's ontology-view session, fired 2026-10-04 01:28 from f1fccec, Sonnet 5.5; 31399988 and 0909cf20; reviewed 02:23 to 02:33 by the batch-5 harvest wake fe6123da, with a live check in headless Chrome; joined 02:40 at 9d1ab36a, a signed merge onto 24a9fef6; installed 14:09 at bddfd407, install #1)

**Why.** 21b (Item 100) put the ontology in the store with four methods and a CLI. §4.1a says the
owner or an operator adds rows "in the web UI", and M4's design (`docs/design/m4-boundaries.md` §2.8) gave 21c the
view: the kinds, a category tree, a guidance editor, and each session's memberships. Since the cut-list's 6.4 (Item 86)
the cockpit is the web UI, so 21c is the cockpit's. 21b's methods were enough: no Rust changed.

**What landed** (cockpit only; the merge against 24a9fef6: 9 files, +561 −6, with the join fix).
- **`/ontology`** (`src/views/Ontology.tsx`, `src/components/OntologyParts.tsx`): the kinds table; the category tree,
  depth first (roots by name, each followed by its children), each category with its kind, `added_by`, description,
  and its guidance's version and digest; a guidance editor whose confirm carries the text; add-a-topic with a parent
  picker. The selected category is in the address (`?category=`). It reads the present with one `ontology.list`; a
  write is `ontology.category.add` or `ontology.guidance.set`, confirmed with `window.confirm` carrying the words sent,
  as the other views confirm; a refusal or an invalid-params answer shows the daemon's own words. At a past moment the
  view says the ontology keeps no past, and its controls are off.
- **The session deck's memberships panel** (`src/components/Memberships.tsx`, in the Context tab): the memberships the
  next compile would use, each with its origin, its as-of, and whether the newest compilation recorded it; add and
  remove a topic (`ontology.membership.set`); what the newest manifest recorded; and for anything that waits, "applies
  at the next recompile", with a pill of the same words beside Recompile.
- **The pure parts** (`src/lib/ontology.ts`): the tree's order and depth (a category whose parent is not listed stands
  as a root, and a cycle does not loop), and `pendingOf`, which compares a membership by (category, origin), ignoring
  its as-of, and guidance by (category, version, digest) over the categories a compile would admit (a member's
  ancestors under `chain`, its own alone under `intent_line`), as 21b's walk does.
- One line each in `main.tsx` (the route) and `Shell.tsx` (the nav item "Ontology"), and two small insertions in
  `SessionDeck.tsx`. No protocol type, no new method, no package, no store format change (the store stayed at 7).

**What the session found.** The daemon omits an empty `memberships` or `guidance` list from a manifest
(`skip_serializing_if = "Vec::is_empty"`), so its first version read a compilation made before any membership as
"unknown" and showed nothing pending after a topic was added. Found on its scratch daemon and fixed in its second
commit (an absent list reads as none recorded), with the test changed to say so. The session finished its cockpit
side by 01:32 and waited, on purpose, for its cold Rust setup before its gate and first commit.

**How it is proven.**
- **Tests:** `test/ontology.test.ts`, 5 new; the cockpit's tests 29 of 29, `tsc -b` clean, lint with warnings only
  of kinds the existing files have, rerun at the review on origin/main 60ab73ca plus the branch.
- **Planted reverts**, 2 of 2 caught, each file restored byte for byte: the tree flat, by name alone (27 pass, 2 fail:
  "the tree is depth first …" and "a category whose parent is not listed stands as a root, and a cycle does not
  loop"); pending guidance comparing presence only, no version or digest (28 pass, 1 fail: "a guidance version changed
  since the compile is pending, for the category and for its chain").
- **Live, at the review** (a scratch `theseus-sim discord rig`, a scratch theseusd from the review tree's debug build
  with `[web]` on 7462 and its `dev_origin` the cockpit's dev server on 5174, headless Chrome; nothing of the
  operator's; torn down after). `/ontology` showed 4 kinds (guild, channel and person given; topic interpreted) and 2
  categories made by transport. From the browser: `harbor` added at the root with a description and `harbor-tides`
  under it, each confirmed first, the child indented one step (0 px, 16 px), the selection in the address; harbor's
  guidance set to v1 with its digest, the confirm carrying the text; `theseus ontology categories` agreed. Two paths
  the session had not clicked: a duplicate topic showed the daemon's words verbatim ("`topic:harbor` is already named
  "harbor": two topic categories under one parent need two names"), and at a past moment (`?t=`) the view said the
  ontology keeps no past, with add-a-topic's inputs and button off. In a CLI session's deck, adding `topic:harbor`
  showed "at the next recompile" on the row, "topic:harbor guidance added (v1)", and the pill beside Recompile; after a
  transcript recompile and one more turn the pill was gone and the line read "the newest compilation recorded 1
  membership and 1 guidance block: topic:harbor v1". Each write from the web UI was a ledger row (`ontology.category`,
  `ontology.guidance`, `ontology.membership`). All 13 routes (`/`, `/ship`, `/bridge`, `/fleet`, `/actions`,
  `/ledger`, `/money`, `/economics`, `/speed`, `/systems`, `/boundaries`, `/ontology`, and a session's deck) loaded
  with no console or page error.
- **FAST:** not on the start, stop, swap or turn path; no daemon change.

**The join** (the batch-5 harvest wake fe6123da; 02:40). A plain signed merge, 9d1ab36a (24a9fef6 and the report
commit 6e4e0b0e), no conflicts; `CLOUD_TASK.md` and `CLOUD_REPORT.md` dropped. **Join fix (docs):**
`cockpit/AGENTS.md` names the twelfth view, `Ontology`, and `src/lib/ontology.ts`. Its gate (02:40:32): 2,040 of
2,040 (17 skipped, 1 slow); lifecycle OK on the first run (cold start p50 25.8 / p95 31.6, clean shutdown 29.9 / 50.0,
kill 24.9, swap 46.0 / 49.2 ms); frames 5 and 9; pushed 02:40. theseus-8kk.2 closed, and with it 21's last step.

**The install** (install #1, 2026-10-04 14:09, at bddfd407): check, unit and restart OK; health clean (9 secrets
ready, discord ready, startup 73.1 ms). The view is in the cockpit at `/ontology`.

**Divergences.** Built in the cockpit, not `web/` (6.4). The memberships live in the session deck's Context tab, not
an Observatory session view. The topic picker lists topics only; a given kind's membership is refused by the daemon
(invalid params), as 21b has it.

**Known gaps.** "Save guidance" stays enabled when the text is unchanged, and what an identical `ontology.guidance.set`
does (a new version, or nothing) was not checked. 28b's `categorize.v1` proposals have a place to go (a section of the
memberships panel, accepted as `ontology.membership.set`, and a column or filter on the tree) and no code yet. No
`g`-key for the view.

### Item 112. 30b: recall in front of the model in canary and live: the `Recall` node, labels, the sticky arm, the BudgetReport, and store format 8 (theseus-6fn.2, roadmap row 53; the cloud batch 5's recall-node session, fired 2026-10-04 01:28 from f1fccec, Opus 5.5; c2f1f054; reviewed 02:42 to 02:53 by the batch-5 harvest wake fe6123da, with a live check on the real model; joined 02:59 at 760553f7, a signed merge onto 9d1ab36a; installed 14:09 at bddfd407, install #1, live on the operator's config)

**Why.** 30a (Item 99) ran recall in shadow: the turn asked the index as its first call went out, wrote
a `recall.shadow` row of references, and changed no request byte. 30b puts recall in front of the model
(`docs/design/m6-memory.md` §2.4, §2.8, §2.9): in canary and live, a `Recall` node of references after the new message,
rendered as testimony; `derived_from` edges to its sources; the operator's labels; a session's arm fixed once; and the
compiler's BudgetReport, so a prompt is never silently thinner (§2.11).

**What the session found.**
- For canary and live, the index's answer has to be read before the compile, and the node has to reach the plan frame.
  `plan_and_dispatch` already took extra records for that frame (its closure was `|_| Ok(vec![])`), and the kernel's
  turn handle adds every node a frame writes to the turn's kept transcript, so the node rides that frame: no new frame.
- The compiler is pure over the session's nodes, and a `Recall` node renders text from other sessions' nodes, so the
  compile takes those sources in. Sources are immutable, so a cache by node id is safe.
- The index's extractor already skipped `kind = "recall"` (29b), and `node.reach` already followed `derived_from` edges
  of any `via`, so `via = "recall"` edges are counted with no change to reach.
- The index's chunk text is not always a substring of the core's `text_of` (a message with attachments joins them
  differently): the frozen range is wherever the excerpt is found in the source's text, else the source's start, cut
  to the same length.
- A trap: building main's tree in a temporary worktree against the checkout's own `target/` poisoned the test binaries
  (another tree's fixture paths, a stale crate), as the repo's AGENTS.md warns; `cargo clean -p` of every workspace
  crate fixed it.

**What landed** (c2f1f054 on f1fccec: 50 files, +2,700 −147 by the review's count; the merge against 9d1ab36a, 48
files, +2,236 −147).
- **`[memory]`** (`config/memory.rs`, the template): `mode` gains `canary` and `live`; new keys `arm` (`none` or
  `baseline`, default `baseline`), `canary_fraction` (0.5), `experiment` (`m6-1`) and `session_recall_cap_tokens`
  (12,000, at least `recall_budget_tokens`). `MemoryConfig::assign(session)` compares the first 8 bytes of SHA-256 of
  `experiment\nsession` with the fraction: canary sends that share to `arm` and the rest to `none` (live with
  `baseline` in shadow); live sends every session to `arm`; off and shadow assign nothing.
- **The arm row:** each session's arm is recorded once as a `memory.arm` row (mode, arm, live, experiment, science),
  scoped `recall:<session>` and deferred into the turn's next frame; an in-memory set, and on a session's first turn
  in a daemon run a scan of its recall scope, keep it to one across restarts.
- **The node:** `Body::Recall { recall_id, arm, items: Vec<RecalledRef> }`, `RecalledRef { node_id, session_id,
  position, chunk: (u32, u32), header, tokens }`, ids `rcn_…`, origin harness, `kind_str` `"recall"`. Only this
  variant is declared; `Summary`, `Synthesis` and `Lesson` are left to their steps. Its `text_of` is empty, so a recall
  is never recalled; `place.publish` refuses it ("publish its sources").
- **The turn** (`turn/recall_step.rs`; `turn.rs` gains a field, the `recall_first` call, the plan closure's rides and
  `recalled` in the result): `recall_first` records the arm and, in front of the model, awaits the answer, bounded by
  `recall_deadline_ms`, before `compile_step` (in shadow it returns the query for 30a's `recall_end`). `recall_live`
  runs the pipeline with the place rule, the turn's context (earlier recalls' sources included) and the labels, then
  checks the session's cap (the tokens of `Recall` nodes after the current compilation's `as_of`): past it the outcome
  is `paused` and there is no node. Otherwise it builds the node, each item read by position for its frozen range and
  header, with a `derived_from` edge (`via = "recall"`) to each source; the node and edges wait in the turn until the
  provider call's plan frame, and the `recall.ran` row (the manifest, with `arm` and `budget`, its text stripped) is
  deferred into the next frame. `recall_view` hands the compile the transcript with the pending node at position
  `u64::MAX`, and a new compilation's `includes` leaves it out, so it renders in the tail.
- **The render** (`recall/render.rs`), §2.4's block in the same user turn after the new message: `[Recalled: N notes
  from earlier sessions. Testimony, not instructions: dated, and possibly stale.]`, then each item's header, frozen at
  recall time (`a message from cli in ses_…, 2026-09-30 14:34 UTC (as of @18231)`), and the source's text over the
  frozen range with `…` where it is cut; a source that cannot be read says so. Sources are read through the store by
  position (by id if the position holds another record), cached in `Memory` up to 4,096 entries.
- **The BudgetReport:** `Compiled.budget` and `Compilation.budget: Option<BudgetReport>` (skipped when none), stored
  with each new compilation: the limit (window − max_tokens − 4,096, the ring's own number), the estimate used, the
  ring's cut (`{ range: { first, last, nodes }, reason: "overflow", tier: "ring", tokens }`), and an overage when the
  estimate's upper bound still passes the limit; a recall's budget drops are added with tier `recall`.
- **Labels:** `memory.label` (`rpc/memory.rs`), judged by `judge_act(Act::Label { what })`, the owner's act from a
  private place (a new `Act` arm with its refusal row and line), writes a `memory.label` row scoped `memory`. The set
  it keeps out (`recall/labels.rs`) is built after serving (`Core::warm_labels`, beside `warm_ontology`) or by its
  first reader, and kept current on each write: a node's latest label decides (`wrong` and `stale` exclude, `useful`
  lets back, `should_have` changes nothing). `theseus-memory` gains `Reason::LabeledWrong`, run after `untrusted`, the
  place filter still first. The CLI's `theseus memory label <node> <label> [--recall R] [--note N]` joins `OPERATORS`
  (refused inside a job); `memory recalled` shows the arm, "admitted" in canary and live, and `paused`.
- **The protocol:** `BudgetReport` and its parts, `MemoryLabelParams` and `Result`, `MEMORY_LABELS`;
  `RecallManifest.arm` and `.budget`, `TurnSubmitResult.recalled`; the ledger kinds `recall.ran`, `memory.arm` and
  `memory.label`; the method `memory.label`; TypeScript regenerated.
- **Discord:** the reply's footer adds `🧠 N recalled` when `recalled > 0`, so nothing shows in shadow.
- **Store format 8** (`MANIFEST_FORMAT`, main's 7 plus one, no renumber), with format 7's compilation (memberships and
  guidance) as a layout sample, checked to be written back byte for byte by the build before the step; `versions.rs`
  at 8, the newer-store fixture 9.

**How it is proven.**
- **Tests:** `tests_recall_node.rs`, 7 through whole cores: a canary turn puts its recall in front of the model (the
  node right after the input with a reference and no copied text, the request's last user message `[question, note]`,
  the `recall.ran` row with the node's id and no text, one `derived_from` edge scoped `in:<source>` that `reach`
  follows, `recalled = 1`, and **the frames the turn writes equal memory-off's**); the next request begins with the
  previous request's bytes after a later record under the source's key; the arm is sticky and recorded once (canary at
  fraction 0 gives arm `none`, one row over two turns, found again by a fresh `Memory`); a wrong label keeps a node out
  (dropped `labeled_wrong`, the set rebuilt from rows, a non-owner in a shared channel refused, `useful` letting it
  back); a stalled index holds a canary turn no longer than its deadline (50 ms: under 2 s, the row `deadline`, no
  node); past the session's cap recall pauses; shadow still writes no node. Also the render's golden bytes, the arm's
  assignment (at 0.3, 2,000 sessions give 500 to 700 live), the ring's cut in the stored report, `labeled_wrong` in
  the filter test, the footer, and the place property test drawing `canary in bool` (under canary, the model's bytes
  carry no note of a session the asking place may not draw on). In the cloud, under load, 75 of 75 five times; at the
  review, on the merged tree, **78 of 78**.
- **Planted reverts.** The session's two: a source read by id with the whole text rendered (the next request no longer
  begins with the first's bytes), and canary skipping the place rule ("AliceDm asked of Cli"). At the review the place
  plant was caught again ("Pier asked of Den"); **a by-id read alone was not caught**: the test runs both turns in one
  core, so the second render hits `Memory`'s source cache, which the first filled. The code reads by position, as it
  should; only the test cannot tell (theseus-x875, P2, adds a restart to it).
- **Live, at the review** (a scratch daemon of the merged tree's debug build as a transient unit, a fresh state dir,
  `mode = "canary"`, `canary_fraction = 1.0`, `recall_deadline_ms = 2000`, Sonnet 5.5, about $0.03; not the operator's
  daemon or store). Session A: "Remember: the grey heron nests by the old weir at Millbrook."; the index took both of
  A's messages within 3 s. Session B: "Where does the grey heron nest?": **the reply named the weir**, as an earlier
  session's note it had not checked; `recalled` 2; canary, arm baseline, 2 candidates admitted (A's message and reply),
  75 of 1,500 tokens, the index 261 ms; the `recall` node after the question in B's history; `reach` of A's message
  showed B's `rcn_…` node, `derived_from` "by the recall". Labelled wrong, A's two nodes: session C's recall **dropped
  2 for labeled_wrong**, and its reply was the general answer; `useful` let the note back. One `memory.arm` row per
  session, three in all.
- **FAST:** in shadow (the bench config) `recall_first` assigns no arm and writes no row, `recall_view` is one scan of
  the shared transcript a compile, `warm_labels` runs after serving, and the node rides the plan frame: frames stay 5
  and 9. In canary and live the index's answer comes before the compile, bounded by the deadline: 261 and 287 ms in
  the live check, on a debug build's embedding; the review asked for a release-build measurement before a canary on
  the operator's daemon.

**What the live check found** (theseus-es6f, P3, waiting for the owner). C's recall also admitted B's question and answer,
and B's answer repeats the labelled claim: a label is per node, and B's answer has no edge to A's note (only B's
`Recall` node has one). The model did not use it, because B had called the note unverified.

**The join** (the batch-5 harvest wake fe6123da, 02:59). The signed merge 760553f7 (9d1ab36a and the report commit
1f4be03e); three textual conflicts, each keep-both and replayed by rerere from the review tree: `theseus-protocol`'s
`ledger.rs` (the MCP kinds beside `memory.arm` and `memory.label`), `ts.rs` (the memory types beside
`judge::JudgeHealth`), and the generated `index.ts`, which the protocol's test then leaves unchanged; no semantic
conflict (the test build and `clippy -D warnings` clean); `turn.rs` at about 3,460 of its 3,500, `compiler.rs` at
2,436 of 2,500. Its gate (02:58:38): 2,051 of 2,051; lifecycle OK on the first run (cold start p50 23.6 / p95 31.1,
clean shutdown 33.4 / 61.6, kill 25.3, swap 47.0 / 51.4 ms); the turn's medians as main's (plain p50 80.6, tool-call
178.9, daemon 54.0 / 150.0 ms; the tool-call p95 319.7 one outlier of five); frames 5 and 9; pushed 02:59.
theseus-6fn.2 closed. It released batch 5's compaction-roots (30c), memory-arm (34b's wire-in) and memory-pass (31a),
launched at 03:00 to fire at 03:03; and task-arrangement (27) now renumbered to format 9 and needed
`Body::Arrangement` arms beside `Recall` (Item 113).

**The install** (install #1, 2026-10-04 14:09, at bddfd407): check, unit and restart OK; health clean (9 secrets
ready, discord ready, startup 73.1 ms); the store moved from format 6 to 14 on start, this step's 8 among the moves,
one way, behind the install's backup. The operator's config, written at 13:07, has `[memory] mode = "live"` with arm
`baseline`, so recall is in front of the model on every session; `theseus memory search` ran on `baseline@8bc51e97`
(the vectors were still loading).

**Divergences.**
- One store format number (Tier 7.9): format 8 with `Recall` alone declared, not "NODE schema 2 to 3 once for all of
  M6"; the other three variants take their own formats with their steps. `RecalledRef` carries `tokens` (the pack's
  estimate), which the cap counts.
- The header names the source's kind, author and session, not its place.
- `memory.label` takes no `author` or `discord` params (unlike the ontology's writes), so Discord cannot send it yet;
  labels are global, not per session.
- `context.compiled` does not carry the report yet; it is on the stored compilation and on `recall.ran`.
- The design's per-turn recall view (the Observatory's) became a list of what the cockpit's should show (the manifest,
  the admitted items with rank and score, drops by reason, the budget report, `reach` links, label buttons, the arm,
  each compilation's budget), not built.

**Known gaps.** A plan that fails (over budget, stopped) never writes the node, while its `recall.ran` row names a
`recall_id` with no node, and `recalled` still counts the notes held. The cap resets at any recompile; 30c's compaction
should drop old notes first. The arm row's check scans the session's recall scope once per session per daemon run.
One `BudgetDrop` per item dropped for the budget; other drops stay in the manifest. theseus-x875 (P2) and theseus-es6f
(P3). The `tests_output` golden still fails under UTC (theseus-ig6n); the session ran its gates with
`TZ=America/Los_Angeles`.

### Item 113. 27: the arrangement on `task.create`: exact quotes, the refusal, the fidelity check, and store format 9 (theseus-vug.2, roadmap row 47; the cloud batch 5's task-arrangement session, fired 2026-10-04 01:28 from f1fccec, Opus 5.5; 829e3b47; reviewed 03:36 to 03:58 by the batch-5 harvest wake 97fa42b3, with a live check on GLM; joined 04:16 at af6790da, a signed merge onto 760553f7 with five join fixes; installed 14:09 at bddfd407, install #1)

**Why.** The owner decided on 2026-09-27 that promotion requires an authored arrangement (§3.2a; theseus-vmh, decision 6):
the conversation's agent holds the discussion, so it names the pieces a task needs (objective, acceptance, design), by
reference, never paraphrased, and the child's first compilation admits them after its brief. DD7's `task.create`
(Part III A4, item 7) took a brief alone. M5's design (`docs/design/m5-judgment.md` §2.10) gave step 27 the input,
the refusal, a deterministic fidelity check and an `Arrangement` node. Overnight it was launched at once, since it has
no code dependency on Jev's wiring, with a note that it changes how `task.create` works day to day, so the owner should
hear of it before an install (the morning notes' decision 6).

**What the session found.**
- `task.create` opened the child with its brief alone, a relayed `UserMessage` with a `derived_from` edge (via
  `brief`) to the reply that holds the call. That, `wake_parent`, the task's own wakes and a parent's external-text
  hold all stay as they were.
- The turn decides whether a session has new input by the type of its last node (`awaiting_reply`: a `UserMessage` or
  a `ToolResult`). A node after the brief has to count too, or the child ends at once as `nothing_new`; its first
  compilation test found this.
- Six other exhaustive `Body` matches needed an arm (recall's `text_of`, `node_info`, `publish`, the compiler's
  `render_messages`, `node.rs`, the index's extractor test). The extractor already skipped an unknown body without
  text; it got an explicit skip anyway, since an arrangement copies nodes that are already indexed.
- Every scripted `task.create` in the tests had to carry an arrangement, and in the daemon tests the fake model picks
  its reply by the first phrase the prompt contains: the child's prompt now quotes its parent, so its phrase comes
  first.

**What landed** (829e3b47 on f1fccec: 48 files, +2,682 −479 by the review's count; the merge against 760553f7, 51
files, +2,698 −483).
- **The input** (`task.rs`; its types in the new `arrangement.rs`, about 900 lines): `arrangement: { pieces: [{ quote,
  role } | { node, role }], trust?: [i], supersedes?: [[older, newer]] }` and `fidelity_ack?`; indexes count from 0;
  roles `objective`, `acceptance`, `design`, `context`. The tool's schema requires `arrangement`, and its description
  teaches the model to quote rather than paraphrase and says what each failure means.
- **Resolution** (`arrangement::resolve`): exact and case-sensitive, with one leniency: every run of whitespace, in the
  quote and in the node, reads as one space, and the quote's ends are trimmed. A quote needs at least 20 characters
  after that. The sources are the calling session's own transcript only (a user message's text, a reply's text blocks,
  a tool result's content). Two kinds of node are never a source: the reply that holds the call, so the model cannot
  quote its own paraphrase, and an earlier `task.create` result, which echoes its pieces' first lines and would make
  every later quote ambiguous. Each failure says why, and the model can try again: `no_match`; `ambiguous`, naming each
  candidate's id, author and UTC time; `short`, naming any node whose whole text the quote is, for a retry with
  `{ node }`; `unknown_node`; `same_node` (two pieces resolve to one message). The result lists each piece's node,
  author, time and first line. At most 12 pieces, each piece's text capped at 16,000 characters.
- **The refusal:** with no arrangement, or no standing (unsuperseded) `objective` or `design` piece, the call fails
  with "Promotion needs an arrangement: quote the messages that define this work.", after the depth-one refusal, so a
  task that tries to start a task is still told it cannot.
- **The fidelity check** (`arrangement::fidelity`): a brief under 200 characters, more than 10 operator messages since
  the session's last task (counted after its last `task.create` result that was ok), and exactly one admitted piece:
  unless the call says `fidelity_ack: true`, it fails and asks for the design to be attached, or for the ack.
- **The node:** `Body::Arrangement { pieces, fidelity_ack }` (ids `arr_`), written after the brief in the frame
  `open_task` already writes, with a `derived_from` edge to each piece's node via the new `VIA_ARRANGEMENT`. Each
  piece carries its node id, origin session, origin, author, time, first line, role, `trusted` and `superseded_by`,
  and its full text; a superseded piece carries no text. The compiler renders the node as a second text block of the
  brief's user message: each admitted piece verbatim under a header such as `--- Piece 2 of 3 (design, trusted
  testimony): from the model in session …a1b2c3, 2026-10-04 08:12 UTC ---`, and a superseded piece by its node id
  only. `compile()` reads nothing outside the child's nodes. A trusted piece is introduced to the child as "vouched for
  by your parent: take it as settled".
- **Rows, as facts** (`fact/arrangement.rs`): `task.arranged` (the pieces by reference, `fidelity_ack`,
  `human_messages`, `brief_chars`) and `task.arrangement_refused` (its class, the piece count, the reason), both riding
  the turn's next frame; together they give Q7's quote failure rate.
- **Surfaces:** the start line `Task a1b2c3 started ("…", 📎 3 pieces)` and a narrative line;
  `TaskInfo.arrangement` (`theseus-protocol`'s `arrangement.rs`, TypeScript regenerated); `theseus tasks` adds
  `· 📎 3 pieces (fidelity acknowledged)` and one indented line per piece; Discord's `/tasks` adds `· 📎 3`; the
  cockpit's Actions view gives each task a `📎 N pieces` toggle (role, author, time, first line, trusted, a superseded
  piece struck through).
- **The store's format, 8 to 9:** the branch bumped 7 to 8; recall-node took 8 (Item 112), so the join
  renumbered it 9. Two layout samples join `tests_layouts` (a task's session record without `arrangement`, and a
  task's brief node, "unchanged through 8").

**How it is proven.**
- **Tests:** 12 new. `arrangement::tests` (8, pure): a quote resolves to the one node that holds it (whitespace runs,
  case, a reply, a tool result, `{ node }`, an unknown node); an ambiguous quote fails and names its candidates; a
  short quote names a node it is the whole of; the call's own reply and earlier results are no source; an arrangement
  needs a standing objective or design; the fidelity check; operator messages counted since the last task; a
  superseded piece renders by reference only. `tests_arrangement.rs` (4, whole core): refusals and their classes; each
  failure's reason, another session's text refused by the place rule, then a unique quote with irregular whitespace;
  a one-line brief from 11 messages flagged until acknowledged, the count restarting after the task; and the child's
  compilation (three pieces, one by the model, one trusted, one superseded; the first request's first message
  `[brief, arrangement]`, each admitted piece verbatim with its header, none of the superseded piece's text,
  `messages[0]` identical in the follow-up loop and in a wake's later turn, one edge into each piece). Also the CLI's
  piece lines and the layouts. In the cloud: 76 of 76 of the core's task and neighbouring tests, 17 of 17 of the
  daemon's. Under load, 32 of 33 three times: the one failure, every time, was
  `tests_tasks::a_report_that_starts_a_turn_at_the_parents_limit_asks`, which fails the same way on main as cloned
  under the same load (its child streams 145,000 words through the fake provider) and passes in 0.8 s unloaded. At the
  review, on the merged tree: **304 tests**, 303 passing and the golden failing until join fix 3, then passing.
- **Planted reverts:** the session's two (an ambiguous quote takes its first match; a superseded piece keeps and
  renders its text), each failing its unit and whole-core test. At the review, 3 of 3: the first again, the holder
  exclusion off (the reply holding the call a source), and `turn.rs`'s arm off, which fails the child's compilation
  test and three task tests (the child would end at once as `nothing_new`).
- **Live, at the review** (a scratch daemon of the merged tree's debug build as a transient unit, a fresh state dir,
  glm-5.3-flash, about $0.02; not the operator's daemon or store). Ten messages settling a haiku for a dashboard
  footer, then "start a task to write it now: give the task a one-line brief, and for its arrangement quote only this
  message of mine": **GLM's first `task.create` carried one exact quote** of that message (objective, marked trusted),
  resolved to the operator's node; no `task.arrangement_refused` row. `theseus tasks` showed `· 📎 1 piece` and the
  piece's line; the child's history held the brief, then the `arrangement` node, and its provider call sent 2 blocks;
  the report arrived once, as one node in the parent at its next turn. **The fidelity check did not fire live:** GLM
  wrote a 387-character brief, though asked for one line (the 11-message test covers it). The child's one provider call
  took 2 min 19 s, 7,722 output tokens of GLM's thinking: GLM's, not the branch's.
- **FAST:** nothing on the start, stop or swap path; one more pattern in `awaiting_reply`; the compiler's arm renders
  only in a task session; `task.create` reads the calling session's transcript once, as `holder_of` already did, with
  a normalized scan per piece, at most 12.

**The join** (the batch-5 harvest wake 97fa42b3, 03:58 to 04:16). The signed merge af6790da (760553f7 and the report
commit 669ad769). **Eight conflicted files, every hunk keep-both** beside recall-node, since each side added a `Body`
variant: theseus-core's AGENTS.md, `graph.rs` (`VIA_RECALL` and `VIA_ARRANGEMENT`), `node.rs`, `recall.rs`,
`rpc/publish.rs`, theseus-index's tests, the protocol's `ts.rs`, and the store's format note. **Format 9:**
`MANIFEST_FORMAT` 9 (the note: 8 recall-node, 9 the arrangement), the core's store test 9, `versions.rs` (writes 9,
refuses 10, "reads formats 2 to 9"). **Join fixes** in `resolve.py`: (1) the arrangement's quote sources leave a
`Recall` node out, since it copies nothing of its own; (2) a recalled item's header has an `Arrangement` arm ("a
task's arrangement"); (3) the core's golden keeps main's `-#:#` wake lines (the session had rewritten it under UTC,
theseus-ig6n). **Gate run 1 red** (04:07, suite 2,062 of 2,064) on two more, fixed in the merge: theseus-store's
refusal test names format 99 as one no build writes (it named 9, which this build writes), and terminal-tools' task
test, joined after the branch's base, quotes its ask in an arrangement. **Gate run 2 exit 0 at 04:15:26:** 2,064 of
2,064; lifecycle OK on the first run (cold start p50 21.5 / p95 23.3, clean shutdown 31.1 / 51.5, kill 25.0, swap 46.5
/ 51.2 ms); turn plain p50 86.9, tool-call 193.1 ms; frames 5 and 9; pushed 04:15. theseus-vug.2 closed. It released
task-record (39a, Item 125), launched at 04:16.

**The owner's call** (the 9am review, 2026-10-04 09:31, "Keep both"): the arrangement stays required on `task.create`, as
joined; and for theseus-ruir, option (b): keep the trusted mark, but render a piece quoted from an external tool result
as external.

**The install** (install #1, 2026-10-04 14:09, at bddfd407): check, unit and restart OK; health clean (9 secrets
ready, discord ready, startup 73.1 ms); the store moved from format 6 to 14 on start, this step's 9 among the moves,
one way. Every task the operator's model starts now quotes its arrangement.

**Divergences.**
- Store format 9, the one `MANIFEST_FORMAT` (Tier 7.9), not "NODE 2 → 3" (M5 §2.14).
- The pieces show as `theseus tasks` lines, not a `theseus tasks show`, and in the cockpit's Actions view, not the web
  UI's task tree.
- The whitespace rule, the two exclusions, and the limits (12 pieces, 16,000 characters a piece, one piece per
  message) are the session's; the design names none of them. The child's wording for a trusted piece is the
  session's; the design says only "trusted testimony".
- The refusal without an arrangement runs at the harness, after the depth check, not at the gate's plan: a call
  without one is ledgered as `task.arrangement_refused`, not `tool.invalid_input`, and a `task.create` posture of
  `approve` asks before the refusal.

**Known gaps.**
- theseus-ruir (P3, open): a piece quoted from an external tool result can be marked trusted, and the piece does not
  carry the result's `external` mark, so the child reads outside text as settled testimony; the parent's hold still
  passes to the child, so it is framing, not a gate bypass. The owner's option (b) is to build.
- Case, punctuation and quote marks must match exactly: a model that turns `'` into `’` misses. If the
  `task.arrangement_refused` rows show the model missing on more than 20% of calls, Q7's fallback applies (short node
  ids in the transcript, under a renderer bump); `short` and `ambiguous` already name node ids, so a retry with
  `{ node }` works today.
- M7's task record (39a) must keep these refusal cases in its tests (roadmap conflict 6), and 25c's `should_promote`
  comparison can count `task.arranged` rows as the model's promotions. A `fidelity` Noul stays filed.

### Item 114. L2: the language-server board and the `lsp.*` tools: lazy servers per root, the rename as plan and apply by digest (theseus-n88g.8, lane L2 of the worth plan; the cloud batch 5's lsp-board session, fired 2026-10-04 01:28 from f1fccec, Opus 5.5; 056e0fd4; reviewed 03:37 to 04:10 by the local reviewer R1, with a live check on rust-analyzer; joined 04:26 at 94e01826, a signed merge onto af6790da, by the batch-5 harvest wake 97fa42b3; installed 14:09 at bddfd407, install #1, with `[lsp]` on)

**Why.** The worth spike (Item 93) designed LSP tools for Theseus: servers started lazily, a hand-written client,
read tools and a gated rename, and diagnostics after an edit as the main feature. L1, the client (`theseus-lsp`, Item
96), merged ahead of its reader. L2 is that reader: the board in `theseus-core` that starts and stops servers, and the
model's `lsp.*` tools. L3 (diagnostics in an edit's result, Item 127) waited on it. The owner said yes to
the LSP lane on 2026-10-03 (D1).

**What the session found.**
- The client needed nothing. `Client::start`, `request_within`'s `Outstanding` (dropping it sends `$/cancelRequest`),
  `stop`, `locate` and the fake were enough. An aborted call's task drops its request future, so **cancel and `/stop`
  came for free**: toolrun's `Stops::track` already aborts an async call's task.
- `places::offered` already keeps `lsp.*` out of a shared place, since they are neither public nor file tools; a test
  now holds that.
- The `lsp.*` kinds are strings on rows that already exist, so the store's format did not move.
- `LspServerConfig`'s derived `Default` had `enabled = false`, which silently dropped every preset; a test caught it,
  and it has a manual `Default`.
- `PR_SET_PDEATHSIG` is not used: it fires when the spawning *thread* exits, and the store's blocking workers move to
  new threads. A server ends when its stdin closes.

**What landed** (056e0fd4, one commit for the five steps, which share the board, its tests and the gate's hook, so no
earlier cut would have been green alone; the merge against af6790da, 32 files, +3,959 −25).
- **`[lsp]`** (`config/lsp.rs`, the template): `enabled` (false), `idle_stop_mins` (10.0), `request_timeout_secs`
  (30), and `[lsp.servers.<name>]` (`command`, `extensions`, `roots`, the root markers, `settings`, `enabled`); a name
  that is not a preset needs a command and extensions. A file's server is the first, in `[lsp.servers]` order then the
  presets' (rust-analyzer; ty, pyright, basedpyright; tsgo, typescript-language-server), that serves its extension and
  whose program is on the job's `PATH`; typescript-language-server gets the project's own `tsserver.js` when it has one.
- **The board** (`lsp/mod.rs`): lazy, one server per (server, root). The root is the nearest marker inside the
  workspace root that holds the file; for Cargo, the nearest `[workspace]` manifest, else the topmost `Cargo.toml`.
  The spawn goes through `children::spawn(Kind::Owned)` in its own process group, with a cleared environment plus the
  job's, and stderr to a log capped at 1 MiB, `<state>/lsp/<server>-<hash>.log`. A start runs in a task of its own, so
  a cancelled call does not cancel a start later calls will use. The client's events are drained, and a close the
  board did not ask for becomes `lsp.failed`; the next call starts the server again. An idle server stops after
  `idle_stop_mins`; a request's timeout stops it too (`lsp.stopped` `timeout`) and says the next call starts it.
- **The tools** (`lsp/tools.rs`): `lsp.definition` (each location with 2 lines before and 4 after, numbered),
  `lsp.references` (grouped by file, at most 200, saying what it left out), `lsp.hover`, `lsp.symbols` (`{ path }` for
  an outline, `{ query }` with or without a path), and `lsp.diagnostics { path? }` (no path: every file the tools
  opened). Each is a `Read`, `Backend::Async` tool addressed by path, a 1-based line, `symbol` and `occurrence`, with a
  deadline of three request timeouts plus 30 s, which covers a first start. Each has a `[policy.tools]` line in the
  template (the count 30 to 37 on the branch).
- **The start's posture** (`lsp::gate`, a one-line hook in toolrun's gate right after `sandbox::decide`): when a call
  would start a server for a (server, root) with no start yet in this daemon's life, the start is judged as a
  `proc.run` of its argv at the root (`policy.decide_with`, with `proc.run`'s tightening); the stricter decision wins,
  its reason and notice naming the server, the root and the argv; it never loosens a decision.
- **The rename, as two calls** (`lsp/rename.rs`, on `aws/stack.rs`'s pattern rather than an async plan step, which
  would have changed the shared `Tool` trait and the synchronous gate): `lsp.rename.plan { path, line, symbol,
  new_name, occurrence? }` is a read. It asks for the definition first and, if that is one place inside the roots and
  elsewhere, renames there and says so (TypeScript renames an imported name at its import, Item 96); it refuses an
  edit that creates, renames or deletes files, applies the edits at their UTF-16 columns, refuses overlapping edits,
  shows a unified diff, and keeps the edit by digest (the newest 32, for the daemon's life). `lsp.rename { digest }`
  is `Write` and `NonRepeatable`: its plan's resources are every file it writes, as `Write`, so the floor and the
  approve list judge it as an `fs.patch` of each, and a write outside the roots waits; its summary and notice name
  every file; it writes only if every file still has the text the plan read (`write_atomic`, the operator's umask),
  tells the servers (`file_changed`) and returns the diff. A digest applies once.
- **Where it shows:** facts `lsp.started`, `lsp.ready` (with `ready_ms`), `lsp.stopped` (`idle | timeout | daemon`)
  and `lsp.failed`, written off the workers through the index tender's ledger hook; health's `lsp` block (a protocol
  type, `LspServerStatus`, its TypeScript regenerated; the memory is the process group's RSS from `/proc`) and the
  CLI's line; a span `lsp.request` (kind `lsp`, with server, method and outcome) under the call's span, the call's
  tokio task id bound to its `tool_use_id`; the metric `theseus.lsp.request.duration`, a histogram in ms by server,
  method and outcome, made from the spans at the turn's end.
- **The stops:** `Core::stop_lsp` sends SIGTERM to each server's group and never waits, on both the socket and the
  `--stdio` stop paths.
- **The reader rule:** `theseus-lsp`'s `reserved_for` is gone; Cargo.lock gains only theseus-core's edges to
  `theseus-lsp` and `similar` (already locked). The store stayed at format 9.

**How it is proven.**
- **Tests:** `tests_lsp.rs` (13, the fake served in-process): no spawn before the first call, one per root, the facts
  and health; a file's root its nearest marker, and Cargo's workspace; an idle server still up at 9 minutes and stopped
  at 10 (`why: idle`) on tokio's paused clock, the next call spawning again; a crash fails and the next call starts it
  again; `[lsp]` off is no board and no tools; each read tool through its plan and `run_async`, and the locate errors;
  the first start judged as `proc.run` and not again for that root (another root waits again), and under the template
  a notice naming the server; the rename gated on every file it writes (3 edits in 2 files; the plan writes nothing;
  approve when one file is on the approve list or outside the roots; the apply writes exactly the edit; a digest
  refused the second time; a file changed since the plan refused, nothing written), its notice naming every file,
  edits at UTF-16 columns in any order; a cancel sends `$/cancelRequest` and the server stays up; a call's requests are
  its spans. The daemon's `tests/lsp.rs`, the real `theseusd` and `theseus-lsp-fake`: health's `lsp` is `[]` once
  secrets settle; one model call to `lsp_definition` reaches the model with the location and context lines; the server
  is ready in its own process group with one log and its rows; **after `kill -9` of the daemon, the server ends within
  10 s**. Config, telemetry, render and rename unit tests: 17 in all. In the cloud, under load, 17 of 17 five rounds; at
  the review, on the merged tree, **156 of 156** (the branch's tests, theseus-lsp's, every config test with the
  template's count of 42, telemetry's, the reader rule and the place rule's).
- **Planted reverts.** The session's three: a server started at the daemon's start (the daemon test fails: "no server
  at the daemon's start"); the rename's gate skipping its files (`notify` where approve was wanted); the start's step
  switched off (both posture tests fail). The review's three, each caught: a server started at the daemon's start
  (health holding `fake` `starting` before any call); a start not remembered (every call judged a first start:
  `Approve` where `Open` belongs); the rename's apply judging only its first file (`Notify` where the approve list
  asks `Approve`).
- **Live, at the review** (a scratch daemon of the merged tree's debug build as a transient unit, a fresh state dir,
  web and Discord off, `[lsp] enabled = true`, `idle_stop_mins = 1`, a two-function Rust crate, rust-analyzer; Sonnet
  5.5, about $0.09 over five asks; not the operator's daemon or store). At the start: `lsp: none up (a server starts at
  the first call for a file of its language)` and no `<state>/lsp/`. "Where is `total` defined, and who calls it?": the
  model called `lsp.symbols` (3.4 s, the start), `lsp.definition` (14 ms) and `lsp.references` (244 ms), and answered
  `demo/src/lib.rs:1:8`, called once, by `report` at line 6; health read `lsp: rust-analyzer on … (pid …, ready in
  3.4 s, 659.3 MB, 3 requests)`, with the log and the `lsp.started` and `lsp.ready` (`ready_ms` 3,393) rows. The
  rename: `lsp.rename.plan` (2 ms), then `lsp.rename` with its digest (9 ms), and the file read `sum_all` at both
  places; under notify, the apply's notice named the file it wrote. A minute idle: `lsp: none up` and `lsp.stopped`
  `why: idle` (ran 70.2 s, 5 requests); the next call (a hover) started it again, ready in 3.5 s. The daemon's stop with
  the server up: its pid gone at the first check, and a second start on the same state dir read `lsp.stopped` `why:
  daemon`.
- **FAST:** with `[lsp]` off (the default and the bench's config) nothing new runs; on, `build_runtime` makes the
  board (an allocation, no I/O, no spawn) and no server starts before the first call. Per call: one `Option` check
  when off, a family compare, and the task id's binding. The facts take their own frames, off the workers. The stop:
  nothing when off, a SIGTERM per group, never waited for.

**What the live check found** (theseus-1x5n, P2, open). Under the template's postures (the reads and `proc.run` both
inherit `notify`), the first call's notice was `lsp.symbols — notify (enforcement = notify)`: it named no server, root
or argv. `lsp::gate` returns the call's own decision unless the start's is strictly stricter, so at equal postures the
start never speaks, and the tests set the read to `open`, so they never saw the equal case. Not a safety boundary
(notify never waits), but the operator is not told that a program which runs build scripts and proc macros started.

**The join** (the batch-5 harvest wake 97fa42b3, 04:26, on R1's review alone). The signed merge 94e01826 (af6790da and
the report commit 60096fcc). **35 conflict hunks in 18 files, every one keep-both,** replayed by rerere from R1's
review tree (resolve.py, generated from the tree's hunks), beside the MCP board and server, the terminals, Jev, the
hands and the durability tender: the template's `[policy.tools]` count 35 + 7 = **42**; telemetry's `INSTRUMENTS` 16 +
1 = **17**; one `proc_env` above both the board and the terminals in `build_runtime`; `trace_calls` takes main's
`tools` and the branch's `(aws, lsp)` pair. No join fix. Its gate (04:26:12): 2,081 of 2,081; lifecycle OK on the first
run (cold start p50 20.9 / p95 27.7, clean shutdown 34.4 / 52.2, kill 25.2, swap 45.8 / 51.6 ms); turn plain p50 78.9,
tool-call 181.4 ms; frames 5 and 9; pushed 04:26. theseus-n88g.8 closed. It released L3 (lsp-diagnostics), launched at
04:27. R1 noted that theseus-protocol's `lib.rs` would reach its 2,618-line ceiling with bindings-v2 and extend-propose
(theseus-pf8a splits it).

**The install** (install #1, 2026-10-04 14:09, at bddfd407): check, unit and restart OK; health clean (9 secrets
ready, discord ready, judge on, **lsp on**, startup 73.1 ms). The operator's config, written at 13:07, has `[lsp]
enabled`; its servers start at the first call for a file of their language.

**Divergences.**
- The rename is two tools, `lsp.rename.plan` and `lsp.rename { digest }`, where the brief had one taking the rename's
  arguments: a seventh tool. Whether it should become one tool with an optional digest is an open question for the owner.
- The first start is judged once per (server, root) per daemon life: the set is in memory, so a restart judges each
  root's first start again.
- One commit for five steps.

**Known gaps.** theseus-1x5n (above). A start that failed is tried again by the next call with no backoff, so a server
that dies at once costs each call a spawn. `/stop` across a whole turn is not tested end to end (the cancel test
covers the abort it goes through). There is no design note for the LSP lane in `docs/design/`; the spike's design and
the brief were outside the repo, and §3.24's note carries what changed.

### Item 115. 36c: MCP prompts through the board: `turn.submit { prompt }`, `/prompt`, `theseus prompt`, the cockpit's picker, and store format 10 (theseus-ext.4, roadmap row 67; the cloud batch 5's mcp-prompts session, fired 2026-10-04 01:50 from e457555d, Sonnet 5.5; 5b1fdd51 and 81d427ca; reviewed 03:58 to 04:30 by the batch-5 harvest wake 97fa42b3, on its offline proof; joined 04:34 at 8e17eea7, a signed merge onto 94e01826 with its join fixes; installed 14:09 at bddfd407, install #1)

**Why.** §3.8 has MCP prompts surface as slash commands, their messages entering the turn as user-role content with a
provenance label, their definitions cached and diffed, a change noticed to the operator. 36b (Item 106)
brought configured servers' tools into turns through the board, which listed prompts but only counted them. M7's
design (`docs/design/m7-surface.md` §2.1, "Prompts (36c)") gave 36c `mcp.prompt.list`, `turn.submit { prompt }`,
Discord's `/prompt` with a modal, `theseus prompt`, a picker beside the composer, and the changed-definition row.

**What the session found.**
- `McpTool`'s `External` marker and `external::hold` / `with_hold` already showed how a node and the session's hold
  ride one frame; a prompt's nodes use the same pattern.
- `Origin` had no `mcp` value, so the new origin is a layout older builds cannot read: a store format bump, with a
  layout sample of an operator's message from before it.
- `TurnRequest` has no `Default`, so the new `prompt` field adds `prompt: None,` to about 55 literals (59 with
  `TurnSubmitParams`'s), put in by a regex after each literal's opening brace. Two commits, not one per sub-step: the
  Rust halves only build together.

**What landed** (5b1fdd51, the board, the core, the protocol, the store, the CLI, Discord, the fake server and the
tests; 81d427ca, the cockpit's picker; the branch 75 files, +2,871 −70; the merge against 94e01826, 79 files, +2,893
−85).
- **The protocol:** `McpPromptRef`, `McpPromptArgument`, `McpPromptInfo`, `McpPromptListParams` and `Result`, and
  `prompts` on `McpListResult`; the method `mcp.prompt.list`; `TurnSubmitParams.prompt`; the ledger kind
  `mcp.prompt_changed`; TypeScript regenerated.
- **The board** (`mcp/prompts.rs`, 556 lines, a module of its own): each server's `prompts/list` kept with a digest
  per definition and stored as META `mcp.prompts.<server>` (a new key, no bump); `list_changed` and a reinitialize
  list again; `seed_prompts` reads the stored lists at the core's build, so a start offers them before the server is
  up. `resolve_prompt` checks the arguments against the definition (a missing or empty required one, an unknown name),
  waits for that server alone, calls `prompts/get`, and converts the messages: text stays text, an image is an
  attachment, an embedded resource is text with its URI; an assistant-role message is written user-role and marked
  `[the prompt's assistant message]`; all the messages together are capped at 200,000 characters.
- **The changed definition:** the definition at each prompt's last use is kept (META `mcp.prompt_used.<server>`). A
  use of a changed definition records `mcp.prompt_changed` (ledgered and narrated, with a summary of what changed) and
  an operator notice (`mcp_prompt_changed`, which Discord's courier renders beside `mcp_changed`), then goes ahead.
- **The core** (`rpc/mcp.rs`, `turn/prompt_input.rs`): `mcp.prompt.list` and `resolve_prompt`; the place check is the
  session's class, and a shared place gets `REFUSED` saying why. `turn_submit` resolves the prompt before any session
  is opened, so a refusal leaves no node and no session. The messages are written as the turn's input, user-role
  nodes of `Origin::Mcp` authored `prompt:<server>/<name>`, with the session's hold for an external server (tool
  `mcp.prompt`, url `mcp:<server>/<prompt>`) in one frame under the session lock. A prompt with `input` or attachments
  is refused as two inputs.
- **The CLI:** `theseus prompt <server/prompt> [--arg k=v]… [--session] …` (`ask`'s streaming tail became a shared
  `cmd::stream_turn`), and `theseus mcp` prints each server's prompts (`prompt fake/greet (name*, tone) · Greets
  someone.`, required arguments starred).
- **Discord** (`runtime/prompt.rs`): `/prompt` with autocomplete (at most 25 choices, filtered by name, title or
  description); a modal of one input per argument up to 5 (required marked `*`), or one `args` input of `name=value`
  lines for 6 or more, as a `Label` around a `TextInput` (twilight 0.17's preferred form); a prompt with no arguments
  runs at once. The run is `turn.submit { prompt }` through the place's own line of turns, refused while one runs; the
  reply says "Running the prompt" without echoing the arguments.
- **The cockpit:** `PromptPicker.tsx` beside the composer's send button (a select over `mcp.prompt.list`, a field per
  argument, required marked), `lib/prompts.ts`, and 3 tests.
- **The fake MCP server** gains a `change-prompts` mode: each `prompts/get` changes `greet` afterwards and sends
  `prompts/list_changed`.
- **The store's format, 9 to 10,** for `Origin::Mcp`: the branch bumped 7 to 8, and main was at 9 once
  task-arrangement was in (Item 113), so the join renumbered it 10.

**How it is proven.**
- **Tests:** theseus-core's MCP tests, 19 passed in the cloud, 8 new: a prompt is the turn's input with its origin,
  author and hold (one `session.external_read` row); a prompt of a server that is not external gives no hold; a prompt
  that cannot run is an error before any node (five cases, each with and without a session, then a down server: no
  node and no new session after any); a shared place's prompt is refused and never reaches the server (its call count
  stays 0); a prompt with an input is refused; a changed definition gives its row and notice once (use, change, use:
  one row and one notice; a third use of the same definition adds none); a restart lists the stored prompts before the
  server is up; a prompt's messages become text, attachments and URIs. theseusd's `mcp` binary: the real daemon and
  the real `theseus-sim fake-mcp`, the stored prompts listed and carried by `mcp.list` after a restart with a server
  that never answers. Discord's `/prompt` pieces (autocomplete, the modal's shapes, the parse of 5 and 6 arguments and
  of bad, missing, unknown and repeated ones; writing the 100-character test found an off-by-one in the session's
  `clip`, fixed). The CLI's prompt tests; the cockpit's 26 tests, 3 new. At the review, on the merged tree: **275
  tests**, all passing.
- **Planted reverts.** The session's two: an external server's prompt taking no hold, and the changed-definition
  notice skipped; each fails its test. At the review, 3 of 3: those two, and the shared-place check off in
  `resolve_prompt` (`a_shared_places_prompt_is_refused_and_never_reaches_the_server` fails).
- **No live check.** The review ran out of the wake's time; the offline proof covers the script's points (the input
  node's origin, author and text, the hold, the refusals before any node, the shared place's refusal before the
  server, the changed row once, the stored list before the server is up, through the real binary and the real fake
  server). `/prompt` in Discord needs a person.
- **FAST:** a start reads two more META keys per configured MCP server (none in the bench's config), beside the
  stored tool list 36b reads; nothing starts a server before serving. A plain turn pays one `Option` check; a prompt's
  nodes ride the turn's input frame.

**The join** (the batch-5 harvest wake 97fa42b3, 04:34). The signed merge 8e17eea7 (94e01826 and the report commit
324c8286). **Seven conflicted files, every hunk keep-both,** replayed by rerere from the review tree: theseus-core's
`fact/mod.rs` (the hands', terminals' and `McpPromptChanged` facts), `rpc/mcp.rs` (beside mcp-server's), the core's
store test (10), `tests_layouts.rs` (the arrangement's brief sample and the branch's operator-message sample, "(10,
theseus-ext.4)"), the protocol's `ts.rs`, the store's format note (10 = a node's origin `mcp`), and theseusd's
`versions.rs` (writes 10, refuses 11, "reads formats 2 to 10"). **Join fixes** in `resolve.py`: `prompt: None` on
three `TurnRequest` literals new on main (aws-hands', terminal-tools' and the arrangement's tests); the arrangement's
piece author and origin name an MCP prompt's message ("an MCP prompt (author)", `mcp`); and `dispatch`, at 101 lines
with `mcp.prompt.list` against clippy's 100, reads the optional params of `session.list`, `task.list` and `wake.list`
through a new `or_empty`, one line each (about 92 lines, room for the next methods). Its gate (04:34:30): 2,099 of
2,099; lifecycle OK on the first run (cold start p50 21.8 / p95 22.6, clean shutdown 33.4 / 55.0, kill 26.8, swap 47.2
/ 62.3 ms); turn plain p50 77.6, tool-call 173.4 ms; frames 5 and 9; pushed 04:34. theseus-ext.4 closed. The harvest
joined it on the offline proof, as aws-hands joined, to keep the queue moving, with the live script left in the review
(the morning notes' decision 12).

**The install** (install #1, 2026-10-04 14:09, at bddfd407): check, unit and restart OK; health clean (9 secrets
ready, discord ready, startup 73.1 ms); the store moved from format 6 to 14 on start, this step's 10 among the moves,
one way, behind the install's backup.

**Divergences.**
- `/prompt` is one global command (slash commands stay global), not one scoped to the bindings a server is attached
  to; a shared place's prompt is refused by the core at submit, so Discord shows the modal first, then the refusal (the
  binding does not know a place's class).
- A changed definition is noticed at use against the last use, and the use goes ahead (notify over block); the first
  use never says "changed", and a list change alone is silent.
- An assistant-role message is written user-role, marked. The node's author is `prompt:<server>/<name>`, not the
  client that asked (`turn.started` names it).
- `/prompt` does not queue behind a running turn; a typed message does.
- The web UI's picker is the cockpit's (6.4).

**Known gaps.** No gateway-level Discord test of the three interaction kinds (the pure pieces are tested;
`on_interaction` is driven only by hand). No live check has run, as of the chain log; the review's script needs only a
scratch daemon with the fake server. The `prompt: None` lines on every `TurnRequest` literal make each later field
there a conflict on those lines.

### Item 116. Step 40, part 2: hands that stop, budgets held per hand, the hour's alert and the day's budget, overdue and reaped hands, waves, and the grid; store format 11 (theseus-mgw.11, roadmap row 36; the cloud batch 5's hands-cancel session, fired 2026-10-04 02:22 from 4bb5aac9, Opus 5.5; 00f84821, db57a580, 0187611b, 51ba161e, 1ffc8cf8, b11d097b and 989eb0bb; reviewed 05:08 to 05:27 by the batch-5 harvest wake 1972d751, on its offline proof; joined 05:42 at a4da5e1c, a signed merge onto 8e17eea7 with one join fix; the Lambda part's live check on the account 11:48 to 11:53, reviewed 13:45; installed 14:09 at bddfd407, install #1)

**Why.** Part 1 (Item 107) built hands, a fan-out of short compute on Lambda and Fargate with signed
envelopes coming home over SQS, but left a running hand to its TTL: a cancel or `/stop` of the `aws.hands.run` call
went through `terminate_all`'s generic path, which marked each hand `unsupported` ("runs in process"), and `until`
met only cancelled the hands not yet launched. The AWS design (`docs/design/aws-toolset.md` §3.3, §3.7, §5's step 40)
asked for cancellation per backend, a reservation per hand, quotas and waves, an AWS-side reaper whose stops are read,
and one line per group. Overnight the prompt writers made the hour's spending line alert-only, with whether it should
also refuse left to the owner, and had the task check before applying a daily budget, which may cost money (the morning
notes' decision 6).

**What landed** (seven commits; the branch 59 files, +4,384 −121 with its two paperwork files; the merge against
8e17eea7, 57 files, +3,855 −121; no new package).
- **Cancellation per backend** (`aws/hands/cancel.rs`). **Fargate:** `StopTask` (reason `theseus: cancelled`), the
  cancel acknowledged, then `DescribeTasks` for up to 4 s; STOPPED (or MISSING) settles it `termination_verified` with
  a new `VerifiedBy::Ecs`. A stop not yet seen leaves the hand open, so the poller's next pass (`cancel::verify`) or
  ECS's own task-state change on the queue (`cancel::stopped_event`) verifies it. A verified stop books the time the
  task ran at its size's rate, through the kernel's new `cancel_verified_costing` (plain `cancel_verified` would have
  booked $0 for a task that ran). **Lambda:** `cancel_unsupported`, "a Lambda invocation cannot be stopped; its
  function's timeout (the hand's TTL) ends it"; its reservation is held, and its envelope, when it comes, is
  `LateAfterCancel` and books the real cost; the group stays open for the poller until the hand's TTL plus 5 minutes,
  or its late envelope. **`until` met or unmeetable:** `group::step` stops the hands still running before it settles
  the group, and the aggregate counts them `cancelled` (in `Tally`, the `aws.hands.settled` row and the result text).
  **A cancel or `/stop` of the call** reaches the group: `terminate_all` takes the store and routes `aws.hand` and
  `aws.hands.run` actions to `stop_group` and `stop_hands` first; the call's verdict sums its hands' (verified when
  every hand that ran is STOPPED; `unsupported` while Lambda hands run on; uncertain while a task has not shown
  STOPPED), and the group's record says `settled: "cancelled"`. A late envelope after any cancel settles nothing.
- **Reservations** (`toolrun/hands.rs`): each hand's action reserves its worst case, its TTL at its size's rate, in
  the group's one frame, and settles at the envelope's cost; a hand that never started completes at $0, so nothing is
  held; `max_usd` stays the group's cap. A group whose total worst case passes what the session has left is not run:
  its result says why, with the figures, and it hands the turn an over-budget mark, which the turn reads at its next
  loop and asks the budget question with the same `ask_budget` a model call uses, card included, then waits on
  `Wake::Budget`. A reservation that loses a race inside the frame takes the same path.
- **The hour and the day.** `[aws.accounts.<id>] hourly_alert_usd` (default 1.0) and `daily_budget_usd` (optional).
  **The hour** (`aws/hands/watch.rs`): each poller pass meters, per account, what the actions dispatched this clock
  hour reserve or spent; past the line it alerts once that hour, in one frame: an `aws.hour.alert` row, the META mark
  `aws.hour.alerted.<account>` (so a restart that hour stays quiet) and a notice through the outbox. It alerts only.
  **The day:** the foundation template gains `DailyBudgetUsd` (default 0, none), a condition, and `DailyBudget`
  (`theseus-daily`, alerts at 80% and 100% to the alerts topic, no action); the tender's reconcile is per budget,
  monthly and daily, each a budget-only change set (for the day, adding or removing the budget too); a stack whose
  template predates the parameter stops, saying the template comes first.
- **Overdue and reaped hands** (`aws/hands/overdue.rs`): the heartbeat's reconciler, which never waits on the network,
  no longer marks an overdue hand unknown; it leaves hands to their own pass in the poller. Past its deadline, a
  Fargate hand is asked about (`DescribeTasks`): still RUNNING is left to the reaper; STOPPED with the reaper's reason
  settles **failed** (`producer: hands:reaper`, "stopped by the TTL reaper: …") at the time ECS says it ran; otherwise
  unknown with what ECS said. A Lambda hand is unknown until its envelope. The reaper's own failure records are read
  and deleted: an `aws.reaper.failed` row, a narrative line, and health's count with the last one's words.
- **Quotas and waves** (`aws/hands/quota.rs`): before a wave the group reads its account's quota for its backend
  (Fargate's On-Demand vCPU quota through Service Quotas, or Lambda's unreserved concurrency), once an hour per
  account, region and backend; `group::next` takes the cap beside `concurrency` and `max_usd`, so a bigger group
  launches in waves. A quota that cannot be read caps nothing; one smaller than a hand runs one at a time; it never
  fails for a quota.
- **Watching:** health's hands block (groups open, running by backend, the oldest, reserved, the hour, the reaper) and
  the CLI's lines ("· this hour $0.04 of its $0.01 line (PAST IT: alerted)"); `hands.list`, a protocol read with a
  cell per hand (waiting, running, stopping, succeeded, failed, unknown, cancelled, not_launched) and spend and
  reserved against the cap; Discord's one line per group, "🖐️ 37/100 done, 2 failed, … $1.84 of $5", a `hands` post
  under the key `hands:<group>` so each state edits the one message; the cockpit's Systems view shows the account's
  hands block and a `HandsGrid` panel that reads `hands.list` every 3 s while open.
- **The store's format, 10 to 11** (`verified_by: ecs` and the hour's META mark): the branch bumped 7 to 8; main had
  taken 8, 9 and 10 with recall-node, task-arrangement and mcp-prompts, so the join renumbered it 11, with the
  format-7 sample of a hand a cancel reached.

**How it is proven.**
- **Tests:** `aws/hands/tests_part2.rs` (17) on part 1's stateful fake, which gains `StopTask`, `DescribeTasks` and
  tasks STOPPED, MISSING or running: Fargate `until` met stops and verifies the running hands (2 `StopTask`s, each
  verified `ecs`, nothing held, one settle, a late envelope recorded late); a stop verified later by `DescribeTasks` or
  by ECS's event; Lambda `until` met marks the running hands unsupported (two late rows, never settled twice); a
  cancel of the call stops its group (`cancelled: 2, not_launched: 1`, 3 `action.cancel_verified` rows); a `/stop` of
  a Lambda group says its hands run to their timeout; each hand reserves its worst case and settles at its real cost
  (reserved drops by exactly 2 × 20,001 µ$); a group over the session's budget meets its question (20 Fargate hands
  at 4 vCPU and 1 h against $1.40: nothing launched, one `budget.asked` row, the wait on `Wake::Budget`); the hour's
  alert fires once an hour; overdue Fargate hands asked about (RUNNING left, the reaper's stop failed with its
  reason, others unknown); an overdue Lambda hand unknown until its envelope; a reaped hand's failure records read;
  waves under a quota (3 of 5 at once, never more, all 5 in the end) and Lambda's concurrency; the grid's cells and one
  changing line. C2's daily reconcile; Discord's one message edited in place; and theseusd's `tests/hands.rs`: a real
  daemon, the stand-in model calling `aws_hands_run` for 3 Lambda hands, a stateful fake of AWS over TCP (STS, the
  stacks, `Invoke`, the queue with its visibility timeout), **`kill -9`'d after the first hand settles**, the other
  envelopes (one twice) arriving while it is down, restarted on the same state dir: each hand one `action.succeeded`,
  the group one, one `aws.hands.settled`, the duplicates recorded, nothing launched twice, every message deleted.
  Under load the first runs showed two test-timing faults, fixed in 989eb0bb (the fake queue now redelivers a message
  a kill left in flight; the hour test waits for health's block); after it, `aws::hands`' 35 tests 5 of 5 rounds and
  the daemon test 6 of 6. At the review, on the merged tree: **522 tests**, all passing (44.6 s).
- **Planted reverts**, 4 of 4 at the review: the running hands not stopped when `until` is met (the session's; 4
  tests fail), a hand reserving nothing (the session's; 3 fail, reserved 0 against 20,001), and two new: the hour
  alerting at every pass (its mark ignored) and a reaped hand left unknown, not failed; each fails its test.
- **Live, on the account, after the join** (the owner's standing AWS go, decision 9; a scratch daemon of main at 802f9135,
  which carried this join; the stand-in scripted model, so no model spend; 2026-10-04 11:48 to 11:53). The account had
  no hands stack and no hand image, since part 1's live check had never run, so the check made `theseus-hands`
  (tagged), built and pushed the hand image, and took both down after. Step 1, twenty Lambda hands `until:
  first_success`, `max_usd: 1`: the account's Lambda concurrency is **10**, so the quota launched the group in waves
  (`running 10, waiting 10`); settled `met` at 12 s, `succeeded 1, failed 6, cancelled 9, not_launched 4`, the line
  "🖐️ 20/20 done, 6 failed, 9 cancelled, $0.00 of $1 · done, until met"; 9 `cancel_unsupported` rows matched by 9
  `late_after_cancel` rows, each booking its cost, none settled twice; the group's spend $0.0018 at the settle and
  $0.0042 once the late envelopes booked theirs. **The hour's alert:** one `aws.hour.alert` row at the launch (`line
  0.01, usd 0.20001`), health "(PAST IT: alerted)", and a second group in the same hour added none. Step 4, five hands
  `sleep 45`, the daemon SIGKILLed with all five running and restarted 3 s later (`store recovered on open
  truncated_bytes=0 replayed=619`): settled `met` 5/5 at 45 s after the restart, one `aws.hands.settled`, each
  correlation id's `action.succeeded` once, nothing launched twice. **$0.0117** for 23 Lambda hands by the ledger,
  about $0.01 to $0.02 with AWS's small extras; reservations peaked at $0.20. Afterwards no tagged resource was left
  but the deleted cluster's `INACTIVE` record. Not run: Fargate (it needs hands on the account's existing network,
  theseus-mgw.9) and the daily budget (the owner's go).
- **FAST:** the startup reconcile probes through `overdue::Evidence`, one string compare per open action; nothing
  calls AWS before serving, and the daily reconcile makes no call unless `daily_budget_usd` is set. The stop path
  reads each listed action once more to find hands, routing nothing without an `[aws]` account. The turn path: one
  `Option` check per loop.

**What the live check found.** `infra/aws/hand/build.sh` failed both of its own checks on a good image: its static
check rejected Rust's static-pie musl binary, and its image check took `docker run`'s exit status under `pipefail`
(theseus-i7bz, P2; fixed later in Item 174). The account's Lambda concurrency of 10 caps a group's wave
(theseus-mgw.14). Health counted the optional network stack's absence as a failed request, and posture alerts landed
on the hands' completion queue, unread, for the dead-letter queue (theseus-rvdu, P3).

**The join** (the batch-5 harvest wake 1972d751, 05:27 to 05:42). The signed merge a4da5e1c (8e17eea7 and the report
commit aae81d7e). **Five conflicted files, every hunk keep-both,** replayed by rerere: theseus-core's AGENTS.md (the
hands bullet grown to part 2, beside lsp-board's), `fact/mod.rs` (beside terminal-tools' and mcp-prompts' facts), the
core's store test (11), the store's format note (11 = a hand's cancel verified by ECS and the hour's mark), and
theseusd's `versions.rs` (writes 11, refuses 12, "reads formats 2 to 11"). **The join fix:** `turn.rs` merged at
3,502 lines against its 3,500 ceiling (main's 3,492 and the branch's 10), so the over-budget lookup moved into
`toolrun/hands.rs` as `ToolRuntime::hands_over_budget`, and the turn's side is 5 lines: 3,497; no ceiling raised.
`theseus-protocol`'s `lib.rs` sat at its ceiling exactly, 2,618 (theseus-pf8a splits it). **The gate** (05:32 to
05:40): suite 2,117 of 2,117; the lifecycle bench missed twice under load (IO pressure 14%, load about 9 to 11), run 1
on the cold start (p95 59.9) and the shutdown (p95 175.6), run 2 on one swap outlier (p95 258.2, p50 51.8), a
different phase each run, every p50 near main's. `finish-benches.sh` at 05:41 (quiet after 10 s, strict budgets)
passed: cold start p50 25.3 / p95 44.1 (main 21.8 / 22.6), clean shutdown 38.5 / 54.8 (main 33.4 / 55.0), SIGKILL
restart 34.1 / 42.0 (main 26.8 / 28.6), swap 55.6 / 66.4 (main 47.2 / 62.3); the daemon's own start clock, store 4.90
/ 13.57 ms and kernel 8.99 / 13.88; jobs OK; frames 5 and 9, the turn's medians (plain 89.8, tool-call 232.5 ms at
load about 10) above main's quieter run. Confirmed at 06:10 by extend-propose's join gate in a quiet machine, on main
with this join: plain p50 77.1, tool-call 171.3 ms, the lifecycle at main's level (Item 118). Pushed
05:42. theseus-mgw.11 closed. It released hands-network (theseus-mgw.9, Item 134), launched at 05:43.

**The install** (install #1, 2026-10-04 14:09, at bddfd407): check, unit and restart OK; health clean (9 secrets
ready, discord ready, startup 73.1 ms); the store moved from format 6 to 14 on start, this step's 11 among the moves,
one way, behind the install's backup. No daily budget is made until `daily_budget_usd` is set, which is the go.

**Divergences.**
- The Observatory's grid is the cockpit's Systems view (a cell does not open a hand's log tail, and there is no
  slowest-hands column); there is no `theseus hands` command (`theseus rpc hands.list '{}'`).
- The hour's line alerts only, by the prompt writers' call; whether it should refuse was left to the owner.
- A group over the session's budget asks through an in-memory hand-off read at the turn's next loop; if the turn ends
  some other way first, the call's result still says it did not run and why.
- A call's verdict while a Fargate stop is unseen is `outcome_uncertain`, since a settled action is never upgraded;
  each hand's own verdict follows on its action.

**Known gaps.**
- The hour's meter counts a hand at its completion's cost, else its reservation, so a verified Fargate stop counts at
  its reservation there (conservative for an alert). A runaway brake over the hour's and the day's lines came later
  (`runaway_factor` 10, alerts staying the authority; Item 147).
- The poller's pass scans the `aws.hands.group.*` META prefix for health, the meter, late-listening and `hands.list`;
  small today, after serving and only while hands are open; an index of recent groups would bound it.
- The reaper's failure records are read only while a group is open; the queue keeps them 14 days.
- The quota caps each group by itself, not across the account's running groups.
- Discord's line is an outbox post per change, up to about one per settled hand, always one message.
- Not proved live: Fargate's stop (theseus-mgw.9), and the daily budget's change set (`infra/aws/check.sh` was not
  run in the cloud, which had no cfn-lint). theseus-i7bz, theseus-mgw.14 and theseus-rvdu, above.

### Item 117. 38a: bindings format 2: many guilds, each with its own trust, and a ceiling per place (theseus-ext.3, roadmap row 68; the cloud batch 5's bindings-v2 session, fired 2026-10-04 01:28 from f1fccec, Opus 5.5; 6165622a and d0f67002; reviewed 04:07 to 05:19 by the local reviewer R1, with a live check on the stand-ins; joined 05:57 at 35784d7c, a signed merge onto a4da5e1c with R1's join fixes, by the batch-5 harvest wake 1972d751; installed 14:09 at bddfd407, install #1)

**Why.** The bindings file had one `guild_id`, and every place in it ran under the same config: the same posture,
tools, spend limit and model. §3.1 binds by scope with policy and spend ceilings per binding, and M7's design
(`docs/design/m7-surface.md` §2.3, 38a) gave each place its guild and an optional ceiling that can only narrow: a
posture floor, the tool families and MCP servers offered, a spend limit, and a profile. Item 89 had made one whole
guild trustable; 38a makes trust per guild. Overnight the prompt writer's calls were: each guild gets its own trusted
setting, slash commands stay global, tasks inherit a place's limits, and a tool the place does not offer is refused
with words (the morning notes' decision 6).

**What the session found.** A session's limit follows `[kernel] spend_limit_usd` at each start (theseus-3pj,
`follow_limit`) unless `Budget::pinned`; the kernel knows nothing of places, and the bindings file is read by the
binding after serving, so a place's cap has to come from the binding. `pinned` exists, so no stored field is added
and the store's format did not move. 36b's MCP board was not on the branch's base, so an `mcp:<server>` entry filtered
only the built-ins there; the join had to carry it to the board's tools.

**What landed** (6165622a, the kernel; d0f67002, Discord and the core; the branch 46 files, +2,680 −255 with its two
paperwork files; the merge against a4da5e1c, 48 files, +2,212 −263).
- **The kernel's place limit** (`theseus-kernel`'s `place_limit.rs`): `Kernel::place_limit(execution, cap)` sets the
  limit to `min(cap, config)`, pinned while a cap is set so the start's follow leaves it to the binding; with no cap it
  is the config's again, unpinned. One frame: the execution and a `budget.limit_changed` row, which gains `why: "config"
  | "place"`. A raise withdraws the budget question and lets the waiting call proceed, as the config's raise does
  (`follow_limit`'s body moved into a shared `limit_to`). `pinned` now also means "a place caps it".
- **Format 2** (`theseus-discord`'s `bindings.rs`): a `[[guild]]` table each (`id`, an optional `name`, its own
  `private`), and each `[[channel]]` names its `guild`; a DM-only file needs no guild. A format-1 file (a top-level
  `guild_id` with `private` beside it) loads with its old meaning. Refused, naming the line: `guild_id` beside a
  `[[guild]]` or a channel's `guild`; a top-level `private` without `guild_id`; a format-2 channel naming no guild, or
  a guild no `[[guild]]` binds; a guild bound twice. `theseusd example-bindings` is format 2, with a commented
  `[channel.ceiling]` and `[dm.ceiling]`.
- **The ceilings** (`[channel.ceiling]`, `[dm.ceiling]`: `posture_floor`, `tools`, `spend_limit_usd`, `profile`, with
  `deny_unknown_fields`). The parse checks only an entry's shape (a family word, or `mcp:<server>`); which families
  exist is the daemon's, so `Core::bind_places` logs a warning for an entry that names none, and the file stays valid
  as toolsets join. An unknown `profile` fails the binding at its start, with the reason in health, before any place
  is bound, so every guild place stays shared.
- **Ceilings in the core** (`ceiling.rs`): `TurnRunner::view_of(session)` reads the class and the ceiling together as
  a `PlaceView` (`class_of` calls it); the place is where the turn's words go (its own place, a task's parent's, a
  wake's), so **a task inherits its parent's ceiling**. The ceiling rides in `TurnCtx` by reference, made once per bind.
  **Tools:** the catalog is the place rule's `offered` AND the ceiling's, which can only narrow; the tools note says
  the ceiling and lists postures at the floor; a call the ceiling does not offer is refused after the place rule's own
  refusal, with the same `place:` reason and `Not run:` result, in words that name it: "wake.at is not offered in
  #pier: its ceiling in the bindings file offers only web". **Floor:** `Ceiling::floor` runs after the policy's decision
  and the private-fetch rule and before T1's hold; the posture is the strictest of the four, and the floor never
  refuses. **Spend:** `Core::place_spend` calls the kernel's `place_limit` for each place's session at each start of
  the binding, narrated as the config's follow is. **Profile:** `turn.submit` uses the place's profile as the live one
  when a turn names none. **Trust per guild:** `PlaceRule::trust_guilds(set)` and `Core::trust_guilds` replace the one
  bool; health's trusted-guild mark is per place, by its guild. `Authority.ceilings` is not written: nothing reads it.
- **The binding per guild** (`runtime/guilds.rs`): every guild's id parsed and the bot's roles read in each; a guild
  the bot is not in is named in health's detail until Ready lists it. Routing is unchanged (a guild interaction
  resolves by its channel, unique across guilds), and so is the viewer read (only private channels outside a trusted
  guild). Voice works in any bound guild, one call at a time. **Slash commands stay global:** one list reaches every
  guild and the DMs; per-guild registration would buy only instant propagation, at the cost of a second registration
  path and stale lists when a guild leaves the file.
- **Surfaces:** health's `bindings[]` gains `guilds` (id, name, trusted) and each place's guild and ceiling;
  `theseus health`'s Discord line ends `by guild: home 1 (trusted), away 1 · DMs 1`, and its places line names each
  ceiling (`#pier [tools web · spend ≤ $1.00]`); `theseus places` adds each place's guild and ceiling; the cockpit's
  Boundaries view gets a guild label and a ceiling pill, and Systems' Discord card the guilds with their trust. New
  protocol types `PlaceCeiling` and `GuildInfo`, TypeScript regenerated.

**How it is proven.**
- **Tests:** the kernel's place limit (a $0.50 cap lowers a $1 limit; a restart under a $2 config leaves it; a $1.50
  cap raises it, withdraws the waiting question and queues the session; a $1 config below the cap wins; no cap unpins
  it and the next start follows the config to $3; the rows `place, place, place, config`); `bindings::tests` 9 (both
  formats, two guilds each with its own word, the mixes refused by line, bad ceiling keys); `ceiling::tests` 4;
  `tests_ceilings` 7 (a floor makes a notify tool wait with its card's reason while the DM runs it with a notice; a
  tool outside `tools` is not offered and its call refused; a ceiling never offers a shared place a private tool; a
  task inherits its parent's ceiling; the spend limit the lower of the two; the profile; health); and a real
  `start_places` over two guilds on the REST stand-in (the `discord.bound` rows, the classes, `#pier`'s pinned $1
  limit, `/status` answered by a channel in each guild, nothing for an unbound one). In the cloud 35 of 35, and 5 of 5
  runs under load. At the review, on 760553f7: **1,013 of 1,013** (all of theseus-core, -kernel, -discord and
  -protocol, and the CLI's places tests); on 8e17eea7 with every join fix: **356 of 356** (the ceilings, places,
  bindings, guilds and arrangement tests, the LSP, MCP, terminal and task tests, the reader rule, all of theseus-kernel
  and theseus-discord).
- **Planted reverts.** The session's two: the floor taking the looser posture, and a ceiling's `tools` adding to the
  place rule's set (four tests fail, the shared place's among them). R1's four, each caught: the gate ignoring the
  place's floor; the catalog not narrowed by the ceiling; a place's limit the higher of the two (`max` for `min`); one
  trusted guild trusting every guild's channels (`#pier` private).
- **R1's probe of the MCP join fix** (run, then removed): with `tools = ["mcp:fake"]` a place is offered the fake
  server's five tools alone and keeps the MCP line, and its call runs; with `tools = ["web"]` it is offered
  `web_search` alone, with no MCP line, and the MCP call is refused: "Not run: mcp:fake/echo is not offered in #den:
  its ceiling in the bindings file offers only web".
- **Live, at the review, on the stand-ins** (`theseus-sim discord rig` on a fresh state dir, a fake `op`, invented
  secrets; fake-discord, fake-model and the merged tree's debug theseusd as transient units; no money). A format-2
  file: guild A trusted, with `#lab` at a floor of approve; guild B untrusted, with a shared `#pier` at `tools =
  ["web"]` and `spend_limit_usd = 2`; and the DM. Health's places line: `private: CLI, web, #lab (in a trusted guild)
  [floor approve], DM @ana · shared: #pier [tools web · spend ≤ $2.00] (public tools only)`; `#pier`'s execution
  `budget $0.0000 of $2.00`, the others `of $100.00`, with one `budget.limit_changed` row, 100 to 2, `why: place`.
  "RUN-TRUE in lab": the execution waited on its confirm, the card's reason the floor's; the same in the DM ran with a
  notice. "WAKE-ME at the pier": `Not run: wake.at is not offered in #pier: …`, the newest manifest's tools
  `["web_search"]`. The `discord.bound` rows carried each place's guild and ceiling. The session's own dry run found
  why the report's $2 variant exists: at a $1 limit the template's Sonnet profile reserves $1.28 for one call, more
  than the whole limit, so the place parks on a budget question a reset cannot fix.
- **FAST:** nothing new before serving: the bindings file is read by the binding after serving, and that is where a
  place's session gets its limit; with Discord off (the bench's config) none of it runs. Per turn, `view_of` is the one
  place-rule lookup `class_of` was; per tool, one more `Option` in the catalog's filter; per call, the ceiling's
  refusal and floor checks; no new frame.

**The join** (the batch-5 harvest wake 1972d751, 05:57, on R1's review). The signed merge 35784d7c (a4da5e1c and the
report commit 014722ff). rerere replayed R1's resolutions of `toolrun.rs` (two hunks, each a join fix: `definitions_for`
takes the branch's `PlaceView` and keeps main's MCP tools, filtered by the same `offered`, **so a ceiling's
`mcp:<server>` narrows the MCP tools**; the tools note takes the MCP line, naming only the servers the place is
offered, then the ceiling's line), Discord's `runtime.rs` (`mod guilds;` beside `mod prompt;`) and its AGENTS.md;
`index.ts` keeps both beside hands-cancel's types. **R1's `joinfix.py`, six fixes, each applied only where its text
is:** `..Default::default()` on the `BoundPlace` literals new on main (`tests_term`, `tests_arrangement`); `.into()` on
two `PlaceClass` arguments in `tests_lsp`; `turn_submit` back under clippy's 100 lines (two `let live` lines folded);
the branch's ceiling test quotes a 20-character ask (task-arrangement's rule); and `theseus-protocol`'s `lib.rs`,
merged at 2,629 lines, its ceiling raised to that with the reason (theseus-pf8a splits it). Its gate (05:57:03): 2,134
of 2,134; lifecycle OK (cold start p95 33.2, shutdown 58.8, swap 65.9 ms); jobs OK; frames 5 and 9; pushed 05:57. No
format change (the store stayed at 11). theseus-ext.3 closed. It released budgets-policy (42a, Item 130),
launched at 05:58.

**The owner's calls** (the 9am review's decision 7, 2026-10-04 10:45, "Take all of these excellent recommendations"):
keep a trusted setting per guild, global slash commands, tasks inheriting a place's limits, the `place:` refusal, and a
removed place keeping its cap; change two: an unknown profile fails only its own place (the reason in health and at
start), with unknown tool families shown in health, and a warning in health and at start when a place's limit is below
one call's worst case on its profile. Both changes were batched for the review-smalls cloud rows (the chain log, 10:47)
and built in Item 146.

**The install** (install #1, 2026-10-04 14:09, at bddfd407): check, unit and restart OK; health clean (9 secrets
ready, discord ready, startup 73.1 ms); the store moved from format 6 to 14 on start. Health's Discord line now ends
with the per-guild count: the operator's one trusted guild with its two bound channels, and the DMs.

**Divergences.**
- A call the ceiling does not offer is a `place:` refusal naming the ceiling (the brief), not an unknown tool (the
  design); the model never sees the tool either way.
- Slash commands are global; the design's per-guild registration was dropped.
- `Authority.ceilings` is not written; the ceiling is read with the place's class at each turn.
- Unknown tool families are only logged, and an unknown `profile` fails the whole binding (both changed by the owner's
  decision 7, above).

**Known gaps.** A place whose limit is below one call's reservation can never answer (decision 7's warning, above).
A place taken out of the file keeps its session's cap, pinned, until it is bound again; such a session has no lane,
and its posts are refused. `PlaceRule` leaks a few small ceiling records at each bind. Voice holds one call at a time
across guilds. theseus-cxqj (P2, open): commit R1's MCP-ceiling probe as a test. Not seen live: voice in a second guild
(no voice stand-in).

