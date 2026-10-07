//! What the gate reads from an operation (AWS design §3.1, "What the plan
//! derives from the model"): its class, its retry class, and its flags.
//!
//! The order, first match wins:
//! 1. **The override table** ([`tables::CLASS`]): what the models get wrong or
//!    do not say (SQS `ReceiveMessage` is a write; STS's mints are writes,
//!    whatever their names; KMS `Decrypt` is a read).
//! 2. **The Run list** ([`tables::RUN`]): operations that start code of the
//!    caller's choosing.
//! 3. **Read** when the model marks the operation `readonly`, its method is
//!    GET or HEAD, or its name starts with a read verb ([`tables::READ_PREFIXES`]).
//! 4. **Write**, otherwise.
//!
//! Then the flags, each from its own tested table: cost-bearing (every Run,
//! unless the table says the run is free), secret-bearing, IaC-only (writes
//! only, never tagging), and inert. The retry class comes from the traits:
//! `readonly`, `idempotent`, or a read is safe to repeat; an
//! `idempotencyToken` member makes the call idempotent with its key; anything
//! else is non-repeatable. A row of [`tables::RETRY`] settles the rest.

use serde::Serialize;

use crate::model::{Method, OperationRef};
use crate::tables;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Read,
    Write,
    /// Starts code of the caller's choosing.
    Run,
}

impl Class {
    /// The catalog's short form: `R`, `W`, `Run`.
    pub fn short(self) -> &'static str {
        match self {
            Class::Read => "R",
            Class::Write => "W",
            Class::Run => "Run",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Class::Read => "read",
            Class::Write => "write",
            Class::Run => "run",
        }
    }
}

/// What a repeat of the call would do (§3.16's retry classes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryClass {
    SafeToRepeat,
    /// The call carries an idempotency token, filled from the call's
    /// correlation id, so AWS runs a repeat at most once.
    IdempotentWithKey,
    /// Retried only on an error that proves the request did not run.
    NonRepeatable,
}

impl RetryClass {
    pub fn as_str(self) -> &'static str {
        match self {
            RetryClass::SafeToRepeat => "safe_to_repeat",
            RetryClass::IdempotentWithKey => "idempotent_with_key",
            RetryClass::NonRepeatable => "non_repeatable",
        }
    }
}

/// Whether the result holds a credential or a secret value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretBearing {
    No,
    Always,
    /// When the named boolean input member is true (SSM `WithDecryption`).
    WhenInputTrue(&'static str),
    /// When the result holds one: a secret a resource keeps only sometimes
    /// (a client's secret, a tunnel's pre-shared key). What is found is
    /// held, and a result with none is returned whole, where `Always` and
    /// `WhenInputTrue` fail closed (theseus-qan5). Never a mint's: there a
    /// walk that finds nothing means the walk is wrong.
    WhenPresent,
}

impl SecretBearing {
    /// Whether this call's input makes the result secret.
    pub fn for_input(self, input: &serde_json::Value) -> bool {
        match self {
            SecretBearing::No => false,
            SecretBearing::Always | SecretBearing::WhenPresent => true,
            SecretBearing::WhenInputTrue(m) => input.get(m).and_then(|v| v.as_bool()) == Some(true),
        }
    }

    /// Whether a secret-bearing result in which nothing is found to hold
    /// is withheld whole: every kind but `WhenPresent`.
    pub fn fails_closed(self) -> bool {
        self != SecretBearing::WhenPresent
    }
}

/// Which rule set the class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassReason {
    /// A row of the override table.
    Override,
    /// The Run list.
    RunList,
    /// The model's `readonly` trait.
    ReadonlyTrait,
    /// GET or HEAD.
    HttpMethod,
    /// A read verb begins the name.
    NamePrefix,
    /// Nothing marked it a read.
    Default,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Classification {
    pub class: Class,
    pub reason: ClassReason,
    pub retry: RetryClass,
    /// The input member filled from the call's correlation id when the
    /// caller leaves it out.
    pub idempotency_token: Option<String>,
    pub cost_bearing: bool,
    pub secret: SecretBearing,
    /// Durable infrastructure: `aws.call` returns invalid input and points to
    /// the stack tools (§3.4).
    pub iac_only: bool,
    /// A write that changes nothing until a later call (CloudFormation
    /// `CreateChangeSet`).
    pub inert: bool,
    /// Why an override row says what it says.
    pub note: Option<&'static str>,
}

