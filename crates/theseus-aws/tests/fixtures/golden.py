"""Generates golden.json: requests and responses for theseus-aws's protocol
tests, from the AWS CLI's own botocore (the reference implementation),
offline.

Each case's request is what botocore sends for the input: its client is
created with fixed credentials and stopped at `before-send`, which returns a
canned answer, so nothing leaves the machine. A case with a `response` also
holds botocore's protocol parser's reading of that answer (the parser alone,
without the handlers that rewrite results, such as IAM's policy decoding).

Run with the CLI's own Python, from this directory:

    <aws-cli>/libexec/bin/python3 golden.py > golden.json

The CLI's version is recorded in the file. The names in it are invented
(the `111122223333` account is AWS's documentation example).
"""
import base64
import datetime
import json
import urllib.parse

from awscli import __version__ as awscli_version
from awscli.botocore import __version__ as botocore_version
from awscli.botocore import xform_name
from awscli.botocore.awsrequest import AWSResponse, HeadersDict
from awscli.botocore.config import Config
from awscli.botocore.parsers import create_parser
from awscli.botocore.session import get_session

ACCESS_KEY = "AKIDEXAMPLE"
SECRET_KEY = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY"

# Request headers that vary per send or that the client and botocore choose
# differently (both valid): the signature, the time, the user agent, the
# length, S3's payload hash and checksums, and S3's `Expect`.
SKIP_HEADERS = {
    "authorization", "x-amz-date", "user-agent", "x-amz-user-agent",
    "amz-sdk-invocation-id", "amz-sdk-request", "content-length", "expect",
    "x-amz-security-token", "x-amzn-trace-id", "x-amz-content-sha256",
    "content-md5", "x-amz-sdk-checksum-algorithm",
}

REQ = "4ba1c0de-0000-4000-8000-00000000000"

