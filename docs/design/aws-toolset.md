# Theseus × AWS: the toolset for an owner-Theseus

_Checked in 2026-09-30 from the design lanes. Scrubbed for this public repository: the Home account's id and alias, its IAM user and the operator's vault item, another project's name and resources, the operator's employer's accounts, usage, and internal tools, and local paths._

*Design, docs only (theseus-mgw.1). Started 2026-09-30 15:18 MST by Tabitha/Claude, in a lane parallel to the
build chain. Nothing here is built or decided until Eddie says yes. It serves his 2026-09-30 theory of the
account: Theseus is its complete, virtual owner, and the only operator hard limits are the budget and a SOC2
stance.*

## Outline (written first, filled in section by section)

0. The answer on one screen
1. What the usage audit says
2. Principles
3. Architecture
   - 3.1 Reach: one generic caller
   - 3.2 Craft: curated tools
   - 3.3 Hands: compute fan-out
   - 3.4 IaC: how infrastructure is made
   - 3.5 Credentials and attribution
   - 3.6 Guardrails: one list, two enforcers
   - 3.7 Budgets, tags, and the reaper
   - 3.8 Observability
   - 3.9 How it meets the gate
   - 3.10 FAST
4. The catalog
5. The build plan
6. Open questions for Eddie, each with its default
7. Appendices: the config draft, names, and the guardrail list

## 0. The answer on one screen

- **Reach: one generic caller, `aws.call`.** It can make any operation of any AWS service. It is a small
  dynamic client of our own: AWS's `aws-sigv4` signer, the workspace's reqwest (ring TLS, no aws-lc), and a
  compact catalog compiled from AWS's official Smithy models. So Theseus reaches every service at one fixed
  cost of a few MB of binary. It needs no per-service crates, and nothing happens at startup. No typed
  `aws-sdk-*` crate goes into the binary. The `aws` CLI stays as the fallback through `proc.run`, now with
  short-lived credentials that name the job.
- **The gate reads AWS from the model.** An operation is Read when it is `@readonly`, uses GET or HEAD, or is
  named Describe, List, Get, Head, and the like. Anything else is Write, and an operation that runs code is Run.
  A table in the crate, with its own tests, adds two flags: cost-bearing and secret-bearing. The posture comes
  from `[policy.aws]`: Read is `open`, Write and Run are `notify`, and the guardrails are the floor.
- **Craft: about fifteen curated tools** where one good tool beats a dozen raw calls. The first ones are
  `aws.whoami`, `aws.describe`, S3 list and get, the Logs query, the stack plan and apply, and the month's cost.
- **Hands: the same static binary, run in AWS.** Lambda takes short jobs, Fargate long ones, Batch arrays take
  a hundred at once, and SSM operates hosts. One SQS queue brings every completion home (§3.16). Every hand
  is tagged and has a TTL. AWS-side, a reaper stops any hand past its TTL even when Theseus is down. A hand's
  estimated cost is reserved against its session's dollar budget.
- **IaC by construction.** Durable infrastructure is made only through CloudFormation change sets, which a
  deployer role applies. Theseus sees each change set before it runs. Theseus's everyday sessions carry a
  guard it applies to itself: they cannot create durable infrastructure directly. Operations stay direct:
  runs, data, and starting or stopping what exists.
- **Credentials and attribution.** The IAM user key in the vault signs only `sts:AssumeRole`, into a
  full-admin `theseus-owner` role: the owner's own hat, not a limit. Each execution gets a session whose name
  is its execution id, so CloudTrail names the execution on every call, and the user agent names the call.
  Jobs get narrower sessions, and hands narrower roles, of Theseus's own choosing.
- **Guardrails: one list, two enforcers.** The list covers public ingress, the audit trail, the budget, and
  long-lived credentials. The gate's floor asks before such a call runs. AWS then refuses what was not approved:
  through the session guard today, and through SCPs if Eddie adopts the Organization (to be renewed with him; not
  decided here).
- **The first slice (the tracer bullet):** a bound account, with `aws.call` for reads on every service,
  `aws.describe`, `aws.whoami`, and `aws.s3.list`, all checked live with read-only calls. Writes arrive in the
  second slice, together with the owner role and the guards. Three parallel slices (the catalog, the
  guardrails, and the templates) can start today, each in its own crate or directory and worktree. The client
  follows the catalog, and the core wiring lands on `main` at roadmap step 14.
- **What Eddie decides:** thirteen questions in §6, each with the default the build takes if he doesn't
  answer.

## 1. What the usage audit says

