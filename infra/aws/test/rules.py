#!/usr/bin/env python3
"""The offline rules for Theseus's CloudFormation templates (P4, theseus-mgw.4).

    python3 infra/aws/test/rules.py [TEMPLATE...]      (default: infra/aws/theseus-*.yaml)

Prints one line per violation, then the managed policies' sizes, and exits 1 if any rule failed.
Needs PyYAML. cfn-lint's resource schemas, when importable, cross-check the tag table; the botocore
models of the `aws` CLI on PATH (or THESEUS_BOTOCORE_DATA) back the wildcard rule.

The rules, each named in its violations:
  tags         every taggable resource carries theseus:owner = theseus and
               theseus:stack = the stack's name (design section 3.7)
  bucket       every bucket: KMS encryption with a key, versioning, all four public-access
               blocks, ACLs off, and a policy that refuses requests without TLS
  log-group    every log group has a retention
  queue        every queue is encrypted
  topic        every topic is encrypted with a KMS key
  retain       every bucket, table, key, and image repository is kept when its stack goes
  policy-size  managed policies fit IAM's 6,144 characters, a role's inline policies its
               10,240, and a trust policy its 2,048 (whitespace is not counted)
  boundary     theseus-boundary is allow-all, plus both guards' denies, plus the statements
               whose Sid starts with HandsOnly: each guard deny is covered by a boundary deny,
               and each other boundary deny by a guard's (so it may merge and drop what a
               broader pattern covers, to fit its 6,144 characters); the guards only deny
  wildcards    no action a guard, the boundary, or theseus-deny-spend denies matches a
               read operation, and every plain action names a real operation
  bounded      every role the hands stack makes has theseus-boundary
  no-ingress   no subnet maps public IPs, no security group admits anything, and an
               internet gateway or Elastic IP exists only under a condition
  inline-code  inline Lambda code fits CloudFormation's 4,096 characters
  template-size  each template fits CloudFormation's 51,200-byte TemplateBody, which the
               foundation's first create must use: the bucket a larger one would go through is
               one of the things it makes
  stack-policy every logical id a stack policy in stack-policies/ names exists in its template
  metric-filter  every metric filter's pattern fits CloudWatch Logs' 1,024 characters
"""
import fnmatch
import glob
import json
import os
import re
import shutil
import sys
from pathlib import Path

import yaml

HERE = Path(__file__).resolve().parent
INFRA = HERE.parent

# Each resource type the templates use, and its tag property; None when the type takes no tags.
# A type missing from this table is itself a violation, so a new type is a conscious choice.
TAG_PROPERTY = {
    "AWS::AccessAnalyzer::Analyzer": "Tags",
    "AWS::Budgets::Budget": "ResourceTags",
    "AWS::Budgets::BudgetsAction": "ResourceTags",
    "AWS::CloudTrail::Trail": "Tags",
    "AWS::CloudWatch::Alarm": "Tags",
    "AWS::DynamoDB::Table": "Tags",
    "AWS::EC2::EIP": "Tags",
    "AWS::EC2::FlowLog": "Tags",
    "AWS::EC2::InternetGateway": "Tags",
    "AWS::EC2::NatGateway": "Tags",
    "AWS::EC2::Route": None,
    "AWS::EC2::RouteTable": "Tags",
    "AWS::EC2::SecurityGroup": "Tags",
    "AWS::EC2::SnapshotBlockPublicAccess": None,
    "AWS::EC2::Subnet": "Tags",
    "AWS::EC2::SubnetRouteTableAssociation": None,
    "AWS::EC2::VPC": "Tags",
    "AWS::EC2::VPCEndpoint": "Tags",
    "AWS::EC2::VPCGatewayAttachment": None,
    "AWS::ECR::Repository": "Tags",
    "AWS::ECS::Cluster": "Tags",
    "AWS::Events::Rule": "Tags",
    "AWS::GuardDuty::Detector": "Tags",
    "AWS::IAM::ManagedPolicy": None,
    "AWS::IAM::Role": "Tags",
    "AWS::KMS::Alias": None,
    "AWS::KMS::Key": "Tags",
    "AWS::Lambda::EventInvokeConfig": None,
    "AWS::Lambda::Function": "Tags",
    "AWS::Lambda::Permission": None,
    "AWS::Logs::LogGroup": "Tags",
    "AWS::Logs::MetricFilter": None,
    "AWS::S3::Bucket": "Tags",
    "AWS::S3::BucketPolicy": None,
    "AWS::SNS::Subscription": None,
    "AWS::SNS::Topic": "Tags",
    "AWS::SNS::TopicPolicy": None,
    "AWS::SQS::Queue": "Tags",
    "AWS::SQS::QueuePolicy": None,
}

