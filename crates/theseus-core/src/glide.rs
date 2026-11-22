//! Gliding (M7 step 38b, theseus-ypy0): a conversation reaches across the
//! operator's places, on the place rule (the owner, 2026-10-04: "Gliding with the
//! place rule: yes!"). Its first design rested on 19a's confidentiality
//! labels, which the place rule replaced on 2026-10-03.
//!
//! - **`channel.post { to, text }`** posts into another bound place: one
//!   outbox post (`glide:<correlation id>`), written in the frame that
//!   settles the call, which that place's lane sends once, in its order.
//!   Class `Write`, and its posture is no looser than the destination's
//!   floor (38a).
//! - **`channel.read`**, which brought a place's last messages into this
//!   session, went with issue 46: the model looks back with the history
//!   tool, `context.history`, which pages any place's exchanges. Its old
//!   `glide.read` rows and their edges stay readable.
//! - **The rule** is the place rule's (`places::glide_rule`): the gate runs
//!   it after the place's floor (`toolrun/order.rs`), and the run checks it
//!   again. Into a private place a post always may; out of a private place,
//!   or between two shared places, it asks the owner first, and only the
//!   owner's answer from a private place counts, as for `/publish`. A post
//!   out of a private place into a shared one is recorded as a publish is,
//!   with a `place.published` row.
//! - **A place** is named by its label (`#deploys`, `DM @zeroaltitude`) or its key
//!   (`channel:<id>`, `dm:<user id>`); one this daemon is not bound to fails
//!   as "not a place Theseus is bound to".
//! - **Seen in**: the row `glide.posted` (from, to, the characters, and how
//!   the rule allowed it), a narrative line, the call's notice, and its card
//!   when it asks.

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use crate::ceiling::Ceiling;
use crate::places::{glide_rule, BoundPlace, End, Glide, PlaceClass};
use crate::policy::{Decision, Posture};
use crate::toolrun::TurnCtx;

pub const POST: &str = "channel.post";
/// The tools this module adds, for the template's `[policy.tools]` list.
pub const NAMES: [&str; 1] = [POST];
/// The longest text a post takes, in characters: two Discord messages.
pub const MAX_POST_CHARS: usize = 4_000;
/// What a place this daemon is not bound to fails with.
pub const UNBOUND: &str = "not a place Theseus is bound to";
/// The setting a glide's ask names.
const SETTING: &str = "the place rule (38b)";

/// Whether `tool` is a glide.
pub fn is_glide(tool: &str) -> bool {
    tool == POST
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PostInput {
    to: String,
    text: String,
}

fn post_input(input: &Value) -> Result<PostInput, String> {
    let i: PostInput = parse(input)?;
    if i.to.trim().is_empty() {
        return Err("`to` is empty: name a place, such as #deploys or DM @zeroaltitude".into());
    }
    if i.text.trim().is_empty() {
        return Err("the text is empty: say what to post".into());
    }
    let n = i.text.chars().count();
    if n > MAX_POST_CHARS {
        return Err(format!(
            "the text is {n} characters, over the {MAX_POST_CHARS} a post takes"
        ));
    }
    Ok(i)
}

/// The toollet side of `channel.post`: its name, schema, and the plan the
/// gate reads. The harness runs it (`toolrun/glide.rs`).
pub struct ChannelPost;

impl Tool for ChannelPost {
    fn name(&self) -> &'static str {
        POST
    }

    fn description(&self) -> &'static str {
        "Post a message into another place Theseus is bound to: a Discord channel or DM, named by \
         its label (`#deploys`, `DM @zeroaltitude`) or its key (`channel:<id>`, `dm:<user id>`). Use it \
         for \"post this summary to #deploys\". It posts once, in that place's order, as \
         Theseus's message, with a line naming this session. The place rule decides who must \
         agree: into a private place (the operator's DM, a channel bound private) it runs at its \
         posture; out of a private place into a shared one, or from one shared place into \
         another, it waits for the operator's approval first. At most 4,000 characters. A place \
         Theseus is not bound to fails."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "to": {
                    "type": "string",
                    "description": "The place to post into: its label, such as #deploys or DM @zeroaltitude, or its key, such as channel:<id>."
                },
                "text": {
                    "type": "string",
                    "description": "What to post there, as its people should read it: at most 4,000 characters."
                }
            },
            "required": ["to", "text"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        // It says something where others read it: a page could steer a post
        // elsewhere.
        ToolClass::Write
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // Its post rides in the frame that settles it, so a run again after a
        // crash posts only when the first left nothing.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i = post_input(input)?;
        let chars = i.text.chars().count() as u64;
        Ok(Plan {
            summary: format!(
                "post {} to {}",
                crate::narrative::count(chars, "character", "characters"),
                i.to.trim()
            ),
            ..Default::default()
        })
    }
}

