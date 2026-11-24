//! A joined task's card (issue 44; the design's §7, "joining to ask how it
//! is going"): what a conversation is shown of a task the owner's message
//! addresses, read from records alone. The task is not touched: no step of
//! it runs, no session of it is opened, nothing is written.
//!
//! - **The card** ([`read`], [`Card::text`]): the objective as it stands, the
//!   state, the spend, the plan's step n of m, the state of each job in
//!   flight, and the task's last message cut at [`LAST_TOKENS`], each line
//!   "as of" its own time.
//! - **Addressing** ([`addressed`]): a card is called in when the message
//!   names exactly one task (the name matcher, through the view's `Names`),
//!   or when the judge's existing `addressed_task` answer names one. Two
//!   tasks named is no address: nothing is called in.
//! - **Who sees it** ([`StoreCards`]): presence does not withhold task cards.
//!   The card retains its home place and author marks. A task's own turns
//!   are shown its own card at its own place (`task_graph::place`).

use anyhow::Result;
use theseus_kernel::{Execution, Kernel};
use theseus_store::{kinds, Store as _};

use super::{children, execution_of, get, state_now, TaskState};
use crate::store::Store;
use crate::view::select::sources::{Names, Ref, TaskCards};
use crate::view::{
    Audience, Author, Form, Handle, Marks, PointOfView, Pos, Sizes, SourceRange, Space,
};

/// The most of the task's last message a card shows, in tokens (est.).
pub const LAST_TOKENS: u64 = 300;
/// What a cut message ends with.
pub const CUT: &str = " …";
/// How a card's text begins.
pub const HEAD: &str = "[Task";

/// A thing's key for a task, as the name matcher names it.
pub fn thing_of(task: &str) -> String {
    format!("task:{task}")
}

/// One job in flight: the tool its call runs and its state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub what: String,
    pub state: String,
}

/// The task's last message, cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Last {
    pub node: String,
    pub text: String,
    pub cut: bool,
    pub at_ms: u64,
}

/// A task's card as its records say it, each part with the time it is as of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub task: String,
    pub title: String,
    pub version: u64,
    pub objective: String,
    /// The record's newest version's time: the objective is as of it.
    pub objective_at_ms: u64,
    pub state: TaskState,
    /// When the record and its execution were read.
    pub read_at_ms: u64,
    /// Spent and limit, in micros, from the execution's budget.
    pub spend: Option<(u64, u64)>,
    /// Plan items closed, and listed; the newest item's time.
    pub plan: Option<(usize, usize, u64)>,
    pub jobs: Vec<Job>,
    pub last: Option<Last>,
    /// The task's session has read text from outside: what it says may carry it.
    pub outside: bool,
}

