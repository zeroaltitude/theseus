//! `[aws]` and `[policy.aws]` (AWS design §3.5, §3.9; rows 29 and 30, C1 and
//! C2): the accounts Theseus owns, and the checks of their tables as the
//! config loads, with no lookup in the catalog.

use std::collections::BTreeMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// `[aws]` (AWS design §3.5; row 29, C1 = 14a): the accounts Theseus owns,
/// each by its id. Empty: no AWS tool is registered, and nothing AWS runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwsConfig {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub accounts: BTreeMap<String, AwsAccountConfig>,
}

impl AwsConfig {
    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }
}

/// One account: its key (the root of trust), its default region, and the
/// regions a call may name. Its key is checked after serving: STS must name
/// this account for it, or no call of the account signs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwsAccountConfig {
    /// The key: the `[secrets]` entries holding its access key id and its
    /// secret access key.
    #[serde(default)]
    pub credentials: AwsCredentialNames,
    /// The region a call goes to unless it names one.
    pub region: String,
    /// The regions a call may name. Empty: `region` alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<String>,
    /// A local stand-in for AWS, `http://127.0.0.1:<port>`: tests and
    /// scratch daemons only, so that no request and no signature leaves the
    /// machine. Every call goes there, signed as AWS's own endpoint would be.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// The owner role `theseus aws bootstrap` made (`theseus-owner`, AWS
    /// design §3.5). Named: every call signs in a role session named by its
    /// execution, under the guards, and the key signs only STS. Unnamed: the
    /// key signs, as before the bootstrap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_role: Option<String>,
    /// Every session's source identity, which survives role chaining: which
    /// Theseus this is (`theseus-desktop`). Default `theseus`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment: Option<String>,
    /// The month's budget in USD (§3.7): the bootstrap's, and reconciled into
    /// the foundation stack's budget after serving when they differ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monthly_budget_usd: Option<u32>,
    /// The durability tender (AWS step 15): after serving, ship the store's
    /// WAL segments and blobs to the foundation's bucket and its index rows
    /// to the durability table, under the deployment's prefix, in the
    /// tender session `theseus-durability`. Needs `owner_role`; one account
    /// at most. Off by default.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub durability: bool,
}

impl AwsAccountConfig {
    /// The regions a call may name: `regions`, or `region` alone.
    pub fn allowed_regions(&self) -> Vec<String> {
        if self.regions.is_empty() {
            vec![self.region.clone()]
        } else {
            self.regions.clone()
        }
    }

    /// Every session's source identity.
    pub fn deployment(&self) -> &str {
        self.deployment.as_deref().unwrap_or("theseus")
    }
}

/// An IAM name's characters (a role, a session, a source identity):
/// letters, digits, and `+=,.@_-`.
fn iam_name_ok(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+=,.@_-".contains(&b))
}

/// The two `[secrets]` entries of an AWS key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwsCredentialNames {
    #[serde(default = "default_aws_key_id_secret")]
    pub access_key_id: String,
    #[serde(default = "default_aws_secret_key_secret")]
    pub secret_access_key: String,
}

fn default_aws_key_id_secret() -> String {
    "aws_access_key_id".into()
}
fn default_aws_secret_key_secret() -> String {
    "aws_secret_access_key".into()
}

impl Default for AwsCredentialNames {
    fn default() -> Self {
        Self {
            access_key_id: default_aws_key_id_secret(),
            secret_access_key: default_aws_secret_key_secret(),
        }
    }
}

/// `us-west-2`, `eu-central-1`, `us-gov-west-1`, `cn-north-1`: a region's
/// form, letters and dashes ending in a number.
fn aws_region_ok(r: &str) -> bool {
    let mut parts = r.split('-');
    let first = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    first.len() == 2
        && first.bytes().all(|b| b.is_ascii_lowercase())
        && rest.len() >= 2
        && rest[..rest.len() - 1]
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase()))
        && rest
            .last()
            .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// A `[policy.aws]` key: a class (`read`, `write`, `run`), a service (`ec2`,
/// `s3`), or a service and an operation (`s3:ListBuckets`), as
/// `aws.describe` names them.
fn aws_policy_key(k: &str) -> Result<(), String> {
    let service = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    match k.split_once(':') {
        _ if matches!(k, "read" | "write" | "run") => Ok(()),
        None if service(k) => Ok(()),
        Some((s, op))
            if service(s)
                && op.starts_with(|c: char| c.is_ascii_uppercase())
                && op.bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            Ok(())
        }
        _ => Err(format!(
            "policy.aws.\"{k}\" is not a class (`read`, `write`, `run`), a service (\"ec2\"), \
             or a service and an operation (\"s3:ListBuckets\") as aws.describe names them"
        )),
    }
}

