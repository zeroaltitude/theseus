//! The harness's part of the one block beside the message (issue 46, line 5): slot I's "now". What
//! the request's tail once carried as a dozen blocks of its own, each glued to the last message on every
//! call, is rendered once at the turn's first compile by each part's own renderer, composed here in one
//! fixed order under one budget, counted in the view's budget before the other bands are admitted, and
//! laid first in the block beside the message, frozen for the turn (`view_step`).
//!
//! - **The order** (the order of [`Name`]): the date and the place's facts, who answers, what stands here, what the
//!   harness did with a yes, the pending wakes, the task graph, the day's observations, the coding hint,
//!   the fan-out cue, the project. Facts come before hints, so a hint that differs leaves the facts' bytes
//!   alone.
//! - **The budget** ([`BUDGET_TOKENS`]): parts are dropped whole from the end of the order until the block
//!   fits, and one line says which were left out. The date and the place's facts, who answers, and what
//!   stands here are never dropped: the standing check fails a request without its records
//!   (`standing::missing_here`).
//! - **Within a turn** the block says what was true at the turn's first call. A tool that changes a task,
//!   a wake or the branch says so in its own result; the next turn composes afresh.

use theseus_protocol::projects::ProjectSliceSummary;
use theseus_protocol::tasks::TaskViewSummary;

use super::{Turn, TurnRunner};
use crate::compiler::view::Frozen;
use crate::node::{Body, Node};
use crate::session::SessionRecord;
use crate::task_graph::view::BYTES_PER_TOKEN;

/// How the harness's part begins: a reader of a request tells it from the person's words
/// (`task_graph::view::is_head`). Each part under it says whose it is by its own head (the standing
/// records' `standing::FRAMING`, the fan-out cue's `CONTEXT_NOT_THE_OWNERS`, the yes-fact's own), so the
/// heading lays no one sentence over all of them.
pub const HEAD: &str =
    "[The harness: this turn's block beside the message, of what is true where you \
work and what stands, waits and is planned here; each part says whose it is.]";

/// The harness's part's one budget, in tokens (estimated at `BYTES_PER_TOKEN`).
pub const BUDGET_TOKENS: u64 = 1_200;

/// One part of the harness's block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Name {
    /// The date, the git line, the place's routes and the profiles (`env_tail::env_text`).
    Env,
    /// Who answers this turn (`engage::Engaged::line`).
    Persona,
    /// The records of the turn's surface (`standing::Block::here_text`).
    Standing,
    /// What the harness did with the owner's yes (`yes_fact::shown`).
    Yes,
    /// The pending wakes, watches and routines (`commitments::shown`).
    Commitments,
    /// The task graph in this conversation's scope (`task_graph::view::shown`).
    Tasks,
    /// What was noted of the owner's own words today (`card::noted::shown`).
    Noted,
    /// A deep-coding turn's reading hint (`tier::CODING_HINT`).
    Coding,
    /// The static fan-out cue's line (`route_step::FAN_OUT_LINE`).
    FanOut,
    /// The project this conversation serves (`project::slice::shown`).
    Project,
}

/// The fixed order of the parts: the order of `Name`.
#[cfg(test)]
const ORDER: [Name; 10] = [
    Name::Env,
    Name::Persona,
    Name::Standing,
    Name::Yes,
    Name::Commitments,
    Name::Tasks,
    Name::Noted,
    Name::Coding,
    Name::FanOut,
    Name::Project,
];

impl Name {
    /// The part's name, as the left-out line and the rows say it.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Env => "the date and the place's facts",
            Self::Persona => "who answers",
            Self::Standing => "what stands here",
            Self::Yes => "what the harness did with a yes",
            Self::Commitments => "the pending wakes",
            Self::Tasks => "the task graph",
            Self::Noted => "today's observations",
            Self::Coding => "the coding hint",
            Self::FanOut => "the fan-out cue",
            Self::Project => "the project",
        }
    }

    /// Never dropped for room.
    fn kept(self) -> bool {
        matches!(self, Self::Env | Self::Persona | Self::Standing)
    }
}

/// Each part as its renderer gave it, once a turn, with what the task graph and the project showed (the
/// measuring row's).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Rendered {
    pub parts: Vec<(Name, String)>,
    pub tasks: Option<TaskViewSummary>,
    pub project: Option<ProjectSliceSummary>,
}

impl Rendered {
    /// `name`'s text, replacing what it had; none removes it. Whether anything changed.
    pub(crate) fn set(&mut self, name: Name, text: Option<String>) -> bool {
        let at = self.parts.iter().position(|(n, _)| *n == name);
        let was = at.map(|i| self.parts[i].1.clone());
        if was == text {
            return false;
        }
        if let Some(i) = at {
            self.parts.remove(i);
        }
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            self.parts.push((name, text));
            self.parts.sort_by_key(|(n, _)| *n);
        }
        true
    }
}