RETAINED = ("AWS::S3::Bucket", "AWS::DynamoDB::Table", "AWS::KMS::Key", "AWS::ECR::Repository")

GUARD_NAMES = ("theseus-guard-iac", "theseus-guard-limits")
BOUNDARY_NAME = "theseus-boundary"
ALLOW_ALL_NAME = "theseus-allow-all"
DENY_SPEND_NAME = "theseus-deny-spend"

MANAGED_POLICY_LIMIT = 6144
INLINE_POLICIES_LIMIT = 10240
TRUST_POLICY_LIMIT = 2048
INLINE_CODE_LIMIT = 4096
TEMPLATE_BODY_LIMIT = 51200
FILTER_PATTERN_LIMIT = 1024

# IAM prefixes whose action names are the botocore operations' names, and their models.
BOTOCORE_SERVICES = {
    "access-analyzer": ["accessanalyzer"],
    "acm": ["acm"],
    "athena": ["athena"],
    "batch": ["batch"],
    "cloudfront": ["cloudfront"],
    "cloudtrail": ["cloudtrail"],
    "cloudwatch": ["cloudwatch"],
    "codebuild": ["codebuild"],
    "config": ["config"],
    "dms": ["dms"],
    "dynamodb": ["dynamodb"],
    "ec2": ["ec2"],
    "ecr": ["ecr"],
    "ecs": ["ecs"],
    "eks": ["eks"],
    "elasticache": ["elasticache"],
    "elasticloadbalancing": ["elbv2", "elb"],
    "elasticmapreduce": ["emr"],
    "emr-serverless": ["emr-serverless"],
    "es": ["es", "opensearch"],
    "events": ["events"],
    "firehose": ["firehose"],
    "fsx": ["fsx"],
    "glue": ["glue"],
    "guardduty": ["guardduty"],
    "iam": ["iam"],
    "kafka": ["kafka"],
    "kinesis": ["kinesis"],
    "kms": ["kms"],
    "lambda": ["lambda"],
    "lightsail": ["lightsail"],
    "logs": ["logs"],
    "mq": ["mq"],
    "organizations": ["organizations"],
    "rds": ["rds"],
    "redshift": ["redshift"],
    "redshift-serverless": ["redshift-serverless"],
    "route53": ["route53"],
    "route53domains": ["route53domains"],
    "sagemaker": ["sagemaker"],
    "savingsplans": ["savingsplans"],
    "scheduler": ["scheduler"],
    "sns": ["sns"],
    "sqs": ["sqs"],
    "states": ["stepfunctions"],
    "transfer": ["transfer"],
    "workspaces": ["workspaces"],
}
# IAM actions with no operation of the same name in their service's model.
IAM_ONLY = {"lambda:InvokeFunction", "iam:PassRole"}
READ_VERBS = ("Describe", "List", "Get", "Head", "Search", "Lookup", "BatchGet", "Query", "Scan")


# ------------------------------------------------------------------------------------------------
# Loading
# ------------------------------------------------------------------------------------------------


class _Loader(yaml.SafeLoader):
    """YAML with CloudFormation's short-form intrinsics, read as their long forms."""


def _intrinsic(loader, suffix, node):
    if isinstance(node, yaml.ScalarNode):
        value = loader.construct_scalar(node)
    elif isinstance(node, yaml.SequenceNode):
        value = loader.construct_sequence(node, deep=True)
    else:
        value = loader.construct_mapping(node, deep=True)
    if suffix == "Ref":
        return {"Ref": value}
    if suffix == "Condition":
        return {"Condition": value}
    if suffix == "GetAtt" and isinstance(value, str):
        value = value.split(".", 1)
    return {"Fn::" + suffix: value}