The Daily Driver audit (theseus-p3k's usage report) counted the `aws` CLI in 139 of 9,421 shell
calls over 30 days (1.5%), 134 of them in the DM. It did not break them down, so I did, from the same Claude
CLI transcripts and the same window, with a script that prints only service and operation names
(a scratch script, not checked in). It found 144 shell calls holding 257 `aws <service> <operation>`
invocations, across 16 services.

| # | operation | calls | class |
|---:|---|---:|---|
| 1 | `s3 ls` | 50 | Read |
| 2 | an operation of the employer-side service, on the operator's employer's accounts | 45 | Read |
| 3 | `s3 cp` | 24 | Read or Write (both directions) |
| 4 | `cloudformation describe-stacks` | 13 | Read |
| 5 | `sts get-caller-identity` | 11 | Read |
| 6 | `application-autoscaling describe-scalable-targets` | 11 | Read |
| 7 | `logs filter-log-events` | 10 | Read |
| 8 | `service-quotas list-service-quotas` | 6 | Read |
| 9 | `ec2 describe-instances` | 6 | Read |
| 10 | `cloudformation describe-stack-events` | 6 | Read |
| 11+ | thirteen more at 3–4 calls each: Logs streams, events, and `tail`; quota change history; IAM policy reads and simulation; more reads of the employer-side service; ECR images; SSM commands; ECS clusters; `s3api list-objects-v2` | 3–4 each | Read |

By service: S3 80 (with `s3api`), the employer-side service 54, Logs 22, CloudFormation 20, Service Quotas 15, STS 11,
Application Auto Scaling 11, IAM 8, EC2 7, SSM 6, SSO admin 6, ECR 3, ECS 3. There were eleven local CLI
commands too (`configure`, `sso login`).

What it says:
- **Reads dominate.** 216 of the 257 (84%) are reads. The rest are `s3 cp` and `s3 sync` (25), two quota
  increase requests, one `ec2 start-instances`, one permission-set provisioning, and one `delete-stack`.
- **The long tail is real.** The second most frequent operation (the employer-side service's) and the sixth
  (Application Auto Scaling) are on no one's hand-picked list. Sixteen services appeared in 30 days. The 2026-09-29
  sketch's curated list (STS, S3, Logs, SSM, Secrets Manager, ECS, Lambda, EC2 describes, cost) would have
  sent 118 of the 246 API calls (48%) back to the shell: the employer-side service, CloudFormation, Service Quotas, Auto
  Scaling, IAM, SSO admin, ECR, and an EC2 start. That is the case for the generic caller.
- **This is today's usage, not an owner's.** Nothing of Theseus's lived in the Home account yet. An
  owner-Theseus with hands will write far more: runs, stacks, and objects. The audit ranks the read side, and
  says nothing about hands.
- **Much of it is the employer's, not Home.** The employer-side service, SSO admin, and `sso login` point at the operator's employer's
  accounts, through profiles. Whether Theseus should reach them is question 9 in §6.

## 2. Principles

1. **The owner model.** Theseus owns the Home account (`<home-account-id>`). It is not a limited user.
   The limits it puts on its own hands (session policies, narrower roles) are its own choices, recorded and
   visible, never the operator's. The toolset asks the operator nothing about ordinary owner work (new
   roles, policies, services) beyond the postures the operator sets. The account also holds another project's resources, which
   Theseus owns too and shares with Eddie. So nothing automatic, the reaper included, ever touches a
   resource Theseus did not tag, and a write to anything outside Theseus's own inventory posts a notice.
2. **Two hard limits, honored from inside the account.**
   - **The budget.** It is an AWS Budget reconciled from the config: authoritative, and AWS-side. Its alerts
     come home as events, and an action fires at 100% (§3.7). Every hand carries a dollar reservation
     (§3.3).
   - **The SOC2 stance**, as the operator's employer practices it:
     - no public ingress;
     - an intact audit trail;
     - IaC for durable infrastructure;
     - no long-lived credentials;
     - encryption at rest.
   - **Honest about where.** Inside the account, an owner can undo any limit set there. The toolset makes the
     limits the default path, visible and asked about. Only SCPs from an Organization that Eddie alone holds
     make them hard. That choice is his, and is renewed with him before step 14's brief (§6, question 1).
3. **IaC for anything durable.**
   - Durable infrastructure is created and changed only through CloudFormation change sets. Each is shown
     before it applies, applied by a deployer role, and kept in git.
   - Direct API calls stay for reads, and for operational actions on what exists: runs, data, start and
     stop, scaling.
4. **Attribution.** Every AWS call traces to its Theseus execution through CloudTrail:
   - the session name is the execution id;
   - the source identity is the Theseus deployment;
   - the user agent carries the call's correlation id.

   Every call's ledger row keeps the AWS request id, which is CloudTrail's `requestID`, so the join is exact.
5. **FAST.** Nothing AWS runs on the start path.
   - The model catalog decodes lazily, one service at a time.
   - The account check, credential minting, SQS polling, the budget's reconcile, and the inventory all run
     after serving.
   - The whole AWS layer costs at most 5 MB of the 60 MB binary budget.
6. **Typed over opaque** (§3.23). An AWS call is typed (service, operation, input), so the gate reads its
   intent without parsing a shell. The CLI path stays for what the caller cannot do yet, and its share of AWS
   calls is the promotion queue.
7. **Notify over block** (§3.9). The gate never refuses an AWS call. AWS may refuse one (the session guard,
   and SCPs), and the result says which and why.
8. **Latest.** The "latest SDK" of 2026-09-29 becomes the latest models. Each week an updater regenerates the
   catalog from AWS's newest Smithy models and bumps `aws-sigv4`. It runs the protocol tests and the gate,
   and commits if they pass. Old API behavior is not supported.
9. **Tracer bullet.** The first slice is one thin path: a bound account, and `aws.call` doing reads end to end
   through the CLI, Discord, and the web UI, checked live with read-only calls. Everything else is filed.

## 3. Architecture

Four layers, bottom up: **reach** (one caller for every operation), **craft** (curated tools built on it),
**hands** (compute fan-out), and **IaC** (how durable infrastructure is made). Across them run
**credentials, guardrails, budgets, and observability**. The code lives in three new crates (the catalog,
the client, and the guardrails; §5), plus wiring in `theseus-core`. None of it is active when the config has
no `[aws]`, as desktop mode requires.

### 3.1 Reach: one generic caller

The Rust SDK is typed at compile time and has no botocore-style "call any operation from JSON". Three ways
to get whole-of-AWS reach:

| | A. Typed SDK crates plus generated glue | **B. A dynamic client over `aws-sigv4`, driven by the models** | C. The `aws` CLI through `proc.run` |
|---|---|---|---|
| Reach | the services compiled in | every service in the models | every service |
| Build | an `aws-sdk-<svc>` crate per service, plus per-operation JSON glue (SDK types have no serde) | one crate: the protocol layer, the catalog, and the protocol tests | none |
| Binary | heavy per service (EC2's crate is among Rust's largest); dozens would break 60 MB | about 1 MB of code plus a 2–4 MB catalog (an estimate; gated) | none in ours (a Python install beside it) |
| Per call | fast | fast | a Python start, hundreds of ms |
| What the gate reads | typed input | typed input: service, operation, input | argv only |
| Credentials | per client | per call, from the core | environment variables per job |
| Eddie's "the SDK, not the CLI" | to the letter | in spirit: AWS's own signer and models, in native Rust | no |

**Recommendation: B**, with C kept as the fallback.
- **Reach is the point.** Eddie wants "the broadest reach possible". The audit's second most frequent
  operation is on no curated list.
- **The model's input is JSON anyway.** Typed crates buy compile-time types that the model can't use, and
  they still need generated glue for every operation to get from JSON to a builder.
- **The cost is fixed.** A new service costs a regenerated catalog, not a crate, and the 60 MB budget holds.
- **It is AWS's own machinery, minus the code generation.** The signer is `aws-sigv4`, the one the SDK
  signs with. The catalog is compiled from the Smithy models the SDK is generated from, and XML is read with
  AWS's `aws-smithy-xml`.
- **It is held to the SDK's standard.** Smithy publishes the AWS protocol compliance tests as models, and the
  SDKs run the same ones. They drive this crate's tests.
- **The SDK is heading this way.** `aws-smithy-schema` 0.2, already a dependency of `aws-sdk-sts` 1.118 in
  the local registry, brings runtime schemas and pluggable protocols ("third parties can create custom
  protocols ... without modifying a code generator"). It still builds its schemas at compile time. When it
  stabilizes it may replace our protocol layer, so the slice that builds the client watches it.

**What is in the catalog (`theseus-aws-catalog`) and the client (`theseus-aws`):**
- **The catalog.** An offline generator (an `xtask`, not a `build.rs`) compiles a pinned snapshot of AWS's
  Smithy models. For each service it keeps:
  - the id, the endpoint prefix, the signing name, and the protocols;
  - each operation's HTTP binding and its input and output shapes: members, types, locations, XML names,
    timestamp formats, and what is required;
  - the traits that matter: `readonly`, `idempotent`, `idempotencyToken`, `paginated`,
    `httpChecksumRequired`, `streaming`, `eventStream`, and `deprecated`.

  The documentation is stripped: the model knows AWS, and the shape gives the exact names. The catalog is
  stored compact and compressed with a pure-Rust codec, embedded with `include_bytes!`, and one service is
  decoded on its first use. Its size is a gate check. If it outgrows the budget, it ships beside the binary
  as a pinned artifact, like the embedding weights (§6 of [the spec](../the-ship-of-theseus.md)).
- **Protocols:** `awsJson1_0`, `awsJson1_1`, `restJson1`, `awsQuery`, `ec2Query`, and `restXml` (S3,
  Route 53, CloudFront). These cover every service in the audit.
  - `rpcv2Cbor` comes when a service needs it.
  - Event streams (Logs Live Tail, Bedrock streaming) stay on the CLI until then.
- **Endpoints:** `{prefix}.{region}.amazonaws.com` from a partition table, plus the global and fixed-region
  ones (IAM, Organizations, Route 53, CloudFront; Budgets and Cost Explorer in us-east-1), and S3's
  virtual-hosted style. It does not implement the full endpoint rules engine; FIPS and dual-stack aren't
  needed.
- **Signing:** `aws-sigv4`, which is pure Rust (hmac and sha2, with no ring and no aws-lc), presigning
  included. SigV4a is not needed.
- **Transport:** the workspace's reqwest 0.12 on ring rustls. There is no second HTTP stack, and aws-lc
  stays out of every build, as it has been since 6ad89a2.
- **Pagination:** an operation with `paginated` follows its tokens for up to the `pages` the call asks for,
  within the result cap.
- **Errors** are parsed per protocol into a code, a message, the request id, and whether a retry could help.
  The model sees AWS's words exactly. When AWS names the enforcer ("explicit deny in a session policy", "a
  service control policy"), the result says which guard it was (§3.6).
- **Attribution:** every request's user agent carries `theseus/<version> exec/<execution id>
  call/<correlation id>`, and CloudTrail records it.
- **Results:** JSON (XML is converted through the output shape), capped. A large result becomes a node that
  the model reads by range (§3.16, references rather than payloads).

**What the plan derives from the model.** `plan` computes this locally, with no network, and the gate reads it:
- **Class.**
  - **Read:** the operation carries `@readonly`; or its HTTP method is GET or HEAD; or its name starts with
    `Describe`, `List`, `Get`, `Head`, `Lookup`, `Search`, `Query`, `Scan`, `BatchGet`, `Simulate`,
    `Validate`, or `Estimate`.
  - **Run:** the operation starts code of the caller's choosing. This is a short named list: Lambda
    `Invoke`, ECS `RunTask` and `StartTask`, Batch `SubmitJob`, SSM `SendCommand` and
    `StartAutomationExecution`, CodeBuild `StartBuild`, Step Functions `StartExecution`, and the SageMaker job
    starts.
  - **Write:** everything else.
  - **Overrides, in a tested table.** For example:
    - SQS `ReceiveMessage` is Write, since it hides the message from other consumers;
    - STS `GetSessionToken` and `GetFederationToken` are Write and secret-bearing, since they mint
      credentials, whatever their names say;
    - Athena `StartQueryExecution` is Write and cost-bearing, since it writes its results to S3;
    - CloudFormation `CreateChangeSet` is Write but harmless: it changes nothing until executed.
- **Cost-bearing.** A tested table flags three kinds of operation:
  - the Run operations;
  - creations and starts of metered resources;
  - operations charged per request: Cost Explorer ($0.01 a call), Athena (per TB scanned), and Logs Insights
    (per GB scanned).

  The flag drives notices and budget reservations. By itself it never makes a call wait.
- **Secret-bearing.** A tested table flags operations whose results hold credentials or secret values:
  - Secrets Manager `GetSecretValue`;
  - SSM `GetParameter(s)` with decryption;
  - KMS `Decrypt` and `GenerateDataKey`;
  - every STS credential mint;
  - ECR and CodeArtifact `GetAuthorizationToken`;
  - IAM `CreateAccessKey`;
  - SSO `GetRoleCredentials`.

  The value never enters the context. The tool returns a broker handle and the value's shape with the
  secret masked, and the scrubber learns the value (§3.5).
- **Retry class** (§3.16), from the traits:
  - `@readonly` or `@idempotent` is `safe_to_repeat`.
  - A member marked `@idempotencyToken` is filled from the call's correlation id, which makes the call
    `idempotent_with_key`. That is a new variant of `theseus_tools::Retry`; the spec already names it.
  - Anything else is `non_repeatable`. It is retried only on an error that proves the request did not run: a
    throttle, or a connection that failed before the request was sent. A timeout after sending makes the
    call `outcome_unknown`. The reconciler resolves it through the matching Describe, or the principal is
    told.
  - Generic SDK-style retry never runs, since it would contradict the class.
- **Resources.** ARNs, names, and ids that the input's shape names (members ending in `Arn`, `Name`, or
  `Id`, and S3 bucket and key). The gate checks them against Theseus's inventory (§3.7), and the ledger keeps
  them.

**`aws.describe` is the model's lens on the catalog.**
- With no argument, it lists the services.
- With a service, it lists that service's operations, each with its class.
- With an operation, it gives the input shape as a compact JSON schema.

It is local, answers in microseconds, and is Read and `open`. So the tool list carries one `aws.call`
schema, not thousands of operations. Tool count stays the toolchain manager's problem (§3.23).

**The fallback, `proc.run aws …`, stays.** It is needed for event streams, `ssm start-session`, `ecs
execute-command`, and anything the caller cannot do yet. It gets a per-job session named by the job's
correlation id, never the root key (§3.5). Its share of AWS calls is watched like the shell-fallback ratio,
and its most frequent operations are the next curated tools.

**Cost to FAST:**
- **Start:** nothing. No catalog is decoded, no client is built, and no network is touched until the first
  AWS call.
- **Binary:** about 1 MB of code, plus the catalog, estimated at 2–4 MB compressed. Slice P1 measures it,
  and the gate holds it at 4 MB.
- **Compile:** one mid-sized crate plus about a dozen small dependencies of `aws-sigv4`, with no TLS
  stack. That is tens of seconds from clean. Slice P2 counts them with `cargo tree`.

### 3.2 Craft: curated tools

A curated tool earns its place when one well-made call beats several raw ones:
- it paginates for you;
- it understands `s3://` paths;
- it waits on an asynchronous API (Logs Insights, Athena, change sets) until the answer is ready;
- it summarizes, so a large result doesn't flood the context;
- it keeps secrets out of the context;
- it adds a safety property: a dry run, a diff, a cap.

Every curated tool is a thin recipe over the same caller. So signing, retry classes, spans, ledger rows, and
attribution are written once. All are `Backend::Async` (DD5's pattern): futures on the daemon's runtime
that hold no core while they wait.

Ranked by the audit's frequency first, then by power for an owner:

| rank | tool | what it does | the audit's calls it covers | verdict |
|---:|---|---|---:|---|
| 1 | `aws.s3.list` | buckets, or objects under an `s3://` prefix; folders by delimiter; paginates up to a cap; a summary (count, bytes, newest) | 55 | **build, slice C1** |
| 2 | `aws.whoami` | the bound account, region, session name (the execution), session expiry, the role, and the budget line | 11 | **build, slice C1** |
| 3 | `aws.describe` | the catalog's services, operations, and input shapes (§3.1) | n/a, but it makes all 257 possible | **build, slice C1** |
| 4 | `aws.logs.query` | Logs Insights: start, wait, and return a table node, with bytes scanned and cost; groups by name or prefix; relative times ("2h") | 22 (all Logs) | **build, slice C3** |
| 5 | `aws.logs.tail` | recent lines of a group, filtered by stream prefix and pattern, optionally followed for up to N seconds by polling | (with the above) | **build, slice C3** |
| 6 | `aws.s3.get` | an object to a workspace file (streamed to disk, checksum checked) or to a text node (capped); byte ranges | 24 (`s3 cp`, both directions) | **build, slice C3** |
| 7 | `aws.stack.plan`, `.apply`, `.status` | the IaC path (§3.4): a change set as a readable diff, applied on its digest, with events until settled | 20 (CloudFormation) | **build, slice C2** |
| 8 | `aws.cost` | month to date and forecast (free, from AWS Budgets), with a by-service or by-tag breakdown (Cost Explorer, $0.01 a call, cached a day) | 0; it is the budget line | **build, slice C2** |
| 9 | `aws.s3.put` | a workspace file to S3; multipart above 64 MB, with parts sent in parallel; tags; encryption by the bucket's default | (with `s3 cp`) | build, slice C3 |
| 10 | `aws.hands.run` | fan-out compute (§3.3) | 0; it is new power | build, step 40 |
| 11 | `aws.ssm.run` | Run Command on instances chosen by id or tag, as a hand (A3), with its output returned | 6 (SSM) | build, step 40 |
| 12 | `aws.trail` | CloudTrail event history by execution, by resource, or by event name | 0; the owner's audit (§3.8) | build, slice C3 |
| 13 | `aws.s3.sync` | a directory to or from a prefix, by size and ETag; a dry run first; deletes only when asked, and then it waits | 1 | later |
| 14 | `aws.athena.query` | start, wait, and return a table node, in a Theseus workgroup whose per-query scan cap is set by IaC | 0 | later |
| 15 | `aws.s3.presign` | a GET or PUT URL, expiring no later than the session that signs it (12 h at most) | 0 | later |
| 16 | `aws.lambda.invoke` | invoke an existing function; the error and the 4 KB log tail decoded | 0 | later; `aws.call` covers it until then |

Judged and not built as tools:
- **STS assume.** Narrowing is an argument of whatever launches the work: `aws_policy` on `proc.run`, and a
  role `profile` on `aws.hands.run`. It is not a tool of its own, and the credentials never reach the model
  (§3.5).
- **ECS run-task.** It is a backend of `aws.hands.run`, not a tool of its own.
- **SSM parameters and Service Quotas.** `aws.call` covers them. SecureString values come back as broker
  handles through the secret-bearing rule. The hands manager reads the quotas it needs itself.
- **Hands' status and cancellation.** A fan-out is one tool call. Cancelling it is the execution's cancel
  (§3.16), because the kernel is not a tool (§3.24). A long fan-out runs in a task session (DD7), so the
  parent keeps talking.

### 3.3 Hands: compute fan-out

_As built (step 40, part 1, 2026-10-04, theseus-mgw.6; Part III Item 107): the role is `theseusd hand`, not `theseus hand`: helper roles live in `theseusd`, and the CLI links only `theseus-protocol`, while a hand needs the AWS client. A group is no new record kind: the call's action, a META record `aws.hands.group.<group>`, and one kernel action per hand (tool `aws.hand`), so settling is the kernel's `accept_completion`, with its dedupe, quarantine and reconciler. Where hands run is discovered from the stacks' outputs (`DescribeStacks`, once per account and region), not configured. A hand's key is HKDF-SHA256 of the account's secret access key with the correlation id, stored nowhere; its envelope's HMAC is checked in constant time, and a bad one is quarantined. Fargate is refused while the hands network's NAT is off. Part 2 (cancellation per backend, the TTL reaper's act mode, reservations, quotas) is Part III Item 116._

**A hand is a job that runs in AWS.** It keeps the job wrapper's contract (§3.16): detached, durable, and
cancellable.
- **The same static binary is the wrapper.** The hand runs `theseus` in a `hand` role (§1: helper processes
  are the same binary with a role flag), which:
  - runs the job's argv under its own deadline;
  - streams its output to CloudWatch Logs (`hands/<group>/<index>`);
  - uploads the result (the exit code, the output's tail, and the files under `out/`) to
    `s3://theseus-<account>-<region>/hands/<correlation id>/`;
  - sends the completion envelope to the queue.
- **Any image can be wrapped** on Fargate. An init container copies the static binary into a shared volume,
  and the main container runs `theseus hand -- <argv>` (ECS container dependencies).
- **The hand image** is the Theseus image in Theseus's own ECR repository: a slim base plus the binary, git,
  Python, and the common CLIs. For Lambda, the same image serves, with the binary as the runtime's bootstrap.

**Backends (the spec's A classes):**

| backend | start | limits | best for | completion | cancel |
|---|---|---|---|---|---|
| **A2 Lambda** (container image, no VPC) | about 1 s warm, seconds cold | 15 min, 10 GB memory, 10 GB of `/tmp` | short, stateless, many at once | the wrapper; failures also through the async-invoke failure destination | `cancel_unsupported`; its timeout is the bound |
| **A1 Fargate** (ECS `RunTask`) | 20–60 s | hours; up to 16 vCPU and 120 GB | long or heavy one-offs | the wrapper; EventBridge's task state change | `StopTask`, verified when the task shows STOPPED |
| **A1 Batch arrays** (Fargate or Spot) | queue, then 20–60 s | arrays of up to 10,000 children | a hundred hands of one kind | per child, the wrapper; EventBridge's job state change | `TerminateJob` on the parent ends its children |
| **A3 SSM Run Command** | seconds | the host's | operating a specific host | EventBridge's command status change, with the output in S3 | `CancelCommand` |
| **A4 dev box** | seconds when running | its own | curated environments | through SSM, as A3 | as A3 |

Until Jev chooses (M5, `shell.v1`), the tool chooses: a job that fits in 10 minutes and 10 GB goes to Lambda;
a larger one goes to Fargate; more than ten of one kind go to a Batch array. The model may name a backend.

**The fan-out tool.** `aws.hands.run` takes:
- the work: `count` or a list of `inputs`, an `argv`, and an optional `image`;
- the size: vCPU and memory;
- the limits: `ttl` and `max_usd`;
- when to finish: `until` (`all`, `first_success`, or a quorum);
- optionally the hands' role `profile` (§3.5; `theseus-hand-basic` by default), and a `backend`.

One call writes one group record and N job records, each with its correlation id, in one WAL frame. Each hand
gets its index and its input (a list item, or one shard of an S3 prefix). When `until` is met or the deadline
passes, the rest are cancelled, and the call's result is the aggregate: each hand's outcome, exit code,
duration, cost, and result reference.

**Tags on every hand:**
- `theseus:execution`, `theseus:session`, `theseus:group`, `theseus:correlation`, `theseus:ttl` (a UTC
  time), and `theseus:deployment`, with `propagateTags` on;
- on ECS, `startedBy` is the group id, so one filter lists or stops a group.

**TTL, in three layers**, so that no layer is the only limit:
1. The wrapper's own deadline, as for every job.
2. The backend's own limit: Lambda's timeout, Batch's attempt duration, SSM's execution timeout.
3. An AWS-side reaper: a small Lambda on a 15-minute EventBridge schedule, part of the hands stack, stops any
   task or job whose `theseus:ttl` has passed. With Theseus off and a wrapper wedged, a hand outlives its TTL
   by at most 15 minutes.

**Budget per hand.**
- At launch, the manager estimates the group's worst case: count × TTL × the backend's rate for its size and
  region, from a price table in the crate that the weekly updater refreshes.
- It reserves that against the session's dollar budget (§3.13), the one that already asks at its limit, and
  settles it at completion from the hands' real durations (question 6).
- `max_usd` caps the group: past it, no hand launches and the running ones are cancelled. _(As built, step 40 part 2: each hand's action reserves its worst case, its TTL at its size's rate, in the group's one frame, and settles at its envelope's cost; a group whose total worst case passes what the session has left is not run, and asks the session's budget question. Quotas are read once an hour per account, region and backend, and cap each group by itself.)_
- Before launch the manager reads the Fargate vCPU and Lambda concurrency quotas (Service Quotas, cached). A
  group bigger than the quota launches in waves or queues in Batch. It never fails for a quota.

**Watching a hundred hands.**
- The Observatory shows one row per group: a grid of N cells (queued, running, succeeded, failed, unknown),
  the cost so far against the cap, and the slowest hands. A cell opens that hand's log tail. _(As built, step 40 part 2, 2026-10-04; Part III Item 116: the cockpit's Systems view, a `HandsGrid` panel reading `hands.list` every 3 s while open, a row per group with a cell per hand (waiting, running, stopping, succeeded, failed, unknown, cancelled, not launched) and spend against the cap; no log tail per cell yet, and no slowest-hands column.)_
- Discord shows one tool line per group, edited in place ("🖐️ 37/100 done, 2 failed, $1.84 of $5"), never
  a hundred lines. That is DD3's lesson on notice volume.
- Under `notify` a group posts one notice, not one per hand.
- Health counts the hands running by backend, the oldest, and the spend reserved.

**Completions come home over SQS (§3.16).**
- There is one standard queue per bound account and region, `theseus-completions`: encrypted, keeping
  messages for 14 days, with a dead-letter queue. It is fed by:
  - the hands' wrappers;
  - EventBridge rules for ECS task, Batch job, SSM command, and Step Functions state changes, filtered to
    Theseus's cluster, queues, and `startedBy` prefix;
  - the budget's alerts (§3.7).
- **The poller** long-polls (20 s) after serving, and only while an AWS action is outstanding; otherwise the
  heartbeat's reconciler covers strays. _(As built, step 40 part 2; Part III Item 116: the heartbeat's reconciler leaves hands to their own pass in the poller, which asks ECS (`DescribeTasks`) about a Fargate hand past its deadline: a task the TTL reaper stopped settles failed with the reaper's reason, and a Lambda hand is unknown until its envelope. The reaper's own failure records are read from the queue and counted in health.)_ While hands run, that is three requests a minute: pennies a month.
- **Every message is checked.**
  - A completion is deduplicated by its correlation id, since settling is idempotent.
  - The wrapper's envelope carries the `signature` field that §3.16 already defines. It is an HMAC keyed per
    dispatch. The key is derived with HKDF from the vault's AWS secret and the correlation id, and handed to
    the hand at launch. So no new secret exists and nothing is written. A completion signed before the key
    was rotated is settled by the reconciler instead.
  - A message that fails either check is quarantined and surfaced, as §3.16 says, never inferred.
- **Settling uses the existing path:** atomic with the continuation, under the execution's lock. A result
  bigger than the cap stays in S3 as the `result_ref`.
- **The reconciler** asks the service only about overdue records (`DescribeTasks`, `DescribeJobs`,
  `GetCommandInvocation`). Events come first, so its cost scales with stuck work, not with all work.

**Network, without public ingress.**
- Lambda hands run outside any VPC: they have egress, and nothing can reach them.
- Fargate and Batch hands run in private subnets of the hands VPC, with no public IP. S3 is reached through a
  gateway endpoint, which is free.
- Internet egress goes through a NAT gateway, which costs about $36 a month when it is always on. By default
  the NAT is a parameter of the network stack: Theseus turns it on with a stack update (about two minutes)
  when a hand needs egress, and off after an idle hour (question 5).
- _Since 2026-10-04 (theseus-mgw.9; Part III Item 134): **an existing VPC.** `[aws.accounts.<id>.hands_network]` names a VPC, its private subnets and, optionally, a security group. The network stack then makes only the hands' tagged group, with no ingress, in that VPC: none of the VPC's own parts and no NAT (its `Rules` refuse a NAT beside an existing VPC, and subnets or a group without one), and its outputs keep their keys (`NatGateway` reads `existing`). `aws.stack.plan` fills the stack's `Existing*` parameters from the config and refuses any other value, or the NAT, before anything is sent. A Fargate launch first reads the VPC's route tables: each subnet's table must send `0.0.0.0/0` to a NAT gateway whose route is not a blackhole, or the launch is refused naming the subnet, and the words never suggest a NAT. No S3 endpoint and no flow logs there (an endpoint would change the other project's route tables), so S3 and ECR pulls pay the existing NAT's data charge, about $0.045 a GB._

### 3.4 IaC: how infrastructure is made

**The engine is CloudFormation, used directly.**
- Templates are YAML, which the model writes fluently, and no Node toolchain sits on the path.
- CDK is an optional front end for authoring. A job runs `cdk synth`, and the template it produces takes the
  same change-set path. Theseus never runs `cdk deploy`, which would skip the review.
- The operator's employer's IaC tools also end in CloudFormation, so Theseus's stacks read like the employer's.

**The source is in git.** Each stack's template and parameters live in a git repository (question 12), and
are committed before the change set is made. The change set's description carries the commit and the
execution id, so the path from a resource back to the model's turn has no gap.

**The path, as tools:**
1. **`aws.stack.plan { stack, template, parameters }`** (Write, `open`):
   - it validates the template, and runs the guardrail scan over its resources (§3.6);
   - it commits the template, uploads it to Theseus's bucket, and creates a change set under the deployer
     role;
   - it waits for the change set, then returns a readable diff node. Each resource is shown as added,
     modified (with the properties that change), replaced (flagged), or removed. Metered resources that are
     added (a NAT, instances, a cluster) get a monthly estimate from the price table. The node also holds the
     scan's findings and the change set's digest.
2. **`aws.stack.apply { stack, change_set_digest }`** (Write, `notify`):
   - it executes exactly that change set; a confirm binds to its digest (§3.9: a confirm authorizes one
     exact action);
   - the stack's events stream to the tool line until it settles, and a failure rolls back as CloudFormation
     does;
   - it waits for approval when the diff replaces or removes a stateful resource (a bucket, table, database,
     file system, key, or log group), and at the floor when the scan found a guardrail.
3. **`aws.stack.status`** (Read, `open`): the stacks, their status, recent events, outputs, and drift.
4. **Drift.** `DetectStackDrift` runs weekly on every Theseus stack (a tender, after serving) and on demand.
   Drift is reported in health and the Observatory, and never reverted automatically: a fix is a new change
   set.
5. **`aws.stack.delete`** waits for approval, and lists what the stack keeps (`DeletionPolicy: Retain`).

**Stacks carry the Theseus tags** (`theseus:stack`, `theseus:execution`, `theseus:owner`, and `theseus:ttl`
for experiments). CloudFormation copies stack tags onto every resource that takes tags, so the reaper and
the cost reports see them. The foundation stacks have termination protection, and a stack policy that
forbids replacing or deleting the trail, its bucket, and its key.

**The deployer role**, `theseus-cfn-deployer`, is CloudFormation's service role. It has administrator access,
and only `cloudformation.amazonaws.com` can assume it. Theseus's sessions may pass it to CloudFormation and
to nothing else (`iam:PassedToService`).

**What is direct, and what goes through a stack.** The rule: anything with a standing cost or a standing
exposure goes through a stack. A run, a piece of data, or an operation on something that exists is direct.

| direct | through a stack |
|---|---|
| reads of anything | VPCs, subnets, gateways, NAT, endpoints, security groups, routes |
| data: objects, items, messages, log events, metrics, parameters | buckets, tables, queues, topics, streams, log groups |
| runs: tasks, jobs, invocations, commands, queries, builds, executions | functions, clusters, services, instances (A4), compute environments |
| operations on what exists: start, stop, scale, redeploy, invalidate, rotate | IAM roles, policies, users; KMS keys |
| tags; change sets; ECS task definitions for runs | load balancers, APIs, distributions, DNS zones and records, certificates |
| | alarms, event rules, schedules, budgets |

**How it is enforced: by construction, not by reading intent.**
- `aws.call`'s plan knows from the guardrail list (§3.6) which operations are IaC-only. It returns such a call
  to the model as invalid input: "CreateBucket makes durable infrastructure; use `aws.stack.plan` (IaC is an
  operator limit)". That is the toollet's own validation (§3.9), not a refusal by the gate, and no request is
  sent.
- Everything that is not `aws.call` (a CLI job, a hand's own process) is covered by the IaC guard.
  Theseus's sessions carry it: a deny-only managed policy listing those same actions. The hand roles'
  boundary includes it too. AWS refuses those actions, and says it was the guard.
- CloudFormation's deployer role is not guarded. So the stack is the one way durable infrastructure changes.
- **The other project's resources predate this, and are not in stacks.** Theseus changes them through CloudFormation
  resource import (a change set of type `IMPORT`), which brings them under IaC first. That is a project to do
  with Eddie, never automatic (question 13).

### 3.5 Credentials and attribution

**The root of trust stays in the core.**
- It is the vault's `<vault reference>`: the IAM user `<iam-user>`, an administrator.
- It resolves into the secrets board after serving, like every secret. It is granted only to the core's AWS
  credential service (`AwsCreds`), a daemon consumer like the providers. No tool, program, or job ever gets
  it.
- Once the foundation stack exists, it signs exactly two operations: `sts:GetCallerIdentity`, for the
  account check, and `sts:AssumeRole`, into `theseus-owner`. Before that, slice C1 signs its reads with the
  key directly, and the bootstrap signs the foundation's creation (§5).
- A CloudTrail metric filter in the posture stack raises an alarm if the key signs anything else.
- Health shows the key's age and warns at 90 days. Rotating it is Eddie's job, since the vault is read-only to
  Theseus.

**The owner role, `theseus-owner`.**
- It has administrator access, and only the `<iam-user>` user can assume it.
- Its sessions last up to 12 hours, and its trust policy allows setting a source identity and tagging
  sessions.
- It is not a limited identity. It is the owner's own hat, and it exists because an IAM user cannot stamp a
  name on a session, while a role session can (question 2).

**Sessions.** `AwsCreds` mints them, caches them, and refreshes each five minutes before it expires. They are
held in zeroizing memory and never written.

| session | its name | its session policies | lifetime | used by |
|---|---|---|---|---|
| **work** | the execution id (`exe_…`: 36 characters of the 64 allowed) | the guards, plus allow-all | up to 12 h, refreshed | every `aws.*` call of that execution |
| **job** | the job's correlation id | the guards, plus the narrowing Theseus chose (`aws_policy`), or allow-all when it chose none | the job's deadline, at most 12 h | `proc.run aws …`, and L1 jobs granted AWS |
| **floor** | `<execution id>.floor` | allow-all, with no guard | 15 min; the core makes the one approved call with it, and nothing else holds it | a floor call the operator approved (§3.6) |
| **tender** | `theseus-<tender>` | the tender's own narrow policy | up to 12 h | durability, the reaper, the budget |

- Every session's source identity is the deployment (for example `theseus-eddie-desktop`). A source
  identity survives role chaining, so it marks "which Theseus" even deep inside hands.
- Every session is tagged with `theseus:execution`, `theseus:session`, and `theseus:deployment`. So a policy
  can require anything created in the session to carry the session's execution tag (`aws:RequestTag` must
  equal `aws:PrincipalTag`).
- **The guards are deny-only managed policies** made by the foundation stack: `theseus-guard-iac` and
  `theseus-guard-limits` (§3.6).
  - A session's allows come from exactly one allow policy: `theseus-allow-all` for a work session, or the
    inline narrowing for a job.
  - That arrangement is correct however AWS combines several session policies. Slice C2's live check
    confirms it against real denials.

**A hand in AWS runs under its role.**
- ECS and Lambda assume a hand's role themselves, so Theseus cannot put a session policy on it. The boundary
  is the role.
- The hands stack defines role profiles, and Theseus picks one per hand:
  - `theseus-hand-basic`, the default: its own S3 prefix, its logs, the completion queue, and pulling images;
  - `theseus-hand-read`, which adds read access to the account;
  - `theseus-hand-owner`, which is allow-all under the guards.

  Theseus adds profiles as it sees fit, through the stack.
- Attribution still holds, through the dispatch record, which keeps each hand's task ARN or command id.
  ECS names a task role's session by the task id, and Lambda by the function name. So a CloudTrail event
  joins to its correlation id and its execution through the ledger.

**Programs that use AWS.**
- A new grant kind: `[broker.programs.aws] account = "<home-account-id>"`.
- At spawn, the broker asks `AwsCreds` for a job session, and gives the program
  `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`, and `AWS_REGION`. It also sets
  `AWS_CONFIG_FILE` and `AWS_SHARED_CREDENTIALS_FILE` to `/dev/null`, so no `~/.aws` profile is ever picked up
  (§3.19: AWS credentials come from the vault, never from `~/.aws`).
- The existing rules hold: direct argv only, `secret.granted` in the ledger, and nothing written.

**Secret-bearing results.**
- A secret value the account returns (Secrets Manager, a SecureString parameter, a decrypted key) goes onto
  the secrets board as a runtime secret: in memory, zeroized, and never written.
- The scrubber learns it at once.
- The tool returns a handle (`aws-secret:<name>`) and the value's shape with the secret masked. A later
  `proc.run` or toollet may be granted the handle. The model never sees the value.

**Attribution, end to end.** For any call Theseus makes, CloudTrail's record says:
- `userIdentity.arn`: `…:assumed-role/theseus-owner/exe_…`, the execution;
- `sourceIdentity`: the deployment;
- `userAgent`: `… call/<correlation id>`, the exact tool call;
- `requestID`: the same id the call's ledger row holds.

`aws.trail` looks events up by session name. Event history keeps 90 days of management events, at no cost.

**Several accounts.**
- There is one `AwsCreds` per `[aws.accounts.<id>]` table.
- A call names its account, or uses the only one.
- After serving, `GetCallerIdentity` must return the table's id. Otherwise that account's tools fail closed,
  and health says why.

### 3.6 Guardrails: one list, two enforcers

**One list.** `guardrails.toml` in `theseus-aws` is the only source. Each entry names the operations, the
input condition where one is needed, the limit it serves, and its SCP form. Appendix C has the draft.

| group | the limit it serves | examples |
|---|---|---|
| **Public ingress** | SOC2 | a security group rule from `0.0.0.0/0` or `::/0`; a public IP on an instance, a network interface, or a task (`assignPublicIp`); `MapPublicIpOnLaunch`; Elastic IPs; internet-facing load balancers; Lambda function URLs with `AuthType NONE`; public API endpoints; S3 Block Public Access turned off, or a policy or ACL granting `*`; any resource policy or trust policy that grants a principal outside the account; `PubliclyAccessible` databases; CloudFront distributions; an internet gateway |
| **The audit trail** | SOC2 | CloudTrail `StopLogging`, `DeleteTrail`, `UpdateTrail`, `PutEventSelectors`; the trail bucket's policy, lifecycle, and objects; its KMS key; stopping Config, GuardDuty, or Access Analyzer; the alarm topic |
| **The budget** | budget | creating, changing, or deleting Budgets and their actions; detaching the budget action's deny policy; `LeaveOrganization` and other Organizations calls; closing the account |
| **Long-lived credentials** | SOC2 (the operator's employer's practice: the one IAM user "anywhere, ever, always") | `CreateUser`, `CreateAccessKey`, `CreateLoginProfile`, service-specific credentials, SSH keys; any change to the `<iam-user>` key |
| **IaC-only** | IaC | the durable-infrastructure actions of §3.4's table (the separate guard, `theseus-guard-iac`) |
| **Another project's resources** _(since 2026-10-04, theseus-mgw.9; Part III Item 134)_ | `others-resources` | `network-not-ours`: a direct change to network plumbing that exists (deleting, modifying, replacing, revoking, disassociating or detaching VPCs, subnets, route tables and routes, NAT, internet and egress-only gateways, ACLs, endpoints, flow logs, security groups and peering), at the floor even where the operation is otherwise IaC-only; in a template, a member naming a resource the template does not make (`not_own`). AWS denies the same on whatever lacks `theseus:owner`. Creates of new things stay IaC-only |

This replaces the 2026-09-29 sketch's floor rule. IAM roles and policies are no longer on the floor: they are
Theseus's business now (Eddie, 2026-09-30). Only what enforces the two hard limits is.

**Enforcer 1: the gate asks first**, locally and deterministically.
- An `aws.*` call's plan checks its operation and typed input against the list.
- `aws.stack.plan` and `aws.stack.apply` check every resource in the template the same way.
- A hit is the floor (§3.9). The call waits for approval at every posture, and its confirm names the
  guardrail: "public ingress: AuthorizeSecurityGroupIngress 0.0.0.0/0 on sg-… port 443".
  - If the operator approves, the call runs in a floor session: no guard, for that one call, in a session
    that the core alone holds and that expires in 15 minutes.
  - It is rare, since most guarded things live in stacks and change through the stack path. An example is
    lifting the budget's stop after the operator raises the budget.
  - If they decline, the call is recorded and never runs.
  - It is never refused and never silent.
- **IaC-only comes first.** An IaC-only operation in `aws.call` is invalid input, as §3.4 says, and its
  guardrail is checked when the stack applies.
  - A few guardrail operations can only ever be direct: lifting the budget's stop (Budgets attached its
    policy outside any stack), Organizations calls, and closing the account.
  - Their entries say `direct = true`, and they go to the floor.

