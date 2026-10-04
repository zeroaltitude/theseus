# Cloud report: hands on Lambda and Fargate, step 40 part 1 (theseus-mgw.6)

Branch `cloud/20261004-aws-hands`, from `main` at `d9b0931`. Started 03:01 UTC, report at about 04:20 UTC.
This commit is not for main (subject `cloud report (not for main)`); drop it at the merge.

| commit | what |
|---|---|
| `5161d0e` | aws: the hand role, `theseusd hand`, and its signed envelope |
| `94c99f0` | infra: the hand image, its Dockerfile and build script |
| `e0060bc` | aws: `aws.hands.run` on Lambda and Fargate, and the completion poller (sub-steps 3 and 4 in one commit: a launch with no path home is half a step) |

## Setup

nextest, cargo-deny (`cargo deny fetch` succeeded, so the deny phase ran offline with advisories), `npm ci` in
`cockpit/` and `web/` (web/ is still on this main), the workspace build. I also installed Debian's `musl-tools` and
started `dockerd` on the VM, to build a static `theseusd` and smoke it in the image's base (see step 2).

## Step 1: the `hand` role (`5161d0e`)

**Found.** Helper roles live in `theseusd` (`job-wrapper`, `job-sandbox`), not the `theseus` CLI. The CLI links only
`theseus-protocol` (its AGENTS.md says so), and the hand needs the AWS client. The prompt and design §3.3 say
`theseus hand`. I built it as **`theseusd hand`**: the static binary that already carries the roles. **The owner
should hear this**, and §3.3 should say `theseusd hand` (or say why the CLI should grow the AWS client).

**Built.**
- `crates/theseus-core/src/aws/hands/envelope.rs`: the per-dispatch key, `HKDF-SHA256(salt "theseus-hands/1", ikm =
  the account's secret access key, info = correlation id)`. No new secret, nothing stored. Also `HandSpec` (its `Debug`
  withholds the key) and `Envelope`, spec §3.16's completion with its `signature`. That is `hmac-sha256:<hex>` over
  length-prefixed fields, the tail by its digest, compared in constant time.
- `hand.rs`: `main()` serves Lambda's custom runtime when `AWS_LAMBDA_RUNTIME_API` is set; otherwise it runs the one
  spec in `THESEUS_HAND` (Fargate). `run()` does five things:
  - runs the argv in `/tmp/hand-<corr>`, in its own process group, under `deadline_secs` (SIGTERM to the group, 5 s,
    then SIGKILL);
  - streams lines to CloudWatch Logs `hands/<group>/<index>` every second;
  - uploads every file under `out/` (≤ 64 MiB each, ≤ 1000 files), `output.txt` (the last 256 KiB), and `result.json`
    to `s3://<bucket>/hands/<corr>/`;
  - signs the envelope and sends it to the queue, with 3 tries;
  - on Lambda, answers the runtime API, or reports an error so the async failure destination fires.
- The job never sees its spec (`env_remove(THESEUS_HAND)`). It gets `THESEUS_HAND_INDEX/_INPUT/_GROUP/_OUT`, and
  `{index}`/`{input}` replaced in its argv.
- Credentials: Lambda's environment, or ECS's container credentials endpoint, fetched again after 10 minutes. Never
  the daemon's.
- `theseusd`'s `Cmd::Hand` (hidden). `hmac`/`hkdf` were already in Cargo.lock (only theseus-core's dependency list
  changed; no package added).

**Proved.**
- `aws::hands::envelope::tests` (4) and `aws::hands::tests_hand` (3), against the local fake (`aws::tests::Fake`). The
  fake answers Logs, S3, and SQS, and the tests cover:
  - a job with input and no key in its environment;
  - log lines, four S3 keys in order, an envelope on the queue that verifies with its key and not another's, and
    `call/<corr>` in every user agent;
  - exit 3 → failed; a deadline of 1 s on `sleep 300 & sleep 300` → failed and timed out in about 1 s; a missing
    program → failed with a note;
  - a queue that refuses → "the envelope was not sent".
- Planted revert: the job given `THESEUS_HAND` → `a_hand_runs_its_job_uploads_its_result_and_signs_its_envelope`
  failed. Restored, touched, and `git status` clean.