_Loader.add_multi_constructor("!", _intrinsic)


def load(path):
    with open(path, encoding="utf-8") as f:
        return yaml.load(f, Loader=_Loader)


def stack_name(path):
    return Path(path).stem


def resources(template):
    return (template.get("Resources") or {}).items()


def as_list(value):
    return value if isinstance(value, list) else [value]


# ------------------------------------------------------------------------------------------------
# Rendering a document for its size: every intrinsic becomes a string at least as long as the
# value AWS would put there, so a document that fits here fits there.
# ------------------------------------------------------------------------------------------------

PSEUDO = {
    "AWS::AccountId": "123456789012",
    "AWS::Partition": "aws-us-gov",
    "AWS::Region": "ap-southeast-4",
    "AWS::URLSuffix": "amazonaws.com",
}
LONG_ARN = "x" * 120


def render(node, template, stack):
    if isinstance(node, dict):
        if len(node) == 1:
            (key, value), = node.items()
            if key == "Ref":
                return _ref(value, template, stack)
            if key == "Fn::Sub":
                text, names = (value, {}) if isinstance(value, str) else (value[0], value[1])
                return re.sub(
                    r"\$\{([^}!]+)\}",
                    lambda m: render(names[m.group(1)], template, stack)
                    if m.group(1) in names
                    else _sub_name(m.group(1), template, stack),
                    text,
                )
            if key in ("Fn::GetAtt", "Fn::ImportValue"):
                return LONG_ARN
            if key == "Fn::Join":
                sep, parts = value
                return sep.join(render(p, template, stack) for p in parts)
            if key == "Fn::If":
                a, b = render(value[1], template, stack), render(value[2], template, stack)
                return a if len(json.dumps(a)) >= len(json.dumps(b)) else b
        return {k: render(v, template, stack) for k, v in node.items()}
    if isinstance(node, list):
        return [render(v, template, stack) for v in node]
    return node


def _ref(name, template, stack):
    if name == "AWS::StackName":
        return stack
    if name in PSEUDO:
        return PSEUDO[name]
    params = template.get("Parameters") or {}
    if name in params:
        default = params[name].get("Default")
        return str(default) if default not in (None, "") else "x" * 64
    return LONG_ARN


def _sub_name(name, template, stack):
    if "." in name:
        return LONG_ARN
    return _ref(name, template, stack)


def policy_size(document, template, stack):
    text = json.dumps(render(document, template, stack), separators=(",", ":"))
    return len(re.sub(r"\s", "", text))


# ------------------------------------------------------------------------------------------------
# The rules. Each takes (path, template) and yields (rule, resource, message).
# ------------------------------------------------------------------------------------------------


def rule_tags(path, template, schemas=None):
    for name, res in resources(template):
        kind = res.get("Type")
        if kind not in TAG_PROPERTY:
            yield "tags", name, f"{kind} is not in the tag table; is it taggable?"
            continue
        prop = TAG_PROPERTY[kind]
        if schemas is not None:
            msg = schemas.disagrees(kind, prop)
            if msg:
                yield "tags", name, msg
        if prop is None:
            continue
        tags = {}
        for tag in (res.get("Properties") or {}).get(prop) or []:
            if isinstance(tag, dict) and "Key" in tag:
                tags[tag["Key"]] = tag.get("Value")
        if tags.get("theseus:owner") != "theseus":
            yield "tags", name, "lacks theseus:owner = theseus"
        if tags.get("theseus:stack") != {"Ref": "AWS::StackName"}:
            yield "tags", name, "lacks theseus:stack = !Ref AWS::StackName"


def _bucket_policies(template):
    """The statements of every bucket policy, by the logical name of the bucket it governs."""
    out = {}
    for _, res in resources(template):
        if res.get("Type") != "AWS::S3::BucketPolicy":
            continue
        props = res.get("Properties") or {}
        bucket = props.get("Bucket")
        if isinstance(bucket, dict) and "Ref" in bucket:
            statements = as_list((props.get("PolicyDocument") or {}).get("Statement") or [])
            out.setdefault(bucket["Ref"], []).extend(statements)
    return out


