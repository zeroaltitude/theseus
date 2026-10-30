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
    /// The owner's card: statements about how the owner works, from clusters
    /// of recent activity (`card/run.rs`).
    Card {
        clusters: u32,
    },
    /// The card's observation of what the owner wrote in a session
    /// (`card/observe.rs`): short observations about how the owner works.
    CardObserve {
        lines: u32,
    },
    /// A message the assistant speaks first, in its own voice
    /// (`initiative/speak.rs`).
    Initiative {
        kind: String,
    },
    /// The names of the people a conversation mentions (`[people]`).
    People {
        lines: u32,
    },
    /// The card of an imported conversation (`[threads] import_profile`).
    Threads {
        episodes: u32,
    },
    /// A rewrite of one judge pack's wording (`[judge.learn]`).
    Learn {
        pack: String,
    },
    /// The name of a new ontology category (`[ontology] name_profile`).
    OntologyName {
        kind: String,
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
            JobKind::Card { clusters } => format!(
                "Read {clusters} clusters of the owner's recent activity and state, for each, \
                 what it shows about how the owner works, as short statements for the owner's \
                 card."
            ),
            JobKind::CardObserve { lines } => format!(
                "Read {lines} lines the owner wrote in one conversation and note what they \
                 show about how the owner works, as short observations for the owner's card."
            ),
            JobKind::Initiative { kind } => format!(
                "Write a short message in the assistant's own voice, speaking first to the \
                 owner, about {}.",
                part(kind, PART_CHARS)
            ),
            JobKind::People { lines } => format!(
                "Read {lines} lines of one conversation and list the names of the people it \
                 mentions, each with the line that names it."
            ),
            JobKind::Threads { episodes } => format!(
                "Write the card of an imported conversation from {episodes} episodes of it: a \
                 few short lines, each citing the episode it rests on."
            ),
            JobKind::Learn { pack } => format!(
                "Rewrite the wording of the judge's pack {} so the mistakes the owner labelled \
                 are not repeated, keeping every id, option and threshold.",
                part(pack, PART_CHARS)
            ),
            JobKind::OntologyName { kind } => format!(
                "Name a new category of {} for the owner's ontology: three names and a line \
                 that says what it holds.",
                part(kind, PART_CHARS)
            ),
        }
    }

    /// The job works on private material: only US options may be listed.
    pub fn private(&self) -> bool {
        match self {
            JobKind::Compaction { .. } => false,
            JobKind::Synthesis { owner_material, .. } => *owner_material,
            JobKind::SelfBuilder { private, .. } | JobKind::SelfReviewer { private, .. } => {
                *private
            }
            // Each of these reads the owner's own material (the keys of
            // `config::data_floor::owner_material_profiles`).
            JobKind::Card { .. }
            | JobKind::CardObserve { .. }
            | JobKind::Initiative { .. }
            | JobKind::People { .. }
            | JobKind::Threads { .. }
            | JobKind::Learn { .. }
            | JobKind::OntologyName { .. } => true,
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
