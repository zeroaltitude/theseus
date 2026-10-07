//! The output-shape rule (theseus-ye7o): the next credential mint a weekly
//! catalog update brings fails here, before a call can hand its keys to the
//! model. It walks every operation's output shape in the embedded catalog
//! and finds each credential-shaped member: one whose name, in any case, is
//! one that [`super::secret`]'s walk holds (`NAMED`: `Credentials`,
//! `SecretAccessKey`, `SessionToken`, …), one the model marks sensitive
//! whose name ends in `Token`, `Password`, `Secret`, `Key` or `Credentials`,
//! or one the walk's `HELD` names by its shape in its service (API Gateway's
//! `ApiKey.value`, theseus-u4pe); a `HELD` row that no secret-bearing answer
//! reaches is stale. A paginator's output token is none (some models mark `NextToken`
//! sensitive), and nor is any `NextToken` (many operations page without a
//! paginator) or a tag's key (`Tag.Key`, marked sensitive by a few).
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

/// Endings that make a sensitive member credential-shaped.
const ENDINGS: &[&str] = &["Token", "Password", "Secret", "Key", "Credentials"];

/// A member that names a Secrets Manager secret, and holds no value.
const NAMES_A_SECRET: &str = "names a Secrets Manager secret by its ARN or id; holds no value";
const PUBLIC_KEY: &str = "a public key";
const TEXT: &str = "text a person wrote (a template's, a visual's): content, no credential";

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
/// [`NAMED`], a sensitive name with a credential's ending, or a [`HELD`] row.
fn credential_shaped(service: &str, parent: &str, member: &str, shape: ShapeRef<'_>) -> bool {
    NAMED.iter().any(|n| n.eq_ignore_ascii_case(member))
        || (shape.is_sensitive() && ENDINGS.iter().any(|e| member.ends_with(e)))
        || HELD
            .iter()
            .any(|(s, p, m)| *s == service && *p == parent && *m == member)
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
            || (shape.name() == "Tag" && ["Key", "tagKey"].contains(&m.name()));
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
    bad.dedup();
    assert!(
        bad.is_empty(),
        "{} findings:\n{}",
        bad.len(),
        bad.join("\n")
    );
}
