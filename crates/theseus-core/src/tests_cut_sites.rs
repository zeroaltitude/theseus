//! Cuts of stored text stop at a known value's edge, through whole cores: a
//! session's title, a task's title and its board view, a capped tool result
//! as stored and as sent, and the helpers that cut a head or a tail. "Whole
//! or absent" means no four characters in a row of the value outside a whole
//! copy of it (a record keeps a value whole; an exit masks it). All values
//! invented.

use serde_json::json;
use theseus_protocol::work::WorkKind;

use crate::provider::{ProviderRequest, Scripted};
use crate::push::WorkScope;
use crate::scrub::{Cuts, Scrubber};
use crate::secrets::Secret;
use crate::session::SessionRecord;
use crate::tests_places::{answers_a_call, result_of, rig_knowing, session, turn, Rig, OWNER};

/// The value the core's board holds.
pub(crate) const V: &str = "kqzv-0417-wjxp-9932-hmfb";

pub(crate) fn whole_or_absent(what: &str, text: &str, value: &str) {
    let rest = text.replace(value, "");
    let chars: Vec<char> = value.chars().collect();
    for w in chars.windows(4) {
        let w: String = w.iter().collect();
        assert!(
            !rest.contains(&w),
            "{what} holds {w:?} of the value: {text:?}"
        );
    }
}

/// A core whose scrubber and board both know `value`, its model answering by
/// `script`, its config changed by `tweak`.
pub(crate) fn knowing_core(
    value: &str,
    tweak: impl FnOnce(&mut crate::config::Config),
    script: impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static,
) -> Rig {
    let r = rig_knowing(tweak, script, vec![(value.into(), "cut_test".into())]);
    r.core.secrets.hold("cut_test", Secret::new(value.into()));
    r
}

/// The text of every tool result a request carries.
pub(crate) fn tool_results(req: &ProviderRequest) -> Vec<String> {
    let mut out = Vec::new();
    for m in &req.messages {
        for b in m["content"].as_array().into_iter().flatten() {
            if b["type"] == "tool_result" {
                out.push(b["content"].to_string());
            }
        }
    }
    out
}

#[tokio::test]
async fn a_session_title_stops_before_a_value_across_sixty() {
    let r = knowing_core(V, |_| {}, |_| Scripted::text("Noted."));
    let sid = session(&r.core, None);
    turn(&r.core, &sid, &format!("{}{V} and more", "x".repeat(50))).await;
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let title = rec.title.unwrap();
    assert_eq!(title, format!("{}…", "x".repeat(50)));
    whole_or_absent("the title", &title, V);
}

#[tokio::test]
async fn a_first_line_stops_before_a_value_holding_a_line_break() {
    let broken = "kqzv-0417-wjxp\n9932-hmfb-ylno";
    let r = knowing_core(broken, |_| {}, |_| Scripted::text("Noted."));
    let sid = session(&r.core, None);
    turn(&r.core, &sid, &format!("note {broken} after")).await;
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let title = rec.title.unwrap();
    assert_eq!(title, "note …");
    whole_or_absent("the title", &title, broken);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_title_and_its_board_view_stop_at_a_known_value() {
    use crate::tests_tasks::{arranged, until, ASK};
    let brief = format!("{}{V} then report", "x".repeat(55));
    let r = knowing_core(
        V,
        |_| {},
        move |req| {
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            if crate::tests_places::last_user(req).contains(ASK) {
                return Scripted::tools(
                    "Starting it.",
                    &[(
                        "t_task",
                        "task_create",
                        json!({"brief": brief, "arrangement": arranged()}),
                    )],
                );
            }
            Scripted::text("Done.")
        },
    );
    r.core.push.ensure(&r.core).await.unwrap();
    let sid = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &sid, ASK).await;
    let titled = || {
        crate::task_graph::all(&r.core.store)
            .unwrap()
            .into_iter()
            .find(|t| t.title.starts_with("xxxx"))
    };
    until("the task's record", || titled().is_some()).await;
    let task = titled().unwrap();
    assert_eq!(task.title, format!("{}…", "x".repeat(55)));
    let board = || r.core.push.work_snapshot(200, &WorkScope::default()).1;
    until("the task on the board", || {
        board().iter().any(|v| v.kind == WorkKind::Task)
    })
    .await;
    for v in &board() {
        whole_or_absent("a view's title", &v.title, V);
    }
}

/// A tool prints the value end to end, so each cut of its capped result
/// would fall inside a copy: the stored result, and the request that carries
/// it, hold whole copies or none.
#[tokio::test]
async fn a_capped_tool_result_keeps_each_value_whole_or_not_at_all() {
    let script = format!("for i in $(seq 1 200); do printf '%s' '{V}'; done");
    let r = knowing_core(
        V,
        |c| c.tools.result_max_chars = 2_000,
        move |req| {
            if answers_a_call(req) {
                return Scripted::text("Done.");
            }
            Scripted::tools(
                "Printing.",
                &[("p1", "proc_run", json!({"argv": ["bash", "-c", script]}))],
            )
        },
    );
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "print it").await;
    let stored = result_of(&r.core, &sid, "p1");
    assert!(stored.contains("not shown"), "the result was cut: {stored}");
    assert!(stored.contains(V), "{stored}");
    whole_or_absent("the stored result", &stored, V);
    let sent: Vec<String> = r.requests().iter().flat_map(tool_results).collect();
    assert!(!sent.is_empty());
    for s in &sent {
        assert!(!s.contains(V), "the request's result holds the value: {s}");
        assert!(
            s.contains("[redacted:cut_test]"),
            "the request's result: {s}"
        );
        whole_or_absent("the request's result", s, V);
    }
}

