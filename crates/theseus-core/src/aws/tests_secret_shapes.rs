//! The output-shape rule (theseus-ye7o): the next credential mint a weekly
//! catalog update brings fails here, before a call can hand its keys to the
//! model. It walks every operation's output shape in the embedded catalog
//! and finds each credential-shaped member.
//!
//! **The rule, in one sentence.** A member is credential-shaped when its
//! name is one that [`super::secret`]'s walk always holds (`NAMED`:
//! `Credentials`, `SecretAccessKey`, `SessionToken`, …), or when **AWS's own
//! model marks it `sensitive` and either its name or the name of the shape
//! it sits in carries a credential word** ([`WORDS`]: `token`, `password`,
//! `secret`, `key`, `credential`, `passphrase`, `seed`), or when its name
//! ends in one of the compound names of `secret::ENDS` — `ParticipantToken`,
//! `ConnectionToken`, `AuthToken`, `ClientSecret`, … — which are credentials
//! whatever the model says, or when one of two short lists names it
//! outright: the walk's `HELD`, for a member no model marks (API Gateway's
//! `ApiKey.value`, theseus-u4pe), and [`MARKED`], for a member the model
//! does mark whose name says nothing of what it is (the pre-shared keys
//! inside EC2's `VpnConnectionDeviceSampleConfiguration`). A `HELD` row that
//! no secret-bearing answer reaches is stale.
//!
//! **Why the marking and the words together, and neither alone**
//! (theseus-37ds). The marking alone is not a credential rule: AWS marks
//! `sensitive` to keep a value out of logs, so the marking covers personal
//! data and customer content too, and holding every marked member would
//! reach 1478 further operations and 2654 further members — a postal code, a
//! given name, a control's description, a tag's value — against this rule's
//! 162 and 118. The words alone are a guess about a name, and the names that
//! matter do not cooperate: a stream key arrives as `StreamKey.value`, a
//! Grafana key as `key`, a TOTP seed as `SecretCode`, a channel's key as
//! `Srt.passphrase`. Read together they keep the marking's authority — the
//! service that produced the value said it was sensitive — and then ask the
//! catalog's own type names, rather than a growing list of spellings, what
//! the value is. Reading the **shape's** words as well as the member's is
//! what finds `StreamKey.value`: `value` says nothing, and `StreamKey` says
//! all of it. Those four figures are measured, not remembered:
//! `the_rules_reach_is_what_the_design_doc_says` asserts them, and fails
//! with the new ones if the rule moves.
//!
//! **The marking is not enough on its own in the other direction either.**
//! A credential no model marks is invisible to it, which is why `NAMED`,
//! `ENDS` and `HELD` exist and why they are read by the walk as well as by
//! this rule. A secret-bearing operation that returns one of those beside a
//! secret it does hold is a *partial hold*, and worse than no hold at all,
//! because its answer reads as covered: Connect handed `ParticipantToken` to
//! the model in the same result as its held join token. Every
//! credential-shaped member of a secret-bearing answer must be one the walk
//! holds, checked below.
//!
//! Words are split at camelCase humps, `_` and `-`, and matched whole, so
//! `keyPrefix` and `SecretToAuthenticateInitiator` carry one and `Monkey`
//! and `Tokenizer` do not.
//!
//! A paginator's output token is none (some models mark `NextToken`
//! sensitive), and nor is any `NextToken` (many operations page without a
//! paginator) or a tag's key (`Tag.Key` in any case, marked sensitive by a
//! few).
//!
//! An operation with one must be secret-bearing (the catalog's `SECRET`
//! table, or `WHEN_PRESENT` for a secret a resource keeps only sometimes),
//! or each of its members must be on [`ALLOWED`] below, with the reason. An
//! allowed row that matches nothing, or that matches a secret-bearing
//! operation, fails too: it is stale; and so does a `WHEN_PRESENT`
//! operation with no such member, since its walk would never hold anything.
//!
//! The rule lives here, not in the catalog, so it reads the walk's own names.

use std::collections::HashSet;

use theseus_aws::catalog::{Catalog, OperationRef, SecretBearing, ShapeId, ShapeRef};

use super::secret::{HELD, NAMED};

/// Credential words. A marked member whose own name, or the name of the
/// shape holding it, carries one of these as a whole word is
/// credential-shaped. Keep the list short: each word costs an [`ALLOWED`]
/// row for every innocent member it reaches, and the owner reads those rows.
const WORDS: &[&str] = &[
    "token",
    "password",
    "secret",
    "key",
    "credential",
    "credentials",
    "passphrase",
    "seed",
];