/// The harness's part as composed: its text, its tokens, each part's tokens, and what was left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Composed {
    pub text: String,
    pub tokens: u64,
    pub shown: Vec<(Name, u64)>,
    pub left_out: Vec<Name>,
}

fn tokens(s: &str) -> u64 {
    (s.len() as u64).div_ceil(BYTES_PER_TOKEN)
}

/// The line that says which parts were left out for room.
fn left_out_line(names: &[Name]) -> String {
    let named: Vec<&str> = names.iter().map(|n| n.as_str()).collect();
    format!("Left out of this block for room: {}.", named.join(", "))
}

fn text_of(parts: &[&(Name, String)], left_out: &[Name]) -> String {
    let mut text = String::from(HEAD);
    for (_, t) in parts {
        text.push_str("\n\n");
        text.push_str(t);
    }
    if !left_out.is_empty() {
        text.push_str("\n\n");
        text.push_str(&left_out_line(left_out));
    }
    text
}

/// The parts in their order under `budget`: parts dropped whole from the end of the order (never a kept
/// one) until the block fits, with the line that says which. None when there is no part.
pub(crate) fn compose(parts: &[(Name, String)], budget: u64) -> Option<Composed> {
    let mut kept: Vec<&(Name, String)> = parts.iter().filter(|(_, t)| !t.is_empty()).collect();
    kept.sort_by_key(|(n, _)| *n);
    if kept.is_empty() {
        return None;
    }
    let mut left_out = Vec::new();
    let mut text = text_of(&kept, &left_out);
    while tokens(&text) > budget {
        let Some(i) = kept.iter().rposition(|(n, _)| !n.kept()) else {
            break;
        };
        left_out.insert(0, kept.remove(i).0);
        text = text_of(&kept, &left_out);
    }
    Some(Composed {
        tokens: tokens(&text),
        shown: kept.iter().map(|(n, t)| (*n, tokens(t))).collect(),
        left_out,
        text,
    })
}

/// The block beside the message with the harness's part first; the part alone when nothing else is
/// beside it. Laid after an occasion's line, so the harness's heading opens the block.
pub(crate) fn beside_with(
    beside: Option<Node>,
    harness: &str,
    (sid, turn): (&str, &str),
) -> Option<Node> {
    Some(match beside {
        Some(mut n) => {
            if let Body::UserMessage { text, .. } = &mut n.body {
                *text = format!("{harness}\n\n{text}");
            }
            n
        }
        None => Frozen::block(sid, turn, "", harness),
    })
}

/// The composition in the turn's trace: its tokens, each part's, and what was left out.
pub(super) fn traced(t: &mut Turn<'_>, c: &Composed) {
    let shown: Vec<serde_json::Value> = (c.shown.iter())
        .map(|(n, k)| serde_json::json!({"part": n.as_str(), "tokens": k}))
        .collect();
    let left: Vec<&str> = c.left_out.iter().map(|n| n.as_str()).collect();
    t.trace.mark(
        "view",
        "harness",
        serde_json::json!({"tokens": c.tokens, "shown": shown, "left_out": left}),
    );
}

/// What the route's lines may add after the view was frozen at the turn's first compile, reserved in the
/// view's budget then: the coding hint, the fan-out cue and a persona's line.
pub(crate) fn routed_reserve() -> u64 {
    tokens(super::tier::CODING_HINT)
        + tokens(super::route_step::FAN_OUT_LINE)
        + ROUTED_PERSONA_TOKENS
}

/// A persona's line ("<name> answers this turn"), at most.
const ROUTED_PERSONA_TOKENS: u64 = 24;