CASES = [
    # ---- query (awsQuery) ------------------------------------------------
    {
        "name": "query: iam ListRoles",
        "service": "iam", "operation": "ListRoles",
        "input": {"PathPrefix": "/theseus/", "MaxItems": 10},
        "response": {"status": 200, "headers": {"x-amzn-requestid": REQ + "1"}, "body": """<ListRolesResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">
  <ListRolesResult>
    <IsTruncated>true</IsTruncated>
    <Marker>marker-2</Marker>
    <Roles>
      <member>
        <Path>/theseus/</Path>
        <RoleName>example-hand-basic</RoleName>
        <RoleId>AROAEXAMPLEID00000001</RoleId>
        <Arn>arn:aws:iam::111122223333:role/theseus/example-hand-basic</Arn>
        <CreateDate>2026-09-30T23:45:00Z</CreateDate>
        <AssumeRolePolicyDocument>%7B%22Version%22%3A%222012-10-17%22%7D</AssumeRolePolicyDocument>
        <MaxSessionDuration>43200</MaxSessionDuration>
        <Tags><member><Key>theseus:deployment</Key><Value>example-desktop</Value></member></Tags>
      </member>
      <member>
        <Path>/theseus/</Path>
        <RoleName>example-hand-read</RoleName>
        <RoleId>AROAEXAMPLEID00000002</RoleId>
        <Arn>arn:aws:iam::111122223333:role/theseus/example-hand-read</Arn>
        <CreateDate>2026-09-30T23:46:01.5Z</CreateDate>
        <MaxSessionDuration>3600</MaxSessionDuration>
      </member>
    </Roles>
  </ListRolesResult>
  <ResponseMetadata><RequestId>4ba1c0de-0000-4000-8000-000000000001</RequestId></ResponseMetadata>
</ListRolesResponse>"""},
    },
    {
        "name": "query: iam TagRole, a list of structures",
        "service": "iam", "operation": "TagRole",
        "input": {"RoleName": "example-hand-basic", "Tags": [
            {"Key": "theseus:execution", "Value": "exe_example"},
            {"Key": "team", "Value": "R&D / tools"}]},
    },
    {
        "name": "query: sns CreateTopic, a map and an empty list",
        "service": "sns", "operation": "CreateTopic",
        "input": {"Name": "example-alerts", "Attributes": {"DisplayName": "Example alerts", "FifoTopic": "false"}, "Tags": []},
    },
    {
        "name": "query: sns GetTopicAttributes, a map in the answer",
        "service": "sns", "operation": "GetTopicAttributes",
        "input": {"TopicArn": "arn:aws:sns:us-west-2:111122223333:example-alerts"},
        "response": {"status": 200, "headers": {}, "body": """<GetTopicAttributesResponse xmlns="http://sns.amazonaws.com/doc/2010-03-31/">
  <GetTopicAttributesResult>
    <Attributes>
      <entry><key>DisplayName</key><value>Example alerts</value></entry>
      <entry><key>SubscriptionsConfirmed</key><value>2</value></entry>
    </Attributes>
  </GetTopicAttributesResult>
  <ResponseMetadata><RequestId>4ba1c0de-0000-4000-8000-000000000002</RequestId></ResponseMetadata>
</GetTopicAttributesResponse>"""},
    },
    {
        "name": "query: sts GetCallerIdentity",
        "service": "sts", "operation": "GetCallerIdentity", "input": {},
        "response": {"status": 200, "headers": {}, "body": """<GetCallerIdentityResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <GetCallerIdentityResult>
    <Arn>arn:aws:iam::111122223333:user/example-user</Arn>
    <UserId>AIDAEXAMPLEUSERID0001</UserId>
    <Account>111122223333</Account>
  </GetCallerIdentityResult>
  <ResponseMetadata><RequestId>4ba1c0de-0000-4000-8000-000000000003</RequestId></ResponseMetadata>
</GetCallerIdentityResponse>"""},
    },
    {
        "name": "query: cloudformation DescribeStacks",
        "service": "cloudformation", "operation": "DescribeStacks",
        "input": {"StackName": "example-foundation"},
        "response": {"status": 200, "headers": {}, "body": """<DescribeStacksResponse xmlns="http://cloudformation.amazonaws.com/doc/2010-05-15/">
  <DescribeStacksResult>
    <Stacks>
      <member>
        <StackId>arn:aws:cloudformation:us-west-2:111122223333:stack/example-foundation/00000000-0000-4000-8000-000000000000</StackId>
        <StackName>example-foundation</StackName>
        <CreationTime>2026-09-30T23:45:00.123Z</CreationTime>
        <StackStatus>CREATE_COMPLETE</StackStatus>
        <DisableRollback>false</DisableRollback>
        <Parameters><member><ParameterKey>Budget</ParameterKey><ParameterValue>50</ParameterValue></member></Parameters>
        <Outputs><member><OutputKey>QueueUrl</OutputKey><OutputValue>https://sqs.us-west-2.amazonaws.com/111122223333/example-done</OutputValue></member></Outputs>
        <Tags/>
      </member>
    </Stacks>
  </DescribeStacksResult>
  <ResponseMetadata><RequestId>4ba1c0de-0000-4000-8000-000000000004</RequestId></ResponseMetadata>
</DescribeStacksResponse>"""},
    },
    {
        "name": "query: iam GetRole, an error",
        "service": "iam", "operation": "GetRole", "input": {"RoleName": "example-missing"},
        "response": {"status": 404, "headers": {}, "body": """<ErrorResponse xmlns="https://iam.amazonaws.com/doc/2010-05-08/">
  <Error><Type>Sender</Type><Code>NoSuchEntity</Code><Message>The role with name example-missing cannot be found.</Message></Error>
  <RequestId>4ba1c0de-0000-4000-8000-000000000005</RequestId>
</ErrorResponse>"""},
    },
    # ---- ec2 ---------------------------------------------------------------
    {
        "name": "ec2: DescribeRegions",
        "service": "ec2", "operation": "DescribeRegions",
        "input": {"AllRegions": True, "RegionNames": ["us-west-2", "us-east-1"]},
        "response": {"status": 200, "headers": {}, "body": """<DescribeRegionsResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <requestId>4ba1c0de-0000-4000-8000-000000000006</requestId>
  <regionInfo>
    <item><regionName>us-west-2</regionName><regionEndpoint>ec2.us-west-2.amazonaws.com</regionEndpoint><optInStatus>opt-in-not-required</optInStatus></item>
    <item><regionName>us-east-1</regionName><regionEndpoint>ec2.us-east-1.amazonaws.com</regionEndpoint><optInStatus>opt-in-not-required</optInStatus></item>
  </regionInfo>
</DescribeRegionsResponse>"""},
    },
    {
        "name": "ec2: DescribeInstances, nested lists",
        "service": "ec2", "operation": "DescribeInstances",
        "input": {"Filters": [{"Name": "tag:theseus:deployment", "Values": ["example-desktop", "example-laptop"]},
                              {"Name": "instance-state-name", "Values": ["running"]}],
                  "MaxResults": 5},
        "response": {"status": 200, "headers": {}, "body": """<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <requestId>4ba1c0de-0000-4000-8000-000000000007</requestId>
  <reservationSet>
    <item>
      <reservationId>r-0example000000001</reservationId>
      <ownerId>111122223333</ownerId>
      <instancesSet>
        <item>
          <instanceId>i-0example000000001</instanceId>
          <imageId>ami-0example000000001</imageId>
          <instanceState><code>16</code><name>running</name></instanceState>
          <instanceType>t3.micro</instanceType>
          <launchTime>2026-09-30T23:45:00.000Z</launchTime>
          <ebsOptimized>false</ebsOptimized>
          <tagSet><item><key>theseus:deployment</key><value>example-desktop</value></item></tagSet>
        </item>
      </instancesSet>
    </item>
  </reservationSet>
  <nextToken>token-2</nextToken>
</DescribeInstancesResponse>"""},
    },
    {
        "name": "ec2: RunInstances, with its token",
        "service": "ec2", "operation": "RunInstances",
        "input": {"ImageId": "ami-0example000000001", "MinCount": 1, "MaxCount": 1,
                  "InstanceType": "t3.micro", "ClientToken": "exe_example.call_1",
                  "TagSpecifications": [{"ResourceType": "instance", "Tags": [{"Key": "theseus:ttl", "Value": "1h"}]}]},
    },
    {
        "name": "ec2: an error with its enforcer",
        "service": "ec2", "operation": "DescribeVpcs", "input": {},
        "response": {"status": 403, "headers": {}, "body": """<?xml version="1.0" encoding="UTF-8"?>
<Response><Errors><Error><Code>UnauthorizedOperation</Code><Message>You are not authorized to perform this operation. User: arn:aws:sts::111122223333:assumed-role/example-owner/exe_example is not authorized to perform: ec2:DescribeVpcs with an explicit deny in a service control policy</Message></Error></Errors><RequestID>4ba1c0de-0000-4000-8000-000000000008</RequestID></Response>"""},
    },
    # ---- json 1.0 ----------------------------------------------------------
    {
        "name": "json 1.0: dynamodb Query, a union with a blob",
        "service": "dynamodb", "operation": "Query",
        "input": {"TableName": "example-durability", "KeyConditionExpression": "pk = :v",
                  "ExpressionAttributeValues": {":v": {"S": "exe_example"}}, "Limit": 5},
        "response": {"status": 200, "headers": {"x-amzn-requestid": "DDB0EXAMPLE0000000000000000000000000000000000000000001"}, "body": json.dumps({
            "Count": 1, "ScannedCount": 1,
            "Items": [{"pk": {"S": "exe_example"}, "n": {"N": "42"}, "raw": {"B": "AAEC"}, "ok": {"BOOL": True},
                       "tags": {"SS": ["a", "b"]}, "doc": {"M": {"k": {"S": "v"}}}}],
            "LastEvaluatedKey": {"pk": {"S": "exe_example"}}})},
    },
    {
        "name": "json 1.0: sqs SendMessage, query-compatible",
        "service": "sqs", "operation": "SendMessage",
        "input": {"QueueUrl": "https://sqs.us-west-2.amazonaws.com/111122223333/example-done",
                  "MessageBody": "{\"job\":\"j1\"}", "DelaySeconds": 0,
                  "MessageAttributes": {"kind": {"DataType": "String", "StringValue": "completion"}}},
        "response": {"status": 200, "headers": {"x-amzn-requestid": "00000000-0000-4000-8000-0000000000aa"},
                     "body": json.dumps({"MD5OfMessageBody": "0123456789abcdef0123456789abcdef", "MessageId": "00000000-0000-4000-8000-0000000000ab"})},
    },
    {
        "name": "json 1.0: sqs, a query-compatible error",
        "service": "sqs", "operation": "GetQueueUrl", "input": {"QueueName": "example-missing"},
        "response": {"status": 400, "headers": {"x-amzn-requestid": "00000000-0000-4000-8000-0000000000ac",
                                                "x-amzn-query-error": "AWS.SimpleQueueService.NonExistentQueue;Sender"},
                     "body": json.dumps({"__type": "com.amazonaws.sqs#QueueDoesNotExist", "message": "The specified queue does not exist."})},
    },
    {
        "name": "json 1.0: dynamodb, an error",
        "service": "dynamodb", "operation": "DescribeTable", "input": {"TableName": "example-missing"},
        "response": {"status": 400, "headers": {"x-amzn-requestid": "DDB0EXAMPLE0000000000000000000000000000000000000000002"},
                     "body": json.dumps({"__type": "com.amazonaws.dynamodb.v20120810#ResourceNotFoundException", "message": "Requested resource not found: Table: example-missing not found"})},
    },
    # ---- json 1.1 ----------------------------------------------------------
    {
        "name": "json 1.1: sagemaker ListEndpoints, epoch timestamps",
        "service": "sagemaker", "operation": "ListEndpoints",
        "input": {"MaxResults": 2, "SortBy": "CreationTime", "CreationTimeAfter": "2026-01-01T00:00:00Z"},
        "response": {"status": 200, "headers": {"x-amzn-requestid": "00000000-0000-4000-8000-0000000000b0"}, "body": json.dumps({
            "Endpoints": [{"EndpointName": "example-embed", "EndpointArn": "arn:aws:sagemaker:us-west-2:111122223333:endpoint/example-embed",
                           "CreationTime": 1790000000.123, "LastModifiedTime": 1790000100, "EndpointStatus": "InService"}],
            "NextToken": "page-2"})},
    },
    {
        "name": "json 1.1: logs FilterLogEvents",
        "service": "logs", "operation": "FilterLogEvents",
        "input": {"logGroupName": "/theseus/example", "startTime": 1790000000000, "filterPattern": "ERROR", "limit": 2},
        "response": {"status": 200, "headers": {}, "body": json.dumps({
            "events": [{"logStreamName": "s1", "timestamp": 1790000000123, "message": "ERROR one", "ingestionTime": 1790000000200, "eventId": "e1"}],
            "searchedLogStreams": [], "nextToken": "t2"})},
    },
    {
        "name": "json 1.1: kms Encrypt, a blob both ways",
        "service": "kms", "operation": "Encrypt",
        "input": {"KeyId": "alias/example", "Plaintext": "hello", "EncryptionContext": {"purpose": "test"}},
        "response": {"status": 200, "headers": {}, "body": json.dumps({
            "CiphertextBlob": base64.b64encode(bytes(range(16))).decode(), "KeyId": "arn:aws:kms:us-west-2:111122223333:key/00000000-0000-4000-8000-000000000000",
            "EncryptionAlgorithm": "SYMMETRIC_DEFAULT"})},
    },
    {
        "name": "json 1.1: an implicit deny",
        "service": "ecs", "operation": "ListClusters", "input": {},
        "response": {"status": 400, "headers": {"x-amzn-requestid": "00000000-0000-4000-8000-0000000000b1"}, "body": json.dumps({
            "__type": "AccessDeniedException",
            "Message": "User: arn:aws:sts::111122223333:assumed-role/example-owner/exe_example is not authorized to perform: ecs:ListClusters because no identity-based policy allows the ecs:ListClusters action"})},
    },
    # ---- rest-json ---------------------------------------------------------
    {
        "name": "rest-json: lambda ListFunctions",
        "service": "lambda", "operation": "ListFunctions",
        "input": {"MaxItems": 10, "FunctionVersion": "ALL"},
        "response": {"status": 200, "headers": {"x-amzn-requestid": "00000000-0000-4000-8000-0000000000c0"}, "body": json.dumps({
            "Functions": [{"FunctionName": "example-reaper", "FunctionArn": "arn:aws:lambda:us-west-2:111122223333:function:example-reaper",
                           "Runtime": "provided.al2023", "MemorySize": 128, "Timeout": 30, "LastModified": "2026-09-30T23:45:00.000+0000",
                           "Architectures": ["arm64"], "Environment": {"Variables": {"STAGE": "test"}}}],
            "NextMarker": "m2"})},
    },
    {
        "name": "rest-json: lambda Invoke, headers and a raw payload",
        "service": "lambda", "operation": "Invoke",
        "input": {"FunctionName": "example-reaper", "InvocationType": "RequestResponse", "LogType": "Tail",
                  "Qualifier": "live", "Payload": "{\"dry_run\": true}"},
        "response": {"status": 200, "headers": {"x-amzn-requestid": "00000000-0000-4000-8000-0000000000c1",
                                                "X-Amz-Executed-Version": "7", "X-Amz-Function-Error": "Unhandled",
                                                "X-Amz-Log-Result": base64.b64encode(b"START RequestId: x\n").decode()},
                     "body": "{\"errorMessage\": \"boom\"}"},
    },
    {
        "name": "rest-json: eks DescribeCluster",
        "service": "eks", "operation": "DescribeCluster", "input": {"name": "example-cluster"},
        "response": {"status": 200, "headers": {}, "body": json.dumps({"cluster": {
            "name": "example-cluster", "arn": "arn:aws:eks:us-west-2:111122223333:cluster/example-cluster",
            "createdAt": 1790000000.5, "version": "1.33", "status": "ACTIVE", "tags": {"theseus:deployment": "example-desktop"}}})},
    },
    {
        "name": "rest-json: an error in the header",
        "service": "lambda", "operation": "GetFunction", "input": {"FunctionName": "example-missing"},
        "response": {"status": 404, "headers": {"x-amzn-requestid": "00000000-0000-4000-8000-0000000000c2",
                                                "x-amzn-ErrorType": "ResourceNotFoundException:http://internal.example/"},
                     "body": json.dumps({"Type": "User", "Message": "Function not found: arn:aws:lambda:us-west-2:111122223333:function:example-missing"})},
    },
    # ---- rest-xml ----------------------------------------------------------
    {
        "name": "rest-xml: s3 ListObjectsV2, flattened lists",
        "service": "s3", "operation": "ListObjectsV2",
        "input": {"Bucket": "example-theseus-bucket", "Prefix": "logs/", "Delimiter": "/", "MaxKeys": 2},
        "response": {"status": 200, "headers": {"x-amz-request-id": "EXAMPLE0000000001", "x-amz-id-2": "host-id"}, "body": """<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Name>example-theseus-bucket</Name><Prefix>logs/</Prefix><KeyCount>3</KeyCount><MaxKeys>2</MaxKeys><Delimiter>/</Delimiter>
  <IsTruncated>true</IsTruncated><NextContinuationToken>token-2</NextContinuationToken>
  <Contents><Key>logs/a.txt</Key><LastModified>2026-09-30T23:45:00.000Z</LastModified><ETag>"0123456789abcdef0123456789abcdef"</ETag><Size>12</Size><StorageClass>STANDARD</StorageClass></Contents>
  <Contents><Key>logs/b &amp; c.txt</Key><LastModified>2026-09-30T23:46:00.000Z</LastModified><ETag>"fedcba9876543210fedcba9876543210"</ETag><Size>0</Size><StorageClass>STANDARD</StorageClass></Contents>
  <CommonPrefixes><Prefix>logs/2026/</Prefix></CommonPrefixes>
</ListBucketResult>"""},
    },
    {
        "name": "rest-xml: s3 ListBuckets",
        "service": "s3", "operation": "ListBuckets", "input": {},
        "response": {"status": 200, "headers": {"x-amz-request-id": "EXAMPLE0000000002"}, "body": """<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Owner><ID>0123456789abcdef</ID></Owner>
  <Buckets>
    <Bucket><Name>example-theseus-bucket</Name><CreationDate>2026-09-30T23:45:00.000Z</CreationDate><BucketRegion>us-west-2</BucketRegion></Bucket>
    <Bucket><Name>example.dotted.bucket</Name><CreationDate>2026-01-02T03:04:05.000Z</CreationDate></Bucket>
  </Buckets>
</ListAllMyBucketsResult>"""},
    },
    {
        "name": "rest-xml: s3 PutObject, a raw body and headers",
        "service": "s3", "operation": "PutObject",
        "input": {"Bucket": "example-theseus-bucket", "Key": "out/a b+c.txt", "Body": "hello",
                  "ContentType": "text/plain", "Metadata": {"theseus-call": "call_1"}},
        "response": {"status": 200, "headers": {"x-amz-request-id": "EXAMPLE0000000003", "ETag": "\"5d41402abc4b2a76b9719d911017c592\"",
                                                "x-amz-version-id": "v1", "x-amz-server-side-encryption": "AES256"}, "body": ""},
    },
    {
        "name": "rest-xml: s3 HeadObject, headers into members",
        "service": "s3", "operation": "HeadObject",
        "input": {"Bucket": "example-theseus-bucket", "Key": "out/a.txt"},
        "response": {"status": 200, "headers": {"x-amz-request-id": "EXAMPLE0000000004", "Content-Length": "12",
                                                "Last-Modified": "Mon, 21 Sep 2026 14:13:20 GMT", "ETag": "\"abc\"",
                                                "Content-Type": "text/plain", "x-amz-meta-theseus-call": "call_1",
                                                "x-amz-server-side-encryption-bucket-key-enabled": "true"}, "body": ""},
    },
    {
        "name": "rest-xml: s3 PutBucketTagging, an XML payload",
        "service": "s3", "operation": "PutBucketTagging",
        "input": {"Bucket": "example-theseus-bucket", "Tagging": {"TagSet": [
            {"Key": "theseus:deployment", "Value": "example-desktop"}, {"Key": "cost", "Value": "lab"}]}},
    },
    {
        "name": "rest-xml: s3 DeleteObjects, a flattened list in a payload",
        "service": "s3", "operation": "DeleteObjects",
        "input": {"Bucket": "example-theseus-bucket", "Delete": {"Objects": [{"Key": "a.txt"}, {"Key": "b.txt", "VersionId": "v2"}], "Quiet": True}},
    },
    {
        "name": "rest-xml: route53 ListHostedZones",
        "service": "route53", "operation": "ListHostedZones", "input": {"MaxItems": "10"},
        "response": {"status": 200, "headers": {"x-amzn-requestid": "00000000-0000-4000-8000-0000000000d0"}, "body": """<?xml version="1.0"?>
<ListHostedZonesResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <HostedZones>
    <HostedZone><Id>/hostedzone/Z0EXAMPLE00001</Id><Name>example.test.</Name><CallerReference>ref-1</CallerReference>
      <Config><Comment>lab</Comment><PrivateZone>false</PrivateZone></Config><ResourceRecordSetCount>4</ResourceRecordSetCount></HostedZone>
  </HostedZones>
  <IsTruncated>false</IsTruncated><MaxItems>10</MaxItems>
</ListHostedZonesResponse>"""},
    },
    {
        "name": "rest-xml: route53 ChangeResourceRecordSets, a root from the input",
        "service": "route53", "operation": "ChangeResourceRecordSets",
        "input": {"HostedZoneId": "Z0EXAMPLE00001", "ChangeBatch": {"Comment": "lab", "Changes": [{"Action": "UPSERT", "ResourceRecordSet": {
            "Name": "x.example.test.", "Type": "A", "TTL": 300, "ResourceRecords": [{"Value": "192.0.2.10"}]}}]}},
    },
    {
        "name": "rest-xml: s3, an error",
        "service": "s3", "operation": "ListObjectsV2", "input": {"Bucket": "example-missing-bucket"},
        "response": {"status": 404, "headers": {"x-amz-request-id": "EXAMPLE0000000005"}, "body": """<?xml version="1.0" encoding="UTF-8"?>
<Error><Code>NoSuchBucket</Code><Message>The specified bucket does not exist</Message><BucketName>example-missing-bucket</BucketName><RequestId>EXAMPLE0000000005</RequestId><HostId>h</HostId></Error>"""},
    },
    {
        "name": "rest-xml: s3 HeadObject, an error with no body",
        "service": "s3", "operation": "HeadObject", "input": {"Bucket": "example-theseus-bucket", "Key": "missing.txt"},
        "response": {"status": 404, "headers": {"x-amz-request-id": "EXAMPLE0000000006"}, "body": ""},
    },
    {
        "name": "rest-xml: route53, an error",
        "service": "route53", "operation": "GetHostedZone", "input": {"Id": "Z0EXAMPLE00009"},
        "response": {"status": 404, "headers": {}, "body": """<?xml version="1.0"?>
<ErrorResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/"><Error><Type>Sender</Type><Code>NoSuchHostedZone</Code><Message>No hosted zone found with ID: Z0EXAMPLE00009</Message></Error><RequestId>00000000-0000-4000-8000-0000000000d1</RequestId></ErrorResponse>"""},
    },
]