def _refuses_plain_http(statements, bucket):
    covers = {json.dumps({"Fn::GetAtt": [bucket, "Arn"]}), json.dumps({"Fn::Sub": "${" + bucket + ".Arn}/*"})}
    for st in statements:
        cond = (st.get("Condition") or {}).get("Bool") or {}
        if (
            st.get("Effect") == "Deny"
            and st.get("Principal") == "*"
            and st.get("Action") in ("s3:*", ["s3:*"])
            and str(cond.get("aws:SecureTransport")).lower() == "false"
            and covers <= {json.dumps(r) for r in as_list(st.get("Resource"))}
        ):
            return True
    return False


def rule_bucket(path, template):
    policies = _bucket_policies(template)
    for name, res in resources(template):
        if res.get("Type") != "AWS::S3::Bucket":
            continue
        props = res.get("Properties") or {}
        sse = ((props.get("BucketEncryption") or {}).get("ServerSideEncryptionConfiguration") or [{}])[0]
        default = sse.get("ServerSideEncryptionByDefault") or {}
        if default.get("SSEAlgorithm") != "aws:kms" or not default.get("KMSMasterKeyID"):
            yield "bucket", name, "is not encrypted by default with a KMS key"
        if (props.get("VersioningConfiguration") or {}).get("Status") != "Enabled":
            yield "bucket", name, "is not versioned"
        block = props.get("PublicAccessBlockConfiguration") or {}
        for key in ("BlockPublicAcls", "BlockPublicPolicy", "IgnorePublicAcls", "RestrictPublicBuckets"):
            if block.get(key) is not True:
                yield "bucket", name, f"does not set {key}"
        rules = (props.get("OwnershipControls") or {}).get("Rules") or []
        if not any(r.get("ObjectOwnership") == "BucketOwnerEnforced" for r in rules):
            yield "bucket", name, "keeps ACLs on (ObjectOwnership is not BucketOwnerEnforced)"
        if not _refuses_plain_http(policies.get(name, []), name):
            yield "bucket", name, "has no bucket policy refusing requests without TLS"


def rule_log_group(path, template):
    for name, res in resources(template):
        if res.get("Type") == "AWS::Logs::LogGroup" and "RetentionInDays" not in (res.get("Properties") or {}):
            yield "log-group", name, "has no retention"


def rule_queue(path, template):
    for name, res in resources(template):
        if res.get("Type") != "AWS::SQS::Queue":
            continue
        props = res.get("Properties") or {}
        if props.get("SqsManagedSseEnabled") is not True and not props.get("KmsMasterKeyId"):
            yield "queue", name, "is not encrypted"


def rule_topic(path, template):
    for name, res in resources(template):
        if res.get("Type") == "AWS::SNS::Topic" and not (res.get("Properties") or {}).get("KmsMasterKeyId"):
            yield "topic", name, "is not encrypted with a KMS key"


def rule_retain(path, template):
    for name, res in resources(template):
        if res.get("Type") in RETAINED:
            for policy in ("DeletionPolicy", "UpdateReplacePolicy"):
                if res.get(policy) != "Retain":
                    yield "retain", name, f"{policy} is not Retain"


def managed_policies(template):
    out = {}
    for name, res in resources(template):
        if res.get("Type") == "AWS::IAM::ManagedPolicy":
            props = res.get("Properties") or {}
            out[props.get("ManagedPolicyName") or name] = (name, props.get("PolicyDocument") or {})
    return out