/// Members AWS's model marks sensitive that are credentials though their
/// names say nothing of it, named by service, shape and member. The walk
/// needs no such list — it holds every marked member of a secret-bearing
/// answer — so this teaches only the rule below, which must see them to
/// require that their operation be secret-bearing at all.
const MARKED: &[(&str, &str, &str)] = &[
    // The device configuration AWS writes for a customer gateway carries
    // the tunnels' pre-shared keys in its body.
    (
        "ec2",
        "GetVpnConnectionDeviceSampleConfigurationResult",
        "VpnConnectionDeviceSampleConfiguration",
    ),
    // A thin client environment's activation code registers a device to it.
    ("workspaces-thin-client", "Environment", "activationCode"),
    (
        "workspaces-thin-client",
        "EnvironmentSummary",
        "activationCode",
    ),
    // A domain's transfer authorization code moves the domain away.
    (
        "route53domains",
        "RetrieveDomainAuthCodeResponse",
        "AuthCode",
    ),
    // The QR code encodes the same seed as the `Base32StringSeed` beside it.
    ("iam", "VirtualMFADevice", "QRCodePNG"),
    // A kdb connection string with the caller's signature in it: named for
    // what it connects to, not for the credential signed into it.
    (
        "finspace",
        "GetKxConnectionStringResponse",
        "signedConnectionString",
    ),
    // The rows below were found by `every_marked_candidate_is_on_a_list`
    // (theseus-3i6d1), which walks the catalog instead of reading this list:
    // every one is a marked member of a secret-bearing answer whose name says
    // nothing of what it is, and no other branch of the rule reaches it. The
    // walk holds them all at run time already (they are marked); what these
    // rows add is that their operation must stay secret-bearing.
    // the social provider's client id and client secret.
    ("amplifybackend", "SocialProviderSettings", "Facebook"),
    // the social provider's client id and client secret.
    ("amplifybackend", "SocialProviderSettings", "Google"),
    // the social provider's client id and client secret.
    (
        "amplifybackend",
        "SocialProviderSettings",
        "LoginWithAmazon",
    ),
    // the provider's key id, team id and private key.
    (
        "amplifybackend",
        "SocialProviderSettings",
        "SignInWithApple",
    ),
    // a signed URL that opens the session: whoever holds it is in.
    ("codecatalyst", "DevEnvironmentAccessDetails", "streamUrl"),
    // the structure holding that URL and the session token.
    (
        "codecatalyst",
        "StartDevEnvironmentSessionResponse",
        "accessDetails",
    ),
    // the auth session that carries a challenge to its answer.
    ("cognito-idp", "AdminInitiateAuthResponse", "Session"),
    // the auth session that carries a challenge to its answer.
    (
        "cognito-idp",
        "AdminRespondToAuthChallengeResponse",
        "Session",
    ),
    // the auth session that carries a challenge to its answer.
    ("cognito-idp", "InitiateAuthResponse", "Session"),
    // the auth session that carries a challenge to its answer.
    ("cognito-idp", "RespondToAuthChallengeResponse", "Session"),
    // the Oracle TDE password DMS uses to read encrypted redo logs.
    ("dms", "OracleSettings", "SecurityDbEncryption"),
    // the generated device configuration, with the tunnels' pre-shared keys in its body, exactly as GetVpnConnectionDeviceSampleConfiguration's is.
    ("ec2", "VpnConnection", "CustomerGatewayConfiguration"),
    // the claim certificate a device provisions itself with.
    (
        "iot-managed-integrations",
        "CreateProvisioningProfileResponse",
        "ClaimCertificate",
    ),
];

