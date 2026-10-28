//! The naming call (theseus-xzxqv; the owner's yes, 2026-10-19): Jev cannot
//! write a name, so when it says `new` at the act band and no name code mined
//! fits, one cheap call to `[ontology] name_profile` (`haiku`, US-based)
//! gives three names and a one-line description, and the session's next
//! `onto.v1` offers them. The only model call this design adds: off the
//! turn's path, reserved on the day's ceiling like any, a row of its own
//! (`onto.named`, a spend), at most once a session, kind and local day (the
//! cap is its META record, written with its row, so a restart keeps it).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_store::NewRecord;

use crate::fact::{self, onto::OntoNamed};
use crate::judge::spend;
use crate::provider::one_tool::{one_tool, Forcing};
use crate::provider::ProviderRequest;
use crate::rpc::Core;

/// The names asked for.
pub const NAMES: usize = 3;
/// The one tool the namer answers through.
pub(crate) const TOOL: &str = "name_category";
/// The answer's tokens at most: three names and a line.
const MAX_TOKENS: u32 = 300;
/// The message the namer reads, in characters.
const SAID_CHARS: usize = 1_500;

const INSTRUCTIONS: &str = "Name a new category with the name_category tool. The names are what \
a person would call it: one to four words, plain, no quotes, no dates. The description is one \
line saying what the category is for.";

/// What a session was last given for a kind: the day it was asked, and the
/// answer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Named {
    day: String,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    description: String,
}

fn key(session: &str, kind: &str) -> String {
    format!("onto.named.{session}.{kind}")
}

fn read(core: &Core, session: &str) -> impl Iterator<Item = Named> {
    let held: Vec<Named> = theseus_judge::builders::ONTO_KINDS
        .iter()
        .filter_map(|k| {
            core.store
                .get_meta::<Named>(&key(session, k))
                .ok()
                .flatten()
        })
        .collect();
    held.into_iter()
}

/// The names the naming call gave this session for any kind.
pub(super) fn suggested(core: &Core, session: &str) -> Vec<String> {
    read(core, session).flat_map(|n| n.names).collect()
}

/// Each suggested name with the description its call gave.
pub(super) fn described(core: &Core, session: &str) -> Value {
    let mut m = serde_json::Map::new();
    for n in read(core, session) {
        for name in n.names {
            m.insert(name, json!(n.description));
        }
    }
    Value::Object(m)
}

fn tool() -> Value {
    json!({
        "name": TOOL,
        "description": "Names for a new category.",
        "input_schema": {
            "type": "object",
            "properties": {
                "names": {"type": "array", "items": {"type": "string"},
                    "description": "Three names, best first."},
                "description": {"type": "string",
                    "description": "One line saying what the category is for."}
            },
            "required": ["names", "description"]
        }
    })
}

/// The request: the kind and what it is for, and what was said, with
/// `how` as `catalog::forcing` gives it: the 300-token cap would be
/// spent thinking first (theseus-opypg).
pub(crate) fn request(
    model: &str,
    how: Forcing,
    kind: &str,
    said: &str,
    scrub: &dyn Fn(&str) -> String,
) -> ProviderRequest {
    let said: String = scrub(said).chars().take(SAID_CHARS).collect();
    let (tool, choice) = one_tool(tool(), how.force);
    let mut extra = std::collections::BTreeMap::new();
    extra.insert("tool_choice".to_string(), choice);
    ProviderRequest {
        model: model.to_string(),
        max_tokens: MAX_TOKENS,
        system: vec![json!({"type": "text", "text": INSTRUCTIONS})],
        messages: vec![json!({"role": "user", "content": [{"type": "text",
            "text": format!("Kind of category: {kind}\n\nWhat was said:\n{said}")}]})],
        tools: vec![tool],
        thinking: how.thinking,
        output_config: None,
        cache_control: None,
        betas: Vec::new(),
        extra,
        image_tokens: 0,
    }
}

/// The answer's names and description; none when it holds no usable call.
fn parse(content: &[Value]) -> Option<(Vec<String>, String)> {
    let input = content
        .iter()
        .find(|b| b["type"] == "tool_use" && b["name"] == TOOL)?
        .get("input")?;
    let mut names: Vec<String> = Vec::new();
    for n in input["names"].as_array()? {
        let n: String = n.as_str()?.split_whitespace().collect::<Vec<_>>().join(" ");
        let n: String = n.chars().take(60).collect();
        if !n.is_empty() && !names.iter().any(|m| m.eq_ignore_ascii_case(&n)) && names.len() < NAMES
        {
            names.push(n);
        }
    }
    let line = input["description"].as_str().unwrap_or_default();
    let line: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    (!names.is_empty()).then(|| (names, line.chars().take(200).collect()))
}

