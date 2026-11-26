//! Signing in to an MCP server from Discord, with no one at the machine (issue 59). A child of
//! `runtime`, on the shared pattern (`consent.rs`): every step is claimed once and passes the
//! owner-in-private judge first, and every step after Sign in needs this binding's record of it.
//!
//! A turn can only **offer** the control: `mcp.sign_in_offer` posts one `mcp_sign_in` card to the
//! owner's direct messages, with **Sign in** and **Sign out** for the named server.
//!
//! - **Sign in** (`mcpin:in:<server>`) is `mcp.login.begin` with `manual` (the paste, as `theseus mcp
//!   login --manual` takes it), and begins the record. The presser alone is told the host he signs in
//!   at and every other place the sign-in goes through, whole; when there is one, an **I agree**
//!   press (`mcpin:agree:<id>`) comes first, exactly where the CLI asks for its typed yes, and only
//!   that user's own press of it moves the record to agreed. Then the address to open, as a link,
//!   and **Paste the address** (`mcpin:paste:<id>`).
//! - **Paste the address** opens a modal with one field: the whole address the browser ended on. Its
//!   submit is `mcp.login.finish { pasted, agreed }`, the daemon's own manual finish, with `agreed`
//!   the record's.
//! - **Sign out** (`mcpin:out:<server>`) is `mcp.logout`, and retires the server's record.
//!
//! **The pasted value passes through Discord to the daemon and is kept nowhere.** It is read off
//! the modal into the finish's params and handed to the request; it is in no message, no log line,
//! no row and no reply. Every answer is fixed words.

use std::time::Duration;

use serde_json::Value;
use theseus_protocol::mcp::{
    McpLoginBeginParams, McpLoginBeginResult, McpLoginFinishParams, McpLoginFinishResult,
    McpLogoutParams, McpLogoutResult,
};
use theseus_protocol::{error_code, method};
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ButtonStyle, Component};

#[cfg(test)]
use super::consent::FIELD;
use super::consent::{self, address_words, button, row, word_ok, Stage, Why};
use super::Shared;
use crate::rpc_client::CallError;

const PREFIX: &str = "mcpin";
/// The modal's custom id: `mcpin:m:<login id>`.
pub(crate) const MODAL: &str = "mcpin:m:";
const WHAT: &str = "mcp sign-in reply";

/// The fixed words a press is answered with.
pub(crate) const NOT_HERE: &str =
    "🔐 Only you can sign in to an MCP server, and only in a private place: \
     your direct messages with Theseus, or a channel bound private. Nothing began.";
pub(crate) const NOT_YOURS: &str =
    "🔐 That sign-in was begun by someone else, so it goes no further here. Nothing was sent.";
pub(crate) const EXPIRED: &str =
    "🔐 That sign-in is no longer open: it ran out, a newer one began, or \
     Theseus restarted. Press **Sign in** again.";
pub(crate) const NOT_THIS: &str =
    "🔐 That is not the address of this sign-in. Press **Sign in** again, \
     and paste the whole address your browser ends on.";
pub(crate) const REFUSED: &str = "🔐 The sign-in was refused, and nothing was kept.";
pub(crate) const SERVER_NO: &str =
    "🔐 The server said no, and nothing was kept. Press **Sign in** to try again.";
pub(crate) const MAY_BE_KEPT: &str =
    "🔐 The sign-in may have been kept, but Theseus could not make sure of it, so it is not used \
     now. Check the server's line in `theseus mcp`, or press **Sign in** again.";
pub(crate) const NO_BEGIN: &str = "🔐 The sign-in could not begin. Nothing was kept.";
pub(crate) const TOO_LONG: &str =
    "🔐 This sign-in's address is too long to show whole here, so it is not shown: sign in at \
     the terminal with `theseus mcp login`. Nothing was kept.";
pub(crate) const PASTE_TOO_LONG: &str =
    "🔐 That is longer than the form takes (4,000 characters), so nothing was sent. Sign in at \
     the terminal with `theseus mcp login`.";
pub(crate) const SIGNED_OUT: &str = "🔐 Signed out: its sign-in is forgotten here.";
pub(crate) const KEPT_REMAINS: &str =
    "🔐 Signed out for now, but its kept sign-in could not be removed: Theseus would use it again \
     when it next starts. Press **Sign out** again, or run `theseus mcp logout` at the terminal.";
pub(crate) const NOTHING_KEPT: &str = "🔐 It had no sign-in kept.";
pub(crate) const NO_SIGN_OUT: &str = "🔐 The sign-out could not be done.";

/// A press of a control of this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Press {
    In(String),
    Out(String),
    Agree(String),
    Paste(String),
}

