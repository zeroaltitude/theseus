//! A joined task's card (issue 44, the design's §7): read from records, the
//! task untouched; called in only for the one task a message addresses, and
//! regardless of who is present.

use serde_json::json;

use super::*;
use crate::tests_task_graph::{rig, rows, session, turn};
use crate::view::{Anchor, PointOfView, Present};

/// The last message is cut at 300 tokens (est.) on a character's boundary.
#[test]
fn the_last_message_is_cut_at_its_tokens() {
    let short = "Charted the north side.";
    assert_eq!(cut(short, LAST_TOKENS), (short.to_string(), false));
    let long = "é".repeat(2_000);
    let (text, was) = cut(&long, LAST_TOKENS);
    assert!(was);
    assert!(text.ends_with(CUT));
    assert!(text.len() <= (LAST_TOKENS * super::super::view::BYTES_PER_TOKEN) as usize + CUT.len());
}

/// Exactly one task named addresses it; two named address none; with none
/// named, the judge's answer does when it is a task.
#[test]
fn exactly_one_task_named_is_addressed() {
    let tasks = ["tsk_a", "tsk_b"];
    let named = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        addressed(&named(&["task:tsk_a", "thread:lantern"]), None, &tasks),
        Some("tsk_a".into())
    );
    assert_eq!(
        addressed(&named(&["task:tsk_a", "task:tsk_b"]), None, &tasks),
        None
    );
    assert_eq!(
        addressed(&named(&["task:tsk_a", "task:tsk_b"]), Some("tsk_a"), &tasks),
        None
    );
    assert_eq!(addressed(&[], Some("tsk_b"), &tasks), Some("tsk_b".into()));
    assert_eq!(addressed(&[], Some("tsk_gone"), &tasks), None);
    assert_eq!(addressed(&[], None, &tasks), None);
}

struct Named(Vec<String>);

impl Names for Named {
    fn named(&self, _: &str) -> Result<Vec<String>> {
        Ok(self.0.clone())
    }
}

struct Home;

impl Homes for Home {
    fn marks(&self, _: &str) -> Marks {
        Marks {
            place: Some("local".into()),
            audience: Audience::Private,
            author: Author::Theseus,
            outside: false,
            sealed: false,
            veiled: false,
        }
    }
}

fn pov(present: Present) -> PointOfView {
    PointOfView {
        anchor: Anchor::Place("local".into()),
        person: Some("owner".into()),
        present,
        at_hand: None,
        now_ms: theseus_protocol::now_unix_ms(),
    }
}

/// The card says the objective, the state and the plan's step, each as of its
/// time; it is read in every presence state, and reading it writes nothing.
#[tokio::test]
async fn a_joined_tasks_card_is_read_from_records_regardless_of_presence() {
    let r = rig();
    let c = &r.core;
    let s = session(c);
    r.calls(&[(
        "task_create",
        json!({"title": "Chart the outer harbour", "acceptance": ["a chart exists"]}),
    )]);
    turn(c, &s, "plan it").await;
    let t = r.by_title("Chart the outer harbour");
    r.calls(&[(
        "task_split",
        json!({"id": t.id, "version": 1, "into": ["Chart the north side", "Chart the south side"]}),
    )]);
    turn(c, &s, "split it").await;
    let north = r.by_title("Chart the north side");
    r.calls(&[(
        "task_close",
        json!({"id": north.id, "version": 1, "outcome": "done", "evidence": [{"identity": "commit:0a1b2c3d"}]}),
    )]);
    turn(c, &s, "the north side is charted").await;
    let t = r.task(&t.id);

    let card = read(&c.store, &c.kernel, &t.id, 5_000).unwrap().unwrap();
    assert_eq!(card.objective, t.objective);
    assert_eq!(card.version, t.version);
    assert_eq!(card.plan.map(|p| (p.0, p.1)), Some((1, 2)));
    assert!(card.last.is_none(), "a plan item has no session");
    let text = card.text();
    assert!(text.starts_with(HEAD), "{text}");
    assert!(text.contains("Objective (as of "), "{text}");
    assert!(text.contains("step 2 of 2"), "{text}");
    assert!(text.contains("State (as of "), "{text}");
    assert!(read(&c.store, &c.kernel, "tsk_none", 5_000)
        .unwrap()
        .is_none());

    let updated = rows(c, "task.updated").len();
    let names = Named(vec![thing_of(&t.id)]);
    let cards = StoreCards {
        store: &c.store,
        kernel: &c.kernel,
        names: &names,
        homes: &Home,
        judged: None,
    };
    let got = cards
        .joined(&pov(Present::OwnerAlone), "how is the chart going")
        .unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].range.space, Space::Task { task: t.id.clone() });
    assert_eq!(got[0].marks.author, Author::Theseus);
    assert_eq!(got[0].marks.place.as_deref(), Some("local"));
    assert!(got[0].sizes.whole > got[0].sizes.line && got[0].sizes.line > 0);
    // Reading it touched nothing.
    assert_eq!(r.task(&t.id), t);
    assert_eq!(rows(c, "task.updated").len(), updated);

    // Presence does not withhold a named task card.
    for p in [
        Present::Private,
        Present::Shared,
        Present::Open,
        Present::Unknown,
    ] {
        assert_eq!(
            cards
                .joined(&pov(p), "how is the chart going")
                .unwrap()
                .len(),
            1
        );
    }
    // Two tasks named: no address.
    let two = Named(vec![thing_of(&t.id), thing_of(&north.id)]);
    let cards = StoreCards {
        names: &two,
        ..cards
    };
    assert!(cards
        .joined(&pov(Present::OwnerAlone), "both")
        .unwrap()
        .is_empty());
    // None named: the judge's answer calls it in.
    let none = Named(vec![]);
    let cards = StoreCards {
        names: &none,
        judged: Some(north.id.clone()),
        ..cards
    };
    let got = cards
        .joined(&pov(Present::OwnerAlone), "and that one?")
        .unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(
        got[0].range.space,
        Space::Task {
            task: north.id.clone()
        }
    );
}
