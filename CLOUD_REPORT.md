# Cloud report: AWS's curated tools, C3 (theseus-mgw.5), and `confirm-alerts` (theseus-9p40)

Branch `cloud/20261004-aws-curated`, from `main` at d9b0931. Not for main: drop this commit at the merge.

The VM: 4 cores, everything runs as root, no AWS credentials. Everything below was built and proved against
the local fake endpoint the C1 and C2 tests use. Each gate ran with `THESEUS_GATE_NO_BENCH=1` and
`TZ=America/Los_Angeles` (see "The gate", below, for why the zone).

## The commits

| commit | step |
|---|---|
| 3273e88 | C3.1: `aws.s3.get` and `.put`, `aws.logs.query` and `.tail`, `aws.trail` |
| 8898567 | C3.2: secret-bearing handles on the secrets board |
| 5b7657c | C3.3: external marking for data-plane reads |
| 0721d6e | C3.4: `aws.inventory`, and the reaper in report mode |
| db0eaaf | 9p40: `theseus aws confirm-alerts` |
| 47857d2 | C3.5: the CloudTrail cross-check tender |

## C3.1: the tools (3273e88)

**What I found.** The C1 and C2 tools are recipes over `Account::request`, which already records each request on the
call's `AwsBinding` (the `aws.called` row and the span). The client (theseus-aws) reads a blob payload whole into memory, up
to `max_response_bytes` (16 MiB). It does not stream, and it has no multipart upload.

**What I changed.** I added new modules beside `tools.rs` (`s3.rs`, `logs.rs`, `trail.rs`). Each tool is `Backend::Async`,
checks its whole call in `plan` with no network, and carries an `AwsPlan` whose guardrail and destructive flags come from
`tools::guard`, as `aws.call`'s do.
- `aws.s3.get`: an object as text, or to a workspace file. The file is a `Write` resource of the plan, so the floor and
  the roots apply to it. A get to a file sends `ChecksumMode: ENABLED`, checks S3's whole-object SHA-256 when there is one,
  and writes nothing on a mismatch. `range` reads part of an object, and an object over 16 MiB is read by ranges.
- `aws.s3.put`: a workspace file (a `Read` resource) or a text, in one `PutObject` of at most 64 MiB. It sends its own
  `x-amz-checksum-sha256` and optional tags. Its class is write, so `[policy.aws] write` applies (`aws::tool_class` too).
- `aws.logs.query`: Logs Insights over named groups, or a `prefix` that `DescribeLogGroups` resolves. `since` and
  `until` are spans ago. The tool waits with tokio's timer (500 ms, doubling to 5 s) up to `wait` (60 s by default, at
  most 300 s). The result is a table with the bytes scanned and their cost (at $0.005 a GB). A query still running hands
  back its `query_id` for a later call.
- `aws.logs.tail`: `FilterLogEvents` with a stream prefix and a filter pattern. `follow` keeps reading for up to 60 s,
  polling every 2 s, with events deduplicated by id.
- `aws.trail`: `LookupEvents` by execution (its session name), by resource, or by event name. Each line shows the
  request id, the `call/<id>` from the user agent, the source identity, and the error code.
- A denial now says who refused, in plain words (`tools::refused_by`): Theseus's own guard (an explicit deny in a
  session policy), the session policy's narrowing (an implicit one), IAM, or an Organization.
- The template's `[policy.tools]` lists the new tools. Its test counts them: `config.rs` changes one number, no lines.

**How I proved it.** `aws/tests_c3.rs` has 7 tests on the fake. They cover each tool's request (method, path, headers,
JSON body) and result, invalid input with nothing sent, the file's checksum and its mismatch, the put's body, checksum,
and tags, the query's wait and table, a denial naming the guard or the session policy, and planning that sends nothing.

**Left.** Multipart puts and streaming gets need the client to stream: a client change, so I left both out. The tool
says so and points to the aws CLI. Logs Insights' price is a constant, $0.005 a GB, us-west-2's.

## C3.2: secret-bearing handles (8898567)