/// `name` in lower-case words, split at camelCase humps, `_` and `-`:
/// `keyPrefix` is `key`, `prefix`; `SSEKMSKeyId` is `ssekms`, `key`, `id`.
fn words(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let cs: Vec<char> = name.chars().collect();
    for (i, &c) in cs.iter().enumerate() {
        if c == '_' || c == '-' {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        // A hump starts a word: after a lower-case letter, or before one.
        let hump = c.is_ascii_uppercase()
            && i > 0
            && (cs[i - 1].is_ascii_lowercase()
                || (i + 1 < cs.len() && cs[i + 1].is_ascii_lowercase()));
        if hump && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(c.to_ascii_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `name` carries a credential word.
fn carries(name: &str) -> bool {
    words(name).iter().any(|w| WORDS.contains(&w.as_str()))
}

/// A member that names a Secrets Manager secret, and holds no value.
const NAMES_A_SECRET: &str = "names a Secrets Manager secret by its ARN or id; holds no value";
const PUBLIC_KEY: &str = "a public key";
const TEXT: &str = "text a person wrote (a template's, a visual's): content, no credential";
/// A member that points at a key and is not one (theseus-37ds).
const KEY_NAMED: &str = "a KMS key's ARN or id: it names the key, and holds none";
const ACCESS_KEY_ID: &str =
    "an access key's id, the public half; its secret is returned only where it is minted";
const LOCKER: &str = "a credential locker's own name or tags, not anything it holds";

/// One allowed member: a service, its operations (globs), the member as
/// `Shape.member` (a glob), and why it is no secret to hold.
struct Allowed {
    service: &'static str,
    ops: &'static [&'static str],
    member: &'static str,
    why: &'static str,
}

const fn allow(
    service: &'static str,
    ops: &'static [&'static str],
    member: &'static str,
    why: &'static str,
) -> Allowed {
    Allowed {
        service,
        ops,
        member,
        why,
    }
}

const ALL: &[&str] = &["*"];

static ALLOWED: &[Allowed] = &[
    // Not a secret, whatever the name.
    allow(
        "apigateway",
        ALL,
        "Integration.credentials",
        "the ARN of the role an integration assumes",
    ),
    allow(
        "appmesh",
        ALL,
        "*TlsFileCertificate.privateKey",
        "a file path on the proxy's file system: it names a key and holds none",
    ),
    allow(
        "backupsearch",
        ALL,
        "S3ResultItem.ObjectKey",
        "an S3 object's key: its name",
    ),
    allow(
        "bedrock-agentcore",
        &["GetBrowserSession"],
        "ExternalProxy.credentials",
        NAMES_A_SECRET,
    ),
    allow(
        "codepipeline",
        &["GetPipelineState"],
        "*Execution.token",
        "an approval's token: it answers the approval only with the caller's own credentials",
    ),
    allow(
        "cognito-idp",
        &["ListWebAuthnCredentials"],
        "ListWebAuthnCredentialsResponse.Credentials",
        "a user's passkeys, described: public",
    ),
    allow(
        "eks",
        ALL,
        "License.token",
        "an EKS Anywhere license's token: an entitlement to support, no access to the account",
    ),
    allow(
        "entityresolution",
        ALL,
        "*PolicyOutput.token",
        "a policy's revision token, for optimistic concurrency",
    ),
    allow(
        "entityresolution",
        ALL,
        "*PolicyStatementOutput.token",
        "a policy's revision token, for optimistic concurrency",
    ),
    allow("evs", ALL, "Environment.credentials", NAMES_A_SECRET),
    allow(
        "iotwireless",
        &["GetDeviceProfile"],
        "SidewalkGetDeviceProfile.ApplicationServerPublicKey",
        PUBLIC_KEY,
    ),
    allow(
        "kendra",
        &["DescribeDataSource"],
        "*.Credentials",
        NAMES_A_SECRET,
    ),
    allow(
        "kms",
        &["GetParametersForImport"],
        "GetParametersForImportResponse.PublicKey",
        PUBLIC_KEY,
    ),
    allow(
        "lightsail",
        &["GetBucketAccessKeys"],
        "AccessKey.secretAccessKey",
        "returned only by CreateBucketAccessKey, which is secret-bearing; never here",
    ),
    allow(
        "neptunedata",
        &["ExecuteFastReset"],
        "FastResetToken.token",
        "a reset's confirmation token: it confirms only with the caller's own credentials",
    ),
    allow(
        "pipes",
        &["DescribePipe"],
        "PipeSource*Parameters.Credentials",
        NAMES_A_SECRET,
    ),
    allow(
        "pipes",
        &["DescribePipe"],
        "PipeTargetKinesisStreamParameters.PartitionKey",
        "a Kinesis partition key: where a record goes",
    ),
    allow("qconnect", ALL, "*.plainText", TEXT),
    allow("quicksight", ALL, "*.PlainText", TEXT),
    allow(
        "rolesanywhere",
        &["GetSubject"],
        "SubjectDetail.credentials",
        "the certificates a subject used: public",
    ),
    allow(
        "socialmessaging",
        &["AssociateWhatsAppBusinessAccount"],
        "WhatsAppSignupCallbackResult.associateInProgressToken",
        "a sign-up's continuation token: it continues only with the caller's own credentials",
    ),
    allow(
        "ssm-sap",
        &["GetDatabase"],
        "Database.Credentials",
        NAMES_A_SECRET,
    ),
    allow(
        "sso-admin",
        ALL,
        "Grant.RefreshToken",
        "a grant's type, an empty structure: it holds no token",
    ),
    allow("wisdom", ALL, "*.plainText", TEXT),
    // A configured secret that the model's own doc says AWS never returns
    // (theseus-qan5; the rest of aws-mints' `ECHO` rows are `WHEN_PRESENT`).
    allow(
        "appstream",
        &["DescribeDirectoryConfigs"],
        "ServiceAccountCredentials.AccountPassword",
        "never returned: \"this password is not returned in the actual response\", says the \
         operation's doc",
    ),
    allow(
        "datasync",
        &["DescribeLocationFsxOntap"],
        "FsxProtocolSmb.Password",
        "never returned: the operation \"doesn't actually return a Password\", says its doc",
    ),
    allow(
        "datasync",
        &["DescribeLocationFsxOpenZfs"],
        "FsxProtocolSmb.Password",
        "never returned: \"response elements related to SMB aren't supported\" here, says the \
         operation's doc",
    ),
    allow(
        "fsx",
        ALL,
        "OntapFileSystemConfiguration.FsxAdminPassword",
        "never returned: \"the password value is always redacted in the response\", says the \
         member's doc",
    ),
    // theseus-37ds. Members the model marks sensitive that carry a
    // credential word, in their own name or in their shape's, and hold no
    // credential. Each is marked because it is personal or identifying, not
    // because it unlocks anything.
    //
    // A key named, never a key: an ARN or an id that points at one.
    allow(
        "chime-sdk-voice",
        ALL,
        "ServerSideEncryptionConfiguration.KmsKeyArn",
        KEY_NAMED,
    ),
    allow(
        "kendra",
        &["DescribeIndex"],
        "ServerSideEncryptionConfiguration.KmsKeyId",
        KEY_NAMED,
    ),
    allow(
        "ebs",
        &["StartSnapshot"],
        "StartSnapshotResponse.KmsKeyArn",
        KEY_NAMED,
    ),
    allow(
        "qbusiness",
        &["GetApplication"],
        "EncryptionConfiguration.kmsKeyId",
        KEY_NAMED,
    ),
    // Not `CreateSession`, which is secret-bearing: its walk holds it.
    allow(
        "s3",
        &[
            "CompleteMultipartUpload",
            "CopyObject",
            "CreateMultipartUpload",
            "GetObject",
            "HeadObject",
            "PutObject",
            "UploadPart",
            "UploadPartCopy",
        ],
        "*Output.SSEKMSKeyId",
        KEY_NAMED,
    ),
    allow(
        "s3",
        &["GetBucketEncryption"],
        "ServerSideEncryptionByDefault.KMSMasterKeyID",
        KEY_NAMED,
    ),
    allow(
        "s3",
        &["*BucketInventoryConfiguration*"],
        "SSEKMS.KeyId",
        KEY_NAMED,
    ),
    allow(
        "kinesisvideo",
        ALL,
        "MediaSourceConfig.MediaUriSecretArn",
        NAMES_A_SECRET,
    ),
    allow(
        "ssm-sap",
        &["GetDatabase"],
        "ApplicationCredential.SecretId",
        NAMES_A_SECRET,
    ),
    // The public half of a pair, or a public key outright.
    allow(
        "ec2",
        &["GetInstanceTpmEkPub"],
        "GetInstanceTpmEkPubResult.KeyValue",
        "the instance's TPM endorsement key, public half",
    ),
    allow(
        "kms",
        &["DescribeCustomKeyStores"],
        "XksProxyConfigurationType.AccessKeyId",
        ACCESS_KEY_ID,
    ),
    allow(
        "lightsail",
        &["GetBucketAccessKeys"],
        "AccessKey.accessKeyId",
        ACCESS_KEY_ID,
    ),
    // A name, a prefix or a label that happens to sit in a credential's
    // shape, or to be called a key.
    allow(
        "bedrock-agent",
        ALL,
        "PromptMetadataEntry.key",
        "a prompt metadata entry's key: the name of the datum beside it",
    ),
    allow(
        "qconnect",
        ALL,
        "RuntimeSessionData.key",
        "a session datum's key: the name of the datum beside it",
    ),
    allow(
        "workspaces-web",
        ALL,
        "S3LogConfiguration.keyPrefix",
        "the prefix of the S3 keys the session log is written under",
    ),
    allow(
        "cloudfront-keyvaluestore",
        &["GetKey"],
        "GetKeyResponse.Value",
        "the value stored under a key of a key-value store: the caller's own data",
    ),
    allow(
        "location",
        &["UpdateKey"],
        "UpdateKeyResponse.UpdateTime",
        "the time the key was updated: a timestamp",
    ),
    allow(
        "iot-managed-integrations",
        ALL,
        "GetCredentialLockerResponse.Name",
        LOCKER,
    ),
    allow(
        "iot-managed-integrations",
        ALL,
        "GetCredentialLockerResponse.Tags",
        LOCKER,
    ),
    allow(
        "iot-managed-integrations",
        ALL,
        "CredentialLockerSummary.Name",
        LOCKER,
    ),
    allow(
        "verifiedpermissions",
        ALL,
        "OpenIdConnect*TokenConfiguration*.principalIdClaim",
        "the name of the claim a principal's id is read from, not the claim's value",
    ),
    // Personal, and so marked; no credential.
    allow(
        "appstream",
        &["DescribeDirectoryConfigs"],
        "ServiceAccountCredentials.AccountName",
        "the directory service account's user name; the password beside it is never returned",
    ),
    allow(
        "quicksight",
        &["DescribeActionConnector"],
        "ReadAPIKeyConnectionMetadata.Email",
        "an email address on a connector's metadata",
    ),
];

/// A glob over names: `*` matches any run of characters.
fn glob(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, rest)) => {
            let Some(tail) = name.strip_prefix(head) else {
                return false;
            };
            (0..=tail.len()).any(|i| tail.is_char_boundary(i) && glob(rest, &tail[i..]))
        }
    }
}