class Raw:
    """A canned body, readable the ways botocore reads one."""

    def __init__(self, body):
        self.body = body
        self.pos = 0

    def stream(self, *args, **kwargs):
        yield self.body

    def read(self, amt=None):
        if amt is None:
            out = self.body[self.pos:]
        else:
            out = self.body[self.pos:self.pos + amt]
        self.pos += len(out)
        return out


def blob(b):
    """The client's convention: printable UTF-8 text, else base64."""
    try:
        s = b.decode("utf-8")
        if all(c in "\n\r\t" or ord(c) >= 0x20 and ord(c) != 0x7f for c in s):
            return s
    except UnicodeDecodeError:
        pass
    return {"base64": base64.b64encode(b).decode()}


def norm(v):
    if isinstance(v, dict):
        return {k: norm(x) for k, x in v.items() if k != "ResponseMetadata"}
    if isinstance(v, list):
        return [norm(x) for x in v]
    if isinstance(v, datetime.datetime):
        u = v.astimezone(datetime.timezone.utc)
        s = u.strftime("%Y-%m-%dT%H:%M:%S")
        if u.microsecond:
            s += ("." + "%06d" % u.microsecond).rstrip("0")
        return s + "Z"
    if isinstance(v, (bytes, bytearray)):
        return blob(bytes(v))
    if hasattr(v, "read"):
        return blob(v.read())
    return v


