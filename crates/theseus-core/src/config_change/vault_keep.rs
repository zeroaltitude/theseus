//! `vault.keep`: the owner's own turn asks to keep a secret in the vault, and
//! the owner gives its value where no turn, card or record sees it (issue 26,
//! line 2; `docs/design/vault-keep.md`).
//!
//! The turn names a vault, an item title, a field label and a one-line purpose,
//! in words. **There is no parameter that carries a value**: a turn that was
//! steered cannot hand one over, and a value typed in chat reaches no tool. The
//! DAEMON finds the target in the vault's own lists with its own token
//! (`target.rs`: new item, a field added, or an existing value REPLACED) and
//! puts ONE card, the settings card (`config.ack`, with a `keep` marker as an
//! MCP server's has `mcp`), which says all of that and where the value is typed.
//!
//! The value is given in a surface's own form, as a request of its own
//! (`vault.keep.answer`, `rpc/vault_keep.rs`): the owner's terminal's hidden
//! prompt, the cockpit's field, and later Discord's form. The daemon writes it
//! with its own token by the path that keeps the Google token
//! (`OpReader::keep_field`: the item on stdin, read back, compared by digest) and
//! tells the owner where it is by name. The question's record holds the target
//! and never the value, answered, expired, withdrawn or not.
//!
//! The wall is `config.propose`'s (`wall.rs`), the outside-text hold is the looks'
//! (`vault_use::ask_if_outside`, which names this tool too), the tool is in
//! `routine::grant::NEVER`, and an item Theseus's own settings read is never
//! replaced by a turn: only from the owner's own terminal.

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_kernel::Proposal;
use theseus_protocol::Plan;
use theseus_tools::{parse, Backend, Retry, Tool, ToolClass, ToolCtx};

use super::{card, outside_text, proof_seam, wall, ACK};
use crate::toolrun::{ToolRuntime, TurnCtx};

pub mod done;
mod settle;
pub mod target;
#[cfg(test)]
mod tests;

use target::Target;

pub const KEEP: &str = "vault.keep";
/// The key of a question's arguments that marks it a keep card, and holds its target.
pub const ARG: &str = "keep";

/// Whether a question's arguments are a keep card's.
pub fn is_card(args: &Value) -> bool {
    args.get(ARG).is_some_and(Value::is_object)
}

/// Whether `a` is a keep card: a settings question that carries a target and
/// takes a value, not a yes.
pub fn is_keep(a: &theseus_kernel::Action) -> bool {
    a.tool == ACK && a.proposal.as_ref().is_some_and(|p| is_card(&p.args))
}

/// What a plain Approve on a keep card is told: there is no value in it.
pub const NO_VALUE: &str = "this is a secret to keep, and a yes carries no value: type the value where only you \
     see it, in your own terminal with `theseus vault keep` or in the cockpit's field on this card. \
     Nothing was kept";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    vault: String,
    title: String,
    field: String,
    purpose: String,
}

/// The card, whole: what is kept where, whether it replaces, where the value is
/// typed, and what a value typed in chat comes to.
pub fn question(t: &Target, outside: bool, steered: bool, purpose: &str) -> String {
    use target::State;
    let what = match t.state {
        State::New => "a new item",
        State::Adds => "a new field in an item that is already there",
        State::Replaces => "the field that is already there",
    };
    let mut out = format!(
        "Keep a secret in the vault?\nIn the vault \"{}\": item \"{}\", field \"{}\" ({what}).\n",
        t.vault, t.title, t.field
    );
    if t.replaces() {
        out.push_str(
            "This REPLACES the value stored there. Theseus cannot show the old one and does not \
             keep it (the vault keeps its own history of the item).\n",
        );
    }
    out.push_str(
        "You give the value yourself, where only you see it; it never goes to the model, to this \
         card or to a record:\n\
         - in your own terminal: run `theseus vault keep` (the value is typed with nothing shown)\n\
         - or in the cockpit, in this card's own field.\n\
         Do not paste it into chat: a value typed in chat is already with the model's provider \
         and in Theseus's record, and Theseus cannot take it back (rotate it at its provider, \
         then give the new one here).\n",
    );
    out.push_str(match t.replaces() {
        true => "It needs a fresh code from your authenticator app, every time.\n",
        false => {
            "It needs your second proof (a code from your authenticator app, unless one \
                  given here earlier still covers this place).\n"
        }
    });
    out.push_str(&card::care_lines(outside, steered));
    let why = card::reason_line(purpose);
    if !why.is_empty() {
        out.push_str(&format!("Theseus's reason, in its own words: \"{why}\"\n"));
    }
    out
}