/// `member` of `parent` in `service`'s answers is credential-shaped: one of
/// [`NAMED`]; or marked sensitive by the model, with a credential word in
/// its own name or in `parent`'s; or named by a [`HELD`] or [`MARKED`] row.
fn credential_shaped(service: &str, parent: &str, member: &str, shape: ShapeRef<'_>) -> bool {
    let row = |rows: &[(&str, &str, &str)]| {
        rows.iter()
            .any(|(s, p, m)| *s == service && *p == parent && *m == member)
    };
    // `named` carries both [`NAMED`] and the unmarked compound names of
    // `secret::ENDS`, so the rule and the walk cannot drift apart.
    super::secret::named(member, shape)
        || (shape.is_sensitive() && (carries(member) || carries(parent)))
        || row(HELD)
        || row(MARKED)
}

/// Whether [`super::secret`]'s walk would hold this member: the model marks
/// it, its name is one of [`NAMED`], or a [`HELD`] row names it. This is
/// `secret.rs`'s own test, repeated here so the rule can ask what the walk
/// would really do with what it finds.
fn walk_holds(service: &str, parent: &str, member: &str, shape: ShapeRef<'_>) -> bool {
    shape.is_sensitive()
        || super::secret::named(member, shape)
        || HELD
            .iter()
            .any(|(s, p, m)| *s == service && *p == parent && *m == member)
}

/// One credential-shaped member of an output: `Shape.member`, the path it
/// was first reached by, and whether the walk would hold it — itself, or
/// under an ancestor the walk holds and masks whole.
struct Hit {
    member: String,
    path: String,
    held: bool,
}

/// One operation's walk over its output shape: what it is walking, and what
/// it has found. The state rides here rather than in a long argument list.
struct Scan<'a> {
    service: &'a str,
    /// The paginator's output tokens, which are no credential.
    tokens: &'a [&'a str],
    seen: HashSet<ShapeId>,
    out: Vec<Hit>,
    /// Whether the walk would hold anything at all in this output.
    any_held: bool,
}

impl Scan<'_> {
    fn walk(&mut self, shape: ShapeRef<'_>, path: &str, held_above: bool) {
        if !self.seen.insert(shape.id()) {
            return;
        }
        for m in shape.members() {
            let p = if path.is_empty() {
                m.name().to_string()
            } else {
                format!("{path}.{}", m.name())
            };
            let skipped = self.tokens.contains(&m.name())
                || m.name().eq_ignore_ascii_case("NextToken")
                // A tag's key, however the service spells it.
                || (shape.name() == "Tag"
                    && ["Key", "tagKey"]
                        .iter()
                        .any(|k| k.eq_ignore_ascii_case(m.name())));
            // The walk stops at the first member it holds and masks
            // everything under it, so an ancestor's hold covers this one.
            let held = held_above || walk_holds(self.service, shape.name(), m.name(), m.shape());
            if !skipped && held {
                // Something in this answer is held, whether or not the rule
                // calls it credential-shaped: SSM's `Parameter.Value` is
                // held by its marking alone, and is no credential by name.
                self.any_held = true;
            }
            if !skipped && credential_shaped(self.service, shape.name(), m.name(), m.shape()) {
                self.out.push(Hit {
                    member: format!("{}.{}", shape.name(), m.name()),
                    path: p.clone(),
                    held,
                });
            }
            self.walk(m.shape(), &p, held);
        }
        if let Some(m) = shape.list_member() {
            self.walk(m.shape(), &format!("{path}[]"), held_above);
        }
        if let Some(m) = shape.map_value() {
            self.walk(m.shape(), &format!("{path}{{}}"), held_above);
        }
    }
}

/// The operation's credential-shaped output members, and whether the walk
/// would hold anything at all in that output.
fn hits(op: OperationRef<'_>) -> (Vec<Hit>, bool) {
    let Some(output) = op.output() else {
        return (Vec::new(), false);
    };
    let tokens: Vec<&str> = op
        .paginator()
        .map(|p| p.output_tokens())
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.rsplit('.').next().unwrap_or(t))
        .collect();
    let mut scan = Scan {
        service: op.service().name(),
        tokens: &tokens,
        seen: HashSet::new(),
        out: Vec::new(),
        any_held: false,
    };
    scan.walk(output, "", false);
    (scan.out, scan.any_held)
}

fn matches(row: &Allowed, service: &str, op: &str, member: &str) -> bool {
    row.service == service && row.ops.iter().any(|p| glob(p, op)) && glob(row.member, member)
}

/// The vocabulary reads whole words, so a credential's name carries one and
/// an innocent name that merely contains the letters does not.
#[test]
fn credential_words_are_whole_words() {
    assert_eq!(words("keyPrefix"), ["key", "prefix"]);
    assert_eq!(words("SSEKMSKeyId"), ["ssekms", "key", "id"]);
    assert_eq!(words("Base32StringSeed"), ["base32", "string", "seed"]);
    assert_eq!(words("activation_code"), ["activation", "code"]);
    for yes in [
        "key",
        "value",
        "StreamKey",
        "Srt.passphrase",
        "SecretCode",
        "ClientSecretValue",
        "SecretToAuthenticateInitiator",
        "Base32StringSeed",
        "tokenValue",
        "CreateWorkspaceApiKeyResponse",
    ] {
        // `value` and `Srt.passphrase` carry nothing on their own; they are
        // reached through their shape's words, checked by the rule.
        if ["value", "Srt.passphrase"].contains(&yes) {
            continue;
        }
        assert!(carries(yes), "{yes} carries a credential word");
    }
    for no in [
        "Monkey",
        "Tokenizer",
        "Keyboard",
        "PostalCode",
        "Description",
        "GivenName",
        "secretariat",
    ] {
        assert!(!carries(no), "{no} carries none");
    }
}