**Enforcer 2: AWS refuses what the gate did not approve.**
- **Now, with no Organization:**
  - The guards are on every work and job session. Every hand role has `theseus-boundary` as its permissions
    boundary: everything is allowed except what the guards deny. (A boundary has to allow, so it cannot be
    the deny-only guard itself.) _(Since 2026-10-04, theseus-mgw.9; Part III Item 134: the deployer role is bound too. `theseus-guard-deployer`, generated from every `scp = "deny"` entry's denies, is attached to `DeployerRole` in the foundation (with `GuardDeployerPolicy` in its stack policy); before it the deployer carried no session guard, and SCPs need an Organization. A stack that ever needs one of those actions fails until that changes. The boundary is then at 6,016 of IAM's 6,144 characters (theseus-mgw.13).)_
  - So AWS refuses a guardrail action that did not come through an approved floor session, whether it came
    from a CLI job, from a hand, or from a mistake.
  - **Honest limit:** the owner role itself is not guarded. Theseus, the owner, could mint an unguarded
    session. The toolset mints one only as a floor session, after the operator's approval, and ledgers every
    mint.
  - **AWS also watches.** The posture stack's CloudTrail alarms (the employer's CIS set: security group changes, IAM
    policy changes, trail changes, root use, and more) email Eddie and come home as events. A breach is seen
    AWS-side even if every Theseus layer failed.
