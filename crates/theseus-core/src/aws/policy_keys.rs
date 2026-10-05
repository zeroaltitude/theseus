//! `[policy.aws]` keys against the catalog (theseus-snhr). The loader checks a
//! key's form alone, since the catalog stays undecoded on the start path
//! (§3.10); a key naming no service or operation loads and never matches, so
//! its call falls to the class's line or `enforcement`, looser than the
//! operator wrote. This runs after serving, beside the `aws.check` phase, and
//! names each such key: in health, and in the log, one warning each. It never
//! fails the start.
//!
//! A call's service and operation are the catalog's own names
//! (`checked.service`, `checked.operation`), so a key matches only by them: an
//! alias that `aws.describe` accepts (`states`, `monitoring`) is not one, and
//! is named with the name that would match.

use theseus_aws::catalog::Catalog;

/// Why `key` can never match, when it can't: `None` for a class, a key of a
/// form the loader refuses, and a service or operation the catalog has.
fn why(catalog: &Catalog, key: &str) -> Option<String> {
    if matches!(key, "read" | "write" | "run") {
        return None;
    }
    let (service, op) = match key.split_once(':') {
        Some((s, op)) => (s, Some(op)),
        None => (key, None),
    };
    let Some(entry) = catalog.entry(service) else {
        return Some(format!("no AWS service is named {service:?}"));
    };
    if entry.name != service {
        return Some(format!(
            "{service:?} is an alias; calls match by the service's name, {:?}",
            entry.name
        ));
    }
    let op = op?;
    let svc = match catalog.service(service) {
        Ok(s) => s,
        Err(e) => return Some(format!("{service}'s operations did not decode: {e}")),
    };
    match svc.operation(op) {
        Some(o) if o.name() == op => None,
        Some(o) => Some(format!(
            "calls match by the operation's name, {:?}",
            o.name()
        )),
        None => Some(format!("{service} has no operation {op:?}")),
    }
}

/// Each key that can never match, with the reason, in the keys' order.
pub fn unknown<'a>(
    catalog: &Catalog,
    keys: impl IntoIterator<Item = &'a str>,
) -> Vec<(String, String)> {
    keys.into_iter()
        .filter_map(|k| why(catalog, k).map(|w| (k.to_string(), w)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(keys: &[&str]) -> Vec<String> {
        let c = Catalog::embedded().unwrap();
        unknown(c, keys.iter().copied())
            .into_iter()
            .map(|(k, _)| k)
            .collect()
    }

    #[test]
    fn a_typo_d_operation_and_a_misspelt_service_are_named() {
        assert_eq!(
            found(&["ec2:TerminateInstance", "cloudformaton", "s3:ListBuckets"]),
            ["ec2:TerminateInstance", "cloudformaton"]
        );
    }

    #[test]
    fn a_right_service_and_operation_are_not() {
        assert!(found(&[
            "read",
            "write",
            "run",
            "ec2",
            "cloudformation",
            "s3",
            "s3:ListBuckets",
            "ec2:TerminateInstances",
            "cloudformation:DeleteStack",
        ])
        .is_empty());
    }

    #[test]
    fn an_unknown_service_with_an_operation_is_named_whole() {
        assert_eq!(found(&["ec3:RunInstances"]), ["ec3:RunInstances"]);
    }

    #[test]
    fn an_alias_and_a_loose_operation_never_match_so_they_are_named() {
        let c = Catalog::embedded().unwrap();
        let named = unknown(c, ["states", "s3:listbuckets"]);
        assert_eq!(named.len(), 2, "{named:?}");
        assert!(named[0].1.contains("stepfunctions"), "{named:?}");
        assert!(named[1].1.contains("ListBuckets"), "{named:?}");
    }

    /// The check keeps what it found for health's status, and says nothing
    /// (an empty list, absent on the wire) when every key is right.
    #[tokio::test]
    async fn the_status_lists_the_unknown_keys_and_is_absent_when_none() {
        let aws = crate::aws::Aws::from_config(
            &crate::aws::tests::account("http://127.0.0.1:9"),
            crate::aws::tests::board(),
        )
        .unwrap();
        let none = aws
            .check_policy_keys(vec!["s3:ListBuckets".into(), "write".into()])
            .await;
        assert!(none.is_empty());
        let json = serde_json::to_value(aws.status()).unwrap();
        assert!(json.get("unknown_policy_keys").is_none(), "{json}");
        let bad = aws
            .check_policy_keys(vec!["ec2:TerminateInstance".into(), "cloudformaton".into()])
            .await;
        assert_eq!(bad, ["ec2:TerminateInstance", "cloudformaton"]);
        let json = serde_json::to_value(aws.status()).unwrap();
        assert_eq!(
            json["unknown_policy_keys"],
            serde_json::json!(["ec2:TerminateInstance", "cloudformaton"])
        );
    }
}
