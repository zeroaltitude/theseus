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
//! `secret`, `key`, `credential`, `passphrase`, `seed`), or when one of two
//! short lists names it outright: the walk's `HELD`, for a member no model
//! marks (API Gateway's `ApiKey.value`, theseus-u4pe), and [`MARKED`], for a
//! member the model does mark whose name says nothing of what it is (the
//! pre-shared keys inside EC2's `VpnConnectionDeviceSampleConfiguration`).
//! A `HELD` row that no secret-bearing answer reaches is stale.
//!
//! **Why the marking and the words together, and neither alone**
//! (theseus-37ds). The marking alone is not a credential rule: AWS marks
//! `sensitive` to keep a value out of logs, so the marking covers personal
//! data and customer content too, and holding every marked member would
//! make 1369 further operations withhold their results — a postal code, a
//! given name, a control's description, a tag's value. The words alone are a
//! guess about a name, and the names that matter do not cooperate: a stream
//! key arrives as `StreamKey.value`, a Grafana key as `key`, a TOTP seed as
//! `SecretCode`, a channel's key as `Srt.passphrase`. Read together they
//! keep the marking's authority — the service that produced the value said
//! it was sensitive — and then ask the catalog's own type names, rather than
//! a growing list of spellings, what the value is. Reading the **shape's**
//! words as well as the member's is what finds `StreamKey.value`: `value`
//! says nothing, and `StreamKey` says all of it.
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
    // The session that completes an MFA association, and which is spent
    // without the caller's own credentials.
    ("cognito-idp", "VerifySoftwareTokenResponse", "Session"),
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
    NAMED.iter().any(|n| n.eq_ignore_ascii_case(member))
        || (shape.is_sensitive() && (carries(member) || carries(parent)))
        || row(HELD)
        || row(MARKED)
}

/// One credential-shaped member of an output: `Shape.member`, and the path
/// it was first reached by.
struct Hit {
    member: String,
    path: String,
}

fn walk(
    service: &str,
    shape: ShapeRef<'_>,
    path: &str,
    tokens: &[&str],
    seen: &mut HashSet<ShapeId>,
    out: &mut Vec<Hit>,
) {
    if !seen.insert(shape.id()) {
        return;
    }
    for m in shape.members() {
        let p = if path.is_empty() {
            m.name().to_string()
        } else {
            format!("{path}.{}", m.name())
        };
        let skipped = tokens.contains(&m.name())
            || m.name().eq_ignore_ascii_case("NextToken")
            // A tag's key, however the service spells it.
            || (shape.name() == "Tag"
                && ["Key", "tagKey"]
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(m.name())));
        if !skipped && credential_shaped(service, shape.name(), m.name(), m.shape()) {
            out.push(Hit {
                member: format!("{}.{}", shape.name(), m.name()),
                path: p.clone(),
            });
        }
        walk(service, m.shape(), &p, tokens, seen, out);
    }
    if let Some(m) = shape.list_member() {
        walk(service, m.shape(), &format!("{path}[]"), tokens, seen, out);
    }
    if let Some(m) = shape.map_value() {
        walk(
            service,
            m.shape(),
            &format!("{path}{{}}"),
            tokens,
            seen,
            out,
        );
    }
}

/// The operation's credential-shaped output members.
fn hits(op: OperationRef<'_>) -> Vec<Hit> {
    let Some(output) = op.output() else {
        return Vec::new();
    };
    let tokens: Vec<&str> = op
        .paginator()
        .map(|p| p.output_tokens())
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.rsplit('.').next().unwrap_or(t))
        .collect();
    let mut out = Vec::new();
    walk(
        op.service().name(),
        output,
        "",
        &tokens,
        &mut HashSet::new(),
        &mut out,
    );
    out
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
            let kind = op.classify().secret;
            let secret = kind != SecretBearing::No;
            let hits = hits(op);
            // One that holds a secret only when it is there must have a
            // member the walk holds: else it would answer whole, always.
            if kind == SecretBearing::WhenPresent && hits.is_empty() {
                bad.push(format!(
                    "stale: {}:{} is when_present, and its output has no credential-shaped member",
                    e.name,
                    op.name()
                ));
            }
            for hit in hits {
                for (i, (s, p, m)) in HELD.iter().enumerate() {
                    if secret && *s == e.name && hit.member == format!("{p}.{m}") {
                        held_used[i] = true;
                    }
                }
                for (i, (s, p, m)) in MARKED.iter().enumerate() {
                    if secret && *s == e.name && hit.member == format!("{p}.{m}") {
                        marked_used[i] = true;
                    }
                }
                let rows: Vec<usize> = (0..ALLOWED.len())
                    .filter(|&i| matches(&ALLOWED[i], &e.name, op.name(), &hit.member))
                    .collect();
                for &i in &rows {
                    used[i] = true;
                    if secret {
                        bad.push(format!(
                            "stale: {}:{} is secret-bearing, and the allowed row for {} still \
                             names it",
                            e.name,
                            op.name(),
                            ALLOWED[i].member
                        ));
                    }
                }
                if !secret && rows.is_empty() {
                    bad.push(format!(
                        "{}:{} returns {} ({}), credential-shaped, and is not secret-bearing: \
                         a mint (CLASS with MINT, SECRET, RETRY), a stored secret (SECRET), or \
                         an allowed row with its reason",
                        e.name,
                        op.name(),
                        hit.path,
                        hit.member
                    ));
                }
            }
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
