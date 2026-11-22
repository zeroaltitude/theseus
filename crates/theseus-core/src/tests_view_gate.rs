//! The gate (issue 44) through the whole core, after "anything goes" (2026-11-22): who is present withholds
//! nothing now, and these are what stays of it. The owner's mark as the store reads it, and the reading of
//! it that a task's objective change requires (`task_graph::words::present_in_person`: authority, not
//! privacy); and a refused submission lends its mark to nothing.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::json;
use theseus_memory::gate::Present;
use theseus_memory::recall::Place;

use crate::places::BoundPlace;
use crate::tests_memory_lookup::script;
use crate::tests_places::{rig_scripted, session, Rig, LAB};
use crate::view_gate::{self, OwnerMarks};
use crate::Core;

/// A guild channel bound shared.
const PIER: u64 = 141_421_356_237_309_504;

/// The marks of a fake ledger: a node's in-person answer, or a read error.
struct Fake(BTreeMap<&'static str, Option<bool>>);

impl OwnerMarks for Fake {
    fn in_person(&self, node: &str) -> Result<Option<bool>> {
        match self.0.get(node) {
            Some(Some(b)) => Ok(Some(*b)),
            Some(None) => anyhow::bail!("the mark of {node} cannot be read"),
            None => Ok(None),
        }
    }
}

fn fake() -> Fake {
    Fake(BTreeMap::from([
        ("his", Some(true)),
        ("agent", Some(false)),
        ("broken", None),
    ]))
}

fn bind(core: &Core) {
    core.bind_places(vec![
        BoundPlace {
            persona: None,
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: true,
            guild: Some("100000000000000001".into()),
            ceiling: None,
        },
        BoundPlace {
            persona: None,
            target: format!("discord:channel:{PIER}"),
            name: "#pier".into(),
            private: false,
            guild: Some("100000000000000001".into()),
            ceiling: None,
        },
    ]);
}

fn rig() -> Rig {
    let r = rig_scripted(|_| {}, script);
    bind(&r.core);
    r
}

/// The owner alone is the terminal, the web page or a direct conversation, and his in-person mark: the
/// reading a task's objective change requires (`task_graph::words::present_in_person`, `Why::NotAlone`).
#[test]
fn owner_alone_is_the_terminal_the_page_or_a_dm_and_his_mark() {
    use crate::task_graph::words::present_in_person;
    let f = fake();
    let dm = "discord:dm:7";
    let lab = format!("discord:channel:{LAB}");
    let p = |place: &Place, target: Option<&str>, node| present_in_person(place, target, node, &f);
    assert_eq!(p(&Place::Private, None, "his"), Present::OwnerAlone);
    assert_eq!(p(&Place::Private, Some(dm), "his"), Present::OwnerAlone);
    // An agent's process at the terminal is not alone; nor is a message with
    // no mark, nor one whose mark cannot be read.
    assert_eq!(p(&Place::Private, None, "agent"), Present::PrivateChannel);
    assert_eq!(
        p(&Place::Private, None, "unmarked"),
        Present::PrivateChannel
    );
    assert_eq!(p(&Place::Private, None, "broken"), Present::Unknown);
    // A channel is never alone, whoever is in it.
    assert_eq!(
        p(&Place::Private, Some(&lab), "his"),
        Present::PrivateChannel
    );
    assert_eq!(
        p(&Place::Shared(lab.clone()), Some(&lab), "his"),
        Present::Shared
    );
    assert_eq!(p(&Place::Unknown, None, "his"), Present::Unknown);
}

#[tokio::test]
async fn the_store_reads_the_owners_mark_as_step_one_writes_it() {
    let r = rig();
    let said = |node: &str, in_person: bool| {
        json!({"node": node, "in_person": in_person, "owner_handle": null, "peer": "person",
               "agent": null, "surface": "cli", "place": "local", "arrived_session": "ses_a",
               "ran_session": "ses_a", "author": null, "at_ms": 1})
    };
    r.core
        .store
        .put_meta("said.nod_his", &said("nod_his", true))
        .unwrap();
    r.core
        .store
        .put_meta("said.nod_agent", &said("nod_agent", false))
        .unwrap();
    let s = &r.core.store;
    assert_eq!(s.in_person("nod_his").unwrap(), Some(true));
    assert_eq!(s.in_person("nod_agent").unwrap(), Some(false));
    assert_eq!(s.in_person("nod_none").unwrap(), None);
}

/// A turn of `sid` whose input `turn.submit` marked as typed in person or by
/// an agent's process.
async fn turn_as(core: &std::sync::Arc<Core>, sid: &str, input: &str, in_person: bool) {
    use crate::session::SessionRecord;
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let mark = view_gate::test_mark(&core.runner, sid, in_person);
    let sink = crate::bus::EventSink::new(core.bus.clone(), sid, None).with_arrival(Some(mark));
    core.runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "cli".into(),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

/// The ledger's F1: an owner's submission that is refused before it runs leaves nothing behind, so an
/// agent's identical text after it carries the agent's own mark, and is not read as the owner alone.
#[tokio::test]
async fn a_refused_owners_submission_lends_nothing_to_an_agents_identical_text() {
    let r = rig();
    let desk = session(&r.core, None);
    let text = "look up the harbour gate in my other conversations";
    // The owner's submission, marked, refused before its turn ran: its sink
    // is dropped with its mark.
    let owner = view_gate::test_mark(&r.core.runner, &desk, true);
    drop(crate::bus::EventSink::new(r.core.bus.clone(), &desk, None).with_arrival(Some(owner)));
    turn_as(&r.core, &desk, text, false).await;
    let node = (r.core.store.session_nodes(&desk).unwrap().into_iter())
        .find_map(|(_, n)| match &n.body {
            crate::node::Body::UserMessage { text: t, .. } if t == text => Some(n.id.clone()),
            _ => None,
        })
        .expect("the agent's message");
    assert_eq!(r.core.store.in_person(&node).unwrap(), Some(false));
    let place = r.core.runner.place_of(&desk);
    assert_eq!(
        crate::task_graph::words::present_in_person(&place, None, &node, &r.core.store),
        Present::PrivateChannel
    );
}
