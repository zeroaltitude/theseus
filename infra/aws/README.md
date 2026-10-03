# infra/aws: the CloudFormation Theseus's AWS account is built from

These are the foundation templates of Theseus's AWS design (theseus-mgw.1, slice P4, theseus-mgw.4), in C2's lean
posture (theseus-nyzn). Each file is one stack, named after the file. The bootstrap's three are carried in
`theseusd` itself (`theseus aws bootstrap`).

| stack | what it holds |
|---|---|
| `theseus-foundation` | the owner and deployer roles; the guards (`theseus-guard-limits`, `theseus-guard-iac` and its parts, `theseus-guard-stacks`), `theseus-allow-all`, and `theseus-boundary`; the Theseus bucket (SSE-S3) and durability table; the completion queue and its dead-letter queue; the alerts topic; and the monthly budget, with `theseus-deny-spend` as its stop |
| `theseus-posture` | the trail, with its own bucket (SSE-S3, or its own KMS key when `TrailKey` is `customer`); the CIS checks, as EventBridge rules on CloudTrail's management events into the alerts topic; IAM Access Analyzer; GuardDuty; and the EBS snapshot public-sharing block |
| `theseus-posture-relay` | in us-east-1 when the home region is another: one rule that forwards us-east-1's CloudTrail events (IAM, Organizations, Budgets, the console's sign-in) to the home region's default bus, where the posture's checks see them |
| `theseus-hands-network` | the hands' VPC: two private subnets, an S3 gateway endpoint, flow logs, and the NAT as a parameter |
| `theseus-hands` | the ECS cluster; the hand role profiles; the hand image's repository; the hands' logs; the rules that bring state changes home to the completion queue; the TTL reaper; and the Lambda hand |

Posture and the hands import the foundation's exports. The foundation imports nothing.

- **The order:** foundation, posture, relay, hands network, hands.
- **The first creation:** `bootstrap.md` (C2's `theseus aws bootstrap`). After it, every change is a change set that
  `theseus-cfn-deployer` applies (`aws.stack.plan`, then `aws.stack.apply`).
- **Stack policies:** `stack-policies/`. They keep the trail, the data, the guards, the budget, and the relay from
  being replaced or deleted by an update.
- **The lean posture** adds about $0.10 a month to a quiet account: no customer KMS key (one when `TrailKey` is
  `customer`, about $1 a month), no CloudWatch alarm, metric filter, or log group. The alerts topic has no SSE: SNS
  encrypts only with a KMS key, and the services that publish to it (EventBridge, Budgets) need a customer key's
  policy for that; its email copy is plain text anyway, and the queue it feeds is SSE-SQS.

## Checking offline

```bash
infra/aws/check.sh
```

It runs these, and makes no AWS call:

- **cfn-lint** over the templates, informational checks included.
- **`test/rules.py`:**
  - every taggable resource carries `theseus:owner` and `theseus:stack`;
  - every bucket is encrypted by default (SSE-S3, or SSE-KMS with a key; both sides of an `Fn::If`), versioned,
    blocked from public access, ACL-free, and TLS-only;
  - every log group has a retention, every queue is encrypted, and every topic is, unless its `Metadata`'s
    `theseus.unencrypted` says why not;
  - stateful resources are retained;
  - every IAM document fits its size limit, and every template fits CloudFormation's 51,200-byte body;
  - the boundary is allow-all, covering every guard's denies, and denying beyond them only by
    `guardrails.toml`'s `[boundary] compact` patterns;
  - no deny pattern catches a read operation in the AWS CLI's models;
  - the hands' roles are bounded;
  - nothing admits ingress.
- **The rules' own tests,** and the TTL reaper's code run against a fake ECS.

cfn-lint comes from `THESEUS_CFN_LINT_VENV` (default `~/.cache/theseus-cfn-lint`), or from `PATH`. The models
come from the `aws` CLI on `PATH`, or from `THESEUS_BOTOCORE_DATA`.

The gate's suite holds the rest: the templates scan clean against the guard list, the lean defaults make no KMS
key, alarm, or log group, and `TrailKey=customer` makes exactly one key (`theseus-core`'s
`aws::tests_c2::the_lean_templates_make_no_key_alarm_or_log_group_by_default`).

## The guards are generated

The foundation's guard and boundary documents are what `crates/theseus-aws-guard/guardrails.toml` generates, one
minified line each, so AWS enforces the list the gate reads. The guard crate's test
`the_foundation_template_carries_the_generated_guards` fails when a line is out of step; after the list changes,
rewrite them with `THESEUS_GUARD_WRITE=1 cargo test -p theseus-aws-guard --test policies`.