/// The head and tail helpers each stop at the value's edge.
#[test]
fn every_head_and_tail_helper_stops_at_a_known_value() {
    let cuts = Scrubber::with_values(vec![(V.into(), "cut_test".into())]).cuts();
    // A capped result: the head (60 % of 400 = 240) and the tail's start.
    let text = format!(
        "{}{V}{}{V}{}",
        "x".repeat(230),
        "y".repeat(800),
        "z".repeat(150)
    );
    let (capped, cut) = crate::toolrun::cap(&text, 400, |_| String::new(), &cuts);
    assert!(cut);
    whole_or_absent("cap", &capped, V);
    // Knowing no value, the same cut falls inside both copies.
    let (raw, _) = crate::toolrun::cap(&text, 400, |_| String::new(), &Cuts::none());
    assert!(raw.contains("kqzv") && raw.contains("hmfb"), "{raw}");
    whole_or_absent("head", cuts.head(&text, 240).0, V);
    let tail = &text[cuts.after(&text, text.len() - 160)..];
    assert_eq!(tail, "z".repeat(150));
}

/// A retraction's gist, which its notes and a window's line said since quote, stops before a known value: one
/// crossing its 160 characters, and one holding a line break the joining of its words would change.
#[tokio::test]
async fn a_retractions_gist_stops_before_a_known_value() {
    let broken = "kqzv-0417-wjxp\n9932-hmfb-ylno";
    let cases = [
        (
            V,
            format!("{}{V} and more", "x".repeat(150)),
            format!("{}…", "x".repeat(150)),
        ),
        (broken, format!("note {broken} after"), "note…".to_string()),
    ];
    for (value, said, want) in cases {
        let r = knowing_core(value, |_| {}, |_| Scripted::text("Noted."));
        let sid = session(&r.core, None);
        turn(&r.core, &sid, &said).await;
        let node = (r.core.store.session_nodes(&sid).unwrap().into_iter())
            .map(|(_, n)| n)
            .find(|n| {
                matches!(&n.body, crate::node::Body::UserMessage { text, .. } if text.contains(value))
            })
            .expect("the message")
            .id;
        let p = theseus_protocol::retract::FactRetractParams {
            node,
            why: "a test".into(),
            session_id: None,
        };
        let out = r.core.fact_retract(&p, "cli").unwrap();
        let gist = out.gist.expect("a gist");
        assert_eq!(gist, want);
        whole_or_absent("the gist", &gist, value);
    }
}

/// A retraction's card to the owner, as the outbox stores it, stops before a known value: a task named by 1,300
/// characters and then a 600-character value runs past the card's 1,800, so the cut would fall inside it.
#[tokio::test]
async fn a_retractions_card_stops_before_a_known_value() {
    use crate::graph::{Edge, EdgeKind};
    use crate::node::Node;
    use theseus_protocol::SessionKind;
    let long = V.repeat(25);
    let r = knowing_core(&long, |_| {}, |_| Scripted::text("Noted."));
    r.core.bind_places(vec![crate::places::BoundPlace {
        target: "discord:dm:5150".into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "The relay listens on port 7714.").await;
    let node = (r.core.store.session_nodes(&sid).unwrap().into_iter())
        .map(|(_, n)| n)
        .find(|n| matches!(&n.body, crate::node::Body::UserMessage { .. }))
        .expect("the message")
        .id;
    let mut rec = SessionRecord::new(SessionKind::Task, None);
    rec.title = Some(format!("{}{long} after", "x".repeat(1_300)));
    rec.task = Some(crate::session::TaskOf {
        parent_session: sid.clone(),
        parent_execution: "exe_none".into(),
        by: "call_none".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let rn = Node::recall(&rec.session_id, "turn_t", "rcl_task", "baseline", vec![]);
    let edge = Edge::new(
        EdgeKind::DerivedFrom,
        &rn.id,
        &node,
        crate::graph::VIA_RECALL,
    );
    (r.core.store)
        .append(&[rn.record().unwrap(), edge.record().unwrap()])
        .unwrap();
    let p = theseus_protocol::retract::FactRetractParams {
        node,
        why: "a test".into(),
        session_id: None,
    };
    assert!(r.core.fact_retract(&p, "cli").unwrap().card);
    let cards: Vec<String> = (r
        .core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter())
    .filter(|a| crate::outbox::kind_of(a) == "retraction")
    .map(|a| {
        crate::outbox::body_of(a)["text"]
            .as_str()
            .unwrap()
            .to_string()
    })
    .collect();
    assert_eq!(cards.len(), 1, "{cards:?}");
    let card = &cards[0];
    assert!(card.ends_with(&format!("{}…", "x".repeat(1_300))), "{card}");
    whole_or_absent("the card", card, &long);
}
