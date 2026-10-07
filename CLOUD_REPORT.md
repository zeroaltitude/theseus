# Cloud report: aws-mints (theseus-ye7o)

Branch `cloud/20261006-aws-mints`, cut from main at 57f265f2 (store format 23, unchanged: no stored record
changes). Three commits, one per step, plus this report.

## Step 1: the three rows (8b3e38cc)

**Found.** As the brief says, the catalog (aws-cli/2.34.15) holds sts `GetDelegatedAccessToken` (`R`),
sts `GetWebIdentityToken` (`R`) and eks-auth `AssumeRoleForPodIdentity` (`W`), none of them secret-bearing.

**Where the code differs from the brief.** The brief expected two of the three to fail closed on the rows
alone, because `walk` matches `NAMED` case-sensitively. They don't: the models mark the shapes sensitive.
- `GetDelegatedAccessToken`: `Credentials` is in `NAMED` (and `SecretAccessKey` is marked sensitive), so the
  walk holds `Credentials` whole.
- `GetWebIdentityToken`: `WebIdentityToken` (`webIdentityTokenType`) is marked sensitive.
- `AssumeRoleForPodIdentity`: the `credentials` structure (`Credentials`) is marked sensitive, so it is held
  whole.

All three are held with step 1's rows alone, with no change to `walk`. I still changed `walk` in step 3
(below), because other operations need it.

**Changed.** `tables.rs`:
- `CLASS`: rows for `GetDelegatedAccessToken` and `GetWebIdentityToken` (`Class::Write`, `MINT`), and one for
  eks-auth's mint, with a comment saying it is a write by its name already and the row is there for the note,
  as `AssumeRole*`'s is.
- `SECRET` and `RETRY` (`SafeToRepeat`): rows for all three.

`tests/mints.rs` (new) checks each mint's label `W 🔑`, its class, `SecretBearing::Always`, `SafeToRepeat`, and
the note, which equals `sts:GetSessionToken`'s (the MINT text). Core `secret.rs`'s
`the_catalogs_secret_members_are_found_and_masked` adds the three outputs, shaped as their models shape them,
with invented values. Each secret is held at its path (`Credentials`, `WebIdentityToken`, `credentials`), and
none of its value is left in the body.

## Step 2: through the core (0b65b5a1)

`crates/theseus-core/src/aws/tests_mints.rs` (new), on tests_handles' rig (`Fake`, `board`, `layer`, `tool`,
`tests::sts`):
- The stand-in answers STS's two mints in query-protocol XML, and eks-auth by its path
  (`…/assume-role-for-pod-identity`) in REST-JSON.
