//! The AWS guardrails (the AWS design's §3.6): one list, two enforcers.
//!
//! `guardrails.toml` is the only source. Theseus's gate reads it before an AWS call
//! ([`GuardList::check_call`]) and before a stack applies ([`GuardList::scan`],
//! [`GuardList::check_change_set`]), and asks at the floor when an entry hits. AWS enforces the same
//! list as the deny-only session guards and, once there is an Organization, as SCPs, all generated
//! here ([`GuardList::policies`]). Nothing here touches the network, and nothing runs until asked:
//! the list is parsed on first use.

mod change_set;
mod check;
mod cidr;
mod eval;
mod list;
mod node;
mod policy;
mod template;

use std::sync::OnceLock;

pub use change_set::{ChangeAction, ChangeHit, ChangeSetVerdict, ResourceChange};
pub use check::{Hit, Verdict};
pub use cidr::is_public;
pub use eval::{glob, Context, Fired, OneOrMany, Path, Truth, When};
pub use list::{
    ChangeRule, Destructive, GuardList, Guardrail, IacGroup, IamCondition, Limit, ListError, Scp,
    TemplateRule,
};
pub use node::Node;
pub use policy::{
    Document, Policy, PolicyKind, Statement, MANAGED_POLICY_LIMIT, SCPS_PER_TARGET, SCP_LIMIT,
    SESSION_POLICY_ARNS, SESSION_POLICY_PLAINTEXT,
};
pub use template::{parse_template, Scan, TemplateError, TemplateHit};

/// The list as shipped, in the crate.
pub const GUARDRAILS_TOML: &str = include_str!("../guardrails.toml");

/// The shipped list, parsed once. Its tests hold it valid, so a failure here is a build defect.
pub fn embedded() -> &'static GuardList {
    static LIST: OnceLock<GuardList> = OnceLock::new();
    LIST.get_or_init(|| {
        GuardList::parse(GUARDRAILS_TOML).expect("guardrails.toml is valid: its tests say so")
    })
}
