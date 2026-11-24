//! Looking is not asking (issue 26): Theseus reads itself, and the names its
//! vault holds, with a tool of its own, so the owner's turn has no reason to
//! reach for the shell.
//!
//! Asked whether a model's profile was set up and whether its key was in the
//! vault, a turn ran Theseus's own command line and the vault's program in the
//! shell. The floor holds a shell command that names either, at every setting
//! (`policy.rs`, `runs_program`), so each was a card the owner had to press;
//! and the vault's program in a shell has no session by design (the daemon's
//! token is never given to a command a turn runs), so it could not have
//! answered. The floor is as it was. These tools are the way to look:
//!
//! - **`look.theseus`** (`look/theseus.rs`): the profiles and the live one,
//!   each provider and whether its key resolved, the daemon's health, the
//!   build. No secret's value.
//! - **`look.vault`** (`look/vault.rs`, `secrets/look.rs`): who the daemon's
//!   token is, the vaults, the titles of the items, one item's field names and
//!   kinds, and which setting reads which item. Never a value or a note.
//! - **`look.work`** (`look/work.rs`, issue 28): Theseus's work whole: what
//!   runs, every conversation's tasks, what waits on the owner, projects,
//!   routines and what is coming, this conversation's first; and this
//!   conversation's last messages, never another's.
//!
//! **The wall** is `config.show`'s: the owner's own turn, in a private place
//! only they write in (`config_change::wall`; a job, a task, the self loop, a
//! session an MCP client opened and a shared place are refused in their own
//! words), told and not asked in a clean turn, and **asked first** when the
//! turn has read text from outside since the owner's last message: a turn that
//! was tricked must not list the vault's names, or map what Theseus runs on,
//! unasked ([`ask_if_outside`]).
//!
//! The whole design, and why no value can reach a turn: `docs/design/look-tools.md`.

use std::sync::{Arc, OnceLock, Weak};

use serde_json::{json, Value};
use theseus_protocol::Plan;
use theseus_tools::{
    AsyncMediaRun, Backend, Registry, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

use crate::config_change::{outside_text, wall};
use crate::secrets::OpReader;
use crate::toolrun::TurnCtx;
use crate::Core;

mod theseus;
mod vault;
pub(crate) mod view;
mod work;

pub(crate) use vault::vault_named;

pub const THESEUS: &str = "look.theseus";
pub const VAULT: &str = "look.vault";
pub const WORK: &str = "look.work";
/// The family the tools' names give them, and the deferred family's name.
pub const FAMILY: &str = "look";
/// The tools' names, as the config's `[policy.tools]` checks them.
pub const NAMES: [&str; 3] = [THESEUS, VAULT, WORK];

/// Register the three tools, in a daemon that was started from a config: a file
/// (`config_file`) or the vault's note (`config_copy`), either of which a start
/// sets and no TOML can. A default test config has neither, so no request a
/// test builds carries them, as it carries none of the settings tools
/// (`config_change::register`): the tests that tune a window to the size of the
/// tools block, and the ones that bound a request's estimate, are not moved by
/// a tool a real daemon always has. The gate and the wall decide who may call
/// them.
pub fn register(cfg: &crate::Config, r: &mut Registry) {
    if cfg.config_file.is_some() || cfg.config_copy.is_some() {
        r.register(Arc::new(LookTheseus));
        r.register(Arc::new(LookVault));
        r.register(Arc::new(LookWork));
    }
}

/// What each tool is about, in the words a refusal and a card use.
fn about(tool: &str) -> &'static str {
    match tool {
        VAULT => "read the names in Theseus's vault",
        WORK => "read what work Theseus has going: its tasks, questions, projects, routines and reminders",
        _ => "look at what Theseus runs as",
    }
}

/// Why this call may not look, or `None` when it may: the wall `config.show`
/// keeps, in this tool's own words.
pub(crate) fn refusal(tc: &TurnCtx<'_>, tool: &str) -> Option<String> {
    worded(wall::refusal(tc), tool)
}

