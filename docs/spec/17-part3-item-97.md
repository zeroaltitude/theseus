# The Ship of Theseus, chapter 17: Part III, A4's Items 97 to 107 ([index](README.md))
### Item 97. C3, AWS's curated tools: S3, Logs and CloudTrail as tools, secrets as handles, AWS text as outside text, the inventory with a reaper that reports, a daily CloudTrail cross-check, and `confirm-alerts` (theseus-mgw.5, roadmap row 31, step 14c, and theseus-9p40, with theseus-celu.22; the fourth cloud batch's aws-curated session, fired 2026-10-03 19:30 from d9b0931, Opus 5.5; 3273e88, 8898567, 5b7657c, 0721d6e, db0eaaf and 47857d2; reviewed 21:52 to 22:06 by the batch-4 harvest wake, with live checks on the account until 22:43; joined 2026-10-04 00:00 at e55e63ba, a signed merge onto 96d01de, by the harvest wake 7379d0cc; installed 14:09 at bddfd407, install #1)

**Why.** C1 and C2 (Items 49 and 78) gave Theseus one generic caller, `aws.call`, and three small curated tools, and
left a secret-bearing read invalid input "until C3". The AWS design's C3 (`docs/design/aws-toolset.md` §5, "the curated
tools, and the owner's eyes") adds the tools the usage audit ranked highest (Logs Insights, a log tail, S3 objects in
and out, CloudTrail by execution), handles for secrets, the inventory and the reaper in report mode, outside-text
marking for data-plane reads, and the CloudTrail cross-check. theseus-9p40 came from C2's bootstrap (Item 79): the
alerts subscription had been removed through the unauthenticated unsubscribe link every SNS email carries, likely by a
mail scanner, and was confirmed again by hand with authenticated unsubscribe. `theseus aws confirm-alerts` makes that a
step any operator can take.

**What landed** (`crates/theseus-core/src/aws/` with small edits in the CLI, the protocol and the secrets board; 32
files, +4,857 −58 against 96d01de; no new dependency, Cargo.lock and `package-lock.json` unchanged).
- **The tools** (`s3.rs`, `logs.rs`, `trail.rs`, 3273e88). Each is `Backend::Async`, checks its whole call in `plan`
  with no network, and carries an `AwsPlan` whose guardrail and destructive flags come from `tools::guard`, as
  `aws.call`'s do.
  - `aws.s3.get`: an object as text, or to a workspace file (a `Write` resource of the plan, so the floor and the
    roots apply). A get to a file sends `ChecksumMode: ENABLED`, checks S3's whole-object SHA-256 when there is one,
    and writes nothing on a mismatch; `range` reads part of an object, and an object over 16 MiB is read by ranges.
  - `aws.s3.put`: a workspace file (a `Read` resource) or a text, one `PutObject` of at most 64 MiB, with its own
    `x-amz-checksum-sha256` and optional tags; its class is write, so `[policy.aws] write` applies.
  - `aws.logs.query`: Logs Insights over named groups, or a `prefix` that `DescribeLogGroups` resolves; `since` and
    `until` as spans ago; it waits on tokio's timer (500 ms, doubling to 5 s) up to `wait` (60 s by default, at most
    300); the result is a table with the bytes scanned and their cost at $0.005 a GB; a query still running hands back
    its `query_id`.
  - `aws.logs.tail`: `FilterLogEvents` with a stream prefix and a filter pattern; `follow` reads on for up to 60 s,
    polling every 2 s, deduplicating events by id.
  - `aws.trail`: `LookupEvents` by execution (its session name), by resource, or by event name; each line shows the
    request id, the `call/<id>` from the user agent, the source identity and the error code.
  - A denial now names who refused, in plain words (`tools::refused_by`): Theseus's own guard (an explicit deny in a
    session policy), the session policy's narrowing, IAM, or an Organization. The template's `[policy.tools]` lists the
    new tools (its count, 24 to 30).
- **Secrets as handles** (`secret.rs`, 8898567). `aws.call` now runs a secret-bearing operation. `aws::secret::hold`
  walks the output along the operation's output shape; each member the model marks `sensitive`, or whose name is a
  secret's (`SecretString`, `SessionToken`, `Plaintext`, `Credentials`, tokens), goes onto the board under
  `aws-secret:<SecretId|Name>#<member path>`. The new `SecretBoard::hold` puts it there: zeroizing memory, never written,
  not counted as a vault round. The result shows the handle and the value's shape, masked (a JSON secret's keys, else
  its length), and `meta.secrets` lists the handles. A secret-bearing call in which no member was found to hold the
  secret returns none of its output: it fails closed. Since the scrubber reads the board, it withholds the value from
  then on.
- **AWS text is outside text** (`external.rs`, 5b7657c). `aws::external::marker` builds the same `External` a fetched
  page carries, for an object read as text (`s3://bucket/key`), log lines (`logs:<region>:<group>`, and
  `logs:<region>:query/<id>` for a query), and CloudTrail events (`cloudtrail:<region>:LookupEvents`), whenever they
  return content. The runtime writes the session's hold and its `session.external_read` row in the frame that writes the
  result. A get to a file, the listings and the inventory hold nothing.
- **The inventory, and the reaper in report mode** (`inventory.rs`, 0721d6e). `aws.inventory` (a read) calls
  `GetResources` filtered on `theseus:owner = theseus` in each of the account's regions, adds each stack's age from
  `DescribeStacks` and, with `costs: true`, each stack's month's spend from Cost Explorer by its `theseus:stack` tag
  ($0.01 a call). What AWS returns is checked again (`inventory::owned`): a resource not tagged `theseus:owner = theseus`
  is counted and named nowhere. The reaper deletes nothing (`inventory::verdict`): a stack past its `theseus:ttl` would be
  deleted whole; a stack's resource goes only with its stack; a loose ECS task or Batch job would be stopped; a loose
  resource would be deleted, or asked about when it holds state; no ttl, a ttl not yet passed, or an unreadable one is
  kept. TTLs read as the hands' Lambda reaper reads them (ISO 8601, UTC with no offset).
- **`theseus aws confirm-alerts [TOKEN] [--account]`** (`alerts.rs`, db0eaaf). The token, or the whole confirmation
  link (copied, never opened), from the argument or stdin (which keeps it out of the shell's history). The new method
  `aws.confirm_alerts` is the CLI's alone: refused unless the connection's surface is the CLI, and refused inside a job
  (`OPERATORS`, now 6), as the bootstrap is. The core calls `sns:ConfirmSubscription` on the account's `theseus-alerts`
  topic with `AuthenticateOnUnsubscribe=true`, signed by the owner role's work session (or the key before the role
  exists), reads the subscription back, and reports `ConfirmationWasAuthenticated` with the address's local part
  masked; the CLI exits nonzero when it is false. The token is never printed, logged, ledgered or kept (the params'
  `Debug` withholds it); a bad or expired token is told apart in plain words (mistyped, or older than SNS's three
  days). The bootstrap's closing text and the help say to paste the token instead of opening the link.
- **The CloudTrail cross-check tender** (`crosscheck.rs`, `tend.rs`, 47857d2). Theseus's identities are the owner
  role's sessions and the key's user. A role session's event is accounted for by its request id in an `aws.called` row,
  by its session being a job's correlation id, or by being a tender's own session (`theseus-*`); any event of the key's
  other than `sts:GetCallerIdentity` or `sts:AssumeRole` is unaccounted, whatever the ledger says. It runs daily from an
  hour after serving, for each account with an owner role, reading the 24 hours that ended an hour ago (at most 20
  pages) in the tender session, whose inline policy gains `cloudtrail:LookupEvents`. Each run writes one
  `aws.trail.checked` row (a new `LedgerKind`; no record layout changes) naming each unaccounted event's time, name,
  session and request id, and the log warns on each.
- **The store's format stays 6.** No record layout changed.

**How it is proven.**
- **The session's tests, on the fake endpoint C1's and C2's tests use** (the VM had no AWS credentials): `tests_c3.rs`
  (7 tests: each tool's request and result, invalid input with nothing sent, the file's checksum and its mismatch, the
  put's body, checksum and tags, the query's wait and table, a denial naming the guard or the session policy, planning
  that sends nothing); `secret::tests` on the catalog's own outputs for Secrets Manager, SSM `GetParameters`, KMS
  `Decrypt` and STS `AssumeRole` (every secret found, masked and on the board); `tests_handles` (a handle before any
  scrubber runs; and, by the scrubber's test pattern, a turn through the whole core reads a secret and no file under the
  state dir, no ledger row, the trace and the result node hold its value); `tests_outside.rs` (each data-plane read
  marked, a get to a file not; through the core, an object read as text holds the session and the next `aws.s3.put`
  asks, naming the object); `tests_inventory` (a fake account that answers as if the tag filter were absent, returning
  another project's resources, untagged, tagged for it, and a foreign stack: none listed or named); `aws::alerts::tests`
  (the confirmation SigV4-signed for sns, the token and `AuthenticateOnUnsubscribe=true`, the read-back, a bad token's
  words, a pasted link's token, params that never print it); `crosscheck::tests` (ledgered, job and tender sessions
  accounted for; another project's role not judged and not named; an unledgered work session and the key signing
  `iam:CreateUser` found; the window and region).
- **Planted reverts, re-run at the review** (plant.py; each failed as the report said, the tree clean after): a secret
  held but left unmasked (3 tests fail: the catalog test and both handle tests); `aws.s3.get` as text with no marker (2
  fail, `left: None, right: Some("s3://example-bucket/notes/a.txt")`); the inventory keeping every resource (fails,
  naming the other project's bucket); `AuthenticateOnUnsubscribe=false` (fails). The session's fifth, dropping the
  key's STS-only rule, failed `the_check_finds_what_nothing_accounts_for`. The review's run of theseus-core's AWS tests:
  56 passed.
- **A live check of `confirm-alerts` in the cloud**, on a scratch daemon with no AWS table: the method dispatched and
  answered "no AWS account is bound" before any token check; with `THESEUS_SESSION` set the CLI refused; no file of the
  daemon's log or state held the token.
- **Live, at the review, on the account** (a scratch daemon of the branch's debug build as a transient user unit, a
  fresh state dir, C2's scratch config plus `owner_role`; Sonnet 5.5; about $0.20 in all):
  - **Logs Insights:** the home region had no log groups; in us-east-1 a 90-day query matched 484 records of 486
    scanned, 140,277 bytes (about $0.0000), a table of `@timestamp` and `@message`; the ledger held an `aws.called` row
    for each request and a `session.external_read` naming `logs:us-east-1:query/<id>`.
  - **CloudTrail by execution:** the first check's execution, since 1 h: 4 events (`DescribeLogGroups`, `StartQuery`,
    `GetQueryResults` twice), and all four request ids and call ids equal that execution's `aws.called` rows.
  - **The inventory with costs:** 34 resources, every one tagged `theseus:owner = theseus` (the three stacks, the two
    buckets, the trail, GuardDuty, Access Analyzer, the durability table, the completions queues, the alerts topic, 19
    EventBridge rules, the budget), and none of another of the operator's projects' resources in the same account; the
    reaper had nothing to reap.
  - **A put and a get in the foundation bucket:** the put notified (`write = notify` in that config), 8 bytes, with its
    SHA-256; the get as text returned `c3 check` and marked the session; the get to a file said "S3's SHA-256 matches".
    The object's version and its delete marker were then deleted by `VersionId`, and the listing showed nothing left.
  - Not run: `confirm-alerts` with a real token (it needs a confirmation email), and the cross-check's first run (the
    scratch daemon stopped at 22:43, before an aws-l1 join gate, an hour after serving not reached).

**What the session and the review found.**
- **The scrubber matches only a secret's raw form** (the session's planted revert found it): a tool that prints JSON
  holding a secret with quotes or backslashes shows it escaped, and the scrubber misses it. The masking keeps AWS's
  handles from it; any other JSON-printing tool could leak past. Filed as theseus-ubp7 (P2, security).
- **The live check's three small gaps** (theseus-4t3c, P3): a query's own `| limit 5` was overridden by the tool's
  default (100 rows); `costs: true` showed no figures though Cost Explorer answered (no spend by the tag yet, or the tag
  not activated for cost allocation: the tool should say "no cost data yet"); the budget is listed twice, a global
  resource returned by both regions.
- **A version delete is a plain write.** The two permanent version deletes in the foundation bucket were notified,
  not asked, since the guard list does not count `DeleteObject` with a `VersionId` as destructive, and the durability
  tender keeps its snapshots in that bucket. Filed as theseus-wand; the owner decided at 23:24 to build it (§3.25 has
  carried the decision since v0.81).
- In the cloud's gates: theseus-sim's seeded-faults test failed 3 of 6 suite runs ("no series was put back", a
  coverage assertion of the repeating wakes), filed as theseus-81ig and fixed in batch 7 (Item 170); the core's
  output golden fails under a UTC clock, so the cloud gates ran with `TZ=America/Los_Angeles` (theseus-ig6n, P3).

**The join** (the harvest wake 7379d0cc, 2026-10-03 23:47 to 2026-10-04 00:00). The signed merge e55e63ba (96d01de and
e55539e); the cloud commits keep their ids; `CLOUD_REPORT.md` removed. Two conflicts: `aws/mod.rs` (aws-l1's
`mod tests_l1;` beside the four new test modules, all kept) and the two generated TypeScript files, whose directory had
moved from `web/` to `cockpit/src/protocol.gen/` (Item 86); theseus-protocol's test regenerated it and changed nothing
more. theseus-protocol's `lib.rs` merged to 2,585 lines of its 2,618 ceiling. Its gate (23:59:50): 1,883 of 1,883 (17
skipped, 1 slow); lifecycle run 1 missed on one cold-start outlier (p95 124.9 ms, p50 29.3) with the machine busy (B5's
Terminal-Bench containers, CPU 68 %, then 90 % at the rerun), and the gate's rerun passed (cold start p50 30.1, p95
37.0; the clean shutdown 89.5 and 102.6 against its 104, near the limit under that load; kill 51.9; swap 63.6 and
117.2); the jobs phase's L1 start p95 13.55 ms; frames 5 and 9; pushed 00:00. The branch adds nothing to the stop path
(its tender first runs an hour after serving), so the shutdown figure was read as load. theseus-mgw.5, theseus-9p40
and theseus-celu.22 closed.