/// `text` cut at `tokens` (est.), on a character's boundary.
pub fn cut(text: &str, tokens: u64) -> (String, bool) {
    let max = (tokens * super::view::BYTES_PER_TOKEN) as usize;
    if text.len() <= max {
        return (text.to_string(), false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}{CUT}", text[..end].trim_end()), true)
}

fn dollars(micros: u64) -> String {
    format!("${:.2}", micros as f64 / 1_000_000.0)
}

impl Card {
    /// The card as a conversation is shown it.
    pub fn text(&self) -> String {
        let at = crate::push::hm;
        let mut out = format!(
            "{HEAD} {} \"{}\", v{}: read from its records; the task was not asked]\n",
            self.task, self.title, self.version
        );
        out.push_str(&format!(
            "Objective (as of {}): {}\n",
            at(self.objective_at_ms),
            self.objective
        ));
        out.push_str(&format!(
            "State (as of {}): {}\n",
            at(self.read_at_ms),
            self.state.as_str()
        ));
        if let Some((spent, limit)) = self.spend {
            out.push_str(&format!(
                "Spend (as of {}): {} of {}\n",
                at(self.read_at_ms),
                dollars(spent),
                dollars(limit)
            ));
        }
        if let Some((done, of, newest)) = self.plan {
            out.push_str(&format!(
                "Plan (as of {}): step {} of {of}\n",
                at(newest),
                (done + 1).min(of)
            ));
        }
        if !self.jobs.is_empty() {
            let jobs: Vec<String> = self
                .jobs
                .iter()
                .map(|j| format!("{} {}", j.what, j.state))
                .collect();
            out.push_str(&format!(
                "Jobs (as of {}): {}\n",
                at(self.read_at_ms),
                jobs.join("; ")
            ));
        }
        match &self.last {
            Some(l) => out.push_str(&format!(
                "Its last message (as of {}{}): {}\n",
                at(l.at_ms),
                if l.cut { ", cut" } else { "" },
                l.text
            )),
            None => out.push_str("Its last message: none yet\n"),
        }
        out
    }

    /// Its sizes (est.): whole is the card; a line is its head.
    pub fn sizes(&self) -> Sizes {
        let tokens = |s: &str| (s.len() as u64).div_ceil(super::view::BYTES_PER_TOKEN);
        let text = self.text();
        let line = tokens(text.lines().next().unwrap_or_default());
        Sizes {
            whole: tokens(&text),
            spoken: tokens(&text),
            line,
        }
    }

    /// The newest time any part of it is as of.
    pub fn newest_ms(&self) -> u64 {
        [
            self.objective_at_ms,
            self.read_at_ms,
            self.plan.map_or(0, |p| p.2),
            self.last.as_ref().map_or(0, |l| l.at_ms),
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
    }
}

/// The jobs in flight of `exec`: each open call's tool and state.
fn jobs_of(kernel: &Kernel, exec: &Execution) -> Vec<Job> {
    exec.outstanding
        .iter()
        .filter_map(|c| kernel.action(c).ok().flatten())
        .map(|a| Job {
            what: a.tool,
            state: a.state.as_str().to_string(),
        })
        .collect()
}

/// The card of task `id` as its records say it now; none for no such task.
/// Reads only: the record, its plan items, its execution, its session's
/// newest message.
pub fn read(store: &Store, kernel: &Kernel, id: &str, now_ms: u64) -> Result<Option<Card>> {
    let Some(rec) = get(store, id)? else {
        return Ok(None);
    };
    let exec = match execution_of(&rec) {
        Some(e) => kernel.execution(&e)?,
        None => None,
    };
    let all = super::all(store)?;
    let items = children(&all, &rec.id);
    let plan = (!items.is_empty()).then(|| {
        (
            items.iter().filter(|t| t.state.is_closed()).count(),
            items.len(),
            items.iter().map(|t| t.updated_at_ms).max().unwrap_or(0),
        )
    });
    let nodes = match &rec.session {
        Some(s) => store.session_nodes(s)?,
        None => Vec::new(),
    };
    let outside = nodes.iter().any(|(_, n)| crate::external::is_outside(n));
    let last = crate::task::last_message(nodes.iter().map(|(_, n)| n)).map(|(node, text)| {
        let at_ms = nodes
            .iter()
            .find(|(_, n)| n.id == node)
            .map_or(0, |(_, n)| n.created_at_ms);
        let (text, cut) = cut(&text, LAST_TOKENS);
        Last {
            node,
            text,
            cut,
            at_ms,
        }
    });
    Ok(Some(Card {
        task: rec.id.clone(),
        title: rec.title.clone(),
        version: rec.version,
        objective: rec.objective.clone(),
        objective_at_ms: rec.updated_at_ms,
        state: state_now(&rec, exec.as_ref()),
        read_at_ms: now_ms,
        spend: exec
            .as_ref()
            .map(|e| (e.budget.spent_micros, e.budget.limit_micros)),
        plan,
        jobs: exec
            .as_ref()
            .map(|e| jobs_of(kernel, e))
            .unwrap_or_default(),
        last,
        outside,
    }))
}

/// The one task a message addresses: the only task among the things it
/// names, else the judge's `addressed_task` answer when that is a task;
/// none when the message names two or more, or none and the judge named
/// none. `tasks` are the ids that exist.
pub fn addressed(named: &[String], judged: Option<&str>, tasks: &[&str]) -> Option<String> {
    let mut hit: Vec<&str> = tasks
        .iter()
        .copied()
        .filter(|t| named.contains(&thing_of(t)))
        .collect();
    hit.dedup();
    match hit.as_slice() {
        [one] => Some((*one).to_string()),
        [] => judged.filter(|j| tasks.contains(j)).map(str::to_string),
        _ => None,
    }
}

/// The marks of what a task's home session says: its place as the gate
/// names it, that place's audience, its seal. A card carries its task's.
pub trait Homes {
    fn marks(&self, session: &str) -> Marks;
}

impl Homes for crate::turn::TurnRunner {
    fn marks(&self, session: &str) -> Marks {
        use theseus_memory::gate::{Seal, Veil};
        use theseus_memory::recall::Place;
        let m = crate::view_gate::session_marks(self, None, session, Veil::Clear);
        Marks {
            place: m.place,
            audience: match m.class {
                Place::Private => Audience::Private,
                Place::Shared(_) => Audience::Shared,
                Place::Unknown => Audience::Unknown,
            },
            author: Author::Theseus,
            outside: false,
            sealed: m.seal != Seal::Clear,
            veiled: false,
        }
    }
}

/// The view's [`TaskCards`] over the store: the card of the one task this
/// message addresses, for a point of view where the owner is alone.
pub struct StoreCards<'a> {
    pub store: &'a Store,
    pub kernel: &'a Kernel,
    pub names: &'a dyn Names,
    pub homes: &'a dyn Homes,
    /// The judge's `addressed_task` answer to this message, as recorded.
    pub judged: Option<String>,
}

impl TaskCards for StoreCards<'_> {
    fn joined(&self, pov: &PointOfView, message: &str) -> Result<Vec<Ref>> {
        let own = match &pov.anchor {
            crate::view::Anchor::Place(p) => super::place::task_of(p),
            crate::view::Anchor::Task(_) => None,
        };
        let all = super::all(self.store)?;
        let mut out = Vec::new();
        if let Some(own) = own {
            out.extend(self.card(pov, &all, own, true)?);
        }
        let ids: Vec<&str> = all.iter().map(|t| t.id.as_str()).collect();
        let named = self.names.named(message)?;
        let other = addressed(&named, self.judged.as_deref(), &ids);
        if let Some(id) = other.filter(|id| Some(id.as_str()) != own) {
            out.extend(self.card(pov, &all, &id, false)?);
        }
        Ok(out)
    }
}

