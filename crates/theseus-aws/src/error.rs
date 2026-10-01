//! What a failed call says (AWS design §3.1, "Errors"): AWS's own code and
//! message, the request id, whether a retry could help, and, for a denial,
//! which enforcer refused (§3.6: IAM, an SCP, or Theseus's own guard).

use serde::Serialize;

/// Whether retrying an AWS error could help.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorRetry {
    /// AWS refused the request to slow the caller down; it did not run, so a
    /// retry is safe for every retry class.
    Throttle,
    /// A server-side failure (5xx, a request timeout). Whether the request
    /// ran is unknown, so only a call that is safe to repeat retries it.
    Transient,
    /// A retry would fail the same way.
    No,
}

/// The enforcers of §3.6.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Enforcer {
    /// IAM: an identity-based or resource-based policy, a permissions
    /// boundary, a VPC endpoint policy, or a session policy that allowed too
    /// little (a job's narrowing, which Theseus chose).
    Iam,
    /// A service control policy, from an Organization.
    Scp,
    /// A resource control policy, from an Organization.
    Rcp,
    /// Theseus's own guard: an explicit deny in a session policy, which on
    /// Theseus's sessions only the deny-only `theseus-guard-*` policies hold.
    Guard,
}

impl Enforcer {
    pub fn as_str(self) -> &'static str {
        match self {
            Enforcer::Iam => "iam",
            Enforcer::Scp => "scp",
            Enforcer::Rcp => "rcp",
            Enforcer::Guard => "guard",
        }
    }
}

/// A denial, as AWS's message describes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Denial {
    pub enforcer: Enforcer,
    /// The kind of policy AWS names: `service control policy`,
    /// `identity-based policy`, `session policy`, and so on.
    pub policy_type: String,
    /// An explicit deny, or no policy that allows.
    pub explicit: bool,
    /// The policy's ARN, when the message carries one.
    pub policy: Option<String>,
}

/// The policy types AWS's denial messages name, and who enforces each.
const POLICY_TYPES: &[(&str, Enforcer)] = &[
    ("service control policy", Enforcer::Scp),
    ("resource control policy", Enforcer::Rcp),
    ("session policy", Enforcer::Guard),
    ("identity-based policy", Enforcer::Iam),
    ("permissions boundary", Enforcer::Iam),
    ("resource-based policy", Enforcer::Iam),
    ("vpc endpoint policy", Enforcer::Iam),
];

/// Reads a denial from AWS's message: "… with an explicit deny in a service
/// control policy", "… because no identity-based policy allows the
/// s3:ListBucket action", sometimes followed by the policy's ARN.
pub fn parse_denial(message: &str) -> Option<Denial> {
    let lower = message.to_ascii_lowercase();
    let (explicit, at) = match lower.find("explicit deny in a") {
        Some(i) => (true, i + "explicit deny in a".len()),
        None => (false, lower.find("because no ")? + "because no ".len()),
    };
    let rest = lower[at..].trim_start_matches('n').trim_start();
    let (policy_type, mut enforcer) = POLICY_TYPES
        .iter()
        .find(|(p, _)| rest.starts_with(p))
        .map(|(p, e)| ((*p).to_owned(), *e))?;
    // An implicit deny from a session policy is a job's narrowing, which
    // allowed too little; only an explicit one is a guard.
    if !explicit && enforcer == Enforcer::Guard {
        enforcer = Enforcer::Iam;
    }
    // A policy ARN may follow (`…policy: arn:aws:iam::…:policy/x`).
    let tail = &message[message.len() - rest.len() + policy_type.len()..];
    let policy = tail
        .trim_start_matches([':', ' '])
        .split_whitespace()
        .next()
        .filter(|w| w.starts_with("arn:"))
        .map(|w| w.trim_end_matches(['.', ',', ';']).to_owned());
    // A guard named by its ARN is a guard whatever the policy type says.
    if policy
        .as_deref()
        .is_some_and(|p| p.contains(":policy/theseus-guard"))
    {
        enforcer = Enforcer::Guard;
    }
    Some(Denial {
        enforcer,
        policy_type,
        explicit,
        policy,
    })
}

/// AWS answered with an error.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AwsError {
    pub status: u16,
    /// AWS's code (`AccessDenied`, `ThrottlingException`), as the model sees it.
    pub code: String,
    pub message: String,
    /// CloudTrail's `requestID`, for the ledger.
    pub request_id: Option<String>,
    pub retry: ErrorRetry,
    /// For a denial, who refused.
    pub denial: Option<Denial>,
}