/// A glide's two places, as the gate reads them and its run reads them
/// again: this session's, the one the call names, and what the rule says.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// This session's place: `discord:<place>`, none for the CLI or the web
    /// UI. Its class is the turn's.
    pub own_target: Option<String>,
    pub own_name: String,
    pub own_class: PlaceClass,
    /// The place the call names, and its class now.
    pub other: BoundPlace,
    pub other_class: PlaceClass,
    pub rule: Glide,
    /// A post's destination's ceiling, whose floor the post takes (38a).
    pub floor: Option<&'static Ceiling>,
    /// A post's text.
    pub text: Option<String>,
}

impl Resolved {
    /// The words came from a private place and go into a shared one: a
    /// post that the owner's approval makes a publish.
    pub fn publishes(&self) -> bool {
        self.own_class == PlaceClass::Private && self.other_class == PlaceClass::Shared
    }

    /// The gate's decision with the glide's word on it: no looser than a
    /// post's destination's floor, and an approval when the rule asks first.
    pub fn gate(&self, d: Decision, tool: &str, summary: &str) -> Decision {
        let d = match self.floor {
            Some(c) => c.floor(d, tool, summary),
            None => d,
        };
        match &self.rule {
            Glide::Allow => d,
            Glide::AskFirst(why) => d.at_least(Posture::Approve, why, SETTING, tool, summary),
        }
    }

    /// How the rule allowed the call, which runs only once allowed:
    /// `allowed`, or `approved` when it asked first.
    pub fn allowed(&self) -> &'static str {
        match self.rule {
            Glide::Allow => "allowed",
            Glide::AskFirst(_) => "approved",
        }
    }

    /// The rule's words, when it asked first.
    pub fn why(&self) -> Option<&str> {
        match &self.rule {
            Glide::AskFirst(why) => Some(why),
            Glide::Allow => None,
        }
    }
}

/// The places of the glide `tool` with `input`, in the turn `tc`: `Err`
/// when its input is not a glide's, or the place it names is not one this
/// daemon is bound to, in words the model reads.
pub fn resolve(tc: &TurnCtx<'_>, tool: &str, input: &Value) -> Result<Resolved, String> {
    let (named, text) = match tool {
        POST => {
            let i = post_input(input)?;
            (i.to, Some(i.text))
        }
        other => return Err(format!("{other} is not a glide")),
    };
    let named = named.trim();
    let other = tc.places.find(named).ok_or_else(|| {
        format!("{named} is {UNBOUND}: name one of the places it is, which `theseus places` lists")
    })?;
    // Where this session speaks, as its class was read (`view_of`).
    let own_target = tc
        .outbox
        .target(tc.session_id)
        .or_else(|| tc.outbox.wake_target(tc.session_id));
    let own_name = tc.places.name_of(own_target.as_deref());
    let view = tc.places.view(Some(&other.target));
    let own = End {
        target: own_target.as_deref(),
        name: &own_name,
        class: tc.class,
    };
    let there = End {
        target: Some(&other.target),
        name: &other.name,
        class: view.class,
    };
    // A post's words go from this session's place to the one it names.
    let (rule, floor) = (glide_rule(&own, &there), view.ceiling);
    Ok(Resolved {
        own_class: tc.class,
        own_target,
        own_name,
        other_class: view.class,
        other,
        rule,
        floor,
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A post's input: what it needs, and the limits, in words the model
    /// reads; and the plan's summary.
    #[test]
    fn a_glides_input_names_a_place_and_keeps_to_its_limits() {
        let ctx = ToolCtx::for_tests(&std::env::temp_dir());
        let p = ChannelPost
            .plan(&json!({"to": "#deploys", "text": "shipped"}), &ctx)
            .unwrap();
        assert_eq!(p.summary, "post 7 characters to #deploys");
        let long = "x".repeat(MAX_POST_CHARS + 1);
        for (input, says) in [
            (json!({"to": " ", "text": "x"}), "`to` is empty"),
            (json!({"to": "#a", "text": "  "}), "the text is empty"),
            (json!({"to": "#a", "text": long}), "over the 4000"),
            (
                json!({"to": "#a", "text": "x", "cc": "#b"}),
                "invalid input",
            ),
        ] {
            let e = ChannelPost.plan(&input, &ctx).unwrap_err();
            assert!(e.contains(says), "{input}: {e}");
        }
    }
}
