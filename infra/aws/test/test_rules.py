"""Tests of the offline rules themselves: each rule has a hit and a near miss.

    python3 -m unittest discover -s infra/aws/test -p 'test_*.py'
"""
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import rules  # noqa: E402

TAGS = """
      Tags:
        - Key: "theseus:owner"
          Value: theseus
        - Key: "theseus:stack"
          Value: !Ref AWS::StackName
"""

GOOD_BUCKET = """
  Bucket:
    Type: AWS::S3::Bucket
    DeletionPolicy: Retain
    UpdateReplacePolicy: Retain
    Properties:
      VersioningConfiguration:
        Status: Enabled
      BucketEncryption:
        ServerSideEncryptionConfiguration:
          - ServerSideEncryptionByDefault:
              SSEAlgorithm: aws:kms
              KMSMasterKeyID: alias/k
      PublicAccessBlockConfiguration:
        BlockPublicAcls: true
        BlockPublicPolicy: true
        IgnorePublicAcls: true
        RestrictPublicBuckets: true
      OwnershipControls:
        Rules:
          - ObjectOwnership: BucketOwnerEnforced
""" + TAGS + """
  BucketPolicy:
    Type: AWS::S3::BucketPolicy
    Properties:
      Bucket: !Ref Bucket
      PolicyDocument:
        Version: "2012-10-17"
        Statement:
          - Effect: Deny
            Principal: "*"
            Action: s3:*
            Resource:
              - !GetAtt Bucket.Arn
              - !Sub "${Bucket.Arn}/*"
            Condition:
              Bool:
                aws:SecureTransport: "false"
"""


def guards_template(boundary_extra="", boundary_denies=None, guard_iac_actions=("iam:Create*",)):
    """A template with the four foundation policies; the boundary's denies are given."""
    iac = "\n".join(f"              - {a}" for a in guard_iac_actions)
    denies = boundary_denies if boundary_denies is not None else list(guard_iac_actions) + ["ec2:AllocateAddress"]
    bound = "\n".join(f"              - {a}" for a in denies)
    return f"""
  GuardIac:
    Type: AWS::IAM::ManagedPolicy
    Properties:
      ManagedPolicyName: theseus-guard-iac
      PolicyDocument:
        Version: "2012-10-17"
        Statement:
          - Sid: Iac
            Effect: Deny
            Action:
{iac}
            Resource: "*"
  GuardLimits:
    Type: AWS::IAM::ManagedPolicy
    Properties:
      ManagedPolicyName: theseus-guard-limits
      PolicyDocument:
        Version: "2012-10-17"
        Statement:
          - Sid: Limits
            Effect: Deny
            Action:
              - ec2:AllocateAddress
              - iam:CreateUser
            Resource: "*"
  AllowAll:
    Type: AWS::IAM::ManagedPolicy
    Properties:
      ManagedPolicyName: theseus-allow-all
      PolicyDocument:
        Version: "2012-10-17"
        Statement:
          - Effect: Allow
            Action: "*"
            Resource: "*"
  Boundary:
    Type: AWS::IAM::ManagedPolicy
    Properties:
      ManagedPolicyName: theseus-boundary
      PolicyDocument:
        Version: "2012-10-17"
        Statement:
          - Sid: AllowAll
            Effect: Allow
            Action: "*"
            Resource: "*"
          - Sid: Guards
            Effect: Deny
            Action:
{bound}
            Resource: "*"
{boundary_extra}
"""


class RuleTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)

    def violations(self, resources, name="theseus-test", models=None):
        path = Path(self.dir.name) / f"{name}.yaml"
        path.write_text('AWSTemplateFormatVersion: "2010-09-09"\nResources:' + resources)
        return [(rule, res, msg) for _, rule, res, msg in rules.check([str(path)], models=models)]

    def rules_hit(self, resources, **kw):
        return {rule for rule, _, _ in self.violations(resources, **kw)}

    # tags --------------------------------------------------------------------------------------

    def test_tags_hit_untagged_and_unknown_type(self):
        found = self.violations("""
  Role:
    Type: AWS::IAM::Role
    Properties:
      AssumeRolePolicyDocument: {}
  Param:
    Type: AWS::SSM::Parameter
    Properties:
      Type: String
      Value: x
""")
        messages = [m for r, _, m in found if r == "tags"]
        self.assertIn("lacks theseus:owner = theseus", messages)
        self.assertTrue(any("not in the tag table" in m for m in messages))

    def test_tags_hit_literal_stack_name(self):
        found = self.violations("""
  Queue:
    Type: AWS::SQS::Queue
    Properties:
      SqsManagedSseEnabled: true
      Tags:
        - Key: "theseus:owner"
          Value: theseus
        - Key: "theseus:stack"
          Value: theseus-foundation
""")
        self.assertIn(("tags", "Queue", "lacks theseus:stack = !Ref AWS::StackName"), found)

    def test_tags_near_miss(self):
        self.assertNotIn("tags", self.rules_hit("""
  Queue:
    Type: AWS::SQS::Queue
    Properties:
      SqsManagedSseEnabled: true""" + TAGS))

    def test_tag_table_agrees_with_cfn_lint_schemas(self):
        schemas = rules.find_schemas()
        if schemas is None:
            self.skipTest("cfn-lint is not importable here")
        for kind, prop in rules.TAG_PROPERTY.items():
            self.assertIsNone(schemas.disagrees(kind, prop), kind)

    # buckets -----------------------------------------------------------------------------------

    def test_bucket_near_miss(self):
        self.assertEqual(self.violations(GOOD_BUCKET), [])

    def test_bucket_hits(self):
        bad = (
            GOOD_BUCKET.replace("SSEAlgorithm: aws:kms", "SSEAlgorithm: AES256")
            .replace("Status: Enabled", "Status: Suspended")
            .replace("RestrictPublicBuckets: true", "RestrictPublicBuckets: false")
            .replace('aws:SecureTransport: "false"', 'aws:SecureTransport: "true"')
        )
        messages = [m for r, _, m in self.violations(bad) if r == "bucket"]
        self.assertIn("is not encrypted by default with a KMS key", messages)
        self.assertIn("is not versioned", messages)
        self.assertIn("does not set RestrictPublicBuckets", messages)
        self.assertIn("has no bucket policy refusing requests without TLS", messages)

    def test_bucket_policy_must_cover_the_objects(self):
        bad = GOOD_BUCKET.replace('              - !Sub "${Bucket.Arn}/*"\n', "")
        self.assertIn("bucket", self.rules_hit(bad))

    # log groups, queues, topics, retain ------------------------------------------------------

    def test_log_group(self):
        self.assertIn("log-group", self.rules_hit("""
  Logs:
    Type: AWS::Logs::LogGroup
    DeletionPolicy: Delete
    UpdateReplacePolicy: Delete
    Properties:
      LogGroupName: /x""" + TAGS))
        self.assertNotIn("log-group", self.rules_hit("""
  Logs:
    Type: AWS::Logs::LogGroup
    Properties:
      RetentionInDays: 30""" + TAGS))

    def test_queue(self):
        self.assertIn("queue", self.rules_hit("""
  Queue:
    Type: AWS::SQS::Queue
    Properties:
      QueueName: q""" + TAGS))
        self.assertNotIn("queue", self.rules_hit("""
  Queue:
    Type: AWS::SQS::Queue
    Properties:
      KmsMasterKeyId: alias/k""" + TAGS))

    def test_topic(self):
        self.assertIn("topic", self.rules_hit("""
  Topic:
    Type: AWS::SNS::Topic
    Properties:
      TopicName: t""" + TAGS))
        self.assertNotIn("topic", self.rules_hit("""
  Topic:
    Type: AWS::SNS::Topic
    Properties:
      KmsMasterKeyId: alias/k""" + TAGS))

    def test_retain(self):
        self.assertIn("retain", self.rules_hit(GOOD_BUCKET.replace("DeletionPolicy: Retain", "DeletionPolicy: Delete")))
        self.assertNotIn("retain", self.rules_hit(GOOD_BUCKET))

    # sizes -------------------------------------------------------------------------------------

    def test_policy_size(self):
        many = "\n".join(f"              - ec2:Action{i:05d}" for i in range(400))
        template = f"""
  Big:
    Type: AWS::IAM::ManagedPolicy
    Properties:
      PolicyDocument:
        Version: "2012-10-17"
        Statement:
          - Effect: Deny
            Action:
{many}
            Resource: "*"
"""
        self.assertIn("policy-size", self.rules_hit(template))
        self.assertNotIn("policy-size", self.rules_hit(template.replace(many, "              - ec2:RunInstances")))

    def test_trust_policy_size(self):
        services = "\n".join(f"                  - service{i:03d}.amazonaws.com" for i in range(80))
        self.assertIn("policy-size", self.rules_hit(f"""
  Role:
    Type: AWS::IAM::Role
    Properties:
      AssumeRolePolicyDocument:
        Version: "2012-10-17"
        Statement:
          - Effect: Allow
            Principal:
              Service:
{services}
            Action: sts:AssumeRole""" + TAGS))

    def test_render_takes_the_long_side_of_every_intrinsic(self):
        template = {"Parameters": {"Name": {"Type": "String"}}}
        rendered = rules.render({"Fn::Sub": "arn:${AWS::Partition}:iam::${AWS::AccountId}:user/${Name}"}, template, "s")
        self.assertEqual(rendered, "arn:aws-us-gov:iam::123456789012:user/" + "x" * 64)

    # the boundary ------------------------------------------------------------------------------

    def test_boundary_near_miss_and_coverage(self):
        # iam:CreateUser (guard-limits) is covered by the boundary's iam:Create*.
        self.assertNotIn("boundary", self.rules_hit(guards_template()))

    def test_boundary_lacks_a_guard_deny(self):
        found = self.violations(guards_template(boundary_denies=["iam:Create*"]))
        self.assertTrue(any(r == "boundary" and "lacks a guard's deny: ec2:AllocateAddress" in m for r, _, m in found))

    def test_boundary_narrower_pattern_does_not_cover(self):
        found = self.violations(guards_template(boundary_denies=["iam:CreateUser", "ec2:AllocateAddress"]))
        self.assertTrue(any("lacks a guard's deny: iam:Create*" in m for r, _, m in found if r == "boundary"))

    def test_boundary_extra_deny(self):
        extra = """
          - Sid: Extra
            Effect: Deny
            Action: iam:PassRole
            Resource: "*"
"""
        self.assertIn("boundary", self.rules_hit(guards_template(boundary_extra=extra)))
        self.assertNotIn("boundary", self.rules_hit(guards_template(boundary_extra=extra.replace("Sid: Extra", "Sid: HandsOnlyExtra"))))

    def test_guard_must_only_deny(self):
        template = guards_template().replace(
            "          - Sid: Limits\n            Effect: Deny",
            "          - Sid: Limits\n            Effect: Allow",
        )
        found = self.violations(template)
        self.assertTrue(any(m == "theseus-guard-limits must only deny" for r, _, m in found if r == "boundary"))

    # wildcards ---------------------------------------------------------------------------------

    def fake_models(self):
        root = Path(self.dir.name) / "models"
        (root / "ec2" / "2016-11-15").mkdir(parents=True)
        ops = {"operations": {name: {} for name in ("CreateVpc", "DeleteVpc", "DescribeVpcs", "RunInstances")}}
        (root / "ec2" / "2016-11-15" / "service-2.json").write_text(json.dumps(ops))
        return rules.Models(root)

    def deny_spend(self, action):
        return f"""
  DenySpend:
    Type: AWS::IAM::ManagedPolicy
    Properties:
      ManagedPolicyName: theseus-deny-spend
      PolicyDocument:
        Version: "2012-10-17"
        Statement:
          - Effect: Deny
            Action:
              - {action}
              - iam:PassRole
            Resource: "*"
"""

    def test_wildcards(self):
        models = self.fake_models()
        found = self.violations(self.deny_spend("ec2:*Vpc*"), models=models)
        self.assertTrue(any("also denies reads: DescribeVpcs" in m for r, _, m in found if r == "wildcards"))
        found = self.violations(self.deny_spend("ec2:CreateVpcz"), models=models)
        self.assertTrue(any("names no ec2 operation" in m for r, _, m in found if r == "wildcards"))
        self.assertNotIn("wildcards", self.rules_hit(self.deny_spend("ec2:*Vpc"), models=models))
        self.assertNotIn("wildcards", self.rules_hit(self.deny_spend("ec2:RunInstances"), models=models))

    # roles, ingress, inline code ---------------------------------------------------------------

    def test_bounded(self):
        role = """
  Hand:
    Type: AWS::IAM::Role
    Properties:
      AssumeRolePolicyDocument: {}
      PermissionsBoundary:
        Fn::ImportValue: !Sub "${FoundationStack}-BoundaryArn\"""" + TAGS
        self.assertNotIn("bounded", self.rules_hit(role, name="theseus-hands"))
        unbounded = role.replace('      PermissionsBoundary:\n        Fn::ImportValue: !Sub "${FoundationStack}-BoundaryArn"\n', "")
        self.assertIn("bounded", self.rules_hit(unbounded, name="theseus-hands"))
        self.assertNotIn("bounded", self.rules_hit(unbounded, name="theseus-posture"))

    def test_no_ingress(self):
        found = self.violations("""
  Subnet:
    Type: AWS::EC2::Subnet
    Properties:
      VpcId: v
      MapPublicIpOnLaunch: true""" + TAGS + """
  Group:
    Type: AWS::EC2::SecurityGroup
    Properties:
      GroupDescription: g
      SecurityGroupIngress:
        - CidrIp: 10.0.0.0/8
          IpProtocol: tcp
          FromPort: 22
          ToPort: 22""" + TAGS + """
  Gateway:
    Type: AWS::EC2::InternetGateway
    Properties:""" + TAGS)
        hits = {res for r, res, _ in found if r == "no-ingress"}
        self.assertEqual(hits, {"Subnet", "Group", "Gateway"})
        self.assertNotIn("no-ingress", self.rules_hit("""
  Gateway:
    Type: AWS::EC2::InternetGateway
    Condition: NatEnabled
    Properties:""" + TAGS))

    def test_template_size(self):
        queue = """
  Queue:
    Type: AWS::SQS::Queue
    Properties:
      SqsManagedSseEnabled: true""" + TAGS
        self.assertIn("template-size", self.rules_hit(queue + "\n# " + "x" * rules.TEMPLATE_BODY_LIMIT))
        self.assertNotIn("template-size", self.rules_hit(queue))

    def test_stack_policy(self):
        policies = Path(self.dir.name) / "stack-policies"
        policies.mkdir()
        deny = {"Effect": "Deny", "Action": "Update:Delete", "Principal": "*"}
        deny["Resource"] = ["LogicalResourceId/Queue", "LogicalResourceId/Gone"]
        (policies / "theseus-test.json").write_text(json.dumps({"Statement": [deny]}))
        found = [v for v in self.violations("""
  Queue:
    Type: AWS::SQS::Queue
    Properties:
      SqsManagedSseEnabled: true""" + TAGS) if v[0] == "stack-policy"]
        self.assertEqual(found, [("stack-policy", "theseus-test.json", "names Gone, which theseus-test.yaml does not define")])

    def test_metric_filter(self):
        long_pattern = "{ " + " || ".join(f"($.eventName = Event{i:04d})" for i in range(40)) + " }"
        template = """
  Filter:
    Type: AWS::Logs::MetricFilter
    Properties:
      LogGroupName: g
      FilterPattern: "PATTERN"
      MetricTransformations:
        - MetricNamespace: n
          MetricName: m
          MetricValue: "1"
"""
        self.assertIn("metric-filter", self.rules_hit(template.replace("PATTERN", long_pattern)))
        self.assertNotIn("metric-filter", self.rules_hit(template.replace("PATTERN", "{ $.eventName = X }")))

    def test_inline_code(self):
        body = "x = 1\\n" * 700
        function = """
  Fn:
    Type: AWS::Lambda::Function
    Properties:
      Role: r
      Code:
        ZipFile: "BODY\"""" + TAGS
        self.assertIn("inline-code", self.rules_hit(function.replace("BODY", body)))
        self.assertNotIn("inline-code", self.rules_hit(function.replace("BODY", "x = 1")))


if __name__ == "__main__":
    unittest.main()