impl Classification {
    /// `R`, `W $ IaC`, `Run $`, `R 🔑`: the catalog table's notation.
    pub fn label(&self) -> String {
        let mut s = self.class.short().to_owned();
        if self.cost_bearing {
            s.push_str(" $");
        }
        if self.secret != SecretBearing::No {
            s.push_str(" 🔑");
        }
        if self.iac_only {
            s.push_str(" IaC");
        }
        s
    }
}

/// A glob over operation names: `*` matches any run of characters.
pub(crate) fn glob(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or("");
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let tail: Vec<&str> = parts.collect();
    let Some((last, middle)) = tail.split_last() else {
        return rest.is_empty();
    };
    for m in middle {
        match rest.find(m) {
            Some(i) => rest = &rest[i + m.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

fn in_table(table: &[tables::Ops], service: &str, op: &str) -> bool {
    table
        .iter()
        .filter(|t| t.service == service)
        .any(|t| t.ops.iter().any(|p| glob(p, op)))
}

/// Classifies one operation.
pub fn classify(op: OperationRef<'_>) -> Classification {
    let svc = op.service().name();
    let name = op.name();
    let over = tables::CLASS
        .iter()
        .find(|o| o.service == svc && glob(o.op, name));

    let (class, reason) = if let Some(c) = over.and_then(|o| o.class) {
        (c, ClassReason::Override)
    } else if in_table(tables::RUN, svc, name) {
        (Class::Run, ClassReason::RunList)
    } else if op.is_readonly() {
        (Class::Read, ClassReason::ReadonlyTrait)
    } else if matches!(op.method(), Method::Get | Method::Head) {
        (Class::Read, ClassReason::HttpMethod)
    } else if tables::READ_PREFIXES
        .iter()
        .any(|p| starts_with_word(name, p))
    {
        (Class::Read, ClassReason::NamePrefix)
    } else {
        (Class::Write, ClassReason::Default)
    };

    let idempotency_token = op.idempotency_token().map(|m| m.name().to_owned());
    // A token outranks `idempotent`: the models mark a call with a token
    // idempotent because of the token, so a repeat is safe only with it.
    let retry = match tables::RETRY
        .iter()
        .find(|r| r.service == svc && glob(r.op, name))
    {
        Some(r) => r.retry,
        None if op.is_readonly() || class == Class::Read => RetryClass::SafeToRepeat,
        None if idempotency_token.is_some() => RetryClass::IdempotentWithKey,
        None if op.is_idempotent() => RetryClass::SafeToRepeat,
        None => RetryClass::NonRepeatable,
    };

    let cost_bearing = match over.and_then(|o| o.cost) {
        Some(c) => c,
        None => {
            (class == Class::Run && !in_table(tables::FREE_RUN, svc, name))
                || in_table(tables::COST, svc, name)
        }
    };

    let secret = match tables::SECRET
        .iter()
        .find(|s| s.service == svc && glob(s.op, name))
    {
        Some(s) => s.secret,
        None if in_table(tables::WHEN_PRESENT, svc, name) => SecretBearing::WhenPresent,
        None => SecretBearing::No,
    };

    let inert = over.is_some_and(|o| o.inert);
    let tagging = tables::TAGGING.iter().any(|p| glob(p, name));
    let iac_only = class == Class::Write
        && !inert
        && !tagging
        && !tables::NOT_IAC_PREFIXES
            .iter()
            .any(|p| starts_with_word(name, p))
        && !in_table(tables::NOT_IAC, svc, name)
        && in_table(tables::IAC, svc, name);

    Classification {
        class,
        reason,
        retry,
        idempotency_token,
        cost_bearing,
        secret,
        iac_only,
        inert,
        note: over.map(|o| o.note),
    }
}

/// `name` starts with the verb `p` as a whole word: `ListRoles` with `List`,
/// but not `Listen` with `List`, nor `Getaway` with `Get`.
fn starts_with_word(name: &str, p: &str) -> bool {
    match name.strip_prefix(p) {
        Some(rest) => {
            rest.is_empty()
                || rest.starts_with(|c: char| c.is_ascii_uppercase() || c.is_ascii_digit())
        }
        None => false,
    }
}

impl<'a> OperationRef<'a> {
    /// The operation's class, retry class, and flags.
    pub fn classify(&self) -> Classification {
        classify(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob("ListObjectsV2", "ListObjectsV2"));
        assert!(!glob("ListObjects", "ListObjectsV2"));
        assert!(glob("*", "Anything"));
        assert!(glob("*Vpc*", "CreateVpcEndpoint"));
        assert!(glob("*Vpc*", "DeleteVpc"));
        assert!(!glob("*Vpc*", "DeleteSubnet"));
        assert!(glob("Restore*", "RestoreDBInstanceFromDBSnapshot"));
        assert!(glob("*Policy", "PutRolePolicy"));
        assert!(!glob("*Policy", "PutRolePolicyVersion"));
        assert!(glob("A*B*C", "AxxBxxC"));
        assert!(!glob("A*B*C", "AxxCxxB"));
        assert!(!glob("AB*BA", "ABA"));
    }

    /// Every table row names a service in the catalog, and each of its
    /// patterns matches an operation there; an IaC row matches a write.
    #[test]
    fn every_table_row_names_real_operations() {
        let c = crate::Catalog::embedded().unwrap();
        let mut bad = Vec::new();
        let check =
            |bad: &mut Vec<String>, table: &str, service: &str, pattern: &str, want_write: bool| {
                let Ok(svc) = c.service(service) else {
                    bad.push(format!("{table}: no service {service}"));
                    return;
                };
                let hits: Vec<_> = svc
                    .operations()
                    .filter(|o| glob(pattern, o.name()))
                    .collect();
                if hits.is_empty() {
                    bad.push(format!("{table}: {service}:{pattern} matches nothing"));
                } else if want_write && !hits.iter().any(|o| o.classify().iac_only) {
                    bad.push(format!(
                        "{table}: {service}:{pattern} matches no IaC-only write"
                    ));
                }
            };
        for (name, table, write) in [
            ("RUN", tables::RUN, false),
            ("FREE_RUN", tables::FREE_RUN, false),
            ("COST", tables::COST, false),
            ("NOT_IAC", tables::NOT_IAC, false),
            ("IAC", tables::IAC, true),
        ] {
            for row in table {
                for p in row.ops {
                    check(&mut bad, name, row.service, p, write);
                }
            }
        }
        for row in tables::CLASS {
            check(&mut bad, "CLASS", row.service, row.op, false);
        }
        for row in tables::SECRET {
            check(&mut bad, "SECRET", row.service, row.op, false);
            if let SecretBearing::WhenInputTrue(m) = row.secret {
                let svc = c.service(row.service).unwrap();
                for o in svc.operations().filter(|o| glob(row.op, o.name())) {
                    if o.input().and_then(|i| i.member(m)).is_none() {
                        bad.push(format!(
                            "SECRET: {}:{} has no input member {m}",
                            row.service,
                            o.name()
                        ));
                    }
                }
            }
        }
        for row in tables::WHEN_PRESENT {
            for p in row.ops {
                check(&mut bad, "WHEN_PRESENT", row.service, p, false);
            }
        }
        for row in tables::RETRY {
            check(&mut bad, "RETRY", row.service, row.op, false);
        }
        assert!(
            bad.is_empty(),
            "{} bad rows:\n{}",
            bad.len(),
            bad.join("\n")
        );
    }

    /// `WhenPresent` is never a mint's, nor shadowed by a `SECRET` row
    /// (theseus-qan5): every operation its table names classifies as
    /// `WhenPresent`, and none carries the MINT note, where `Always`' fail
    /// closed is the tripwire for a walk that stops finding the member.
    #[test]
    fn a_mint_is_never_when_present() {
        let c = crate::Catalog::embedded().unwrap();
        let mut bad = Vec::new();
        for row in tables::WHEN_PRESENT {
            let svc = c.service(row.service).unwrap();
            for o in svc
                .operations()
                .filter(|o| row.ops.iter().any(|p| glob(p, o.name())))
            {
                let k = o.classify();
                let what = format!("{}:{}", row.service, o.name());
                if k.secret != SecretBearing::WhenPresent {
                    bad.push(format!("{what} is {:?}: a SECRET row shadows it", k.secret));
                }
                if k.note == Some(tables::MINT) {
                    bad.push(format!("{what} is a mint"));
                }
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
        // aws.describe says so, for the model and the operator.
        let idp = c.service("cognito-idp").unwrap();
        let d = crate::describe_operation(idp.operation("DescribeUserPoolClient").unwrap());
        assert_eq!(d["secret"], "when_present", "{d}");
        assert_eq!(d["label"], "R 🔑", "{d}");
        assert!(!SecretBearing::WhenPresent.fails_closed());
        assert!(SecretBearing::Always.fails_closed());
        assert!(SecretBearing::WhenPresent.for_input(&serde_json::json!({})));
    }

    #[test]
    fn read_verbs_are_whole_words() {
        assert!(starts_with_word("ListRoles", "List"));
        assert!(starts_with_word("Get", "Get"));
        assert!(!starts_with_word("Getaway", "Get"));
        assert!(!starts_with_word("Listen", "List"));
        assert!(starts_with_word("BatchGetItem", "BatchGet"));
    }
}
