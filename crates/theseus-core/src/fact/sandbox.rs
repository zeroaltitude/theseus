//! L1's facts (M4 17b; design §2.11): a job that starts in the sandbox,
//! and the probe after serving.

use std::path::Path;

use serde_json::{json, Value};
use theseus_kernel::job::L1;
use theseus_protocol::sandbox::SandboxProbe;
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Tool;

use super::{Fact, Say};
use crate::narrative;
use crate::sandbox::Sandbox;
use crate::scrub::Scrubber;

/// A job started in L1 (`sandbox.started`): its limits, its cgroup, its
/// read-only paths, and its egress (none until 18c). Its `tool.started`
/// carries `class: l1` beside it.
pub struct SandboxStarted<'a> {
    pub correlation_id: &'a str,
    pub tool: &'a str,
    pub argv: &'a [String],
    pub cwd: &'a Path,
    pub view: &'a L1,
    pub sandbox: &'a Sandbox,
    /// For the narrative's subject, which may name a secret's value.
    pub scrubber: &'a Scrubber,
}

impl Fact for SandboxStarted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SandboxStarted);

    fn row(&self) -> Value {
        let v = self.view;
        json!({"correlation_id": self.correlation_id, "tool": self.tool, "class": "l1",
            "limits": {"memory_mb": v.cgroup.as_ref().map(|_| v.memory_mb), "pids": v.limits.pids,
                "scratch_mb": v.limits.scratch_mb, "output_mb": v.limits.output_mb},
            "cgroup": self.sandbox.cgroup_line(), "ro_paths": v.ro_paths, "egress": []})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let subject = narrative::subject(self.tool, Some(self.argv), None, self.cwd);
        say.line(
            Tool,
            format!(
                "{} runs in L1, the sandbox: no network, no secret, {}; what it writes goes to \
                 scratch, and is discarded.",
                self.scrubber.scrub(&subject).0,
                self.sandbox.limits_line()
            ),
        );
    }
}

/// The probe after serving, or after a restart in place (`sandbox.probe`):
/// whether L1 runs here, and if not why; the start's time; what it found;
/// and the cgroup. No sentence: no session hears it.
pub struct SandboxProbed<'a> {
    pub probe: &'a SandboxProbe,
    pub cgroup: Option<&'a str>,
}

impl Fact for SandboxProbed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SandboxProbe);

    fn row(&self) -> Value {
        let p = self.probe;
        json!({"ok": p.ok, "why": p.why, "start_ms": p.start_ms, "sys": p.sys, "lo": p.lo,
            "skipped": p.skipped, "cgroup": self.cgroup})
    }
}
