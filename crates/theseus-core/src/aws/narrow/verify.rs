//! The checks a candidate passes before it is proposed.
//!
//! 1. IAM Access Analyzer's `ValidatePolicy` (identity policy): an `ERROR` or
//!    a `SECURITY_WARNING` blocks; `WARNING` and `SUGGESTION` are listed.
//! 2. `SimulateCustomPolicy` of every observed call against the candidate and
//!    the policies that stay: each must be `allowed`, or the proposal is not
//!    made (a denial, or an answer that needed context the simulator was not
//!    given, is not proof).
//! 3. The guard crate's own scan of the template that would apply it
//!    (`apply::patch`), so a candidate the guards would stop is never offered.
//!
//! The service is a [`Checker`]: the account's in production, a fake in tests.

use std::future::Future;

use serde_json::Value;

use super::build::Candidate;
use super::policy::Doc;
use super::usage::Observed;

/// One finding of the analyzer.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// `ERROR`, `SECURITY_WARNING`, `WARNING` or `SUGGESTION`.
    pub kind: String,
    pub code: String,
    pub detail: String,
}

impl Finding {
    pub fn blocks(&self) -> bool {
        matches!(self.kind.as_str(), "ERROR" | "SECURITY_WARNING")
    }
}

/// One simulated call: the action, the ARNs it named (none: any).
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub action: String,
    pub resources: Vec<String>,
}

/// One answer of the simulator.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub action: String,
    pub resource: Option<String>,
    /// `allowed`, `implicitDeny` or `explicitDeny`.
    pub decision: String,
    /// Context keys the simulator wanted and was not given.
    pub missing: Vec<String>,
}

pub trait Checker {
    fn validate(&self, doc: &Value) -> impl Future<Output = Result<Vec<Finding>, String>> + Send;
    fn simulate(
        &self,
        policies: &[Value],
        calls: &[Call],
    ) -> impl Future<Output = Result<Vec<Answer>, String>> + Send;
}

/// What the checks found.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Verdict {
    pub blocking: Vec<Finding>,
    pub listed: Vec<Finding>,
    /// Observed calls the candidate would not allow, in words.
    pub denied: Vec<String>,
    /// Calls simulated.
    pub simulated: usize,
}

impl Verdict {
    pub fn ok(&self) -> bool {
        self.blocking.is_empty() && self.denied.is_empty()
    }

    /// Why there is no proposal, when there is none.
    pub fn why_not(&self) -> Option<String> {
        if self.ok() {
            return None;
        }
        let mut w = Vec::new();
        for f in &self.blocking {
            w.push(format!(
                "Access Analyzer {} {}: {}",
                f.kind, f.code, f.detail
            ));
        }
        for d in &self.denied {
            w.push(format!("the candidate would deny an observed call: {d}"));
        }
        Some(w.join("; "))
    }
}

/// The calls to simulate: every observed use of the role the candidate
/// replaces, with the ARNs it named.
pub fn calls_of(o: &Observed, cand: &Candidate) -> Vec<Call> {
    o.used
        .iter()
        .filter(|(a, _)| !cand.elsewhere.contains(a) && !cand.unexplained.contains(a))
        .map(|(a, u)| Call {
            action: a.clone(),
            resources: if u.unresolved {
                Vec::new()
            } else {
                u.resources.iter().cloned().collect()
            },
        })
        .collect()
}