pub(crate) fn parse(custom_id: &str) -> Option<Press> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    let (verb, word) = rest.split_once(':')?;
    let press = match verb {
        "in" => Press::In(word.into()),
        "out" => Press::Out(word.into()),
        "agree" => Press::Agree(word.into()),
        "paste" => Press::Paste(word.into()),
        _ => return None,
    };
    let w = match &press {
        Press::In(w) | Press::Out(w) | Press::Agree(w) | Press::Paste(w) => w,
    };
    word_ok(w).then_some(press)
}

/// The server the core's `mcp_sign_in` card is for.
pub(crate) fn server_of(body: &Value) -> String {
    body["server"].as_str().unwrap_or_default().to_string()
}

/// The card's two buttons.
pub(crate) fn buttons(server: &str) -> Vec<Component> {
    if !word_ok(server) {
        return vec![];
    }
    row(vec![
        button(
            format!("{PREFIX}:in:{server}"),
            "Sign in",
            ButtonStyle::Primary,
        ),
        button(
            format!("{PREFIX}:out:{server}"),
            "Sign out",
            ButtonStyle::Secondary,
        ),
    ])
}

/// The modal "Paste the address" opens: one field.
pub(crate) fn modal(id: &str) -> Value {
    consent::modal(PREFIX, id, "Finish the sign-in")
}

/// What the presser is shown first, line by line: the host, and every other place it goes through.
pub(crate) fn host_lines(b: &McpLoginBeginResult) -> Vec<String> {
    let mut t = vec![format!("🔐 You will sign in at `{}`.", b.host)];
    for h in &b.elsewhere {
        t.push(format!(
            "Part of the sign-in goes through `{h}`, which is not its issuer's own address."
        ));
    }
    if !b.elsewhere.is_empty() {
        t.push("Press **I agree** to go on; nothing is sent until you do.".into());
    }
    t
}

/// A failure, in fixed words: never any part of what the daemon or the presser said. Chosen by how
/// the daemon's words begin, or by fixed words of its own.
pub(crate) fn failure(e: &CallError) -> &'static str {
    if e.code == error_code::REFUSED {
        return NOT_HERE;
    }
    let m = e.message.as_str();
    if m.starts_with("the sign-in was not made sure to last") {
        MAY_BE_KEPT
    } else if m.starts_with("no sign-in is open") || m.contains("the redirect was taken") {
        EXPIRED
    } else if m.contains("not this sign-in's")
        || m.contains("the address")
        || m.contains("redirect")
    {
        NOT_THIS
    } else if m.starts_with("the sign-in was not completed") {
        SERVER_NO
    } else {
        REFUSED
    }
}

/// What a sign-out says: forgotten, still kept on disk, or nothing to forget.
pub(crate) fn signed_out(out: &McpLogoutResult) -> &'static str {
    if out.kept_remains {
        KEPT_REMAINS
    } else if out.forgot {
        SIGNED_OUT
    } else {
        NOTHING_KEPT
    }
}

/// A step refused before any request, in fixed words.
fn refused(why: Why) -> &'static str {
    match why {
        Why::NotYours => NOT_YOURS,
        Why::Missing | Why::Stale | Why::Stage => EXPIRED,
    }
}

impl Shared {
    /// A press of this module's controls.
    pub(super) async fn mcp_sign_in_press(&self, i: &Interaction, press: Press, who: &str) {
        let Some((user, origin)) = self.consent_gate(i, WHAT, NOT_HERE).await else {
            return;
        };
        match press {
            Press::In(server) => self.mcp_begin(i, &server, who, user, origin).await,
            Press::Agree(id) => {
                let ok = |r: &consent::Record| r.stage == Stage::Begun && r.needs_agree;
                match self
                    .consents
                    .step(PREFIX, &id, Some((user, origin.channel_id.as_str())), ok)
                {
                    Ok(r) => {
                        self.consents.set(PREFIX, &id, Stage::Agreed);
                        let words = address_words(&r.url).unwrap_or_else(|| TOO_LONG.into());
                        self.consent_reply(i, WHAT, &words, consent::paste_button(PREFIX, &id))
                            .await;
                    }
                    Err(why) => self.consent_reply(i, WHAT, refused(why), vec![]).await,
                }
            }
            Press::Paste(id) => match self.consents.step(
                PREFIX,
                &id,
                Some((user, origin.channel_id.as_str())),
                |r| r.ready(),
            ) {
                Ok(_) => self.respond_modal(i, &modal(&id)).await,
                Err(why) => self.consent_reply(i, WHAT, refused(why), vec![]).await,
            },
            Press::Out(server) => self.mcp_logout(i, &server, who, origin).await,
        }
    }