def rule_policy_size(path, template, sizes=None):
    stack = stack_name(path)
    for policy, (name, document) in managed_policies(template).items():
        size = policy_size(document, template, stack)
        if sizes is not None:
            sizes.append((stack, policy, size))
        if size > MANAGED_POLICY_LIMIT:
            yield "policy-size", name, f"{policy} is {size} characters, over {MANAGED_POLICY_LIMIT}"
    for name, res in resources(template):
        if res.get("Type") != "AWS::IAM::Role":
            continue
        props = res.get("Properties") or {}
        inline = sum(policy_size(p.get("PolicyDocument") or {}, template, stack) for p in props.get("Policies") or [])
        if inline > INLINE_POLICIES_LIMIT:
            yield "policy-size", name, f"its inline policies are {inline} characters, over {INLINE_POLICIES_LIMIT}"
        trust = policy_size(props.get("AssumeRolePolicyDocument") or {}, template, stack)
        if trust > TRUST_POLICY_LIMIT:
            yield "policy-size", name, f"its trust policy is {trust} characters, over {TRUST_POLICY_LIMIT}"


def _deny_triples(statements, skip_sid_prefix=None):
    """Each Deny statement as (action, resource, condition) triples, one per action and resource."""
    out = set()
    for st in statements:
        if st.get("Effect") != "Deny":
            continue
        if skip_sid_prefix and str(st.get("Sid", "")).startswith(skip_sid_prefix):
            continue
        cond = json.dumps(st.get("Condition"), sort_keys=True)
        for action in as_list(st.get("Action")):
            for resource in as_list(st.get("Resource")):
                out.add((action, json.dumps(resource, sort_keys=True), cond))
    return out


def rule_boundary(path, template):
    policies = managed_policies(template)
    present = [n for n in (*GUARD_NAMES, BOUNDARY_NAME, ALLOW_ALL_NAME) if n in policies]
    if not present:
        return
    for missing in sorted({*GUARD_NAMES, BOUNDARY_NAME, ALLOW_ALL_NAME} - set(present)):
        yield "boundary", missing, "is missing beside the others"
    if len(present) < 4:
        return

    def statements(policy):
        return as_list(policies[policy][1].get("Statement") or [])

    allow_all = {"Effect": "Allow", "Action": "*", "Resource": "*"}

    def allows(policy):
        return [{k: v for k, v in st.items() if k != "Sid"} for st in statements(policy) if st.get("Effect") == "Allow"]

    if allows(ALLOW_ALL_NAME) != [allow_all]:
        yield "boundary", policies[ALLOW_ALL_NAME][0], "must allow everything, in one statement, and deny nothing"
    if allows(BOUNDARY_NAME) != [allow_all]:
        yield "boundary", policies[BOUNDARY_NAME][0], "must allow everything in exactly one statement"
    for guard in GUARD_NAMES:
        if allows(guard):
            yield "boundary", policies[guard][0], f"{guard} must only deny"
    guards = set()
    for guard in GUARD_NAMES:
        guards |= _deny_triples(statements(guard))
    every = _deny_triples(statements(BOUNDARY_NAME))
    own = _deny_triples(statements(BOUNDARY_NAME), skip_sid_prefix="HandsOnly")
    for triple in sorted(guards):
        if not any(_covers(b, triple) for b in every):
            yield "boundary", policies[BOUNDARY_NAME][0], f"lacks a guard's deny: {triple[0]} on {triple[1]}"
    for triple in sorted(own):
        if not any(_covers(g, triple) for g in guards):
            yield "boundary", policies[BOUNDARY_NAME][0], f"denies {triple[0]} on {triple[1]}, which no guard does (prefix the Sid HandsOnly if meant)"


def _covers(big, small):
    """Whether the deny triple big denies everything the deny triple small does."""
    action, resource, condition = big
    if not fnmatch.fnmatchcase(small[0].lower(), action.lower()):
        return False
    if resource != json.dumps("*") and resource != small[1]:
        return False
    return condition == "null" or condition == small[2]


class Models:
    """Operation names from the AWS CLI's botocore models, by service."""

    def __init__(self, root):
        self.root = Path(root)
        self._ops = {}

    def ops(self, service):
        if service not in self._ops:
            names = set()
            for model in sorted(glob.glob(str(self.root / service / "*" / "service-2.json")))[-1:]:
                with open(model, encoding="utf-8") as f:
                    names = set(json.load(f).get("operations", {}))
            self._ops[service] = names
        return self._ops[service]


