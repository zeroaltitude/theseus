# CLOUD REPORT: durability, before it goes on (theseus-bfk9, theseus-9ai1)

Branch `cloud/20261005-durability-on`, from `9c0b3c55` (main at `c4f79e9f`). Session started 00:40 UTC on
2026-10-06; this report was written at about 01:35 UTC, well before the deadline (03:40).

| Step | Commit | What |
|---|---|---|
| 1 | `b8297bc8` | bfk9: `ListItsPrefix` under `StringLike` in both sessions; the fake judges as real S3 does; new tests; a policy-print helper |
| 2 | `b2758865` | 9ai1: health's durability line (`crates/theseus/src/render/aws.rs`) |
| 3 | `33ace03e` | 9ai1: the cockpit's AWS card shows the tender (`cockpit/src/lib/durability.ts`) |

No protocol type changes, no stored record changes, no new dependency: no regenerated TypeScript, no store format bump.

## Step 1: theseus-bfk9, `StringLike` in both `ListItsPrefix` statements (`b8297bc8`)

**What I found.** It matches the brief. `durable::policy` (`aws/durable.rs`) and `read::policy` (`aws/durable/read.rs`)
both had `StringLikeIfExists` on `s3:prefix`. `read.rs`'s comment said the check is "judged with the `GetObject`'s own
context, which has no `s3:prefix`", and `durable.rs` pointed to that comment. The fake in `tests_durable.rs`
(`lists_without_prefix`) let a session know a key was missing only when its list statement had no condition that
needed `s3:prefix`, or had an `IfExists` one. So it rejected `StringLike` for the same wrong reason, which is why a
planted revert to `StringLike` used to "fail" four tests.

**What I changed.**
- Both policies now use `{"StringLike": {"s3:prefix": ["durability/<dep>/*"]}}`. Both comments now say what real S3
  does: the implied list carries the key as `s3:prefix`, so a missing key under the prefix is 404 / `NoSuchKey`, and a
  list that names no prefix carries none, so `StringLike` refuses it. I also brought the module docs and `policy`'s
  doc comment into line: "listing the key names under its own prefix", not "the bucket's key names".
- The fake now has `may_list(policy, prefix: Option<&str>)` in place of `lists_without_prefix`. It is a small IAM
  judge for `s3:ListBucket`. A condition on a key the request lacks fails unless its operator ends in `IfExists`.
  `StringLike` does glob matching (`*`, `?`) and `StringEquals` exact matching. `may_know_missing` asks it with
  `Some(key)`: a HEAD's or GET's implied list carries the key itself.
- The two tests that asserted `StringLikeIfExists` (`the_tender_session_is_narrowed_to_its_prefix_and_its_rows`,
  `the_restore_session_only_reads_and_nothing_is_fetched_while_a_daemon_serves`) now assert `StringLike`. I rewrote
  the doc comment of `a_missing_blob_and_a_gap_are_said`.
- New file `aws/tests_list_prefix.rs`. I kept the test files separate because `tests_durable.rs` is already 1,276
  lines.
  - `the_tender_session_lists_only_its_own_prefix` and `the_restore_session_lists_only_its_own_prefix`: a list under
    the session's prefix (the prefix itself, `…/wal/`, a key) is allowed. A list with no prefix is refused, and so are
    `""`, `durability/`, another deployment's prefix and key, and `durability/theseus-lab` (no trailing slash).
  - `the_fake_admits_a_list_with_no_prefix_under_if_exists_only`: this test holds the fake's own model of IAM,
    including `StringEquals` on the exact key against `…/wal/`, as in the probe table.
  - `the_tender_heads_a_missing_key_under_its_prefix_as_missing`: the real tender `Bucket::checksum`, signed in the
    minted `theseus-durability` session, reads a missing key under its prefix as missing (`Ok(None)`). Under another
    deployment's prefix it gets an error (403).
  - `the_restore_gets_a_missing_key_under_its_prefix_as_missing`: the real `Reader::get`, in the minted
    `theseus-restore` session, gets `ReadError::Missing` under its prefix. Under another deployment's prefix it gets
    `ReadError::Other` containing `AccessDenied`.
  - `print_the_durability_policies` (ignored): prints both policies; see the live check.
- theseus-core's AGENTS.md: the tests list now names `tests_list_prefix.rs` and says how the fake judges.

