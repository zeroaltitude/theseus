//! Role sessions (AWS design §3.5; C2 = 14b): once the config names the
//! owner role the bootstrap made, the key signs only `sts:AssumeRole` into
//! it and `sts:GetCallerIdentity`, and every other call signs in a session
//! named for what made it.
//!
//! | session | its name | its session policies | lifetime |
//! |---|---|---|---|
//! | work | the execution id | the guards, then `theseus-allow-all` | 12 h |
//! | job | the job's correlation id | the guards and the stack path's, then allow-all | the job's deadline |
//! | floor | `<execution>.floor` | `theseus-allow-all` alone | 15 min, one approved call |
//! | tender | `theseus-<tender>` | the tender's own inline policy | 12 h |
//!
//! A session's allows come from exactly one allow policy (allow-all, or a
//! tender's own), and the guards only deny, so they hold however AWS combines
//! the session policies ([`policy_arns`] is what the composition test reads).
//! Each session's source identity is the deployment, and its tags name the
//! execution, the kind, and the deployment. Sessions are minted on first use,
//! cached in zeroizing memory, refreshed five minutes before they expire, and
//! never written.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use theseus_aws::{Attribution, Call, Credentials};

/// A session's kind, and what it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Every `aws.*` call of an execution.
    Work,
    /// A job's: the program a `[broker.programs]` grant gives AWS, at launch.
    Job,
    /// The one call the operator approved at the floor: no guard.
    Floor,
    /// A tender of the core's own, with its own inline policy: the budget's
    /// reconcile and reads.
    Tender(&'static str),
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Work => "work",
            Kind::Job => "job",
            Kind::Floor => "floor",
            Kind::Tender(_) => "tender",
        }
    }
}

/// The managed policies a session of `kind` carries, by name, in order: the
/// guards the list generates (each part), the stack path's for a job, then
/// `theseus-allow-all`; a floor session carries allow-all alone, and a
/// tender none (its inline policy is its allow).
pub fn policy_names(kind: Kind) -> Vec<String> {
    let l = theseus_aws_guard::embedded();
    let guards = || {
        l.guard_limits()
            .into_iter()
            .chain(l.guard_iac())
            .map(|p| p.name)
    };
    let allow = || std::iter::once(super::ALLOW_ALL.to_string());
    match kind {
        Kind::Work => guards().chain(allow()).collect(),
        Kind::Job => guards()
            .chain(l.guard_stacks().into_iter().map(|p| p.name))
            .chain(allow())
            .collect(),
        Kind::Floor => allow().collect(),
        Kind::Tender(_) => Vec::new(),
    }
}

/// [`policy_names`] as the ARNs `AssumeRole` takes.
pub fn policy_arns(account: &str, kind: Kind) -> Vec<String> {
    policy_names(kind)
        .into_iter()
        .map(|n| format!("arn:aws:iam::{account}:policy/{n}"))
        .collect()
}

/// The longest a role session lasts: `theseus-owner`'s `MaxSessionDuration`.
pub const LONGEST: Duration = Duration::from_secs(12 * 3600);
/// The shortest STS gives, and a floor session's whole life.
pub const SHORTEST: Duration = Duration::from_secs(900);
/// A cached session is minted again this long before it expires.
const REFRESH_BEFORE: Duration = Duration::from_secs(300);

/// An IAM session name: letters, digits, and `+=,.@_-`, 2 to 64 of them.
pub fn session_name(s: &str) -> String {
    let mut n: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "+=,.@_-".contains(c) {
                c
            } else {
                '-'
            }
        })
        .take(64)
        .collect();
    while n.len() < 2 {
        n.push('-');
    }
    n
}

/// One session to mint: its kind, its name, how long it lasts, and a
/// tender's inline policy.
pub struct Want<'a> {
    pub kind: Kind,
    pub name: String,
    pub execution: Option<&'a str>,
    pub lasts: Duration,
    pub inline: Option<&'a Value>,
}

impl Want<'_> {
    /// The cache's key: the kind, the name, and the inline policy's digest.
    fn key(&self) -> String {
        let inline = self
            .inline
            .map(|p| digest(&p.to_string()))
            .unwrap_or_default();
        format!("{}|{}|{inline}", self.kind.as_str(), self.name)
    }
}

/// A short digest of a policy's text, for the cache and the ledger.
pub fn digest(text: &str) -> String {
    hex::encode(&Sha256::digest(text.as_bytes())[..8])
}

struct Cached {
    creds: Credentials,
    until: Instant,
}