impl StoreCards<'_> {
    /// Task `id`'s card as a source's reference: marked as its home place's, or, for the task's `own` view,
    /// as the task's own place (its session's audience and seal).
    fn card(
        &self,
        pov: &PointOfView,
        all: &[super::TaskRecord],
        id: &str,
        own: bool,
    ) -> Result<Option<Ref>> {
        let Some(card) = read(self.store, self.kernel, id, pov.now_ms)? else {
            return Ok(None);
        };
        let Some(rec) = all.iter().find(|t| t.id == id) else {
            return Ok(None);
        };
        let marks = match (own, rec.session.as_deref()) {
            (true, Some(s)) => Marks {
                place: Some(thing_of(id)),
                ..self.homes.marks(s)
            },
            (true, None) => return Ok(None),
            (false, _) => self.homes.marks(&super::home(all, rec)),
        };
        Ok(Some(Ref {
            range: SourceRange {
                space: Space::Task { task: id.into() },
                first: Pos {
                    at_ms: rec.created_at_ms,
                    id: id.into(),
                },
                last: Pos {
                    at_ms: card.newest_ms(),
                    id: id.into(),
                },
            },
            marks: Marks {
                outside: card.outside,
                ..marks
            },
            sizes: card.sizes(),
            handle: Handle(format!("{}@v{}", thing_of(id), card.version)),
            form: Form::Whole,
            unit: None,
        }))
    }
}

/// The judge's recorded `addressed_task` answer to the message `node` of
/// `session`: a task id, or none (its `none` choice, or no answer).
pub fn judged_task(store: &Store, session: &str, node: &str) -> Option<String> {
    use crate::ledger::LedgerRow;
    use theseus_protocol::LedgerKind;
    use theseus_store::pages::ledger_kind_session;
    use theseus_store::Page;
    let page = Page {
        kind: kinds::LEDGER,
        tags: vec![ledger_kind_session(LedgerKind::JudgeCall.as_str(), session)],
        limit: 24,
        ..Page::default()
    };
    let got = store.inner().page(&page).ok()??;
    for r in got.records.iter().rev() {
        let Ok(row) = r.decode::<LedgerRow>() else {
            continue;
        };
        let Ok(j) = serde_json::from_value::<theseus_judge::Judgment>(row.data) else {
            continue;
        };
        if j.pack != crate::judge::inbound::CLASSIFY_PACK
            || j.context["node"].as_str() != Some(node)
        {
            continue;
        }
        return addressed_of(&j);
    }
    None
}

/// A `classify.v4` judgment's `addressed_task` answer, when it is a task.
pub fn addressed_of(j: &theseus_judge::Judgment) -> Option<String> {
    j.answers
        .iter()
        .find(|a| a.question == "addressed_task")
        .and_then(|a| a.ran())
        .map(|v| v.to_string())
        .filter(|v| v.starts_with(super::ID_PREFIX))
}

#[cfg(test)]
#[path = "tests_joined.rs"]
mod tests;