def body_of(req, protocol):
    b = req.body
    if b is None:
        return None
    if hasattr(b, "read"):
        b = b.read()
    if isinstance(b, str):
        b = b.encode()
    if not b:
        return None
    if protocol in ("query", "ec2"):
        return {"form": urllib.parse.parse_qsl(b.decode(), keep_blank_values=True)}
    if protocol in ("json", "rest-json") and b[:1] in (b"{", b"["):
        try:
            return {"json": json.loads(b)}
        except ValueError:
            pass
    if b.lstrip()[:1] == b"<":
        return {"xml": b.decode()}
    return {"raw": blob(b)}


def run(case):
    session = get_session()
    cfg = Config(retries={"max_attempts": 1}, request_checksum_calculation="when_required",
                 response_checksum_validation="when_required")
    client = session.create_client(case["service"], region_name=case.get("region", "us-west-2"),
                                   aws_access_key_id=ACCESS_KEY, aws_secret_access_key=SECRET_KEY, config=cfg)
    # botocore's own choice, not the protocol's: S3's automatic EncodingType.
    # (The CLI's botocore registers its handlers as `botocore.handlers`, so
    # they are found by name.)
    for op in ("ListObjects", "ListObjectsV2", "ListObjectVersions"):
        event = f"before-parameter-build.s3.{op}"
        for h in list(client.meta.events._handlers.prefix_search(event)):
            if getattr(h, "__name__", "") == "set_list_objects_encoding_type_url":
                client.meta.events.unregister(event, h)
    model = client.meta.service_model
    protocol = model.protocol
    captured = {}
    resp = case.get("response") or {"status": 200, "headers": {}, "body": ""}

    def before_send(request, **kwargs):
        captured["method"] = request.method
        captured["url"] = request.url
        captured["headers"] = {k: (v.decode() if isinstance(v, bytes) else v)
                               for k, v in request.headers.items() if k.lower() not in SKIP_HEADERS
                               and not k.lower().startswith("x-amz-checksum-")}
        captured["body"] = body_of(request, protocol)
        # The client reads this; the case's answer is read by the parser
        # alone, below.
        return AWSResponse(request.url, 200, {}, Raw(resp["body"].encode()))

    client.meta.events.register("before-send", before_send)
    try:
        getattr(client, xform_name(case["operation"]))(**case["input"])
    except Exception as e:  # after the send, the canned answer may not suit the client
        if "url" not in captured:
            raise RuntimeError(f"{case['name']}: no request was sent: {e}") from e
    split = urllib.parse.urlsplit(captured["url"])
    out = {
        "name": case["name"],
        "service": case["service"],
        "operation": case["operation"],
        "region": case.get("region", "us-west-2"),
        "protocol": protocol,
        "input": case["input"],
        "request": {
            "method": captured["method"],
            "origin": f"{split.scheme}://{split.netloc}",
            "path": split.path,
            "query": urllib.parse.parse_qsl(split.query, keep_blank_values=True),
            "headers": captured["headers"],
            "body": captured["body"],
        },
    }
    if case.get("response"):
        out["response"] = resp
        parser = create_parser(protocol)
        op_model = model.operation_model(case["operation"])
        # Headers are case-insensitive, as botocore's own HeadersDict is.
        parsed = parser.parse({"status_code": resp["status"], "headers": HeadersDict(resp["headers"]),
                               "body": resp["body"].encode()}, op_model.output_shape)
        if resp["status"] >= 300:
            err = parsed.get("Error", {})
            out["error"] = {"code": err.get("Code"), "message": err.get("Message"),
                            "request_id": parsed.get("ResponseMetadata", {}).get("RequestId")}
        else:
            out["output"] = norm(parsed)
    return out


print(json.dumps({
    "generator": "tests/fixtures/golden.py",
    "models": f"aws-cli/{awscli_version}",
    "botocore": botocore_version,
    "cases": [run(c) for c in CASES],
}, indent=1, sort_keys=False))