/// The line a keep is settled with.
pub fn settled(t: &Target) -> String {
    format!(
        "keep a secret in the vault \"{}\": item \"{}\", field \"{}\"",
        t.vault, t.title, t.field
    )
}

/// Run `vault.keep` for the call `correlation_id` of the turn `tc`. An error is
/// the result the model reads.
pub async fn run(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    if let Some(why) = wall::refusal_for(tc, KEEP) {
        return Err(why);
    }
    // Run again after a restart, it answers with what the first run asked.
    if let Ok(done) = super::proposed_by_call(tc, correlation_id) {
        return Ok(done);
    }
    let i: Input = parse(input)?;
    let purpose = card::reason_line(&i.purpose);
    if purpose.is_empty() {
        return Err("give a purpose in the owner's words, one line".into());
    }
    let Some(reader) = rt.look.reader().filter(|r| !r.has_no_vault()) else {
        return Err(
            "this Theseus has no vault session (its secrets come from the environment or from \
             files), so it cannot keep a secret in the vault"
                .into(),
        );
    };
    let core = rt
        .config_change
        .core()
        .ok_or("Theseus is not ready to keep a secret: try again in a moment")?;
    let t = target::resolve(
        &reader,
        &target::readers(&core),
        (&i.vault, &i.title, &i.field),
    )
    .await?;
    if t.protected {
        return Err(format!(
            "the item \"{}\" in \"{}\" is one Theseus's own settings read, so a turn does not \
             replace it. The owner does that from their own terminal: `theseus vault keep \
             --vault \"{}\" --item \"{}\" --field \"{}\"`",
            t.title, t.vault, t.vault, t.title, t.field
        ));
    }
    let needs = match t.replaces() {
        true => proof_seam::Needs::Fresh,
        false => proof_seam::Needs::Proof,
    };
    let (outside, steered) = outside_text(tc);
    let q = question(&t, outside, steered, &purpose);
    let proposal = Proposal {
        tool: ACK.into(),
        args: json!({
            "question": q,
            "settled": settled(&t),
            ARG: t.args(),
            "needs_fresh_proof": t.replaces(),
            "reason": purpose,
            "outside_text": outside || steered,
            "asked_by": {
                "session_id": tc.session_id,
                "execution_id": tc.execution_id,
                "turn_id": tc.turn_id,
                "place": tc.outbox.target(tc.session_id),
            },
        }),
        resource: Some("config".into()),
        policy_context: json!({}),
    };
    let said = format!(
        "Asked the owner:\n{q}Nothing is kept until they give the value on the card and a code. \
         You never see the value. When it is kept they are told, by name; then, if they want it \
         used, put it to use with `vault.use` (a second card).",
    );
    super::ask(rt, tc, proposal, correlation_id, &said, needs)?;
    Ok((said, json!({"keep": t.state.as_str()})))
}

/// `vault.keep`.
pub struct VaultKeep;

impl Tool for VaultKeep {
    fn name(&self) -> &str {
        KEEP
    }

    fn description(&self) -> &str {
        "Ask the owner to keep a NEW secret in Theseus's vault: name the vault, the item's title, the field's label \
         and a one-line purpose. There is no value to give and none is ever asked of you: the owner types it into \
         the card's own field or at a hidden prompt in their terminal, and Theseus writes it with its own account. \
         If someone pastes a secret into chat, say it is now with the provider and in Theseus's record, to be \
         rotated, and ask for the new one by this tool. Shows ONE card that says whether it makes an item, adds a \
         field or REPLACES a value (a replace needs a fresh code); an item Theseus's own settings read is not \
         replaced by a turn. When it is kept, use it by its name with `vault.use`. Offered only to the owner, in a \
         private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "vault": {"type": "string", "description": "The vault's name or id, as `look.vault` lists it."},
                "title": {"type": "string", "description": "The item's title (letters, digits, spaces, - _ .)."},
                "field": {"type": "string", "description": "The field's label, e.g. `credential`."},
                "purpose": {"type": "string", "description": "One line, in the owner's words: what the secret is for."}
            },
            "required": ["vault", "title", "field", "purpose"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Run
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // Run again after a restart, it answers with what it asked.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i: Input = parse(input)?;
        Ok(Plan {
            summary: format!(
                "ask the owner to keep a secret as {} in the vault",
                crate::secrets::look::clean(&i.title)
            ),
            ..Default::default()
        })
    }
}