**What I changed.** `aws.call` now runs a secret-bearing operation. Until now that was invalid input, "until 14c".
`aws::secret::hold` walks the output along the operation's output shape. Each member that the model marks `sensitive`,
or whose name is a secret's (`SecretString`, `SessionToken`, `Plaintext`, `Credentials`, tokens), goes onto the board
under a handle, `aws-secret:<SecretId|Name>#<member path>`. The new `SecretBoard::hold` puts it there: zeroizing memory,
never written, and not counted as a vault round. The result shows the handle and the value's shape with the secret
masked (a JSON secret's keys, else its length), and `meta.secrets` lists the handles. A secret-bearing call in which
no member was found to hold the secret returns none of its output, so it fails closed.

**How I proved it.**
- `secret::tests` take the catalog's own outputs for Secrets Manager, SSM `GetParameters`, KMS `Decrypt`, and STS
  `AssumeRole`: every secret is found, masked, and on the board.
- `tests_handles::a_secret_becomes_a_handle_before_the_scrubber_sees_it`: the tool's text and meta, before any
  scrubber runs, carry the handle and never the value.
- `tests_handles::a_handles_value_never_reaches_a_node_the_wal_or_the_ledger`: this is the scrubber's test pattern. A
  turn through the whole core reads a secret. Then every file under the state dir (the WAL, the index), every ledger
  row, the trace, and the result node are searched for the value, and none holds it. The board holds it, and the
  scrubber withholds it.
- **Planted revert:** the value held on the board but left unmasked in the result. Three tests failed: the catalog
  test, the tool test, and the core test. I restored the file, touched it, and `git status` was clean.

**For the owner.**
- The planted revert turned up a gap in the scrubber. `aws.call` prints its output as pretty JSON, so a secret that
  contains quotes appears JSON-escaped, and the scrubber, which matches the raw value, missed it. The masking stops that
  happening here. But a tool that prints JSON with a secret inside would leak past the scrubber. Suggestion (not made,
  since it is the security lane's file): have `scrub.rs` also match each value's JSON-escaped form.
- A program cannot be granted a handle yet. `[broker]` grants name `[secrets]` entries, which the config checks
  (`config.rs` around line 1277). Granting `aws-secret:*` handles needs a broker change.
- Handles appear by name in health's secrets `ready` list. Names only.

## C3.3: external marking (5b7657c)

**What I changed.** `aws::external::marker` builds the same `theseus_tools::External` a fetched page carries. These
reads now return it:
- `aws.s3.get` read as text (`s3://bucket/key`);
- `aws.logs.tail` (`logs:<region>:<group>`) and `aws.logs.query` (`logs:<region>:query/<id>`), when they return lines;
- `aws.trail` (`cloudtrail:<region>:LookupEvents`), when it returns events.

The runtime then writes the session's hold, with its `session.external_read` row, in the frame that writes the result.
A get to a file, the listings, and the inventory hold nothing.

**How I proved it.** `aws/tests_outside.rs`:
- each data-plane read returns its marker, and a get to a file returns none;
- through the core, an object read as text holds the session, and the next `aws.s3.put` asks: its
  `tool.confirm_requested` reason names the object, and `pending_confirms` carries it;
- a session that only saved an object holds nothing.

**Planted revert:** `aws.s3.get` returning no marker. The first two tests failed.

## C3.4: the inventory, and the reaper in report mode (0721d6e)

**What I changed.** I added `aws.inventory` (a read). It calls `GetResources`, filtered on `theseus:owner = theseus`,
in each of the account's regions. It adds each stack's age from `DescribeStacks`, and, with `costs: true`, each stack's
spend this month from Cost Explorer by its `theseus:stack` tag ($0.01 a call). What AWS returns is checked again
(`inventory::owned`). A resource whose tags do not say `theseus:owner = theseus` is left out: it is counted, and named
nowhere.

The reaper reports and deletes nothing (`inventory::verdict`):
- a stack whose `theseus:ttl` has passed would be deleted whole;
- a resource of a stack (`aws:cloudformation:stack-name` or `theseus:stack`) goes only with its stack;
- a loose ECS task or Batch job would be stopped;
- a loose resource would be deleted, or asked about when it holds state;
- a resource with no ttl, a ttl not yet passed, or an unreadable ttl is kept.

TTLs are read as the hands' Lambda reaper reads them: ISO 8601, UTC when no offset is given.

**How I proved it.**
- `inventory::tests` cover the reaper's rule per kind, and ARNs to kinds.
- `tests_inventory` uses a fake account that answers as if the tag filter were absent. It returns another project's
  resources: untagged, tagged for that project, and a foreign stack. None is listed or named. Each region's request
  carries the tag filter, only reads are sent, and the report names the expired stack, the task, and the stateful
  bucket.
- **Planted revert:** keeping untagged resources. The test failed, naming the other project's bucket.

**Left.** The reaper is a tool, not yet the hourly tender of §3.7. A tender in report mode would need a health line,
which means a protocol field in `lib.rs`, at its ceiling. The 14 days of reports before it acts are a later step.

## Part 2: `theseus aws confirm-alerts` (db0eaaf)

**What I changed.**
- `theseus aws confirm-alerts [TOKEN] [--account]` takes the token, or the whole confirmation link (copied, never
  opened), from the argument or from stdin. Stdin keeps the token out of the shell's history.
- It sends the new method `aws.confirm_alerts`, which is the CLI's alone. It is refused from Discord, the web UI, and
  inside a job, as the bootstrap is; it is in `OPERATORS`.
- The core (`aws::alerts`) calls `sns:ConfirmSubscription` on `arn:aws:sns:<region>:<account>:theseus-alerts` with
  `AuthenticateOnUnsubscribe=true`, signed by the account: the owner role's work session, or the key before the role
  exists. It reads the subscription back with `GetSubscriptionAttributes`.
- It reports `ConfirmationWasAuthenticated`, with the address's local part masked. The CLI exits nonzero when it is
  false, and says what to do.
- The token is never printed, logged, ledgered, or kept. The params' `Debug` withholds it, and AWS's words come back
  with it masked.
- A bad or expired token says so in plain words: mistyped, or older than SNS's three days, and how to get a new one.
- The bootstrap's closing text and the command's help tell the operator to paste the token instead of opening the link.
- The web apps' generated TypeScript gains the two types.

**How I proved it.** `aws::alerts::tests`, on the fake:
- the confirmation is SigV4-signed for sns in us-west-2, carries the token and `AuthenticateOnUnsubscribe=true`, and
  goes to the alerts topic;
- the subscription is read back and reported authenticated, with no token in the result or its lines;
- a bad token gets the plain words and is not echoed;
- a pasted link gives its token;
- the params never print it.

The CLI's existing operator test (`client::tests`) covers the in-a-job refusal. **Planted revert:**
`AuthenticateOnUnsubscribe=false`. The signed-confirmation test failed.

**A live check on a scratch daemon of this build** (`theseusd --config <the template> --op-token-file <a dummy>
--socket <scratch> --state-dir <fresh>`, with a stand-in `op` that fails, so it serves with no secrets and no AWS
table):
- `theseus aws confirm-alerts <token>`, a link piped on stdin, and a short token each answered "no AWS account is bound:
  the config has no [aws.accounts.<id>] table" (exit 1). The method is dispatched, and the error comes before any token
  check.
- With `THESEUS_SESSION` set, the CLI refused: "theseus aws confirm-alerts refused: it is the operator's to run…".
- No file of the daemon's log or state dir held the token.
- The bound path (STS, then SNS) needs a key from the vault, so it was proved on the fake instead.

**Shared files touched:** the CLI's `main.rs` (one `AwsCmd` variant), `rpc/server.rs` (one dispatch arm),
theseus-protocol's `lib.rs` (one method, +3 lines, under its ceiling) and `ts.rs`, and `web/src/protocol.gen/`. That
directory moves to `cockpit/` in another change, so regenerate it there at the merge.

## C3.5: the CloudTrail cross-check tender (47857d2)

**What I changed.**
- `aws/crosscheck.rs` holds the comparison: Theseus's identities are the owner role's sessions and the key's user. A
  role session's event is accounted for by its request id in an `aws.called` row, by its session being a job's
  correlation id, or by being a tender's own session (`theseus-*`). Any event of the key's other than
  `sts:GetCallerIdentity` or `sts:AssumeRole` is unaccounted, whatever the ledger says.
- The tender runs daily from an hour after serving, for each account with an owner role (`tend.rs`). It reads the 24
  hours that ended an hour ago (at most 20 pages), in the tender session, whose inline policy gains
  `cloudtrail:LookupEvents`.
- Each run writes one `aws.trail.checked` row (a new `LedgerKind`; no record layout changes, so `MANIFEST_FORMAT`
  stays). The row names each unaccounted event's time, name, session, and request id, and the log warns on each.

**How I proved it.**
- `crosscheck::tests::the_check_finds_what_nothing_accounts_for` covers ledgered, job, and tender sessions (accounted
  for), another project's role (not judged, and not named in the row), an unledgered work session, and the key signing
  `iam:CreateUser` (both found).
- `a_check_reads_the_day_that_ended_an_hour_ago` checks the window, the region, and that the caller is read from each
  event.
- **Planted revert:** dropping the key's STS-only rule failed the first test.

**Left, and for the owner.**
- The tenders' own calls write no `aws.called` rows (they have no binding), so the check accounts for them by session
  name. A tender call is therefore not joined by request id, which is weaker than a call's.
- Only the account's own region is read. CloudTrail keeps each region's history, and IAM's events live in us-east-1.
- An unaccounted event is a ledger row and a log warning. The design calls it a security notice: a push to the
  operator's DM through the outbox would make it one, as a follow-up.

## The live checks (the maintainer's, on the owner's account)

Use a scratch daemon of this build with the operator's AWS account bound (its `[aws.accounts.<id>]` table, with
`owner_role`), on a fresh state dir, from a private place. `S` is its socket.

Read-only first:
1. `theseus --socket $S ask "Run a Logs Insights query over the last 2 hours of <a log group the account has>: fields
   @timestamp, @message | sort @timestamp desc | limit 20"`. It runs `aws_logs_query`, and the result is a table with
   "records matched", "bytes scanned (about $…)". `theseus --socket $S ledger` shows
   `aws.called` rows for StartQuery and GetQueryResults, and `session.external_read` naming `logs:<region>:query/<id>`.
   The session now holds external text: a following write asks.
2. `theseus --socket $S ask "Look up in CloudTrail the calls of execution <an exe_ id from C2's ledger>, since 7d"`.
   `aws_trail` lists events by `…assumed-role/theseus-owner/exe_…`, each with a request id that matches an
   `aws.called` row of that execution, and `call act_…`.
3. `theseus --socket $S ask "Take the AWS inventory, with costs"`. `aws_inventory` lists only resources tagged
   `theseus:owner = theseus` (the foundation's, posture's, relay's), never the other project's. "The reaper, in report
   mode" says nothing to reap unless a `theseus:ttl` has passed. One Cost Explorer call ($0.01).

Then writes:

4. `theseus --socket $S ask "Write the text 'c3 check' to s3://<the foundation's bucket>/checks/c3.txt, then read it
   back"`. The put is notified (`[policy.aws] write = notify`); if the get ran first in the same session, the put asks,
   which is correct. The get returns "c3 check", and its text holds the session. A get with `to` writes the file, and
   says "S3's SHA-256 matches" for a single-part object.
5. When a confirmation email arrives (a new foundation, or the address subscribed again):
   `theseus aws confirm-alerts` from the operator's own shell, pasting the link's address on stdin, without opening the
   link. It prints "Confirmed o…@domain's subscription to arn:aws:sns:…:theseus-alerts (…)" and
   "ConfirmationWasAuthenticated = true". Afterwards, `aws sns get-subscription-attributes` in an owner session shows
   `ConfirmationWasAuthenticated: true`.
6. After a day of the scratch daemon: an `aws.trail.checked` row with `unaccounted: []`.

## The gate

On every step, every phase passed but the suite. Its failures, every one known or not this branch's:
- **The sandbox's contract tests (theseus-sandbox::contract, 13 or so) and theseusd::sandbox (12 or so), and
  theseus-sandbox::bench spawn_100.** About 32 per run, failing because this VM runs everything as root (theseus-pv6i).
  The L1 jobs bench (`bench jobs --class l1`) panics for the same reason.
- **theseus-sim::sim `the_kernel_holds_its_invariants_under_seeded_faults`**, in three of the six suite runs (steps 1 and 2, and Part 2).
  It fails with "no series was put back": a coverage assertion from the repeating wakes (37a), whose fresh seeds
  sometimes take no repeating wake. Run alone it passed 2 of 3 times. It touches nothing of AWS. It is not on the flaky
  list, so it should get an issue.
- **theseus-core `tests_output::the_cores_output_matches_its_golden`** fails under this VM's UTC zone: a wake's line
  prints `+00:00` where the golden has the owner's `-07:00`. It passes with `TZ=America/Los_Angeles`, which is why the
  gates ran with it. The golden depends on the machine's zone, which deserves an issue.

The phases after the suite (protocol types, the lifecycle bench, the turn bench's 5 frames) I ran by hand each time,
and they passed. `cargo deny fetch` worked, so the deny phase ran.

## Docs to change (the maintainer's)

- `crates/theseus-core/AGENTS.md`, "Tool calls": list `aws.s3.get`/`.put`, `aws.logs.query`/`.tail`, `aws.trail`,
  `aws.inventory`, the handles (`aws/secret.rs`, `SecretBoard::hold`), external marking (`aws/external.rs`), the
  cross-check (`aws/crosscheck.rs`), `aws/alerts.rs`, and the test files `tests_c3.rs`, `tests_handles.rs`,
  `tests_outside.rs`, `tests_inventory.rs`.
- `docs/design/aws-toolset.md` §5's C3: what was built. The reaper as a tool, not yet a tender. The Observatory's panel
  becomes the cockpit's (below).
- Part III's item and `docs/status.md`, for C3 and 9p40.

## What the cockpit's AWS panel should show (instead of the Observatory's)

1. **Account:** health's `aws` block, with the key's age and the live sessions.
2. **Spend:** the budget line, and `aws.cost`'s breakdown.
3. **Calls:** the `aws.called` rows per execution (operation, class, outcome, latency, request id), each joined to its
   CloudTrail event by request id.
4. **The cross-check:** the latest `aws.trail.checked` row, with any unaccounted event flagged red.
5. **The inventory and the reaper's report:** what `aws.inventory` returns. This needs a read method, or a periodic
   ledger row, since a tool's result is a session's.
6. **Guardrails:** floor hits, and the denials AWS reported, with their enforcer from the `aws.called` rows.
7. **Alerts:** whether the subscription is authenticated. That needs a periodic `GetSubscriptionAttributes` read.