- **With the Organization** (Eddie's decision, to be renewed before step 14's brief):
  - The same list becomes SCPs that Theseus cannot detach, plus the budget's deny-spend SCP at 100%.
  - The crate generates them from the list: `theseus aws scp` prints the JSON for Eddie to apply in the
    management account he alone holds. A test keeps the gate's list and the SCPs in step.
  - Some input no IAM condition key can see; a security group rule's CIDR is one. For those, the SCP denies
    the action to everyone but the deployer role, and the gate's scan of the template asks first.

**Detective controls: the posture stack.** It mirrors the operator's employer's posture template, sized to the budget:
- CloudTrail: multi-region, with log-file validation and KMS, recording management events. Data events cost
  extra and stay off.
- IAM Access Analyzer, for external-access findings, which are free.
- S3 Block Public Access on at the account level, and EBS encryption by default.
- The CIS alarms, from CloudTrail metric filters, to an SNS topic that emails Eddie and feeds the completion
  queue, so Theseus sees them.
- GuardDuty and AWS Config are question 11.

### 3.7 Budgets, tags, and the reaper

**The monthly budget: config → stack → AWS Budgets.**
- `monthly_budget_usd` in `[aws.accounts.<id>]` is the source of truth. The budget is infrastructure too, so
  it lives in the foundation stack, and the amount is a stack parameter.