impl TurnRunner {
    /// Every part, each by its own renderer, as the turn's first compile sees it.
    pub(super) async fn now_rendered(&self, t: &Turn<'_>, session: &SessionRecord) -> Rendered {
        let mut r = Rendered::default();
        r.set(Name::Env, self.env_text(t).await);
        Self::now_routed(t, &mut r);
        r.set(
            Name::Standing,
            (t.standing.here.as_ref())
                .and_then(|b| b.here_text())
                .map(str::to_string),
        );
        let target = self.target_of(&session.session_id);
        if self.cfg.standing.yes_fact && session.task.is_none() {
            let yes = crate::yes_fact::shown(
                &self.yes_facts,
                t.tc.session_id,
                t.tc.turn_id,
                t.tc.place().class,
                target.as_deref(),
            );
            r.set(Name::Yes, yes);
        }
        // The wakes this conversation has pending (theseus-lherl): from the turn's list, never a read of its
        // own; the routines it made, from memory (theseus-pvm5c).
        let routines = crate::routine::lines(
            &self.kernel,
            &self.tools.routines.made_in(&session.session_id),
        );
        let wakes =
            crate::commitments::shown(t.tc.commitments, &routines, self.kernel.now_ms(), &|w| {
                // A watch's observed value is outside text the transcript does not show: latched for the
                // turn before it is shown, or left out (issue 44).
                let seen = self.kernel.watch_state(&w.id).ok().flatten();
                if seen.is_some_and(|s| s.last.is_some())
                    && !crate::task_graph::judged::latch_once(t.tc.store, t.tc.turn_id)
                {
                    return None;
                }
                crate::watch::run::shown_value(&self.kernel, w)
            });
        r.set(Name::Commitments, wakes.map(|s| s.text));
        // A check sees the task it checks by title and state (theseus-w8ys).
        if let Some(v) = crate::task_graph::view::shown(&self.store, &self.kernel, session) {
            r.set(Name::Tasks, Some(v.text));
            r.tasks = Some(v.summary);
        }
        // What the observer noted of the owner's own words today (theseus-6f8t0): only where the owner is
        // alone, and never for a task's session.
        if session.task.is_none() {
            let noted = crate::card::noted::shown(
                &self.store,
                t.tc.place().class,
                target.as_deref(),
                self.kernel.now_ms(),
            );
            r.set(Name::Noted, noted);
        }
        if let Some(s) = crate::project::slice::shown(&self.store, &self.kernel, session) {
            r.set(Name::Project, Some(s.text));
            r.project = Some(s.summary);
        }
        r
    }

    /// The parts the turn's route decides: who answers, the coding hint, the fan-out cue. Whether any
    /// changed.
    pub(super) fn now_routed(t: &Turn<'_>, r: &mut Rendered) -> bool {
        let persona = t.route.persona.as_ref().map(|p| p.line());
        let coding = (t.route.injection.as_ref())
            .is_some_and(|b| b.coding)
            .then(|| super::tier::CODING_HINT.to_string());
        let fan_out = t
            .route
            .fan_out
            .then(|| super::route_step::FAN_OUT_LINE.to_string());
        let a = r.set(Name::Persona, persona);
        let b = r.set(Name::Coding, coding);
        let c = r.set(Name::FanOut, fan_out);
        a || b || c
    }
}

/// The harness's part as a request carries it: the text block that opens with [`HEAD`], up to the view's
/// own heading (tests read a part of it by the part's own head).
#[cfg(test)]
pub(crate) fn carried(req: &crate::provider::ProviderRequest) -> Option<String> {
    let texts = req.messages.iter().flat_map(|m| match &m["content"] {
        serde_json::Value::String(s) => vec![s.clone()],
        c => (c.as_array().into_iter().flatten())
            .filter_map(|b| b["text"].as_str().map(str::to_string))
            .collect(),
    });
    let block = texts.into_iter().find(|t| t.starts_with(HEAD))?;
    let view = format!("\n\n{}", crate::view::wire_text::BESIDE_HEAD);
    Some(
        block
            .split(view.as_str())
            .next()
            .unwrap_or_default()
            .to_string(),
    )
}

