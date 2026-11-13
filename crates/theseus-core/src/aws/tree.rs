//! The tree the model walks to reach an AWS operation nobody wrote a tool
//! for (theseus-0nnk, row B6), and the rule that settles such an
//! operation's posture.
//!
//! **There is no tool per operation, and there never was meant to be.** The
//! sixteen names in [`super::NAMES`] are the *surfaces*: `aws.call` under
//! them reaches any operation of any service, and [`super::tools::Describe`]
//! (`aws.describe`) is the tree that finds one, read out of the embedded
//! catalog with no network:
//!
//! 1. **the root** — `aws.describe {}`: every service the catalog holds,
//!    with the snapshot the models came from;
//! 2. **a service** — `aws.describe {service}`: its operations by class
//!    (read, write, run), and the flags that matter (cost-bearing,
//!    secret-bearing, IaC-only, deprecated);
//! 3. **an operation** — `aws.describe {service, operation}`: its input and
//!    output as compact JSON schemas, its class, and its paginator;
//! 4. **the call** — `aws.call {service, operation, input}`.
//!
//! So the breadth of what a model can reach is a property of the catalog,
//! not of the tool list, and it does not grow by a `NAMES` entry:
//! [`breadth`] measures it, and `tests_tree` pins the figures.
//!
//! **A discovered operation's posture.** The gate reads an AWS call's
//! `[policy.aws]` line by its operation, then its service, then its *class*
//! (`crate::policy::ToolPolicy::aws_line`), and the class is the catalog's
//! own ([`theseus_aws::catalog::classify`]). The owner's cautious decision
//! for this row is that **a discovered AWS operation defaults to WRITE
//! posture unless the catalog proves it a read**, and [`proves_a_read`] is
//! that rule, named here so a new [`ClassReason`] cannot join the catalog
//! and be taken for a read in silence: its `match` is exhaustive, and
//! `tests_tree::the_catalog_never_calls_an_unproved_operation_a_read` holds
//! the two sides together over every operation in the catalog. The
//! fall-through is `ClassReason::Default`, which the catalog classes
//! `Class::Write`; nothing else may.
//!
//! Nothing here reads a secret, opens a connection, or decides an approval:
//! the posture is `crate::policy`'s, the guard list `theseus_aws_guard`'s,
//! and the floor's question the operator's as before.

use theseus_aws::catalog::{Catalog, Class, ClassReason};

/// Whether a reason *may* establish that an operation is a read
/// (theseus-0nnk; the owner's cautious decision).
///
/// It is one-directional on purpose. A reason that may prove a read does
/// not always: [`ClassReason::Override`] is a reviewed row of the catalog's
/// own table, and such a row may name any class (SQS `ReceiveMessage` is
/// overridden to a *write*). What the rule forbids is the other direction —
/// an operation called a read for a reason that cannot prove one, and an
/// operation nothing marked at all being anything but a write.
///
/// Each arm is a branch of the catalog's classification, in its order. The
/// `match` is exhaustive on purpose: a new [`ClassReason`] must be judged
/// here, and the safe judgement is `false`.
pub fn may_prove_a_read(reason: ClassReason) -> bool {
    match reason {
        // A reviewed row of the catalog's own override table.
        ClassReason::Override
        // AWS's model says the operation is readonly.
        | ClassReason::ReadonlyTrait
        // The protocol says so: a GET or a HEAD.
        | ClassReason::HttpMethod
        // A read verb begins the name, as a whole word.
        | ClassReason::NamePrefix => true,
        // The Run list: it starts code of the caller's choosing, so the
        // reason can never make a read.
        ClassReason::RunList => false,
        // Nothing marked it a read. It is a write, and it takes the write
        // posture: the cautious half of the owner's decision.
        ClassReason::Default => false,
    }
}

/// How far the tree reaches, counted over the whole catalog: the measure
/// this row cares about, since none of it needs a [`super::NAMES`] entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Breadth {
    /// Services at the root.
    pub services: usize,
    /// Operations under them: every one `aws.describe` can describe.
    pub operations: usize,
    /// Reads the catalog proves, by each branch of its rule.
    pub proved_reads: usize,
    /// Writes, the fall-through among them.
    pub writes: usize,
    /// Writes nothing proved: `ClassReason::Default`.
    pub unproved: usize,
    /// Operations that start code of the caller's choosing.
    pub runs: usize,
    /// Writes `aws.call` refuses as invalid input, pointing at the stack
    /// tools: durable infrastructure goes through a stack.
    pub iac_only: usize,
}

impl Breadth {
    /// What `aws.call` will plan: every operation but the IaC-only writes.
    pub fn callable(self) -> usize {
        self.operations - self.iac_only
    }
}

/// Walks the catalog and counts it. It decodes every service, so it is an
/// operator's read or a test's, never a turn's path.
pub fn breadth(catalog: &Catalog) -> Breadth {
    let mut b = Breadth {
        services: catalog.services().len(),
        ..Breadth::default()
    };
    for entry in catalog.services() {
        let Ok(svc) = catalog.service(&entry.name) else {
            continue;
        };
        for op in svc.operations() {
            let k = op.classify();
            b.operations += 1;
            match k.class {
                Class::Read => b.proved_reads += 1,
                Class::Write => b.writes += 1,
                Class::Run => b.runs += 1,
            }
            if k.reason == ClassReason::Default {
                b.unproved += 1;
            }
            if k.iac_only {
                b.iac_only += 1;
            }
        }
    }
    b
}