- **The reconcile.** After serving, and after any config change (which is a restart, §3.19), a tender
  compares the config to the stack's parameter.
  - When they differ, it makes a change set and applies it without asking: the amount is the operator's own
    stated will, from the vault.
  - It applies only a diff that touches nothing but the budget. Any other diff stops the reconcile, which
    reports it.
  - The reconcile is ledgered as `aws.budget.reconciled`, with the old and new amounts.
- **The budget:** COST, MONTHLY, named `theseus-monthly`. Its notifications fire at 50%, 80%, and 100% of
  actual spend, and at 100% of forecast. They go to an SNS topic that emails Eddie and feeds the completion
  queue, so Theseus narrates each alert to the owner.
- **The AWS-side stop.** At 100% of actual spend, a Budgets action (automatic) attaches `theseus-deny-spend`
  to the owner role, the deployer role (so a stack cannot add metered resources either), and the hand roles. That policy denies the cost-bearing actions: runs, launches,
  creations of metered resources, and paid queries.
  - It stops new spend. Hands that are running end within their TTL.
  - Inside the account the owner could lift it, by reversing the Budgets action or detaching the policy. That
    is why both are on the guardrail list. With the Organization, it becomes an SCP action, and hard.
  - Billing data refreshes a few times a day, so this stop lags spend by hours. That is why every hand also
    carries a reservation (§3.3).
- **Theseus's side.**
  - Health shows "AWS: $123 of $300 this month (forecast $280)". It comes from `DescribeBudget`, which is
    free, refreshed every six hours after serving. Cost Explorer is used only for `aws.cost`'s breakdowns.
  - At 100% of actual spend, cost-bearing calls wait for approval, as the model's spend limit does (question
    7).
  - If Bedrock ever becomes a provider, its spend lands in the same account, and one budget covers both.
- _As built (step 40 part 2, 2026-10-04; Part III Item 116): a **daily budget** beside the month's, `daily_budget_usd` reconciled into the foundation's `DailyBudget` (`DailyBudgetUsd`, 0 for none; alerts at 80% and 100%, no action) by the same budget-only change set, setting it being the go, since AWS may charge for a budget past an account's first two; and **the hour's meter**, `hourly_alert_usd` (default $1): what the hour's AWS actions reserve or spend, alerting once an hour past it (`aws.hour.alert`, a META mark, a notice), alert only._
- _Runaway-train mode (built 2026-10-04, theseus-ext.12; Eddie at 11:26: "let the budget notify be the authority unless 'runaway train' mode is triggered, which is, observationally spend is 10x over the limit"; Part III Item 147). The hands' `hourly_alert_usd` and `daily_budget_usd` stay alerts. When observed spend (reserved by running hands, the cost of settled ones) reaches `runaway_factor` (10 by default, at least 2) times a line, or would reach it with an admitted group's own worst case, the account enters runaway mode until the hour or the local day turns: META `aws.runaway.<account>` and an `aws.runaway` row in one frame, one notice, a RUNAWAY line in health, and every new reserving action refused with the words. A cancel, a list or status read, the reaper and settling always run. The refusal latches for the period, and two groups admitted at the same instant can both pass._

**Tags.** The Theseus set:
- `theseus:owner = theseus`, on everything Theseus made;
- `theseus:deployment`, `theseus:execution`, and `theseus:session`;
- `theseus:stack`, on what a stack made;
- `theseus:group` and `theseus:correlation`, on runs;
- `theseus:ttl`, on anything meant to die: experiments, hands, scratch stacks.

Tagging is guaranteed three ways:
- stacks copy their tags onto their resources;
- the hands manager tags every run;
- work and job sessions require `theseus:execution` on creation, wherever AWS supports tag-on-create
  conditions.

Once, the foundation step activates `theseus:owner`, `theseus:stack`, and `theseus:session` as cost-allocation
tags. They apply from about a day after activation. Cost Explorer can then split the bill: Theseus's share
against the other project's (untagged), by stack, and by session.

**The reaper** is a tender that runs hourly, after serving.
- **The inventory:**
  - the Resource Groups Tagging API (`GetResources`, filtered on `theseus:owner`) across the allowed regions;
  - ECS tasks by `startedBy`, and Batch jobs by queue.

  The inventory is cached. It is also the gate's local answer to "is this resource ours?" (§3.9).
- **Expired** means `theseus:ttl` is in the past.
- **What it does:**
  - For a resource a stack made, it deletes the stack, and only when the stack itself carries an expired
    `theseus:ttl`. It never deletes a stack's resources one by one.
  - Loose runs (tasks, jobs, task definitions) get their type's stop or delete.
  - **Anything without `theseus:owner` is never touched.** The other project's untagged resources are invisible to the
    reaper by construction, because it lists by tag.
- **It reports before it acts.** For its first 14 days, it only reports ("would delete stack theseus-exp-42,
  expired 3 h ago, $0.40 a day"). Then it acts. Deleting anything stateful (a bucket that holds objects, a
  table) waits for approval.
- **Its session** allows `Delete*`, `Stop*`, and `Terminate*` only on resources tagged `theseus:owner`,
  wherever AWS supports the `aws:ResourceTag` condition. The code's own check comes first.
- Every reap is ledgered as `aws.reaped`, and each sweep posts one notice.

### 3.8 Observability

**A span per call**, through O1's native OTLP exporter.
- It follows OTel's AWS conventions: `rpc.system = aws-api`, `rpc.service`, `rpc.method`,
  `aws.request_id`, `cloud.account.id`, `cloud.region`.
- It adds Theseus's own attributes: the correlation id, the class and flags, the session name, the status,
  retries, and bytes in and out. _(Since 2026-10-05, theseus-0zm4; spec Part III Item 180: a call that waited
  for approval has its requests' spans under its call in the continuation that ran it, where before they were never
  traced.)_
- Metrics count calls and measure latency by service, operation, and outcome. _(As built 2026-10-05, theseus-ku5f; spec Part III Item
  178: `theseus.aws.calls` and `theseus.aws.duration_ms`, by `rpc.service`, `rpc.method` and
  `theseus.outcome` (`ok`, `unbound`, `error`), walked from every turn's trace, a failed turn's too. AWS's error code is
  not an attribute: it is the service's own text, unbounded, and stays on the span and the `aws.called` row.)_

**A ledger row per call**, `aws.called`: the account, region, service, operation, class and flags, session,
correlation id, request id, status, AWS error code, duration, bytes, and resources. Beside it:
- `aws.session.minted`: the kind, name, role, the policy's digest, and the expiry. Never the credentials.
- `aws.hands.launched` and `aws.hands.settled`, with the cost.
- `aws.stack.planned`, `aws.stack.applied`, and `aws.stack.drift`.
- `aws.budget.*`, `aws.reaped`, and `aws.guardrail` (a floor hit, and what the operator answered).

**Correlating with CloudTrail.**
- The request id is the join key. The session name gives the execution, and the user agent gives the call.
- `aws.trail` and the Observatory read CloudTrail's event history (`LookupEvents`): free, 90 days of
  management events, arriving in about 5–15 minutes, at 2 requests a second.
- **A daily cross-check (a tender)** compares CloudTrail with the ledger. It finds events from Theseus's
  sessions whose request id has no ledger row.
  - Jobs and hands are named by their correlation ids, so their calls are accounted for.
  - An event nothing accounts for is a security notice: AWS saw a call that Theseus did not record.
  - This check also catches the root key signing anything but STS.

**The Observatory's AWS panel:**
1. **Account:** the bound account and regions, when the account was last checked, the key's age, and the
   live sessions.
2. **Spend:** month to date against the budget, the forecast, the top services, Theseus's share by tag, and
   the hands' reservations.
3. **Calls:** per execution, each call's operation, class, outcome, latency, and request id, with its
   CloudTrail event once it arrives.
4. **Hands:** the group grids of §3.3.
5. **Stacks:** status, the last change set, and drift.
6. **The reaper:** the inventory's counts, what has expired, what it would delete (while it only reports),
   and its last sweep.
7. **Guardrails:** floor hits, approved and declined, and the guard's denials that AWS reported.

Hands' logs live in CloudWatch Logs, and `aws.logs.*` reads them. Shipping Theseus's own telemetry to
CloudWatch (§1: "CloudWatch for historical search") is separate work, and not part of this toolset.

### 3.9 How it meets the gate

**What changes in the tool contract** (core wiring, on `main`):
1. **`Plan.aws: Option<AwsPlan>`** holds the account, region, service, operation, class, flags
   (cost-bearing, secret-bearing, destructive), resources, the guardrail it hit if any, and whether it writes
   to a resource outside Theseus's inventory.
2. **`Plan.class: Option<ToolClass>`, a per-call class.** The gate uses it instead of `Tool::class()` when it
   is set. The external-text rule (`external::gate`) and parallel reads (theseus-a60) read it too. So an
   `aws.call` Describe runs beside the response's other reads, and after an external read it keeps its
   posture.
3. **`Retry::IdempotentWithKey`**, the retry class §3.16 already names.
4. **`ToolCtx.aws: Option<Arc<dyn AwsAccess>>`**, bound per call by the runtime: the account binding, a
   signer for the execution's work session, and the correlation id for the user agent. Never the key.

**The order** (§3.9), extended. The first match wins:
1. **The floor:** Theseus's own state and the vault, as now, and **the AWS guardrails**.
2. **The approve lists:** argv, paths, private addresses, and, for AWS, destructive operations. These are
   deleting a stateful resource, deleting a stack, and a change set that replaces or removes a stateful
   resource.
3. **The allow list** (argv).
4. **The posture:**
   - `[policy.tools]` for the tool;
   - then, for an AWS tool, `[policy.aws]`: `"service:Operation"`, then `"service"`, then the class's
     default (`read`, `write`, `run`);
   - then `enforcement`.

After the order, as now, come a granted secret's posture and the external-text hold. Two AWS rules join them,
and the stricter answer wins:
- a write to a resource outside Theseus's inventory (the other project's) is at least `notify`;
- at 100% of the month's budget, a cost-bearing call waits (question 7).
- _(As built 2026-10-05, theseus-6hkx; spec Part III Item 174: the hands' runaway brake, whose
  refusal tells the operator to raise `hourly_alert_usd` or `runaway_factor` under `[aws.accounts.<id>]` and restart,
  does what it says within the hour or day: a mark is ignored once the config gives its line more room (the line or
  the factor raised, or the line removed), and admission decides again; a lowered line keeps the mark. It is read at
  admission and in health, never at the start.)_

The template's lines:

```toml
[policy.aws]
read = "open"
write = "notify"
run = "notify"
# Per service, or per operation (most specific first):
# "ec2" = "approve"
# "s3:DeleteObject" = "open"
```

**External text** (§3.9, T1).
- Data-plane reads bring outside text into the context: object bodies read as text, log lines, queue
  messages, table items, Athena rows, and CloudTrail events (whose user agents and request parameters an
  attacker can set).
- Their result nodes are marked external, with their `s3://` path or ARN, so the session's hold applies.
- Control-plane reads (Describe, List) are not marked (question 4).

**What the surfaces show.**
- A notified call's line: `🔔 aws s3:PutObject s3://theseus-…/x (write = notify)`.
- A card names the account, the region, the operation, and its resources: `<home-account-id> us-west-2 ·
  ec2:TerminateInstances i-0abc… (destructive)`.
- A group of hands is one line.

### 3.10 FAST

- **On the start path:** parsing `[aws]` and registering the `aws.*` tools' fixed definitions, in
  microseconds. Nothing else.
- **After serving:**
  - the vault resolves the key, as it does every secret;
  - then `AwsCreds` checks the account with one `GetCallerIdentity`, and health says `aws: bound
    <home-account-id> us-west-2`;
  - a call made before that waits for it, bounded at 30 s like a secret, then fails with `aws_unbound` and
    the reason.
- _(As built 2026-10-05, theseus-snhr; spec Part III Item 174: before that check, at the top of the
  same `aws.check` phase task and on a blocking thread, each `[policy.aws]` key is checked against the catalog. One
  that names no service or operation (a typo, an alias, a case-loose operation) is named in health's `aws:` line and
  once in the log; the check took about 20 ms and 5 MB in a debug build, so the account check starts that much
  later. The start never fails on one.)_
- **Lazily:**
  - a service's catalog entry is decoded on its first call;
  - a session is minted on an execution's first AWS call, and cached.
- **In the background, off the path, and idle when no AWS work is outstanding:** the SQS poller, the budget
  refresh, the inventory, drift, and the price table.
