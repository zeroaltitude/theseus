//! An owner role that exists and cannot be assumed, repaired by the bootstrap
//! (theseus-ps12; the AWS design's §3.5).
//!
//! The owner role is what every call signs in once the config names it
//! (`aws/session.rs`). A role AWS has and will not let the key assume used to
//! end the bootstrap with `sts:AssumeRole into theseus-owner failed: …` after a
//! minute of retries, which reads as a credential fault and is not one: the
//! role's trust policy does not name the key. The bootstrap repairs that and
//! says it did.
//!
//! **The trust policy only.** The owner's decision for this row is the careful
//! one: the trust policy is repaired, and the boundary is never touched without
//! a fresh yes from him. So a role whose trust policy already names the key is
//! left exactly as it is, with the reason said in words — the refusal is then
//! the boundary's, an attached policy's, or IAM's propagation, and none of those
//! is this code's to change.
//!
//! **Only under the operator's live yes.** [`trust_policy`] is reached from
//! `bootstrap::floor`, which only `bootstrap::apply` calls, and an apply runs
//! only on the operator's own `--apply <digest>` or their yes at the question.
//! A plan writes nothing, here as everywhere.

use std::sync::Arc;

use serde_json::{json, Value};

use super::{Account, Failure, Request, Signer};

/// What the repair did, in the operator's words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Repaired {
    /// The key's principal was added to the role's trust policy.
    Trust { role: String, principal: String },
    /// The trust policy already names the key, so the refusal is not its: the
    /// boundary, an attached policy, or IAM's propagation. Nothing was changed.
    NotTheTrustPolicy { role: String },
    /// IAM has no such role, so there is nothing to repair.
    NoRole { role: String },
    /// The repair itself failed; the mint's own error still stands.
    Failed { role: String, why: String },
}

impl Repaired {
    /// Did it change IAM?
    pub fn changed(&self) -> bool {
        matches!(self, Repaired::Trust { .. })
    }

    /// The line the bootstrap's result carries to the operator.
    pub fn line(&self) -> String {
        match self {
            Repaired::Trust { role, principal } => format!(
                "{role} existed and would not let this account's key assume it: its trust policy \
                 did not name {principal}. The bootstrap added that principal to the trust policy \
                 (sts:AssumeRole) and minted the session again. Nothing else was changed: a \
                 permissions boundary is never repaired without your word."
            ),
            Repaired::NotTheTrustPolicy { role } => format!(
                "{role} would not let this account's key assume it, and its trust policy already \
                 names the key, so the refusal is not the trust policy's: look at the role's \
                 permissions boundary and its attached policies (IAM may also still be \
                 propagating a fresh role). Nothing was changed."
            ),
            Repaired::NoRole { role } => format!(
                "IAM has no {role} in this account, so there was nothing to repair: the \
                 foundation stack makes that role, and this bootstrap makes the stack."
            ),
            Repaired::Failed { role, why } => {
                format!("{role}'s trust policy could not be read or written: {why}")
            }
        }
    }
}

/// Does the mint's own error say AWS refused the `sts:AssumeRole`, rather than
/// failing to reach it? Only a refusal is worth a repair: a throttle, a
/// timeout, a network failure or a missing key is nothing a policy edit mends.
///
/// The text is `aws::session::mint`'s (`sts:AssumeRole into {role} failed: …`,
/// carrying AWS's own code and message) or `Account::session`'s unbound words.
pub fn is_assume_refusal(said: &str) -> bool {
    let lower = said.to_ascii_lowercase();
    if !lower.contains("assumerole") {
        return false;
    }
    // AWS answers a trust policy that does not name the caller, and a role it
    // does not have, with the same `AccessDenied`; which it is, only IAM says.
    ["accessdenied", "not authorized to perform"]
        .iter()
        .any(|w| lower.contains(w))
}