/// Jev said `new` for `kind` and no mined name fit: the naming call, once a
/// session, kind and day, in a task of its own.
pub(super) fn name_when_due(core: &Arc<Core>, session: &str, kind: &str, said: &str) {
    let Ok(rt) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let (core, session, kind, said) = (
        core.clone(),
        session.to_string(),
        kind.to_string(),
        said.to_string(),
    );
    rt.spawn(async move {
        if let Err(why) = core.onto_name(&session, &kind, &said).await {
            tracing::debug!(%why, "the naming call was not made");
        }
    });
}

impl Core {
    /// The naming call for `kind` in `session`, once a day: `Err` says why it
    /// was not made (already today, the day's ceiling, no price); a failed
    /// call is made, its row written.
    pub async fn onto_name(&self, session: &str, kind: &str, said: &str) -> Result<(), String> {
        let today = spend::local_day(theseus_protocol::now_unix_ms());
        let marker = key(session, kind);
        let held = self
            .store
            .get_meta::<Named>(&marker)
            .map_err(|e| format!("the marker was not read: {e:#}"))?;
        if held.is_some_and(|n| n.day == today) {
            return Err(format!("{kind} was named for this session today"));
        }
        let profile = self.cfg.ontology.name_profile.clone();
        let (live, _) = self.live_profile();
        let target = self
            .runner
            .resolve_target(&live, Some(&profile), None, None)
            .map_err(|e| format!("[ontology] name_profile {profile:?} does not resolve: {e:#}"))?;
        let provider = self
            .runner
            .providers
            .get(&target.provider)
            .cloned()
            .ok_or_else(|| format!("no provider {:?} for {profile}", target.provider))?;
        // What the session said is sent: its labels hold the call.
        let provider = crate::data_gate::bound(provider, session);
        let scrubber = self.runner.judge.scrubber.clone();
        let how = self.runner.catalog.forcing(&target.model);
        let request = request(&target.model, how, kind, said, &|t| scrubber.scrub(t).0);
        let need = self
            .audit_reserve(&request)
            .ok_or_else(|| format!("no price for {}", target.model))?;
        let hold = self.runner.day_hold(need, "ontology")?;
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        let answered = provider.stream_message(&request, &mut quiet).await;
        let cost = answered
            .as_ref()
            .map_or(need, |r| self.runner.priced(r, &target.model, need));
        hold.settle(cost);
        let cut = answered
            .as_ref()
            .is_ok_and(|r| crate::provider::hit_cap(r.stop_reason.as_deref()));
        let (model, usage, found, failed) = match &answered {
            // A cut answer's tool input is empty or partial: never a whole one.
            Ok(r) if cut => (
                r.model.clone(),
                Some(r.usage.clone()),
                None,
                Some(crate::provider::cap_phrase(MAX_TOKENS)),
            ),
            Ok(r) => match parse(&r.content) {
                Some(f) => (r.model.clone(), Some(r.usage.clone()), Some(f), None),
                None => (
                    r.model.clone(),
                    Some(r.usage.clone()),
                    None,
                    Some("the answer held no name_category call".to_string()),
                ),
            },
            Err(e) => (target.model.clone(), None, None, Some(format!("{e:#}"))),
        };
        let (names, description) = found.unwrap_or_default();
        let f = OntoNamed {
            session,
            kind,
            profile: &target.profile,
            model: &model,
            cost_usd: theseus_kernel::micros_to_usd(cost),
            input_tokens: usage.as_ref().map_or(0, |u| u.input_tokens),
            output_tokens: usage.as_ref().map_or(0, |u| u.output_tokens),
            names: &names,
            description: &description,
            failed: failed.as_deref(),
        };
        let row = fact::row(&f, Some(session), None).map_err(|e| e.to_string())?;
        // A call cut at its cap names nothing, and does not spend the day's
        // slot: the session's next such message asks again.
        let mut records = vec![row.scoped("judge:onto")];
        if !cut {
            let mark = Named {
                day: today,
                names: names.clone(),
                description: description.clone(),
            };
            records.push(
                NewRecord::json(theseus_store::kinds::META, Some(&marker), &mark)
                    .map_err(|e| e.to_string())?,
            );
        }
        self.people_write(&records)
            .await
            .map_err(|e| format!("the naming's row was not written: {e:#}"))?;
        self.rec(Some(session)).announce(&f);
        Ok(())
    }
}
