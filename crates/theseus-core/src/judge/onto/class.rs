//! The class call (issue 9): a kind is a class of things (a country), and
//! the thing that prompted it (France) is one of its entries. Jev answers
//! only choices and scores, so it cannot write the word for a class; this is
//! the one cheap text call that does, like the naming call (`name.rs`).
//!
//! - **Asked once per cluster** the sweep is about to mint a kind for: given
//!   the cluster's names and the titles of the sessions that said them, one
//!   lowercase singular word. A word that is usable is kept in the day's mark
//!   (`Mark::classed`), so a later night does not ask again.
//! - **Paid under the sweep's own daily ceiling**
//!   (`[ontology] sweep_usd_per_day`, `Run::left`): no new ceiling, none
//!   raised. It is also held on the day's ceiling like any call.
//! - **When it fails, is over the ceiling, or gives a word a kind cannot hold,
//!   or one of the names itself, no kind is made that night** and the next
//!   night asks again.
//! - A row of its own: `onto.named`, with kind `class`.

use serde_json::{json, Value};
use theseus_judge::price::Micros;
use theseus_store::NewRecord;

use crate::fact::{self, onto::OntoNamed};
use crate::provider::one_tool::{one_tool, Forcing};
use crate::provider::ProviderRequest;
use crate::rpc::Core;

/// The one tool the call answers through.
const TOOL: &str = "name_class";
/// The answer's tokens at most: one word.
const MAX_TOKENS: u32 = 100;
/// Titles shown at most.
const TITLES: usize = 6;
/// The longest title shown, in characters.
const TITLE_CHARS: usize = 120;

const INSTRUCTIONS: &str = "Name the class of things the given names belong to, with the \
name_class tool. The class is one lowercase singular word a person would use for what each of \
the names is (country, person, tool, company). It is never one of the names itself, never plural, \
and has no spaces.";

/// What the call came to.
pub(super) enum Classed {
    /// A usable word, and what the call cost.
    Said(String, Micros),
    /// The call was made and gave nothing usable (a failure, a cut answer, a
    /// word a kind cannot hold), at what it cost.
    Unusable(Micros),
    /// Its worst case would pass what the sweep's day cap leaves.
    Over(Micros),
    /// Not made: no model, no price, or the day's ceiling refused.
    Off,
}

fn tool() -> Value {
    json!({
        "name": TOOL,
        "description": "The class of things the names belong to.",
        "input_schema": {
            "type": "object",
            "properties": {
                "class": {"type": "string",
                    "description": "One lowercase singular word for the class, such as country."}
            },
            "required": ["class"]
        }
    })
}

/// The request: the names, then what the sessions that said them were titled.
pub(crate) fn request(
    model: &str,
    how: Forcing,
    names: &[String],
    titles: &[String],
    scrub: &dyn Fn(&str) -> String,
) -> ProviderRequest {
    let shown: Vec<String> = titles
        .iter()
        .take(TITLES)
        // plain cut: the scrubber has masked every known value in it first
        .map(|t| scrub(t).chars().take(TITLE_CHARS).collect())
        .collect();
    let text = format!(
        "Names:\n{}\n\nConversations they came up in:\n{}",
        names
            .iter()
            .map(|n| format!("- {}", scrub(n)))
            .collect::<Vec<_>>()
            .join("\n"),
        shown
            .iter()
            .map(|t| format!("- {t}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let (tool, choice) = one_tool(tool(), how.force);
    let mut extra = std::collections::BTreeMap::new();
    extra.insert("tool_choice".to_string(), choice);
    ProviderRequest {
        model: model.to_string(),
        max_tokens: MAX_TOKENS,
        system: vec![json!({"type": "text", "text": INSTRUCTIONS})],
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": text}]})],
        tools: vec![tool],
        thinking: how.thinking,
        output_config: None,
        cache_control: None,
        betas: Vec::new(),
        extra,
        image_tokens: 0,
    }
}

/// The class word of an answer, when a kind can hold it and it is not one of
/// `names` (the thing itself, which is what the sweep made a kind of before).
pub(crate) fn parse(content: &[Value], names: &[String]) -> Option<String> {
    let word = content
        .iter()
        .find(|b| b["type"] == "tool_use" && b["name"] == TOOL)?
        .get("input")?["class"]
        .as_str()?
        .trim()
        .to_lowercase();
    let fold = |s: &str| theseus_ontology::sweep::fold(s);
    let usable = !word.is_empty()
        && word.chars().count() <= 32
        && word.starts_with(|c: char| c.is_ascii_lowercase())
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && !names.iter().any(|n| fold(n) == fold(&word));
    usable.then_some(word)
}

impl Core {
    /// The class of the cluster `names`, said in `titles` by `session`'s kind
    /// of conversation: one call, `left` the most the sweep may still spend.
    pub(super) async fn onto_class(
        &self,
        session: &str,
        names: &[String],
        titles: &[String],
        left: Micros,
    ) -> Classed {
        let Ok((target, provider)) = self.onto_model("class").await else {
            return Classed::Off;
        };
        let scrubber = self.runner.judge.scrubber.clone();
        let how = self.runner.catalog.forcing(&target.model);
        let request = request(&target.model, how, names, titles, &|t| scrubber.scrub(t).0);
        let Some(need) = self.audit_reserve(&request) else {
            return Classed::Off;
        };
        if need > left {
            return Classed::Over(need);
        }
        let Ok(hold) = self.runner.day_hold(need, "ontology") else {
            return Classed::Off;
        };
        let started_ms = theseus_protocol::now_unix_ms();
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        let answered = provider.stream_message(&request, &mut quiet).await;
        let cost = answered.as_ref().map_or(need, |r| {
            self.runner.priced(r, &target.model, need, started_ms)
        });
        hold.settle(cost);
        let cut = answered
            .as_ref()
            .is_ok_and(|r| crate::provider::hit_cap(r.stop_reason.as_deref()));
        let (model, usage, found, failed): (String, _, Option<String>, Option<String>) =
            match &answered {
                Ok(r) if cut => (
                    r.model.clone(),
                    Some(r.usage.clone()),
                    None,
                    Some(crate::provider::cap_phrase(MAX_TOKENS)),
                ),
                Ok(r) => match parse(&r.content, names) {
                    Some(w) => (r.model.clone(), Some(r.usage.clone()), Some(w), None),
                    None => (
                        r.model.clone(),
                        Some(r.usage.clone()),
                        None,
                        Some("the answer held no usable name_class call".to_string()),
                    ),
                },
                Err(e) => (target.model.clone(), None, None, Some(format!("{e:#}"))),
            };
        let word: Vec<String> = found.iter().cloned().collect();
        let f = OntoNamed {
            session,
            kind: "class",
            profile: &target.profile,
            model: &model,
            cost_usd: theseus_kernel::micros_to_usd(cost),
            input_tokens: usage.as_ref().map_or(0, |u| u.input_tokens),
            output_tokens: usage.as_ref().map_or(0, |u| u.output_tokens),
            names: &word,
            description: "",
            failed: failed.as_deref(),
        };
        let Ok(row) = fact::row(&f, Some(session), None) else {
            return Classed::Unusable(cost);
        };
        let record: NewRecord = row.scoped("judge:onto");
        if let Err(e) = self.people_write(&[record]).await {
            tracing::warn!(error = %format!("{e:#}"), "ontology: the class call's row was not written");
        }
        self.rec(Some(session)).announce(&f);
        match found {
            Some(w) => Classed::Said(w, cost),
            None => Classed::Unusable(cost),
        }
    }
}
