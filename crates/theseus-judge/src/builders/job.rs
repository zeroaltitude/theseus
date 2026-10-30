//! The job message builder (theseus-9f72b, design §2's closing paragraph): one
//! function that turns a background job into the same three-question ask every
//! other site makes. A one-paragraph `message` describing the job's work,
//! `place_kind = "job"`, and the route facts.

use serde::{Deserialize, Serialize};

use super::{inbound, InboundInput, Prepared, RouteFacts};
use crate::state::Scrub;

/// What the job is.
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

/// The job's `inbound` input.
pub fn job_input(_i: &JobInput) -> InboundInput {
    InboundInput::default()
}

/// The job's state and dynamic items, on the route pack's builder.
pub fn job(i: &JobInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    inbound(&job_input(i), cap, scrub)
}