    /// **Sign in**: the daemon's manual begin, as the presser; the record begun.
    async fn mcp_begin(
        &self,
        i: &Interaction,
        server: &str,
        who: &str,
        user: u64,
        origin: theseus_protocol::DiscordOrigin,
    ) {
        if !self.consent_ack(i, WHAT).await {
            return;
        }
        let params = McpLoginBeginParams {
            name: server.to_string(),
            manual: true,
            author: Some(who.to_string()),
            discord: Some(origin.clone()),
        };
        consent::sending(method::MCP_LOGIN_BEGIN);
        let r = self
            .rpc
            .call::<_, McpLoginBeginResult>(method::MCP_LOGIN_BEGIN, params)
            .await;
        let b = match r {
            Ok(b) => b,
            Err(e) => {
                let words = match e.code == error_code::REFUSED {
                    true => NOT_HERE,
                    false => NO_BEGIN,
                };
                return self.consent_finish(i, WHAT, words, &[]).await;
            }
        };
        let (Some(address), Some(mut shown)) =
            (address_words(&b.url), consent::pages(&host_lines(&b)))
        else {
            return self.consent_finish(i, WHAT, TOO_LONG, &[]).await;
        };
        let needs_agree = !b.elsewhere.is_empty();
        let lives = Duration::from_secs(b.expires_secs);
        let rec = consent::record(
            &self.consents,
            PREFIX,
            &b.login_id,
            user,
            server,
            origin,
            needs_agree,
            lives,
            &b.url,
        );
        self.consents.begin(rec);
        let controls = if needs_agree {
            row(vec![button(
                format!("{PREFIX}:agree:{}", b.login_id),
                "I agree",
                ButtonStyle::Primary,
            )])
        } else {
            shown.push(address);
            consent::paste_button(PREFIX, &b.login_id)
        };
        self.consent_pages(i, WHAT, &shown, &controls).await;
    }

    /// The form's submit: the daemon's manual finish, as the presser, with what the record says was
    /// agreed. The field goes into the request and nowhere else.
    pub(super) async fn mcp_sign_in_modal(
        &self,
        i: &Interaction,
        m: &ModalInteractionData,
        who: &str,
    ) {
        let Some((user, origin)) = self.consent_gate(i, WHAT, NOT_HERE).await else {
            return;
        };
        let Some(login_id) = consent::submitted(m, PREFIX) else {
            return self.consent_reply(i, WHAT, EXPIRED, vec![]).await;
        };
        let rec = match self.consents.step(
            PREFIX,
            &login_id,
            Some((user, origin.channel_id.as_str())),
            |r| r.ready(),
        ) {
            Ok(r) => r,
            Err(why) => return self.consent_reply(i, WHAT, refused(why), vec![]).await,
        };
        let Some(pasted) = consent::pasted(m) else {
            return self.consent_reply(i, WHAT, PASTE_TOO_LONG, vec![]).await;
        };
        self.consents.set(PREFIX, &login_id, Stage::Pasted);
        if !self.consent_ack(i, WHAT).await {
            return;
        }
        let params = McpLoginFinishParams {
            login_id: login_id.clone(),
            pasted: Some(pasted),
            wait_secs: 0,
            agreed: rec.stage == Stage::Agreed,
            author: Some(who.to_string()),
            discord: Some(origin),
        };
        consent::sending(method::MCP_LOGIN_FINISH);
        let r = self
            .rpc
            .call::<_, McpLoginFinishResult>(method::MCP_LOGIN_FINISH, params)
            .await;
        self.consents.retire(PREFIX, &login_id);
        let (text, controls) = match r {
            Ok(done) if done.state == "done" => (
                format!(
                    "✅ Signed in to `{}`: {} {} found.",
                    done.name,
                    done.tools,
                    if done.tools == 1 { "tool" } else { "tools" }
                ),
                buttons(&done.name),
            ),
            Ok(_) => (EXPIRED.to_string(), vec![]),
            Err(e) => (failure(&e).to_string(), vec![]),
        };
        self.consent_finish(i, WHAT, &text, &controls).await;
    }

    /// **Sign out**: the daemon's logout, as the presser; the server's record retired.
    async fn mcp_logout(
        &self,
        i: &Interaction,
        server: &str,
        who: &str,
        origin: theseus_protocol::DiscordOrigin,
    ) {
        if !self.consent_ack(i, WHAT).await {
            return;
        }
        self.consents.retire_target(PREFIX, server);
        let params = McpLogoutParams {
            name: server.to_string(),
            author: Some(who.to_string()),
            discord: Some(origin),
        };
        consent::sending(method::MCP_LOGOUT);
        let r = self
            .rpc
            .call::<_, McpLogoutResult>(method::MCP_LOGOUT, params)
            .await;
        let text = match r {
            Ok(out) => signed_out(&out),
            Err(e) if e.code == error_code::REFUSED => NOT_HERE,
            Err(_) => NO_SIGN_OUT,
        };
        self.consent_finish(i, WHAT, text, &[]).await;
    }
}

#[cfg(test)]
mod tests;