- **Shutdown waits for nothing AWS.**
  - An in-flight call is a dispatched record.
  - Hands keep running, detached.
  - Their completions wait in SQS for up to 14 days, and the restart's reconciler collects them.
  - The 100 ms budget holds, and a binary swap never touches a hand.
- **The bench.** The lifecycle bench gains a case with `[aws]` configured and the network unreachable, which
  must meet the same budgets. A test proves that no AWS request leaves before the socket answers.
- **Binary:** at most 5 MB for the whole AWS layer. That is a gate check.

## 4. The catalog

Class: **R** read, **W** write, **Run** starts code; **$** cost-bearing; **🔑** secret-bearing (the value
comes back as a handle). Posture is the default: **floor** always asks, **IaC** means the call returns
invalid input and points to a stack. **★ marks the first slice (C1).** Slices are defined in §5.

| service | tool, or operations through `aws.call` | class | posture | slice | notes |
|---|---|---|---|---|---|
| **every service** | `aws.call`: any operation | from the model | by class | **★ C1** reads; C2 writes | C1 accepts reads only; writes arrive with the guards |
| the catalog | `aws.describe` | R | open | **★ C1** | local; no network |
| STS | `aws.whoami` (`GetCallerIdentity`) | R | open | **★ C1** | the account, the session, the budget line |
| STS | `AssumeRole` | W 🔑 | core only | C2 | minted by `AwsCreds`, never by the model |
| S3 | `aws.s3.list` (`ListBuckets`, `ListObjectsV2`, `HeadBucket`) | R | open | **★ C1** | the audit's #1 |
| S3 | `aws.s3.get` (`GetObject`) | R | open | C3 | streamed to a file; external text when read as text |
| S3 | `aws.s3.put` (`PutObject`, multipart) | W | notify | C3 | |
| S3 | `DeleteObject(s)` | W | notify | C2 | a template line may open scratch prefixes |
| S3 | `aws.s3.sync`, `aws.s3.presign` | W | notify; sync's deletes approve | later | a presigned URL dies with its session |
| S3 | `CreateBucket`, bucket policy, public access block | IaC; guardrail | IaC; floor | C2 | |
| CloudWatch Logs | `aws.logs.query` (Logs Insights) | R $ | open | C3 | external text; bytes scanned in the result |
| CloudWatch Logs | `aws.logs.tail` (`FilterLogEvents`) | R | open | C3 | external text |
| CloudFormation | `Describe*` | R | open | **★ C1** | through `aws.call` until `aws.stack.status` |
| CloudFormation | `aws.stack.plan` (change set, diff, guardrail scan) | W | open | C2 | changes nothing until applied |
| CloudFormation | `aws.stack.apply` | W | notify; destructive approve; guardrail floor | C2 | bound to the change set's digest |
| CloudFormation | `aws.stack.status`, drift | R | open | C2 | drift weekly, by a tender |
| CloudFormation | `aws.stack.delete` | W | approve | C2 | lists what the stack keeps |
| Budgets | `DescribeBudget` | R | open | C2 | health's budget line |
| Budgets | changing the budget; lifting its stop (`ExecuteBudgetAction`) | guardrail | IaC, then floor at apply; lifting: floor | C2 | the core's reconcile is not a model call |
| Cost Explorer | `aws.cost` | R $ | open | C2 | $0.01 a call, cached a day |
| CloudTrail | `aws.trail` (`LookupEvents`) | R | open | C3 | external text: event fields |
| CloudTrail | `StopLogging`; deleting or changing the trail | guardrail | stop: floor; changes: IaC, then floor at apply | C2 | |
| ECS / Fargate | `aws.hands.run`, A1 (`RunTask`, `StopTask`) | Run $ | notify | 40 | one notice per group |
| Batch | `aws.hands.run`, arrays (`SubmitJob`, `TerminateJob`) | Run $ | notify | 40+ | a hundred hands in one call |
| Lambda | `aws.hands.run`, A2 (`Invoke`, async) | Run $ | notify | 40 | cancel unsupported; timeout-bound |
| Lambda | `aws.lambda.invoke` (an existing function) | Run $ | notify | later | `aws.call` until then |
| SSM | `aws.ssm.run` (`SendCommand`) | Run | notify | 40+ | A3, on hosts chosen by id or tag |
| SSM | `GetParameter(s)` | R (🔑 for SecureString) | open | ★ C1 for String; C3 for SecureString | a SecureString comes back as a handle |
| SSM | `PutParameter` | W | notify | C2 | data, so direct |
| Secrets Manager | `GetSecretValue` | R 🔑 | open | C3 | the value never enters the context |
| EC2 | `Describe*` | R | open | **★ C1** | |
| EC2 | start, stop, reboot instances | W | notify | C2 | operations on what exists |
| EC2 | `TerminateInstances` | W | approve | C2 | destructive |
| EC2 | `RunInstances`; VPC, subnets, security groups | IaC | IaC | C2 | through a stack |
| EC2 | ingress from anywhere; public IPs; internet gateways | guardrail | IaC, then floor at apply; a run with a public IP: floor | C2 | `aws.hands.run` never asks for a public IP |
| IAM | `Get*`, `List*`, `Simulate*` | R | open | **★ C1** | |
| IAM | roles and policies | IaC | IaC | C2 | Theseus's own business, not the floor |
| IAM | users, access keys, login profiles | guardrail | IaC, then floor at apply | C2 | long-lived credentials |
| KMS | `Decrypt`, `GenerateDataKey` | R 🔑 | open | C3 | the trail key's disable or deletion is the floor |
| the employer-side service | `Describe*`, `List*` | R | open | **★ C1** | the audit's #2, through `aws.call` |
| Service Quotas | `Get*`, `List*` | R | open | **★ C1** | the hands manager reads them too |
| Service Quotas | `RequestServiceQuotaIncrease` | W | notify | C2 | |
| Auto Scaling (application) | `Describe*` | R | open | **★ C1** | registering targets goes through a stack |
| ECR | `Describe*`, `BatchGetImage` | R | open | **★ C1** | `GetAuthorizationToken` is 🔑 |
| SQS | send, receive, delete messages | W | notify | C2 | the completion queue belongs to the core |
| Athena | `aws.athena.query` | W $ | notify | later | a workgroup with a per-query scan cap |
| Organizations, Account | every mutation | guardrail | floor | C2 | the budget's limit |
| Bedrock | `InvokeModel` | Run $ | notify | later | model spend on the same budget |
| **everything else** | `aws.call` | from the model | by class | **★ C1** | |

## 5. The build plan

Two kinds of slice. **Parallel slices (P)** are pure crates or templates. Each runs off `main`:
- in its own worktree (`~/projects/theseus-wt/<slice>`, branch `aws/<slice>`);
- with its own `CARGO_TARGET_DIR` (`~/.cache/theseus-target/<slice>`), since a worktree that shares `target/`
  poisons it (per the agents' operating notes for this repo);
- touching only its own crate or directory, plus one line in the workspace's explicit `members` list.

Each lands through the usual review loop, rebased onto `main`, with the full gate. **Main-chain slices (C)**
wire the core, and run in the roadmap's order. Each slice is about one subagent-hour. Its live check starts
with read-only calls, and a check that writes waits for Tabitha's go-ahead with Eddie.

**The order:**
- **P1, P3, and P4 can start today.** P2 starts once P1's first commit has fixed the catalog's types.
- **C1 (step 14a)** needs P1 and P2.
- **C2 (step 14b)** needs P3, P4, and C1.
- **C3 (step 14c)** needs C2.
- **Steps 15 and 16** need C2.
- **Steps 17 and 18** use C2's job sessions.
- **Step 40** needs C2, and P4's hands stacks.

### Parallel slices

**P1. `theseus-aws-catalog`: the catalog and the classification.**
- **Build:**
  - the `xtask` generator (Smithy JSON AST in, compact catalog out), run from a pinned snapshot of AWS's
    public Smithy models;
  - the format, and the lookup API;
  - the derivation of class, retry class, and flags, with the tables that override it.
- **Tests:**
  - every service in the snapshot decodes;
  - golden classifications for about fifty operations. For example: `ListObjectsV2` is R and safe;
    `RunInstances` is W, $, IaC, and idempotent with its `ClientToken`; SQS `ReceiveMessage` is W; Lambda
    `Invoke` is Run $; `GetSecretValue` is R 🔑;
  - the compressed size is at most 4 MB;
  - one service decodes in under 5 ms.
- **Live check:** none; it works offline. Fetching the models once needs the network, which the builder
  does, and the snapshot's commit is recorded.

**P2. `theseus-aws`: the client.**
- **Build:**
  - the six protocols;
  - `aws-sigv4` signing, and presigning;
  - endpoints from the partition table;
  - retries by retry class;
  - pagination;
  - errors, and the enforcer they name;
  - the attribution user agent.
- **Tests:**
  - Smithy's AWS protocol compliance tests, requests and responses, for all six protocols;
  - AWS's SigV4 test vectors;
  - a local fake endpoint for retries (a throttle is retried; a timeout on a `non_repeatable` call becomes
    `outcome_unknown`), pagination, and error parsing;
  - `cargo tree` counts what it adds;
  - a size probe measures the binary's delta.
- **Live check** (Tabitha, read-only, on the Home account): one read per protocol family:
  - IAM `ListRoles` (awsQuery);
  - EC2 `DescribeRegions` (ec2Query);
  - the employer-side service's `ListEndpoints` (awsJson1_1);
  - Lambda `ListFunctions` (restJson1);
  - S3 `ListBuckets` and Route 53 `ListHostedZones` (restXml);
  - STS `GetCallerIdentity`, in whichever protocol its model now names.

**P3. `theseus-aws-guard`: the guardrails.**
- **Build:**
  - `guardrails.toml`;
  - the evaluator of input conditions over typed input: CIDRs, principals, public flags;
  - the template scanner, over CloudFormation resource properties;
  - the IaC-only and destructive lists;
  - generators for the deny-only guard policies and for the SCP JSON.
- **Tests:**
  - each entry is tested for a hit and a near miss. `10.0.0.0/8` is not public, and `0.0.0.0/0` is. A
    trust policy naming this account is fine, and one naming another account is a hit;
  - the generated policies fit IAM's size limits;
  - every operation the list names exists in P1's catalog, once P1 has landed.
- **Live check** (read-only): `iam:SimulateCustomPolicy` runs the generated guard against sample actions. It
  is free and changes nothing.

**P4. `infra/aws/`: the foundation templates.** This is CloudFormation YAML, with no Rust, and it lives at the
repo's root.
- **Build:**
  - `theseus-foundation`:
    - the owner role and the deployer role;
    - the guards, `theseus-allow-all`, and `theseus-boundary`;
    - the Theseus bucket (versioned, KMS, public access blocked, TLS only), and the durability table
      (DynamoDB, on demand) for steps 15 and 16;
    - the completion queue and its dead-letter queue, and the alerts topic;
    - the budget, with its action and `theseus-deny-spend`.
  - `theseus-posture`: the trail, its bucket and key, Access Analyzer, and the CIS alarms.
  - `theseus-hands-network`: the VPC, two private subnets, the S3 gateway endpoint, and the NAT as a
    parameter.
  - `theseus-hands`:
    - the ECS cluster, and the hand role profiles, with `theseus-boundary` as their permissions boundary;
    - the ECR repository and the log group;
    - the EventBridge rules into the queue;
    - the TTL-reaper Lambda and its schedule, and the Lambda hand.
  - Two account settings have no CloudFormation type, as far as I know: S3's account-level Block Public
    Access, and EBS encryption by default. The bootstrap sets them once, and ledgers them.
- **Tests** (offline):
  - `cfn-lint` passes;
  - P3's scanner finds only the expected guardrail hits (the NAT's internet gateway and Elastic IP), each
    documented as one floor approval at bootstrap;
  - rules for tags and encryption: every bucket encrypted, versioned, and blocked from public access; every
    log group with a retention; every queue encrypted.
- **Live check** (read-only): `ValidateTemplate` on each template. The first change set is C2's.

### Main-chain slices