/// The wall's refusal, in this tool's own words.
fn worded(why: Option<&'static str>, tool: &str) -> Option<String> {
    let what = about(tool);
    Some(match why? {
        w if w == wall::NOT_PRIVATE => format!(
            "{tool} can {what}, so it is only for the owner's own private place: it is not offered in a shared one"
        ),
        w if w == wall::NOT_ALONE => format!(
            "{tool} can {what}, so it is for a place only the owner writes in: this one is shared with others"
        ),
        w if w == wall::NOT_A_TASK => format!(
            "{tool} is not for a task's session: a task may not {what}; report what is needed and the owner asks for it"
        ),
        _ => format!(
            "{tool} is only for the owner's own conversation: this session belongs to a job, the self-improvement loop or a connected client, none of which may {what}"
        ),
    })
}

/// The gate's hold on a look that is not free: when the turn has read text
/// from outside since the owner's last message, the call is put to the owner
/// as any call that asks is, whatever its posture.
pub(crate) fn ask_if_outside(tc: &TurnCtx<'_>, name: &str, plan: Plan) -> Plan {
    if !NAMES.contains(&name) {
        return plan;
    }
    let (outside, steered) = outside_text(tc);
    held(name, outside, steered, plan)
}

/// [`ask_if_outside`], on the facts it reads.
fn held(name: &str, outside: bool, steered: bool, mut plan: Plan) -> Plan {
    if NAMES.contains(&name) && (outside || steered) {
        plan.authority = Some(format!(
            "{}: this conversation has read text from outside since your last message, so the \
             request may not be only yours",
            about(name)
        ));
    }
    plan
}

/// `look.theseus`.
pub struct LookTheseus;

impl Tool for LookTheseus {
    fn name(&self) -> &str {
        THESEUS
    }

    fn description(&self) -> &str {
        "Look at Theseus itself: its model profiles and which is live, each provider and whether its key \
         resolved, its health and the build that runs; when asked for (`ontology`), its kinds, categories \
         and pending proposals, and the people it holds by name. Use this for such questions; Theseus's \
         own command line in a shell is not the way, and the floor holds it. Never a key's value. Changes \
         nothing. Offered only to the owner, in a private place."
    }

    fn input_schema(&self) -> Value {
        theseus::schema()
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Async
    }

    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let parts = theseus::parts_of(input)?;
        Ok(Plan {
            summary: format!("look at Theseus: {}", parts.join(", ")),
            ..Default::default()
        })
    }

    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "look.theseus runs through the runtime, which reads Theseus",
        ))))
    }
}

/// `look.vault`.
pub struct LookVault;

impl Tool for LookVault {
    fn name(&self) -> &str {
        VAULT
    }

    fn description(&self) -> &str {
        "Look at what Theseus's vault holds, by name: the account the daemon's token is, the vaults, the \
         titles of the items (a vault and part of a title narrow it; the cap is said), for one item (`fields_of`) \
         the names and kinds of its fields, and which setting reads which item, resolved or not. The daemon \
         asks, with its own token. Never a value, a field or a note; for one item (`fields_of`) only the names \
         and kinds of its fields. A secret is used by its name (`vault.use`), never asked for or shown. The vault's program in a shell has no session by design and is not to be \
         tried; neither is Theseus's own command line. Changes nothing. Offered only to the owner, in a \
         private place."
    }

    fn input_schema(&self) -> Value {
        vault::schema()
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Async
    }

    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let ask = vault::ask_of(input)?;
        Ok(Plan {
            summary: format!("read the vault's names: {}", ask.summary()),
            ..Default::default()
        })
    }

    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "look.vault runs through the runtime, which asks the vault",
        ))))
    }
}

/// `look.work`.
pub struct LookWork;

impl Tool for LookWork {
    fn name(&self) -> &str {
        WORK
    }

    fn description(&self) -> &str {
        "Theseus's work, whole: what runs, tasks, what waits on the owner, projects, routines, reminders; `scope: \
         here` for this conversation's. Messages: this one's only (another's: `theseus history \
         SESSION`). Owner only, private place."
    }

