# Bootstrapping Theseus's AWS account

The notes for slice C2's `theseus aws bootstrap` (Theseus's AWS design, theseus-mgw.1, §3.4–3.7 and §5).
The bootstrap runs once, with the operator's approval. Everything after it changes through change sets that the
deployer role applies.

## 0. Read-only checks first

- **Who:** `sts:GetCallerIdentity` gives the account and the root-of-trust IAM user. That user's name is the
  `OwnerUserName` parameter of the foundation and posture stacks.
- **Singletons that would collide:**
  - a GuardDuty detector in the region (if there is one, posture takes `GuardDuty=disabled`);
  - an `ACCOUNT` analyzer in IAM Access Analyzer (if so, `AccessAnalyzer=disabled`);
  - other trails (the trail's management events are free only for the account's first copy);
  - stacks, exports, roles, policies, buckets, tables, queues, log groups, repositories, aliases, and rules named
    `theseus*`.
- **Before account-level S3 Block Public Access:** for every bucket, its policy status, any ACL grant to a group,
  and any website configuration. A bucket that is public on purpose must move first.
- **Before blocking EBS snapshot sharing:** the account's public snapshots. Posture's `SnapshotPublicSharing`
  defaults to `block-all-sharing`, which also hides the ones already public.
- **Quotas hands will meet:** Lambda's concurrent executions (a new account starts at 10, and the reaper shares
  them) and Fargate's On-Demand and Spot vCPU counts. Raising them is a Service Quotas request, which is a write:
  step 40 makes it, with the operator.

## 1. The foundation, signed with the user key

The deployer role doesn't exist yet, and neither does the bucket a template would be uploaded to. So:

- a change set of type `CREATE` for the stack `theseus-foundation`;
- the template **by body**, which must stay within CloudFormation's 51,200 bytes (`test/rules.py` checks this);
- **no role ARN**;
- `CAPABILITY_NAMED_IAM`, since the roles and policies have the design's names;
- parameters: `OwnerUserName`, `MonthlyBudgetUsd` (the config's `monthly_budget_usd`), and `AlertEmail` (the
  operator's);
- stack tags: `theseus:owner=theseus`, `theseus:stack=theseus-foundation`, `theseus:deployment`, and
  `theseus:execution`;
- termination protection on, and the stack policy `stack-policies/theseus-foundation.json`.

Show the change set and wait for the operator's approval. Then execute it and wait for `CREATE_COMPLETE`. SNS
mails the operator a confirmation link for the alerts topic, which they must open once.

## 2. Off the key

- Assume `theseus-owner` with a **source identity** (the deployment's name), because the trust policy refuses a
  session without one. The session name is the bootstrap's execution id.
- From here on, the key signs only `sts:AssumeRole` and `sts:GetCallerIdentity`. Once posture exists, its
  `theseus-root-of-trust-key-use` alarm fires on anything else the key signs.
- The rest of the bootstrap makes guarded changes, such as the account settings in step 4. So it runs in a
  **floor session**: the owner role with no guard. The design's floor session covers one call, so C2 decides
  whether the operator's one approval of the bootstrap covers them all, or each guarded call is approved alone.

## 3. Posture, through the deployer

- A change set of type `CREATE` for `theseus-posture`, with the role ARN `theseus-cfn-deployer` and
  `CAPABILITY_NAMED_IAM`.
- Its parameters are `FoundationStack`, `OwnerUserName`, and `GuardDuty`, `AccessAnalyzer`, and
  `SnapshotPublicSharing`, as step 0's checks found.
- Turn termination protection on, and set the stack policy `stack-policies/theseus-posture.json`.
- Then update `theseus-foundation` once with the deployer's role ARN and no template change. Its later change sets
  then run as the deployer.

## 4. Account settings that have no CloudFormation type

Each is a direct call, made once, and ledgered. None is in a stack. (The schemas checked are cfn-lint 1.57.1's,
dated 2026-09-28; EBS snapshot sharing does have a type, and posture sets it.)

- **S3 account-level Block Public Access:** `s3control:PutPublicAccessBlock` for the account, with all four
  settings `true`.
- **EBS encryption by default:** `ec2:EnableEbsEncryptionByDefault`, in each allowed region (`us-west-2` and
  `us-east-1`). The default key, `aws/ebs`, will do.
- **AMI public sharing:** `ec2:GetImageBlockPublicAccessState` should already say `block-new-sharing`, the default
  for newer accounts. If it doesn't, call `ec2:EnableImageBlockPublicAccess`.
- **The hands VPC's default security group** (once `theseus-hands-network` exists): revoke the rules AWS gives it,
  so it admits nothing (CIS 5.4). CloudFormation can't remove a default group's rules.
- **Cost-allocation tags:** activate `theseus:owner`, `theseus:stack`, and `theseus:session` with
  `ce:UpdateCostAllocationTagsStatus`. A key can be activated only once it has appeared in the billing data, about
  a day after the first resource carries it, so this step comes a day later.

## 5. The hands (they may wait for step 40)

- **`theseus-hands-network`** starts with `NatGateway=disabled`, so it has no internet gateway and no Elastic IP.
  Enabling the NAT later makes the template's only two guardrail hits, the internet gateway and the Elastic IP:
  public ingress, as the template scanner reports them, and one floor approval. With the NAT off, Fargate hands
  can reach only S3, through the gateway endpoint, so they can't pull images or write logs. Turn the NAT on before
  any Fargate hand runs.
- **`theseus-hands`** starts with `HandImageUri` empty. Step 40 pushes the image to `theseus/hand` and sets the
  parameter, which creates the Lambda hand.

## 6. What C2's live check proves after

These are writes, so they wait for the operator.

- CloudTrail shows the bootstrap's calls as `assumed-role/theseus-owner/<execution id>`, with the deployment as
  the source identity.
- **The alerts path, end to end.** The alerts topic is KMS-encrypted, and its key lets Budgets, CloudWatch, and
  EventBridge use it only for that topic. Nothing offline can prove that grant.
  - From a floor session (`theseus-guard-limits` denies `SetAlarmState` on `theseus-*` alarms), set one
    `theseus-cis-*` alarm to `ALARM` with `cloudwatch:SetAlarmState`.
  - Both the operator's email and a message in `theseus-completions` must arrive.
- The budget `theseus-monthly` holds the configured amount, and both of its actions are in `STANDBY`.
- A job session's `aws s3api create-bucket` is denied, and the denial names the session policy.