**Proof.**
- `cargo nextest run --workspace -E 'package(theseus-core) & test(/aws::/) | package(theseus) & test(/render::aws/)'`:
  151 run, 151 passed.
- Planted reverts, each with the file restored, `touch`ed and checked with `git status`/`git diff`:
  - `StringLikeIfExists` back in `durable::policy`: `tests_list_prefix::the_tender_session_lists_only_its_own_prefix`
    failed at its "a list with no prefix passed" assertion (line 29), and so did
    `the_tender_session_is_narrowed_to_its_prefix_and_its_rows` (2 of 15 failed).
  - The same in `read::policy`: `tests_list_prefix::the_restore_session_lists_only_its_own_prefix` failed at line 29.
    The run stopped at the first failure; the restore test's literal assertion would fail too.
  - The fake judging a HEAD or GET as carrying no `s3:prefix` (`may_list(p, None)`): 6 of 25 failed. They were
    `the_tender_heads_a_missing_key_under_its_prefix_as_missing`, `the_restore_gets_a_missing_key_under_its_prefix_as_missing`,
    `tests_durable::a_refusal_is_said_in_health_and_the_next_pass_ships`, `an_object_in_flight_that_s3_never_stored_is_sent_once`,
    `an_upload_s3_no_longer_lists_starts_again_once`, and `tests_restore::a_missing_blob_and_a_gap_are_said`.

**FAST.** A policy is a `serde_json::Value` built when a session is minted: `durable::policy` from the tender's
session (`aws/session.rs` through `Kind::Tender(TENDER)`, first used after `START_AFTER`, 2 s after serving), and
`read::policy` from `read::session` in `theseusd restore`, an offline command. Nothing on the start path or the turn
path.

## Step 2: theseus-9ai1, health's durability line (`b2758865`)

**What I found.** `aws_tended_lines` rendered the signer, budget, GuardDuty, reconcile and hands, and never
`a.durability`.

**What I changed.** I added `durability_lines`, called from `aws_tended_lines` when the account has a status. It
prints two lines, because one line ran past about 200 characters:

```
aws: 111122223333 durability caught_up · shipped to position 1,234, 3 min ago · nothing unshipped
aws: 111122223333 durability since the start: 2 segments, 1 tail, 0 blobs, 1,500 rows, 2,097,152 bytes · to s3://theseus-111122223333-us-west-2/durability/theseus-lab/ and the table theseus-durability
```

- Waiting: the reason reads on, `durability waiting for the WAL's sync: position 1240 is written, not yet synced`.
  When `oldest_unshipped_unix_ms` is set, the lag reads `lag 2 min: the oldest record not yet shipped was written
  2 min ago`.
- Shipped: `nothing shipped yet` when the position is 0 and nothing was shipped since the start. `shipped to position
  N (before this start)` when the cursor holds a position but nothing has shipped since the start.
- Loud: `aws: <acct> WARNING: durability failing: <error> (it retries)` and `aws: <acct> WARNING: durability stopped:
  <error> (it will not retry; the store is not shipped off the machine)`.
- Any other state with an error: `durability <state>: <error>`.