/// Percent-decoding, for IAM's `AssumeRolePolicyDocument`, which comes back
/// URL-encoded. Only `%XX` and only valid pairs; anything else is itself.
fn url_decoded(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let pair = (i + 2 < b.len() && b[i] == b'%')
            .then(|| {
                let hex = |c: u8| (c as char).to_digit(16);
                Some(hex(b[i + 1])? * 16 + hex(b[i + 2])?)
            })
            .flatten();
        match pair {
            Some(v) => {
                out.push(v as u8);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Does `doc` already let `principal` assume the role? An `Allow` of
/// `sts:AssumeRole` (or a wildcard over it) whose `Principal.AWS` names the
/// principal itself, or the account root it belongs to, which covers it.
pub fn allows(doc: &Value, principal: &str) -> bool {
    let root = principal
        .split_once(":user/")
        .or_else(|| principal.split_once(":role/"))
        .map(|(head, _)| format!("{head}:root"));
    let one_of = |v: &Value| -> Vec<String> {
        match v {
            Value::String(s) => vec![s.clone()],
            Value::Array(a) => a.iter().filter_map(|x| x.as_str()).map(String::from).collect(),
            _ => Vec::new(),
        }
    };
    let names = |v: &Value| {
        one_of(v).iter().any(|p| {
            p == principal || root.as_deref() == Some(p.as_str()) || p == "*"
        })
    };
    let assumes = |v: &Value| {
        one_of(v)
            .iter()
            .any(|a| a == "sts:AssumeRole" || a == "sts:*" || a == "*")
    };
    statements(doc).iter().any(|s| {
        s["Effect"] == "Allow" && assumes(&s["Action"]) && names(&s["Principal"]["AWS"])
    })
}

fn statements(doc: &Value) -> Vec<Value> {
    match &doc["Statement"] {
        Value::Array(a) => a.clone(),
        one @ Value::Object(_) => vec![one.clone()],
        _ => Vec::new(),
    }
}

/// `doc` with `principal` allowed to assume the role, or `None` when it
/// already is. Every statement the role had is kept, in its order: a trust
/// policy is the operator's, and this adds to it.
pub fn with_principal(doc: &Value, principal: &str) -> Option<Value> {
    if allows(doc, principal) {
        return None;
    }
    let mut statement = statements(doc);
    statement.push(json!({
        "Sid": "TheseusOwnerKey",
        "Effect": "Allow",
        "Principal": {"AWS": principal},
        "Action": "sts:AssumeRole"
    }));
    let version = doc["Version"].as_str().unwrap_or("2012-10-17").to_string();
    Some(json!({"Version": version, "Statement": statement}))
}

/// Read `role`'s trust policy with the key, and add the key's principal to it
/// when it is missing. Signed by the key (`Signer::Key`): the role cannot be
/// assumed, which is the whole point, so no session of it can sign this.
pub async fn trust_policy(account: &Arc<Account>, role: &str) -> Repaired {
    let failed = |why: String| Repaired::Failed {
        role: role.to_string(),
        why,
    };
    let Some((principal, _)) = account.identity() else {
        return failed("the account's key is not bound, so nothing was sent".into());
    };
    let region = account.cfg.region.clone();
    let input = json!({"RoleName": role});
    let req = Request {
        service: "iam",
        operation: "GetRole",
        input: &input,
        region: &region,
        pages: 1,
        class: "read",
        signer: Signer::Key,
    };
    let out = match account.request(None, &req).await {
        Ok(out) => out,
        Err(f) => {
            let said = super::bootstrap::said(&f);
            return if no_such_role(&f) {
                Repaired::NoRole {
                    role: role.to_string(),
                }
            } else {
                failed(said)
            };
        }
    };
    let encoded = out.body["Role"]["AssumeRolePolicyDocument"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let doc: Value = match serde_json::from_str(&url_decoded(&encoded)) {
        Ok(doc) => doc,
        Err(e) => return failed(format!("its trust policy does not parse as JSON: {e}")),
    };
    let Some(repaired) = with_principal(&doc, &principal) else {
        return Repaired::NotTheTrustPolicy {
            role: role.to_string(),
        };
    };
    let input = json!({
        "RoleName": role,
        "PolicyDocument": serde_json::to_string(&repaired)
            .expect("a trust policy document serializes"),
    });
    let req = Request {
        service: "iam",
        operation: "UpdateAssumeRolePolicy",
        input: &input,
        region: &region,
        pages: 1,
        class: "write",
        signer: Signer::Key,
    };
    match account.request(None, &req).await {
        Ok(_) => Repaired::Trust {
            role: role.to_string(),
            principal,
        },
        Err(f) => failed(super::bootstrap::said(&f)),
    }
}

/// IAM says it has no such role: `NoSuchEntity`.
fn no_such_role(f: &Failure) -> bool {
    matches!(f, Failure::Call(theseus_aws::CallError::Aws(e)) if e.code == "NoSuchEntity")
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "arn:aws:iam::111122223333:user/example";

    fn doc(statements: Value) -> Value {
        json!({"Version": "2012-10-17", "Statement": statements})
    }

    #[test]
    fn a_refusal_of_the_assume_is_told_from_a_failure_to_reach_it() {
        assert!(is_assume_refusal(
            "sts:AssumeRole into theseus-owner failed: AccessDenied: User: \
             arn:aws:iam::111122223333:user/example is not authorized to perform: sts:AssumeRole"
        ));
        // Nothing a policy edit mends.
        for other in [
            "sts:AssumeRole into theseus-owner failed: ThrottlingException: Rate exceeded",
            "sts:AssumeRole into theseus-owner failed: the request timed out",
            "sts:AssumeRole into theseus-owner answered with no credentials",
            "AWS account 111122223333 is not bound: its key is missing",
            "AccessDenied: not authorized to perform: iam:GetRole",
        ] {
            assert!(!is_assume_refusal(other), "{other}");
        }
    }

    #[test]
    fn a_trust_policy_that_names_the_key_is_left_alone() {
        let named = doc(json!([{
            "Effect": "Allow",
            "Principal": {"AWS": KEY},
            "Action": "sts:AssumeRole"
        }]));
        assert!(allows(&named, KEY));
        assert_eq!(with_principal(&named, KEY), None);
        // The account root covers its own users, and so does a wildcard.
        let root = doc(json!([{
            "Effect": "Allow",
            "Principal": {"AWS": ["arn:aws:iam::111122223333:root"]},
            "Action": ["sts:AssumeRole", "sts:TagSession"]
        }]));
        assert_eq!(with_principal(&root, KEY), None);
        // Another account's root does not.
        let elsewhere = doc(json!([{
            "Effect": "Allow",
            "Principal": {"AWS": "arn:aws:iam::444455556666:root"},
            "Action": "sts:AssumeRole"
        }]));
        assert!(with_principal(&elsewhere, KEY).is_some());
    }

    #[test]
    fn the_repaired_policy_keeps_every_statement_and_adds_the_key() {
        let before = doc(json!([{
            "Effect": "Allow",
            "Principal": {"Service": "cloudformation.amazonaws.com"},
            "Action": "sts:AssumeRole"
        }]));
        let service = before["Statement"][0].clone();
        let after = with_principal(&before, KEY).expect("a repair");
        assert_eq!(after["Version"], "2012-10-17");
        let st = after["Statement"].as_array().expect("statements").clone();
        assert_eq!(st.len(), 2, "{after}");
        assert_eq!(st[0], service, "the statement the role had is kept, first");
        assert_eq!(st[1]["Principal"]["AWS"], KEY);
        assert_eq!(st[1]["Action"], "sts:AssumeRole");
        assert_eq!(st[1]["Effect"], "Allow");
        // And the repaired document allows what it was repaired for, so a
        // second repair of the same role changes nothing.
        assert!(allows(&after, KEY));
        assert_eq!(with_principal(&after, KEY), None);
    }

    #[test]
    fn a_deny_of_the_assume_is_no_allow_and_an_allow_of_another_action_is_not_one_either() {
        let denied = doc(json!([{
            "Effect": "Deny",
            "Principal": {"AWS": KEY},
            "Action": "sts:AssumeRole"
        }]));
        assert!(!allows(&denied, KEY));
        let other = doc(json!([{
            "Effect": "Allow",
            "Principal": {"AWS": KEY},
            "Action": "sts:TagSession"
        }]));
        assert!(!allows(&other, KEY));
    }

    #[test]
    fn iams_url_encoded_document_decodes() {
        let encoded = "%7B%22Version%22%3A%222012-10-17%22%2C%22Statement%22%3A%5B%5D%7D";
        assert_eq!(
            url_decoded(encoded),
            r#"{"Version":"2012-10-17","Statement":[]}"#
        );
        // A stray percent, and a short tail, are themselves.
        assert_eq!(url_decoded("100% sure %2"), "100% sure %2");
    }

    #[test]
    fn a_trust_policy_with_one_statement_object_reads_as_one() {
        let one = doc(json!({
            "Effect": "Allow",
            "Principal": {"AWS": KEY},
            "Action": "sts:AssumeRole"
        }));
        assert!(allows(&one, KEY));
        let other = doc(json!({
            "Effect": "Allow",
            "Principal": {"Service": "ecs-tasks.amazonaws.com"},
            "Action": "sts:AssumeRole"
        }));
        let after = with_principal(&other, KEY).expect("a repair");
        assert_eq!(after["Statement"].as_array().expect("statements").len(), 2);
    }

    #[test]
    fn every_outcome_says_what_it_did_and_whether_it_changed_iam() {
        let role = "theseus-owner".to_string();
        let changed = Repaired::Trust {
            role: role.clone(),
            principal: KEY.to_string(),
        };
        assert!(changed.changed());
        assert!(changed.line().contains(KEY));
        for left in [
            Repaired::NotTheTrustPolicy { role: role.clone() },
            Repaired::NoRole { role: role.clone() },
            Repaired::Failed {
                role,
                why: "throttled".into(),
            },
        ] {
            assert!(!left.changed(), "{left:?}");
            assert!(left.line().contains("theseus-owner"), "{left:?}");
        }
    }
}
