//! The three states of the route family (theseus-h66v4.6, the owner's 2026-10-30 16:01 answer
//! 1: "three distinct jev calls with noticeably different contexts for each"). Each slice reads
//! the one [`InboundInput`] the daemon builds for a message and keeps only what its question
//! needs, so a pack's state is its own and its static header is the only thing it shares.
//!
//! - **route** (`route.v10`, the model): the message, who sent it and where, the previous human
//!   message and the last reply, and the daemon's facts (`current_model`, `switch_cost`,
//!   `restricted_models`, `price_period`, with `est_tokens`, `last_reply_model` and a task's
//!   `task_models` that the carried model blocks read). The persona and thread fields ride it
//!   too, because the pack still asks `persona`, `seal` and `personal` (they were never one of the
//!   owner's three calls, and moving them is not this row's).
//! - **effort** (`effort.v1`): the message, the previous human message and the last reply, and
//!   nothing else.
//! - **work** (`work.v1`): the message, the live tasks and where the message came in.

use serde_json::{json, Value};
use std::sync::Arc;

use super::{
    first_line, left_out, persona, route_facts, share, Clipper, InboundInput, Prepared, LIVE_TASKS,
};
use crate::pack::Dynamic;
use crate::state::{clip_with, Keep, Scrub, StateBuilder};

/// The keys of the `effort` slice when the message has a previous message and a reply.
pub const EFFORT_STATE_KEYS: [&str; 3] = ["last_reply", "message", "previous_human_message"];

/// The keys of the `work` slice when the message is in a place with live tasks.
pub const WORK_STATE_KEYS: [&str; 3] = ["live_tasks", "message", "place_kind"];

fn message(b: &mut StateBuilder, cap: u64, i: &InboundInput) {
    b.text("message", 10, share(cap, 30), Keep::Both, &i.message);
}

fn recent(b: &mut StateBuilder, cap: u64, i: &InboundInput) {
    b.opt_text(
        "previous_human_message",
        7,
        share(cap, 15),
        Keep::Both,
        i.previous_human_message.as_deref(),
    )
    .opt_text(
        "last_reply",
        6,
        share(cap, 15),
        Keep::Both,
        i.last_reply.as_deref(),
    );
}

/// `route.v10`'s state.
pub fn route(i: &InboundInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let clip = |s: &str, n: usize| clip_with(scrub, s, n);
    let mut b = StateBuilder::new("route", super::ROUTE_SLICE_VERSION, cap, scrub);
    message(&mut b, cap, i);
    b.scalar("author", c.clip(&i.author, 40))
        .cut_if("author", c.cut())
        .scalar("place_kind", c.clip(&i.place_kind, 40))
        .cut_if("place_kind", c.cut());
    recent(&mut b, cap, i);
    if i.in_thread {
        b.scalar("in_thread", true);
    }
    let in_thread = persona::state(&mut b, &c, cap, i);
    route_facts::state(&mut b, &c, i);
    let mut dynamic = Dynamic::default();
    persona::dynamic(&mut dynamic, i, &clip, in_thread);
    Prepared {
        state: Arc::new(b.build()),
        dynamic,
    }
}

/// `effort.v1`'s state.
pub fn effort(i: &InboundInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let mut b = StateBuilder::new("effort", super::EFFORT_SLICE_VERSION, cap, scrub);
    message(&mut b, cap, i);
    recent(&mut b, cap, i);
    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}

/// `work.v1`'s state.
pub fn work(i: &InboundInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let tasks = &i.live_tasks[..i.live_tasks.len().min(LIVE_TASKS)];
    let mut b = StateBuilder::new("work", super::WORK_SLICE_VERSION, cap, scrub);
    message(&mut b, cap, i);
    b.scalar("place_kind", c.clip(&i.place_kind, 40))
        .cut_if("place_kind", c.cut());
    let items: Vec<Value> = tasks
        .iter()
        .map(|t| {
            json!({
                "id": c.clip(&t.id, 40),
                "brief": c.clip(first_line(&t.brief), 160),
                "state": c.clip(&t.state, 30),
            })
        })
        .collect();
    b.list_head(
        "live_tasks",
        8,
        share(cap, 20),
        items,
        left_out(&i.live_tasks, tasks.len()),
    )
    .cut_if("live_tasks", c.cut());
    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}