**C1 = step 14a. The bound account, and reads: the tracer bullet.**
- **Wire:**
  - `[aws.accounts.<id>]`: `credentials`, `region`, and `regions`. The keys enter the template only when
    honored (§3.19);
  - `AwsCreds`: the key from the board, the account check after serving, the health line, and failing
    closed;
  - `ToolCtx.aws`, `Plan.aws`, the per-call class, and `[policy.aws]`;
  - the tools: `aws.call` (reads only: anything else, and anything secret-bearing, is invalid input that
    names the slice that brings it), `aws.describe`, `aws.whoami`, and `aws.s3.list`;
  - a span and an `aws.called` row per call; the CLI line, the Discord tool line, and the web UI's account
    panel.
- **Tests:**
  - the config template's three tests;
  - the tools against a local fake endpoint;
  - the bench's offline case;
  - no AWS request before the socket answers.
- **Live check** (read-only), on a scratch daemon, from Discord: "what stacks are there, what's in our
  buckets, and which endpoints of the employer-side service exist?" That runs `aws.whoami`, `aws.call cloudformation
  DescribeStacks`, `aws.s3.list`, and `aws.call <employer-side service> ListEndpoints`. Fifteen minutes later, CloudTrail's
  event history shows each call, with the user agent naming its correlation id.

**C2 = step 14b. Stacks, the owner role, writes, and the budget.**
- **Wire:**
  - `aws.stack.plan`, `.apply`, `.status`, and `.delete`, over the IaC repository (question 12);
  - `theseus aws bootstrap`: it plans the foundation and posture stacks with the user key, the operator
    approves, and it applies them;
  - then `AwsCreds` switches to role sessions (work, job, floor, and tender), with the guards on;
  - `aws.call` writes, with the floor, IaC-only as invalid input, and destructive operations waiting;
  - the budget's reconcile (config → stack parameter), the health line, and `aws.cost`;
  - the program grant `[broker.programs.aws]`.
- **Tests:**
  - the floor, per guardrail group;
  - IaC-only as invalid input;
  - how session policies compose;
  - the reconcile is idempotent (against a fake);
  - the bootstrap is idempotent (a second run finds no diff).
- **Live check:**
  - read-only first: `ValidateTemplate`, and the foundation's diff shown but not executed;
  - then, with Eddie's go-ahead (the first writes to his account): apply the bootstrap.
  - Then check that CloudTrail shows `assumed-role/theseus-owner/exe_…`.
  - Check that a `proc.run aws s3api create-bucket` in a job session is denied, naming the session policy.
  - Check that the budget holds the configured amount.

**C3 = step 14c. The curated tools, and the owner's eyes.** _Built 2026-10-04 (theseus-mgw.5 and theseus-9p40; Part III Item 97): every wire item but the panel. The reaper is a tool in report mode, not yet a tender; a put is one `PutObject` of at most 64 MiB and a large get reads by ranges, since the client reads a body whole (no multipart, no streaming); the Observatory's AWS panel became a list of what the cockpit's should show (the report's seven points), not built. The live check ran on the account at the review: the query, the trail lookup (its request ids equal the ledger's), the inventory (34 resources, none of the other project's), and a put and get in the foundation bucket._
- **Wire:**
  - `aws.s3.get` and `.put`, `aws.logs.query` and `.tail`, and `aws.trail`;
  - secret-bearing handles, as runtime secrets on the board;
  - the inventory, and the reaper in report mode;
  - external marking for data-plane reads;
  - the Observatory's AWS panel;
  - the CloudTrail cross-check tender.
- **Tests:**
  - a handle's value never reaches a node, the WAL, or the ledger (the scrubber's test pattern);
  - the reaper never lists an untagged resource (a fake inventory with resources shaped like the other project's);
  - a data-plane read sets the session's hold.
- **Live check:**
  - read-only first: a Logs Insights query on one of the other project's log groups, a trail lookup of C2's sessions, and the
    inventory;
  - then a put and get round trip in the Theseus bucket.

**Step 15, the durability tender.** WAL segments go to S3 (multipart `PutObject`), and index rows to DynamoDB
(`BatchWriteItem`). It runs under a tender session narrowed to the bucket and table the foundation made. It
adds no tool. Live check: segments land within the 5–60 s target, and `oldest_unshipped` is exported. _Built 2026-10-04 (theseus-mgw.7; Part III Item 108): in the daemon, reading the WAL through `theseus-follow`; the open segment's tails beside sealed segments and blobs; the session `theseus-durability` (put, get, list parts and abort under its prefix, and `BatchWriteItem` under `dynamodb:LeadingKeys`; no delete). The live check on the account caught up 4.0 s after a start and shipped a turn 11.2 s after it, each tail's checksum equal to its row's; a sealed segment was not seen (64 MiB of WAL)._ _(Fixed 2026-10-05, theseus-iame and theseus-mgw.12; spec Part III Item 171: only frames the
WAL has synced ship, bounded by the writer's own synced position, read at each batch, so a power loss never leaves a
tail in S3 past the log; a pass held by an unsynced frame says `waiting` "for the WAL's sync" in health and passes
again a settle later. The tender's session may list its own prefix (`s3:ListBucket` under `StringLikeIfExists` on
`durability/<deployment>/*`, as the restore's has it), so a missing key heads 404, not 403, and an object in flight at
a crash is sent once instead of retried forever.)_ _(Since 2026-10-06, theseus-bfk9; Part III Item 198: `StringLike`, not `StringLikeIfExists`. Real S3 judges a missing key's HEAD or GET on the implied `s3:ListBucket` with `s3:prefix` set to the key itself, so plain `StringLike` on `durability/<deployment>/*` already answers 404 / `NoSuchKey`, and `IfExists` only widened the list to every key name in the bucket; a list that names no prefix is refused now. The tender's and the restore's sessions list only their own prefix, as a live probe with the joined policies showed, 12 of 12.)_

**Step 16, ~~`theseus restore --from s3://…`~~ `theseusd restore --from s3://…`.** It uses the client's list and get from the CLI, under a tender
session. Live check: a scratch store restored from S3 (read-only on AWS). _Built 2026-10-04 (theseus-mgw.10; Part III Item 123): `theseusd restore --from s3://<bucket>/durability/<deployment>/`. It signs in a session of its own, `theseus-restore`, not a tender's, minted for the URL's deployment under an inline policy that only reads (`s3:GetObject` and `s3:ListBucket` under the deployment's prefix, `dynamodb:Query` on its leading keys). The rows decide what is current, not a listing: a segment comes from its sealed object when there is one, else from its tails stitched only where each begins where the last ended; every object is checked against its row's length and SHA-256, and a mismatch refuses the whole restore; a gap stops it there and is said. Reads up to 8 MiB are one `GetObject` with S3's checksum mode, larger ones ranged. When the config's deployment is the one restored, the tender's cursor and the shipped blobs are seeded, so the next start ships only the restore's own row. The live check on the account (under a cent) was left for the owner at the join._ _(Since 2026-10-04, theseus-mgw.10's policy; Part III Item 147: the restore session lists its prefix under `StringLikeIfExists` on `s3:prefix`, so S3 answers a missing object's `GetObject` 404, said as missing, not 403; a list with no prefix now passes, which shows key names, never contents outside the prefix. The live check with a deleted blob waits for the owner's go. The durability tender's session has no `s3:ListBucket`, so real S3 would answer its `HeadObject` of a missing key 403.)_ _(Since 2026-10-06, theseus-bfk9; Part III Item 198: both sessions' list statements are `StringLike`; a missing object still answers 404 / `NoSuchKey` under the session's prefix, and a list with no prefix is refused again.)_ _(Since 2026-10-05, theseus-b9x6; spec Part III Item 171: a restore joins a sealed object to
the tails that start at its end, when the first begins at the object's last position + 1 ("from its object and N
tail(s) after it"), so the frames shipped after a restore's own row, while that segment was still open, are read by
the next restore; a tail that does not join is said, never stitched. The restore's gap and seed guards each have a
test.)_

**Steps 17–18, L1.** An L1 job starts with no AWS credentials. The `aws` program grant gives it a job session
(the guards, plus its `aws_policy`). Since L1 blocks the metadata service, that is the only credential it can
hold. Live check: `aws sts get-caller-identity` inside L1 names the job's correlation id, and a guarded action
is denied, naming the guard.

**Step 40, hands: A2 and A1, with SQS and EventBridge.** _(Part 1 built 2026-10-04, theseus-mgw.6; Part III Item 107: the role, the image's Dockerfile and build script, `aws.hands.run` on Lambda and Fargate, and the SQS poller with each completion's check. The image could not be built in the cloud, and the live check waited for hands on an existing network (theseus-mgw.9).)_ _Built 2026-10-04 in two parts: part 1 (theseus-mgw.6, Part III Item 107) and part 2 (theseus-mgw.11, Item 116): cancellation per backend, reservations, quotas and waves, overdue and reaped hands, the hour's alert and the daily budget, `hands.list`, Discord's line and the cockpit's grid. Lambda's part was proven live on the account (twenty hands, the hour's alert, a `kill -9` mid-group); Fargate's waits for hands on the account's existing network (theseus-mgw.9, Item 134)._
- **Wire:**
  - the binary's `hand` role, and the hand image (built, then pushed to ECR);
  - `aws.hands.run` on Lambda and Fargate;
  - the SQS poller, the check of each completion, and the reconciler's AWS path;
  - cancellation per backend;
  - the TTL reaper;
  - budget reservations;
  - the Observatory's grid, and Discord's group line.
- **Tests:** the simulator's standing scenarios against a fake queue: completions dropped, duplicated, and
  late; a crash in the middle of a group; a restart with completions waiting. Also the cancel lifecycle per
  backend.
- **Live check:**
  - read-only first: the quotas, and `DescribeClusters`;
  - then one Lambda hand, one Fargate hand, and a twenty-hand group with `until: first_success` and
    `max_usd: 1`;
  - the daemon is `kill -9`'d in the middle of the group, and after the restart every result settles exactly
    once.

**Filed, not built** (Beads issues at priority 2 or 3, once C1 lands): Batch arrays, `aws.ssm.run` (A3), the A4 dev
box, `aws.s3.sync` and `.presign`, `aws.athena.query`, `aws.lambda.invoke`, `rpcv2Cbor` and event streams,
and the lazy NAT (if question 5 keeps it). Applying the SCPs is Eddie's, in his management account.

## 6. Open questions for Eddie, each with its default

Nothing waits on these: each slice takes the default, and Eddie can overturn it later.

1. **Where do the hard limits live?** This is the SCP recommendation you asked to have renewed "when we get
   deep into aws". It is renewed before step 14's brief, and not decided here.
   *Until then:* the guards and the floor, which are soft inside the account, plus the posture stack's alarms
   to your email. The SCP JSON is generated and waiting for your management account.
2. **Is one full-admin role consistent with "no theseus-only IAM"?** An IAM user cannot put a name on a
   session, so attribution by execution needs a role that Theseus assumes: `theseus-owner`, with
   administrator access.
   *Default:* yes. It is the owner's own hat, not a limited identity, and the user key then signs only STS.
3. **What is the root of trust?**
   *Default:* keep the `<iam-user>` IAM user key in the vault for v1. Health warns when it is 90 days old, and
   you rotate it. Revisit if Theseus moves onto EC2, where an instance profile replaces the key.
4. **Is text read from AWS "external"?** Log lines, object bodies read as text, queue messages, table items,
   Athena rows, and CloudTrail events can all carry text an attacker wrote.
   *Default:* yes, marked external, so the next acting call waits. So "check the logs, then restart the
   service" asks once. `[policy] external_text = "notify"` is the knob if that proves too much.
5. **The NAT gateway.** Fargate hands have no public IP, so they reach the internet through a NAT, about $36
   a month when it is always on.
   *Default:* lazy. It comes on when a hand needs egress, and goes off after an idle hour, so the first hand
   after an idle spell waits about two minutes. The alternatives are always on, or no NAT at all (Lambda
   hands, and Fargate hands that reach only AWS).
6. **One dollar budget per session, for models and hands together?**
   *Default:* yes. A hand's estimated cost is reserved against the session's $100 at launch and settled at
   completion, so a runaway fan-out meets the same "reset?" question as a runaway model loop.
7. **The monthly amount, and a stop on Theseus's side.** You called the Theseus-side gate optional.
   *Default:* $300 (the 2026-09-29 draft's figure). At 100% of actual spend, cost-bearing calls wait for
   approval: it is one comparison, and the AWS-side stop lags spend by hours.
8. **Regions.**
   *Default:* `us-west-2` for work, with `us-east-1` also allowed, since Budgets, Cost Explorer, and the
   global services' control planes live there. Any other region needs a config line.
9. **The operator's employer's accounts.** The audit's calls to the employer-side service, SSO, and Auto Scaling are the employer's work, done through
   profiles.
   *Default:* no. Only the Home account is bound. Another account could be added later as its own
   `[aws.accounts.<id>]` table, but that is a separate conversation: it isn't Theseus's to own.
10. **Long-lived credentials on the floor.** A stack that would create IAM users, access keys, or console
    passwords always asks, following the operator's employer's "one IAM user, ever".
    *Default:* yes.
11. **Detective controls against cost.**
    *Default:* on: CloudTrail (management events), Access Analyzer, S3's account-level Block Public Access,
    EBS encryption by default, and the CIS alarms, which are all cheap. GuardDuty is on too: its cost scales
    with events and is small in a quiet account, and its 30-day trial shows the real number. AWS Config stays
    off until an audit needs it, since hands churn network interfaces and Config bills per item recorded.
12. **Where does Theseus's IaC live?**
    *Default:* a new private repository, `zeroaltitude/theseus-infra`, cloned at `~/projects/theseus-infra`,
    with signed commits like Theseus's own. The foundation templates ship inside Theseus itself
    (`infra/aws/`).
13. **The other project.**
    *Defaults:*
    - a write to anything outside Theseus's inventory posts a notice, and is never silent;
    - the reaper never touches it;
    - changing the infrastructure of one of the other project's resources means importing it into a stack first, as a project done
      with you, never automatically;
    - the system prompt describes the other project as shared work.

## 7. Appendices

### A. The config, a draft

These keys enter `theseusd example-config` only in the slice that honors them (§3.19: no inert config).

```toml
[aws.accounts.<home-account-id>]
credentials = "<secret name>"         # C1: a [secrets] pair, the key id and secret; the root of trust
region = "us-west-2"                  # C1: the default for this account's calls
regions = ["us-west-2", "us-east-1"]  # C1: the allow-list (question 8)
deployment = "theseus-eddie-desktop"  # C2: every session's source identity
monthly_budget_usd = 300              # C2: reconciled into the foundation stack's budget (question 7)
owner_role = "theseus-owner"          # C2: set by the bootstrap; until then calls sign with the key
nat = "lazy"                          # step 40: "lazy" | "on" | "off" (question 5)

[policy.aws]                          # C1
read = "open"
write = "notify"
run = "notify"

[broker.programs.aws]                 # C2: `proc.run aws …` gets a job session
account = "<home-account-id>"
```

### B. Names

| kind | name | made by |
|---|---|---|
| role | `theseus-owner` (administrator, assumed by the `<iam-user>` user) | foundation |
| role | `theseus-cfn-deployer` (CloudFormation's service role) | foundation |
| role | `theseus-budget-action` (runs the Budgets action) | foundation |
| roles | `theseus-hand-basic`, `-read`, `-owner` (bounded by `theseus-boundary`) | hands |
| policies | `theseus-guard-iac`, `theseus-guard-limits` (deny-only); `theseus-allow-all`; `theseus-boundary`; `theseus-deny-spend` | foundation |
| bucket | `theseus-<home-account-id>-us-west-2`: `hands/`, `stacks/` (templates), `durability/` (steps 15–16) | foundation |
| table | `theseus-durability` (DynamoDB, on demand: the index rows of steps 15–16) | foundation |
| queue | `theseus-completions`, with `theseus-completions-dlq` | foundation |
| topic | `theseus-alerts` (budget and CIS alarms: email and the queue) | foundation |
| budget | `theseus-monthly` | foundation |
| trail | `theseus-trail`, with its own bucket and KMS key | posture |
| network | `theseus-hands` VPC: two private subnets, an S3 gateway endpoint, the NAT as a parameter | hands-network |
| compute | ECS cluster `theseus-hands`; ECR `theseus/hand`; log group `/theseus/hands`; the TTL reaper's schedule | hands |

### C. The guardrail list, a draft of its shape

Each entry maps API operations (what the gate sees) to IAM actions (what policies and SCPs name) where the two
differ. For example, `UpdateBudget` and `DeleteBudget` are the IAM action `budgets:ModifyBudget`.

```toml
[[guardrail]]
name = "sg-ingress-anywhere"
limit = "soc2.public-ingress"
operations = ["ec2:AuthorizeSecurityGroupIngress", "ec2:ModifySecurityGroupRules"]
when = { cidr_any = ["0.0.0.0/0", "::/0"] }
template = { types = ["AWS::EC2::SecurityGroup", "AWS::EC2::SecurityGroupIngress"], cidr_any = ["0.0.0.0/0", "::/0"] }
scp = "deny-except-deployer"      # no condition key sees the CIDR

[[guardrail]]
name = "public-ip"
limit = "soc2.public-ingress"
operations = ["ec2:RunInstances", "ecs:RunTask", "ecs:CreateService", "ec2:ModifySubnetAttribute"]
when = { any_true = ["AssociatePublicIpAddress", "MapPublicIpOnLaunch"], any_equals = { assignPublicIp = "ENABLED" } }
scp = "deny-where-condition-key"  # ec2:AssociatePublicIpAddress; the rest deny-except-deployer

[[guardrail]]
name = "elastic-ip"
limit = "soc2.public-ingress"
operations = ["ec2:AllocateAddress", "ec2:AssociateAddress"]
scp = "deny-except-deployer"      # the hands network's NAT gets its address through the stack

[[guardrail]]
name = "trail-tamper"
limit = "soc2.audit-trail"
operations = ["cloudtrail:StopLogging", "cloudtrail:DeleteTrail", "cloudtrail:UpdateTrail", "cloudtrail:PutEventSelectors"]
scp = "deny-except-deployer"      # the posture stack can still change the trail; nothing can stop it

[[guardrail]]
name = "budget-tamper"
limit = "budget"
operations = ["budgets:UpdateBudget", "budgets:DeleteBudget", "budgets:UpdateBudgetAction", "budgets:DeleteBudgetAction"]
scp = "deny-except-deployer"      # the budget lives in the foundation stack

[[guardrail]]
name = "budget-stop-lift"
limit = "budget"
operations = ["budgets:ExecuteBudgetAction", "iam:DetachRolePolicy"]
when = { policy = "theseus-deny-spend" }   # the detach only; reversing the action always
direct = true                     # Budgets attached it outside any stack, so only a direct call lifts it
scp = "deny"

[[guardrail]]
name = "long-lived-credentials"
limit = "soc2.credentials"
operations = ["iam:CreateUser", "iam:CreateAccessKey", "iam:CreateLoginProfile", "iam:UpdateLoginProfile", "iam:CreateServiceSpecificCredential", "iam:UploadSSHPublicKey"]
scp = "deny"                      # so rotating the <iam-user> key then needs Eddie's management account
```

The IaC-only list and the destructive list have the same shape, with no `when`, grouped by service. The
IaC-only list generates the deny-only `theseus-guard-iac` and `aws.call`'s invalid-input check (§3.4). The
destructive list feeds the gate's approve step (§3.9).

_(Since 2026-10-04, theseus-mgw.9; Part III Item 134: the list gained `network-not-ours`, the first entry of a new limit, `others-resources`. It is `direct`, with a `when` of `present` on the plumbing ids a call names, so even an otherwise IaC-only change to plumbing that exists goes to the floor. Its `iam` patterns (`ec2:Delete*`, `Disassociate*`, `Detach*`, `Replace*`, `Modify*`, `Associate*`, `Attach*`, `Authorize*`, `Revoke*`, `CreateRoute`, `CreateNetworkAclEntry`, `CreateTags`) are denied, `scp = "deny"`, on the ARNs of VPCs, subnets, route tables, NATs, internet gateways, ACLs and security groups that lack `theseus:owner` and are not being tagged at a create; security-group-rule ARNs, and `CreateSecurityGroup`, are left to the gate, so the hands' own tagged group can be made in another VPC. The template rule's new `not_own` test asks at plan for a member naming a resource the template does not make: a literal or a parameter's value yes, an import maybe, the template's own `Ref` or `GetAtt` no. Every `scp = "deny"` entry now also generates `theseus-guard-deployer`.)_

*— written by Tabitha/Claude, 2026-09-30*

### D. The operating manual, a first draft (added at review, 2026-09-30 16:11)

Eddie tied the owner model to "the right operational docs (system prompt, etc)". This is the first draft of
what Theseus's system prompt says when an account is bound, as a context file (`[context] files`, or a
`theseus-aws` persona's). C1 ships it with the tools; each later slice extends it.

> **You own the Home AWS account** (`<home-account-id>`). It is yours: you create, change, and remove what you need
> without asking, within the postures your operator sets. You are not a limited user. The limits you place on
> your own hands (narrower sessions and roles) are your own tools for safety, so use them.
>
> **Two limits are the operator's, and you keep them:**
> - **The budget.** The month's spend and your session's dollars are real money. Estimate before you fan out,
>   set `max_usd` on groups of hands, and stop and ask when a plan's cost is unclear. The budget's stop is
>   AWS-side, and it lags spend by hours, so don't lean on it.
> - **The SOC2 stance.** No public ingress, ever: no public IPs, no open security groups, no public buckets. The
>   audit trail stays intact. Durable infrastructure changes only through stacks (change sets you plan, show,
>   and apply), never with direct calls. No long-lived credentials: no IAM users, access keys, or console
>   passwords. Encrypt at rest.
>
> **How you work:**
> - Read freely: `aws.call` reaches every service, and `aws.describe` and the curated tools are faster for
>   the common questions.
> - Operate directly: runs, data, and starting, stopping, or scaling what exists.
> - Build through stacks, kept in git in the infra repository, with signed commits.
> - Hands: Lambda for short jobs, Fargate for long ones, groups for fan-out. Every hand is tagged, has a TTL,
>   and has a dollar reservation. Cancel what you no longer need.
> - Everything you do is attributed to your execution in CloudTrail. Work as if the operator will read the
>   trail, because they can.
>
> **Another project is shared work.** The account also holds that project's resources, which you build with Eddie.
> Your cleanup never touches what you did not create. A change to that project's infrastructure is a project you
> propose to Eddie (import into a stack first), never a side effect. Reading its logs and data is fine, and
> text you read there is external: after it, your acting calls wait, as after a web page.
>
> **When to ask:** before anything irreversible to data you did not create; before spend beyond the session's
> budget; whenever the guardrail floor asks; and whenever you are unsure whether something is that project's.

### E. Review notes (Tabitha, 2026-09-30 16:11)

- **The approach holds.** One dynamic client over the service models is the right shape for "the broadest
  reach possible": every service at one fixed cost, and no per-service SDK crates bloating the build. The
  cost is ours to carry: six protocols done right. The Smithy protocol compliance tests and SigV4's vectors
  are what make that affordable. S3's restXml quirks, checksums, and presigning deserve the most tests.
- **P1's models: use the local AWS CLI's botocore data, not a network fetch.** A build subagent can't fetch
  AWS's Smithy models from the web: a URL in a Bash command taints its session (the provenance trap). The AWS
  CLI v2 installed here (2.34.15) carries botocore's JSON models for 337 services
  (`/usr/local/aws-cli/v2/current/dist/awscli/botocore/data`: `service-2.json`, `paginators-1.json`,
  `waiters-2.json`, and `endpoint-rule-set-1.json` per service). They are the same service definitions, in the
  format boto3 uses for exactly this kind of dynamic calling, and they need no network. They carry no
  `@readonly` trait, so classification leans on the HTTP method and the operation's name, and P1's override
  table becomes the authority for the rest. The weekly updater becomes "update the CLI, regenerate". If
  Smithy's richer traits prove necessary later, Tabitha fetches a snapshot outside a build session.
- **Question 4's default is right, and important.** With owner power, text from AWS (logs, objects, messages)
  is exactly where an injection would come from. T1's hold is the floor.
- **The parallel slices P1, P3, and P4 are offline**: no account writes, and only read-only live calls (P3's
  `SimulateCustomPolicy`, P4's `ValidateTemplate`). They can start as soon as Eddie says go. C1 and later touch
  the account, and follow the SCP conversation (question 1).

<!-- REPORT COMPLETE -->