impl std::fmt::Display for AwsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (HTTP {})", self.code, self.status)?;
        if !self.message.is_empty() {
            write!(f, ": {}", self.message)?;
        }
        if let Some(d) = &self.denial {
            write!(f, " [refused by {}]", d.enforcer.as_str())?;
        }
        if let Some(id) = &self.request_id {
            write!(f, " [request {id}]")?;
        }
        Ok(())
    }
}

/// Why a call failed.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum CallError {
    /// Not a call the catalog describes: an unknown service or operation, or
    /// input that does not match the operation's shape. Nothing was sent.
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// A call this client cannot make yet (an event stream, SigV2, a bearer
    /// token, S3 directory buckets); the CLI can. Nothing was sent.
    #[error("unsupported by the AWS client: {0}")]
    Unsupported(String),
    /// AWS refused or failed the call.
    #[error("{0}")]
    Aws(Box<AwsError>),
    /// The request was sent, and nothing proves whether it ran: a timeout or
    /// a dropped connection after sending, or a server error, on a call that
    /// is not safe to repeat. The reconciler resolves it through the matching
    /// read, or the principal is told (§3.1, "Retry class").
    #[error("outcome unknown after {attempts} attempt(s): {reason}")]
    OutcomeUnknown {
        attempts: u32,
        reason: String,
        /// AWS's error, when it answered with one.
        error: Option<Box<AwsError>>,
    },
    /// The request never left: the connection failed before sending, on every
    /// attempt.
    #[error("not sent after {attempts} attempt(s): {reason}")]
    NotSent { attempts: u32, reason: String },
    /// AWS answered, but not as the model says. A 2xx status means the call
    /// ran.
    #[error("unreadable response (HTTP {status}): {reason}")]
    Unreadable {
        status: u16,
        request_id: Option<String>,
        reason: String,
    },
    /// The call ran, and its answer is bigger than the client reads.
    #[error("the response exceeds {limit} bytes; the call ran (request {request_id:?})")]
    TooLarge {
        limit: usize,
        request_id: Option<String>,
    },
}

impl CallError {
    /// AWS's error, if AWS answered with one.
    pub fn aws(&self) -> Option<&AwsError> {
        match self {
            CallError::Aws(e) => Some(e),
            CallError::OutcomeUnknown { error, .. } => error.as_deref(),
            _ => None,
        }
    }

    /// Whether the call may have run: the reconciler's question.
    pub fn may_have_run(&self) -> bool {
        matches!(
            self,
            CallError::OutcomeUnknown { .. }
                | CallError::TooLarge { .. }
                | CallError::Unreadable { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denials_name_their_enforcer() {
        let scp = parse_denial(
            "User: arn:aws:sts::111122223333:assumed-role/theseus-owner/exe_1 is not \
             authorized to perform: s3:DeleteBucket on resource: arn:aws:s3:::example-bucket \
             with an explicit deny in a service control policy",
        )
        .unwrap();
        assert_eq!(scp.enforcer, Enforcer::Scp);
        assert!(scp.explicit);
        assert_eq!(scp.policy, None);

        let guard = parse_denial(
            "User: arn:aws:sts::111122223333:assumed-role/theseus-owner/exe_1 is not \
             authorized to perform: ec2:CreateVpc with an explicit deny in a session policy",
        )
        .unwrap();
        assert_eq!(guard.enforcer, Enforcer::Guard);

        let named = parse_denial(
            "User: arn:aws:iam::111122223333:user/example is not authorized to perform: \
             iam:CreateUser on resource: arn:aws:iam::111122223333:user/x with an explicit \
             deny in an identity-based policy: arn:aws:iam::111122223333:policy/theseus-guard-limits",
        )
        .unwrap();
        assert_eq!(named.policy_type, "identity-based policy");
        assert_eq!(
            named.policy.as_deref(),
            Some("arn:aws:iam::111122223333:policy/theseus-guard-limits")
        );
        assert_eq!(named.enforcer, Enforcer::Guard);

        let implicit = parse_denial(
            "User: arn:aws:iam::111122223333:user/example is not authorized to perform: \
             codecommit:ListRepositories because no identity-based policy allows the \
             codecommit:ListRepositories action",
        )
        .unwrap();
        assert_eq!(
            (implicit.enforcer, implicit.explicit),
            (Enforcer::Iam, false)
        );

        let narrowed =
            parse_denial("… because no session policy allows the s3:PutObject action").unwrap();
        assert_eq!(narrowed.enforcer, Enforcer::Iam);

        let boundary = parse_denial("… with an explicit deny in a permissions boundary").unwrap();
        assert_eq!(boundary.enforcer, Enforcer::Iam);
        assert_eq!(parse_denial("Access Denied"), None);
    }
}
