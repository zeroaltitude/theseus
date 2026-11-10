//! The tables that settle what the models do not say (AWS design §3.1 and
//! appendix E: botocore has `readonly` on only some services, so the HTTP
//! method and the name carry most of the reads, and these tables are the
//! authority for the rest).
//!
//! Services are botocore's names. Operation names may hold `*` globs. A test
//! checks every row against the embedded catalog: the service exists, and
//! each pattern matches at least one operation.

use crate::classify::{Class, RetryClass, SecretBearing};

pub(crate) struct Ops {
    pub(crate) service: &'static str,
    pub(crate) ops: &'static [&'static str],
}

const fn ops(service: &'static str, ops: &'static [&'static str]) -> Ops {
    Ops { service, ops }
}

/// An override of what the rules would say.
pub(crate) struct ClassRow {
    pub(crate) service: &'static str,
    pub(crate) op: &'static str,
    pub(crate) class: Option<Class>,
    pub(crate) cost: Option<bool>,
    pub(crate) inert: bool,
    pub(crate) note: &'static str,
}

const fn class(
    service: &'static str,
    op: &'static str,
    class: Class,
    note: &'static str,
) -> ClassRow {
    ClassRow {
        service,
        op,
        class: Some(class),
        cost: None,
        inert: false,
        note,
    }
}

const fn paid(
    service: &'static str,
    op: &'static str,
    class: Class,
    note: &'static str,
) -> ClassRow {
    ClassRow {
        service,
        op,
        class: Some(class),
        cost: Some(true),
        inert: false,
        note,
    }
}

pub(crate) const MINT: &str = "mints a credential, whatever its name says";
const CRYPTO: &str = "a cryptographic operation with a key; it changes nothing";
const ANALYSIS: &str = "an analysis of the input; it changes nothing, and is charged per call";

/// Read verbs (§3.1). A name that starts with one, as a whole word, is a read.
pub(crate) static READ_PREFIXES: &[&str] = &[
    "Describe", "List", "Get", "Head", "Lookup", "Search", "Query", "Scan", "BatchGet", "Simulate",
    "Validate", "Estimate",
];

pub(crate) static CLASS: &[ClassRow] = &[
    class(
        "sqs",
        "ReceiveMessage",
        Class::Write,
        "hides the messages it returns from other consumers",
    ),
    class("sts", "AssumeRole*", Class::Write, MINT),
    class("sts", "AssumeRoot", Class::Write, MINT),
    class("sts", "GetSessionToken", Class::Write, MINT),
    class("sts", "GetFederationToken", Class::Write, MINT),
    // Read by their `Get`, but each makes a credential: temporary keys, and a
    // signed JWT that outside OIDC services take as this account's identity.
    class("sts", "GetDelegatedAccessToken", Class::Write, MINT),
    class("sts", "GetWebIdentityToken", Class::Write, MINT),
    class(
        "sts",
        "DecodeAuthorizationMessage",
        Class::Read,
        "decodes a denial's message; it changes nothing",
    ),
    class("sso", "GetRoleCredentials", Class::Write, MINT),
    // A write by its name already; the row says why, as `AssumeRole*`'s does.
    class("eks-auth", "AssumeRoleForPodIdentity", Class::Write, MINT),
    // Found by the output-shape rule (theseus-ye7o): each makes a credential
    // or a token that stands for one, whatever its name says.
    class("deadline", "AssumeFleetRoleFor*", Class::Write, MINT),
    class("deadline", "AssumeQueueRoleFor*", Class::Write, MINT),
    class("s3", "CreateSession", Class::Write, MINT),
    class("s3control", "GetDataAccess", Class::Write, MINT),
    class("ssm", "GetAccessToken", Class::Write, MINT),
    class(
        "lakeformation",
        "GetTemporaryDataLocationCredentials",
        Class::Write,
        MINT,
    ),
    class(
        "lakeformation",
        "AssumeDecoratedRoleWithSAML",
        Class::Write,
        MINT,
    ),
    class("gamelift", "GetComputeAccess", Class::Write, MINT),
    class("gamelift", "GetInstanceAccess", Class::Write, MINT),
    class("gamelift", "RequestUploadCredentials", Class::Write, MINT),
    class(
        "finspace-data",
        "GetProgrammaticAccessCredentials",
        Class::Write,
        MINT,
    ),
    class(
        "finspace-data",
        "GetExternalDataViewAccessDetails",
        Class::Write,
        MINT,
    ),
    class("emr", "GetClusterSessionCredentials", Class::Write, MINT),
    class(
        "emr-containers",
        "GetManagedEndpointSessionCredentials",
        Class::Write,
        MINT,
    ),
    class("datazone", "GetEnvironmentCredentials", Class::Write, MINT),
    class("signin", "CreateOAuth2Token", Class::Write, MINT),
    class(
        "amplifyuibuilder",
        "ExchangeCodeForToken",
        Class::Write,
        MINT,
    ),
    class("amplifyuibuilder", "RefreshToken", Class::Write, MINT),
    class(
        "bedrock-agentcore",
        "GetWorkloadAccessToken*",
        Class::Write,
        MINT,
    ),
    class(
        "bedrock-agentcore",
        "GetResourceOauth2Token",
        Class::Write,
        MINT,
    ),
    class("connect", "GetFederationToken", Class::Write, MINT),
    class("ivs-realtime", "CreateParticipantToken", Class::Write, MINT),
    class("ivschat", "CreateChatToken", Class::Write, MINT),
    class("mwaa", "CreateCliToken", Class::Write, MINT),
    class("mwaa", "CreateWebLoginToken", Class::Write, MINT),
    class("license-manager", "GetAccessToken", Class::Write, MINT),
    class("redshift", "GetIdentityCenterAuthToken", Class::Write, MINT),
    class(
        "redshift-serverless",
        "GetIdentityCenterAuthToken",
        Class::Write,
        MINT,
    ),
    class("workmail", "AssumeImpersonationRole", Class::Write, MINT),
    class(
        "kinesis-video-signaling",
        "GetIceServerConfig",
        Class::Write,
        MINT,
    ),
    // A new ingest password each, always in the answer (theseus-qan5).
    class(
        "mediapackage",
        "RotateChannelCredentials",
        Class::Write,
        MINT,
    ),
    class(
        "mediapackage",
        "RotateIngestEndpointCredentials",
        Class::Write,
        MINT,
    ),
    class(
        "cognito-identity",
        "GetId",
        Class::Write,
        "creates an identity",
    ),
    class(
        "cognito-identity",
        "GetCredentialsForIdentity",
        Class::Write,
        MINT,
    ),
    class("cognito-identity", "GetOpenIdToken*", Class::Write, MINT),
    class("redshift", "GetClusterCredentials*", Class::Write, MINT),
    class("redshift-serverless", "GetCredentials", Class::Write, MINT),
    class(
        "lakeformation",
        "GetTemporaryGlue*Credentials",
        Class::Write,
        MINT,
    ),
    class("ecr", "GetAuthorizationToken", Class::Write, MINT),
    class("ecr-public", "GetAuthorizationToken", Class::Write, MINT),
    class("codeartifact", "GetAuthorizationToken", Class::Write, MINT),
    paid(
        "athena",
        "StartQueryExecution",
        Class::Write,
        "writes its results to S3, and is charged per TB scanned",
    ),
    ClassRow {
        service: "cloudformation",
        op: "CreateChangeSet",
        class: Some(Class::Write),
        cost: None,
        inert: true,
        note: "changes nothing until it is executed",
    },
    class("kms", "Decrypt", Class::Read, CRYPTO),
    class("kms", "Encrypt", Class::Read, CRYPTO),
    class("kms", "ReEncrypt", Class::Read, CRYPTO),
    class("kms", "GenerateDataKey*", Class::Read, CRYPTO),
    class("kms", "GenerateRandom", Class::Read, CRYPTO),
    class("kms", "GenerateMac", Class::Read, CRYPTO),
    class("kms", "VerifyMac", Class::Read, CRYPTO),
    class("kms", "Sign", Class::Read, CRYPTO),
    class("kms", "Verify", Class::Read, CRYPTO),
    class("kms", "DeriveSharedSecret", Class::Read, CRYPTO),
    class("logs", "FilterLogEvents", Class::Read, "reads log events"),
    paid(
        "logs",
        "StartQuery",
        Class::Read,
        "a Logs Insights query; charged per GB scanned",
    ),
    paid(
        "s3",
        "SelectObjectContent",
        Class::Read,
        "S3 Select; charged per GB scanned",
    ),
    class("dynamodb", "TransactGetItems", Class::Read, "reads items"),
    paid("rekognition", "Detect*", Class::Read, ANALYSIS),
    paid("rekognition", "RecognizeCelebrities", Class::Read, ANALYSIS),
    paid("rekognition", "CompareFaces", Class::Read, ANALYSIS),
    paid("textract", "Analyze*", Class::Read, ANALYSIS),
    paid("textract", "DetectDocumentText", Class::Read, ANALYSIS),
    paid("comprehend", "Detect*", Class::Read, ANALYSIS),
    paid("comprehend", "BatchDetect*", Class::Read, ANALYSIS),
    paid("comprehend", "ClassifyDocument", Class::Read, ANALYSIS),
    paid("comprehend", "ContainsPiiEntities", Class::Read, ANALYSIS),
    paid("translate", "TranslateText", Class::Read, ANALYSIS),
    paid("translate", "TranslateDocument", Class::Read, ANALYSIS),
    paid("polly", "SynthesizeSpeech", Class::Read, ANALYSIS),
];

/// AWS session mints (theseus-a3s3; the owner, 2026-10-07: the model may ask,
/// and the operator approves each): `aws.call` waits for the operator's
/// approval at every posture, whatever `[policy.aws]` says, and the keys
/// each mints stay held as handles (`SECRET`). A session minted here does
/// not carry the work session's guards (§3.6). STS's `GetWebIdentityToken`
/// is not one: it signs an assertion for an outside service, no AWS session.
pub(crate) static SESSION_MINT: &[Ops] = &[ops(
    "sts",
    &[
        "AssumeRole*",
        "AssumeRoot",
        "GetSessionToken",
        "GetFederationToken",
        "GetDelegatedAccessToken",
    ],
)];

/// Operations that start code of the caller's choosing (§3.1's short list).
pub(crate) static RUN: &[Ops] = &[
    ops(
        "lambda",
        &["Invoke", "InvokeAsync", "InvokeWithResponseStream"],
    ),
    ops("ecs", &["RunTask", "StartTask", "ExecuteCommand"]),
    ops("batch", &["SubmitJob"]),
    ops(
        "ssm",
        &[
            "SendCommand",
            "StartAutomationExecution",
            "StartSession",
            "StartChangeRequestExecution",
        ],
    ),
    ops(
        "codebuild",
        &[
            "StartBuild",
            "StartBuildBatch",
            "RetryBuild",
            "RetryBuildBatch",
        ],
    ),
    ops(
        "stepfunctions",
        &["StartExecution", "StartSyncExecution", "RedriveExecution"],
    ),
    ops(
        "sagemaker",
        &[
            "CreateTrainingJob",
            "CreateProcessingJob",
            "CreateTransformJob",
            "CreateHyperParameterTuningJob",
            "CreateAutoMLJob",
            "CreateAutoMLJobV2",
            "CreateCompilationJob",
            "CreateEdgePackagingJob",
            "CreateInferenceRecommendationsJob",
            "CreateLabelingJob",
            "CreateOptimizationJob",
            "StartPipelineExecution",
        ],
    ),
    ops(
        "sagemaker-runtime",
        &[
            "InvokeEndpoint",
            "InvokeEndpointAsync",
            "InvokeEndpointWithResponseStream",
        ],
    ),
    ops(
        "bedrock-runtime",
        &[
            "InvokeModel",
            "InvokeModelWithResponseStream",
            "Converse",
            "ConverseStream",
            "StartAsyncInvoke",
        ],
    ),
    ops(
        "bedrock-agent-runtime",
        &[
            "InvokeAgent",
            "InvokeFlow",
            "InvokeInlineAgent",
            "RetrieveAndGenerate",
            "RetrieveAndGenerateStream",
        ],
    ),
    ops(
        "bedrock",
        &[
            "CreateModelCustomizationJob",
            "CreateModelInvocationJob",
            "CreateEvaluationJob",
        ],
    ),
    ops("glue", &["StartJobRun", "StartWorkflowRun"]),
    ops("emr", &["RunJobFlow", "AddJobFlowSteps"]),
    ops("emr-serverless", &["StartJobRun"]),
    ops("emr-containers", &["StartJobRun"]),
    ops("codepipeline", &["StartPipelineExecution"]),
    ops("apigateway", &["TestInvokeMethod", "TestInvokeAuthorizer"]),
    ops("kinesisanalyticsv2", &["StartApplication"]),
    ops("amplify", &["StartJob"]),
    ops("cloudfront", &["TestFunction"]),
];

/// Runs that cost nothing in themselves (SSM's commands run on hosts that
/// are already paid for; §4's table has `SendCommand` as Run without `$`).
pub(crate) static FREE_RUN: &[Ops] = &[
    ops(
        "ssm",
        &[
            "SendCommand",
            "StartAutomationExecution",
            "StartSession",
            "StartChangeRequestExecution",
        ],
    ),
    ops("cloudfront", &["TestFunction"]),
];

/// Cost-bearing beyond the Run operations: creations and starts of metered
/// resources, and operations charged per request or per byte scanned.
pub(crate) static COST: &[Ops] = &[
    ops("ce", &["*"]),
    ops(
        "athena",
        &[
            "StartQueryExecution",
            "StartSession",
            "StartCalculationExecution",
        ],
    ),
    ops("logs", &["StartQuery"]),
    ops("s3", &["SelectObjectContent", "RestoreObject"]),
    ops("glacier", &["InitiateJob"]),
    ops(
        "ec2",
        &[
            "RunInstances",
            "StartInstances",
            "CreateNatGateway",
            "AllocateAddress",
            "CreateVolume",
            "CreateVpcEndpoint",
            "CreateTransitGateway",
            "CreateTransitGatewayVpcAttachment",
            "CreateVpnConnection",
            "CreateClientVpnEndpoint",
            "RequestSpotInstances",
            "RequestSpotFleet",
            "CreateFleet",
            "PurchaseReservedInstancesOffering",
            "PurchaseScheduledInstances",
            "PurchaseHostReservation",
            "PurchaseCapacityBlock",
            "CreateCapacityReservation",
            "CreateCapacityReservationFleet",
            "AllocateHosts",
            "CreateSnapshot",
            "CreateSnapshots",
            "CopySnapshot",
            "CreateImage",
            "CopyImage",
            "CreateTrafficMirrorSession",
            "CreateVerifiedAccessEndpoint",
        ],
    ),
    ops(
        "rds",
        &[
            "CreateDBInstance",
            "CreateDBCluster",
            "CreateDBInstanceReadReplica",
            "StartDBInstance",
            "StartDBCluster",
            "RestoreDB*",
            "CreateDBProxy",
            "CreateDBSnapshot",
            "CreateDBClusterSnapshot",
            "CopyDBSnapshot",
            "CopyDBClusterSnapshot",
            "CreateBlueGreenDeployment",
            "PurchaseReservedDBInstancesOffering",
        ],
    ),
    ops(
        "elasticache",
        &[
            "CreateCacheCluster",
            "CreateReplicationGroup",
            "CreateServerlessCache",
            "PurchaseReservedCacheNodesOffering",
        ],
    ),
    ops(
        "redshift",
        &[
            "CreateCluster",
            "ResumeCluster",
            "RestoreFromClusterSnapshot",
            "PurchaseReservedNodeOffering",
        ],
    ),
    ops("redshift-serverless", &["CreateWorkgroup"]),
    ops("opensearch", &["CreateDomain"]),
    ops("es", &["CreateElasticsearchDomain"]),
    ops(
        "sagemaker",
        &[
            "CreateEndpoint",
            "UpdateEndpoint",
            "CreateNotebookInstance",
            "StartNotebookInstance",
            "CreateApp",
            "CreateInferenceComponent",
            "CreateCluster",
        ],
    ),
    ops("elbv2", &["CreateLoadBalancer"]),
    ops("elb", &["CreateLoadBalancer"]),
    ops(
        "eks",
        &["CreateCluster", "CreateNodegroup", "CreateFargateProfile"],
    ),
    ops("ecs", &["CreateService"]),
    ops(
        "dynamodb",
        &[
            "CreateTable",
            "RestoreTableFromBackup",
            "RestoreTableToPointInTime",
            "ImportTable",
            "ExportTableToPointInTime",
            "CreateBackup",
        ],
    ),
    ops("kinesis", &["CreateStream"]),
    ops("kafka", &["CreateCluster", "CreateClusterV2"]),
    ops("mq", &["CreateBroker"]),
    ops("efs", &["CreateFileSystem"]),
    ops("fsx", &["CreateFileSystem", "CreateFileSystemFromBackup"]),
    ops("kms", &["CreateKey", "ReplicateKey"]),
    ops("secretsmanager", &["CreateSecret"]),
    ops("route53", &["CreateHostedZone", "CreateHealthCheck"]),
    ops(
        "route53domains",
        &["RegisterDomain", "TransferDomain", "RenewDomain"],
    ),
    ops(
        "cloudwatch",
        &["PutMetricAlarm", "PutCompositeAlarm", "PutDashboard"],
    ),
    ops("globalaccelerator", &["CreateAccelerator"]),
    ops(
        "lightsail",
        &[
            "CreateInstances",
            "CreateRelationalDatabase",
            "CreateLoadBalancer",
            "CreateDistribution",
        ],
    ),
    ops("apprunner", &["CreateService"]),
    ops("memorydb", &["CreateCluster"]),
    ops("neptune", &["CreateDBInstance", "CreateDBCluster"]),
    ops("docdb", &["CreateDBInstance", "CreateDBCluster"]),
    ops("lambda", &["PutProvisionedConcurrencyConfig"]),
    ops("bedrock", &["CreateProvisionedModelThroughput"]),
    ops("workspaces", &["CreateWorkspaces"]),
    ops("appstream", &["CreateFleet", "StartFleet"]),
    ops("glue", &["CreateDevEndpoint", "StartCrawler"]),
    ops(
        "cloudfront",
        &["CreateDistribution", "CreateDistributionWithTags"],
    ),
    ops("transfer", &["CreateServer"]),
    ops("directconnect", &["CreateConnection"]),
    ops(
        "dms",
        &["CreateReplicationInstance", "StartReplicationTask"],
    ),
    ops("datasync", &["StartTaskExecution"]),
    ops(
        "backup",
        &["StartBackupJob", "StartRestoreJob", "StartCopyJob"],
    ),
    ops(
        "rekognition",
        &[
            "Detect*",
            "RecognizeCelebrities",
            "CompareFaces",
            "SearchFaces",
            "SearchFacesByImage",
            "IndexFaces",
            "Start*",
        ],
    ),
    ops("textract", &["Analyze*", "DetectDocumentText", "Start*"]),
    ops(
        "comprehend",
        &[
            "Detect*",
            "BatchDetect*",
            "ClassifyDocument",
            "ContainsPiiEntities",
            "Start*Job",
        ],
    ),
    ops(
        "translate",
        &[
            "TranslateText",
            "TranslateDocument",
            "StartTextTranslationJob",
        ],
    ),
    ops("polly", &["SynthesizeSpeech", "StartSpeechSynthesisTask"]),
    ops(
        "transcribe",
        &[
            "StartTranscriptionJob",
            "StartMedicalTranscriptionJob",
            "StartCallAnalyticsJob",
        ],
    ),
];

pub(crate) struct SecretRow {
    pub(crate) service: &'static str,
    pub(crate) op: &'static str,
    pub(crate) secret: SecretBearing,
}

const fn secret(service: &'static str, op: &'static str) -> SecretRow {
    SecretRow {
        service,
        op,
        secret: SecretBearing::Always,
    }
}

const fn secret_when(service: &'static str, op: &'static str, member: &'static str) -> SecretRow {
    SecretRow {
        service,
        op,
        secret: SecretBearing::WhenInputTrue(member),
    }
}

/// Results that hold credentials or secret values (§3.1, §3.5): the value
/// goes onto the secrets board, and the model sees a handle.
pub(crate) static SECRET: &[SecretRow] = &[
    secret("secretsmanager", "GetSecretValue"),
    secret("secretsmanager", "BatchGetSecretValue"),
    secret("secretsmanager", "GetRandomPassword"),
    secret_when("ssm", "GetParameter", "WithDecryption"),
    secret_when("ssm", "GetParameters", "WithDecryption"),
    secret_when("ssm", "GetParametersByPath", "WithDecryption"),
    secret_when("ssm", "GetParameterHistory", "WithDecryption"),
    secret("kms", "Decrypt"),
    secret("kms", "GenerateDataKey"),
    secret("kms", "GenerateDataKeyPair"),
    secret("kms", "GenerateRandom"),
    secret("kms", "DeriveSharedSecret"),
    secret("sts", "AssumeRole*"),
    secret("sts", "AssumeRoot"),
    secret("sts", "GetSessionToken"),
    secret("sts", "GetFederationToken"),
    secret("sts", "GetDelegatedAccessToken"),
    secret("sts", "GetWebIdentityToken"),
    secret("eks-auth", "AssumeRoleForPodIdentity"),
    // Found by the output-shape rule (theseus-ye7o). The mints, as `CLASS`
    // names them:
    secret("deadline", "AssumeFleetRoleFor*"),
    secret("deadline", "AssumeQueueRoleFor*"),
    secret("s3", "CreateSession"),
    secret("s3control", "GetDataAccess"),
    secret("ssm", "GetAccessToken"),
    secret("lakeformation", "GetTemporaryDataLocationCredentials"),
    secret("lakeformation", "AssumeDecoratedRoleWithSAML"),
    secret("gamelift", "GetComputeAccess"),
    secret("gamelift", "GetInstanceAccess"),
    secret("gamelift", "RequestUploadCredentials"),
    secret("finspace-data", "GetProgrammaticAccessCredentials"),
    secret("finspace-data", "GetExternalDataViewAccessDetails"),
    secret("emr", "GetClusterSessionCredentials"),
    secret("emr-containers", "GetManagedEndpointSessionCredentials"),
    secret("datazone", "GetEnvironmentCredentials"),
    secret("signin", "CreateOAuth2Token"),
    secret("amplifyuibuilder", "ExchangeCodeForToken"),
    secret("amplifyuibuilder", "RefreshToken"),
    secret("bedrock-agentcore", "GetWorkloadAccessToken*"),
    secret("bedrock-agentcore", "GetResourceOauth2Token"),
    secret("connect", "GetFederationToken"),
    secret("ivs-realtime", "CreateParticipantToken"),
    secret("ivschat", "CreateChatToken"),
    secret("mwaa", "CreateCliToken"),
    secret("mwaa", "CreateWebLoginToken"),
    secret("license-manager", "GetAccessToken"),
    secret("redshift", "GetIdentityCenterAuthToken"),
    secret("redshift-serverless", "GetIdentityCenterAuthToken"),
    secret("workmail", "AssumeImpersonationRole"),
    secret("kinesis-video-signaling", "GetIceServerConfig"),
    secret("mediapackage", "RotateChannelCredentials"),
    secret("mediapackage", "RotateIngestEndpointCredentials"),
    // A secret made or kept, read back: a tunnel's tokens, a stream key, a
    // long-lived token, a client's secret, a key.
    secret("iotsecuretunneling", "OpenTunnel"),
    secret("iotsecuretunneling", "RotateTunnelAccessToken"),
    secret("ivs-realtime", "CreateIngestConfiguration"),
    secret("ivs-realtime", "GetIngestConfiguration"),
    secret("ivs-realtime", "UpdateIngestConfiguration"),
    secret("license-manager", "CreateToken"),
    secret("route53domains", "TransferDomainToAnotherAwsAccount"),
    secret("pca-connector-scep", "CreateChallenge"),
    secret("pca-connector-scep", "GetChallengePassword"),
    secret("pcs", "RegisterComputeNodeGroupInstance"),
    secret("sso-oidc", "RegisterClient"),
    secret("wickr", "RegisterOpentdfConfig"),
    secret("wickr", "GetOpentdfConfig"),
    secret("finspace-data", "ResetUserPassword"),
    secret("iot-managed-integrations", "CreateProvisioningProfile"),
    secret("iotwireless", "AssociateAwsAccountWithPartnerAccount"),
    secret("location", "CreateKey"),
    secret("location", "DescribeKey"),
    secret("payment-cryptography-data", "DecryptData"),
    secret("bedrock-agentcore", "GetResourceApiKey"),
    secret("chime-sdk-meetings", "CreateAttendee"),
    secret("chime-sdk-meetings", "BatchCreateAttendee"),
    secret("chime-sdk-meetings", "GetAttendee"),
    secret("chime-sdk-meetings", "CreateMeetingWithAttendees"),
    secret("connect", "StartWebRTCContact"),
    secret("chime", "CreateBot"),
    secret("chime", "GetBot"),
    secret("chime", "UpdateBot"),
    secret("chime", "RegenerateSecurityToken"),
    secret("lightsail", "CreateContainerServiceRegistryLogin"),
    secret("ecr", "GetAuthorizationToken"),
    secret("ecr-public", "GetAuthorizationToken"),
    secret("codeartifact", "GetAuthorizationToken"),
    secret("iam", "CreateAccessKey"),
    secret("iam", "CreateServiceSpecificCredential"),
    secret("iam", "ResetServiceSpecificCredential"),
    secret("sso", "GetRoleCredentials"),
    secret("sso-oidc", "CreateToken*"),
    secret("cognito-identity", "GetCredentialsForIdentity"),
    secret("cognito-identity", "GetOpenIdToken*"),
    secret("cognito-idp", "InitiateAuth"),
    secret("cognito-idp", "AdminInitiateAuth"),
    secret("cognito-idp", "RespondToAuthChallenge"),
    secret("cognito-idp", "AdminRespondToAuthChallenge"),
    secret("cognito-idp", "GetTokensFromRefreshToken"),
    secret("redshift", "GetClusterCredentials*"),
    secret("redshift-serverless", "GetCredentials"),
    secret("lakeformation", "GetTemporaryGlue*Credentials"),
    secret("lightsail", "GetInstanceAccessDetails"),
    secret("lightsail", "DownloadDefaultKeyPair"),
    secret("lightsail", "CreateKeyPair"),
    secret("lightsail", "GetRelationalDatabaseMasterUserPassword"),
    secret("lightsail", "CreateBucketAccessKey"),
    secret("ec2", "CreateKeyPair"),
    secret("ec2", "GetPasswordData"),
    secret("iot", "CreateKeysAndCertificate"),
    secret("iot", "CreateProvisioningClaim"),
    secret("acm", "ExportCertificate"),
    // An API key: API Gateway's `value`, AppSync's `id`, which the walk
    // holds by their shapes (theseus-u4pe). A listing is `WHEN_PRESENT`.
    secret("apigateway", "CreateApiKey"),
    secret_when("apigateway", "GetApiKey", "includeValue"),
    secret("appsync", "CreateApiKey"),
    secret("appsync", "UpdateApiKey"),
    // Credentials the output-shape rule could not see until theseus-37ds,
    // because their members are named for the thing they unlock and not for
    // what they are: a stream's `value`, a workspace's `key`, a channel's
    // `passphrase`, an MFA device's `Base32StringSeed`. The rule now reads
    // the model's own `sensitive` marking together with the words of the
    // member and of the shape it sits in, so each of these is found.
    //
    // A credential made: the call mints one, and it is always in the answer.
    secret("grafana", "CreateWorkspaceApiKey"),
    secret("grafana", "CreateWorkspaceServiceAccountToken"),
    secret("cognito-idp", "AddUserPoolClientSecret"),
    // The TOTP seed a software MFA factor is built from.
    secret("cognito-idp", "AssociateSoftwareToken"),
    secret("ivs", "CreateStreamKey"),
    // Mints a stream key, and returns the channel's SRT passphrase with it.
    secret("ivs", "CreateChannel"),
    secret("ecs", "ExecuteCommand"),
    secret("codecatalyst", "CreateAccessToken"),
    secret("codecatalyst", "StartDevEnvironmentSession"),
    secret("route53globalresolver", "CreateAccessToken"),
    secret("iam", "CreateVirtualMFADevice"),
    secret("payment-cryptography", "ExportKey"),
    secret("payment-cryptography-data", "GenerateAs2805KekValidation"),
    secret("payment-cryptography-data", "TranslateKeyMaterial"),
    // A PIN block: marked sensitive, required, and named for what it is
    // rather than for a credential word, so no name rule reached it
    // (theseus-3i6d1). The member's plain reading, not a sourced fact.
    secret("payment-cryptography-data", "TranslatePinData"),
    secret("payment-cryptography-data", "GeneratePinData"),
    secret("payment-cryptography-data", "GenerateMacEmvPinChange"),
    // A credential kept, read back whole.
    secret("ivs", "GetStreamKey"),
    secret("route53globalresolver", "GetAccessToken"),
    secret("route53domains", "RetrieveDomainAuthCode"),
    // The tunnels' pre-shared keys, in the device configuration AWS writes
    // for the customer gateway. `DescribeVpnConnections` was already
    // secret-bearing; this one, which returns the same keys, was not.
    secret("ec2", "GetVpnConnectionDeviceSampleConfiguration"),
    // Credentials no model marks at all, found once the rule learned the
    // compound names of `secret::ENDS` (theseus-37ds): a participant's
    // token, an `AuthToken`, a WAF API key.
    secret("connect", "StartChatContact"),
    secret("connect", "CreateParticipant"),
    secret("athena", "CreatePresignedNotebookUrl"),
    secret("athena", "GetSessionEndpoint"),
    secret("gamelift", "GetComputeAuthToken"),
    secret("wafv2", "CreateAPIKey"),
    // Marked, but named for what it connects to rather than for the
    // credential signed into it.
    secret("finspace", "GetKxConnectionString"),
];

/// Results that hold a secret only sometimes (theseus-qan5): what the walk
/// finds is held, and a result with none is returned whole. A `SECRET` row
/// wins over these. Each operation's output has a member the walk
/// recognizes (the output-shape rule fails one with none), and none is a
/// mint.
pub(crate) static WHEN_PRESENT: &[Ops] = &[
    // A secret configured on a resource, echoed in its description when it
    // is set (aws-mints' `ECHO` rows until theseus-qan5).
    ops(
        "amplify",
        &[
            "CreateApp",
            "DeleteApp",
            "GetApp",
            "ListApps",
            "UpdateApp",
            "CreateBranch",
            "DeleteBranch",
            "GetBranch",
            "ListBranches",
            "UpdateBranch",
        ],
    ),
    // Its `SocialProviderSettings` providers are marked sensitive, so the
    // walk holds each provider structure whole and the `ClientSecret` and
    // `PrivateKey` inside it never reach the model. It was **never** a
    // partial hold: see `theseus_core::aws::tests_secret_shapes`'s
    // `MARKED` rows for the correction and why one was once claimed
    // (theseus-3i6d1).
    ops("amplifybackend", &["GetBackendAuth"]),
    // Its model says only DescribeDirectoryConfigs never returns the
    // password; it says nothing of these two.
    ops(
        "appstream",
        &["CreateDirectoryConfig", "UpdateDirectoryConfig"],
    ),
    ops(
        "chime",
        &[
            "CreateUser",
            "GetUser",
            "ListBots",
            "ListUsers",
            "ResetPersonalPIN",
            "UpdateUser",
        ],
    ),
    ops("chime-sdk-identity", &["DescribeAppInstanceUserEndpoint"]),
    ops(
        "chime-sdk-meetings",
        &["ListAttendees", "UpdateAttendeeCapabilities"],
    ),
    // A client's secret, generated (`GenerateSecret`) or given.
    ops(
        "cognito-idp",
        &[
            "CreateUserPoolClient",
            "DescribeUserPoolClient",
            "UpdateUserPoolClient",
        ],
    ),
    ops("connecthealth", &["GetPatientInsightsJob"]),
    ops(
        "datazone",
        &["CreateConnection", "ListConnections", "UpdateConnection"],
    ),
    ops(
        "dms",
        &[
            "CreateEndpoint",
            "DeleteEndpoint",
            "DescribeEndpoints",
            "ModifyEndpoint",
        ],
    ),
    ops("ds", &["DescribeDirectories"]),
    ops(
        "ec2",
        &[
            "AttachVerifiedAccessTrustProvider",
            "CreateVerifiedAccessTrustProvider",
            "DeleteVerifiedAccessTrustProvider",
            "DescribeVerifiedAccessTrustProviders",
            "DetachVerifiedAccessTrustProvider",
            "ModifyVerifiedAccessTrustProvider",
            "ExportVerifiedAccessInstanceClientConfiguration",
            "CreateVpnConnection",
            "DescribeVpnConnections",
            "ModifyVpnConnection",
            "ModifyVpnConnectionOptions",
            "ModifyVpnTunnelCertificate",
            "ModifyVpnTunnelOptions",
        ],
    ),
    ops("iot", &["GetTopicRule"]),
    ops("iot-managed-integrations", &["GetManagedThing"]),
    ops("ivs-realtime", &["CreateStage"]),
    ops(
        "lexv2-models",
        &[
            "DescribeBotRecommendation",
            "StartBotRecommendation",
            "UpdateBotRecommendation",
        ],
    ),
    ops(
        "medialive",
        &[
            "CreateChannel",
            "DeleteChannel",
            "DescribeChannel",
            "RestartChannelPipelines",
            "StartChannel",
            "StopChannel",
            "UpdateChannel",
            "UpdateChannelClass",
        ],
    ),
    ops(
        "mediapackage",
        &[
            "ConfigureLogs",
            "CreateChannel",
            "DescribeChannel",
            "ListChannels",
            "UpdateChannel",
        ],
    ),
    ops("quicksight", &["DescribeAssetBundleImportJob"]),
    ops(
        "rds",
        &[
            "CreateDBCluster",
            "CreateDBInstance",
            "CreateDBInstanceReadReplica",
            "CreateTenantDatabase",
            "DeleteDBCluster",
            "DeleteDBInstance",
            "DeleteTenantDatabase",
            "DescribeDBClusters",
            "DescribeDBInstances",
            "DescribeTenantDatabases",
            "FailoverDBCluster",
            "ModifyDBCluster",
            "ModifyDBInstance",
            "ModifyTenantDatabase",
            "PromoteReadReplica",
            "PromoteReadReplicaDBCluster",
            "RebootDBCluster",
            "RebootDBInstance",
            "RestoreDBClusterFromS3",
            "RestoreDBClusterFromSnapshot",
            "RestoreDBClusterToPointInTime",
            "RestoreDBInstanceFromDBSnapshot",
            "RestoreDBInstanceFromS3",
            "RestoreDBInstanceToPointInTime",
            "StartDBCluster",
            "StartDBInstance",
            "StopDBCluster",
            "StopDBInstance",
            "SwitchoverReadReplica",
        ],
    ),
    ops(
        "redshift",
        &[
            "CreateCluster",
            "DeleteCluster",
            "DescribeClusters",
            "DisableSnapshotCopy",
            "EnableSnapshotCopy",
            "FailoverPrimaryCompute",
            "ModifyCluster",
            "ModifyClusterDbRevision",
            "ModifyClusterIamRoles",
            "ModifyClusterMaintenance",
            "ModifySnapshotCopyRetentionPeriod",
            "PauseCluster",
            "RebootCluster",
            "ResizeCluster",
            "RestoreFromClusterSnapshot",
            "ResumeCluster",
            "RotateEncryptionKey",
        ],
    ),
    // A secret that comes with some answers and not others: a job's
    // artifact keys (none when there is no job, or no artifact), a build's
    // upload keys ("returned only when the operation is called without a
    // storage location"), WebRTC's join token (a chat has none), an OIDC
    // client's secret and tokens (each optional), and a connection's
    // credentials, which only some connections carry, whatever
    // `withSecret` says.
    ops(
        "codepipeline",
        &["GetJobDetails", "GetThirdPartyJobDetails", "PollForJobs"],
    ),
    ops("gamelift", &["CreateBuild"]),
    ops("connectparticipant", &["CreateParticipantConnection"]),
    ops("wickr", &["GetOidcInfo", "RegisterOidcConfig"]),
    ops("datazone", &["GetConnection"]),
    // API keys in a listing, none when there is no key (or, for API
    // Gateway's, unless `includeValues`), and API Gateway's key as an
    // update returns it (theseus-u4pe).
    ops("apigateway", &["GetApiKeys", "UpdateApiKey"]),
    ops("appsync", &["ListApiKeys"]),
    // theseus-37ds. A credential a resource keeps, in answers that carry it
    // only sometimes: a channel's SRT passphrase only where SRT ingest is
    // configured, a batch read with no members, a listing with none, and an
    // MFA seed, which AWS returns only for a device not yet enabled.
    ops(
        "ivs",
        &[
            "GetChannel",
            "BatchGetChannel",
            "UpdateChannel",
            "GetStreamSession",
            "BatchGetStreamKey",
        ],
    ),
    ops(
        "cognito-idp",
        &["ListUserPoolClientSecrets", "VerifySoftwareToken"],
    ),
    ops("storagegateway", &["DescribeChapCredentials"]),
    ops("iam", &["ListVirtualMFADevices"]),
    // A thin client environment's activation code, which registers a device
    // against the environment; gone once the environment stops issuing one.
    ops(
        "workspaces-thin-client",
        &[
            "CreateEnvironment",
            "GetEnvironment",
            "ListEnvironments",
            "UpdateEnvironment",
        ],
    ),
    // theseus-37ds, the unmarked compound names of `secret::ENDS`: a secret
    // a resource was configured with and echoes back only where it is set.
    // A webhook's shared secret, set only for `GITHUB_HMAC` authentication.
    ops("codepipeline", &["ListWebhooks", "PutWebhook"]),
    // An OIDC client secret on a listener's or a rule's authenticate action,
    // present only on actions of that type.
    ops(
        "elbv2",
        &[
            "CreateListener",
            "CreateRule",
            "DescribeListeners",
            "DescribeRules",
            "ModifyListener",
            "ModifyRule",
            "SetRulePriorities",
        ],
    ),
    // The API key of an API Gateway asset, on assets of that kind only.
    ops(
        "dataexchange",
        &[
            "CreateJob",
            "GetAsset",
            "GetJob",
            "ListJobs",
            "ListRevisionAssets",
            "UpdateAsset",
        ],
    ),
    // Splunk's HEC token, on Splunk destinations only.
    ops("firehose", &["DescribeDeliveryStream"]),
    // A job's source-control token, set only where source control is.
    ops("glue", &["BatchGetJobs", "GetJob", "GetJobs"]),
    // A VPN tunnel's pre-shared key, inside the resource detail of a finding
    // about a VPN connection.
    ops("securityhub", &["GetFindings"]),
    // A listing with no key in it answers whole.
    ops("wafv2", &["ListAPIKeys"]),
    // A password a resource is holding and will apply, or was configured
    // with, and which no model marks: present only while the change is
    // pending, or only on a cluster that uses the feature at all.
    ops(
        "docdb",
        &[
            "CreateDBInstance",
            "DeleteDBInstance",
            "DescribeDBInstances",
            "ModifyDBInstance",
            "RebootDBInstance",
        ],
    ),
    ops(
        "neptune",
        &[
            "CreateDBInstance",
            "DeleteDBInstance",
            "DescribeDBInstances",
            "ModifyDBInstance",
            "RebootDBInstance",
        ],
    ),
    ops(
        "lightsail",
        &["GetRelationalDatabase", "GetRelationalDatabases"],
    ),
    // The cluster's Kerberos passwords, on a Kerberized cluster only.
    ops("emr", &["DescribeCluster"]),
    // The HSM cluster's pre-crypto-officer password, before it is claimed.
    ops(
        "cloudhsmv2",
        &[
            "CreateCluster",
            "DeleteCluster",
            "DescribeClusters",
            "ModifyCluster",
        ],
    ),
    // The claim certificate a profile's Get answers with when it has one:
    // the Create twin is a `SECRET` row. Optional here, so `WHEN_PRESENT`.
    ops("iot-managed-integrations", &["GetProvisioningProfile"]),
    // The shared secret of a procurement portal, where one is configured.
    ops("invoicing", &["GetProcurementPortalPreference"]),
    // A workspace's `grafanaToken`, which the model does not mark and which
    // rides each description of the workspace (theseus-3i6d1).
    ops(
        "grafana",
        &[
            "AssociateLicense",
            "CreateWorkspace",
            "DeleteWorkspace",
            "DescribeWorkspace",
            "DisassociateLicense",
            "ListWorkspaces",
            "UpdateWorkspace",
        ],
    ),
    // The Firebase server key of a GCM channel, `Credential`, unmarked; and
    // the Baidu channel's `Credential`, measured the same way (theseus-3i6d1).
    ops(
        "pinpoint",
        &[
            "DeleteGcmChannel",
            "GetGcmChannel",
            "UpdateGcmChannel",
            "DeleteBaiduChannel",
            "GetBaiduChannel",
            "UpdateBaiduChannel",
        ],
    ),
    // An EKS Anywhere subscription's `licenses[].token`. It was left to an
    // allowed row ("an entitlement to support, no access to the account");
    // the owner, 2026-11-09 15:48: "a token is a token, and the field exists
    // to authenticate" (theseus-3i6d1). The walk holds `token` by name.
    ops(
        "eks",
        &[
            "CreateEksAnywhereSubscription",
            "DeleteEksAnywhereSubscription",
            "DescribeEksAnywhereSubscription",
            "ListEksAnywhereSubscriptions",
            "UpdateEksAnywhereSubscription",
        ],
    ),
];

/// Tagging is direct (§3.4), so it is never IaC-only.
pub(crate) static TAGGING: &[&str] = &[
    "Tag*",
    "Untag*",
    "*Tags",
    "*Tagging",
    "AddTags*",
    "RemoveTags*",
    "*TagResource",
    "*TagsToResource",
    "*TagsFromResource",
];

/// Operations on what exists are direct (§3.4: start, stop, scale, redeploy,
/// invalidate, rotate), whatever their noun.
pub(crate) static NOT_IAC_PREFIXES: &[&str] = &[
    "Start",
    "Stop",
    "Reboot",
    "Test",
    "Failover",
    "Copy",
    "Cancel",
    "Pause",
    "Resume",
    "Rotate",
    "Invalidate",
];

/// Exceptions to [`IAC`] by name.
pub(crate) static NOT_IAC: &[Ops] = &[
    ops("cloudfront", &["*Invalidation*"]),
    ops("budgets", &["ExecuteBudgetAction"]),
];

/// Durable infrastructure, made and changed only through a stack (§3.4's
/// table): networks, buckets, tables, queues, topics, streams, log groups,
/// functions, clusters, services, instances, IAM, KMS keys, load balancers,
/// APIs, distributions, DNS, certificates, alarms, rules, schedules, and
/// budgets. Only writes are IaC-only; a pattern that also matches a read or
/// a run is harmless.
pub(crate) static IAC: &[Ops] = &[
    ops(
        "ec2",
        &[
            "RunInstances",
            "RequestSpotInstances",
            "RequestSpotFleet",
            "ModifySpotFleetRequest",
            "CreateFleet",
            "DeleteFleets",
            "ModifyFleet",
            "ModifyInstanceAttribute",
            "ModifyInstancePlacement",
            "ModifyInstanceMetadataOptions",
            "*Vpc*",
            "*Subnet*",
            "*InternetGateway*",
            "*NatGateway*",
            "*CarrierGateway*",
            "*SecurityGroup*",
            "*Route*",
            "*NetworkAcl*",
            "*TransitGateway*",
            "*VpnGateway*",
            "*VpnConnection*",
            "*CustomerGateway*",
            "*DhcpOptions*",
            "*LaunchTemplate*",
            "*PlacementGroup*",
            "*FlowLogs*",
            "*Address*",
            "*NetworkInterface*",
            "*KeyPair*",
            "*ClientVpn*",
            "*ManagedPrefixList*",
            "*Ipam*",
            "*Volume*",
            "*CapacityReservation*",
            "AllocateHosts",
            "ReleaseHosts",
            "ModifyHosts",
            "*VerifiedAccess*",
            "*TrafficMirror*",
            "*InstanceConnectEndpoint*",
        ],
    ),
    ops(
        "s3",
        &[
            "CreateBucket",
            "DeleteBucket",
            "PutBucket*",
            "DeleteBucket*",
            "*PublicAccessBlock",
            "PutObjectLockConfiguration",
        ],
    ),
    ops(
        "s3control",
        &[
            "*PublicAccessBlock",
            "*AccessPoint*",
            "*StorageLensConfiguration",
            "*AccessGrantsInstance",
            "CreateBucket",
            "DeleteBucket",
        ],
    ),
    ops(
        "dynamodb",
        &[
            "CreateTable",
            "DeleteTable",
            "UpdateTable",
            "UpdateTableReplicaAutoScaling",
            "*GlobalTable*",
            "UpdateTimeToLive",
            "UpdateContinuousBackups",
            "*KinesisStreamingDestination",
            "UpdateContributorInsights",
            "*ResourcePolicy",
            "RestoreTableFromBackup",
            "RestoreTableToPointInTime",
            "ImportTable",
        ],
    ),
    ops(
        "sqs",
        &[
            "CreateQueue",
            "DeleteQueue",
            "SetQueueAttributes",
            "AddPermission",
            "RemovePermission",
        ],
    ),
    ops(
        "sns",
        &[
            "CreateTopic",
            "DeleteTopic",
            "SetTopicAttributes",
            "Subscribe",
            "Unsubscribe",
            "SetSubscriptionAttributes",
            "AddPermission",
            "RemovePermission",
            "*PlatformApplication*",
            "PutDataProtectionPolicy",
        ],
    ),
    ops(
        "kinesis",
        &[
            "CreateStream",
            "DeleteStream",
            "*StreamConsumer",
            "*ResourcePolicy",
        ],
    ),
    ops(
        "logs",
        &[
            "CreateLogGroup",
            "DeleteLogGroup",
            "PutRetentionPolicy",
            "DeleteRetentionPolicy",
            "*SubscriptionFilter",
            "*MetricFilter",
            "*ResourcePolicy",
            "PutDestination*",
            "DeleteDestination",
            "*DataProtectionPolicy",
            "*AccountPolicy",
            "AssociateKmsKey",
            "DisassociateKmsKey",
            "*DeliveryDestination*",
            "*DeliverySource",
            "CreateDelivery",
            "DeleteDelivery",
            "*LogAnomalyDetector",
            "*Transformer",
            "*IndexPolicy",
        ],
    ),
    ops(
        "lambda",
        &[
            "CreateFunction",
            "DeleteFunction",
            "UpdateFunctionConfiguration",
            "*FunctionUrlConfig",
            "*EventSourceMapping",
            "AddPermission",
            "RemovePermission",
            "*LayerVersionPermission",
            "*FunctionEventInvokeConfig",
            "*CodeSigningConfig",
            "PutRuntimeManagementConfig",
        ],
    ),
    ops(
        "ecs",
        &[
            "CreateCluster",
            "DeleteCluster",
            "UpdateCluster",
            "UpdateClusterSettings",
            "PutClusterCapacityProviders",
            "CreateService",
            "DeleteService",
            "*CapacityProvider",
        ],
    ),
    ops(
        "eks",
        &[
            "CreateCluster",
            "DeleteCluster",
            "UpdateClusterConfig",
            "UpdateClusterVersion",
            "*Nodegroup*",
            "*FargateProfile",
            "*Addon",
            "*AccessEntry",
            "*AccessPolicy",
            "*IdentityProviderConfig",
            "*PodIdentityAssociation",
            "RegisterCluster",
            "DeregisterCluster",
            "AssociateEncryptionConfig",
        ],
    ),
    ops(
        "rds",
        &[
            "CreateDBInstance",
            "DeleteDBInstance",
            "ModifyDBInstance",
            "CreateDBCluster",
            "DeleteDBCluster",
            "ModifyDBCluster",
            "*DBSubnetGroup",
            "*DBParameterGroup",
            "*DBClusterParameterGroup",
            "*OptionGroup",
            "*DBProxy*",
            "*DBSecurityGroup*",
            "CreateDBInstanceReadReplica",
            "RestoreDB*",
            "*GlobalCluster",
            "*EventSubscription",
            "*DBClusterEndpoint",
            "*CustomDBEngineVersion",
            "*BlueGreenDeployment",
            "*Integration",
            "*DBShardGroup",
            "AddRoleTo*",
            "RemoveRoleFrom*",
        ],
    ),
    ops(
        "docdb",
        &[
            "CreateDBInstance",
            "DeleteDBInstance",
            "ModifyDBInstance",
            "CreateDBCluster",
            "DeleteDBCluster",
            "ModifyDBCluster",
            "*DBSubnetGroup",
            "*DBClusterParameterGroup",
            "RestoreDB*",
            "*GlobalCluster",
        ],
    ),
    ops(
        "neptune",
        &[
            "CreateDBInstance",
            "DeleteDBInstance",
            "ModifyDBInstance",
            "CreateDBCluster",
            "DeleteDBCluster",
            "ModifyDBCluster",
            "*DBSubnetGroup",
            "*DBClusterParameterGroup",
            "*DBParameterGroup",
            "RestoreDB*",
        ],
    ),
    ops(
        "elasticache",
        &[
            "*CacheCluster",
            "*ReplicationGroup*",
            "*CacheSubnetGroup",
            "*CacheParameterGroup",
            "*CacheSecurityGroup*",
            "*ServerlessCache",
            "*GlobalReplicationGroup",
            "CreateUser",
            "DeleteUser",
            "ModifyUser",
            "*UserGroup",
        ],
    ),
    ops(
        "redshift",
        &[
            "CreateCluster",
            "DeleteCluster",
            "ModifyCluster",
            "*ClusterSubnetGroup",
            "*ClusterParameterGroup",
            "*ClusterSecurityGroup*",
            "RestoreFromClusterSnapshot",
            "*EndpointAccess",
            "*UsageLimit",
            "*Integration",
            "*HsmClientCertificate",
            "*HsmConfiguration",
            "*EventSubscription",
            "ModifyClusterIamRoles",
        ],
    ),
    ops(
        "redshift-serverless",
        &["*Namespace", "*Workgroup", "*EndpointAccess", "*UsageLimit"],
    ),
    ops(
        "opensearch",
        &[
            "CreateDomain",
            "DeleteDomain",
            "UpdateDomainConfig",
            "*VpcEndpoint",
            "*OutboundConnection",
        ],
    ),
    ops(
        "es",
        &[
            "CreateElasticsearchDomain",
            "DeleteElasticsearchDomain",
            "UpdateElasticsearchDomainConfig",
        ],
    ),
    ops(
        "elbv2",
        &[
            "CreateLoadBalancer",
            "DeleteLoadBalancer",
            "ModifyLoadBalancerAttributes",
            "*Listener",
            "ModifyListenerAttributes",
            "*TargetGroup",
            "ModifyTargetGroupAttributes",
            "*Rule",
            "SetRulePriorities",
            "SetSecurityGroups",
            "SetSubnets",
            "SetIpAddressType",
            "*TrustStore",
            "AddListenerCertificates",
            "RemoveListenerCertificates",
        ],
    ),
    ops(
        "elb",
        &[
            "CreateLoadBalancer",
            "DeleteLoadBalancer",
            "CreateLoadBalancerListeners",
            "DeleteLoadBalancerListeners",
            "ModifyLoadBalancerAttributes",
            "ApplySecurityGroupsToLoadBalancer",
        ],
    ),
    ops(
        "sagemaker",
        &[
            "CreateEndpoint",
            "DeleteEndpoint",
            "CreateEndpointConfig",
            "DeleteEndpointConfig",
            "CreateNotebookInstance",
            "DeleteNotebookInstance",
            "UpdateNotebookInstance",
            "*NotebookInstanceLifecycleConfig",
            "CreateDomain",
            "DeleteDomain",
            "UpdateDomain",
            "*UserProfile",
            "*Space",
            "CreateModel",
            "DeleteModel",
            "*InferenceComponent",
            "CreateCluster",
            "DeleteCluster",
            "UpdateCluster",
        ],
    ),
    ops(
        "efs",
        &[
            "CreateFileSystem",
            "DeleteFileSystem",
            "UpdateFileSystem",
            "*MountTarget*",
            "*FileSystemPolicy",
            "*AccessPoint",
            "PutLifecycleConfiguration",
            "*ReplicationConfiguration",
            "PutBackupPolicy",
            "UpdateFileSystemProtection",
        ],
    ),
    ops(
        "fsx",
        &[
            "CreateFileSystem*",
            "DeleteFileSystem",
            "UpdateFileSystem",
            "*StorageVirtualMachine",
            "*Volume*",
            "*DataRepositoryAssociation",
            "*FileCache",
        ],
    ),
    ops(
        "autoscaling",
        &[
            "CreateAutoScalingGroup",
            "DeleteAutoScalingGroup",
            "UpdateAutoScalingGroup",
            "*LaunchConfiguration",
            "PutScalingPolicy",
            "DeletePolicy",
            "PutScheduledUpdateGroupAction",
            "BatchPutScheduledUpdateGroupAction",
            "DeleteScheduledAction",
            "BatchDeleteScheduledAction",
            "PutLifecycleHook",
            "DeleteLifecycleHook",
            "PutNotificationConfiguration",
            "DeleteNotificationConfiguration",
            "*WarmPool",
            "AttachLoadBalancer*",
            "DetachLoadBalancer*",
            "AttachTrafficSources",
            "DetachTrafficSources",
        ],
    ),
    ops(
        "application-autoscaling",
        &[
            "RegisterScalableTarget",
            "DeregisterScalableTarget",
            "PutScalingPolicy",
            "DeleteScalingPolicy",
            "PutScheduledAction",
            "DeleteScheduledAction",
        ],
    ),
    ops(
        "stepfunctions",
        &[
            "CreateStateMachine",
            "DeleteStateMachine",
            "UpdateStateMachine",
            "*StateMachineAlias",
            "CreateActivity",
            "DeleteActivity",
        ],
    ),
    ops(
        "codebuild",
        &[
            "CreateProject",
            "DeleteProject",
            "UpdateProject",
            "*Webhook",
            "*ReportGroup",
            "*SourceCredentials",
            "*Fleet",
        ],
    ),
    ops(
        "codepipeline",
        &[
            "CreatePipeline",
            "DeletePipeline",
            "UpdatePipeline",
            "*Webhook",
            "*CustomActionType",
        ],
    ),
    ops(
        "glue",
        &[
            "CreateJob",
            "DeleteJob",
            "UpdateJob",
            "*Crawler",
            "*Trigger",
            "*Connection",
            "*DevEndpoint",
            "CreateWorkflow",
            "DeleteWorkflow",
            "UpdateWorkflow",
            "*SecurityConfiguration",
            "*Registry",
            "*Blueprint",
            "*MLTransform",
        ],
    ),
    ops(
        "athena",
        &["*WorkGroup", "*DataCatalog", "*CapacityReservation"],
    ),
    ops(
        "secretsmanager",
        &[
            "PutResourcePolicy",
            "DeleteResourcePolicy",
            "ReplicateSecretToRegions",
            "RemoveRegionsFromReplication",
        ],
    ),
    ops(
        "ssm",
        &[
            "*MaintenanceWindow*",
            "*Association",
            "CreateAssociationBatch",
            "*PatchBaseline",
            "*ResourceDataSync",
            "*Activation",
        ],
    ),
    ops(
        "ecr",
        &[
            "CreateRepository",
            "DeleteRepository",
            "*RepositoryPolicy",
            "*LifecyclePolicy",
            "PutImageScanningConfiguration",
            "PutImageTagMutability",
            "*RegistryPolicy",
            "PutReplicationConfiguration",
            "*PullThroughCacheRule",
            "PutRegistryScanningConfiguration",
            "*RepositoryCreationTemplate",
        ],
    ),
    ops(
        "iam",
        &[
            "CreateRole",
            "DeleteRole",
            "UpdateRole",
            "UpdateRoleDescription",
            "UpdateAssumeRolePolicy",
            "*RolePolicy",
            "*RolePermissionsBoundary",
            "CreatePolicy",
            "DeletePolicy",
            "*PolicyVersion",
            "CreateUser",
            "DeleteUser",
            "UpdateUser",
            "*UserPolicy",
            "*UserPermissionsBoundary",
            "CreateGroup",
            "DeleteGroup",
            "UpdateGroup",
            "AddUserToGroup",
            "RemoveUserFromGroup",
            "*GroupPolicy",
            "*InstanceProfile",
            "*OpenIDConnectProvider*",
            "*SAMLProvider",
            "*AccessKey",
            "*LoginProfile",
            "*ServiceSpecificCredential",
            "*SSHPublicKey",
            "*ServerCertificate",
            "*SigningCertificate",
            "*VirtualMFADevice",
            "*AccountPasswordPolicy",
            "*AccountAlias",
        ],
    ),
    ops(
        "kms",
        &[
            "CreateKey",
            "ScheduleKeyDeletion",
            "PutKeyPolicy",
            "CreateAlias",
            "DeleteAlias",
            "UpdateAlias",
            "EnableKeyRotation",
            "DisableKeyRotation",
            "UpdateKeyDescription",
            "ReplicateKey",
            "UpdatePrimaryRegion",
            "*CustomKeyStore",
        ],
    ),
    ops(
        "apigateway",
        &[
            "CreateRestApi",
            "DeleteRestApi",
            "UpdateRestApi",
            "ImportRestApi",
            "PutRestApi",
            "*Resource",
            "*Method",
            "*Integration",
            "*IntegrationResponse",
            "*MethodResponse",
            "*Authorizer",
            "*DomainName",
            "*BasePathMapping",
            "*Stage",
            "*UsagePlan*",
            "*VpcLink",
            "*RequestValidator",
            "*Model",
            "*GatewayResponse",
        ],
    ),
    ops(
        "apigatewayv2",
        &[
            "CreateApi",
            "DeleteApi",
            "UpdateApi",
            "ImportApi",
            "ReimportApi",
            "*Route",
            "*RouteResponse",
            "*Integration",
            "*IntegrationResponse",
            "*Authorizer",
            "*DomainName",
            "*ApiMapping",
            "*Stage",
            "*VpcLink",
            "*Model",
            "*RouteSettings",
            "*CorsConfiguration",
            "*AccessLogSettings",
        ],
    ),
    ops(
        "cloudfront",
        &[
            "*Distribution*",
            "*OriginAccessControl",
            "*CloudFrontOriginAccessIdentity",
            "*CachePolicy",
            "*OriginRequestPolicy",
            "*ResponseHeadersPolicy",
            "CreateFunction",
            "UpdateFunction",
            "DeleteFunction",
            "PublishFunction",
            "*KeyGroup",
            "*PublicKey",
            "*FieldLevelEncryption*",
            "*RealtimeLogConfig",
            "*ContinuousDeploymentPolicy",
            "*KeyValueStore",
            "*VpcOrigin",
            "AssociateAlias",
        ],
    ),
    ops(
        "route53",
        &[
            "CreateHostedZone",
            "DeleteHostedZone",
            "UpdateHostedZoneComment",
            "ChangeResourceRecordSets",
            "*VPCWithHostedZone",
            "*HealthCheck",
            "*TrafficPolicy*",
            "*QueryLoggingConfig",
            "*ReusableDelegationSet",
            "*KeySigningKey",
            "*HostedZoneDNSSEC",
            "*VPCAssociationAuthorization",
            "*CidrCollection",
        ],
    ),
    ops(
        "acm",
        &[
            "RequestCertificate",
            "ImportCertificate",
            "DeleteCertificate",
            "UpdateCertificateOptions",
        ],
    ),
    ops(
        "cloudwatch",
        &[
            "PutMetricAlarm",
            "PutCompositeAlarm",
            "DeleteAlarms",
            "PutDashboard",
            "DeleteDashboards",
            "PutAnomalyDetector",
            "DeleteAnomalyDetector",
            "PutMetricStream",
            "DeleteMetricStream",
            "PutInsightRule",
            "DeleteInsightRules",
            "PutManagedInsightRules",
        ],
    ),
    ops(
        "events",
        &[
            "PutRule",
            "DeleteRule",
            "PutTargets",
            "RemoveTargets",
            "*EventBus",
            "PutPermission",
            "RemovePermission",
            "*Connection",
            "*ApiDestination",
            "*Archive",
            "*Endpoint",
        ],
    ),
    ops(
        "scheduler",
        &[
            "CreateSchedule",
            "UpdateSchedule",
            "DeleteSchedule",
            "CreateScheduleGroup",
            "DeleteScheduleGroup",
        ],
    ),
    ops(
        "budgets",
        &[
            "CreateBudget",
            "UpdateBudget",
            "DeleteBudget",
            "CreateBudgetAction",
            "UpdateBudgetAction",
            "DeleteBudgetAction",
            "*Notification",
            "*Subscriber",
        ],
    ),
    ops(
        "batch",
        &["*ComputeEnvironment", "*JobQueue", "*SchedulingPolicy"],
    ),
    ops(
        "cloudformation",
        &[
            "CreateStack",
            "UpdateStack",
            "DeleteStack",
            "ExecuteChangeSet",
            "*StackSet",
            "*StackInstances",
            "SetStackPolicy",
            "UpdateTerminationProtection",
        ],
    ),
    ops(
        "cloudtrail",
        &[
            "CreateTrail",
            "DeleteTrail",
            "UpdateTrail",
            "PutEventSelectors",
            "PutInsightSelectors",
            "*EventDataStore",
            "*Channel",
            "PutResourcePolicy",
            "DeleteResourcePolicy",
        ],
    ),
    ops(
        "config",
        &[
            "*ConfigurationRecorder",
            "*DeliveryChannel",
            "*ConfigRule",
            "*ConformancePack",
            "*ConfigurationAggregator",
            "*AggregationAuthorization",
            "PutRemediationConfigurations",
            "DeleteRemediationConfiguration",
        ],
    ),
    ops(
        "guardduty",
        &[
            "CreateDetector",
            "DeleteDetector",
            "UpdateDetector",
            "*Filter",
            "*IPSet",
            "*ThreatIntelSet",
            "*PublishingDestination",
            "*MalwareProtectionPlan",
        ],
    ),
    ops(
        "wafv2",
        &[
            "Create*",
            "Update*",
            "Delete*",
            "Associate*",
            "Disassociate*",
            "Put*",
        ],
    ),
    ops(
        "network-firewall",
        &[
            "Create*",
            "Update*",
            "Delete*",
            "Associate*",
            "Disassociate*",
            "Put*",
        ],
    ),
    ops("vpc-lattice", &["Create*", "Update*", "Delete*", "Put*"]),
    ops("globalaccelerator", &["Create*", "Update*", "Delete*"]),
    ops(
        "directconnect",
        &[
            "Create*",
            "Delete*",
            "Update*",
            "Allocate*",
            "Associate*",
            "Disassociate*",
        ],
    ),
    ops("transfer", &["Create*", "Update*", "Delete*"]),
    ops("opensearchserverless", &["Create*", "Update*", "Delete*"]),
    ops("memorydb", &["Create*", "Update*", "Delete*"]),
    ops("kafka", &["Create*", "Update*", "Delete*"]),
    ops("mq", &["Create*", "Update*", "Delete*"]),
    ops("dax", &["Create*", "Update*", "Delete*"]),
    ops(
        "elasticbeanstalk",
        &[
            "CreateEnvironment",
            "TerminateEnvironment",
            "UpdateEnvironment",
            "CreateApplication",
            "DeleteApplication",
            "UpdateApplication",
        ],
    ),
    ops(
        "cognito-idp",
        &[
            "CreateUserPool",
            "DeleteUserPool",
            "UpdateUserPool",
            "*UserPoolClient",
            "*UserPoolDomain",
            "*IdentityProvider",
            "*ResourceServer",
            "SetUserPoolMfaConfig",
        ],
    ),
    ops(
        "appsync",
        &[
            "CreateGraphqlApi",
            "DeleteGraphqlApi",
            "UpdateGraphqlApi",
            "CreateApi",
            "DeleteApi",
            "UpdateApi",
            "*DataSource",
            "*Resolver",
            "*DomainName",
        ],
    ),
    ops(
        "amplify",
        &[
            "CreateApp",
            "DeleteApp",
            "UpdateApp",
            "*Branch",
            "*DomainAssociation",
            "*Webhook",
            "*BackendEnvironment",
        ],
    ),
    ops(
        "apprunner",
        &[
            "CreateService",
            "DeleteService",
            "UpdateService",
            "*AutoScalingConfiguration",
            "*Connection",
            "*VpcConnector",
            "*VpcIngressConnection",
            "*ObservabilityConfiguration",
        ],
    ),
    ops(
        "lightsail",
        &[
            "Create*",
            "Delete*",
            "Update*",
            "Attach*",
            "Detach*",
            "Allocate*",
            "Release*",
            "OpenInstancePublicPorts",
            "PutInstancePublicPorts",
            "CloseInstancePublicPorts",
        ],
    ),
];

pub(crate) struct RetryRow {
    pub(crate) service: &'static str,
    pub(crate) op: &'static str,
    pub(crate) retry: RetryClass,
}

const fn retry(service: &'static str, op: &'static str, retry: RetryClass) -> RetryRow {
    RetryRow { service, op, retry }
}

/// Retry classes the traits do not give.
pub(crate) static RETRY: &[RetryRow] = &[
    // A repeat writes the same bytes to the same key, or deletes what is
    // already gone.
    retry("s3", "PutObject", RetryClass::SafeToRepeat),
    retry("s3", "CopyObject", RetryClass::SafeToRepeat),
    retry("s3", "DeleteObject", RetryClass::SafeToRepeat),
    retry("s3", "DeleteObjects", RetryClass::SafeToRepeat),
    // A repeat mints another short-lived credential; the first expires.
    retry("sts", "AssumeRole*", RetryClass::SafeToRepeat),
    retry("sts", "AssumeRoot", RetryClass::SafeToRepeat),
    retry("sts", "GetSessionToken", RetryClass::SafeToRepeat),
    retry("sts", "GetFederationToken", RetryClass::SafeToRepeat),
    retry("sts", "GetDelegatedAccessToken", RetryClass::SafeToRepeat),
    retry("sts", "GetWebIdentityToken", RetryClass::SafeToRepeat),
    retry(
        "eks-auth",
        "AssumeRoleForPodIdentity",
        RetryClass::SafeToRepeat,
    ),
    retry("deadline", "AssumeFleetRoleFor*", RetryClass::SafeToRepeat),
    retry("deadline", "AssumeQueueRoleFor*", RetryClass::SafeToRepeat),
    retry("s3", "CreateSession", RetryClass::SafeToRepeat),
    retry("s3control", "GetDataAccess", RetryClass::SafeToRepeat),
    retry("ssm", "GetAccessToken", RetryClass::SafeToRepeat),
    retry(
        "lakeformation",
        "GetTemporaryDataLocationCredentials",
        RetryClass::SafeToRepeat,
    ),
    retry(
        "lakeformation",
        "AssumeDecoratedRoleWithSAML",
        RetryClass::SafeToRepeat,
    ),
    retry("gamelift", "GetComputeAccess", RetryClass::SafeToRepeat),
    retry("gamelift", "GetInstanceAccess", RetryClass::SafeToRepeat),
    retry(
        "gamelift",
        "RequestUploadCredentials",
        RetryClass::SafeToRepeat,
    ),
    retry(
        "finspace-data",
        "GetProgrammaticAccessCredentials",
        RetryClass::SafeToRepeat,
    ),
    retry(
        "finspace-data",
        "GetExternalDataViewAccessDetails",
        RetryClass::SafeToRepeat,
    ),
    retry(
        "emr",
        "GetClusterSessionCredentials",
        RetryClass::SafeToRepeat,
    ),
    retry(
        "emr-containers",
        "GetManagedEndpointSessionCredentials",
        RetryClass::SafeToRepeat,
    ),
    retry(
        "datazone",
        "GetEnvironmentCredentials",
        RetryClass::SafeToRepeat,
    ),
    retry("amplifyuibuilder", "RefreshToken", RetryClass::SafeToRepeat),
    retry(
        "bedrock-agentcore",
        "GetWorkloadAccessToken*",
        RetryClass::SafeToRepeat,
    ),
    retry(
        "bedrock-agentcore",
        "GetResourceOauth2Token",
        RetryClass::SafeToRepeat,
    ),
    retry("connect", "GetFederationToken", RetryClass::SafeToRepeat),
    retry(
        "ivs-realtime",
        "CreateParticipantToken",
        RetryClass::SafeToRepeat,
    ),
    retry("ivschat", "CreateChatToken", RetryClass::SafeToRepeat),
    retry("mwaa", "CreateCliToken", RetryClass::SafeToRepeat),
    retry("mwaa", "CreateWebLoginToken", RetryClass::SafeToRepeat),
    retry(
        "license-manager",
        "GetAccessToken",
        RetryClass::SafeToRepeat,
    ),
    retry(
        "redshift",
        "GetIdentityCenterAuthToken",
        RetryClass::SafeToRepeat,
    ),
    retry(
        "redshift-serverless",
        "GetIdentityCenterAuthToken",
        RetryClass::SafeToRepeat,
    ),
    retry(
        "workmail",
        "AssumeImpersonationRole",
        RetryClass::SafeToRepeat,
    ),
    retry(
        "kinesis-video-signaling",
        "GetIceServerConfig",
        RetryClass::SafeToRepeat,
    ),
    retry("sso", "GetRoleCredentials", RetryClass::SafeToRepeat),
    retry("ecr", "GetAuthorizationToken", RetryClass::SafeToRepeat),
    // A repeat would receive, and hide, other messages.
    retry("sqs", "ReceiveMessage", RetryClass::NonRepeatable),
];
