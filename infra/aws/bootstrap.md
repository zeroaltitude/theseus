# Bootstrapping Theseus's AWS account

`theseus aws bootstrap` (C2, theseus-nyzn; Theseus's AWS design, theseus-mgw.1, §3.4–3.7 and §5) makes an account's
first stacks from the templates `theseusd` carries: `theseus-foundation` and `theseus-posture` in the account's region,
and `theseus-posture-relay` in us-east-1 when that is another. It runs once, with the operator's approval.
Everything after it changes through change sets that the deployer role applies.

```bash
theseus aws bootstrap --alert-email <the operator's address>     # the plan, then the question
theseus aws bootstrap --plan-only                                # the plan alone
theseus aws bootstrap --alert-email <address> --apply <digest>   # the plan whose digest that is, no question
```

`--trail-key customer` gives the trail its own KMS key (about $1 a month); the default is SSE-S3. The account's
`[aws.accounts.<id>]` table must exist in the config, with no `owner_role` yet; `monthly_budget_usd` (default 50) is
the budget's amount.

## 0. The plan is read-only

It makes no change set: a create's change set makes a stack in `REVIEW_IN_PROGRESS`, which is a write. It reads:

- **Who:** `sts:GetCallerIdentity` (the account's check) gives the root-of-trust IAM user, the stacks'
  `OwnerUserName`. It tries `sts:AssumeRole` into `theseus-owner` once: refused before the foundation exists, so the
  plan reads with the key; after, it reads in that session, so a second plan signs nothing with the key but STS.
- **Each stack:** `DescribeStacks`. A new stack's plan is its template's resources under its parameters, read
  statically. An existing one is compared with `GetTemplate` and its parameters: equal is no change, so a plan after
  the apply shows none. An existing stack's termination protection (`DescribeStacks`) and policy (`GetStackPolicy`)
  are read too: what it lacks, the apply sets.
- **Each stack's policy** is its file in `stack-policies/`, cut to the resources the template makes under the plan's
  parameters: AWS refuses a policy that names a logical id its stack lacks, and the lean posture
  (`TrailKey=aws-managed`) has no `TrailKmsKey`. The digest covers the policy.
- **Singletons that would collide**, for a new posture: a GuardDuty detector in the region (then
  `GuardDuty=disabled`), an `ACCOUNT` analyzer (then `AccessAnalyzer=disabled`), and other trails (the trail's
  management events are free only as the account's first copy). Each is a warning in the plan.

The plan prints every stack, its parameters, each resource a create makes, what an existing stack lacks (`none; its
stack policy and termination protection are not set, and the apply sets them`), each policy the apply sets, the
warnings, and its digest.

## 1. The apply, on the operator's yes

It plans again, and refuses a plan whose digest is not the one approved. Then, in order:

1. **The foundation, signed with the key:** a change set of type `CREATE`, by body (`test/rules.py` keeps every
   template within 51,200 bytes), with no role ARN (the deployer doesn't exist yet) and `CAPABILITY_NAMED_IAM`.
   Its resources are checked against the plan before it is executed. SNS mails the alert address a confirmation
   link, which the operator opens once.
2. **Off the key:** a floor session of `theseus-owner` (allow-all, no guard; the source identity is the
   deployment), tried for up to a minute while IAM learns the new role. The operator's one approval of the plan
   covers every guarded change the bootstrap makes in it.
3. **The posture and the relay**, by change sets the deployer applies, each checked against the plan first.
4. **Each stack's policy and termination protection,** in the floor session, with no change set: on each stack the
   apply made, and on each existing stack that lacks them. A policy that is set but is not the one the plan computes
   stays as it is, and the plan warns: a hand edit is the operator's.

The foundation's later change sets name the deployer too (the budget's reconcile does), so CloudFormation uses it
from then on.

### When an apply stops

- **Run the bootstrap again.** A stack the stopped run made plans as `none`, with what it lacks: the apply sets its
  policy and termination protection, and goes on with the stacks still to make.
- **An owner role that can't mint sessions** is one thing the bootstrap can't repair: it changes an existing
  foundation only in a floor session of that same role. (The first live run hit this: the role's trust refused
  `sts:TagSession`.) The operator's key can apply the fix as a foundation change set directly, checked first:
  1. an `UPDATE` change set of `theseus-foundation` from the fixed template, every parameter `UsePreviousValue`, with
     `CAPABILITY_NAMED_IAM`, signed with the key;
  2. described before it runs: execute it only if its one change is `Modify OwnerRole`, with no replacement and only
     the properties the fix names (for the trust, `AssumeRolePolicyDocument`); otherwise delete it;
  3. wait for `UPDATE_COMPLETE`, then run the bootstrap again.

  Once the posture exists, each call the key signs, but `GetCallerIdentity` and an `AssumeRole` into
  `theseus-owner`, trips `theseus-posture-root-of-trust-key-use`: those alerts are this repair's.

## 2. After

- **Name the owner role:** add `owner_role = "theseus-owner"` to `[aws.accounts.<id>]` and restart. Every call then
  signs in a role session named by its execution, the key signs only STS, and the posture's
  `theseus-posture-root-of-trust-key-use` rule alerts on anything else it signs. Until then the key signs, and that
  rule fires on each call: the alerts say the config is behind.
- **The tenders** start with the owner role: the budget's reconcile once a start, its line every six hours, and
  GuardDuty's usage weekly (health warns past $1 a month).

## 3. Account settings that have no CloudFormation type

Each is a direct call, made once by hand, in a floor session, and checked first. None is in a stack. (The schemas
checked are cfn-lint 1.57.1's, dated 2026-09-28; EBS snapshot sharing does have a type, and posture sets it.)

- **S3 account-level Block Public Access:** `s3control:PutPublicAccessBlock` for the account, with all four
  settings `true`. First, for every bucket: its policy status, any ACL grant to a group, and any website
  configuration. A bucket that is public on purpose must move first.
- **EBS encryption by default:** `ec2:EnableEbsEncryptionByDefault`, in each allowed region (`us-west-2` and
  `us-east-1`). The default key, `aws/ebs`, will do.
- **AMI public sharing:** `ec2:GetImageBlockPublicAccessState` should already say `block-new-sharing`, the default
  for newer accounts. If it doesn't, call `ec2:EnableImageBlockPublicAccess`.
- **The hands VPC's default security group** (once `theseus-hands-network` exists): revoke the rules AWS gives it,
  so it admits nothing (CIS 5.4). CloudFormation can't remove a default group's rules.
- **Cost-allocation tags:** activate `theseus:owner`, `theseus:stack`, and `theseus:session` with
  `ce:UpdateCostAllocationTagsStatus`. A key can be activated only once it has appeared in the billing data, about
  a day after the first resource carries it.
- **Before blocking EBS snapshot sharing** (the posture's `SnapshotPublicSharing`, default `block-all-sharing`):
  the account's public snapshots, which it also hides.

## 4. The hands (step 40)

- **`theseus-hands-network`** starts with `NatGateway=disabled`, so it has no internet gateway and no Elastic IP.
  Enabling the NAT later makes the template's only two guardrail hits, the internet gateway and the Elastic IP:
  public ingress, as the template scanner reports them, and one floor approval. With the NAT off, Fargate hands
  can reach only S3, through the gateway endpoint, so they can't pull images or write logs. Turn the NAT on before
  any Fargate hand runs.
- **`theseus-hands`** starts with `HandImageUri` empty. Step 40 pushes the image to `theseus/hand` and sets the
  parameter, which creates the Lambda hand.

## 5. What the live check proves after

These are writes, so they wait for the operator.

- CloudTrail shows the bootstrap's calls after the foundation as `assumed-role/theseus-owner/<session>`, with the
  deployment as the source identity.
- **The alerts path, end to end.** A job session's `aws s3api create-bucket` is denied, naming the session policy
  (`theseus-guard-iac`), and that denial trips `theseus-posture-unauthorized-api-calls`: both the operator's email
  and a message in `theseus-completions` must arrive, the message naming the rule.
- An IAM call by the key (any `iam:List*`) trips `theseus-posture-root-of-trust-key-use` through the relay.
- The budget `theseus-monthly` holds the configured amount, and its action is in `STANDBY`.