## Step 2: the hand image (`94c99f0`)

**Built.** `infra/aws/hand/Dockerfile` contains:
- `debian:bookworm-slim` pinned by digest (pulled 2026-10-04);
- ca-certificates, curl, git, jq, less, openssh-client, python3, python3-venv, unzip, and zip;
- the AWS CLI v2 2.27.50, pinned by its zip's sha256 (I downloaded it and computed the sum);
- user 1000, `HOME=/tmp`, and `ENTRYPOINT ["/usr/local/bin/theseusd"]` with `CMD ["hand"]`.

`build.sh` refuses a dirty tree. It builds `theseusd` for `x86_64-unknown-linux-musl` with `scripts/build.sh
--profile release-thin`, checks the binary is static, builds the image as `theseus/hand:<commit>`, and checks it
answers as a hand. With `--push <region>` it logs in to ECR, pushes, and prints `HandImageUri=…@sha256:…`.
`README.md` documents both.

**Proved.**
- `hadolint` exit 0 (one documented `DL3008` ignore for the unpinned apt packages; DL4006's pipe removed);
  `shellcheck` exit 0 (both run from their Docker images).
- **The full image could not be built here:** Debian's mirrors answer 403 from inside a Docker build on this VM.
- **The binary was smoke-run in the pinned base instead.** I built a debug static musl `theseusd`, put it in an image of
  the same base, `ENTRYPOINT`, and `CMD` without the apt layer, and ran it against a small Python fake of Lambda's
  runtime API and of AWS:
  - with no spec: `theseus hand: THESEUS_HAND is not set`;
  - as Lambda's bootstrap: `CreateLogStream`, 2× `PutLogEvents`, PUT `out/f.txt`, `output.txt`, `result.json`,
    `SendMessage`, then `POST /runtime/invocation/req-smoke-1/response {"outcome":"succeeded","exit_code":0}`;
  - the envelope: `succeeded 0 hand:lambda req-smoke-1`, signed.

## Steps 3 and 4: `aws.hands.run` and the poller (`e0060bc`)

**Design choices the owner should hear about:**
1. **No new record kind, so `MANIFEST_FORMAT` is unchanged.**
   - The group is three things: the call's own action, a **META record** keyed `aws.hands.group.<group>` (the
     pattern of `wake.target.*` and the task META), and **one kernel action per hand** (tool `aws.hand`, `resource =
     aws.hands.group.<group>`).
   - So settling is literally `Kernel::accept_completion`: dedupe, quarantine of an unknown id, late-after-cancel, and
     the reconciler's `overdue_no_evidence` at a hand's deadline (TTL + 5 min) all come free.
   - If the maintainer counts a new META layout as a format change, it needs a bump and a sample in `tests_layouts`.
2. **The call answers `background`.** It runs through `toolrun/hands.rs` (one `if` in `execute`), not `run_inproc`.
   The group's aggregate is its late result (one `if` in `job_result`). The call's frame holds every hand's planned
   and authorized records, the first wave's dispatch, and the group record. Each later wave is dispatched (its own
   frame, the claim) before it launches.