def rule_wildcards(path, template, models=None):
    if models is None:
        return
    for policy, (name, document) in managed_policies(template).items():
        if policy not in (*GUARD_NAMES, BOUNDARY_NAME, DENY_SPEND_NAME):
            continue
        for st in as_list(document.get("Statement") or []):
            if st.get("Effect") != "Deny":
                continue
            for action in as_list(st.get("Action")):
                prefix, _, pattern = action.partition(":")
                services = BOTOCORE_SERVICES.get(prefix)
                if not services or action in IAM_ONLY:
                    continue
                ops = set()
                for service in services:
                    ops |= models.ops(service)
                if not ops:
                    yield "wildcards", name, f"{action}: no botocore model for {prefix}"
                    continue
                matched = [op for op in ops if fnmatch.fnmatchcase(op.lower(), pattern.lower())]
                if not matched:
                    yield "wildcards", name, f"{policy}: {action} names no {prefix} operation"
                reads = sorted(op for op in matched if op.startswith(READ_VERBS))
                if reads:
                    yield "wildcards", name, f"{policy}: {action} also denies reads: {', '.join(reads[:5])}"


def rule_bounded(path, template):
    if stack_name(path) != "theseus-hands":
        return
    for name, res in resources(template):
        if res.get("Type") != "AWS::IAM::Role":
            continue
        boundary = (res.get("Properties") or {}).get("PermissionsBoundary")
        if boundary != {"Fn::ImportValue": {"Fn::Sub": "${FoundationStack}-BoundaryArn"}}:
            yield "bounded", name, "lacks theseus-boundary as its permissions boundary"


def rule_no_ingress(path, template):
    for name, res in resources(template):
        kind = res.get("Type")
        props = res.get("Properties") or {}
        if kind == "AWS::EC2::Subnet" and props.get("MapPublicIpOnLaunch") is not False:
            yield "no-ingress", name, "does not turn MapPublicIpOnLaunch off"
        if kind == "AWS::EC2::SecurityGroup" and props.get("SecurityGroupIngress"):
            yield "no-ingress", name, "has an ingress rule"
        if kind == "AWS::EC2::SecurityGroupIngress":
            yield "no-ingress", name, "is an ingress rule"
        if kind in ("AWS::EC2::InternetGateway", "AWS::EC2::EIP") and "Condition" not in res:
            yield "no-ingress", name, f"{kind} exists unconditionally"


def rule_inline_code(path, template):
    for name, res in resources(template):
        if res.get("Type") == "AWS::Lambda::Function":
            code = ((res.get("Properties") or {}).get("Code") or {}).get("ZipFile")
            if isinstance(code, str) and len(code) > INLINE_CODE_LIMIT:
                yield "inline-code", name, f"inline code is {len(code)} characters, over {INLINE_CODE_LIMIT}"


def rule_metric_filter(path, template):
    for name, res in resources(template):
        if res.get("Type") == "AWS::Logs::MetricFilter":
            pattern = render((res.get("Properties") or {}).get("FilterPattern", ""), template, stack_name(path))
            if len(pattern) > FILTER_PATTERN_LIMIT:
                yield "metric-filter", name, f"its pattern is {len(pattern)} characters, over {FILTER_PATTERN_LIMIT}"


def rule_template_size(path, template):
    size = Path(path).stat().st_size
    if size > TEMPLATE_BODY_LIMIT:
        yield "template-size", stack_name(path), f"is {size} bytes, over the {TEMPLATE_BODY_LIMIT} a TemplateBody takes"


def rule_stack_policy(path, template):
    policy = Path(path).resolve().parent / "stack-policies" / f"{stack_name(path)}.json"
    if not policy.exists():
        return
    with open(policy, encoding="utf-8") as f:
        statements = as_list(json.load(f).get("Statement") or [])
    names = {name for name, _ in resources(template)}
    for st in statements:
        for resource in as_list(st.get("Resource")):
            if resource == "*":
                continue
            logical = resource.removeprefix("LogicalResourceId/")
            if logical not in names:
                yield "stack-policy", policy.name, f"names {logical}, which {Path(path).name} does not define"


