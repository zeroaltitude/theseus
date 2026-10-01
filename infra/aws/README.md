# infra/aws: the CloudFormation Theseus's AWS account is built from

These are the foundation templates of Theseus's AWS design (theseus-mgw.1, slice P4, theseus-mgw.4). Each file is
one stack, named after the file.

| stack | what it holds |
|---|---|
| `theseus-foundation` | the owner and deployer roles; the guards (`theseus-guard-iac`, `theseus-guard-limits`), `theseus-allow-all`, and `theseus-boundary`; the Theseus key, bucket, and durability table; the completion queue and its dead-letter queue; the alerts topic; and the monthly budget, with `theseus-deny-spend` as its stop |
| `theseus-posture` | the trail, with its own bucket and key; the CIS alarms; IAM Access Analyzer; GuardDuty; and the EBS snapshot public-sharing block |
| `theseus-hands-network` | the hands' VPC: two private subnets, an S3 gateway endpoint, flow logs, and the NAT as a parameter |
| `theseus-hands` | the ECS cluster; the hand role profiles; the hand image's repository; the hands' logs; the rules that bring state changes home to the completion queue; the TTL reaper; and the Lambda hand |

Posture and the hands import the foundation's exports. The foundation imports nothing.

- **The order:** foundation, posture, hands network, hands.
- **The first creation:** `bootstrap.md` (slice C2's `theseus aws bootstrap`). After it, every change is a change
  set that `theseus-cfn-deployer` applies.
- **Stack policies:** `stack-policies/`. They keep the trail, the data, the guards, and the budget from being
  replaced or deleted by an update.

## Checking offline

```bash
infra/aws/check.sh
```

It runs these, and makes no AWS call:

- **cfn-lint** over the four templates, informational checks included.
- **`test/rules.py`:**
  - every taggable resource carries `theseus:owner` and `theseus:stack`;
  - every bucket is KMS-encrypted, versioned, blocked from public access, ACL-free, and TLS-only;
  - every log group has a retention, and every queue and topic is encrypted;
  - stateful resources are retained;
  - every IAM document fits its size limit, and every template fits CloudFormation's 51,200-byte body;
  - the boundary is exactly allow-all, minus what the guards deny;
  - no deny pattern catches a read operation in the AWS CLI's models;
  - the hands' roles are bounded;
  - nothing admits ingress.
- **The rules' own tests,** and the TTL reaper's code run against a fake ECS.

cfn-lint comes from `THESEUS_CFN_LINT_VENV` (default `~/.cache/theseus-cfn-lint`), or from `PATH`. The models
come from the `aws` CLI on `PATH`, or from `THESEUS_BOTOCORE_DATA`.

## The guards are v0

The guard documents in `theseus-foundation.yaml` are written by hand from the design. Slice P3
(`theseus-aws-guard`) generates them from `guardrails.toml`. When the two meet, its output replaces these
statements, and `test/rules.py` keeps the boundary in step with them and within IAM's 6,144 characters.
