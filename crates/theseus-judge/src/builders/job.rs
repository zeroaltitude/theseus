//! The job message builder (theseus-9f72b, design §2's closing paragraph): one
//! function that turns a background job into the same three-question ask every
//! other site makes. A one-paragraph `message` describing the job's work,
//! `place_kind = "job"`, and the route facts.

use serde::{Deserialize, Serialize};

use super::{inbound, InboundInput, Prepared, RestrictedModels, RouteFacts};
use crate::state::Scrub;

/// The most characters of a free-text part of a job's paragraph.
const PART_CHARS: usize = 1_500;
/// The most labels a compaction's paragraph names, and a label's length.
const LABELS: usize = 12;
const LABEL_CHARS: usize = 40;

/// What the job is. Each kind says whether it works on private material:
/// owner material (a synthesis's sources, a motif drawn from private
/// sessions), which only a model of US origin may read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "job", rename_all = "snake_case")]
pub enum JobKind {
    Compaction {
        oldest_tokens: u64,
        summary_tokens: u64,
        labels: Vec<String>,
    },
    Synthesis {
        subject: String,
        sources: u32,
        owner_material: bool,
    },
    SelfBuilder {
        brief: String,
        private: bool,
    },
    SelfReviewer {
        diff_summary: String,
        builder_model: Option<String>,
        private: bool,
    },
}

/// A job and the facts to ask it with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobInput {
    pub kind: JobKind,
    pub facts: RouteFacts,
}

/// A part of the paragraph: its words on one line, cut at `max` characters.
fn part(text: &str, max: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(max) {
        Some((at, _)) => format!("{}\u{2026}", &line[..at]),
        None => line,
    }
}

impl JobKind {
    /// The one-paragraph message: the work, in the words the route pack reads.
    fn message(&self) -> String {
        match self {
            JobKind::Compaction {
                oldest_tokens,
                summary_tokens,
                labels,
            } => {
                let labels: Vec<String> = labels
                    .iter()
                    .take(LABELS)
                    .map(|l| part(l, LABEL_CHARS))
                    .collect();
                let labels = if labels.is_empty() {
                    "none".to_string()
                } else {
                    labels.join(", ")
                };
                format!(
                    "Summarize the oldest {oldest_tokens} tokens of this session's ring into at \
                     most {summary_tokens} tokens; the session's labels: {labels}."
                )
            }
            JobKind::Synthesis {
                subject,
                sources,
                owner_material,
            } => {
                let own = if *owner_material {
                    " The sources are the owner's own material."
                } else {
                    ""
                };
                format!(
                    "Write a synthesis of {sources} sources about {}, a note the assistant keeps \
                     and reads later in place of the sources.{own}",
                    part(subject, PART_CHARS)
                )
            }
            JobKind::SelfBuilder { brief, .. } => format!(
                "Write a change to the assistant's own code from this motif: {}",
                part(brief, PART_CHARS)
            ),
            JobKind::SelfReviewer { diff_summary, .. } => format!(
                "Review this change to the assistant's own code before it merges: {}",
                part(diff_summary, PART_CHARS)
            ),
        }
    }

    /// The job works on private material: only US options may be listed.
    fn private(&self) -> bool {
        match self {
            JobKind::Compaction { .. } => false,
            JobKind::Synthesis { owner_material, .. } => *owner_material,
            JobKind::SelfBuilder { private, .. } | JobKind::SelfReviewer { private, .. } => {
                *private
            }
        }
    }
}

/// The job's `inbound` input: its paragraph as the message, `job` as the
/// place, and the facts. Private material floors `restricted_models` at
/// `not_allowed` whatever the facts say, and nothing here ever loosens it. A
/// reviewer's state names the builder's model beside the task's.
pub fn job_input(i: &JobInput) -> InboundInput {
    let mut facts = i.facts.clone();
    if i.kind.private() {
        facts.restricted_models = RestrictedModels::NotAllowed;
    }
    if let JobKind::SelfReviewer {
        builder_model: Some(m),
        ..
    } = &i.kind
    {
        if !facts.task_models.contains(m) {
            facts.task_models.insert(0, m.clone());
        }
    }
    InboundInput {
        message: i.kind.message(),
        author: "daemon".into(),
        place_kind: "job".into(),
        route: Some(facts),
        ..InboundInput::default()
    }
}

/// The job's state and dynamic items, on the route pack's builder: the same
/// state every other site asks, for the three questions.
pub fn job(i: &JobInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    inbound(&job_input(i), cap, scrub)
}
