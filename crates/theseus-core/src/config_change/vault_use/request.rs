//! `http.request`'s harness side (issue 65): the owner's own turn sends one request carrying a
//! key from the vault. The request itself is `web::request`'s; this side decides who may, reads
//! the item, holds its value, and keeps what is stored free of it.
//!
//! - **The wall is `vault.use`'s** (`wall.rs`), and a routine never has the tool.
//! - **It asks first** at the gate, before anything is read, as the run form does
//!   ([`once::ask_first_for`]): after outside text since the owner's last message at both
//!   `outside` settings, in a flagged session, and in a turn the owner's own message did not
//!   begin. The answer of an earlier `http.request` to the same host with the same item is not
//!   counted as outside text here ([`same`]): that service already holds the key. A DELETE asks
//!   every time (the tool's plan).
//! - **The item** is read as the run form reads it (`check::reading`), once. An item Theseus
//!   itself runs on is not sent ([`once::runs_on`]), nor a value too short to mask.
//! - **The value** is held on the board under `vault-use:<call>` before anything is sent, so every
//!   exit masks it, and the answer is masked of it before it is stored: its text and every string
//!   of its meta. The item is remembered by name (`used.rs`), so every start reads and masks it
//!   again.
//! - **The hold is read again when the call runs**, as the run form's is: outside text an earlier
//!   call of the same response read stops it, unless the owner approved this call ([`AGAIN`]).
//! - **The owner is told** once per item and host in a span: the item, the field and the vault
//!   by the vault's names, the method, the host and the path. None of the model's words.
//! - **After a restart** a request that was out answers that its outcome is not known
//!   (`web::request::again`); it is never sent again.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::Plan;
use theseus_tools::Tool;
use zeroize::Zeroizing;

use super::check::{self, Read};
use super::once;
use crate::node::{Body, Node};
use crate::scrub::Scrubber;
use crate::toolrun::{ToolRuntime, TurnCtx};
use crate::web::request::{self as req, Checked, REQUEST};

/// The answer for an item Theseus itself runs on.
pub const RUNS_ON: &str = "this item is a key Theseus itself runs on, so it is not sent in a \
     request by this tool: use `vault.use` with `name` (and `also`) instead, which puts it to the \
     owner on a card";

/// The answer for a value the scrubber cannot mask.
pub const SHORT: &str = "this item's value is shorter than 8 characters, which Theseus cannot \
     mask in what it keeps, so it is not sent";

/// The answer when outside text reached the conversation after the gate judged this call.
pub const AGAIN: &str = "text from outside reached this conversation after this call was \
     checked, so nothing was read from the vault: call `http.request` again, on its own, and the \
     owner is asked first";

/// What the tool asks to do, as the hold says it.
const WHAT: &str = "send a key from Theseus's vault in a request";

/// The one value this call's tool is handed, by [`req::KEY`].
struct One(Zeroizing<String>);

impl std::fmt::Debug for One {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("One(..)")
    }
}

impl theseus_tools::Secrets for One {
    fn secret(&self, name: &str) -> Result<Zeroizing<String>, String> {
        match name == req::KEY {
            true => Ok(self.0.clone()),
            false => Err(format!("this call was handed no secret {name}")),
        }
    }
}

/// Whether `n` is the answer of an earlier `http.request` to `host` with the item `item` of the
/// vault `vault`, by the names the vault listed (any case).
fn same(n: &Node, host: &str, vault: &str, item: &str) -> bool {
    let Body::ToolResult { tool, meta, .. } = &n.body else {
        return false;
    };
    let named = |k: &str, v: &str| {
        meta["vault"][k]
            .as_str()
            .is_some_and(|x| x.eq_ignore_ascii_case(v.trim()))
    };
    tool == REQUEST
        && meta["host"].as_str() == Some(host)
        && named("vault", vault)
        && named("item", item)
}

/// The gate's hold on `http.request`: the run form's, but an earlier answer from the same host
/// with the same item is not outside text for it. A call the wall refuses is left to the wall.
pub(crate) fn ask_first(tc: &TurnCtx<'_>, input: &Value, plan: Plan) -> Plan {
    if crate::config_change::wall::refusal_for(tc, REQUEST).is_some() {
        return plan;
    }
    // The tool's own plan has checked the input; this reads the names from it.
    let Ok(c) = req::check(input, true) else {
        return plan;
    };
    let (host, k) = (c.host(), &c.input.key);
    once::ask_first_for(tc, WHAT, &|n| same(n, &host, &k.vault, &k.item), plan)
}

/// The hold read again when the call runs: outside text an earlier call of the same response
/// read, unless the owner approved this very call.
fn held_since(tc: &TurnCtx<'_>, c: &Checked, correlation_id: &str) -> bool {
    let (host, k) = (c.host(), &c.input.key);
    let skip = |n: &Node| same(n, &host, &k.vault, &k.item);
    once::hold_reason_for(tc, WHAT, &skip).is_some() && !once::approved(tc, correlation_id)
}

/// Whether the owner was told of this item and host in this span: an earlier answer in it is
/// one.
fn told(tc: &TurnCtx<'_>, host: &str, c: &super::Checked) -> bool {
    let Ok(nodes) = tc.store.session_nodes(tc.session_id) else {
        return false;
    };
    let (from, _) = crate::config_change::hold::span(&nodes);
    nodes[from..]
        .iter()
        .any(|(_, n)| same(n, host, &c.vault, &c.item))
}

/// Every string of `v`, masked.
fn masked(v: Value, mask: &Scrubber) -> Value {
    match v {
        Value::String(s) => Value::String(mask.scrub(&s).0),
        Value::Array(a) => Value::Array(a.into_iter().map(|x| masked(x, mask)).collect()),
        Value::Object(o) => {
            Value::Object(o.into_iter().map(|(k, x)| (k, masked(x, mask))).collect())
        }
        other => other,
    }
}

