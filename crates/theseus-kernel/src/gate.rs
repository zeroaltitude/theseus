//! The kernel's half of the gate (§3.17): the proposal a confirmation binds,
//! and the digest that binds it. The core decides first (the toollet's plan,
//! then the tool policy); the kernel plans the action under the proposal's
//! digest, a confirm binds that digest, and `authorize` refuses a proposal
//! whose digest changed after it was planned or confirmed.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// A proposed tool call as the model (or a test) states it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub tool: String,
    pub args: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    /// Policy context the confirm is bound to (binding revision, role, channel).
    #[serde(default)]
    pub policy_context: Value,
}

/// sha256 over the proposal's JSON, keys sorted.
pub fn digest_proposal(p: &Proposal) -> String {
    digest_json(&serde_json::to_value(p).unwrap_or(Value::Null))
}

/// sha256 (hex) over a JSON value's compact form. `serde_json::Map` is a
/// `BTreeMap` while serde_json's `preserve_order` feature stays off, so object
/// keys come out sorted and equal values digest equally, whatever order their
/// keys were written in. These are the bytes every stored digest was taken
/// over: an action's `args_digest`, a request digest, a manifest's tools digest.
pub fn digest_json(v: &Value) -> String {
    hex::encode(Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn digest_is_key_order_independent_and_argument_sensitive() {
        let a = Proposal {
            tool: "fs.write".into(),
            args: json!({"path": "/x", "bytes": 3}),
            resource: None,
            policy_context: json!({}),
        };
        let b = Proposal {
            tool: "fs.write".into(),
            args: json!({"bytes": 3, "path": "/x"}),
            resource: None,
            policy_context: json!({}),
        };
        assert_eq!(digest_proposal(&a), digest_proposal(&b));
        let mut c = a.clone();
        c.args["bytes"] = json!(4);
        assert_ne!(digest_proposal(&a), digest_proposal(&c));
    }

    /// `digest_json` replaced two hand-written sorted-key serializers (the
    /// kernel's and the provider's). Its bytes must equal theirs, or every
    /// stored `args_digest` stops matching and a confirm parked before the
    /// upgrade can never be authorized.
    #[test]
    fn digest_json_hashes_the_bytes_the_old_canonical_form_hashed() {
        // The serializer it replaced, verbatim, as the reference.
        fn canonical(v: &Value) -> String {
            match v {
                Value::Object(m) => {
                    let mut keys: Vec<_> = m.keys().collect();
                    keys.sort();
                    let parts: Vec<String> = keys
                        .into_iter()
                        .map(|k| {
                            format!("{}:{}", serde_json::to_string(k).unwrap(), canonical(&m[k]))
                        })
                        .collect();
                    format!("{{{}}}", parts.join(","))
                }
                Value::Array(a) => format!(
                    "[{}]",
                    a.iter().map(canonical).collect::<Vec<_>>().join(",")
                ),
                other => serde_json::to_string(other).unwrap_or_default(),
            }
        }
        let mut inserted_backwards = serde_json::Map::new();
        for k in ["z", "y", "b", "a"] {
            inserted_backwards.insert(k.into(), json!(k));
        }
        let corpus = [
            Value::Object(inserted_backwards),
            json!({"b": 1, "a": {"d": [3, {"z": null, "y": true}], "c": "x"}}),
            json!({"a": 1, "B": 2, "_": 3, "é": 4, "10": 5, "9": 6, "": 7, "aa": 8, "a\u{0}": 9, "Z": 10}),
            json!([
                "quote \" backslash \\ newline \n tab \t nul \u{0} bell \u{7} del \u{7f}",
                "line sep \u{2028} para \u{2029} crab 🦀 e-acute é",
                "",
                "\u{fffd}"
            ]),
            json!([
                0,
                -0.0,
                1.5,
                1e300,
                -1e-300,
                0.1,
                1.0,
                u64::MAX,
                i64::MIN,
                i64::MAX
            ]),
            json!({"empty_obj": {}, "empty_arr": [], "nested": [[[{}]], {"": []}]}),
            json!(null),
            json!(true),
            json!("just a string"),
        ];
        for v in &corpus {
            assert_eq!(serde_json::to_string(v).unwrap(), canonical(v), "{v}");
            assert_eq!(
                digest_json(v),
                hex::encode(Sha256::digest(canonical(v).as_bytes())),
                "{v}"
            );
        }
        // Taken from the code this replaced (at 8a1e41d), before the change.
        let p1 = Proposal {
            tool: "fs.write".into(),
            args: json!({"path": "/x", "bytes": 3}),
            resource: None,
            policy_context: json!({}),
        };
        let p2 = Proposal {
            tool: "proc.run".into(),
            args: json!({"argv": ["cargo", "test", "--", "é ✓ \"q\" \\ \n"], "timeout_secs": 30, "env": {"Z": "1", "A": "2", "_": null}}),
            resource: Some("/home/x/projects/y".into()),
            policy_context: json!({"roots": ["/home/x/projects"], "cwd": "/home/x/projects/y"}),
        };
        assert_eq!(
            digest_proposal(&p1),
            "3f1e1c601d9bdd05afa84031bdee5acca135b7c84c7506763f76da748ace2501"
        );
        assert_eq!(
            digest_proposal(&p2),
            "076048f25f9873bc0c573b68173c17195ec130f57a2e581db788dfc982b4e01e"
        );
    }

    /// Every digest above depends on sorted keys. serde_json's `preserve_order`
    /// feature, enabled by any crate in the graph, would turn `Map` into an
    /// insertion-ordered map and silently change them all; this fails first.
    #[test]
    fn serde_json_maps_keep_their_keys_sorted() {
        let mut m = serde_json::Map::new();
        m.insert("b".into(), json!(1));
        m.insert("a".into(), json!(2));
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            r#"{"a":2,"b":1}"#,
            "serde_json's preserve_order feature is on: stored digests would change"
        );
    }
}
