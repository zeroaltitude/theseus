//! L1's facts (M4 17b; design §2.11): a job that starts in the sandbox,
//! and its egress. The probe after serving and its `sandbox.probe` rows are
//! gone (theseus-gyin): a stored row still reads, as an unknown kind.

use std::path::Path;

use serde_json::{json, Value};
use theseus_kernel::job::L1;
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Tool;
use theseus_sandbox::egress::{Reached, Refused};

use super::{Fact, Say};
use crate::narrative;
use crate::sandbox::Sandbox;
use crate::scrub::Scrubber;

/// A job started in L1 (`sandbox.started`): its limits, its read-only
/// paths, and its egress list (18c; empty for no network). Its
/// `tool.started` carries `class: l1` and the list beside it, and each grant
/// it was given has its own `secret.granted`.
pub struct SandboxStarted<'a> {
    pub correlation_id: &'a str,
    pub tool: &'a str,
    pub argv: &'a [String],
    pub cwd: &'a Path,
    pub view: &'a L1,
    pub sandbox: &'a Sandbox,
    /// For the narrative's subject, which may name a secret's value.
    pub scrubber: &'a Scrubber,
    /// The variables its grants gave it at its launch, names only.
    pub given: &'a [&'a str],
}

impl Fact for SandboxStarted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SandboxStarted);

    fn row(&self) -> Value {
        let v = self.view;
        json!({"correlation_id": self.correlation_id, "tool": self.tool, "class": "l1",
            "limits": {"pids": v.limits.pids, "scratch_mb": v.limits.scratch_mb,
                "output_mb": v.limits.output_mb},
            "ro_paths": v.ro_paths, "egress": v.egress})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let subject = narrative::subject(self.tool, Some(self.argv), None, self.cwd);
        let reach = theseus_protocol::sandbox::reach(&self.view.egress);
        say.line(
            Tool,
            format!(
                "{} runs in L1, the sandbox: {reach}, {}, {}; what it writes goes to scratch, \
                 and is discarded.",
                self.scrubber.scrub(&subject).0,
                crate::sandbox::given(self.given),
                self.sandbox.limits_line()
            ),
        );
    }
}

/// A host an L1 job reached through its egress proxy (`sandbox.egress`,
/// M4 18c): one row per host per job, with its connections, the bytes each
/// way, and the tunnels' milliseconds. It rides in the frame that writes
/// the job's result.
pub struct SandboxEgress<'a> {
    pub correlation_id: &'a str,
    pub tool: &'a str,
    pub reached: &'a Reached,
}

impl Fact for SandboxEgress<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SandboxEgress);

    fn row(&self) -> Value {
        let r = self.reached;
        json!({"correlation_id": self.correlation_id, "tool": self.tool, "host": r.host,
            "port": r.port, "connections": r.connections, "up": r.up, "down": r.down, "ms": r.ms})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Tool,
            format!(
                "Job {} reached {}.",
                narrative::short(self.correlation_id),
                crate::egress::reached(self.reached)
            ),
        );
    }
}

/// The `CONNECT`s an L1 job's proxy refused for one reason at one host
/// (`sandbox.egress_refused`, M4 18c): the host, the port, the reason, and
/// how many times.
pub struct SandboxEgressRefused<'a> {
    pub correlation_id: &'a str,
    pub tool: &'a str,
    pub refused: &'a Refused,
}

impl Fact for SandboxEgressRefused<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SandboxEgressRefused);

    fn row(&self) -> Value {
        let r = self.refused;
        json!({"correlation_id": self.correlation_id, "tool": self.tool, "host": r.host,
            "port": r.port, "why": r.why, "count": r.count})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Tool,
            format!(
                "Job {}'s proxy refused a connection: {}.",
                narrative::short(self.correlation_id),
                self.refused.why
            ),
        );
    }
}