/// No [`MARKED`] row can be deleted in silence (theseus-37ds).
///
/// Staleness guards one direction only: a row that reaches nothing fails.
/// The other direction is the dangerous one — delete a row that is the only
/// thing making a member credential-shaped and the suite can stay green,
/// because the member is marked, so the walk still holds it and the
/// operation is still secret-bearing. Nothing would notice that its
/// operation is no longer *required* to be secret-bearing, and a later
/// change could quietly take the row off the table.
///
/// So each row must be load-bearing: without it, no other branch of
/// [`credential_shaped`] may reach the same member. A row this test calls
/// redundant should be deleted, not kept.
#[test]
fn no_marked_row_can_be_deleted_in_silence() {
    let c = Catalog::embedded().expect("the embedded catalog decodes");
    let mut bad = Vec::new();
    for (service, parent, member) in MARKED {
        let Ok(svc) = c.service(service) else {
            bad.push(format!("{service} is no service of the catalog"));
            continue;
        };
        let Some(shape) = svc.shapes().find(|s| s.name() == *parent) else {
            bad.push(format!("{service} has no shape {parent}"));
            continue;
        };
        let Some(m) = shape.member(member) else {
            bad.push(format!("{service}'s {parent} has no member {member}"));
            continue;
        };
        // Every branch of the rule except `MARKED` itself.
        let without = super::secret::named(member, m.shape())
            || (m.shape().is_sensitive() && (carries(member) || carries(parent)))
            || HELD
                .iter()
                .any(|(s, p, x)| s == service && p == parent && x == member);
        if without {
            bad.push(format!(
                "redundant: the marked row ({service}, {parent}, {member}) is reached by \
                 another branch of the rule, so deleting it would change nothing: delete it"
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "{} findings:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// The rule as it was before theseus-37ds: a sensitive member whose own
/// name *ends* in one of these, plus `NAMED` and `HELD`. Kept so the reach
/// of the rule can be measured against what it replaced.
const BEFORE_37DS: &[&str] = &["Token", "Password", "Secret", "Key", "Credentials"];

/// How far a rule reaches: operations, and distinct `Shape.member`.
fn reach(c: &Catalog, rule: &dyn Fn(&str, &str, &str, ShapeRef<'_>) -> bool) -> (usize, usize) {
    fn go(
        rule: &dyn Fn(&str, &str, &str, ShapeRef<'_>) -> bool,
        service: &str,
        shape: ShapeRef<'_>,
        tokens: &[&str],
        seen: &mut HashSet<ShapeId>,
        out: &mut Vec<String>,
    ) {
        if !seen.insert(shape.id()) {
            return;
        }
        for m in shape.members() {
            let skipped = tokens.contains(&m.name())
                || m.name().eq_ignore_ascii_case("NextToken")
                || (shape.name() == "Tag"
                    && ["Key", "tagKey"]
                        .iter()
                        .any(|k| k.eq_ignore_ascii_case(m.name())));
            if !skipped && rule(service, shape.name(), m.name(), m.shape()) {
                out.push(format!("{}.{}", shape.name(), m.name()));
            }
            go(rule, service, m.shape(), tokens, seen, out);
        }
        if let Some(m) = shape.list_member() {
            go(rule, service, m.shape(), tokens, seen, out);
        }
        if let Some(m) = shape.map_value() {
            go(rule, service, m.shape(), tokens, seen, out);
        }
    }
    let old = |service: &str, parent: &str, member: &str, shape: ShapeRef<'_>| {
        NAMED.iter().any(|n| n.eq_ignore_ascii_case(member))
            || (shape.is_sensitive() && BEFORE_37DS.iter().any(|e| member.ends_with(e)))
            || HELD
                .iter()
                .any(|(s, p, m)| *s == service && *p == parent && *m == member)
    };
    let (mut ops, mut members) = (0usize, std::collections::BTreeSet::new());
    for e in c.services() {
        let svc = c.service(&e.name).expect("the service decodes");
        for op in svc.operations() {
            let Some(output) = op.output() else { continue };
            let tokens: Vec<&str> = op
                .paginator()
                .map(|p| p.output_tokens())
                .unwrap_or_default()
                .into_iter()
                .map(|t| t.rsplit('.').next().unwrap_or(t))
                .collect();
            let mut now = Vec::new();
            let mut was = Vec::new();
            go(
                rule,
                &e.name,
                output,
                &tokens,
                &mut HashSet::new(),
                &mut now,
            );
            go(
                &old,
                &e.name,
                output,
                &tokens,
                &mut HashSet::new(),
                &mut was,
            );
            let added: Vec<&String> = now.iter().filter(|m| !was.contains(m)).collect();
            if !added.is_empty() {
                ops += 1;
                for m in added {
                    members.insert(m.clone());
                }
            }
        }
    }
    (ops, members.len())
}

/// The numbers the module doc and `docs/design/aws-toolset.md` quote for why
/// the rule is a conjunction, measured rather than remembered.
///
/// These are committed in a design document, and a wrong number in a design
/// document is believed by whoever reads it next. So they are asserted here:
/// change the rule and this fails with the new figures, which is the prompt
/// to correct the prose in the same commit.
///
/// Both are counted the same way and against the same baseline — the rule as
/// it stood before theseus-37ds — over every operation in the catalog, and
/// independently of which operations the `SECRET` tables happen to list.
#[test]
fn the_rules_reach_is_what_the_design_doc_says() {
    let c = Catalog::embedded().expect("the embedded catalog decodes");
    let conjunction = reach(c, &credential_shaped);
    // What taking AWS's `sensitive` marking on its own would have reached.
    let marking_alone = |_service: &str, _parent: &str, member: &str, shape: ShapeRef<'_>| {
        shape.is_sensitive() || NAMED.iter().any(|n| n.eq_ignore_ascii_case(member))
    };
    let marking = reach(c, &marking_alone);
    assert_eq!(
        (conjunction, marking),
        ((162, 118), (1478, 2654)),
        "the rule's reach moved: the conjunction now selects {conjunction:?} \
         (operations, members) beyond the rule it replaced, and the marking alone would \
         select {marking:?}. Correct the module doc above and docs/design/aws-toolset.md."
    );
}

#[test]
fn globs_match_as_the_catalogs_do() {
    assert!(glob("*", "Anything"));
    assert!(glob("*Settings.*Password", "KafkaSettings.SaslPassword"));
    assert!(!glob("*Settings.*Password", "KafkaSettings.SaslUser"));
    assert!(glob("Tag.Key", "Tag.Key"));
    assert!(!glob("Tag.Key", "Tag.Keys"));
}

/// Every operation whose output holds a credential-shaped member is
/// secret-bearing, or allowed here with its reason; and no allowed row is
/// stale.
#[test]
fn every_credential_shaped_output_is_secret_bearing_or_allowed() {
    let c = Catalog::embedded().expect("the embedded catalog decodes");
    let mut used = vec![false; ALLOWED.len()];
    let mut held_used = vec![false; HELD.len()];
    let mut marked_used = vec![false; MARKED.len()];
    let mut bad = Vec::new();
    for e in c.services() {
        let svc = c.service(&e.name).expect("the service decodes");
        for op in svc.operations() {
            bad.extend(scan_operation(
                &e.name,
                op,
                &mut used,
                &mut held_used,
                &mut marked_used,
            ));
        }
    }
    for (row, used) in ALLOWED.iter().zip(&used) {
        assert!(!row.why.is_empty());
        // A row names its member (theseus-b586): one whose member is a glob
        // alone (`*`, `Shape.*`) would pass any credential a weekly update
        // adds to that shape unseen.
        let named = row
            .member
            .rsplit_once('.')
            .is_some_and(|(_, m)| m.chars().any(char::is_alphanumeric));
        if !named {
            bad.push(format!(
                "the allowed row {}:{:?} {} names no member: name each",
                row.service, row.ops, row.member
            ));
        }
        if !used {
            bad.push(format!(
                "stale: the allowed row {}:{:?} {} matches no credential-shaped member",
                row.service, row.ops, row.member
            ));
        }
    }
    for (row, used) in HELD.iter().zip(&held_used) {
        if !used {
            bad.push(format!(
                "stale: the walk's held row {row:?} is in no secret-bearing answer"
            ));
        }
    }
    for (row, used) in MARKED.iter().zip(&marked_used) {
        if !used {
            bad.push(format!(
                "stale: the marked row {row:?} is in no secret-bearing answer"
            ));
        }
    }
    bad.dedup();
    assert!(
        bad.is_empty(),
        "{} findings:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// One operation's findings, marking the rows that reached it.
fn scan_operation(
    service: &str,
    op: OperationRef<'_>,
    used: &mut [bool],
    held_used: &mut [bool],
    marked_used: &mut [bool],
) -> Vec<String> {
    let kind = op.classify().secret;
    let secret = kind != SecretBearing::No;
    let (hits, any_held) = hits(op);
    let mut bad = Vec::new();
    // Every secret-bearing operation must leave the walk something to hold:
    // a `when_present` one would otherwise answer whole, always, and an
    // `Always` one would fail closed on every call.
    //
    // This is also what keeps [`MARKED`] and [`HELD`] *complete* rather than
    // merely unstale (theseus-37ds): their staleness check fails a row that
    // reaches nothing, and this one fails the other way — delete the row
    // that is the only thing making an operation's member credential-shaped
    // and its operation is left secret-bearing with nothing to hold, which
    // fails here. A list whose rows can be deleted in silence is not
    // guarded.
    if secret && !any_held {
        bad.push(format!(
            "stale: {service}:{} is secret-bearing ({kind:?}), and the walk would hold \
             nothing in its output",
            op.name()
        ));
    }
    for hit in hits {
        for (i, (s, p, m)) in HELD.iter().enumerate() {
            if secret && *s == service && hit.member == format!("{p}.{m}") {
                held_used[i] = true;
            }
        }
        for (i, (s, p, m)) in MARKED.iter().enumerate() {
            if secret && *s == service && hit.member == format!("{p}.{m}") {
                marked_used[i] = true;
            }
        }
        let rows: Vec<usize> = (0..ALLOWED.len())
            .filter(|&i| matches(&ALLOWED[i], service, op.name(), &hit.member))
            .collect();
        for &i in &rows {
            used[i] = true;
            if secret {
                bad.push(format!(
                    "stale: {service}:{} is secret-bearing, and the allowed row for {} still \
                     names it",
                    op.name(),
                    ALLOWED[i].member
                ));
            }
        }
        if !secret && rows.is_empty() {
            bad.push(format!(
                "{service}:{} returns {} ({}), credential-shaped, and is not secret-bearing: \
                 a mint (CLASS with MINT, SECRET, RETRY), a stored secret (SECRET), or an \
                 allowed row with its reason",
                op.name(),
                hit.path,
                hit.member
            ));
        }
        // A partial hold is worse than no hold: the call is secret-bearing,
        // so its answer reads as covered, and it hands this credential to
        // the model beside the one it held (theseus-37ds; the row named
        // Connect's `ParticipantToken` beside its held join token). Every
        // credential-shaped member of a secret-bearing answer must be one
        // the walk holds: marked, in `NAMED` or `ENDS`, or a `HELD` row.
        if secret && !hit.held {
            bad.push(format!(
                "partial hold: {service}:{} is secret-bearing and returns {} ({}), which the \
                 walk does not hold: it reaches the model beside the secrets that were held. \
                 Mark it with a HELD row, or name it in NAMED or ENDS",
                op.name(),
                hit.path,
                hit.member
            ));
        }
    }
    bad
}

/// Members of a secret-bearing answer that AWS marks `sensitive` and whose
/// names say nothing of what they are — the shape [`MARKED`] exists for —
/// which someone has looked at and judged **not** credentials
/// (theseus-3i6d1).
///
/// A row is a decision, so write what the member actually is. A row that
/// matches nothing is stale and fails, exactly as an [`ALLOWED`] row does.
const TRIAGED: &[(&str, &str, &str, &str)] = &[
("amplify", "App", "buildSpec", "the app's build specification, the project's own build file"),
    ("amplify", "AutoBranchCreationConfig", "buildSpec", "the build specification a new branch inherits"),
    ("amplify", "Branch", "buildSpec", "the branch's build specification"),
    ("amplifybackend", "CreateBackendAuthVerificationMessageConfig", "EmailSettings", "the verification email's subject and body"),
    ("amplifybackend", "CreateBackendAuthVerificationMessageConfig", "SmsSettings", "the verification message's text"),
    ("chime", "Bot", "BotEmail", "an email address"),
    ("chime", "Bot", "DisplayName", "a display name"),
    ("chime-sdk-identity", "AppInstanceUserEndpoint", "Name", "an endpoint's name"),
    ("chime-sdk-meetings", "Attendee", "ExternalUserId", "the caller's own id for an attendee; the attendee's JoinToken is held by name"),
    ("chime-sdk-meetings", "CreateAttendeeError", "ExternalUserId", "the same id, echoed on the error"),
    ("chime-sdk-meetings", "Meeting", "ExternalMeetingId", "the caller's own id for the meeting"),
    ("chime-sdk-meetings", "Meeting", "MeetingHostId", "the host's identifier"),
    ("cognito-idp", "UserPoolClientType", "ClientId", "the app client's public id; its ClientSecret carries a credential word and is held by the rule"),
    ("connecthealth", "PatientInsightsEncounterContext", "encounterReason", "clinical text about a patient"),
    ("connecthealth", "PatientInsightsPatientContext", "dateOfBirth", "a patient's date of birth"),
    ("connecthealth", "PatientInsightsPatientContext", "patientId", "a patient's identifier"),
    ("connecthealth", "PatientInsightsPatientContext", "pronouns", "a patient's pronouns"),
    ("connecthealth", "UserContext", "userId", "the requesting clinician's identifier"),
    ("datazone", "CreateConnectionOutput", "description", "the connection's description"),
    ("datazone", "GetConnectionOutput", "description", "the connection's description"),
    ("datazone", "UpdateConnectionOutput", "description", "the connection's description"),
    ("ds", "DirectoryDescription", "ShareNotes", "the note sent with a directory share request"),
    ("firehose", "HttpEndpointCommonAttribute", "AttributeName", "the name of an attribute the operator chose to send with each record"),
    ("firehose", "HttpEndpointCommonAttribute", "AttributeValue", "its value, which the operator chose; the destination's AccessKey is held by name"),
    ("firehose", "HttpEndpointDescription", "Url", "the HTTP destination's URL"),
    ("firehose", "RedshiftDestinationDescription", "Username", "a user name; the password beside it carries a credential word"),
    ("firehose", "SnowflakeDestinationDescription", "AccountUrl", "the Snowflake account's URL"),
    ("firehose", "SnowflakeDestinationDescription", "ContentColumnName", "a column name"),
    ("firehose", "SnowflakeDestinationDescription", "Database", "a database name"),
    ("firehose", "SnowflakeDestinationDescription", "MetaDataColumnName", "a column name"),
    ("firehose", "SnowflakeDestinationDescription", "Schema", "a schema name"),
    ("firehose", "SnowflakeDestinationDescription", "Table", "a table name"),
    ("firehose", "SnowflakeDestinationDescription", "User", "a user name; the private key beside it carries a credential word"),
    ("firehose", "SnowflakeRoleConfiguration", "SnowflakeRole", "a role name"),
    ("firehose", "SnowflakeVpcConfiguration", "PrivateLinkVpceId", "a VPC endpoint id"),
    ("gamelift", "InstanceAccess", "IpAddress", "the instance's address; the Credentials beside it are held by name"),
    ("glue", "Job", "CodeGenConfigurationNodes", "the visual job's generated node graph"),
    ("grafana", "WorkspaceDescription", "description", "the workspace's description; its grafanaToken is held by a HELD row"),
    ("grafana", "WorkspaceDescription", "name", "the workspace's name"),
    ("grafana", "WorkspaceDescription", "organizationRoleName", "a role name"),
    ("grafana", "WorkspaceDescription", "organizationalUnits", "organizational unit names"),
    ("grafana", "WorkspaceDescription", "workspaceRoleArn", "a role's ARN"),
    ("grafana", "WorkspaceSummary", "description", "the workspace's description; its grafanaToken is held by a HELD row"),
    ("grafana", "WorkspaceSummary", "name", "the workspace's name"),
    ("iot-managed-integrations", "GetManagedThingResponse", "Brand", "a device's brand"),
    ("iot-managed-integrations", "GetManagedThingResponse", "Classification", "a device's classification"),
    ("iot-managed-integrations", "GetManagedThingResponse", "ConnectorDeviceId", "a device's id with its connector"),
    ("iot-managed-integrations", "GetManagedThingResponse", "InternationalArticleNumber", "a device's article number"),
    ("iot-managed-integrations", "GetManagedThingResponse", "MacAddress", "a device's MAC address"),
    ("iot-managed-integrations", "GetManagedThingResponse", "Model", "a device's model"),
    ("iot-managed-integrations", "GetManagedThingResponse", "Owner", "a device's owner"),
    ("iot-managed-integrations", "GetManagedThingResponse", "SerialNumber", "a device's serial number"),
    ("iot-managed-integrations", "GetManagedThingResponse", "Tags", "the device's tags"),
    ("iot-managed-integrations", "GetManagedThingResponse", "UniversalProductCode", "a device's product code"),
    ("mediapackage", "IngestEndpoint", "Username", "the ingest user's name; its password carries a credential word"),
    ("quicksight", "AssetBundleImportSourceDescription", "Body", "where the bundle under import was read from"),
    ("s3", "CreateSessionOutput", "SSEKMSEncryptionContext", "the KMS encryption context, which is additional authenticated data and not a key"),
    ("ssm", "Parameter", "Value", "whatever the operator stored; held by its marking alone and no credential by name, as the module doc says"),
    ("ssm", "ParameterHistory", "Value", "the same value, in its history"),
    ("workspaces-thin-client", "Environment", "desktopEndpoint", "the desktop service's URL"),
    ("workspaces-thin-client", "Environment", "deviceCreationTags", "the tags a new device is given"),
    ("workspaces-thin-client", "Environment", "name", "the environment's name; its activationCode is on MARKED"),
    ("workspaces-thin-client", "EnvironmentSummary", "desktopEndpoint", "the desktop service's URL"),
    ("workspaces-thin-client", "EnvironmentSummary", "name", "the environment's name; its activationCode is on MARKED"),
];

/// One candidate for [`MARKED`]: a marked, wordless member of a
/// secret-bearing answer that no branch of [`credential_shaped`] reaches.
fn marked_candidates(c: &Catalog) -> std::collections::BTreeMap<String, String> {
    fn go(
        service: &str,
        shape: ShapeRef<'_>,
        tokens: &[&str],
        seen: &mut HashSet<ShapeId>,
        out: &mut std::collections::BTreeMap<String, String>,
        op: &str,
    ) {
        if !seen.insert(shape.id()) {
            return;
        }
        for m in shape.members() {
            let skipped = tokens.contains(&m.name())
                || m.name().eq_ignore_ascii_case("NextToken")
                || (shape.name() == "Tag"
                    && ["Key", "tagKey"]
                        .iter()
                        .any(|k| k.eq_ignore_ascii_case(m.name())));
            // Marked by AWS, and wordless: neither its own name nor the name
            // of the shape holding it says what it is. That is precisely the
            // case a reader cannot settle from the catalog alone, and so
            // precisely the case a list must settle.
            let candidate = !skipped
                && m.shape().is_sensitive()
                && !carries(m.name())
                && !carries(shape.name())
                && !super::secret::named(m.name(), m.shape())
                && !HELD
                    .iter()
                    .any(|(s, p, x)| *s == service && *p == shape.name() && *x == m.name());
            if candidate {
                out.entry(format!("{service} {} {}", shape.name(), m.name()))
                    .or_insert_with(|| format!("in {service}:{op}'s answer"));
            }
            go(service, m.shape(), tokens, seen, out, op);
        }
        if let Some(m) = shape.list_member() {
            go(service, m.shape(), tokens, seen, out, op);
        }
        if let Some(m) = shape.map_value() {
            go(service, m.shape(), tokens, seen, out, op);
        }
    }
    let mut out = std::collections::BTreeMap::new();
    for e in c.services() {
        let svc = c.service(&e.name).expect("the service decodes");
        for op in svc.operations() {
            if op.classify().secret == SecretBearing::No {
                continue;
            }
            let Some(output) = op.output() else { continue };
            let tokens: Vec<&str> = op
                .paginator()
                .map(|p| p.output_tokens())
                .unwrap_or_default()
                .into_iter()
                .map(|t| t.rsplit('.').next().unwrap_or(t))
                .collect();
            go(
                &e.name,
                output,
                &tokens,
                &mut HashSet::new(),
                &mut out,
                op.name(),
            );
        }
    }
    out
}

/// **Completeness for [`MARKED`]** (theseus-3i6d1), in the shape of
/// `crates/complete.rs`.
///
/// `no_marked_row_can_be_deleted_in_silence` and the staleness checks in
/// `every_credential_shaped_output_is_secret_bearing_or_allowed` guard one
/// direction: a row that stops belonging fails. Nothing failed in the other
/// direction — when something **belongs** on the list and is not on it — and
/// that asymmetry is the whole of the complaint: a weekly catalog update can
/// add a marked, bland-named credential beside a held one and no test has a
/// question to ask about it.
///
/// So the catalog is walked, not the list. Every marked member of a
/// secret-bearing answer whose name (and whose shape's name) carries no
/// credential word, and that no other branch of the rule reaches, is a
/// candidate: it must be on [`MARKED`], because it is a credential, or on
/// [`TRIAGED`], because someone read it and it is not. A candidate on
/// neither fails, naming the member, the operation, and both lists.
///
/// Why a secret-bearing answer and not every answer: AWS marks `sensitive`
/// for logging, so the whole marked set is 2,654 members — postal codes,
/// given names, a tag's value. The members that matter are the ones already
/// travelling beside a credential, where a miss is a *partial hold*: an
/// answer that reads as covered and is not.
#[test]
fn every_marked_candidate_is_on_a_list() {
    let c = Catalog::embedded().expect("the embedded catalog decodes");
    let found = marked_candidates(c);
    let listed: Vec<(String, String)> = MARKED
        .iter()
        .map(|(s, p, m)| (format!("{s} {p} {m}"), "MARKED: a credential".to_string()))
        .chain(
            TRIAGED
                .iter()
                .map(|(s, p, m, why)| (format!("{s} {p} {m}"), format!("TRIAGED: {why}"))),
        )
        .collect();
    let spec = crate::complete::MustBeComplete {
        item: "a marked, wordless member of a secret-bearing answer",
        list: "aws::tests_secret_shapes::MARKED and TRIAGED",
        world: "the walk of the embedded catalog's secret-bearing operations",
        add: "read the member. If it is a credential, add it to MARKED, which \
              makes it credential-shaped and requires its operation to be \
              secret-bearing. If it is not, add it to TRIAGED with what it \
              actually is. Leaving it on neither means the next person has no \
              way to tell which was decided",
        drop: "the member is gone from the catalog, or another branch of the \
               rule now reaches it: delete the row",
        // 83 now: 19 on MARKED and 64 on TRIAGED (76 and 57 before the
        // grafana workspace rows, theseus-3i6d1).
        least: 60,
        stale_fails: true,
    };
    let walked = crate::complete::check(&spec, listed, found);
    assert!(
        walked >= MARKED.len(),
        "the walk found {walked} candidates and MARKED alone has {}: \
         the walk cannot be reaching the rows it is meant to judge",
        MARKED.len()
    );
}