/// Check the call by the tool's own rule and the hold again, then read its item once: an item
/// Theseus runs on, and a value too short to mask, are not sent. The item is remembered by name
/// before its value can be sent, so every start masks it again. Nothing is read when the call is
/// refused.
async fn read_key(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    (tool, input, correlation_id): (&dyn Tool, &Value, &str),
    ctx: &theseus_tools::ToolCtx,
) -> Result<(Checked, Read), String> {
    // The tool's own rule on the URL, the headers and the body, before anything is read.
    tool.plan(input, ctx)
        .map_err(|e| format!("Not sent: {e}"))?;
    let c: Checked = req::check(input, true).map_err(|e| format!("Not sent: {e}"))?;
    if crate::config_change::card::reason_line(&c.input.reason).is_empty() {
        return Err("give a reason in the owner's words, one line".into());
    }
    if held_since(tc, &c, correlation_id) {
        return Err(format!("Not sent: {AGAIN}"));
    }
    let Some(reader) = rt.look.reader().filter(|r| !r.has_no_vault()) else {
        return Err(
            "Not sent: this Theseus has no vault session (its secrets come from the environment \
             or from files), so it cannot use a vault item"
                .into(),
        );
    };
    let k = &c.input.key;
    let read = check::reading(
        &reader,
        &k.vault,
        &k.item,
        &k.field,
        k.section
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
    )
    .await
    .map_err(|e| format!("Not sent: {e}"))?;
    if once::runs_on(rt, &read).map_err(|e| format!("Not sent: {e}"))? {
        return Err(format!("Not sent: {RUNS_ON}"));
    }
    if !crate::scrub::protectable(read.value.expose()) {
        return Err(format!("Not sent: {SHORT}"));
    }
    super::used::remember(tc.store, &read.checked).map_err(|e| format!("Not sent: {e}"))?;
    Ok((c, read))
}

/// The answer's text, with the node's meta made whole; or, when nothing went out, Theseus's own
/// words. An answer is marked as outside text; one that may have reached the server unanswered
/// is `outcome_unknown`.
fn answered(
    got: theseus_tools::AsyncResult,
    c: &Checked,
    meta: &mut Value,
) -> Result<String, String> {
    match got {
        Ok((out, external)) => {
            if let (Some(m), Value::Object(o)) = (meta.as_object_mut(), out.meta) {
                m.extend(o);
            }
            meta["external"] = json!(external.map_or_else(|| c.marker(), |e| e.url));
            Ok(out.text)
        }
        Err(f) if f.meta["outcome_unknown"].as_bool() == Some(true) => {
            meta["outcome_unknown"] = json!(true);
            Ok(f.message)
        }
        Err(f) => Err(f.message),
    }
}

/// Run `http.request` for the call `correlation_id` of the turn `tc`. An error is the result the
/// model reads: Theseus's own words.
pub async fn run(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    tool: &dyn Tool,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    if let Some(why) = crate::config_change::wall::refusal_for(tc, REQUEST) {
        return Err(why);
    }
    let mut ctx = rt.ctx_in(tc).into_owned();
    let (c, read) = read_key(rt, tc, (tool, input, correlation_id), &ctx).await?;
    let Read { checked, value, .. } = read;
    let handle = format!("{}{correlation_id}", crate::broker::ONCE_HANDLE);
    let mask = Scrubber::with_values(vec![(value.expose().to_string(), handle.clone())]);
    rt.broker.hold(&handle, value.clone());
    let host = c.host();
    let was_told = told(tc, &host, &checked);
    ctx.secrets = Some(Arc::new(One(Zeroizing::new(value.expose().to_string()))));
    drop(value);
    let got = tool.run_async(input, &ctx).await;
    drop(ctx);
    let method = c.method.as_str().to_string();
    let names = json!({"vault": checked.vault, "item": checked.item, "field": checked.field});
    let mut meta =
        json!({"vault": names, "as": c.input.key.place.word(), "host": host, "method": method});
    // Nothing went out: Theseus's own words, masked all the same.
    let text = answered(got, &c, &mut meta).map_err(|e| mask.scrub(&e).0)?;
    tc.record(&crate::fact::tool::SecretGranted {
        grant: &crate::broker::Grant {
            to: REQUEST.into(),
            variable: None,
            secret: handle,
        },
        correlation_id,
        tool: REQUEST,
        once: Some(
            json!({"once": true, "vault": names, "request": {"host": host, "method": method}}),
        ),
    });
    if !was_told {
        tell(tc, &c, &checked, &mask);
    }
    Ok((mask.scrub(&text).0, masked(meta, &mask)))
}

/// The owner's one note for this item and host in this span.
fn tell(tc: &TurnCtx<'_>, c: &Checked, checked: &super::Checked, mask: &Scrubber) {
    let short = crate::narrative::short(tc.session_id);
    let body = format!(
        "In conversation {short}, Theseus sent the vault item \"{}\" (field \"{}\", in the vault \
         \"{}\") as {} in a {} to {}{}.",
        checked.item,
        checked.field,
        checked.vault,
        c.input.key.place.said(),
        c.method,
        c.host(),
        c.path()
    );
    let note = crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        "A key from the vault, sent in a request",
        &mask.scrub(&body).0,
        "Nothing was written to the settings. Theseus remembers which items it has used this way, \
         by name only, and reads them again at each start so that it keeps masking their values; \
         the next use reads the vault again. To stop it being used at all, take the item out of \
         what Theseus's vault account can see.",
    );
    crate::config_change::hold::tell_owner(tc, &note, REQUEST);
}
