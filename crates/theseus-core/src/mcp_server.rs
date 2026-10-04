//! The MCP server's sessions (step 41b, M7 §2.5): what a session that an MCP
//! client opened may do. The listener is `theseusd`'s (`mcp.rs`), over
//! `theseus-mcp`'s `server` module, and it reaches the core only through the
//! protocol, on an in-process connection whose surface is `Surface::Mcp`.
//!
//! - **Never an approval surface.** An answer, a trust, a press, an undo, or
//!   a publish from `Surface::Mcp` is refused (`approval::Answerer::unknown`),
//!   ledgered as `approval.refused` with `via: mcp`; every method but those
//!   and the few the server's tools need is refused at once (`allowed`).
//! - **The principal is `mcp`.** A session opened on that surface gets an
//!   execution whose authority names `mcp` and carries `[mcp_server]`'s
//!   floor as a ceiling (`authority`), with `[mcp_server] spend_limit_usd` as
//!   its limit. The authority is stored with the execution, so the floor
//!   holds after a restart and in every task the session starts (the kernel
//!   copies a parent's authority). No record gains a field.
//! - **The floor.** Every call in such a session whose class is not `Read`
//!   runs at no looser a posture than the floor (`approve` by default), after
//!   the whole order of §3.9, as external text's hold does (`floor`). So an
//!   MCP client's turn reads freely, and every call that acts waits for the
//!   operator, who answers from the CLI, the web UI, or a private place.
//! - **Its own sessions only.** A turn from `Surface::Mcp` goes only into a
//!   session whose principal is `mcp` (`own`): an MCP client cannot write
//!   into the operator's other conversations, which have no floor.
//! - **Its place is private.** Its words go back to the MCP client: a
//!   process of the operator's own uid (the listener refuses any other, as
//!   the web UI does), holding the operator's key. That process can already
//!   reach the daemon's socket and read every session, so a shared class
//!   would hide nothing from it, and would take away the file tools and
//!   `proc.run` its turns are for. The floor is what keeps its acts the
//!   operator's.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use theseus_kernel::Authority;
use theseus_protocol::mcp_server::McpServerHealth;
use theseus_protocol::method;
use theseus_tools::ToolClass;

use crate::config::mcp_server::McpServerConfig;
use crate::policy::{Decision, Posture};

/// The principal of every session an MCP client opens: one key, so one
/// shared principal (§1, "Deferred").
pub const PRINCIPAL: &str = "mcp";

/// The authority's ceiling that holds the floor, by its posture's name.
pub const FLOOR_CEILING: &str = "posture_floor";

/// The setting a floored call's notice names.
pub const SETTING: &str = "[mcp_server] posture_floor";

/// The methods an MCP connection may call: the five tools' needs, all reads
/// but `session.open` and `turn.submit`. Every other is refused, approvals
/// first among them.
pub const ALLOWED: [&str; 7] = [
    method::SESSION_OPEN,
    method::TURN_SUBMIT,
    method::SESSION_WAIT,
    method::SESSION_HISTORY,
    method::TASK_LIST,
    method::WAKE_LIST,
    method::HEALTH,
];

/// The owner's acts, which reach their one judgment (`Core::judge_act`) and
/// are refused there, ledgered as `approval.refused` with `via: mcp`, so a
/// refusal is recorded as every other surface's is.
pub const JUDGED: [&str; 5] = [
    method::ACTION_CONFIRM,
    method::POLICY_TIGHTEN,
    method::POLICY_UNTIGHTEN,
    method::POLICY_TRUST,
    method::PLACE_PUBLISH,
];

/// Whether `Surface::Mcp` may call `name`: the tools' methods, and the
/// owner's acts, which their judgment refuses.
pub fn allowed(name: &str) -> bool {
    ALLOWED.contains(&name) || JUDGED.contains(&name)
}

/// The authority of a session an MCP client opens.
pub fn authority(cfg: &McpServerConfig) -> Authority {
    Authority {
        principal: PRINCIPAL.into(),
        delegated_by: None,
        ceilings: BTreeMap::from([(FLOOR_CEILING.into(), cfg.posture_floor.as_str().into())]),
    }
}