    fn input_schema(&self) -> Value {
        work::schema()
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Async
    }

    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let ask = work::ask_of(input)?;
        Ok(Plan {
            summary: format!("look at Theseus's work: {}", ask.summary()),
            ..Default::default()
        })
    }

    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "look.work runs through the runtime, which reads Theseus's work",
        ))))
    }
}

/// What a look gives the model: its words, and which look it was.
fn output(text: String, look: &str) -> ToolOutput {
    ToolOutput {
        text,
        meta: json!({ "look": look }),
    }
}

/// What the tools read: the core, and the daemon's vault reader, each given
/// once it exists.
#[derive(Default)]
pub struct Board {
    core: OnceLock<Weak<Core>>,
    vault: OnceLock<Arc<OpReader>>,
}

impl Board {
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    /// The daemon's own reader of the vault, token and all. A turn never has
    /// it: it is read here, in the daemon, for the look alone.
    pub fn set_vault(&self, op: Arc<OpReader>) {
        let _ = self.vault.set(op);
    }

    /// The daemon's reader of the vault, for the one other thing that checks an
    /// item before it names it (`vault.use`): the daemon's own code,
    /// never a turn.
    pub(crate) fn reader(&self) -> Option<Arc<OpReader>> {
        self.vault.get().cloned()
    }

    fn core(&self) -> Option<Arc<Core>> {
        self.core.get().and_then(Weak::upgrade)
    }

    /// The call, in its own task, behind the wall.
    pub fn run(&self, tc: &TurnCtx<'_>, name: &str, input: &Value) -> AsyncMediaRun {
        let fail = |why: String| -> AsyncMediaRun {
            Box::pin(std::future::ready(Err(ToolFailure::new(why))))
        };
        if let Some(why) = refusal(tc, name) {
            return fail(why);
        }
        let Some(core) = self.core() else {
            return fail(format!("{name} is not ready: the core is not built"));
        };
        let target = tc.target.cloned();
        match name {
            THESEUS => {
                let parts = match theseus::parts_of(input) {
                    Ok(p) => p,
                    Err(e) => return fail(e),
                };
                let (session, turn) = (tc.session_id.to_string(), tc.turn_id.to_string());
                Box::pin(async move {
                    let mut said = theseus::words(&core, target.as_ref(), &parts);
                    if parts.contains(&"view") {
                        // Its proof walks the store: on the blocking pool, as the owner's `view.show`.
                        let read = tokio::task::spawn_blocking(move || {
                            view::words(&core, &session, &turn)
                        });
                        said.push_str(
                            &read.await.unwrap_or_else(|e| {
                                format!("this turn's view cannot be read: {e}\n")
                            }),
                        );
                    }
                    Ok((output(said, THESEUS), None, None))
                })
            }
            WORK => {
                let ask = match work::ask_of(input) {
                    Ok(a) => a,
                    Err(e) => return fail(e),
                };
                let (session, turn) = (tc.session_id.to_string(), tc.turn_id.to_string());
                // One snapshot of the values the runtime's scrubber knows, for every cut of this call.
                let cuts = tc.scrubber.cuts();
                Box::pin(async move {
                    let who = work::Caller {
                        session: &session,
                        turn: &turn,
                    };
                    work::words(&core, &who, &ask, &cuts)
                        .await
                        .map(|(said, shown)| {
                            // The messages it shows: its result's edges to them (`copies`).
                            let mut o = output(said, WORK);
                            o.meta[crate::copies::KEY] = json!(shown);
                            (o, None, None)
                        })
                        .map_err(ToolFailure::new)
                })
            }
            _ => {
                let ask = match vault::ask_of(input) {
                    Ok(a) => a,
                    Err(e) => return fail(e),
                };
                let op = self.vault.get().cloned();
                Box::pin(async move {
                    vault::run(core, op, ask)
                        .await
                        .map(|said| (output(said, VAULT), None, None))
                        .map_err(ToolFailure::new)
                })
            }
        }
    }
}

#[cfg(test)]
mod tests;