/// Run the analyzer and the simulator on `cand`, with `rest` (what stays
/// attached) beside it in the simulation.
pub async fn verify(
    c: &impl Checker,
    o: &Observed,
    cand: &Candidate,
    rest: &Doc,
) -> Result<Verdict, String> {
    let doc = cand.doc.to_value();
    let mut v = Verdict::default();
    for f in c.validate(&doc).await? {
        if f.blocks() {
            v.blocking.push(f);
        } else {
            v.listed.push(f);
        }
    }
    let calls = calls_of(o, cand);
    v.simulated = calls.len();
    let mut policies = vec![doc];
    if !rest.stmts.is_empty() {
        policies.push(rest.to_value());
    }
    let answers = c.simulate(&policies, &calls).await?;
    for call in &calls {
        let mine: Vec<&Answer> = answers
            .iter()
            .filter(|a| a.action.eq_ignore_ascii_case(&call.action))
            .collect();
        if mine.is_empty() {
            v.denied
                .push(format!("{} (the simulator answered nothing)", call.action));
            continue;
        }
        for a in mine {
            if a.decision != "allowed" {
                let on = a
                    .resource
                    .as_deref()
                    .map(|r| format!(" on {r}"))
                    .unwrap_or_default();
                let ctx = if a.missing.is_empty() {
                    String::new()
                } else {
                    format!(" (needs context: {})", a.missing.join(", "))
                };
                v.denied
                    .push(format!("{}{on}: {}{ctx}", a.action, a.decision));
            }
        }
    }
    Ok(v)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use super::super::build::build;
    use super::super::build::tests::{allow_all, basics, seen, universe};
    use super::super::policy::{covers_action, Doc as D};
    use super::*;

    /// A fake client: the findings it returns, and a simulator that reads
    /// the policies it was given with this crate's own matcher.
    pub(crate) struct Fake {
        pub findings: Vec<Finding>,
        pub skip_simulation: bool,
        pub asked: Mutex<Vec<usize>>,
    }

    impl Checker for Fake {
        async fn validate(&self, _doc: &Value) -> Result<Vec<Finding>, String> {
            Ok(self.findings.clone())
        }
        async fn simulate(
            &self,
            policies: &[Value],
            calls: &[Call],
        ) -> Result<Vec<Answer>, String> {
            self.asked.lock().unwrap().push(calls.len());
            if self.skip_simulation {
                return Ok(Vec::new());
            }
            let docs: Vec<D> = policies.iter().map(|p| D::from_value(p).unwrap()).collect();
            Ok(calls
                .iter()
                .map(|c| Answer {
                    action: c.action.clone(),
                    resource: c.resources.first().cloned(),
                    decision: if docs.iter().any(|d| covers_action(d, &c.action)) {
                        "allowed".into()
                    } else {
                        "implicitDeny".into()
                    },
                    missing: Vec::new(),
                })
                .collect())
        }
    }

    fn fake(findings: Vec<Finding>) -> Fake {
        Fake {
            findings,
            skip_simulation: false,
            asked: Mutex::default(),
        }
    }

    fn f(kind: &str) -> Finding {
        Finding {
            kind: kind.into(),
            code: "X".into(),
            detail: "d".into(),
        }
    }

    #[tokio::test]
    async fn an_error_or_a_security_warning_blocks_and_suggestions_are_listed() {
        let o = seen(&[("ec2:DescribeInstances", &[])]);
        let c = build(&o, &allow_all(), &basics(), &universe());
        for kind in ["ERROR", "SECURITY_WARNING"] {
            let v = verify(&fake(vec![f(kind)]), &o, &c, &basics())
                .await
                .unwrap();
            assert!(!v.ok(), "{kind}");
            assert!(v.why_not().unwrap().contains("Access Analyzer"));
        }
        let v = verify(
            &fake(vec![f("SUGGESTION"), f("WARNING")]),
            &o,
            &c,
            &basics(),
        )
        .await
        .unwrap();
        assert!(v.ok());
        assert_eq!(v.listed.len(), 2);
        assert_eq!(v.simulated, 1);
    }

    #[tokio::test]
    async fn a_candidate_that_would_deny_an_observed_call_is_not_ok() {
        let o = seen(&[("ec2:DescribeInstances", &[])]);
        let mut c = build(&o, &allow_all(), &basics(), &universe());
        // Plant the defect: the observed action dropped from the candidate.
        for s in &mut c.doc.stmts {
            s.actions.retain(|a| a != "ec2:DescribeInstances");
        }
        let v = verify(&fake(vec![]), &o, &c, &basics()).await.unwrap();
        assert!(!v.ok());
        assert!(
            v.denied[0].contains("ec2:DescribeInstances"),
            "{:?}",
            v.denied
        );
    }

    #[tokio::test]
    async fn a_simulator_that_answers_nothing_is_not_proof() {
        let o = seen(&[("ec2:DescribeInstances", &[])]);
        let c = build(&o, &allow_all(), &basics(), &universe());
        let mut fk = fake(vec![]);
        fk.skip_simulation = true;
        let v = verify(&fk, &o, &c, &basics()).await.unwrap();
        assert!(!v.ok());
        assert!(v.denied[0].contains("answered nothing"));
    }
}