/// The floor an execution's authority carries: none for the operator's own.
pub fn floor_of(a: &Authority) -> Option<Posture> {
    if a.principal != PRINCIPAL {
        return None;
    }
    // A floor that does not read is the strictest: the authority says one
    // is there.
    Some(match a.ceilings.get(FLOOR_CEILING).map(String::as_str) {
        Some("open") => Posture::Open,
        Some("notify") => Posture::Notify,
        _ => Posture::Approve,
    })
}

/// Whether a session with this authority is one an MCP client may write to.
pub fn own(a: &Authority) -> bool {
    a.principal == PRINCIPAL
}

/// The gate's decision for a call of `class` under `floor`, after the whole
/// order of §3.9: a call that acts runs at no looser a posture than the
/// floor; a read keeps its own.
pub fn floor(
    d: Decision,
    class: ToolClass,
    floor: Option<Posture>,
    tool: &str,
    summary: &str,
) -> Decision {
    match floor {
        Some(f) if class != ToolClass::Read => d.at_least(
            f,
            "an MCP client opened this session, and its calls that act wait for the operator",
            SETTING,
            tool,
            summary,
        ),
        _ => d,
    }
}

/// Health's `mcp_server` block, as the listener last set it.
#[derive(Default)]
pub struct Board {
    live: RwLock<Option<Live>>,
}

#[derive(Clone)]
enum Live {
    /// Starting, stopped, or failed: as said.
    Said(McpServerHealth),
    /// Listening: the server's own counters, read when health is asked.
    Listening(Arc<dyn Fn() -> McpServerHealth + Send + Sync>),
}

impl Board {
    /// The listener's state while it does not listen.
    pub fn say(&self, h: McpServerHealth) {
        *self.live.write().unwrap() = Some(Live::Said(h));
    }

    /// It listens: health reads its counters from `read`.
    pub fn listening(&self, read: Arc<dyn Fn() -> McpServerHealth + Send + Sync>) {
        *self.live.write().unwrap() = Some(Live::Listening(read));
    }

    /// The block, while `[mcp_server]` is on: `starting` until the listener
    /// says otherwise.
    pub fn health(&self, enabled: bool) -> Option<McpServerHealth> {
        if !enabled {
            return None;
        }
        let live = self.live.read().unwrap().clone();
        Some(match live {
            None => McpServerHealth {
                state: "starting".into(),
                ..McpServerHealth::default()
            },
            Some(Live::Said(h)) => h,
            Some(Live::Listening(read)) => read(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The floor raises a call that acts and leaves a read alone; the
    /// operator's own sessions have none; a floor that does not read is the
    /// strictest.
    #[test]
    fn the_floor_holds_acts_and_leaves_reads() {
        let cfg = McpServerConfig::default();
        let a = authority(&cfg);
        assert_eq!(floor_of(&a), Some(Posture::Approve));
        assert!(own(&a));
        let operator = Authority {
            principal: "operator".into(),
            ..Authority::default()
        };
        assert_eq!(floor_of(&operator), None);
        assert!(!own(&operator));
        let mut odd = a;
        odd.ceilings.insert(FLOOR_CEILING.into(), "lenient".into());
        assert_eq!(floor_of(&odd), Some(Posture::Approve));
        let open = || Decision {
            posture: Posture::Open,
            reason: "fs.write: open".into(),
            notify: None,
            floor: false,
            granted: None,
            external: None,
        };
        let d = floor(
            open(),
            ToolClass::Write,
            Some(Posture::Approve),
            "fs.write",
            "x",
        );
        assert_eq!(d.posture, Posture::Approve);
        assert!(
            d.reason.contains("an MCP client opened this session"),
            "{}",
            d.reason
        );
        let d = floor(
            open(),
            ToolClass::Read,
            Some(Posture::Approve),
            "fs.read",
            "x",
        );
        assert_eq!(d.posture, Posture::Open);
        let d = floor(open(), ToolClass::Run, None, "proc.run", "x");
        assert_eq!(d.posture, Posture::Open);
        let notify = McpServerConfig {
            posture_floor: Posture::Notify,
            ..McpServerConfig::default()
        };
        let d = floor(
            open(),
            ToolClass::Run,
            floor_of(&authority(&notify)),
            "proc.run",
            "x",
        );
        assert_eq!(d.posture, Posture::Notify);
        assert!(allowed("turn.submit") && allowed("action.confirm"));
        assert!(!allowed("execution.cancel") && !allowed("shutdown") && !allowed("ledger.tail"));
    }
}
