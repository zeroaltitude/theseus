//! The deterministic policy gate (§3.17 ordering):
//!
//! `proposed → transform → schema validation → live policy → confirmation
//! bound to the final action → final revalidation → durable dispatch`.
//!
//! M2 tests the *ordering*, not the policies: `Policy` is a trait with a
//! permissive default and a scripted implementation for the simulator. What
//! the gate guarantees is structural: a transform can never run after the
//! confirm was bound, a policy deny is never overridden by anything later, and
//! the digest the confirm binds is the digest that dispatches.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::types::Authority;

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

/// sha256 over the canonical (sorted-key) JSON of the proposal.
pub fn digest_proposal(p: &Proposal) -> String {
    let canon = canonical(&serde_json::to_value(p).unwrap_or(Value::Null));
    let mut h = Sha256::new();
    h.update(canon.as_bytes());
    hex::encode(h.finalize())
}

fn canonical(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<_> = m.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", serde_json::to_string(k).unwrap(), canonical(&m[k])))
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum PolicyDecision {
    Allow,
    Deny {
        reason: String,
    },
    /// Allowed only with a confirmation from this principal.
    Confirm {
        by: String,
    },
}

pub trait Policy: Send + Sync {
    /// Hooks may tighten or rewrite the proposal before anything else looks at it.
    fn transform(&self, _p: &mut Proposal, _auth: &Authority) {}
    /// Schema and argument validation. Deterministic; no I/O.
    fn validate(&self, _p: &Proposal) -> Result<(), String> {
        Ok(())
    }
    /// Live policy and resource checks against the execution's authority.
    fn decide(&self, _p: &Proposal, _auth: &Authority) -> PolicyDecision {
        PolicyDecision::Allow
    }
}

/// Allow everything, transform nothing.
pub struct AllowAll;
impl Policy for AllowAll {}

/// Steps the gate took, for the ledger and the trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateTrace {
    pub transformed: bool,
    pub digest_before: String,
    pub digest_after: String,
    pub validated: bool,
    pub decision: PolicyDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "gate")]
pub enum GateResult {
    /// Proceed to `authorize` with no confirm required.
    Allow,
    /// Proceed to `authorize` only after `bind_confirm` from `by`.
    NeedsConfirm {
        by: String,
    },
    Deny {
        reason: String,
    },
}

/// Run the ordered gate over a proposal. The proposal is mutated in place by
/// `transform`; the returned digest is the one that must be planned, bound,
/// and authorized. Nothing after this function may change `proposal` without
/// rerunning it (the kernel enforces that by digest).
pub fn run_gate(
    policy: &dyn Policy,
    proposal: &mut Proposal,
    auth: &Authority,
) -> (GateResult, GateTrace) {
    let before = digest_proposal(proposal);
    policy.transform(proposal, auth);
    let after = digest_proposal(proposal);
    let mut trace = GateTrace {
        transformed: before != after,
        digest_before: before,
        digest_after: after,
        validated: false,
        decision: PolicyDecision::Allow,
    };
    if let Err(reason) = policy.validate(proposal) {
        trace.decision = PolicyDecision::Deny {
            reason: format!("validation: {reason}"),
        };
        return (
            GateResult::Deny {
                reason: format!("validation: {reason}"),
            },
            trace,
        );
    }
    trace.validated = true;
    let d = policy.decide(proposal, auth);
    trace.decision = d.clone();
    let r = match d {
        PolicyDecision::Allow => GateResult::Allow,
        PolicyDecision::Deny { reason } => GateResult::Deny { reason },
        PolicyDecision::Confirm { by } => GateResult::NeedsConfirm { by },
    };
    (r, trace)
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

    struct Scripted;
    impl Policy for Scripted {
        fn transform(&self, p: &mut Proposal, _: &Authority) {
            p.args["redacted"] = json!(true);
        }
        fn validate(&self, p: &Proposal) -> Result<(), String> {
            if p.tool.is_empty() {
                Err("empty tool".into())
            } else {
                Ok(())
            }
        }
        fn decide(&self, p: &Proposal, _: &Authority) -> PolicyDecision {
            if p.tool.starts_with("proc.") {
                PolicyDecision::Confirm { by: "eddie".into() }
            } else {
                PolicyDecision::Allow
            }
        }
    }

    #[test]
    fn gate_runs_in_order_and_reports() {
        let mut p = Proposal {
            tool: "proc.run".into(),
            args: json!({"argv": ["ls"]}),
            resource: None,
            policy_context: json!({}),
        };
        let (r, t) = run_gate(&Scripted, &mut p, &Authority::default());
        assert!(t.transformed);
        assert_eq!(p.args["redacted"], json!(true));
        assert!(t.validated);
        assert_eq!(r, GateResult::NeedsConfirm { by: "eddie".into() });
        assert_eq!(t.digest_after, digest_proposal(&p));
        let mut bad = Proposal {
            tool: "".into(),
            args: json!({}),
            resource: None,
            policy_context: json!({}),
        };
        let (r, t) = run_gate(&Scripted, &mut bad, &Authority::default());
        assert!(matches!(r, GateResult::Deny { .. }));
        assert!(!t.validated);
    }
}