# ------------------------------------------------------------------------------------------------
# cfn-lint's schemas, when available, as a cross-check on the tag table
# ------------------------------------------------------------------------------------------------


class Schemas:
    def __init__(self, base):
        self.base = Path(base)
        with open(self.base / "providers" / "us-west-2.json", encoding="utf-8") as f:
            self.providers = json.load(f)

    def disagrees(self, kind, prop):
        digest = self.providers.get(kind)
        if digest is None:
            return f"{kind} is not in cfn-lint's us-west-2 schemas"
        with open(self.base / "resources" / f"{digest}.json", encoding="utf-8") as f:
            schema = json.load(f)
        tagging = schema.get("tagging") or {}
        props = schema.get("properties") or {}
        if prop is None:
            if tagging.get("taggable") or "Tags" in props:
                return f"cfn-lint's schema says {kind} takes tags; the table says it does not"
            return None
        declared = tagging.get("tagProperty")
        if declared and declared != f"/properties/{prop}":
            return f"cfn-lint's schema puts {kind}'s tags in {declared}, not {prop}"
        if prop not in props:
            return f"cfn-lint's schema has no {prop} on {kind}"
        return None


def find_schemas():
    try:
        import cfnlint  # noqa: PLC0415
    except ImportError:
        return None
    base = Path(cfnlint.__file__).resolve().parent / "data" / "schemas"
    return Schemas(base) if (base / "providers" / "us-west-2.json").exists() else None


def find_models():
    """The botocore models of the `aws` on PATH (or THESEUS_BOTOCORE_DATA).

    Not /usr/local/aws-cli/v2/current blindly: on the build machine that is an older install
    (2.9.13, January 2023) than the `aws` on PATH (Homebrew's 2.34.15).
    """
    candidates = []
    if os.environ.get("THESEUS_BOTOCORE_DATA"):
        candidates.append(Path(os.environ["THESEUS_BOTOCORE_DATA"]))
    aws = shutil.which("aws")
    if aws:
        prefix = Path(aws).resolve().parent.parent
        candidates += sorted(prefix.glob("lib/python3*/site-packages/awscli/botocore/data"))
        candidates += sorted(prefix.glob("dist/awscli/botocore/data"))
    for root in candidates:
        if (root / "ec2").is_dir():
            return Models(root)
    return None


RULES = (
    rule_bucket,
    rule_log_group,
    rule_queue,
    rule_topic,
    rule_retain,
    rule_boundary,
    rule_bounded,
    rule_no_ingress,
    rule_inline_code,
    rule_template_size,
    rule_stack_policy,
    rule_metric_filter,
)


def check(paths, schemas=None, models=None, sizes=None):
    """Every violation in the templates at paths, as (path, rule, resource, message)."""
    out = []
    for path in paths:
        template = load(path)
        for rule, resource, message in rule_tags(path, template, schemas):
            out.append((path, rule, resource, message))
        for rule, resource, message in rule_policy_size(path, template, sizes):
            out.append((path, rule, resource, message))
        for rule, resource, message in rule_wildcards(path, template, models):
            out.append((path, rule, resource, message))
        for fn in RULES:
            for rule, resource, message in fn(path, template):
                out.append((path, rule, resource, message))
    return out


def main(argv):
    paths = argv[1:] or sorted(glob.glob(str(INFRA / "theseus-*.yaml")))
    schemas = find_schemas()
    models = find_models()
    if models is None:
        print("rules: the AWS CLI's botocore models were not found; set THESEUS_BOTOCORE_DATA", file=sys.stderr)
        return 2
    sizes = []
    violations = check(paths, schemas=schemas, models=models, sizes=sizes)
    for path, rule, resource, message in violations:
        print(f"{Path(path).name}: {rule}: {resource}: {message}")
    for stack, policy, size in sizes:
        print(f"size: {stack}: {policy}: {size} of {MANAGED_POLICY_LIMIT}")
    print(
        f"rules: {len(paths)} templates, {len(violations)} violations"
        f" (tag table checked against cfn-lint's schemas: {'yes' if schemas else 'no'};"
        f" botocore models: {models.root})"
    )
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