**Proof.** Render tests: `a_caught_up_tender_says_what_it_shipped_and_where` (both lines byte for byte),
`a_waiting_tender_says_why_and_its_lag`, `a_failing_or_stopped_tender_reads_loud_with_its_error` (and that caught_up,
shipping and waiting are not loud), and `no_durability_status_no_line`. 6 of 6 pass in `render::aws`. Planted reverts:
- The `durability_lines` call removed: 4 tests failed (caught_up, waiting, failing/stopped, and none's count check).
- `failing` without `WARNING:`: only `a_failing_or_stopped_tender_reads_loud_with_its_error` failed.

**FAST.** `aws_lines` runs in the CLI when `theseus health` renders. The daemon's health already carried the status,
so nothing changed in the daemon.

## Step 3: theseus-9ai1, the cockpit's AWS card (`33ace03e`)

**What I changed.**
- `cockpit/src/lib/durability.ts`: `durabilityView(d, now)` is pure and imports only types. It returns
  `{label, tone, error?, shipped, lag, counts, where}` in the CLI's words. The tones follow the card's existing map:
  `caught_up` ok, `waiting` and `shipping` idle, `failing` wait, `stopped` fault, and an unknown state is said as it
  is, idle.
- `SystemsCards.tsx`: a `DurabilityFields` block per account with a status. It shows a `durability` title with a
  `Pill` and the error or reason in full (wrapped, not truncated, coloured by tone), then `shipped`, `lag`, `since
  the start` and `to` as `Field`s.
- `cockpit/test/durability.test.ts`: four tests (caught_up good with every text, waiting/shipping neutral with the
  reason and lag, failing a warning and stopped bad with their errors, and an unknown state).

**Proof.**
- `npm ci`, `npm run lint` (exit 0, only warnings that were already there), `npm test` (90 pass, 0 fail) and
  `npm run build` (the gate's cockpit phase) all pass.
- Planted revert: `stopped` mapped to `idle` made `not ok 13 - failing is a warning and stopped is bad, each with its
  error` fail (89 pass, 1 fail).
- Not done here: loading the view in a browser against a scratch daemon with durability on. That needs AWS; see the
  live check.

**FAST.** It renders from the page's health read. Nothing changed in the daemon.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` ran once on the whole tree of all three steps, before the
commits. The fmt, shape, features, clippy, cockpit, test build and reader rule phases passed. The suite ran 2,828
tests: 2,795 passed and **33 failed**. All 33 are the known L1-as-root cases (theseus-pv6i): 19 of theseus-sandbox's
contract tests, its bench `spawn_100`, and 13 of theseusd's `sandbox` tests. Nothing else failed, and no test was
retried; the core's golden passed under the TZ.

I ran the phases after the suite by hand, and all passed: protocol types (no change under `cockpit/src/protocol.gen`),
the turn bench (`theseus-sim bench turn --check --runs 5 --burst 0`: plain 5 frames, tool 9, both within budget), and
deny (`cargo deny --offline check`: advisories, bans, licenses and sources ok; the database came from `cargo deny
fetch` in setup). With `NO_BENCH`, lifecycle and jobs are skipped.

The commits touch disjoint files (step 1 only theseus-core's `aws/` and its AGENTS.md, step 2 only the CLI's
`render/aws.rs`, step 3 only the cockpit), so I ran one gate over the three rather than three gates. Each commit's
own tests were run on that tree.

## The live check (the maintainer's)

**(a) The exact policies.** Use the ignored test:

```
THESEUS_POLICY_ACCOUNT=<account> THESEUS_POLICY_REGION=<region> THESEUS_POLICY_DEPLOYMENT=<deployment> \
  cargo nextest run --workspace --run-ignored only --no-capture -E 'test(print_the_durability_policies)'
```

It prints `# durable::policy (session theseus-durability)` and `# read::policy (session theseus-restore)`, each
followed by its pretty JSON. Without the variables it prints the fixtures' account, `us-west-2` and `theseus-lab`.

Mint a session with each JSON as the inline policy (owner role, no managed policies), then probe. Each session should
show:
- `aws s3api list-objects-v2 --bucket theseus-<acct>-<region>` (no prefix): **AccessDenied**.
- The same with `--prefix durability/`: AccessDenied.
- The same with `--prefix durability/<dep>/`: allowed.
- The same with `--prefix durability/<other-dep>/`: AccessDenied.
- `aws s3api head-object --key durability/<dep>/wal/none`: **404**.
- `aws s3api get-object --key durability/<dep>/wal/none /dev/null`: **NoSuchKey**.

**(b) A scratch daemon with durability on.** Use a config with the account's `[aws.accounts.<acct>]` from the
operator's, plus `durability = true` and a deployment of its own so it ships under a fresh prefix (for example
`deployment = "theseus-scratch-9ai1"`), web off or on a scratch port:

```
S=/tmp/dur-scratch && mkdir -p $S
theseusd --config $S/theseus.toml --socket $S/d.sock --state-dir $S/state &
for i in $(seq 60); do theseus --socket $S/d.sock health | grep ' durability '; sleep 1; done
```

What it should show:
- For the first ~2 s, `durability waiting for the start to settle · nothing shipped yet · nothing unshipped`.
- Possibly `waiting for the account's check (...)`.
- Then, after the first pass, `durability caught_up · shipped to position N, 0 min ago · nothing unshipped`, with the
  second line's counts and `s3://theseus-<acct>-<region>/durability/theseus-scratch-9ai1/ and the table
  theseus-durability`.
- Sending a turn (`theseus --socket $S/d.sock ask "hello"`) may briefly read `waiting for the WAL's sync: …` with a
  lag, then caught_up again with a higher position.
- The cockpit's Systems view shows the same in the AWS card: a green `caught up` pill.

**Making a scratch tender `failing` needs no code change**, as long as the daemon is not root:

```
chmod a-w $S/state/durability
theseus --socket $S/d.sock ask "hello"
```

The next pass cannot save its cursor (`Halt::Retry("saving the cursor: …")`). Health should then read `aws: <acct>
WARNING: durability failing: saving the cursor: … (it retries)`, and the card a yellow `failing: it retries` pill with
the error. `chmod u+w $S/state/durability` lets the next retry, within a minute's backoff, reach `caught_up`.

Under root the chmod does nothing. Another way is to point the scratch account's `endpoint` at a closed port, but
then its key check fails too.

I know of no way to make the tender `stopped` without code (a `Halt::Stop` is a WAL the follower cannot open, apart
from a rewind). Its rendering is held by the render and cockpit tests.

Teardown: `theseus --socket $S/d.sock shutdown`, then delete `durability/theseus-scratch-9ai1/` from the bucket and
its rows (`theseus-scratch-9ai1#*`) from the table.

## Docs to fold in (I edited none)

- **Spec Part III Item 147** (`docs/spec/21-part3-item-139.md`, ~line 1071, "The restore's policy"). Add after the
  bullet: "Superseded by theseus-bfk9 (2026-10-06): real S3 judges a missing key's HEAD or GET on the implied
  `s3:ListBucket` with `s3:prefix` set to the key itself, so `StringLike` already answers 404 / `NoSuchKey`, and
  `IfExists` only widened the list to every key name in the bucket; both sessions now use `StringLike`." The heading
  at ~line 1041 ("the restore's list under `StringLikeIfExists`") and the §3.5 note at ~line 1164 can stay as history.
- **Item 171** (`docs/spec/24-part3-item-171.md`, lines 25–30, "The tender lists its prefix"). Its sentences "under
  `StringLikeIfExists` … A HEAD carries no `s3:prefix`, so only `IfExists` lets it through" are false on real S3.
  Suggested note: "(theseus-bfk9: a HEAD's implied list carries the key as `s3:prefix`; the statement is now
  `StringLike`, which refuses a list with no prefix.)" Line 87 ("needs `StringLikeIfExists` too if it ever reads
  back") should read "`ListBucket` under `StringLike` on its prefix". Line 105's "health's durability line" now
  exists (theseus-9ai1).