/// The account's sessions, minted on first use and cached.
#[derive(Default)]
pub struct Sessions {
    cache: Mutex<HashMap<String, Cached>>,
}

/// What a mint returns: the credentials, and its `aws.session.minted` row.
pub struct Minted {
    pub creds: Credentials,
    /// The row, when this call minted it; none from the cache.
    pub row: Option<Value>,
}

impl Sessions {
    /// A session, from the cache or minted with `key` (the root of trust)
    /// through `client`. Never the key itself.
    pub async fn get(
        &self,
        client: &theseus_aws::Client,
        key: &Credentials,
        account: &str,
        role: &str,
        deployment: &str,
        want: &Want<'_>,
    ) -> Result<Minted, String> {
        let k = want.key();
        // A floor session is the one approved call's alone: never cached.
        let floor = want.kind == Kind::Floor;
        if let Some(c) = self.cache.lock().unwrap().get(&k).filter(|_| !floor) {
            if Instant::now() + REFRESH_BEFORE < c.until {
                return Ok(Minted {
                    creds: c.creds.clone(),
                    row: None,
                });
            }
        }
        let (creds, row) = mint(client, key, account, role, deployment, want).await?;
        if floor {
            return Ok(Minted {
                creds,
                row: Some(row),
            });
        }
        let lasts = want.lasts.clamp(SHORTEST, LONGEST);
        let until = Instant::now() + lasts.saturating_sub(Duration::from_secs(60));
        let mut cache = self.cache.lock().unwrap();
        // An ended execution's session goes once it has expired.
        cache.retain(|_, c| c.until > Instant::now());
        cache.insert(
            k,
            Cached {
                creds: creds.clone(),
                until,
            },
        );
        Ok(Minted {
            creds,
            row: Some(row),
        })
    }
}

/// `sts:AssumeRole` into the owner role, signed with the key: the session's
/// name, its source identity, its tags, and its policies.
async fn mint(
    client: &theseus_aws::Client,
    key: &Credentials,
    account: &str,
    role: &str,
    deployment: &str,
    want: &Want<'_>,
) -> Result<(Credentials, Value), String> {
    let arns = policy_arns(account, want.kind);
    let lasts = want.lasts.clamp(SHORTEST, LONGEST);
    let mut input = json!({
        "RoleArn": format!("arn:aws:iam::{account}:role/{role}"),
        "RoleSessionName": want.name,
        "SourceIdentity": deployment,
        "DurationSeconds": lasts.as_secs(),
        "Tags": [
            {"Key": "theseus:execution", "Value": want.execution.unwrap_or("none")},
            {"Key": "theseus:session", "Value": want.kind.as_str()},
            {"Key": "theseus:deployment", "Value": deployment},
        ],
    });
    if !arns.is_empty() {
        input["PolicyArns"] = arns.iter().map(|a| json!({"arn": a})).collect();
    }
    if let Some(p) = want.inline {
        input["Policy"] = json!(p.to_string());
    }
    let attribution = Attribution {
        execution: want.execution.map(String::from),
        call: Some(format!("session-{}", want.kind.as_str())),
    };
    let call = Call {
        service: "sts",
        operation: "AssumeRole",
        input: &input,
        region: None,
        pages: 1,
        attribution: &attribution,
    };
    let out = client
        .call(&call, key)
        .await
        .map_err(|e| format!("sts:AssumeRole into {role} failed: {e}"))?;
    let c = &out.body["Credentials"];
    let text = |k: &str| c[k].as_str().map(String::from);
    let (Some(id), Some(secret), Some(token)) = (
        text("AccessKeyId"),
        text("SecretAccessKey"),
        text("SessionToken"),
    ) else {
        return Err(format!(
            "sts:AssumeRole into {role} answered with no credentials"
        ));
    };
    let creds = Credentials::new(id, secret, Some(token), Some(SystemTime::now() + lasts));
    let row = json!({
        "account": account,
        "kind": want.kind.as_str(),
        "name": want.name,
        "role": role,
        "source_identity": deployment,
        "policies": policy_names(want.kind),
        "inline_digest": want.inline.map(|p| digest(&p.to_string())),
        "seconds": lasts.as_secs(),
        "execution_id": want.execution,
        "request_id": out.request_id,
    });
    Ok((creds, row))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_name_is_an_iam_name() {
        assert_eq!(session_name("exe_0123/x y"), "exe_0123-x-y");
        assert_eq!(session_name("a"), "a-");
        assert_eq!(session_name(&"x".repeat(80)).len(), 64);
    }
}