**The install** (install #1, 2026-10-04 14:09, at bddfd407, by the operator's install script, with `/etc/theseus/theseus.toml`):
check, unit and restart OK; health named the config, 9 secrets ready, discord ready, judge on (every pack shadow), lsp
on, startup 73.1 ms; the store moved from format 6 to 14 on start. The new tools reach the operator's daemon there;
`confirm-alerts` waits for the next confirmation email.

**Divergences.**
- The reaper is a tool, not yet the hourly tender of the design's §3.7: a tender in report mode needs a health line,
  a protocol field in `lib.rs` at its ceiling, and its 14 days of reports before it acts are a later step.
- No multipart put and no streamed get: the client reads a body whole, up to 16 MiB, so a put is one `PutObject` of at
  most 64 MiB and a large get reads by ranges; the tool says so and points to the aws CLI.
- The design's Observatory AWS panel was not built (the cockpit replaced the Observatory, Item 86); the report lists
  what the cockpit's panel should show: the account, spend, calls joined to CloudTrail by request id, the latest
  cross-check, the inventory and the reaper's report, guardrail hits, and the alerts subscription.

**Known gaps.** The cross-check reads only the account's own region (IAM's events live in us-east-1), accounts for a
tender's calls by session name (they write no `aws.called` rows), and reports an unaccounted event as a ledger row and
a log warning, not yet a security notice to the operator. A program cannot yet be granted an `aws-secret:` handle
(`[broker]` grants name `[secrets]` entries). Logs Insights' price is a constant ($0.005 a GB). theseus-ubp7 (P2),
theseus-4t3c (P3), theseus-wand (to build), theseus-ig6n (P3).

### Item 98. 37b: a task sets one-shot wakes and parks on them, and never waits on input with nothing to wake it (theseus-7kg, roadmap row 65, with theseus-celu.21; the fourth cloud batch's task-wakes session, fired 2026-10-03 19:30 from d9b0931, Opus 5.5; e837edc and ce9c585; reviewed 22:02 to 22:12 by the batch-4 harvest wake; joined 2026-10-04 00:10 at e369db7b, a signed merge onto e55e63ba, by the harvest wake 7379d0cc; installed 14:09 at bddfd407, install #1)

**Why.** Until 37b, `wake.set` refused every wake in a task, and a task's turn that would wait on input ended the task
("a task that has nothing left to wait on is done"). So "check the build now, and again in ten minutes, then report"
could not be a task. M7's design (`docs/design/m7-surface.md` §2.2) gave 37b one rule: a task may set one-shot wakes,
parks on them, and reports once when it has nothing left to wait on; a repeating wake stays refused, since a task must
end.

**What the session found.** The kernel already did most of it: a task's end and its cancel both drop its wakes in
their own frame, `wakes::free` treats waiting on input as free so the due scan fires a parked task's wake, and the
driver takes a task's continuation turns. So parking needed only the turn's end decision. It also found a gap the naive
change would open: an operator can cancel a parked task's last wake (`theseus cancel <wake>`, `/cancel`), and the task
would then wait on input forever, since no one gives a task input; it would never report, and `wake_parent` would never
fire. The same if that cancel lands while the task's turn is ending.

**What landed** (19 files, +1,071 −31 against e55e63ba; no new dependency; no store shape changed, `WakeInfo` being
wire-only, so the store's format stays 6).
- **`wake.at` in a task** refuses only a repeating wake: "Refused: this session is a task, and a task must end, so it
  cannot set a repeating wake. Set a one-shot wake instead (`after` or `at`, without `every`): you park on it, its turn
  continues this task, and you report when you have nothing left to wait on." The tool's description gains one
  sentence on what a wake does in a task.
- **Parking** (`task::parks_on_wake`, three lines in `turn.rs`). A task's turn that would wait on input takes another
  turn on unread results or the loop cap, as before; else, with a wake pending on its execution, it ends
  `Wait { Input }` and parks; else it completes and reports once on DD7's path, and `wake_parent` fires there.
- **The kernel's rule: a task never waits on input with nothing to wake it** (`wakes::task_unparked`). `cancel_wake`
  queues a task it leaves waiting with no wake, in the cancel's frame (`execution.queued`, `why: "wake_cancelled"`), and
  `end_turn` does the same when that cancel landed as the turn ended (`why: "task_unparked"`). Its next turn finds
  nothing new, and the task ends and reports with its last message. A conversation is unaffected, and so is `/stop`
  (`end_turn`'s stopped branch returns before the rule).
- **The bounds hold as for any session:** the cap of 5 wakes per execution; the task's turns spend from its carve, and
  the parent's spend counts them; a cancel of the task drops its wakes in the cancel's frame.
- **Surfaces.** `WakeInfo.task`, the task's short id, set when the wake's execution has a parent (theseus-protocol's
  `lib.rs`, +4); `theseus wakes` prints `task a1b2c3 (waiting)`; Discord's `/wakes` adds `· task a1b2c3`; the cockpit's
  Wakes panel shows the task in place of the session's title; `task.list` says a parked task waits on `a wake`, not
  `input`.
- **`theseus-sim fake-model --rules <file>`** (ce9c585, `fake_model.rs`): a scripted stand-in model whose rules match a
  turn's text in order and answer with text or tool calls. None of the simulator's stand-ins could script `task_create`
  or `wake_at`, and the live check needs both.

**How it is proven.**
- **Tests:** `tests_task_wakes.rs` (7: a task sets a wake, parks, wakes and reports once, with the wake turn's spend in
  the parent's; a cancelled task's five wakes dropped in the cancel's frame, the sixth refused by the cap; a cancelled
  task's wake never fires past its time; a repeating wake refused and a one-shot one set in the same turn; a task with
  no wake reports at once, as before; a restart while parked fires the wake once and reports once; the operator's cancel
  of a parked task's last wake ends it and it reports once); the kernel's
  `a_task_left_waiting_with_no_wake_is_queued_in_that_frame`; the daemon's
  `a_kill_while_a_task_is_parked_on_its_wake_then_a_restart_reports_once` (the real `theseusd`, the fake Discord and the
  stand-in model: SIGKILL while parked, a restart, one report, no wake left, one model request for the wake's turn); the
  stand-in's own test; the CLI's and Discord's renderer cases. The review's run: 57 passed.
- **Planted reverts, re-run at the review:** `parks_on_wake` always false fails 6 of the 7 core tests, each at "the
  task parked on its wake", the one with no wake passing as it should (the report counted 5 and named the passing one:
  its count was one short); `cancel_wake` without `task_unparked` fails the kernel test (`left: (Waiting, false) right:
  (Queued, true)`) and the core's last-wake test. The session's third, `Kernel::cancel` without `drop_wakes`, failed the
  cancelled-task test and the kernel's own `a_cancel_clears_the_wake_and_nothing_fires`.
- **Under load** (`nice -n 19` beside four busy loops, five rounds of 29 tests): every 37b test passed every round.
  The one failure in rounds 2 to 5, `a_report_that_starts_a_turn_at_the_parents_limit_asks`, failed the same way on
  `main` d9b0931 under the same load (its stand-in answer is 145,000 words, which outlasts the test's 20 s wait in a
  debug build at nice 19). `theseus-sim kernel-sim --seeds 300`: every invariant held (1,615 crashes, 4,655 wakes set,
  481 cancelled).
- **Live, at the review, the report's own script** (copies of the branch's debug binaries, the scripted stand-in model
  on loopback; nothing reached a model, Discord or the vault; 22:02:54 to 22:04:02): at 3 s `theseus tasks` showed the
  task `○ ready · wake 22:03 … 1 turn … wakes its parent`, and `theseus wakes` named it (`task 18d6c8 (waiting)`); at
  65 s the task was `complete … 2 turns … reported`, no wake was pending, and the parent held one report ("finished
  after 2 turns … Checked again: the build is green. Done."), answered by the parent's turn; the ledger held one
  `wake.set`, one `wake.fired` (`late_ms` 211) and one `task.report_wake`, exactly as the report said.

**The join** (the harvest wake 7379d0cc, 00:00 to 00:10). The signed merge e369db7b (e55e63ba and d86016e), no
conflicts; `WakeInfo.ts` regenerated in its new place, unchanged; `CLOUD_REPORT.md` removed. Its gate started at 00:07,
after the gate-flakes review's plants finished, so no build ran beside it. Gate (00:10:25): 1,893 of 1,893 (17 skipped,
1 slow); lifecycle ok on the first run (cold start p50 21.7, p95 23.2; the clean shutdown 36.2 and 41.2; kill 25.6; swap
48.5 and 51.3 ms); the jobs phase's L1 start p95 6.69 ms; frames 5 and 9; pushed 00:10. theseus-7kg and
theseus-celu.21 closed.

**The install** (install #1, 2026-10-04 14:09, at bddfd407): check, unit and restart OK; health named
`/etc/theseus/theseus.toml`, 9 secrets ready, discord ready, judge on (every pack shadow), lsp on, startup 73.1 ms.
From then a task on the operator's daemon parks on its one-shot wakes.

**Divergences.** The kernel rule reaches further than 37b's text: it also catches a task's loop-cap and late-result
path, which the core then wakes a second time, so such a turn writes two `execution.queued` rows (`task_unparked`, then
`late_result`) where it wrote one; harmless, and it closes the old crash window between those two frames. The
Observatory was being deleted (Item 86), so only the cockpit's panel names the task.

**Known gaps.** theseus-7axv (P3): a `/stop` on a task still leaves it waiting on input with no wake (pre-existing,
W1's stop branch), so only a turn submitted to the task's session continues it; and a parked task's pill reads
`○ ready · wake HH:MM`, "ready for you", though no one gives a task input ("parked · wake HH:MM" would be truer, in
theseus-protocol's `attention()`). The kernel simulator has no invariant for the new rule (a blanket one would be wrong,
since `/stop` legitimately leaves a task waiting). The core's output golden fails under a UTC clock (theseus-ig6n).

### Item 99. 30a: recall's first stage, in shadow: the `MemoryScience` trait and its baseline, the place-safe pipeline, a `recall.shadow` row of references, `memory.search` and `memory.recalls` (theseus-6fn.1, roadmap row 52, with theseus-celu.20; the fourth cloud batch's memory-recall session, fired 2026-10-03 19:30 from d9b0931, Opus 5.5; 022d088 and 56f8218; reviewed 22:06 to 22:16 by the batch-4 harvest wake, with a live check on a copy of the operator's store; joined 2026-10-04 00:19 at 8e38ff74, a signed merge onto e369db7b, by the harvest wake 7379d0cc; installed 14:09 at bddfd407, install #1)

**Why.** The index tender (Items 50 and on) answers `index.query`, but nothing asked it on a turn. M6's design
(`docs/design/m6-memory.md` §2.3 and §2.4) makes recall the first spine step of memory: the science's trait and its
baseline, a pipeline of filters that each drop a candidate with a reason, a budgeted pack, and a deadline, all in shadow
first, so that every turn on the operator's daemon records what recall would have admitted, and why it dropped the
rest, before anything reaches the model. Its one hard rule is §3.9's: in a shared place recall draws only on that
place's own sessions.

**What landed** (`theseus-memory` and `theseus-core`, with the protocol and the CLI; 39 files, +3,063 −21 against
e369db7b; Cargo.lock gains only theseus-core's edge to theseus-memory).
- **The science** (022d088). `science.rs`: `MemoryScience` with §2.3's verbs (`gate`, `schedule`, `activate`,
  `decay_sweep`, `rank`), plus `id` and `min_score`. `Baseline` answers them by §2.3's table: cosine for the gate (0.92
  to merge, 0.75 to supersede a correction), no retention, no activation, age-only sweep hints, the fused order for
  rank. `ScienceId` names the parameter set by an FNV-1a digest (`baseline@<16 hex>`), and every row carries it. The
  crate's `reserved_for` is gone, and it gains its own AGENTS.md.
- **The pipeline** (`theseus-memory`'s `recall.rs`). Each candidate goes to the first filter that takes it, in order:
  `place`, `in_context`, `untrusted`, `recursion`, `threshold`. The science ranks the rest, and a greedy pack fills the
  budget (1,500 tokens, 6 items, 400 tokens an excerpt); a drop for the budget is `budget`, and a second chunk of an
  admitted node is `in_context`. The threshold is 0 until shadow's rows calibrate it. `labeled_wrong` waits for 30b,
  whose `memory.label` produces it.
- **The place rule as built.** A turn in a shared place draws only on sessions whose current target is that place,
  a task of it included (a task's target is its parent's). A turn in a private place draws only on private places (the
  CLI, the web UI, an owner's DM, a channel bound `private = true`). A place that cannot be read is no place's. The core
  reads each candidate session's place itself (`TurnRunner::place_of`, by `class_of`'s rule), never the tender's guess.
- **The turn's hook** (`theseus-core`: `recall.rs`'s `Memory`, `turn/recall_step.rs`). The query is the turn's new
  user-message nodes (input, wakes, reports), their files' names, and the first 500 characters of the reply before
  them, read `as_of` the first new node's position, k = 40. The index is asked in a spawned task as the first loop's
  model call goes out and read right after the call returns, never past `[memory] recall_deadline_ms` (250); the
  tender is asked only while it runs, else the answer is `unavailable` with its state and why. In shadow the model gets
  the request compiled without recall. `turn.rs` gains a field, a `mod` line and five lines around `call_model` (3,408
  lines, under its 3,500 ceiling).
- **The record** (`fact/recall.rs`). The `recall.shadow` row is the `RecallManifest`, scoped `recall:<session>` so
  `memory.recalls` reads one session's rows with a scope scan, and rides in the turn's next frame (`Store::defer`). It
  keeps references only (node, session, position, rank, scores, tokens), never another session's text, and the query's
  length and the first 16 hex digits of its SHA-256, never the query. A `recall` span with the index's stages as
  children, and a narrative line under `Context` ("Recall (shadow) found 12 candidates in 34 ms … would admit 3 (1,140
  tokens) from 2 sessions; dropped 2 for the budget, 1 for its place.").
- **Methods and the CLI.** `memory.search` runs the pipeline and writes nothing, whatever the mode: with a session, as
  that session's place, its nodes in context; with none, as the CLI's private place. `memory.recalls` returns a
  session's newest rows, each admitted item's text read from its node. `theseus memory search <query> [--session S]
  [-k N]` and `theseus memory recalled <session> [-n N]`.
- **`[memory]`** (`config/memory.rs`): off by default; `mode = "shadow"` the only other mode (canary and live fail as
  unknown variants); `recall_budget_tokens`, `recall_max_items` (1 to 40), `recall_deadline_ms` (1 to 5,000),
  `include_external`. The template documents the section, commented. The bench config runs recall in shadow, so the
  lifecycle bench times every phase with it, and the turn bench counts a plain turn's frames with it.
- **The store's format stays 6.** One new ledger kind, no record kind or field; an older build reads a row of an
  unknown kind (`LedgerKind`'s rule).

**How it is proven.**
- **Tests:** theseus-memory's 45 (each filter's reason, the pack's limits, the excerpt's cut, the baseline's verbs, the
  digest, and the place property test `the_place_rule_holds_for_every_pack` over generated askers and candidates,
  checking the rule itself against an independent match); theseus-core's `tests_recall.rs` (7): a private turn admits
  a CLI session's note and drops a shared channel's for `place` and its own input for `in_context`, the raw row holding
  none of the note's words; a shared place admits only its own task's note; **the place property test over whole
  cores**, `the_place_rule_holds_over_generated_stores` (24 generated stores a run, up to 11 sessions placed among the
  CLI, an owner's DM, another person's DM, a private channel, two shared channels and a task, through a real turn:
  "dropped for place" if and only if a hand-written rule forbids it, never admitted when forbidden); a stalled index
  never holds the turn (`unavailable`, and with a stand-in that never answers and a 50 ms deadline, `deadline`); memory
  off asks nothing and writes nothing; **shadow writes no frame and changes no request byte** (the same two turns with
  memory off and in shadow, shadow admitting the note: the provider requests equal as JSON, the `request_digest`s
  equal, a plain turn at 5 frames or fewer); `memory.search` writes nothing. The review's run: 56 passed; 5 of 5 rounds
  of 56 passed under load in the cloud.
- **Planted reverts:** the place filter removed fails 6 tests, both property tests among them (re-run at the review,
  the report's count; the core's minimal case reads "Cli asked of Pier"); shadow putting its pack in the request fails
  the digest test with "shadow changed the request" (the session's; the review read the test and checked the property
  live instead).
- **The lifecycle bench with recall in shadow,** in the cloud (debug, 10 runs, idle VM): cold start p95 13.3 ms, the
  clean shutdown 7.0, kill and restart 16.1, swap 17.7; a plain turn 5 frames.
- **Live, at the review, on a copy of the operator's store** (2.4 MB, copied to a scratch dir and deleted after; the
  branch's debug `theseusd` with `theseus-index` beside it; Sonnet 5.5, about $0.05). The index came up ready (hybrid,
  166 nodes) and began its vector backfill. A note in session A ("the grey heron … nests by the old weir"), a question in
  B: `deadline` twice, since the debug index took 231 ms embedding the query while backfilling 185 vectors, at the edge
  of the 250 ms budget. With `recall_deadline_ms = 2000` on the same state: `memory search` as B's place found 40
  candidates (bm25 7, vector 40) and would admit 4, A's note among them, dropping 33 for place (the copy's channel
  sessions); a turn in a fresh session ran (index 265.6 ms) and would admit 6 (609 of 1,500 tokens) from 3 sessions, and
  the reply never mentioned the weir. **A strict A/B:** the same daemon restarted with `mode = "off"` compiled a
  `context.compiled` digest equal to the shadow turn's. A shared channel's session: would admit 0, all 40 dropped for
  place. The tender stopped for one turn: the row said `deadline`, the turn at the model's own pace. The narrative
  carried both lines.

**What the review found.** At the default 250 ms, a debug build on this machine misses often while the index embeds:
measure a release-thin index's query time before trusting 250 ms (30b's canary moves the read before the call).

