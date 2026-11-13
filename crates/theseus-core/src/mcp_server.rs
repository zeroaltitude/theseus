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

/// The methods an MCP connection may call: the seven tools' needs, all reads
/// but `session.open` and `turn.submit` (`memory.lookup` answers only while
/// `[mcp_server] memory` is on). Every other is refused, approvals
/// first among them.
pub const ALLOWED: [&str; 8] = [
    method::SESSION_OPEN,
    method::TURN_SUBMIT,
    method::SESSION_WAIT,
    method::SESSION_HISTORY,
    method::TASK_LIST,
    method::WAKE_LIST,
    method::HEALTH,
    method::MEMORY_LOOKUP,
];

/// The owner's acts, which reach their one judgment (`Core::judge_act`) and
/// are refused there, ledgered as `approval.refused` with `via: mcp`, so a
/// refusal is recorded as every other surface's is.
pub const JUDGED: [&str; 7] = [
    method::ACTION_CONFIRM,
    method::WORK_ANSWER,
    method::POLICY_TIGHTEN,
    method::POLICY_UNTIGHTEN,
    method::CONTENT_UNLABEL,
    method::PLACE_PUBLISH,
    method::TASK_START,
];

/// Whether `Surface::Mcp` may call `name`: the tools' methods, and the
/// owner's acts, which their judgment refuses.
pub fn allowed(name: &str) -> bool {
    // A fork of a conversation it opened (`Core::fork_allowed` holds the rest).
    ALLOWED.contains(&name) || JUDGED.contains(&name) || name == method::SESSION_FORK
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

    /// **The requirement**: every method of the protocol, classified for
    /// `Surface::Mcp` (theseus-a7pr, in the shape of `crates/complete.rs`).
    ///
    /// `allowed` is an allowlist already, so a method nobody thought about
    /// is refused — which is safe, and is also why nobody thought about
    /// four of them. `session.history` handed an imported session's text to
    /// every surface for months; `node.list` and `session.list` do the same
    /// today and are out of an MCP client's reach only because they are
    /// absent from `ALLOWED`. Safety by accident is not a guard, and this
    /// list is what turns it into one: the three lists below must *partition*
    /// `theseus_protocol::method::ALL`, so a method added to the protocol
    /// cannot land until someone has written down which of the three it is.
    /// The act of adding a name to `ALLOWED` is the act at which "does this
    /// return a session's content, and does it take a surface?" gets asked.
    ///
    /// This is the other document, kept here rather than beside `ALLOWED`
    /// because nothing in the daemon reads it: the binary is unchanged.
    /// `the_three_lists_classify_every_method` walks `method::ALL` — it never
    /// reads `ALLOWED` to decide what to ask — and holds the lists to that
    /// walk in both directions, so a method left unclassified fails, and a
    /// name left behind by a removed or renamed method fails too.
    ///
    /// A method belongs here when the MCP server's tools have no use for it.
    /// To move one out, put it in `ALLOWED` (a tool needs it) or in `JUDGED`
    /// (it is one of the owner's acts, refused at its own judgment) and read
    /// `docs/design/imported-surface.md` first.
    const NOT_FOR_MCP: &[&str] = &[
        method::SESSION_LIST,
        method::SESSION_RETIRE,
        method::SESSION_REOPEN,
        method::SESSION_TYPING,
        method::LEDGER_TAIL,
        method::LEDGER_WINDOW,
        method::LEDGER_VERIFY,
        method::PROFILE_LIST,
        method::PROFILE_USE,
        method::PROFILE_RELEASE,
        method::PROFILE_PINS,
        method::PROFILE_UNPIN,
        method::EXECUTION_LIST,
        method::EXECUTION_CANCEL,
        method::EXECUTION_STOP,
        method::EXECUTION_DISMISS,
        method::TASK_RETRY,
        method::ACTION_LIST,
        method::CONFIRM_LIST,
        method::SESSION_WATCH,
        method::SESSION_UNWATCH,
        method::SESSION_RECOMPILE,
        method::SESSION_NUDGE,
        method::SESSION_MODE,
        method::SESSION_PROFILE,
        method::FILES_CHANGES,
        method::FILES_RESTORE,
        method::CATALOG_LIST,
        method::COMPILATION_LIST,
        method::NODE_LIST,
        method::NODE_REACH,
        method::TOOL_LIST,
        method::TOOL_DEFER,
        method::SHUTDOWN,
        method::NARRATIVE_WATCH,
        method::NARRATIVE_UNWATCH,
        method::ONTOLOGY_LIST,
        method::HANDS_LIST,
        method::BUDGET_LIST,
        method::POLICY_EXPLAIN,
        method::ONTOLOGY_CATEGORY_ADD,
        method::ONTOLOGY_GUIDANCE_SET,
        method::ONTOLOGY_MEMBERSHIP_SET,
        method::ONTOLOGY_PROPOSALS,
        method::ONTOLOGY_PROPOSAL_ACCEPT,
        method::THREAD_NEW,
        method::THREAD_LIST,
        method::THREAD_SHOW,
        method::THREAD_EDIT,
        method::THREAD_PAUSE,
        method::THREAD_RESUME,
        method::THREAD_DONE,
        method::THREAD_REOPEN,
        method::THREAD_IMPORT,
        method::CARD_PROPOSE,
        method::CARD_LIST,
        method::CARD_ANSWER,
        method::CARD_DONE,
        method::CARD_OBSERVED,
        method::THREAD_SEAL,
        method::THREAD_UNSEAL,
        method::THREAD_SEAL_DISMISS,
        method::THREAD_ANSWER,
        method::THREAD_MOVE,
        method::THREAD_MERGE,
        method::THREAD_UNMERGE,
        method::THREAD_SPLIT,
        method::THREAD_SPLIT_PLAN,
        method::THREAD_UNSPLIT,
        method::THREAD_LINK,
        method::THREAD_UNLINK,
        method::THREAD_UNCLAIM,
        method::PREFIX_REPORT,
        method::PERSONA_LIST,
        method::PERSONA_IMPORT,
        method::PERSONA_CARD_CONFIRM,
        method::PERSONA_ADD,
        method::PERSONA_SOUL,
        method::PERSONA_SOUL_READ,
        method::PERSONA_SET,
        method::PERSONA_RETIRE,
        method::PERSONA_DEFAULT,
        method::INITIATIVE_RATE,
        method::INITIATIVE_WEEK,
        method::VOICE_SHOW,
        method::VOICE_AUDITION,
        method::STANDING_LIST,
        method::STANDING_ADD,
        method::STANDING_REVOKE,
        method::STANDING_PROPOSALS,
        method::STANDING_KEEP,
        method::STANDING_DISMISS,
        method::STANDING_ANSWER,
        method::ONTOLOGY_UNDO,
        method::ONTOLOGY_KIND_ADD,
        method::ONTOLOGY_KIND_RETIRE,
        method::JUDGE_LATENCY,
        method::ONTOLOGY_PROPOSAL_REJECT,
        method::ONTOLOGY_PROPOSAL_ACCEPT_ALL,
        method::ONTOLOGY_PERSON_MERGE,
        method::ONTOLOGY_PROPOSAL_REJECT_ALL,
        method::ONTOLOGY_PROPOSAL_NEVER,
        method::ONTOLOGY_NEVER_LIST,
        method::ONTOLOGY_NEVER_UNDO,
        method::ONTOLOGY_PERSON_NOT_SAME,
        method::ONTOLOGY_PERSON_NOT_SAME_LIST,
        method::ONTOLOGY_PERSON_CANDIDATES,
        method::FORECAST_CALIBRATION,
        method::TASK_CANCEL,
        method::TASK_PAUSE,
        method::TASK_RESUME,
        method::TASK_REHOME,
        method::TASK_NOT_THIS,
        method::TASK_RESCHEDULE,
        method::WAKE_CANCEL,
        method::WAKE_CREATE,
        method::WAKE_SNOOZE,
        method::WAKE_PAUSE,
        method::WAKE_RESUME,
        method::WAKE_DONE,
        method::ROUTINE_CREATE,
        method::ROUTINE_LIST,
        method::ROUTINE_RUNS,
        method::ROUTINE_PAUSE,
        method::ROUTINE_RESUME,
        method::ROUTINE_RUN_NOW,
        method::ROUTINE_SKIP,
        method::ROUTINE_TRY,
        method::ROUTINE_UPDATE,
        method::ROUTINE_DELETE,
        method::WATCH_CREATE,
        method::WATCH_LIST,
        method::PROJECT_CREATE,
        method::PROJECT_LIST,
        method::PROJECT_SHOW,
        method::PROJECT_ATTACH,
        method::PROJECT_TRUST,
        method::PROJECT_UNTRUST,
        method::PROJECT_TRUSTED,
        method::EXECUTIONS_WATCH,
        method::EXECUTIONS_UNWATCH,
        method::WORK_WATCH,
        method::WORK_UNWATCH,
        method::WORK_AGENDA,
        method::WORK_DIGEST,
        method::WORK_PEEK,
        method::JOB_STOP,
        method::ACTION_OUTPUT,
        method::ACTION_UNFOLLOW,
        method::STATUS_SHORT,
        method::INDEX_STATUS,
        method::INDEX_QUERY,
        method::INDEX_REBUILD,
        method::MEMORY_SEARCH,
        method::MEMORY_RECALLS,
        method::MEMORY_CITES,
        method::MEMORY_LABEL,
        method::MEMORY_CONSOLIDATE,
        method::FACT_RETRACT,
        method::TURN_REPLAY,
        method::ROUTE_REPORT,
        method::JUDGE_LIST,
        method::JUDGE_GET,
        method::JUDGE_LABEL,
        method::LEARNING_REPORT,
        method::ROUTE_CORRECT,
        method::ROUTE_CORRECTIONS,
        method::JUDGE_REPLAY,
        method::JUDGE_AUDIT,
        method::JUDGE_BACKFILL,
        method::JUDGE_LEARN,
        method::JUDGE_PROVE,
        method::PACK_LIST,
        method::PACK_PROMOTE,
        method::PACK_ROLLBACK,
        method::IMPORT_EPISODES,
        method::IMPORT_ERASE,
        method::IMPORT_LIST,
        method::IMPORT_SESSIONS,
        method::CONTEXT_EXPLAIN,
        method::CONTEXT_CENSUS,
        method::TURN_INSPECT,
        method::BOOKS_LIST,
        method::BOOKS_PAGE,
        method::IMPORT_TOPICS,
        method::IMPORT_PEOPLE,
        method::IMPORT_SYNC_PLAN,
        method::IMPORT_SYNC_RECORD,
        method::BENCH_HISTORY,
        method::SANDBOX_USAGE,
        method::AWS_BOOTSTRAP,
        method::AWS_CONFIRM_ALERTS,
        method::MCP_LIST,
        method::MCP_RESTART,
        method::MCP_PROMPT_LIST,
        method::EXTEND_LIST,
        method::EXTENSION_REVOKE,
        method::SELF_HALT,
        method::SELF_RESUME,
        method::SELF_LOG,
        method::SELF_DIGEST,
        method::SELF_BACKLOG,
        method::SELF_DROP,
        method::SELF_MOVE,
        method::SELF_RETRY,
        method::SELF_FILE,
        method::SELF_BRANCHES,
        method::SELF_EXAMS,
        method::SELF_REVERT,
        method::SELF_PIPELINE,
        method::SELF_PATTERNS,
        method::SELF_NARROW,
        method::GOOGLE_AUTH_BEGIN,
        method::GOOGLE_AUTH_FINISH,
        method::GOOGLE_REVOKE,
        method::GOOGLE_STATUS,
        method::CALENDAR_WINDOW,
    ];

    /// The one method `allowed` admits without naming it on a list: a fork
    /// of a conversation the client itself opened, which
    /// `Core::fork_allowed` holds the rest of.
    const FORK: &[&str] = &[method::SESSION_FORK];

    /// Each list, with what it says about its entries, for the failure.
    fn classified() -> Vec<(String, String)> {
        let say = |names: &[&str], what: &'static str| {
            names
                .iter()
                .map(|m| ((*m).to_string(), what.to_string()))
                .collect::<Vec<_>>()
        };
        [
            say(&ALLOWED, "ALLOWED: the MCP server's tools call it"),
            say(&JUDGED, "JUDGED: an owner's act, refused at its judgment"),
            say(
                FORK,
                "allowed() admits it by name: a fork of its own session",
            ),
            say(NOT_FOR_MCP, "NOT_FOR_MCP: refused, deliberately"),
        ]
        .concat()
    }

    /// The three lists and `method::ALL` are the same set, so no method is
    /// unclassified and no list names one that is gone (theseus-a7pr).
    #[test]
    fn the_three_lists_classify_every_method() {
        // The positive bar: `ALL` is built from the same table as the
        // constants, so a walk that found a handful read the wrong thing.
        let spec = crate::complete::MustBeComplete {
            item: "a protocol method, classified for the MCP surface",
            list: "mcp_server's ALLOWED, JUDGED and tests::NOT_FOR_MCP",
            world: "theseus_protocol::method::ALL",
            add: "a method was added to the protocol and nobody said what the \
                  MCP server may do with it. Put it in ALLOWED (a tool calls \
                  it), in JUDGED (it is an owner's act), or in NOT_FOR_MCP \
                  (everything else). A method that returns a session's \
                  content and goes in ALLOWED must take the asking surface \
                  first: docs/design/imported-surface.md says what that means",
            drop: "this method is gone from theseus_protocol::method, or was \
                   renamed: delete its row from the list that still names it",
            least: 200,
            stale_fails: true,
        };
        let walked = crate::complete::check(
            &spec,
            classified(),
            method::ALL
                .iter()
                .map(|m| ((*m).to_string(), "the method table".to_string())),
        );
        assert!(walked >= 200, "{walked} methods walked");
    }

    /// They partition it: no method is on two lists, so a name moved out of
    /// NOT_FOR_MCP and left behind cannot read as classified twice.
    #[test]
    fn no_method_is_classified_twice() {
        let mut seen = std::collections::BTreeMap::new();
        let mut twice = Vec::new();
        for (m, what) in classified() {
            if let Some(first) = seen.insert(m.clone(), what.clone()) {
                twice.push(format!("  {m}\n    {first}\n    {what}"));
            }
        }
        assert!(
            twice.is_empty(),
            "{} of a protocol method classified twice; each belongs to exactly \
             one of ALLOWED, JUDGED and NOT_FOR_MCP:\n{}",
            twice.len(),
            twice.join("\n"),
        );
        assert!(seen.len() >= 200, "{} classified", seen.len());
    }

    /// `allowed` answers from the lists and nothing else: every ALLOWED and
    /// JUDGED entry passes, and every NOT_FOR_MCP one is refused.
    #[test]
    fn allowed_says_yes_to_each_list_and_no_to_the_rest() {
        let yes: Vec<&str> = [ALLOWED.as_slice(), JUDGED.as_slice(), FORK].concat();
        assert_eq!(yes.len(), 16, "the methods an MCP connection may call");
        for m in &yes {
            assert!(allowed(m), "{m} is listed and allowed() refuses it");
        }
        for m in NOT_FOR_MCP {
            assert!(!allowed(m), "{m} is NOT_FOR_MCP and allowed() admits it");
        }
    }

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
            outside: vec![],
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