- Each call is planned and checked as a `Write`, then run with a binding (`aws.bind`, the way
  `tests_c2`'s writes run).
- The text and meta hold the handles (`meta.secrets` equals them exactly) and no value. The call's request
  rows hold no value. The board holds each value.

Step 3 adds ECR's `GetAuthorizationToken` to this test (below).

## Step 3: the shape rule, and every hit decided (420c4a5e)

**Where.** In theseus-core, `aws/tests_secret_shapes.rs`, beside secret.rs. It reads `secret::NAMED` itself
(now `pub(super)`), so the rule and the walk can't drift apart. Run time in a debug build: 0.58 to 0.78 s
under nextest (it decodes every service, as `every_service_decodes` does). The rule needed no generator
change: the compiled catalog keeps members, list members, map values, the sensitive mark, and paginator
output tokens. The generator's doc (`examples/theseus-aws-catalog-gen.rs`) has a new paragraph naming the rule
and the three decisions.

**The rule as built.** It walks every operation's output shape with a seen-set. A member is
credential-shaped when either:
- its name, in any case, is one of `NAMED`'s (which holds `Credentials`, `SecretAccessKey`, `SessionToken`);
  or
- the model marks its shape sensitive and its name ends in `Token`, `Password`, `Secret`, `Key` or
  `Credentials`.

These are never credential-shaped: a paginator's output token.

**Tightened.** The rule as briefed hit 288 operations, so I tightened it:
1. Any member named `NextToken`, in any case, is excluded too. 17 operations page without a paginator in their
   model, and chime-sdk's mark it sensitive.
2. A tag's key (`Key` or `tagKey` of a shape named `Tag`) is excluded. 8 services mark it sensitive.
3. An allowed row names a member, not a whole operation: `(service, operation globs, "Shape.member" glob,
   reason)`. One row then covers a family, for example rds's 29 operations that echo
   `PendingModifiedValues.MasterUserPassword`.

A row is stale, and fails, when it matches no credential-shaped member, or when it matches one in a
secret-bearing operation.

An operation with a hit must be secret-bearing, or each of its hits must be on an allowed row. After the
tightenings, 263 operations hit. Each is decided below.

**`walk` taught names in any case.** `named()` matches `NAMED` with `eq_ignore_ascii_case`. This was needed,
but not for the three mints. A check of every secret-bearing operation found that these already failed closed
on main, because their secret's name is lower case and unmarked:
- ecr `GetAuthorizationToken` and ecr-public `GetAuthorizationToken` (`authorizationToken`);
- lightsail `GetInstanceAccessDetails` (`password`, `privateKey`, …).

Every call to them answered "no member of its output was found to hold it". Now they hold, and so does
lightsail `CreateContainerServiceRegistryLogin` (`password`). ECR is now a case in secret.rs's walk test and in
tests_mints.rs (the JSON protocol, by its `x-amz-target`).

The same check found SECRET rows that still hold nothing and fail closed on every call. Each operation is
safe, but useless through `aws.call`. These are outside this step; a follow-up should teach their names:

| Operations | Members the walk misses |
|---|---|
| apigateway `CreateApiKey`, `GetApiKey`, `GetApiKeys` | `value` |
| appsync `CreateApiKey`, `ListApiKeys` | the key is its `id` |
| lightsail `CreateKeyPair`, `DownloadDefaultKeyPair` | `privateKeyBase64` and the like |

### Every hit, and its decision (for the owner's review)

**Mints:** a `CLASS` row (Write, MINT), `SECRET`, and `RETRY SafeToRepeat`. Each makes a short-lived
credential or token, always present in its answer.

| Service | Operations |
|---|---|
| sts | `GetDelegatedAccessToken`, `GetWebIdentityToken` (step 1) |
| eks-auth | `AssumeRoleForPodIdentity` (step 1) |
| deadline | `AssumeFleetRoleForRead`, `AssumeFleetRoleForWorker`, `AssumeQueueRoleForRead`, `AssumeQueueRoleForUser`, `AssumeQueueRoleForWorker` (as globs `AssumeFleetRoleFor*`, `AssumeQueueRoleFor*`) |
| s3 | `CreateSession` (S3 Express session keys; was R) |
| s3control | `GetDataAccess` (Access Grants keys; was R) |
| ssm | `GetAccessToken` (was R) |
| lakeformation | `GetTemporaryDataLocationCredentials` (was R), `AssumeDecoratedRoleWithSAML` |
| gamelift | `GetComputeAccess`, `GetInstanceAccess` (both were R), `RequestUploadCredentials` |
| finspace-data | `GetProgrammaticAccessCredentials`, `GetExternalDataViewAccessDetails` (both were R) |
| emr | `GetClusterSessionCredentials` (was R) |
| emr-containers | `GetManagedEndpointSessionCredentials` (was R) |
| datazone | `GetEnvironmentCredentials` (was R; its whole output shape is marked sensitive, so it is held whole) |
| signin | `CreateOAuth2Token` |
| amplifyuibuilder | `ExchangeCodeForToken`, `RefreshToken` |
| bedrock-agentcore | `GetWorkloadAccessToken`, `GetWorkloadAccessTokenForJWT`, `GetWorkloadAccessTokenForUserId` (glob `GetWorkloadAccessToken*`), `GetResourceOauth2Token` (all were R) |
| connect | `GetFederationToken` (was R) |
| ivs-realtime | `CreateParticipantToken` |
| ivschat | `CreateChatToken` |
| mwaa | `CreateCliToken`, `CreateWebLoginToken` |
| license-manager | `GetAccessToken` (was R) |
| redshift | `GetIdentityCenterAuthToken` (was R) |
| redshift-serverless | `GetIdentityCenterAuthToken` (was R) |
| workmail | `AssumeImpersonationRole` |
| kinesis-video-signaling | `GetIceServerConfig` (TURN credentials; was R) |

**Stored or made secrets read back:** `SECRET` alone, with their class unchanged.

| Service | Operations |
|---|---|
| datazone | `GetConnection`, only when `withSecret` is true (`secret_when`) |
| codepipeline | `GetJobDetails`, `GetThirdPartyJobDetails`, `PollForJobs` (the job's artifact keys) |
| gamelift | `CreateBuild` (upload keys) |
| iotsecuretunneling | `OpenTunnel`, `RotateTunnelAccessToken` |
| ivs-realtime | `CreateIngestConfiguration`, `GetIngestConfiguration`, `UpdateIngestConfiguration` (stream key) |
| license-manager | `CreateToken` (a long-lived refresh token) |
| route53domains | `TransferDomainToAnotherAwsAccount` (the transfer password) |
| pca-connector-scep | `CreateChallenge`, `GetChallengePassword` |
| pcs | `RegisterComputeNodeGroupInstance` (`sharedSecret`) |
| sso-oidc | `RegisterClient` (`clientSecret`) |
| wickr | `RegisterOidcConfig`, `RegisterOpentdfConfig`, `GetOidcInfo`, `GetOpentdfConfig` |
| finspace-data | `ResetUserPassword` |
| iot-managed-integrations | `CreateProvisioningProfile` (a private key) |
| iotwireless | `AssociateAwsAccountWithPartnerAccount` (`AppServerPrivateKey`) |
| location | `CreateKey`, `DescribeKey` (the API key) |
| payment-cryptography-data | `DecryptData` (plaintext, as kms `Decrypt`) |
| bedrock-agentcore | `GetResourceApiKey` |
| chime-sdk-meetings | `CreateAttendee`, `BatchCreateAttendee`, `GetAttendee`, `CreateMeetingWithAttendees` (join tokens; `Attendees` is a required input) |
| connect | `StartWebRTCContact` (join token) |
| connectparticipant | `CreateParticipantConnection` (join token) |
| chime | `CreateBot`, `GetBot`, `UpdateBot`, `RegenerateSecurityToken` (the bot's security token) |
| lightsail | `CreateContainerServiceRegistryLogin` (IaC-only, so `aws.call` refuses it anyway) |

**Allowed: not secrets, whatever the name** (`ALLOWED` in the test, each row with its reason):

| Service | Member | Operations | Why |
|---|---|---|---|
| apigateway | `Integration.credentials` | 10 ops | the role ARN an integration assumes |
| backupsearch | `S3ResultItem.ObjectKey` | `ListSearchJobResults` | an S3 object's key |
| bedrock-agentcore | `ExternalProxy.credentials` | `GetBrowserSession` | a Secrets Manager ARN |
| codepipeline | `ActionExecution.token`, `RuleExecution.token` | `GetPipelineState` | an approval's token, usable only with the caller's own credentials |
| cognito-idp | `TokenValidityUnitsType.*` | Create, Describe, Update `UserPoolClient` | the token lifetime's units |
| cognito-idp | `ListWebAuthnCredentials` | `ListWebAuthnCredentials` | passkeys' public descriptions |
| eks | `License.token` | the five EKS Anywhere subscription ops | a license entitlement, not account access (**owner: please confirm**) |
| entityresolution | `token` | `AddPolicyStatement`, `DeletePolicyStatement`, `GetPolicy`, `PutPolicy` | the policy's revision token |
| evs | `Environment.credentials` | `CreateEnvironment`, `DeleteEnvironment`, `GetEnvironment` | Secrets Manager ARNs |
| iotwireless | `ApplicationServerPublicKey` | `GetDeviceProfile` | a public key |
| kendra | `Credentials` | `DescribeDataSource` | a secret ARN |
| kms | `PublicKey` | `GetParametersForImport` | the wrapping public key |
| lightsail | `AccessKey.secretAccessKey` | `GetBucketAccessKeys` | AWS returns the secret only from `CreateBucketAccessKey`, which is already SECRET (from memory of AWS's API reference: **unverified here**) |
| neptunedata | `FastResetToken.token` | `ExecuteFastReset` | the reset's confirmation token |
| pipes | `PipeSource*Parameters.Credentials` | `DescribePipe` | secret ARNs |
| pipes | `PartitionKey` | `DescribePipe` | a Kinesis partition key |
| qconnect, wisdom | `plainText` | message templates and quick responses | content |
| quicksight | `PlainText` | 7 ops: visuals' text | content |
| rolesanywhere | `SubjectDetail.credentials` | `GetSubject` | certificates seen |
| socialmessaging | `associateInProgressToken` | `AssociateWhatsAppBusinessAccount` | a sign-up continuation token |
| ssm-sap | `Database.Credentials` | `GetDatabase` | secret ids |
| sso-admin | `Grant.RefreshToken` | `GetApplicationGrant`, `ListApplicationGrants` | a grant type, an empty structure |

**Allowed: echoed configuration (`ECHO`; owner's decision needed).** These are real secrets, configured on a
resource and echoed as one optional member of its description or of a write's result. `SECRET` would make the
whole call fail closed whenever the member is absent, which for most of these is the usual case, and for a
write, after it ran. So they stay as they are on main: if AWS echoes the value, the model sees it.

| Service | Member | Operations |
|---|---|---|
| amplify | `basicAuthCredentials` | App, Branch and their auto-branch config: 9 ops |
| appstream | `ServiceAccountCredentials.AccountPassword` | 3 ops |
| chime | `Bot.SecurityToken` | `ListBots` |
| chime-sdk-identity | `EndpointAttributes.DeviceToken`, `VoipDeviceToken` | `DescribeAppInstanceUserEndpoint` |
| chime-sdk-meetings | `Attendee.JoinToken` | `ListAttendees`, `UpdateAttendeeCapabilities` |
| cognito-idp | `UserPoolClientType.ClientSecret` | Create, Describe, Update `UserPoolClient` |
| connecthealth | `FHIRServer.oauthToken` | `GetPatientInsightsJob` |
| datasync | `FsxProtocolSmb.Password` | `DescribeLocationFsxOntap`, `DescribeLocationFsxOpenZfs` |
| datazone | connection credentials (`*`) | `CreateConnection`, `ListConnections`, `UpdateConnection` |
| dms | `*Settings.*Password` (14 members) | Create, Delete, Describe, Modify `Endpoint(s)` |
| ds | `RadiusSettings.SharedSecret` | `DescribeDirectories` |
| ec2 | `OidcOptions.ClientSecret` | the verified-access trust provider ops |
| ec2 | `TunnelOption.PreSharedKey` | the VPN connection ops, 6 |
| ec2 | `ExportVerifiedAccessInstanceClientConfiguration`'s `ClientSecret` | that op |
| fsx | `OntapFileSystemConfiguration.FsxAdminPassword` | 18 ops |
| iot | `SalesforceAction.token` | `GetTopicRule` |
| iot-managed-integrations | `DeviceSpecificKey` | `GetManagedThing` |
| ivs-realtime | `ParticipantToken.token` | `CreateStage` (tokens only when asked for) |
| lexv2-models | `EncryptionSetting.*Password` | 3 ops |
| medialive | `HlsAkamaiSettings.Token` | 8 channel ops |
| mediapackage | `IngestEndpoint.Password` | 7 channel ops |
| quicksight | asset bundle data source credentials | `DescribeAssetBundleImportJob` |
| rds | `*PendingModifiedValues.MasterUserPassword` | 29 ops |
| redshift | `PendingModifiedValues.MasterUserPassword` | 17 ops |

**Proposal for the owner.** Add `SecretBearing::WhenPresent`: hold what the walk finds, and don't fail closed
when nothing is found. Then every ECHO row can move to `SECRET`. It is about 30 lines (classify.rs, the
`p.secret` arm in tools.rs, describe.rs's flag). I didn't build it: it changes `aws.call`'s fail-closed
contract, which is the owner's call.

## Plants (each failed its test, then the file was restored and `touch`ed; `git status` clean after each)

- **Each of the three `SECRET` rows removed.**
  - `tests/mints.rs` failed: `left: "W"`, `right: "W 🔑"`.
  - Core `aws::tests_mints::each_mint_answers_with_handles_and_the_board_holds_its_values` failed at its
    `meta.secrets` assertion. The output came back plain: for example `GetDelegatedAccessToken: …
    "AssumedPrincipal": …`, the value in the text.
  - secret.rs's walk test calls `hold` directly, without the classification, so it doesn't fail on this
    plant. It holds the shapes; tests_mints holds the rows.
- **`GetDelegatedAccessToken`'s `CLASS` row removed.** `tests/mints.rs` failed: `left: "R 🔑"`,
  `right: "W 🔑"`.
- **`secret("s3", "CreateSession")` removed.** The rule failed, with 3 findings, each naming
  `s3:CreateSession` (`Credentials`, `Credentials.SecretAccessKey`, `Credentials.SessionToken`).
- **`secret("kms", "GetParametersForImport")` added** (an allowed operation). The rule failed:
  `stale: kms:GetParametersForImport is secret-bearing, and the allowed row for
  GetParametersForImportResponse.PublicKey still names it`.
- **`walk`'s any-case match reverted to `NAMED.contains`.**
  - secret.rs's walk test failed: `ecr:GetAuthorizationToken left: [] right:
    ["authorizationData[0].authorizationToken"]`.
  - tests_mints failed: `ecr:GetAuthorizationToken in us-west-2 returns a secret, and no member of its output
    was found to hold it, so none of its output is returned`.
  - This is the plant the brief asked for. Its target is ECR, since the three mints don't need the change.

## FAST

The rows are table lookups in `classify`, which runs when a call is planned and when `aws_describe` answers.
Nothing is new on the turn path or the start path. The rule is a test (0.6 to 0.8 s, debug).

## Proof, offline

- theseus-aws-catalog's suite: all pass. The lib suite has 18 tests, including
  `every_table_row_names_real_operations`, which holds every new row to a real operation. `tests/catalog.rs`
  has 10, with `GOLDEN` unchanged. `tests/mints.rs` has 1.
- Core: `aws::secret::*`, `aws::tests_mints`, `aws::tests_handles`, `aws::tests_secret_shapes::*`.
- The gate's suite covers every `aws::` test, and every test named secret, handle or redact across the
  workspace.
- Gate on steps 1 and 2 (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`): fmt, shape, features,
  clippy, cockpit and the test build were green. Suite: 3028 run, 2995 passed, 33 failed. All 33 are the known
  L1 failures (theseus-sandbox's contract and `spawn_100`, and theseusd's `sandbox` tests: a root VM with no
  job cgroup, theseus-pv6i). I ran the phases after the suite by hand: protocol.gen unchanged, and
  `cargo deny --offline check` reports advisories, bans, licenses and sources all ok.
- Gate on step 3 (420c4a5e): see the result section at the end.

## Live check (the maintainer's; a real mint needs account changes, so none is called)

On a scratch daemon of this build, with AWS bound (the maintainer's keys) and the stand-in model:

```sh
cat > /tmp/mints-rules.json <<'J'
[{"when": "describe the mints", "calls": [
  {"name": "aws_describe", "input": {"service": "sts", "operation": "GetDelegatedAccessToken"}},
  {"name": "aws_describe", "input": {"service": "sts", "operation": "GetWebIdentityToken"}},
  {"name": "aws_describe", "input": {"service": "eks-auth", "operation": "AssumeRoleForPodIdentity"}}]}]
J
theseus-sim fake-model --addr 127.0.0.1:9448 --rules /tmp/mints-rules.json &
# scratch config: [model] api_base = "http://127.0.0.1:9448", the [aws] account as usual
theseusd --config <scratch>/theseus.toml --socket <scratch>/sock --state-dir <scratch>/state &
theseus --socket <scratch>/sock ask "describe the mints"
theseus --socket <scratch>/sock history     # the most recent session
theseus --socket <scratch>/sock shutdown
```

`history` should show each of the three describes with `"class": "write"`, `"label": "W 🔑"`,
`"retry": "safe_to_repeat"` and `"note": "mints a credential, whatever its name says"`. On main, the two STS
ones read `"class": "read"` and `"label": "R"`, eks-auth's reads `"W"`, and none has 🔑.

The same check on, for example, s3 `CreateSession`, ssm `GetAccessToken` and ecr `GetAuthorizationToken` shows
the step-3 rows. With a real ECR repository, `aws_call ecr GetAuthorizationToken` now answers with the handle
`aws-secret:GetAuthorizationToken#authorizationData[0].authorizationToken`. On main it fails closed.

## Report also: two design questions

**Should `aws.call` refuse the STS mints to the model altogether?** I'd say yes, for `AssumeRole*`,
`AssumeRoot`, `GetSessionToken`, `GetFederationToken` and `GetDelegatedAccessToken`:
- The design's table says `AssumeRole` is "core only", and Theseus's own session machinery (session.rs, the
  hands, the broker's job sessions) already mints what it needs.
- A handle to keys the model can't use is no use to it, while a minted session still widens what exists in
  the world.
- Refusing these as invalid input, pointing to the session tools (as an IaC-only call points to the stack
  tools), would be a small `guard`-style check.

`GetWebIdentityToken` is different. A token for an outside OIDC service is the kind of thing an agent might
legitimately need to pass on, so it should stay callable as a secret-bearing write.

**Should `classify` take secret-bearing from the output's shape at run time?** Not instead of the table, I
think:
- The shape rule over-reaches for a run-time decision: it hits text bodies, role ARNs and tag keys, which need
  a person's reasons, and some real secrets are unmarked and oddly named (apigateway `value`, appsync `id`).
- A run-time rule would silently start masking, or fail closed on, describes a weekly update marks sensitive
  (rds's 29 operations would fail closed).
- The table, held by a test that fails the update, makes each new case a reviewed decision.

What would help at run time is the `WhenPresent` variant above, plus an any-case and sensitive-mark walk as a
second net. That second net already exists for secret-bearing calls.

## Left and uncertain

- **The ECHO list is the owner's to decide:** keep it as it is, or build `WhenPresent` and move it to SECRET.
  Until then those values reach the model as they do on main.
- **SECRET operations whose secret can be absent fail closed then:**
  - codepipeline `PollForJobs` with no jobs; bedrock-agentcore `GetResourceOauth2Token` when it answers with an
    authorization URL instead of a token; chime-sdk-meetings `BatchCreateAttendee` when every attendee errors;
    wickr's config reads without a client secret.
  - Each is safe, and says so. `WhenPresent` would fix them.
- **Pre-existing SECRET rows that always fail closed:** apigateway `CreateApiKey`, `GetApiKey`, `GetApiKeys`;
  appsync `CreateApiKey`, `ListApiKeys`; lightsail `CreateKeyPair`, `DownloadDefaultKeyPair`. A follow-up
  should teach `walk` their members.
- **Classes changed by MINT rows.** Several mints were reads and are now writes, so they now take the write
  posture: s3 `CreateSession`, ssm `GetAccessToken`, connect `GetFederationToken`, the bedrock-agentcore and
  redshift token reads, kinesis-video-signaling `GetIceServerConfig`, and the others marked "was R" above. The
  design calls for this. A config whose `[policy.aws]` opens reads but not writes will now ask for these.
- **The eks `License.token` and lightsail `GetBucketAccessKeys` reasons** come from my reading of AWS's
  documentation, without network access to check.
- **No doc edits.** docs/design/aws-toolset.md §3.1 could say that the rule holds "every STS credential
  mint", and every other credential-shaped output, to a table decision. The spec's Part III item, when written,
  should list the step-3 decisions.

## Gate result

The gate at 420c4a5e (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`):
- fmt, shape, features, clippy, cockpit and the test build: green.
- The suite: 3030 run, 2997 passed, 33 failed. All 33 are the known L1 failures on this root VM
  (theseus-pv6i):
  - theseus-sandbox: the `contract` tests (21) and `bench spawn_100`;
  - theseusd: the `sandbox` tests (11).
- None of the timing tests on the brief's flaky list failed, and nothing else failed.
- The phases after the suite, run by hand: `cockpit/src/protocol.gen` is unchanged (no protocol types
  changed), and `cargo deny --offline --log-level error check` reports advisories, bans, licenses and sources
  ok. `cargo deny fetch` succeeded during setup.
- The benches were skipped (`THESEUS_GATE_NO_BENCH=1`), as the brief says.

The first try of step 3's gate failed at clippy, because secret.rs's walk test grew to 102 lines (the limit is
100). The planted values moved into a `PLANTED` const, and the rerun above is green.