3. **Where hands run is discovered, not configured.** `DescribeStacks` of `theseus-foundation`, `theseus-hands`, and
   `theseus-hands-network`, read once per account and region and kept in the group record, so a restarted poller
   needs no network to know its queue. No config key, so the template only gains the commented
   `[policy.tools] "aws.hands.run"` line (and `config.rs`'s count of those lines goes from 24 to 25).
4. **The NAT stays off.**
   - A Fargate call while the network stack's `NatGateway` output is `disabled` fails with "Fargate hands need the hands
     VPC's NAT, which is off (about $36 a month while on): … NatGateway=enabled, or the call runs on Lambda", and
     nothing is launched.
   - Nothing in the code turns it on; the lazy NAT stays filed.
   - The default for short work is Lambda (no VPC).
   - Note: with the NAT off, a Fargate hand cannot even pull its image or reach the queue (the stack has no interface
     endpoints).
5. **Backend choice** (§3.3, until Jev):
   - Lambda when there is no `image`, the profile is `basic`, `memory_mb` ≤ 10240, and `ttl_secs` ≤ 600; otherwise
     Fargate.
   - A named `lambda` that cannot run there is invalid input.
   - The Lambda function's memory is the stack's `LambdaHandMemoryMb`, whatever the call asks.
   - Batch is not built.
6. **Budget.** Only a launch cap.
   - `max_usd` limits how many hands launch, at their worst case (TTL × the rate for their size). The rates are
     constants in `launch.rs` (us-west-2, x86).
   - Each hand's cost from its real duration is its completion's `cost_micros`, summed into the group.
   - Reservations against the session budget are part 2's.
7. **Running hands are not cancelled** when `until` is met: they run to their TTL. Their late envelopes settle their
   own actions and nothing else. Never-launched hands are `cancel_verified` (nothing ran).
8. **Poller.**
   - It starts after serving (`Core::poll_hands_after_serving`, one line in `theseusd`'s `after_serving`, beside the AWS
     check).
   - It polls only while a group's call or any hand is dispatched; otherwise it waits on a `Notify` that a launch rings.
     It sends 20 s `ReceiveMessage` calls, takes envelopes first in each batch, and deletes each message once its frame
     is written.
   - Messages it does not read (budget alerts, other sources) are left for their reader and, in time, the DLQ.
   - Its own `ReceiveMessage` and `DeleteMessage` are not ledgered (about three rows a minute otherwise); launches and
     discovery are.
9. **What the poller reads, besides envelopes:**
   - A Lambda failure-destination record counts only if its `requestPayload.key` is the hand's derived key; a forged
     one is quarantined.
   - An ECS STOPPED event whose `hand` container did not exit 0 marks the hand `outcome_unknown`; the envelope, if it
     comes, resolves it. Exit 0 means the envelope is coming, so the event is ignored.
10. **Quarantine.** A `quarantine:<corr>` completion record (the kernel's own prefix, so health's quarantined count
    sees it) and a `completion.quarantined` row with `why`. Nothing settles.
11. **Two known gaps in the key's handling, for review:**
    - The key rides in the Lambda event and in the Fargate task's `containerOverrides` environment. CloudTrail records
      `RunTask`'s request parameters, so a reader of the trail could forge **that one hand's** completion.
    - On Fargate the job can read `/proc/1/environ` (same uid), which lets it forge only its own result.
    - Part 2 could hand the key through the hand's own S3 prefix instead.

**Files.**
- New, under `aws/hands/`: `launch.rs`, `group.rs`, `poller.rs`, `tool.rs`, and `tests_hands.rs`.
- New: `toolrun/hands.rs`.
- Small additions to shared files:
  - `aws/mod.rs`: the `mod` line, a `hands` field, the tool in `NAMES` and `tool_class` (`run`), and its registration;
  - `toolrun.rs` (2 lines), `toolrun/job.rs` (3 lines plus `pub(super)`), and `fact/mod.rs` (3 entries);
  - `ledger.rs`: 2 kinds;
  - `theseusd/src/main.rs`: 1 call;
  - `aws/tests.rs`: the expected tool list gains `aws_hands_run`.
- `crates/theseus-core/AGENTS.md` and `crates/theseusd/AGENTS.md` describe the hands and the role.

**Proved** (`aws::hands::tests_hands`, 9 tests through the whole core, against a stateful fake of STS,
CloudFormation, Lambda, ECS, Logs, S3, and SQS with a real queue):
- `a_lambda_group_launches_then_settles_from_its_hands_envelopes`:
  - the record, 2 hand actions dispatched, and a background answer;
  - 2 Invokes (`Event`), each with its own spec and key, and `aws.called` rows; no key in any row;
  - then the **real hand role** runs each spec against the fake, the poller settles each hand and the group, and
    deletes both messages;
  - the continuation's late result reads "2 of 2 succeeded" with each hand's tail and S3 prefix.
- `a_groups_record_and_its_hands_are_one_frame`: a spy on the turn's frames sees the group record with 3× planned,
  3× authorized, and 2× dispatched (`concurrency: 2`) in one frame. With that frame failed (`fail_turn_frame`): no hand
  action, no META, no launch.
- `a_bad_signature_is_quarantined_and_never_settled`: the forged envelope is quarantined, its message is deleted, and
  the hand stays dispatched; the real one then settles it.
- `a_duplicate_settles_once_and_a_late_one_settles_only_its_hand`: one `action.succeeded` and one
  `completion.duplicate`; the late hand settles after its group did, with one `aws.hands.settled`.
- `first_success_stops_launching_after_the_first_success`: 4 hands at concurrency 1; #0 fails, #1 launches and
  succeeds, exactly 2 Invokes, and #2 and #3 are cancelled and never dispatched; the result reads "1 of 4 succeeded,
  1 failed, 2 not launched".
- `the_poller_is_idle_with_nothing_outstanding`: 0 receives and 0 requests for 800 ms; polls once a group launches;
  quiet again once it settles.
- `a_restart_with_completions_waiting_settles_each_once`: core A launches and is dropped; envelopes, one of them
  duplicated, wait in the queue; core B on the same store polls and settles each hand once and the group once.
- `a_lambda_failure_record_settles_its_hand_and_a_forged_one_is_quarantined`: the dropped-completion case.
- `fargate_waits_for_the_nat_and_runs_tagged_in_private_subnets`:
  - NAT off → refused, and no ECS call;
  - NAT on → one task definition (the init container and its `dependsOn SUCCESS`, the image, and the task role);
  - 2 RunTasks: FARGATE, `startedBy` the group, `assignPublicIp DISABLED`, the subnets, all 7 tags, and the spec in the
    environment;
  - a STOPPED event with exit 137 → unknown, then the envelope resolves it (one `action.resolved`).
- Plus unit tests: the backend choice, request checks, Fargate sizes, cost, and the TTL's UTC form.

**Planted reverts** (each restored, `touch`ed, and `git status` checked):
- Skip the signature check (`if false && !e.verify(..)` in `poller.rs`) → `a_bad_signature_is_quarantined_and_never_settled`
  failed ("waited 20 s for the forgery's quarantine": it settled instead).
- Settle a duplicate twice (the kernel's `Succeeded | Failed` dedupe arm disabled) →
  `a_restart_with_completions_waiting_settles_each_once` failed ("settled once: left 2, right 1"), and
  `a_duplicate_settles_once_and_a_late_one_settles_only_its_hand` failed (no `completion.duplicate` row).
- `first_success` not done at its first success → `first_success_stops_launching_after_the_first_success` failed.

**Under load.** All 21 hands tests ran with `nice -n 19` beside four busy loops at nice 0 (killed by their pids),
three times: 21/21 each time (about 17 s against 3.6 s unloaded). They also passed 21/21 three times unloaded.

**The lifecycle bench, for the shape:** `theseus-sim bench lifecycle --runs 5` → `LIFECYCLE OK` (the poller starts
only with `[aws]`, and only after serving). The turn bench: 5 frames at the p95, budget 5.

## The live check (the maintainer's; it costs money, so the owner says when)

Run from the operator's own shell, against a scratch daemon of this branch's install build (its own `--config`,
`--socket`, `--state-dir`), with the account bound (`[aws.accounts.<id>]`, `owner_role` set) and the foundation and
posture stacks in place. `<acct>` and `<region>` stand for the bound account and its home region; nothing here names
them. `S=<the scratch daemon's socket>`.

### 0. The image (once a commit)

```bash
infra/aws/hand/build.sh --push <region>
```

Expect: `build.sh: theseus/hand:<commit> answers as a hand`, the push, and a last line
`HandImageUri=<acct>.dkr.ecr.<region>.amazonaws.com/theseus/hand@sha256:<digest>`. Docker inside this VM could not
reach Debian's mirrors, so the full image was never built here (the binary was smoke-run in the pinned base; see the
report). If `apt-get` or the AWS CLI's checksum fails in a normal network, that is this step's finding.

### 1. The hands stacks (NAT off)

Through Theseus's stack path (the deployer applies; each plan shows its change set first):

```bash
theseus --socket $S ask 'Plan the stack theseus-hands-network from infra/aws/theseus-hands-network.yaml with NatGateway=disabled (aws.stack.plan), and show me its change set.'
theseus --socket $S ask 'Apply that change set (aws.stack.apply).'
theseus --socket $S ask 'Plan the stack theseus-hands from infra/aws/theseus-hands.yaml with HandImageUri=<the URI from step 0>, then show me its change set.'
theseus --socket $S ask 'Apply that change set.'
```

Expect: both stacks `CREATE_COMPLETE`; the network stack's outputs `NatGateway = disabled`; the hands stack's
`LambdaHandArn` present (it exists only once `HandImageUri` is set). The NAT's internet gateway and Elastic IP are not
created while it is disabled, so no floor approval is asked.

### 2. Read-only first

```bash
theseus --socket $S ask 'With aws.call: ecs DescribeClusters for theseus-hands; lambda GetFunction for theseus-hand-basic; service-quotas GetServiceQuota ServiceCode=fargate QuotaCode=L-3032A538; lambda GetAccountSettings.'
```

Expect: the cluster `ACTIVE`; the function `Active`, `PackageType: Image`, timeout 900; a Fargate vCPU quota and a
Lambda concurrency limit (part 2 reads these before a launch; part 1 does not).

### 3. One Lambda hand

```bash
theseus --socket $S ask 'Run one hand with aws.hands.run: {"argv": ["sh", "-c", "echo hello from hand {index}; uname -a; python3 --version; mkdir -p out && date -u > out/when.txt"]}. Tell me its result when it comes.'
```

Expect, in order:
- the call's line under `[policy.aws] run = notify` (a notice, no card), and its answer at once: `Started a group of 1
  hand on lambda as background group act_…`;
- within about 30 s a late result: `Hands group act_… on lambda: 1 of 1 succeeded, … #0 succeeded exit 0 … s
  s3://theseus-<acct>-<region>/hands/act_…/: Python 3.11.…`;
- `theseus --socket $S ledger --kind aws.hands.launched` shows the group with one hand and an
  `external_op_id` (Lambda's request id); `aws.hands.settled` shows `met: true`; two `aws.called` rows (`Invoke`, and
  the three `DescribeStacks` of the first call);
- `aws s3 ls --recursive s3://theseus-<acct>-<region>/hands/<hand correlation id>/` lists `out/when.txt`,
  `output.txt`, and `result.json`;
- `aws logs tail /theseus/hands --log-stream-names hands/<group>/0` shows `hello from hand 0`.

### 4. One Fargate hand (turns the NAT on for the check, and off after)

```bash
theseus --socket $S ask 'Run one hand with aws.hands.run on Fargate: {"argv": ["sh", "-c", "echo fargate {index}"], "backend": "fargate"}.'
```

Expect first: an error result, `Fargate hands need the hands VPC's NAT, which is off …`, and no `RunTask` in
CloudTrail. Then:

```bash
theseus --socket $S ask 'Plan theseus-hands-network with NatGateway=enabled and show me the change set.'   # asks at the floor: an internet gateway and an Elastic IP
theseus --socket $S ask 'Apply it.'
theseus --socket $S ask 'Run one hand with aws.hands.run on Fargate: {"argv": ["sh", "-c", "echo fargate {index}"], "backend": "fargate"}.'
```

Expect: a background answer; in CloudTrail a `RegisterTaskDefinition` (family `theseus-hand-act_…`) and a `RunTask`
with `startedBy` the group, `assignPublicIp DISABLED`, the private subnets, and the tags `theseus:owner`,
`theseus:execution`, `theseus:session`, `theseus:group`, `theseus:correlation`, `theseus:ttl`, `theseus:deployment`;
then (20 to 60 s) the late result `1 of 1 succeeded`. The ECS task-state events (RUNNING, STOPPED with exit 0) are
taken off the queue and change nothing. Then turn the NAT off again (about $36 a month while on):

```bash
theseus --socket $S ask 'Plan theseus-hands-network with NatGateway=disabled, show me, then apply it.'
```

### 5. A small group, `until: first_success`, `max_usd: 1`

```bash
theseus --socket $S ask 'aws.hands.run: {"argv": ["sh", "-c", "sleep 5; test {index} -ge 5"], "count": 20, "concurrency": 4, "until": "first_success", "max_usd": 1}'
```

Expect: 4 Invokes at once; hands 0 to 3 fail, then the next wave of 4; hand 5 (or the first of 4 to 7 to finish
with index ≥ 5) succeeds; the group settles `met: true` with the hands of later waves `not launched` (cancelled,
never dispatched), and no Invoke after the first success. The worst case at launch is 20 × 600 s × 2 GB, about $0.40,
under `max_usd`. A group with `"max_usd": 0.01` launches fewer hands at a time, and says `max_usd $0.01 leaves no
room…` when none fits.

### 6. A forged completion is quarantined

```bash
aws sqs send-message --queue-url "$(aws cloudformation describe-stacks --stack-name theseus-foundation --query "Stacks[0].Outputs[?OutputKey=='CompletionQueueUrl'].OutputValue" --output text)" \
  --message-body '{"v":1,"correlation_id":"act_forged_example","group":"act_none","index":0,"outcome":"succeeded","started_at_ms":0,"finished_at_ms":0,"producer":"hand:lambda","signature":"hmac-sha256:00"}'
```

…while a group is running (the poller reads only then). Expect: a `completion.quarantined` row, `why: its signature is
not its hand's key's`, and health's quarantined count up by one; nothing settles.

### 7. The kill -9 (part 2's prove; worth a first look now)

Start step 5's group, `kill -9` the scratch daemon's pid mid-group, start it again: the poller finds the open group
from the store and every hand and the group settle once (`action.succeeded` / `.failed` once each hand, one
`aws.hands.settled`). Hands of waves not yet launched at the kill wait for the next completion to launch them.

## What is left, or uncertain

- **Part 2:**
  - cancellation per backend (`StopTask` verified STOPPED; Lambda `cancel_unsupported`), so a group that meets `until`
    stops its running hands;
  - the TTL reaper's act mode;
  - budget reservations, and the price table;
  - quotas before a launch, and waves for a quota;
  - the reconciler's AWS path (`DescribeTasks` for overdue hands: today an overdue hand goes `outcome_unknown` by the
    kernel's generic reconcile);
  - the Observatory's grid, and Discord's group line;
  - the kill -9 prove.
- **Not built:** Batch arrays, a hand whose input is a shard of an S3 prefix (inputs are list items), and the per-group
  notice under `notify` (each call is one notice today, since a group is one call).
- **The full image was not built here** (step 2). The `apt` layer and the AWS CLI's checksum are unproven until the
  maintainer's build.
- **The key's two exposures** (point 11 above).
- **Health** does not yet count running hands by backend, the oldest, or the spend reserved (§3.3's "watching"). It
  does count quarantined completions.
- **Docs the maintainer should write:**
  - `docs/design/aws-toolset.md` §3.3: the role is `theseusd hand`; and the group is a META record plus kernel actions,
    so no new record kind.
  - The spec's Part III item for step 40 part 1.
  - `docs/status.md`.
  - `infra/aws/README.md`'s table: one row pointing at `infra/aws/hand/` (I left that file alone).

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on each commit's tree:

| phase | result |
|---|---|
| fmt, shape, clippy `-D warnings`, bench build, test build, reader rule (9/9) | pass |
| suite | sub-step 3's final tree: 1772 tests, 1739 passed, 33 failed, 10 skipped |
| protocol types (run by hand) | no diff |
| turn bench (run by hand) | 5 frames, budget 5: ok |
| cargo deny, offline with advisories (run by hand) | ok |
| web lint and build (run by hand) | ok, and no change to the committed dist |

Every failure in the suite is one of these, and none is this change's:
- **32 sandbox tests that need a non-root user** (theseus-pv6i): all of `theseus-sandbox::contract`, `bench spawn_100`,
  and `theseusd::sandbox`.
- **`theseus-core tests_output::the_cores_output_matches_its_golden`.** Its only difference is the wake line's UTC
  offset, `+#:#` here against the golden's `-#:#`. This VM runs at UTC, and the golden was written at a negative
  offset. It fails identically on the first commit's tree, which touches nothing it prints. The golden should pin
  `TZ`; that is not this branch's to fix.
- **`theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults`** failed once, in gate 1's suite, and passed
  when run alone. It is not on the flaky list, so it is reported here: this change does not touch the kernel or the
  simulator.

Gate 2's tree also failed `config::tests::example_template_lists_every_tool_under_policy_tools`. It was mine (the new
tool missing from the template's `[policy.tools]`), fixed before the commit, and gate 3 passed it.