**The join** (the harvest wake 7379d0cc, 00:10 to 00:19). The signed merge 8e38ff74 (e369db7b and 092d0b5);
`CLOUD_REPORT.md` removed. The resolutions (resolve.py): `config.rs` keeps the sparse config's module and re-export
beside memory's; the CLI's help keeps main's `--spawn` line and adds `memory recalled`; theseus-core's AGENTS.md keeps
the trusted guild's sentence and says recall draws in a shared place only on that place's own sessions; the seven
generated TypeScript files to `cockpit/src/protocol.gen/`. Its gate (00:19:23): 1,913 of 1,913 (17 skipped, 1 slow);
lifecycle ok on the first run, with recall in shadow in the bench config (cold start p50 31.6, p95 38.1; the clean
shutdown 41.5 and 59.5; kill 34.5; swap 57.7 and 78.6 ms, B5's containers busy beside); the jobs phase's L1 start p95
10.82 ms; a plain turn 5 frames and a tool-call turn 9, with recall in shadow; pushed 00:19. theseus-6fn.1 and
theseus-celu.20 closed.

**The install** (install #1, 2026-10-04 14:09, at bddfd407): check, unit and restart OK; health named
`/etc/theseus/theseus.toml`, 9 secrets ready, discord ready, judge on (every pack shadow), lsp on, startup 73.1 ms; and
`theseus memory search` ran on the operator's daemon on `baseline@8bc51e97`, with the vectors still loading.

**Divergences.**
- The read overlaps the model call: the design puts recall before the call, under its deadline, but in shadow nothing
  needs it first. A healthy index costs the turn nothing; a stalled one at most the deadline past the call. 30b moves it
  before the call (`recall_begin` and `recall_end` are split for that).
- `[memory]` is off by default, not shadow as the design's template says, with shadow its only other mode until 30b.
- `index_down` is `unavailable`, and the design's "audience property test" is the place property test.
- Not built here: `context.compiled`'s `recall` summary (it would change a notification's shape while recall is
  shadow-only), health's `memory` block, `[memory]`'s later keys, and the `bench recall` row.

**Known gaps.** A session displaced from its place by `/new` loses its outbox target and reads as the CLI's: a shared
place does not recall its own older sessions (a miss, not a leak). `memory.search` with no session runs as a private
place and is not refused inside a job (a job in a private place could already read the same through `index.query`).
`memory.recalls` reads a session's whole scope and keeps the newest N, one row per turn. The cockpit's recall view
(per turn: outcome, admitted items, drops by reason, budget, the index's time against the deadline; a search box) is
the report's list for a later step.

### Item 100. 21b: the ontology wired in: categories, guidance and memberships as records, a snapshot after serving, judged writes, a compile that composes twice, and store format 7 (theseus-8kk.1, roadmap row 26, with theseus-celu.25; the cloud batch 4b's ontology-wire-in session, fired 2026-10-03 19:45 from d9b0931, Opus 5.5; 6aff9ea; reviewed 22:27 to 22:36 by the batch-4 harvest wake, with a live check on a copy of the operator's store; joined 2026-10-04 00:52 at d953dd4c, a signed merge onto 1431fa7b with two join fixes, by the harvest wake 7379d0cc; installed 14:09 at bddfd407, install #1)

**Why.** §4.1a makes the ontology fungible: kinds, categories, memberships and guidance are data, and a compile walks
a session's memberships to admit each category's guidance. `theseus-ontology` (row 21a) had merged ahead of its reader
with everything the wire-in needed (`Record`, `Ontology::{load, put, check, compose, memberships, mint_id}`, and the
manifest's `MembershipUsed` and `GuidanceUsed`). 21b is M4's wire-in: records in the store, a snapshot, judged writes,
the compile walk and a CLI (`docs/design/m4-boundaries.md` §2.8). It waited for the trusted guild's join (Item 89),
since it reads places.

**What the session found.** The compiler decides append or recompile from the manifest's digests, and a turn builds
its request spec once. So the walk needs two specs: an append must render the memberships its manifest recorded, and a
recompile the current ones; otherwise a membership change would itself change the system block and recompile at once.
A bound place carries no guild id, so `guild:` categories could not be made without a change to the places and the
binding, which the brief left alone. And a job's process reaches the socket as the CLI does, so the daemon cannot tell
it apart: the CLI's job refusal (`OPERATORS`, Tier 7.8) is the speed bump.

**What landed** (6aff9ea; 45 files, +2,373 −40 against 1431fa7b; Cargo.lock gains one edge, theseus-core to
theseus-ontology, whose `reserved_for` is removed).
- **Records and rows.** Kinds, categories, guidance and membership lists are META records under `onto:`. Each write
  puts its records and its ledger row in one frame: `ontology.category`, `ontology.guidance`, `ontology.membership`
  (new `LedgerKind`s). Nothing writes kinds yet, so `ontology.kind` is not declared (the reader rule).
- **The snapshot** (`theseus-core`'s `ontology.rs`, `Board`), built after serving by one `onto:` prefix scan
  (`Core::warm_ontology`, beside the outbox's warm), or by the first reader, once, under the same lock. A write checks
  its records against the snapshot, appends them with their rows and swaps an `Arc<Ontology>`, under one mutex on the
  blocking pool. Nothing on the start path reads it.
- **Given kinds.** Each bound place's category is made at its first bind (`channel:<id>`, named as the binding names
  it, and `person:<user>` for a DM), all new or renamed ones in one frame; a later bind with nothing new writes nothing.
  A session's given membership is read from its place at compile, origin `transport`, never stored; a write of a given
  kind is refused as invalid params.
- **The compile walk.** `TurnRunner::walk` fixes the turn's `Walk` (one line in `turn.rs`): the snapshot as of the
  turn's start, the place's given membership, and the interpreted memberships, in a private place only. With no
  category in the store there is no walk, and a store renders exactly as before. `compiler::compile` composes twice,
  the append spec from the manifest's recorded `memberships` and the recompile spec from the current ones, and decides
  on the append's digests; a recompile, a ring or the image strip renders the recompile spec. The guidance goes into the
  system's second block, after the context files (`# Guidance (topic theseus)`), so the header and its cache breakpoint
  are untouched. The manifest gains `memberships` (kind, category, origin, confidence, as-of) and `guidance` (category,
  version, digest).
- **The place rule first.** The admissions (tools, context files, the class) are decided before the walk and never
  read a membership. A shared place's walk composes only `transport` memberships, guarded twice (`Walk::of` adds the
  interpreted ones only in a private place, and `Walk::compose` keeps only transport ones in a shared place, which also
  covers memberships recorded while the session was private): a shared place gets only its own place's guidance.
- **The protocol and the CLI.** `ontology.list` (with an optional `session_id`: that session's given and interpreted
  memberships), and three writes, `ontology.category.add`, `ontology.guidance.set` and `ontology.membership.set`, each
  calling `judge_act(Act::Ontology { method, what })` before it reads anything; a refusal is `REFUSED` with an
  `approval.refused` row. `theseus ontology kinds | categories | topic add NAME [--parent P] [--desc D] | guide
  CATEGORY [TEXT|-] | member SESSION [+CAT|-CAT …]`; the three writes join `OPERATORS`, so a job's shell refuses them
  before sending.
- **The store's format, 6 to 7** (the `onto:` records and the manifest's two fields). The branch bumped 5 to 6; the
  wal-synced lane had taken 6 on main (Item 92), so the join renumbered it 7.

**How it is proven.**
- **The design's 21b tests** (`tests_ontology.rs`, 7): a topic's guidance in the system block under its header, with
  the manifest's membership and guidance; a membership change waits for the next recompile (the next turn appends, and
  a manual transcript recompile brings the guidance); a guidance edit in play forces exactly one `system_changed`
  across three turns, and an edit the session does not carry changes nothing; given memberships refuse writes, and the
  bind made its 2 given categories once; **admissions are identical with and without memberships**, in the CLI, the
  owner's DM and a shared channel, the shared channel getting its own guidance and never the topic's; an ontology write
  counts only from the owner in a private place (an unnamed connection, another Discord user and the owner from a
  shared channel each refused, 3 `approval.refused` rows); the snapshot rebuilt after a restart equals the one kept in
  memory. Also the CLI's `a_jobs_process_cannot_write_the_ontology` and a renderer test. The review's run: 9 passed;
  5 of 5 runs under load in the cloud.
- **Planted reverts.** The session's three, each failing only its test: a membership reaching the admission filter
  (the shared place offered `proc_run` and the file tools), a guidance edit skipping its recompile (`["new_session"]`
  against `["new_session", "system_changed"]`), an append taking the current memberships. The review's two: a shared
  place's walk taking interpreted memberships, both guards off, fails the identical-admissions test (each guard alone
  holds it, by design); `ontology.guidance.set` ignoring `judge_act`'s refusal fails the owner-only test.
- **Benches, for their shape, in the cloud:** a plain turn 5 frames; lifecycle ok (cold start p95 17.4 ms).
- **Live, at the review, on a copy of the operator's store** (5 sessions and 21 turns, moved to the branch's format on
  the copy only; the branch's debug build; Sonnet 5.5): a topic added, its guidance set (version 1, 83 bytes), a CLI
  session made a member, and the next turn appended; after a transcript recompile, asked with the crates described,
  the model named "`theseus-core`" first, in backticks, while a control session without the topic answered "The turn
  loop is in **theseus-core** …": the guidance changed the answer. The recompile's manifest listed the membership and
  the guidance's digest, the first compilation neither; an edit to version 2 gave exactly one `system_changed`, then
  appends; the CLI refused a job's write. The copy was deleted.