impl super::Config {
    /// `[aws.accounts.<id>]` and `[policy.aws]` (AWS design §3.5, §3.9):
    /// checked as they are read, with no lookup in the catalog, which stays
    /// undecoded until a call (§3.10).
    pub(super) fn validate_aws(&self) -> Result<()> {
        for (id, a) in &self.aws.accounts {
            if id.len() != 12 || !id.bytes().all(|b| b.is_ascii_digit()) {
                anyhow::bail!("aws.accounts.\"{id}\" is not an account's id, which is 12 digits");
            }
            for (key, name) in [
                ("access_key_id", &a.credentials.access_key_id),
                ("secret_access_key", &a.credentials.secret_access_key),
            ] {
                if !self.secrets.contains_key(name) {
                    anyhow::bail!(
                        "aws.accounts.{id}.credentials.{key} = {name:?} has no matching entry \
                         under [secrets]"
                    );
                }
            }
            if let Some(r) = std::iter::once(&a.region)
                .chain(&a.regions)
                .find(|r| !aws_region_ok(r))
            {
                anyhow::bail!("aws.accounts.{id}: {r:?} is not a region's name, as us-west-2 is");
            }
            if !a.allowed_regions().contains(&a.region) {
                anyhow::bail!(
                    "aws.accounts.{id}.region = {:?} is not one of its regions: {}",
                    a.region,
                    a.regions.join(", ")
                );
            }
            if let Some(e) = &a.endpoint {
                if !super::dev_origin_ok(e) {
                    anyhow::bail!(
                        "aws.accounts.{id}.endpoint = {e:?} is not a stand-in on this machine: it \
                         must be http://, then localhost or a loopback address, then a port, and \
                         nothing more"
                    );
                }
            }
            for (key, v, max) in [
                ("owner_role", &a.owner_role, 64),
                ("deployment", &a.deployment, 64),
            ] {
                if let Some(v) = v.as_deref() {
                    if !iam_name_ok(v) || v.len() < 2 || v.len() > max {
                        anyhow::bail!(
                            "aws.accounts.{id}.{key} = {v:?} is not an IAM name: 2 to {max} \
                             letters, digits, and +=,.@_-"
                        );
                    }
                }
            }
            if a.monthly_budget_usd == Some(0) {
                anyhow::bail!(
                    "aws.accounts.{id}.monthly_budget_usd is 0: the budget's stop would hold at \
                     once; name the month's dollars"
                );
            }
        }
        let shipping: Vec<&String> = self
            .aws
            .accounts
            .iter()
            .filter(|(_, a)| a.durability)
            .map(|(id, _)| id)
            .collect();
        if let Some(id) = shipping
            .iter()
            .find(|id| self.aws.accounts[**id].owner_role.is_none())
        {
            anyhow::bail!(
                "aws.accounts.{id}.durability needs owner_role: the tender signs in a role session \
                 narrowed to the foundation's bucket and table, never with the key"
            );
        }
        if shipping.len() > 1 {
            anyhow::bail!(
                "durability is on for {} accounts ({}): the store ships to one",
                shipping.len(),
                shipping
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        for key in self.policy.aws.keys() {
            aws_policy_key(key).map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::Config;

    /// C2's keys (row 30): an account's owner role, its deployment, and its
    /// month's budget; and a program's AWS job session, which names a bound
    /// account. A grant that gives nothing, and a bad name, fail to load.
    #[test]
    fn an_aws_account_names_its_owner_role_and_budget_and_a_program_its_session() {
        let with = |more: &str| {
            Config::parse(&format!(
                "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\
                 aws_access_key_id = \"op://v/k/notesPlain#AWS_ACCESS_KEY_ID\"\n\
                 aws_secret_access_key = \"op://v/k/notesPlain#AWS_SECRET_ACCESS_KEY\"\n\n\
                 [aws.accounts.111122223333]\nregion = \"us-west-2\"\n{more}\n"
            ))
            .map(|(c, _)| c)
        };
        let ok = with(
            "owner_role = \"theseus-owner\"\ndeployment = \"theseus-lab\"\nmonthly_budget_usd = 50\n\n\
             [broker.programs.aws]\naws_account = \"111122223333\"",
        )
        .unwrap();
        let a = &ok.aws.accounts["111122223333"];
        assert_eq!(
            (
                a.owner_role.as_deref(),
                a.deployment(),
                a.monthly_budget_usd
            ),
            (Some("theseus-owner"), "theseus-lab", Some(50))
        );
        assert!(ok.broker.programs["aws"].env.is_empty());
        // The durability tender: off by default, on with the owner role.
        assert!(!a.durability);
        let on = with("owner_role = \"theseus-owner\"\ndurability = true").unwrap();
        assert!(on.aws.accounts["111122223333"].durability);
        assert_eq!(
            with("").unwrap().aws.accounts["111122223333"].deployment(),
            "theseus"
        );
        for (bad, says) in [
            ("owner_role = \"theseus owner\"", "owner_role = \"theseus owner\" is not an IAM name"),
            ("deployment = \"x\"", "is not an IAM name: 2 to 64"),
            ("monthly_budget_usd = 0", "monthly_budget_usd is 0"),
            ("durability = true", "aws.accounts.111122223333.durability needs owner_role"),
            (
                "\n[broker.programs.aws]\naws_account = \"444455556666\"",
                "broker.programs.aws.aws_account = \"444455556666\" is not an account under [aws.accounts]",
            ),
            ("\n[broker.programs.aws]", "broker.programs.aws grants nothing"),
        ] {
            let e = format!("{:#}", with(bad).unwrap_err());
            assert!(e.contains(says), "{bad}: {e}");
        }
    }
}
