//! A row names who answered by the surface's own judgement (issue 29): the author a request names is taken only
//! from the Discord binding, which read the press. On the terminal and the web UI any process of the owner's own
//! user could name anyone, so the label there is the surface's own and `via` says where the answer came from.
//! `work.answer` and `action.confirm`, the two ways a question is answered.

use serde_json::{json, Value};
use theseus_protocol::method;

use super::tests::{change, rig};
use crate::approval::Surface;
use crate::tests_m3::{rpc_as, surface};

const OWNER: u64 = 4_100_000_000_000_000_001;
const CEILING: &str = "kernel.daily_spend_ceiling_usd";

async fn declined_by(
    method: &str,
    client: crate::approval::Client,
    named: &str,
    origin: Option<Value>,
) -> String {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change(CEILING, json!(300))).await;
    let q = r.asked().pop().expect("a card waits").correlation_id;
    let mut p = match method {
        method::ACTION_CONFIRM => json!({"correlation_id": q, "approve": false, "author": named}),
        _ => json!({"question_id": q, "option": "decline", "author": named}),
    };
    if let Some(o) = origin {
        p["discord"] = o;
    }
    rpc_as(&r.core, client, method, p).await.unwrap();
    let a = r.core.kernel.action(&q).unwrap().unwrap();
    a.resolution.unwrap_or_default()
}

/// From the terminal and the web UI a named author is not taken: the label is the surface's own.
#[tokio::test]
async fn a_named_author_is_not_taken_on_the_terminal_or_the_web_ui() {
    for m in [method::ACTION_CONFIRM, method::WORK_ANSWER] {
        for (s, label) in [(Surface::Cli, "the CLI"), (Surface::Web, "the web UI")] {
            let said = declined_by(m, surface("x#1", s), "discord:someone-else", None).await;
            assert!(said.contains(label), "{m} {label}: {said}");
            assert!(!said.contains("someone-else"), "{m} {label}: {said}");
        }
    }
}

/// From the Discord binding it is the person it read off the press.
#[tokio::test]
async fn the_binding_names_the_person_it_read() {
    let origin = json!({"user_id": OWNER.to_string(), "channel_id": (OWNER + 1).to_string(), "guild_id": null});
    for m in [method::ACTION_CONFIRM, method::WORK_ANSWER] {
        let said = declined_by(
            m,
            surface("discord#1", Surface::Discord),
            "discord:dockhand",
            Some(origin.clone()),
        )
        .await;
        assert!(said.contains("discord:dockhand"), "{m}: {said}");
    }
}