**The join** (the harvest wake 7379d0cc, 00:40 to 00:52). The signed merge d953dd4c (1431fa7b, docs v0.81, and
d2a2a4b), amended twice; `CLOUD_REPORT.md` removed. **`MANIFEST_FORMAT` 7:** the store's doc keeps 6 and adds 7 for
the ontology; the pins moved (theseus-core's store test 7; theseusd's `versions.rs`: the older store at 7, 8 newer,
"this build reads formats 2 to 7"); the compilation layout sample's text reads "(5, unchanged at 6) … before the
ontology's memberships and guidance (7, theseus-8kk.1)". 12 keep-both hunks in 10 files beside memory-recall
(resolve.py), and ten TypeScript files to `cockpit/src/protocol.gen/`. **Two join fixes:** memory-recall and this
branch each added a `TurnRunner::place_of`; recall's keeps the name and returns a `Place`, and the ontology's, which
returns the raw target, is now `target_of`. And clippy's `too_many_lines`: the merged `dispatch` was 102 lines against
100, so the ontology's four methods go through one arm and `Core::rpc_ontology`. The warm passed on its third try. Its
gate (00:52:24): 1,922 of 1,922 (17 skipped, 1 slow); lifecycle ok on the first run (cold start p50 31.0, p95 33.8; the
clean shutdown 34.2 and 62.8; kill 31.2; swap 58.0 and 98.1 ms, B5's containers busy beside); the jobs phase's L1 start
p95 7.86 ms; frames 5 and 9; pushed 00:52. theseus-8kk.1 and theseus-celu.25 closed.

**The install** (install #1, 2026-10-04 14:09, at bddfd407): check, unit and restart OK; health clean (9 secrets
ready, discord ready, startup 73.1 ms); the operator's store moved from format 6 to 14 on start, this step's 7 among
the moves, one way, behind the install's backup. The bound places' `channel:` and `person:` categories are made at the
daemon's first bind.

**Divergences.**
- Two composes per loop, where the design had one walk at recompile: tens of microseconds at today's tool count, with a
  clone of the request spec when the composition used anything. A later step can skip the second when the recorded and
  current memberships name the same categories.
- The guardrail "a session in a shared place gets guidance only from what that place may see" is read strictly: a
  shared place drops every interpreted membership and keeps its own given ones. A topic's guidance in a shared channel
  would need a readers mark on guidance, as context files have.
- No `guild:` categories, and a channel's listed users are not made `person` memberships.
- The Observatory's memberships became the cockpit's (21c, Item 111).
- The design's `COMPILATION 4` is the one `MANIFEST_FORMAT` (Tier 7.9, Item 82): 7.

**Known gaps.** A task's given membership is its parent's place, but its interpreted memberships are its own session's,
so topics are not inherited. `ontology.list` is not judged and not refused inside a job, so a job can read guidance
text, as it can read other sessions' history. If a turn arrives before the after-serving warm, its walk builds the
snapshot (one prefix scan, never on the start path). Discord's `/topic` stays filed.

### Item 101. L3's report generator: `theseus-judge prove`, the exit report from finished-task records, honest about too little data (theseus-0j2.2, the judge lane's L3, with theseus-celu.31; the cloud batch 4c's judge-prove session, fired 2026-10-03 20:00 from d9b0931, Sonnet 5.5; 8ea50b1; reviewed 22:36 to 22:39 by the batch-4 harvest wake; joined 2026-10-04 00:58 at 038b889c, a signed merge onto d953dd4c, the first of three chained merges under one gate with Item 102 and Item 103, by the harvest wake 7379d0cc; installed 14:09 at bddfd407, install #1, though no installed binary carries it)

**Why.** M5's exit is a prove (P7; `docs/design/m5-judgment.md` §2.9): on finished tasks, a canary arm against a
control arm at equal total budget, judge calls included, on task success, false completion and unnecessary
continuation, with rates per task and per dollar. The judge lane's third step, L3, is that report as a generator,
and a one-command wire-in from the ledger later (row 50, L3's join). `theseus-judge` had the learning math
(`learn.rs`) but no exit report. Batch 4c sent the generator alone to the cloud, so it could be built and proven on
synthetic records before any canary data exists.

**What landed** (`crates/theseus-judge` only; 9 files, +1,378 at the merge).
- **The generator** (`src/prove.rs`, 723 lines). Its input is JSONL, one finished task a line: `task`, `arm`,
  `success` (true, false or `null`), `spend_micros` (the judge's calls included), `judge_micros`, `turns`, `nudges`,
  `unnecessary_nudges`, `false_completion`, and `stops` (each `{decision, should_stop}`). A record without `success`
  is refused; `null` means the outcome is unknown, so the task is counted and left out of every rate.
- **Intervals:** Wilson for a proportion, Newcombe's hybrid for the difference of two, the ratio estimator for a rate
  per dollar, and the normal for means and their differences.
- **Minimums:** 30 labeled tasks per arm and 30 labeled items per precision, recall, false-completion or nudge rate
  (`learn::Minimum.per_acting_class`; 200 is the promotion holdouts'), both flags. A metric under its minimum has no
  value and says how far short it is ("labeled tasks: 29 of 30").
- **The verdict** rests on the per-dollar completion difference, canary minus control: `canary_better` needs its
  interval wholly above zero; a per-task difference wholly below zero makes it `canary_worse` too, so a cheaper but
  much less successful canary is never called better; `insufficient` when either arm is short. A total spend out of
  balance (canary over control outside 0.80 to 1.25) is named in the verdict's reasons, and the per-dollar rates carry
  the comparison.
- **The binary**: `theseus-judge prove <records.jsonl> [--json P|-] [--markdown P|-] [--min-tasks N]
  [--min-labeled N]`, Markdown to stdout by default, exit 0 whenever a report was made and 1 on bad input. Its flags
  are parsed by hand, so it needs no feature: a second `[[bin]]` in the crate's manifest beside `jev-probe`.
- **Fixtures** (`fixtures/prove/`): `canary_wins.jsonl`, `small.jsonl` and the Markdown golden
  (`THESEUS_JUDGE_BLESS=1` rewrites it). No new package; Cargo.lock unchanged.
- The wire-in must supply `success`, `false_completion` and the stop labels exactly as §2.9 defines them; the
  generator never recomputes them.

**How it is proven.**
- **Tests:** 13 in `src/tests_prove.rs` and 3 in `tests/prove_cli.rs`; the crate's whole suite 105 of 105 at the
  review (102 in the library, 3 of the CLI). The exact case: canary 30 of 40 and control 20 of 40 at $0.50 each give
  completion 0.75 and 0.50, 1.5 and 1.0 per dollar, Wilson [0.598, 0.858] and [0.352, 0.648], and Newcombe's
  difference 0.25 [0.0379, 0.4333], each figure computed independently in Python. Small cohorts (29 per arm, one arm
  short, an empty input, `small.jsonl`) all say `insufficient`, with their counts and no number.
- **Planted reverts.** The session's: the arm selection swapped (`r.arm != a`) failed 10 of the 13; the per-dollar
  rate with a constant denominator failed the test that a canary costing twice as much is worse per dollar though
  equal per task. The review's re-run of the same two: 11 tests and 5 tests failed.
- **What the session found:** a test exposed a missing `success` read silently as unknown; it is now required, with a
  test.
- **The live check, at the review** (no keys needed): `small.jsonl` gave `## Verdict: insufficient` and "canary: 5
  tasks, 4 labeled, 3 successes, $2.00 spent"; `canary_wins.jsonl` gave `## Verdict: canary_better`, 1.500
  completions per USD [1.228, 1.772] against 1.000 [0.686, 1.314], the difference +0.500 [+0.085, +0.915].
- Not run under load: a pure function, with no timing or concurrency.

**The join** (the harvest wake 7379d0cc; the owner at 23:24 on 2026-10-03: joins may be batched, "whatever improves our
efficiency"). Three cloud branches joined under one gate: judge-prove at 038b889c on d953dd4c, install-smalls at
3dee91df on 038b889c, gate-speed at f1fccecf on 3dee91df. The chain log's 00:58 line says each was "its own signed
merge on d953dd4"; git shows them chained, each on the one before. No conflict here; `CLOUD_REPORT.md` removed; the
cloud commits keep their ids. The batch's gate was gate-speed's new gate's first run on main (features 0 s, test build
1 s, no bench build row): exit 0 at 00:58:41, 1,940 of 1,940 (17 skipped, 1 slow); lifecycle OK on the first run
(cold start p50 26.3; clean shutdown 59.8 and 69.3; kill 31.8; swap 73.5 and 88.1 ms, B5's Terminal-Bench containers
busy beside); the jobs phase's L1 start p95 7.80 ms; frames 5 and 9; pushed 00:58. Closed theseus-0j2.2 and
theseus-celu.31, with the batch's -0v8s, -4xyj, -a7gx, -7ykr, -dr2x, -i5xo, celu.34 and celu.33. The review's
watch-point, that jev-wire-in removes the manifest's `reserved_for` block just above the new `[[bin]]`, merged
cleanly at jev-wire-in's join (Item 105).

**The install** (install #1, 2026-10-04 14:09, at bddfd407). The crate rode in the tree, but `theseus-judge` is not
one of `scripts/build.sh`'s five shipped binaries, so no installed binary carries the generator.

**Divergences.** The design names the command `theseus judge prove`; this step built `theseus-judge prove`, a binary
of the judge crate over records in a file. The CLI's command, with the ledger-to-records mapping, is L3's join (row
50). The crate's `reserved_for` (row 37) now had a reader in its own binary; jev-wire-in removed the marker anyway.

**Known gaps.** The wire-in (`theseus judge prove` reading the ledger's finished tasks) was L3's join, theseus-0j2.18
(row 50), recorded in Item 179. `scripts/build.sh` does not install the binary. The design's L3 entry
(m5 §3) did not yet name `prove.rs`, its input shape or the binary (amended in this version).

### Item 102. Install smalls: a crash loop that stops after ten starts, a user apply that refuses to write a unit naming no token, and `check` that follows the installed unit's config (theseus-0v8s, theseus-4xyj and theseus-a7gx, with theseus-celu.34; the cloud batch 4c's install-smalls session, fired 2026-10-03 20:00 from d9b0931, Sonnet 5.5; f331a39, c2e065c and 375f747; reviewed 22:37 to 22:40 by the batch-4 harvest wake; joined 2026-10-04 00:58 at 3dee91df, a signed merge onto 038b889c, the second of three chained merges under one gate, by the harvest wake 7379d0cc; installed 14:09 at bddfd407, install #1, which rewrote the unit)

**Why.** Three small gaps in the installer (`theseusd install`, Item 18; its `--user` unit, Item 56) and the
user-service script, filed as the operator's daemon moved to a systemd user unit. A daemon that panicked at every start was restarted every 5 s,
forever, each time leaving one more file in `crashes/`: systemd's default start limit (5 in 10 s) never trips at that
pace (theseus-0v8s). `--user --apply` with no token file wrote a unit whose daemon could not reach the vault: the plan
noted it, and the apply went ahead (theseus-4xyj). And `scripts/user-service.sh check`, run in a shell with no
`THESEUS_CONFIG`, checked the plan's default config, not the one the installed unit names (theseus-a7gx).

**What landed** (installer and script only; 10 files, +169 −19 at the merge).
- **A bounded crash loop** (f331a39, theseus-0v8s). Both daemon units, `--user` and `--separate` (they share
  `SERVICE_COMMON`), restart after `RestartSec=1`, and their `[Unit]` gains `START_LIMIT`: `StartLimitIntervalSec=300`
  and `StartLimitBurst=10`. With `Restart=on-failure`, a daemon that panics at every start stops after about ten
  tries, `failed` with start-limit-hit, ten files in `crashes/`; `systemctl --user reset-failed theseusd` and a start
  bring it back. The job host's unit keeps `RestartSec=5`. The goldens `user.plan`, `separate.plan` and
  `separate-migrate.plan` changed with it.
- **A refused token-less apply** (c2e065c, theseus-4xyj). `--user --apply` with neither `--op-token-file` nor
  `THESEUS_OP_TOKEN_FILE` stops before writing anything, naming both ways out; the new `--token-from-drop-in`
  (`--user` only) lets it through for an operator whose drop-in supplies the token. The plan, `--check` and
  `--remove` are unchanged, and the plan's note mentions the refusal. The check sits in `install/mod.rs`'s apply
  branch, the one place that knows the mode. One sentence in theseusd's `AGENTS.md`.
- **`check` follows the unit** (375f747, theseus-a7gx). `resolve_from_unit` also reads the installed unit's
  `--config`, for the `check` command only and only when the shell has no `THESEUS_CONFIG`; the OK line says
  "THESEUS_CONFIG is not set here, so this is the installed unit's config". `install` still checks the config the
  unit would get, and with no unit installed the plan's path is unchanged.
- No new package; Cargo.lock and package-lock.json unchanged.

**How it is proven.**
- **Tests:** `the_daemon_units_bound_a_crash_loop`; `a_user_apply_without_a_token_file_refuses_unless_a_drop_in_supplies_it`
  (the tree untouched on a refusal; plan and check pass; the flag lets the apply through; remove needs no token);
  `check_follows_the_installed_unit_for_the_config_when_the_shell_has_none` (a variable still decides). At the review
  the `install::` tests and `user_service_script`, 55 of 55 (34 and 21). The 21 script tests skip as root, so the
  cloud VM ran them as `nobody`; the review's machine is not root, and they ran as written.
- **Planted reverts.** The session's three: `RestartSec=5` and `StartLimitBurst=5` again failed 4 (the new test and
  the three goldens); the refusal's condition inverted failed 4; the unit-following switched off failed with the
  reported symptom (`FAIL config: …/.theseus/theseus.toml is not a readable file`). The review re-ran two:
  `RestartSec=5` again (the same 4) and the drop-in test inverted (it fails).
- **The session's live checks:** `theseusd install --user` printed `StartLimitIntervalSec=300`, `StartLimitBurst=10`
  and `RestartSec=1`; `env -u THESEUS_OP_TOKEN_FILE theseusd install --user --apply` exited 1, "nothing was changed",
  and `--token-from-drop-in` wrote the unit.

**The join** (00:58, the batch under one gate; its numbers are in Item 101). The merge 3dee91df had one
content conflict, theseusd's `AGENTS.md`: both sides' sentences kept, with main's `cockpit/dist` line.
`CLOUD_REPORT.md` removed. Gate exit 0 at 00:58:41, 1,940 of 1,940. Closed theseus-0v8s, -4xyj, -a7gx and
theseus-celu.34. Not installed then: 0v8s's unit change waited for the next install, with the operator.

**The install** (install #1, 2026-10-04 14:09, at bddfd407, by the operator's install script). Its unit step ran `theseusd
--op-token-file … install --user --apply` with the new binary, so the operator's unit was rewritten with the start
limit and `RestartSec=1`; check, unit and restart OK at 14:09. `scripts/user-service.sh install` always passes a token
file, so 4xyj's refusal never touches that path.

**Divergences.** The issue named the `--user` unit; the session bounded both daemon units, since they share
`SERVICE_COMMON` and the same loop.

**Known gaps.** `docs/user-service.md` still said `RestartSec=5` and did not name `--token-from-drop-in` (amended in
this version).

### Item 103. The gate without its bench build: the test build proves the benches' binaries are its own, a `features` phase holds the shipped binaries to what the gate compiles, and the cockpit's modules installed when missing (theseus-7ykr, theseus-dr2x and theseus-i5xo, with theseus-celu.33; the cloud batch 4c's gate-speed session, fired 2026-10-03 20:00 from `lane/cockpit-parity` at c7ae697, Opus 5.5; 655cd35, d67f88e and 6cd2d4c; reviewed 2026-10-04 00:11 to 00:14 by the batch-4 harvest wake 7379d0cc; joined 00:58 at f1fccecf, a signed merge onto 3dee91df, the third of three chained merges under one gate, by the same wake; no installed binary changed: scripts and docs only)

**Why.** Three findings about the gate (`scripts/gate.sh`; Items 24, 51 and 75). Its `bench build` phase built the five
shipped binaries with their own features before the test build, and the benches then ran whatever the test build had
left in `target/debug` (theseus-7ykr). The recheck that the workspace widens no shipped crate's features was a line in
`scripts/AGENTS.md` to run by hand, so an install could build a feature set the gate had never compiled
(theseus-dr2x). And with `cockpit/node_modules` missing the cockpit phase printed a skip line and passed, so the test of
`/` took its not-built branch and passed too (theseus-i5xo). The session branched from the cockpit-parity lane
(Item 86), whose cockpit move the third fix needs.

**What the session found** (theseus-7ykr). It logged the inode and mtime of each `target/debug/<bin>` every 0.5 s
through a cold gate, and which executables ran. The bench build linked all five (03:20:33 to 03:20:51 on the VM's
clock); the test build then relinked four of them with new inodes (theseus-index, theseusd, theseus, theseus-sim);
theseus-tui kept the bench build's copy, since no integration test needs it and no bench runs it. The turn bench ran
theseus-sim, theseusd and theseus-index at the test build's inodes, and the lifecycle bench theseus-sim and theseusd:
**the benches only ever ran the test build's binaries**, and the bench build's were never run. The two builds were
different units: the test build's binaries carry the workspace's features and the dev-dependencies, as the gate's
comments already said.

**What landed** (`scripts/gate.sh`, `scripts/build.sh`, `scripts/AGENTS.md`, and one sentence of `cockpit/AGENTS.md`;
4 files, +141 −46; no Rust, no dependency, no lockfile change).
- **No bench build** (d67f88e, 7ykr). The phase and `bench_build` go. `test build` becomes `test_build`: the same
  `cargo nextest run --workspace --no-run`, with `--cargo-message-format json-render-diagnostics` saved to
  `$gate_tmp/test-build.json`. Cargo names every binary the build produced, fresh or relinked, so the phase fails
  naming a binary of `bench_bins` (theseusd, theseus-sim, theseus-index) that is missing from that list, or whose
  `target/debug/<bin>` is not that file (`-ef`, which covers a lane's `target` symlink). That keeps the guarantee that
  the benches' binaries exist, and adds one the bench build never gave: they are this build's, not an older one's.
  `bench_bins` names what the benches run, not what an install ships, so it stays a list of its own.
- **A `features` phase** (655cd35, dr2x), after `shape` and before `clippy`. `cargo tree -q -e normal,build --prefix
  none -f '{p} {f}'` for each of the five (`-p`) and for `--workspace`, cleaned and sorted: any workspace line the five
  lack, for a package the five link, is a widening, and the phase names the package and the features it adds. A
  package seen twice (a build dependency's feature set and a normal one's) is measured against every set the five have
  for it. About a second (1.26 s measured; 7 s with cargo compiling beside it). The five come from `scripts/build.sh
  --shipped`, a new option that prints build.sh's list and builds nothing, so the list lives in one place; build.sh
  builds from the same array. The bench build had checked by the way that the five compile alone with their own
  features: this phase now covers that.
- **The cockpit's modules** (6cd2d4c, i5xo). `cockpit` is now `cockpit_modules && lint && test && build &&
  cockpit_built`. `cockpit_modules` does nothing when `node_modules` exists; otherwise it runs `npm ci --offline`
  (npm's cache), then `npm ci` over the network, and if both fail it removes the half-installed tree and fails with
  the last 40 lines of npm's output (with no npm at all, it says so). `cockpit_built` fails when the build leaves no
  `crates/theseusd/cockpit/dist/index.html`. The log is `$gate_tmp/cockpit-ci.log`.

**How it is proven.** On the session's 4-core VM, cold (`cargo clean`, and `node_modules` and the cockpit's build
removed for the after run), with `THESEUS_GATE_NO_BENCH=1`:

| phase | before (c7ae697), s | after (6cd2d4c), s |
|---|---:|---:|
| fmt | 4 | 3 |
| shape | 2 | 2 |
| features | – | 1 |
| clippy | 342 | 328 |
| cockpit | 18 | 24 (with `npm ci --offline`) |
| bench build | 384 | – |
| test build | 262 | 488 |
| reader rule | 2 | 2 |
| suite | 107 | 108 |
| **total** | **1,121** | **956** |

The test build grows because it now compiles the dependencies the bench build compiled first: the two builds took 646
s before and 488 s after, **158 s saved cold (14 % of the gate)**, after 8 s for the npm install. Warm, after a `touch`
of `theseus-core/src/lib.rs`, the bench build had cost 9.0 s and the test build after it 20.9 s (the test build alone
23.1 s); with nothing changed the bench build cost 0.75 s. Each gate after the change ran fmt 3 to 4 s, shape 1 to 2,
features 1 to 2, clippy 1 to 3, cockpit 16, test build 2 to 9, suite 104 to 108.
- **7ykr:** `test_build` alone passes; with theseus-tui added to `bench_bins` it fails (a stale copy from the bench
  build, which the test build did not build). Planted: `crates/theseus-sim/tests/sim.rs` moved aside, so cargo no
  longer builds theseus-sim and the old binary stays on disk: the gate failed in `test build` after 28 s ("the test
  build did not build target/debug/theseus-sim, which the benches run, so they would run a stale one…").
- **dr2x:** a planted widening, theseus-ontology (outside the five then) asking serde_json for `preserve_order`:
  the check printed `serde_json v1.0.151: the workspace builds it with indexmap, preserve_order` and exited 1, alone
  and as the whole gate's `features` phase (after 6 s). The review re-ran it on the branch merged onto main e369db7:
  exit 0 on the tree in 0.8 s, exit 1 with the plant naming the same line, exit 0 again after the restore.
- **i5xo:** with `node_modules` and the build removed, the gate installed from npm's cache, built, and the test of `/`
  passed on its built branch; a planted `<title>Planted</title>` in `dist/index.html` fails that test; an empty cache
  with the registry unreachable failed `cockpit` after 10 s with npm's `ENOTCACHED`, leaving no `node_modules`; with
  no npm on PATH the function said so and exited 1.
- The suite at each commit: the same known failures as the baseline (the 32 L1 tests a root VM refuses, and the output
  golden's time zone, theseus-ig6n). Not re-run at the review: 7ykr's stale-binary plant and i5xo's cache and npm
  cases; the join gate ran `test_build` and `cockpit_modules` for real.

**The join** (00:58, the batch under one gate; its numbers are in Item 101). The merge f1fccecf (the
review's merge-check onto e369db7 was clean, and the merge message names no conflict); `CLOUD_REPORT.md` removed. **The batch's
gate was the new gate's first run on main:** features 0 s, test build 1 s, no bench build row; exit 0 at 00:58:41,
1,940 of 1,940. The review's watch-point held: with the ontology wired in at d953dd4c (Item 100),
theseus-ontology is linked by the five, and its features count as theirs. Closed theseus-7ykr, -dr2x, -i5xo and
theseus-celu.33. The harvest's own warm script stopped building the five with `-p` (redundant once the gate no longer
does).

**What it caught next.** The `features` phase stopped mcp-tools' first join gate at 01:39:54, 4 s in: its theseus-core
took theseus-mcp with `default-features = false` while the workspace builds the crate's server for its own tests
(Item 106).

**The install.** Scripts and docs only; nothing installed changed. Install #1 (2026-10-04 14:09, at bddfd407) built
from a tree that carries it.

**Divergences.** `bench_bins` is a second list beside build.sh's five, on purpose. The benches still run binaries
built with the dev-dependencies' features, not an install's: true before, said in the gate's comments, and the price
of not building twice.

**Known gaps.** `cargo tree -e normal,build` resolves without the dev-dependencies, as an install builds, so a feature
that only a dev-dependency adds reaches the tests alone and the check does not see it (by design; `scripts/AGENTS.md`
says so). A widening that brings in a new package shows as the widened package, not the newcomer. Only a missing
`node_modules` is installed: one present but stale after a `package-lock.json` change is not (its lint or build fails
then anyway). The theseusd test of `/` keeps its not-built branch, which a plain `cargo test` outside the gate needs.

### Item 104. Gate flakes at their causes: no secrets row after the stop's last checkpoint, a paused clock, a rendezvous, a split that panics instead of eating memory, and a reaping test that prints its evidence (theseus-81kk, -lgtj, -qh0u, -dsmp and -1m3s, with theseus-celu.32; the cloud batch 4c's gate-flakes session, fired 2026-10-03 20:00 from d9b0931, Opus 5.5, finished 22:31; b4800a5, 70e101d, cc3d2ef, 54c5d6a and 7093b64; reviewed 23:53 to 2026-10-04 00:10 by the batch-4 harvest wake 7379d0cc; joined 01:28 at dd94bc6b, a signed merge onto f1fccecf, by the harvest wake b86bd6c2; installed 14:09 at bddfd407, install #1)

**Why.** Five tests that had failed the gate under load, each sent to the cloud to be fixed at its cause, not retried
away. One of them was a product fault: a `secrets.*` row could land after the stop's last checkpoint, so the next start
replayed it (theseus-81kk). The session ran on a 4-core VM as root in UTC; "under load" is AGENTS.md's recipe (the test
at `nice -n 19` beside four busy loops at nice 0), with the test binaries run directly, since cargo's own start took
about two minutes a run at nice 19.

| flake | commit | reproduced on the base | after the fix, under load | planted revert |
|---|---|---|---|---|
| theseus-lgtj | b4800a5 | not at load (260 runs); yes with a planted slow append | 50 of 50 | caught (2) |
| theseus-qh0u | 70e101d | yes, 4 of the first 6 runs at 32 loops | 20 of 20 at 4 loops; 10 of 10 at 32 (load 33) | caught |
| theseus-dsmp | cc3d2ef | (a planted revert is the case) | 20 of 20, each property test | caught in 0.15 to 0.41 s, about 130 MB |
| theseus-81kk | 54c5d6a | not from outside (40 runs, half with a slowed `op`); yes in a unit test | the unit test 30 of 30; both versions tests 20 of 20 | caught |
| theseus-1m3s | 7093b64 | no (20 runs at the recipe) | diagnostics only | – |

**What landed** (8 files, +161 −23 at the merge; no store shape, no package).
- **81kk, the product change.** `Core::watch_secrets` appended `secrets.resolved` or `secrets.failed` whenever the
  secrets settled, and never asked whether the daemon was stopping: an `op` that answered after `finish_stop`'s
  `checkpoint_for_close` and before the runtime dropped the watcher wrote a row after the last checkpoint (a window up
  to telemetry's flush, at most 1 s). Now `Core` has a `closed: RwLock<bool>`; `finish_stop` calls
  `close_late_rows()` just before its last checkpoint, which takes the lock to write through `theseus_store::blocking`
  (so it waits for an append in progress, never for a turn) and sets it. The watcher's two appends go through
  `Core::ledger_unless_closed`, which holds the read guard across its synchronous append and, once closed, drops the
  row with a debug line. So a late row is either before the checkpoint or not written; the next start resolves the
  secrets and writes its own. A poisoned lock is read through, so a panic elsewhere cannot wedge the stop. The smaller
  `watch_secrets` lost its `#[expect(clippy::cognitive_complexity)]`. The versions tests now wait for what a start
  writes on its own time (`Rig::settled`: the history check, `driver.started`, a `secrets.*` row, all after the last
  `server.stopping`), and the SIGTERM/SIGINT test shares it.
- **lgtj.** `webui::tests::a_flush_writes_every_held_row_in_one_frame` held six refusals inside a 300 ms span of wall
  clock, and on a slow disk a later refusal found its span over and was written at once ("2 rows and 4 frames, not 1
  and 2"). It runs on tokio's paused clock now, as its two siblings do (theseus-56r7); `Refusals` already reads
  `tokio::time::Instant`.
- **qh0u.** `tests_m3::parallel::a_write_and_a_program_are_barriers` asserted that the two reads after a program
  overlapped in wall time; loaded, one ran before the other started. `Timing::rendezvous_after(skip, n)` lets the next
  `skip` runs go alone and makes the `n` after them wait for each other once started (theseus-i1i4's 4 s give-up);
  `rendezvous(n)` is `rendezvous_after(0, n)`. The overlap now holds by construction for a parallel dispatch and fails
  for a serial one. tests_m3.rs +20 (7,695 of its 8,050 ceiling).
- **dsmp.** A Discord split that stopped moving made `within` abandon its thread on the timeout, and the thread went
  on pushing empty parts while proptest shrank more cases, past 1.9 GB. Under `#[cfg(test)]` only, `split_text` counts
  its loop's turns and asserts no more than the text's length in bytes: each turn moves on by a byte or ends, so a
  working split never reaches the cap, and a broken one panics at once (the property fails "split_text did not
  return", fast). A release build has no counter. Chosen over a child process with an address-space limit: no new
  plumbing, and it catches any loop that stops moving, allocating or not.
- **1m3s.** Not reproduced; the reaping test's assertion now prints each matching log line with the three lines
  before it, then the tail, so a failure carries its evidence. The session's reading: the one failed injection was the
  fake `op`'s deliberate first failure, with an io error from the template's write or `wait_with_output` on an `op`
  already exited (not `ECHILD`, not the exempt broken pipe), which points at `OpReader::inject`'s write path, not the
  reaper.

**How it is proven.**
- The table's runs, and the planted reverts: for lgtj a planted slow disk (`thread::sleep(160 ms)` after each pair of
  refusals) fails the wall-clock base as the gate did and passes on the paused clock, and `flush` writing each held
  row in its own frame fails the fixed test; for qh0u every call dispatched alone fails it in 5.3 s; for dsmp the
  progress guarantee removed (`if cut == 0 {` made false) fails all three split tests in 0.13 to 0.21 s, at about 134
  and 132 MB; for 81kk the new `rpc::tests::secrets_that_settle_after_the_stops_last_checkpoint_write_no_row` (a gated
  vault, the watcher, `finish_stop`, then the vault opened) fails with the `closed` check made false: `a row after the
  last checkpoint: ["secrets.resolved"]`.
- **At the review:** the touched tests 18 of 18 at nice 19 (the flush test, the barrier test, the new 81kk test,
  Discord's split tests, every `versions` and `reaping` test), and three plants re-run, each failing as the session
  said: 81kk in 0.08 s; dsmp's three in 0.01 to 1.3 s, at the new guard, with no memory blow-up (the fourth split test
  never reaches a zero cut); qh0u in 5.5 s (the rendezvous' 4 s give-up, then the overlap assertion). None of the five
  was on `.config/nextest.toml`'s flaky list, so nothing came off it.
- The session's gate: the known root and time-zone failures only (theseus-pv6i's L1 refusals, theseus-ig6n); its first
  run failed in clippy on the unfulfilled `expect`, fixed before the commits.

**The join** (the harvest wake b86bd6c2, 01:21 to 01:28; alone, not in a batch, since 81kk is on the stop path and
its lifecycle rows had to be read). The signed merge dd94bc6b on f1fccecf, no conflicts, no merged file near its
long-files ceiling; `CLOUD_REPORT.md` removed; the cloud commits keep their ids. The warm (under review-step.sh) waited
while the jev-join-fix lane's benches held the gate lock. **Gate exit 0 at 01:27:58:** 1,941 of 1,941 (17 skipped, 1
slow: the 63 s repeating-wake test), no retries; lifecycle OK on the first run (cold start p50 20.8, p95 27.3; the
clean shutdown with executions waiting 35.5 and 48.8: 81kk's stop-path change costs nothing visible; SIGKILL then
restart 24.4 and 26.6; swap 51.4 and 70.7 ms); the jobs phase's L1 start p95 7.75 ms; frames 5 and 9; pushed 01:28.
Closed theseus-81kk, -lgtj, -qh0u, -dsmp and theseus-celu.32; theseus-1m3s stays open with its next step.

**The install** (install #1, 2026-10-04 14:09, at bddfd407). The operator's daemon closes late rows at every stop
from then on (the install's own restart was the old build's stop).

**Divergences.** Only the secrets watcher is gated. Other rows written on their own time after serving can race the
stop the same way (the driver's `driver.started`, the index tender's rows); the session did not see them race, and did
not make the store refuse every append after its close checkpoint, since that would drop kernel records such as a
job's completion.

**Known gaps.** The question for the owner, whether "drop late rows" should extend to `driver.started` and the index
tender's rows (facts a start writes again anyway), stayed open in the review. theseus-1m3s is open: run the reaping
test at load 30 until it fails and read the printed line (an io error from the write other than `EPIPE` means treating
any write error from an exited child as the broken pipe; a reaped or missing child means the reaper's). `Rig::settled`
anchors on the last `server.stopping`, which assumes the fixture's WAL ends with a stop. theseus-core's `AGENTS.md`
does not yet name the rule (a row written on its own time after serving goes through `Core::ledger_unless_closed`).

### Item 105. 23a: Jev wired in: `[judge]`, `JudgeService`, the judgment sink and the shadow budget, `loop.v1` in shadow after a turn ends, and a turn bench with the judge off (theseus-0j2.1, roadmap row 37, with theseus-0j2.3 and theseus-celu.19; the fourth cloud batch's jev-wire-in session, fired 2026-10-03 19:30 from d9b0931, Opus 5.5; a7fd996 and cd18eec; reviewed 22:14 to 22:24 by the batch-4 harvest wake, with a FAST A/B of frozen builds and a live check with the operator's own key; a first join 2026-10-04 00:20 to 00:39, red in the turn bench and parked; re-merged by the `jev-join-fix` lane, 00:58 to 01:41, as 1f489ad5 on f1fccecf, with fix (a) baee746c; joined 01:35:53 at 3add3f53, a signed merge onto dd94bc6b; reviewed 01:50 to 01:56 by the DM thread; installed 14:09 at bddfd407, install #1, with the judge on)

**Why.** `theseus-judge` merged ahead of its reader in the first merge batch (Item 16): the typed Jev client, the
bands, batching, the breaker, the question packs and a fake Jev, with `security.v2` and `.v3` in shadow since (Items 80
and 95), and its marker said row 37. Nothing in the core called it. 23a is M5's first spine step
(`docs/design/m5-judgment.md` §2.5, §2.6 and §3's 23a): `[judge]`, a service the core owns, every judgment recorded
in the ledger and priced inside a shadow budget, and `loop.v1` judging in shadow after a turn ends, acting on nothing.
Six of batch 5's rows launched on its join (23b, 24, 25a, 25b, 28b and 32c).

**What the session found.** The crate already had what the core needed (`JevJudge` with pricing, batching, the breaker
and strict parsing; `Recording` with a `JudgmentSink`; the `loop` builder over a plain `LoopInput`;
`JevPrice::jev_1_13_0`; the fake Jev with every mode), so the core change is wiring, and the crate changed only its
manifest (the `reserved_for` marker removed). `TurnRunner::run` already has one point after the turn's last frame is
written and its session hold dropped, where the judgment is handed off. §2.5 needs no new record kind: judgments are
LEDGER rows with a key and a scope, and the budget is one META record under a new key, so **the store's format is
unchanged** and an older binary reads the rows as kinds it does not know. The core's catalog rows are a model's
(window, cache prices, thinking), not a judge's, and the template's `[catalog]` copy was being removed (Item 90), so
Jev's price went beside the speech prices.

**What landed** (33 files, +2,479 −15 over dd94bc6b at the join, the fix and the join fix included).
- **`[judge]`** (`config/judge.rs`): `enabled` (false), `key_secret` (`jev_api_key`, an existing `[secrets]` entry, so
  no new secret source), `max_mode`, `shadow_limit_usd_per_day` (1.0), `max_in_flight`, `connect_secs`, `total_secs`,
  `api_base`, and `[judge.packs."<pack>"] mode / sample`. `mode_of` takes the lowest of the ladder's mode (shadow for
  every pack until 26a), `max_mode` and the pack's own line, and Off when disabled, so the config can lower a pack's mode
  and never raise it. The loader refuses an unknown key or pack, a sample outside 0 to 1, and an enabled judge with no
  key entry. The template has the section.
- **Jev's price:** `catalog::judge_prices()`, `jev-1.13.0` at the crate's figures, built in only. A pack whose model
  has no price is skipped as `unpriced` and never called.
- **`JudgeService`** (`judge/mod.rs`), built with the core and lazy: it reads, builds and sends nothing until the first
  judgment, which builds the client, the breaker and the sink's task. The key is read from the secret board at each
  call (`BoardKey`); an unsettled key skips the judgment as `no_key`. `TurnRunner` gains one field, and `run` one call
  after the turn's last frame: `judge.after_turn(res, is_task)`, which spawns `loop.v1` only when the baseline ended
  the turn (`stop_reason == "no_tool_calls"`) within the pack's sample (a sticky hash of the turn id), and returns at
  once. The spawned task reads the session's nodes, builds the state (scrubbed by the core's `Scrubber`, capped by the
  builder), writes the blob, reserves, calls and settles; it holds the service only around those blocking halves,
  never across the HTTP call, so a stop never waits on Jev to release the store.
- **The sink** (`judge/sink.rs`): every judgment is a `judge.call` row, answered, skipped or failed, keyed `jdg_…` and
  scoped `judge:<pack>`, carrying the whole `Judgment` (pack, version and its sha256, the state's digest and size,
  every answer with its band, usage, cost, timing, the outcome and its error class) and the core's context (session,
  execution, turn, loops, baseline, workload class, the blob's digest), with `budget: "shadow"`. Rows go in the sink's
  own frames, up to 32 rows or whatever lands within 2 s of the first, with `judge.circuit` rows when the breaker
  moves, `judge.shed` (one a minute at most), and the budget's META record. The state's blob is written before its
  row.
- **The shadow budget** (`judge/spend.rs`): per local day (the daemon's own time zone); blocks of $0.01 written to META
  `judge.budget` before the calls that draw on them; settled spend rides in the sink's frames. A judgment that would
  pass the limit is skipped and counted, never queued, with one `judge.paused` row a day. At this process's first
  judgment a reserved rest from earlier in the day is booked as spent, with one `judge.block_booked` row: that covers a
  crash and any restart, never before serving.
- **Facts and kinds** (`fact/judge.rs`): `judge.call`, `judge.paused`, `judge.block_booked`, `judge.circuit` and
  `judge.shed`. Rows only; notifications, sentences and spans are 23b's.
- **Health's `judge` block** (`JudgeHealth`): enabled, `max_mode`, each wired pack's mode, the breaker (`idle` before
  the first judgment, then `closed`, `open (Ns left)` or `half_open`), calls in flight, the day's calls, failures,
  skips and spend against the limit, and whether shadow is paused; the CLI's `judge:` line. **`theseus judge log [-n
  N] [--session S]`**, the newest `judge.call` rows through `ledger.tail` (no new protocol method), one line a
  judgment.
- **The benches.** The lifecycle bench's config runs `[judge] enabled = true` with Jev at `127.0.0.1:9`, where nothing
  listens. **Fix (a)** (baee746c, theseus-0j2.3): the turn bench's `quiet_config` turns the judge off beside Discord,
  since no background work should reach for a service nothing serves inside a measured turn.
- **The reader rule:** the crate's marker removed; Cargo.lock gains two edges (theseus-core and theseusd to
  theseus-judge) and no package. The fake Jev is a dev-dependency feature only, so no daemon build carries it.

**How it is proven.**
- **Tests:** `tests_judge.rs` (9, against the fake Jev): one `judge.call` row for a turn that ends with no tool calls,
  keyed, scoped, shadow, answered, `work_state` complete in the act band, its blob holding the ask and the final text,
  Jev given a bearer of the key's length and the key in no row, `cost_micros > 0` and the session's cost the turn's;
  a judged turn within its frame budget (at most 5); a failing Jev by class (`Down` network, `Slow` timeout, 429
  rate_limited, `Malformed` malformed), each with the turn's output, stop reason, loops, usage, cost and the whole
  provider request equal to a judge-off core's, the turn under 3 s, health counting 1 call and 1 failure; a Jev that is
  down opening the breaker (5 × network, then 2 × `circuit_open`, exactly 5 connections, one `judge.circuit` row); an
  off judge, pack or `max_mode` calling nothing; an unpriced model never called; a $0.0002 day limit pausing shadow
  with one `judge.paused` row; a restart booking the block's rest at its first judgment, not before; the loop input's
  ask and calls. Plus config, catalog, sampling and render tests, and three daemon tests (`tests/judge.rs`, the real
  `theseusd` with the stand-in Messages API and the fake Jev): a start with the judge on builds nothing (breaker
  `idle`); a turn judged after it, with the model's request body byte-identical judge on and off; a `kill -9`
  mid-block, then one `judge.block_booked` row at the next start's first judgment. At the review, 15 judge tests passed.
- **Under load** (the recipe, 5 rounds of the 16): the first rounds each failed one test on an absolute 900 ms turn
  bound ("Down: the turn took 1.215249381s"); the bound became a margin (Jev slow 10 s against a 5 s total, the turn
  under 3 s) that still catches the plant, and then 5 of 5.
- **Planted reverts.** The turn waiting on the judgment (`rt.spawn(judge_loop(..))` made a `block_on`): the failing-Jev
  test fails on its time bound ("Slow(10s): the turn took 5.027584766s"). The sink's append dropped: "0 of 1
  judgments recorded". The config raising a mode (`own.min(max_mode)`): `the_config_lowers_a_packs_mode_and_never_raises_it`
  (`left: Live, right: Shadow`) and the shadow test (`["loop.v1: live"]`). The review re-ran the first and third. Fix
  (a)'s test, `perf::tests::the_quiet_config_turns_discord_and_the_judge_off` (which also holds the lifecycle config's
  Discord and judge on), fails with the judge planted back on: `assertion failed: !cfg.discord.enabled &&
  !cfg.judge.enabled && !cfg.web.enabled`.
- **FAST first, at the review:** two frozen copies of the branch's debug build, identical but for `bench_config`'s
  `[judge] enabled`, run in one hold of the shared gate lock (22:17:43 to 22:18:45, IO pressure 0 to 7 %), palindrome
  order on, off, off, on, on, off, each `bench lifecycle --runs 10 --check`. Medians of the p50s, judge on against off:
  cold start 20.1 and 20.8 ms, from the copy 20.4 and 21.0, clean shutdown 30.4 and 30.8, kill 24.1 and 24.1, swap 45.0
  and 46.3. **No phase moves with the judge on**, and all three judge-on runs were OK; the one miss was a judge-off
  run's single outlier (the copy's p95 75.5 ms).
- **Live, at the review, with the operator's own key** (a scratch daemon of the branch's debug build, a fresh state
  dir each run, Discord, web and index off). Health before: `judge: loop.v1: shadow · max live · breaker idle · 0 calls
  today … $0.000000 of $1.00 today`. One turn ("Say the word done, and nothing else.") answered `done`; `theseus judge
  log` showed `loop.v1 (shadow)` with `work_state=complete 1.00 act`, `stopping_point_defined=yes 0.93 act`,
  `cost_out_of_proportion=no 0.32 confirm` among its answers, and $0.000056; health after: `breaker closed · 1 call
  today (0 failed, 0 skipped) · $0.000056 of $1.00 today`; the row answered, `cost_micros` 56. The key, read into a
  checking process only, was in neither the `judge.call` rows nor any of the four files under the state dir. **The
  request digest was the same with the judge on and off** (fresh state at the same path, the same prompt; the off run
  read the whole cached prefix, 5,648 tokens, and its health said `judge: off ([judge] enabled = false)`).

**The joins.**
- **The first** (the batch-4 harvest wake 7379d0cc). At 00:19:52 it stood down before committing: the DM thread had
  taken docs v0.81's review lock 29 s earlier, so v0.81 joined first (1431fa7b, 00:28:41). The second attempt made the
  signed merge 74945a6 on 1431fa7b: 13 hunks in 9 files kept both sides beside memory-recall (resolve.py), and one join
  fix: `theseus judge log`'s `LedgerTailParams` takes `..Default::default()`, since ledger-perf (Item 91) had added
  `before`, `since_ms` and `until_ms` (E0063 at the first warm). Suite 1,932 of 1,932, lifecycle OK; then **red in the
  turn bench, twice** (00:37:58): "a tool-call turn's trace counts Some(9) frames, and the WAL holds 10". The tenth was
  the sink's own frame, `[ledger:judge.call ×15, ledger:judge.circuit, ledger:judge.shed, meta]`: the bench config ran
  the judge against a port nothing answers, each plain turn's end spawned a failing judgment, the breaker opened, later
  ones were shed, and the batched frame landed inside a measured tool-call turn. The cloud had run only `bench turn
  --runs 5 --burst 0`, plain turns, which never saw it. Main reset to 1431fa7b; the merge parked on a local branch;
  filed theseus-0j2.3 (P1).
- **The decision** (the DM thread, 00:58; overnight decision 1 for the owner's review): option (a), the turn bench's quiet
  config turns the judge off as it does Discord, and the lifecycle bench keeps it on. With Jev unreachable the bench
  never measured a judged turn either way, only a turn whose background judge was failing; (a) keeps the turn bench
  strict about the turn's own frames. The harvest had leaned (b), leaving the judge's frames out of the count, "because
  it keeps measuring a judged turn". To reverse: one config line and a frame filter, then a re-gate.
- **The lane** `jev-join-fix` (00:58 to 01:41). It re-merged `cloud/20261004-jev-wire-in` (6efad2b) onto f1fccecf:
  rerere replayed the parked merge's 9 hunks in 6 files, and 7 new hunks in 5 files beside the ontology kept both
  sides (resolve2.py asserts each: theseus-core's `Cargo.toml`, `TurnRunner`'s fields, `Core::build`, the fact modules
  and registry, the CLI's renderers); `JudgeHealth.ts` landed in `cockpit/src/protocol.gen/`, regenerated unchanged;
  the join fix again. `compare.py` showed this merge's lines over f1fccecf equal to the parked merge's over 1431fa7b,
  file by file. Seams checked: the ontology's `onto:` scan against the judge's `judge.budget` key; judge-prove's
  binary and manifest auto-merged beside the marker's removal (Item 101); gate-speed's `features` phase
  passes, since the fake feature comes only through dev-dependencies (Item 103). Warm exit 0 at 01:12:20 on
  a cold target; 1f489ad5 signed. The lane's gate (01:15 to 01:19): suite 1,960 of 1,960, lifecycle missed twice on
  single outliers with no settle wait (cold start p95 274.4 ms; then 588.9, and a clean shutdown p95 of 1,104.3);
  the benches re-run by finish-benches.sh's rule (01:20 to 01:23, with the busy allowance, CPU 22 % from
  Terminal-Bench's VMs) were OK on the first run, every phase inside its strict budget, a plain turn 5 frames and a
  tool-call turn 9, the burst 180.
- **The join** (lock at 01:28:34, after gate-flakes' done line). `git merge --no-ff -S` of the lane onto dd94bc6b made
  3add3f53, no conflicts. **Gate exit 0 at 01:35:41** (408 s): 1,961 of 1,961 (17 skipped, 1 slow); lifecycle, after
  a 55 s settle at IO 0 %, OK on the first run, strictly (cold start p50 20.8, p95 24.3; from the copy 22.0 and 25.1;
  clean shutdown 33.9 and 53.3; a post in flight 73.6 and 79.9, unbudgeted; SIGKILL then restart 27.2 and 31.2; swap
  49.0 and 53.3; restore 120.5 and 132.6, unbudgeted); the jobs phase's L1 start p95 6.52 ms; turn frames 5 and 9, the
  burst 180; deny ok. Pushed 01:35:53. Closed theseus-0j2.3, theseus-0j2.1 and theseus-celu.19; the follow-up is
  theseus-0j2.8. The store's format stayed 7.
- **An incident, undone** (overnight decision 7). The lane's cleanup script, land.sh, began with `git push origin
  main`, meant as a no-op; but the done line had released main, and the harvest had merged mcp-tools on top (fbc1d2a0,
  01:36:10, still warming). At 01:36:54 that ungated merge reached origin; the lane restored origin/main to 3add3f53 at
  01:38:11 with a lease push, 77 s later. It reached nothing: fbc1d2a0 is not in main (the harvest re-did that merge
  and gated it as e457555d, Item 106), no cloud session fired in the window (batch 5 fired at 01:28, 01:45
  and 01:50), and no lane fetched. land.sh now pushes only its own lane's join, skips when origin has it, and refuses
  when another join sits on top; the review tightened its merge case to need HEAD's first parent to be origin/main, and
  its scratch-repo test passes all three cases.

**The install** (install #1, 2026-10-04 14:09, at bddfd407). Health: "judge on (every pack shadow, as configured)":
the operator's config turns `[judge]` on (the m5 design's one line), and `jev_api_key` resolves among the 9 secrets.
From then on every turn the baseline ends is judged by `loop.v1` in shadow, within $1.00 a day.

**Divergences.**
- Jev's price is `catalog::judge_prices()`, built in only; the design had a `[catalog."jev-1.13.0"]` row in the
  built-in table and the template, whose catalog copy was removed (Item 90).
- The turn bench runs with the judge off (0j2.3's option (a)); the judge's own frame per judged batch is unmeasured
  there (theseus-0j2.8).
- `loop.v1` judges every turn the baseline ends with `no_tool_calls`, a task's included, and skips one that ends on
  the loop cap, a refusal, `max_tokens` or a confirm. It does not sample `continue` decisions (§2.4), which would need
  a judgment mid-turn; 23a kept the turn's path untouched.
- The ask is the newest operator message in the transcript (for a task, its brief), not "the exchange's first human
  message"; they differ when several operator messages come before a reply.
- A clean stop drops the sink's last 2 s window of rows, as a crash does (§2.5 said a crash); their spend is booked at
  the next start.
- The design's 23a test "the frame-budget test still counts 8" is, as built, a judged turn within its 5 frames.

**Known gaps.** Not built in 23a: `judge.resumed` (the day's roll resets the pause), the trace's `mark` span,
notifications and narrative lines, and `judge.list/get` (23b, Item 119); the frame-budget test's
judged variant for live calls (26b). theseus-0j2.8 (P2) is open: measure the sink's frame per judged turn with a
judge-on turn bench and a fake Jev, and coalesce it into the next frame if the cost matters; perf.rs's module doc and
the turn bench's header still say only "Discord and the web UI off". theseus-core's `AGENTS.md` gained its judge
bullet with batch 5's judging rows (Item 119, Item 126), not here; theseus-judge
has no `AGENTS.md`.

### Item 106. 36b: MCP servers' tools in turns: `[mcp.servers]`, a board started after serving, the stored list, results outside text, and private places only (theseus-ext.1, roadmap row 66, with theseus-celu.26; the cloud batch 4b's mcp-tools session, fired 2026-10-03 19:45 from d9b0931, Opus 5.5; 29c91aa, 06c14f2, 5967b0f and 6aa253f; reviewed 22:31 to 22:40 by the batch-4 harvest wake, with a live check on a scratch daemon and the real model; joined 2026-10-04 01:46 at e457555d, a signed merge onto 3add3f53 with one join fix, by the harvest wake b86bd6c2; installed 14:09 at bddfd407, install #1)

**Why.** M7's first spine step (`docs/design/m7-surface.md` §2.1). `theseus-mcp`, the protocol alone (a client over
stdio and streamable HTTP, a fake server, the server side), had merged ahead of its readers in the first merge batch
(Item 16), and the gate already resolved `mcp:<server>/<tool>` under `[policy.mcp]`. Nothing started a server or
offered its tools to a turn. 36b is the wire-in: the operator's servers, their tools in turns through the gate, and
every surface. It waited in the cloud's batch 4b for t7-kernel's tool path and the trusted guild's places (Items 88
and 89).

**What landed** (45 files, +3,380 −41 over 3add3f53 at the join; theseus-core's `mcp/` and `config/mcp.rs`, small
edits in shared files, the protocol and CLI, `theseus-sim fake-mcp`).
- **The trait change** (29c91aa). `Tool::name()`, `description()` and `family()` return `&str` borrowed from the tool,
  since a server's tool is named at runtime. An impl returning `&'static str` still satisfies the new signature, so no
  built-in tool changed and other branches' tools merge as they are; only `tests_m3`'s `Slowed`, which forwards
  another tool's description, changed. New `Tool::wire_name()`, by default the dotted name with underscores, so an MCP
  tool can call itself `mcp__<server>__<tool>`; `Registry::by_wire` and `definitions_of` use it.
- **`[mcp.servers.<name>]`** (06c14f2, `config/mcp.rs`): stdio (`command` as argv, no shell; `env` mapping variables
  to `[secrets]` names) or HTTP (`url`, `auth_secret` sent as a bearer); both take `read`, `sandbox`, `external`
  (default true), `enabled` (default true), `start_timeout_secs` (30) and `call_timeout_secs` (110, at most 115). Every
  table denies unknown keys; the loader wants exactly one transport, every secret a `[secrets]` entry, `env` only on
  stdio and `auth_secret` only on HTTP, and a name of letters, digits, `_` and `-`, at most 32 characters, with no `__`
  (its tools' wire names carry it). **`sandbox = "l1"` is refused for now**, naming the follow-up: a long-lived
  server under L1 needs the L1 spawn path to keep a child alive with live pipes, which was built for a job that runs to
  an end. Servers run at L0. The template gained a commented `mcp` section and a commented `docs_mcp_token` secret.
- **`McpBoard`** (5967b0f, `mcp/mod.rs`). It starts every enabled server **after serving** (`core.mcp.start()` in
  theseusd's `after_serving`), nothing before: stdio through `children::spawn(Kind::Owned, …)` in its own process
  group, its environment cleared, then the job's (`proc_env`), then its `env` secrets from the secret board, its cwd
  the first workspace root, its stderr to `<state>/mcp/<name>.log` capped at 1 MiB; HTTP through theseus-mcp's
  `HttpTarget`. The connection is behind a `Connect` trait (the daemon's `Spawn`; an in-process fake in tests).
  States: `stopped` (before serving), `starting`, `ready`, `restarting`, `failed`, `disabled`. A crash, or a start
  that fails or exceeds its timeout, restarts after 1 s, then 5 s, then 30 s; the third crash within 10 minutes leaves
  it `failed` with its reason until `mcp.restart`, which also forgets the count. Every clean stop sends SIGTERM to each
  server's group and never waits; a stdio server whose stdin closes ends by itself, so the daemon's `kill -9` leaves
  none running (no `PR_SET_PDEATHSIG`: it follows the spawning thread, and tokio's blocking threads come and go).
- **A changed list.** `list_changed` (or an HTTP session made again) lists again and rebuilds the catalog. A turn's
  request spec is fixed at its start, so the new list applies from the next turn. A changed name, description or
  schema writes `mcp.tools_changed` (added, removed, changed) with a narrative line, and posts an `mcp_changed`
  operator notice (the Discord courier's arm sends it to the owner's DM).
- **The catalog and `McpTool`** (`mcp/tool.rs`). Canonical name `mcp:<server>/<tool>` (what the gate and
  `[policy.mcp]` use), wire name `mcp__<server>__<tool>` from theseus-mcp's `names::wire_names` (unique across
  servers), family `mcp`, backend `Async`, sorted by canonical name. Class `Run` and `NonRepeatable` unless the
  server's `read` lists the tool (`Read`, `SafeToRepeat`); a server's own hints show in `mcp.list` and never loosen.
  Each tool is granted its server's secrets (`Broker::grant_tool`), so a call runs at no looser a posture than its
  secrets'. Descriptions capped at 2,000 characters, saying so; a missing `"type": "object"` added; a non-object input
  refused at plan; the summary `mcp:<server>/<tool> <args, cut to 120 characters>`.
- **Results.** Text as text, resource links and embedded resources with their URIs, `structuredContent` as JSON text.
  **Outside text** unless `external = false`, through the async path's `External`, so the hold is written in the
  result's own frame. `isError` is a failure the model reads, still outside text. A timeout or a closed connection is
  `unknown` (`action.outcome_unknown`). toolrun's async branch now keeps a failure's `meta` (a new `failure()` reads
  `outcome_unknown` and `external`), the one change to the existing call path. A cancel or `/stop` aborts the task, and
  theseus-mcp sends `notifications/cancelled`.
- **The stored list.** Each server's last good list is a META record, `mcp.tools.<server>` (the tools and a digest of
  names, descriptions and schemas), written off the runtime's workers only when it differs. `Core::build` reads one
  key per configured server, so **a start offers the tools at once**; a call to a server not up yet waits for that
  server alone, up to `start_timeout_secs`, then fails `mcp_unavailable: … Nothing was sent`.
- **Places.** `definitions_for(class)` appends MCP tools after the built-ins through `places::offered`, so a shared
  place is offered none, and the gate refuses one there anyway (`places::refusal`). The tools note names the attached
  servers and says their results are outside text.
- **Surfaces.** Health's `mcp[]` (transport, state, pid, tools, prompts, the stored list in use, calls, errors,
  crashes, last error, start time, protocol, digest, and `read` entries the server does not list) and an `mcp:` line;
  `mcp.list` and `mcp.restart`; `tool.list` lists MCP tools; `theseus mcp` and `theseus mcp restart <name>`; five
  ledger kinds (`mcp.started`, `.ready`, `.exited`, `.failed`, `.tools_changed`) with narrative lines; a call's span
  carries `mcp.call`, `mcp.server` and `mcp.tool`, and its counts are `theseus.tool.*` with family `mcp`.
  `theseus-sim fake-mcp` serves theseus-mcp's fake (stdio by default, `--http`; modes `ok`, `slow`, `crash-after=N`,
  `change-tools`, `error`).
- **The reader rule:** theseus-mcp is reached now, so its marker is gone; its `Cargo.toml` says the `server` feature
  waits for row 72 (41b). A new `crates/theseus-mcp/AGENTS.md`, an "MCP servers" bullet in theseus-core's, a map line
  in the root's.
- **No store format change:** the stored list is a value under the META kind and the new rows are new kind names
  (the session said: bump it here if the version rule is read as covering META keys; the review agreed it does not).

**How it is proven.**
- **Core tests** (`mcp/tests.rs`, 9, plus 2 of the config; an in-process fake over pipes and a stand-in model): a call
  answered with its notice and the hold (the result `ok` and outside text, one `tool.notified` row for
  `mcp:fake/echo`, the session's T1 hold naming it, `mcp.started` and `mcp.ready` rows, health and `mcp.list` showing
  class `run`, posture `notify`, 1 call, hint `read-only`, the stored list written); a shared place's turn offered no
  `mcp__` tool, and the model's call anyway answered `Not run: mcp:fake/echo is not offered in a shared place`, with 0
  calls at the server; a changed list applied at the next turn with its row and notice; a stored list offered at once
  and a call that waits for its own server alone; a crash restarting at 0 s, 1 s and 1 s + 5 s on the paused clock,
  then `failed` until `restart`; hints that never loosen; an error result still outside text; a server that never
  comes up answered `mcp_unavailable`, "Nothing was sent"; the description cap. **Daemon tests** (`tests/mcp.rs`, 2,
  the real `theseusd` and `fake-mcp`): `mcp.started`'s WAL position after `server.serving`'s, and the server's pid gone
  within 10 s of the daemon's `kill -9`; a start whose server never answers offering the stored list (`stored: true`, 5
  tools). At the review, 12 and 2 passed.
- **Planted reverts:** results not outside text failed the hold test (at the review, 3 tests); MCP tools offered in a
  shared place (the `offered` filter made `|_| true`) failed the shared-place test.
- **Under load** (the recipe; 12 tests, 5 runs): 12 of 12 each time. **Cold start with three fake servers**
  (`bench lifecycle --phases cold --runs 20`, the VM's debug build): p50/p95 12.0/16.0 and 12.2/14.5 ms with none,
  12.8/17.4 and 11.9/12.9 with three: unchanged within the noise, the daemon reading one META key a server before
  serving and spawning nothing. A plain turn 5 frames.
- **Live, at the review** (the branch's debug binaries on a scratch daemon, a fresh state dir, Sonnet 5.5;
  `[mcp.servers.fake]` with `read = ["add"]`, and MCP's reference "everything" server from a pre-installed copy, so no
  network fetch): health `mcp: everything ready (13 tools) · fake ready (5 tools)`; `theseus mcp` listed both
  (stdio, pid, protocol 2025-11-25) and every tool, `mcp__fake__echo` staying `run` though its server says read-only,
  `mcp__fake__add` `read`; the model called `mcp:fake/echo` with "live check" and quoted it back; **the ledger had
  `server.serving` at position 7, then `mcp.started` and `mcp.ready` for fake (9, 11) and everything (12, 14)**: no
  spawn before serving; `session.external_read` named `mcp:fake/echo`, and the next ask to write a file was held
  ("this session read external text (mcp:fake/echo …), and a call that acts waits for approval after that (§3.9)");
  after a clean stop and start, `mcp__fake__add` with 2 and 3 answered 5, and `theseus mcp restart fake` restarted
  it; with the fake's command made `sleep 30`, health read `starting, stored: true, 5 tools` and all five were
  offered; a `kill -9` of the daemon (started outside systemd) left neither server running at the first check.

**The join** (the harvest wake b86bd6c2, 01:30 to 01:46; locks taken at 01:36, after jev-join-fix's done line).
Prepared in a scratch worktree on 3add3f53: 19 conflicts, 8 replayed by rerere from a first preparation on dd94bc6b.
Cargo.lock and theseus-core's `Cargo.toml` keep every edge (theseus-mcp beside theseus-judge, -memory and -ontology,
no package added); toolrun.rs keeps Tier 7.1's `job_waits` beside `mcp`, with `tool()` and `tool_by_wire()` after
the constructor; the facts, `Core::build`, the protocol's lib, ledger and ts lists, the CLI and theseus-sim keep both
sides; theseus-core's `AGENTS.md` keeps main's sparse-note sentence and adds the MCP paragraph; the template keeps
both secrets examples and the judge's section beside the mcp one; five TypeScript files to `cockpit/src/protocol.gen/`.
**Gate run 1 red in `features`** (01:39:54, 4 s in), the phase gate-speed added after this branch was written
(Item 103): theseus-core took theseus-mcp with `default-features = false` while the workspace builds the
crate's `server` feature for its own tests, so the shipped binaries would have built a feature set the gate never
tests. **The join fix** (overnight decision 6, the harvest's call at 01:41): theseus-core takes theseus-mcp with its
default features, the gate's own advice; theseus-mcp's `Cargo.toml` and `AGENTS.md` say so; Cargo.lock unchanged.
The alternative, a non-default `server` feature, would have skipped the crate's server tests until 41b joined; to
reverse it is one line once nothing shipped needs the server code. The merge amended as e457555d. **Gate run 2 exit 0
at 01:46:21:** 1,975 of 1,975 (17 skipped, 1 slow); lifecycle OK on the first run (cold start p50 24.9, p95 42.6; clean
shutdown 34.1 and 48.7; kill 26.0 and 30.5; swap 49.4 and 55.7 ms); the jobs phase's L1 start p95 6.96 ms; frames 5 and
9; pushed 01:46. Closed theseus-ext.1 and theseus-celu.26. Batch 5's mcp-prompts (36c) and extend-propose (43a)
launched on its done line. A first preparation of this merge, fbc1d2a0, reached origin for 77 s before its gate,
through another lane's cleanup script; it was never main (Item 105).

**The install** (install #1, 2026-10-04 14:09, at bddfd407). The board is in the operator's daemon; it starts only the
servers `[mcp.servers]` names, and install #1's sources record none.

**Divergences.**
- The board starts servers after serving, not "once the vault confirms the config"; servers run at L0 until the L1
  follow-up (built with 43a, Item 118).
- An MCP image is a line naming it, not an image block: `AsyncResult` carries no image.
- No `secret.granted` row at a server's spawn (that fact borrows a turn's context); each call is still held to its
  secrets' posture through the broker's tool grant.
- No `theseus.mcp.calls` or `.call.duration` (they would repeat `theseus.tool.*` with family `mcp`), and no
  `theseus.mcp.servers.up` (the metrics module has no gauge kind).
- The Observatory's MCP section became a list of what the cockpit's should show (each server's state, transport,
  pid, tools and prompts, calls and errors, crashes and last error, protocol, the stored and live digests, a restart
  button; each tool's names, class, posture, hints and full description); not built here.

**Known gaps.** Prompts are only counted (36c's, Item 115). The MCP image follow-up (`AsyncResult` with an
optional image) and the spawn's `secret.granted` stay open as the report names them.

### Item 107. 40, part 1: hands on Lambda and Fargate: `theseusd hand` and its signed envelope, `aws.hands.run`, and the completion poller (theseus-mgw.6, roadmap row 35, with theseus-celu.28; the cloud batch 4c's aws-hands session, fired 2026-10-03 20:00 from d9b0931, Opus 5.5; 5161d0e, 94c99f0 and e0060bc; reviewed offline 2026-10-04 01:33 to 01:47 by the harvest wake b86bd6c2, its live check left for theseus-mgw.9; joined 02:01 at ab54f037, a signed merge onto e457555d with one join fix (a test), by the same wake; installed 14:09 at bddfd407, install #1)

**Why.** The AWS design's hands (`docs/design/aws-toolset.md` §3.3; step 40): a hand is a job that runs in AWS and
keeps the job wrapper's contract (§3.16): detached, durable and cancellable. Many hands make a group; Lambda takes
short, stateless work and Fargate long or heavy work; every completion comes home through the foundation's queue and
settles as the kernel settles any completion. C2 (Item 78) made the stacks and the owner-role sessions it needs. Part 1
is the role, the image, the tool and the poller; part 2 (cancellation per backend, the TTL reaper's act mode, budget
reservations, quotas) followed as hands-cancel (Item 116).

**What landed** (26 files, +4,904 −3 over e457555d at the join).
- **The role, `theseusd hand`** (5161d0e, `aws/hands/hand.rs`). Helper roles live in `theseusd` (`job-wrapper`,
  `job-sandbox`), not the CLI, which links only `theseus-protocol` while a hand needs the AWS client; so the role is
  `theseusd hand`, hidden, where the design said `theseus hand`. `main()` serves Lambda's custom runtime when
  `AWS_LAMBDA_RUNTIME_API` is set, and otherwise runs the one spec in `THESEUS_HAND` (Fargate). It runs the argv in
  `/tmp/hand-<corr>` in its own process group under `deadline_secs` (SIGTERM to the group, 5 s, SIGKILL); streams lines
  to CloudWatch Logs `hands/<group>/<index>` every second; uploads every file under `out/` (at most 64 MiB each and
  1,000 files), `output.txt` (the last 256 KiB) and `result.json` to `s3://<bucket>/hands/<corr>/`; signs the envelope
  and sends it to the queue (3 tries); on Lambda answers the runtime API, or reports an error so the async failure
  destination fires. **The job never sees its spec** (`env_remove(THESEUS_HAND)`): it gets
  `THESEUS_HAND_INDEX/_INPUT/_GROUP/_OUT`, and `{index}` and `{input}` replaced in its argv. Its credentials are
  Lambda's environment or ECS's container endpoint (fetched again after 10 minutes), never the daemon's.
- **The envelope** (`envelope.rs`). A per-dispatch key, `HKDF-SHA256(salt "theseus-hands/1", ikm = the account's
  secret access key, info = the correlation id)`: no new secret, nothing stored, and the daemon derives it again to
  check an envelope. The envelope is §3.16's completion with `signature = hmac-sha256:<hex>` over length-prefixed
  fields (the tail by its digest), verified in constant time (`verify_slice`). `HandSpec`'s `Debug` withholds the key.
  `hmac` and `hkdf` were already locked: two edges, no package.
- **The image** (94c99f0, `infra/aws/hand/`). A Dockerfile on `debian:bookworm-slim` pinned by digest, with
  ca-certificates, curl, git, jq, less, openssh-client, python3 and its venv, unzip and zip, the AWS CLI v2 2.27.50
  pinned by its zip's sha256, user 1000, `HOME=/tmp`, `ENTRYPOINT ["/usr/local/bin/theseusd"]` and `CMD ["hand"]`.
  `build.sh` refuses a dirty tree, builds a static musl `theseusd` (`scripts/build.sh --profile release-thin`), checks
  it is static, builds `theseus/hand:<commit>`, checks the image answers as a hand, and with `--push <region>` pushes
  to ECR and prints `HandImageUri=…@sha256:…`.
- **`aws.hands.run`** (e0060bc; `launch.rs`, `group.rs`, `poller.rs`, `tool.rs`, `toolrun/hands.rs`), run-class.
  - **No new record kind.** A group is three things: the call's own action, a META record `aws.hands.group.<group>`
    (the pattern of `wake.target.*`), and one kernel action per hand (tool `aws.hand`, its `resource` the group's key).
    So settling is `Kernel::accept_completion`, and dedupe, the quarantine of an unknown id, late-after-cancel and the
    reconciler's `overdue_no_evidence` at a hand's deadline (TTL + 5 min) come with it. The store's format is
    unchanged.
  - **The call answers `background`**, through `toolrun/hands.rs` (one `if` in `execute`), not `run_inproc`; the
    group's aggregate is its late result (one `if` in `job_result`). The call's frame holds the group record with the
    hands' records and the first wave's dispatch; each later wave is dispatched in its own frame before it launches.
  - **Where hands run is discovered, not configured:** `DescribeStacks` of `theseus-foundation`, `theseus-hands` and
    `theseus-hands-network`, read once per account and region and kept in the group record, so a restarted poller
    needs no network to know its queue. The template gains only the commented `[policy.tools] "aws.hands.run"` line.
  - **The backend** until Jev chooses: Lambda when there is no `image`, the profile is `basic`, `memory_mb` ≤ 10,240
    and `ttl_secs` ≤ 600; otherwise Fargate. A named `lambda` that cannot run there is invalid input; the Lambda
    function's memory is the stack's `LambdaHandMemoryMb`, whatever the call asks. Batch is not built.
  - **The NAT stays off.** A Fargate call while the network stack's `NatGateway` output is `disabled` fails with
    "Fargate hands need the hands VPC's NAT, which is off (about $36 a month while on): … NatGateway=enabled, or the
    call runs on Lambda", before any ECS call. Nothing in the code turns it on.
  - **Budget:** a launch cap only. `max_usd` limits how many hands launch, each at its worst case (TTL × the rate for
    its size; the rates are constants in `launch.rs`); each hand's real cost is its completion's `cost_micros`, summed
    into the group. `until`: all, `first_success` or a quorum; running hands are not cancelled when it is met (part
    2), and never-launched hands are `cancel_verified` (nothing ran).
  - **The poller** starts after serving (`Core::poll_hands_after_serving`, beside the AWS check) and polls only while a
    group's call or any hand is dispatched, otherwise waiting on a `Notify` a launch rings: 20 s `ReceiveMessage`
    calls, envelopes first in each batch, each message deleted once its frame is written. Messages it does not read
    are left for their reader and, in time, the DLQ. Its own receives and deletes are not ledgered (about three rows a
    minute otherwise); launches and discovery are. A Lambda failure-destination record counts only if its
    `requestPayload.key` is the hand's derived key; an ECS STOPPED event whose `hand` container did not exit 0 marks
    the hand `outcome_unknown` until the envelope resolves it.
  - **Quarantine:** a `quarantine:<corr>` completion record (the kernel's own prefix, so health's quarantined count
    sees it) and a `completion.quarantined` row with `why`; nothing settles. A bad signature, a failure record without
    its hand's key, or an envelope naming an action that is not a hand of its group is quarantined.
- Two ledger kinds (`aws.hands.launched`, `aws.hands.settled`); theseus-core's and theseusd's `AGENTS.md` describe the
  hands and the role.

**How it is proven.**
- **Tests:** `envelope::tests` (4) and `tests_hand` (3) against the local fake (Logs, S3, SQS): a job with input and
  no key in its environment; log lines, four S3 keys in order, an envelope that verifies with its key and not another's,
  `call/<corr>` in every user agent; exit 3 failed; a 1 s deadline on `sleep 300 & sleep 300` failed and timed out in
  about 1 s; a missing program failed with a note; a refusing queue "the envelope was not sent". `tests_hands` (9,
  through the whole core, against a stateful fake of STS, CloudFormation, Lambda, ECS, Logs, S3 and SQS with a real
  queue): a Lambda group launched, then settled from its hands' envelopes, **the real hand role** running each spec
  against the fake ("2 of 2 succeeded", each hand's tail and prefix, no key in any row); a group's record and its hands
  in one frame, and a failed frame launching nothing; a bad signature quarantined, then the real one settling; a
  duplicate settling once and a late one settling only its hand; `first_success` at concurrency 1 (#0 fails, #1
  succeeds, exactly 2 Invokes, #2 and #3 never dispatched, "1 of 4 succeeded, 1 failed, 2 not launched"); the poller
  idle with nothing outstanding (0 receives for 800 ms); a restart with completions waiting, one duplicated, settling
  each once; a forged Lambda failure record quarantined; Fargate refused with the NAT off and, with it on, one task
  definition (the init container, `dependsOn SUCCESS`), two RunTasks with `assignPublicIp DISABLED`, the private
  subnets and all 7 tags, and a STOPPED exit 137 resolved by the envelope. At the review, with `aws::tests`, 31 of 31.
- **Planted reverts, all five failing at the review:** the signature unchecked (the forgery settles); a Lambda failure
  record taken without its hand's key (the review's); the job given its spec; `first_success` not done at its first
  success; Fargate launched with the NAT off (the review's). The session's own also included a duplicate settled twice
  ("settled once: left 2, right 1").
- **Under load:** the 21 hands tests three times at nice 19 beside four busy loops, 21 of 21 each time (about 17 s
  against 3.6 s unloaded). The lifecycle bench OK for the shape (the poller starts only with `[aws]`, after serving); a
  plain turn 5 frames.
- **The image** could not be built in the cloud: Debian's mirrors answered 403 inside its Docker build. The static
  binary was smoke-run in the pinned base against a small fake of Lambda's runtime API and of AWS: with no spec, `theseus
  hand: THESEUS_HAND is not set`; as Lambda's bootstrap it streamed logs, put `out/f.txt`, `output.txt` and
  `result.json`, sent a signed envelope (`succeeded 0 hand:lambda req-smoke-1`) and answered the runtime API.
  `hadolint` and `shellcheck` exit 0.

**The review** (offline). The owner's condition for the live check (2026-10-03 23:24) was that hands reuse an existing
NAT gateway of another of the operator's projects. The branch cannot take an existing VPC: the hands stacks always make
their own (`theseus-hands-network.yaml`), and `launch.rs` reads only that stack's outputs. The harvest filed
**theseus-mgw.9** (hands on an existing network), and the live check waited for it (overnight decision 5). A nit, not a
blocker: the failure-record check compares hex strings with `!=`, not in constant time; its inputs come from the
account's own queue, whose writers already hold account access.

**The join** (the harvest wake b86bd6c2, 01:44 to 02:01). Prepared in a scratch worktree on e457555d: five conflicts.
`aws/mod.rs` keeps aws-curated's modules and tools beside hands (`NAMES` 16, sorted; `aws.s3.put` writes and
`aws.hands.run` runs; Item 97); `aws/tests.rs`'s expected list gains `aws_hands_run` in order; the facts
kept; the template keeps aws-curated's six commented tool lines and the hands line, `config.rs`'s count 31. **Gate run
1 red in the suite** (01:55:00, 1,995 of 1,996): `a_groups_record_and_its_hands_are_one_frame` expected each hand's
planned, authorized and dispatched copies in the group's frame, but Tier 7.3 (Item 88, after the branch's base) keeps a
frame's last copy of each record. **The join fix, a test only:** the frame holds each hand once, the first wave
dispatched and the rest authorized; the frame's one-ness, which the test is for, holds. The hands tests 20 of 20 on
main with it; amended as ab54f037. **Gate run 2 exit 0 at 02:01:38:** 1,996 of 1,996 (17 skipped, 1 slow); lifecycle
OK on the first run (cold start p50 20.6, p95 26.7; clean shutdown 36.6 and 57.7; kill 24.3 and 31.6; swap 48.9 and
121.4 ms); the jobs phase's L1 start p95 6.04 ms; frames 5 and 9; pushed 02:01. Closed theseus-mgw.6 and
theseus-celu.28. Batch 5's hands-cancel launched on its done line.

**The install** (install #1, 2026-10-04 14:09, at bddfd407). The poller runs only with an `[aws]` account bound, and
only while a group is open. The branch's own live check never ran as written; hands first ran on the account in
hands-cancel's paid live check before the install (11:20 to 13:45, under the owner's decision 9 at 11:26: 23 Lambda
hands for $0.0117, everything torn down after; Item 116), which found this step's `build.sh` failing its
own two checks on a good image (theseus-i7bz: Rust's musl target links static-pie, which `file` does not call
"statically linked", and `set -o pipefail` takes `docker run`'s status in the image check).

**Divergences.** The role is `theseusd hand`, not `theseus hand` (§3.3). A group is a META record and kernel actions,
not a new record kind. The backend rule is the tool's until Jev's `shell.v1`; Batch is not built. No reservation
against the session's budget (part 2's). The hand's view of its own key: it rides in the Lambda event and in the
Fargate task's `containerOverrides` environment, so a reader of CloudTrail's `RunTask` parameters could forge that one
hand's completion, and a Fargate job can read `/proc/1/environ` (its own result only); part 2 could hand the key
through the hand's own S3 prefix.

**Known gaps.** The live check waited for theseus-mgw.9, joined at 12:05 (Item 134). Part 2's list:
cancellation per backend, the TTL reaper's act mode, reservations and a price table, quotas before a launch, the
reconciler's AWS path (an overdue hand goes `outcome_unknown` by the kernel's generic reconcile today), the cockpit's
grid and Discord's group line, the `kill -9` prove. Not built: Batch arrays, a hand whose input is a shard of an S3
prefix, the per-group notice under `notify`. Health does not yet count running hands by backend, the oldest, or the
spend reserved; it counts quarantined completions. The full image's build (`infra/aws/hand/build.sh --push <region>`)
was unproven at the join; its two broken checks were theseus-i7bz (since closed). `infra/aws/README.md`'s table has
no row for `infra/aws/hand/`.