/// The part of the harness's block that opens with `head`, up to the next part's own head; none when the
/// request carries no such part.
#[cfg(test)]
pub(crate) fn part(req: &crate::provider::ProviderRequest, head: &str) -> Option<String> {
    let text = carried(req)?;
    let at = text.find(&format!("\n\n{head}"))? + 2;
    let heads = [
        super::env_tail::HEAD,
        crate::standing::HERE,
        crate::yes_fact::HEAD,
        crate::commitments::HEAD,
        crate::task_graph::view::HEAD,
        crate::card::noted::HEAD,
        super::tier::CODING_HINT,
        super::route_step::FAN_OUT_LINE,
        crate::project::slice::HEAD,
        "Left out of this block for room:",
    ];
    let rest = &text[at..];
    let end = (heads.iter())
        .filter(|h| **h != head)
        .filter_map(|h| rest.find(&format!("\n\n{h}")))
        .min()
        .unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// Who the harness's block says answers this turn: the name in its persona's line.
#[cfg(test)]
pub(crate) fn answering(req: &crate::provider::ProviderRequest) -> Option<String> {
    (carried(req)?.split("\n\n"))
        .find_map(|p| p.strip_suffix(" answers this turn").map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(of: &[(Name, usize)]) -> Vec<(Name, String)> {
        of.iter()
            .map(|(n, len)| (*n, format!("{} {}", n.as_str(), "x".repeat(*len))))
            .collect()
    }

    /// Each part goes in the fixed order whatever order it was given in, under the harness's heading.
    #[test]
    fn the_parts_go_in_the_fixed_order_under_one_heading() {
        let given = parts(&[
            (Name::Project, 10),
            (Name::Env, 10),
            (Name::Tasks, 10),
            (Name::Standing, 10),
        ]);
        let c = compose(&given, BUDGET_TOKENS).unwrap();
        assert!(c.text.starts_with(HEAD), "{}", c.text);
        let at = |n: Name| c.text.find(n.as_str()).unwrap();
        assert!(at(Name::Env) < at(Name::Standing));
        assert!(at(Name::Standing) < at(Name::Tasks));
        assert!(at(Name::Tasks) < at(Name::Project));
        assert_eq!(
            c.shown.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            [Name::Env, Name::Standing, Name::Tasks, Name::Project]
        );
        assert!(c.left_out.is_empty() && !c.text.contains("Left out"));
        assert_eq!(c.tokens, tokens(&c.text));
        assert!(ORDER.windows(2).all(|w| w[0] < w[1]));
    }

    /// Over the budget, parts go whole from the end of the order, one line says which, and the block fits.
    #[test]
    fn over_the_budget_parts_go_whole_from_the_end_and_one_line_says_which() {
        let given = parts(&[
            (Name::Env, 60),
            (Name::Standing, 300),
            (Name::Commitments, 600),
            (Name::Tasks, 3_000),
            (Name::Project, 900),
        ]);
        let c = compose(&given, BUDGET_TOKENS).unwrap();
        assert_eq!(c.left_out, [Name::Tasks, Name::Project]);
        assert!(c.tokens <= BUDGET_TOKENS, "{}", c.tokens);
        assert!(
            c.text
                .ends_with("Left out of this block for room: the task graph, the project."),
            "{}",
            c.text
        );
        assert!(!c.text.contains(Name::Tasks.as_str()) || c.text.contains("Left out"));
        assert!(c.text.contains(Name::Commitments.as_str()));
    }

    /// The date, who answers and what stands here are never dropped, however large.
    #[test]
    fn the_kept_parts_are_never_dropped() {
        let given = parts(&[
            (Name::Env, 300),
            (Name::Persona, 30),
            (Name::Standing, 4_000),
            (Name::Coding, 300),
        ]);
        let c = compose(&given, BUDGET_TOKENS).unwrap();
        assert_eq!(c.left_out, [Name::Coding]);
        assert!(c.text.contains(Name::Standing.as_str()));
        assert!(
            c.tokens > BUDGET_TOKENS,
            "the kept parts are counted as they are"
        );
    }

    /// No part, no block; an empty part is no part.
    #[test]
    fn nothing_to_say_is_no_block() {
        assert_eq!(compose(&[], BUDGET_TOKENS), None);
        assert_eq!(
            compose(&[(Name::Tasks, String::new())], BUDGET_TOKENS),
            None
        );
    }

    /// A part set again replaces itself; set to none it goes; the same text changes nothing.
    #[test]
    fn a_part_set_again_replaces_itself() {
        let mut r = Rendered::default();
        assert!(r.set(Name::Project, Some("p".into())));
        assert!(r.set(Name::Env, Some("e".into())));
        assert!(!r.set(Name::Env, Some("e".into())));
        assert!(r.set(Name::Env, Some("e2".into())));
        assert_eq!(
            r.parts,
            [
                (Name::Env, "e2".to_string()),
                (Name::Project, "p".to_string())
            ]
        );
        assert!(r.set(Name::Project, None));
        assert!(!r.set(Name::Project, None));
        assert_eq!(r.parts.len(), 1);
    }

    /// The heading is the harness's, so a reader that tells the harness's blocks from the person's
    /// words skips the whole block.
    #[test]
    fn the_heading_is_read_as_the_harness_s() {
        let c = compose(&parts(&[(Name::Env, 5)]), BUDGET_TOKENS).unwrap();
        assert!(crate::task_graph::view::is_head(&c.text), "{}", c.text);
    }

    /// The route's lines fit in what the view reserved for them.
    #[test]
    fn the_route_s_lines_fit_their_reserve() {
        let line = "a persona with a long name answers this turn";
        assert!(tokens(line) <= ROUTED_PERSONA_TOKENS);
        assert!(routed_reserve() < BUDGET_TOKENS / 4, "{}", routed_reserve());
    }
}