- **Item 118** (`docs/spec/19-part3-item-118.md`, ~line 687): "(the fix would be `StringLikeIfExists` …)". Add:
  "(it was not needed: theseus-bfk9's live probes show `StringLike` gives 404.)"
- **`docs/design/aws-toolset.md`** ~line 1100: "`s3:ListBucket` under `StringLikeIfExists` on
  `durability/<deployment>/*`" should read "`s3:ListBucket` under `StringLike` on `s3:prefix` =
  `durability/<deployment>/*` (a missing key's implied list carries the key as its prefix, so it heads 404; a list
  with no prefix is refused)".
- **The new Part III item** should say: health's text and the cockpit's AWS card show the tender (state, why, the
  position shipped and when, the lag, the counts, where), and `failing`/`stopped` say `WARNING:`.
- **`docs/status.md`**: durability's line can say that health now shows it.

## Left or uncertain

- The fake's IAM judge covers what the policies use (`StringLike`, `StringEquals`, `IfExists`, and keys other than
  `s3:prefix` treated as absent). It does not model `Deny` or `NotAction`.
- The fake does not check `GetObject`'s own resource scope. A missing key under another prefix is 403 there only
  because the implied list is refused, which matches real S3's answer but not its full reasoning.
- Design choice: health says the durability block in two lines, because one ran past ~200 characters. The counts and
  where it ships go on the second line, so the first, with the state, stays short and the `WARNING:` is easy to see.
- The TUI has no AWS view, and I left it alone as asked.
- The cockpit change was not looked at in a browser here. The live check covers it.
