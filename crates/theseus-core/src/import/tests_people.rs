//! A tag's people (theseus-wy7y): a fixture history of three made-up people
//! across a Slack DM, a Discord DM and a channel gives three people with
//! their handles and each session's memberships, origin `import`; a dry run
//! counts and writes nothing; a second run writes nothing; a person whose
//! Discord id a DM's person holds is that person; the erase takes them back;
//! a shared place's walk reads none of them; a merge moves them and its undo
//! puts them back; and `import.people` is the owner's, from a private place.

use std::sync::Arc;

use serde_json::json;
use theseus_ontology::{Category, CategoryId, Ontology, Origin, Record};
use theseus_protocol::import::{ImportEpisodesParams, ImportLine, ImportPeopleResult};
use theseus_protocol::{method, OntologyPersonMerged};

use super::tests::{call, call_as};
use super::{episode, session_id_of, write};
use crate::approval::{Client, Surface};
use crate::config::MemoryMode;
use crate::ontology::Walk;
use crate::places::PlaceClass;
use crate::tests_recall::{rig_with, Rig};
use crate::Core;

const TAG: &str = "gull-2026-04";

/// Session `i`'s place and its messages' authors: a Slack DM with Marlo,
/// a Discord DM with Pell, and a channel where both speak with Tamsin and
/// an agent, beside the owner's own words.
fn shape(i: usize) -> (serde_json::Value, Vec<&'static str>) {
    match i % 3 {
        0 => (
            json!({"kind": "dm", "name": "Marlo Quill", "id": "U0GULL01"}),
            vec!["owner", "person:Marlo Quill", "agent:tern"],
        ),
        1 => (
            json!({"kind": "dm", "name": "pell", "id": "500000000000000007"}),
            vec!["person:Pell", "owner"],
        ),
        _ => (
            json!({"kind": "slack-channel", "name": "harbour-log", "id": "C0GULL09"}),
            vec![
                "person:Marlo Quill",
                "person:Tamsin Reef",
                "person:Pell",
                "person:Tamsin Reef",
                "tool",
                "outside",
            ],
        ),
    }
}

fn episode_id(i: usize) -> String {
    format!("ep_{:064x}", 0x9a11_0000 + i)
}

fn sid(i: usize) -> String {
    session_id_of(&episode_id(i))
}

fn episode_line(i: usize) -> String {
    let (place, authors) = shape(i);
    let messages: Vec<serde_json::Value> = authors
        .iter()
        .enumerate()
        .map(|(k, a)| {
            json!({"idx": k, "time": "2026-04-03T09:00:00Z", "author": a,
                "integrity": if *a == "outside" { "outside" } else if a.starts_with("agent:") || *a == "tool" { "agent" } else { "operator" },
                "text": format!("Gull count {i}.{k}: eleven on the breakwater."),
                "unit": format!("unit-{i}-{k}"), "sha256": "ab".repeat(32)})
        })
        .collect();
    let mut v = json!({
        "format": 1, "import_tag": TAG, "episode_id": episode_id(i),
        "source": "openclaw-store", "agent": null, "place": place,
        "as_of": {"start": "2026-04-03T09:00:00Z", "end": "2026-04-03T09:20:00Z"},
        "labels": {"sensitivity": "personal", "topic": []},
        "summary": null, "messages": messages,
    });
    v["hash"] = json!(episode::hash_of(&v));
    serde_json::to_string(&v).unwrap()
}

const N: usize = 9;

fn import(core: &Core) {
    let lines: Vec<ImportLine> = (0..N)
        .map(|i| ImportLine {
            line: i as u64 + 1,
            text: episode_line(i),
        })
        .collect();
    let r = write::import_batch(
        &core.store,
        &ImportEpisodesParams {
            file: "gulls.jsonl".into(),
            lines,
        },
        "test",
    )
    .unwrap();
    assert!(r.rejected.is_empty(), "{:?}", r.rejected);
}

fn rig() -> Rig {
    rig_with(MemoryMode::Canary, |c| c.memory.canary_fraction = 1.0)
}

fn snapshot(c: &Core) -> Arc<Ontology> {
    c.runner.ontology.snapshot(&c.store).unwrap()
}

async fn run(c: &Arc<Core>, dry_run: bool) -> ImportPeopleResult {
    serde_json::from_value(
        call(
            c,
            method::IMPORT_PEOPLE,
            json!({"tag": TAG, "dry_run": dry_run}),
        )
        .await
        .unwrap(),
    )
    .unwrap()
}

fn people(o: &Ontology) -> Vec<(String, Vec<String>)> {
    o.categories()
        .filter(|c| c.kind() == "person")
        .map(|c| (c.name.clone(), theseus_ontology::handles_of(c)))
        .collect()
}

/// A session's people by name.
fn held(o: &Ontology, session: &str) -> Vec<String> {
    o.memberships(session)
        .iter()
        .filter(|m| m.origin == Origin::Import)
        .map(|m| o.category(&m.category).unwrap().name.clone())
        .collect()
}

#[tokio::test]
async fn a_tags_people_are_found_once_each_with_their_handles_and_sessions() {
    let rig = rig();
    let c = &rig.core;
    import(c);
    let dry = run(c, true).await;
    assert_eq!(
        (dry.sessions, dry.people, dry.made, dry.joined, dry.frames),
        (9, 3, 3, 9, 0)
    );
    assert!(people(&snapshot(c)).is_empty(), "a dry run writes nothing");

    let r = run(c, false).await;
    assert_eq!((r.people, r.made, r.held, r.joined), (3, 3, 0, 9));
    // Marlo by name and Slack id, Pell by name and Discord id (the DM's
    // "pell" and the author "Pell" are one: they spoke in that DM alone),
    // and Tamsin by name alone. The owner, the agent, a tool and outside
    // text are no one's.
    let o = snapshot(c);
    let mut got = people(&o);
    got.sort();
    assert_eq!(
        got,
        [
            (
                "Marlo Quill".to_string(),
                vec!["slack:U0GULL01".to_string(), "name:Marlo Quill".to_string()]
            ),
            (
                "Tamsin Reef".to_string(),
                vec!["name:Tamsin Reef".to_string()]
            ),
            (
                "pell".to_string(),
                vec![
                    "discord:500000000000000007".to_string(),
                    "name:pell".to_string()
                ]
            ),
        ]
    );
    assert_eq!(held(&o, &sid(0)), ["Marlo Quill"]);
    assert_eq!(held(&o, &sid(1)), ["pell"]);
    // The channel: by messages, Tamsin (2) first.
    assert_eq!(held(&o, &sid(2)), ["Tamsin Reef", "Marlo Quill", "pell"]);

    // A second run writes nothing.
    let again = run(c, false).await;
    assert_eq!(
        (again.made, again.held, again.joined, again.frames),
        (0, 3, 0, 0)
    );

    // A shared place's walk reads none of them; a private one reads them,
    // and composes a person's guidance line.
    let marlo = o
        .categories()
        .find(|c| c.name == "Marlo Quill")
        .unwrap()
        .id
        .clone();
    c.runner
        .ontology
        .write(
            &c.store,
            vec![Record::Guidance(theseus_ontology::Guidance::new(
                marlo,
                "Runs the gull survey's counts; send tallies to the harbour log.",
                1,
                "the operator",
            ))],
            Origin::Operator,
            |_| Ok(vec![]),
        )
        .unwrap();
    let o = snapshot(c);
    let shared = Walk::of(o.clone(), &sid(2), None, PlaceClass::Shared, 1).unwrap();
    assert!(shared.current.is_empty());
    let private = Walk::of(o, &sid(2), None, PlaceClass::Private, 1).unwrap();
    assert_eq!(private.current.len(), 3);
    let c2 = private.compose(&private.current);
    let shared_c = shared.compose(&private.current);
    assert!(c2.skipped.is_empty(), "{:?}", c2.skipped);
    assert!(
        c2.render().contains("Marlo Quill: Runs the gull survey"),
        "{}",
        c2.render()
    );
    assert!(shared_c.render().is_empty(), "the place rule comes first");
}

#[tokio::test]
async fn a_dms_person_holding_the_id_is_used_and_the_erase_takes_the_rest_back() {
    let rig = rig();
    let c = &rig.core;
    // Pell's DM was bound before: the transport's person holds the id.
    let dm = Category::new(
        CategoryId::new("person", "500000000000000007").unwrap(),
        "@pell",
        "transport",
    );
    c.runner
        .ontology
        .write(
            &c.store,
            vec![Record::Category(dm.clone())],
            Origin::Transport,
            |_| Ok(vec![]),
        )
        .unwrap();
    import(c);
    let r = run(c, false).await;
    assert_eq!((r.people, r.made, r.held), (3, 2, 1));
    let o = snapshot(c);
    assert_eq!(o.memberships(&sid(1))[0].category, dm.id);

    let e: theseus_protocol::import::ImportEraseResult = serde_json::from_value(
        call(c, method::IMPORT_ERASE, json!({"tag": TAG}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!((e.memberships, e.people), (9, 2));
    let o = snapshot(c);
    let left: Vec<String> = people(&o).into_iter().map(|(n, _)| n).collect();
    assert_eq!(left, ["@pell"], "the DM's own person stays");
    assert!(o.memberships(&sid(2)).is_empty());
    // A restart reads the same.
    c.runner.ontology.forget();
    assert_eq!(people(&snapshot(c)).len(), 1);
}

#[tokio::test]
async fn a_merge_moves_a_person_and_its_undo_puts_it_back() {
    let rig = rig();
    let c = &rig.core;
    import(c);
    run(c, false).await;
    let o = snapshot(c);
    let id_of = |name: &str| {
        o.categories()
            .find(|c| c.name == name)
            .unwrap()
            .id
            .to_string()
    };
    let (tamsin, marlo) = (id_of("Tamsin Reef"), id_of("Marlo Quill"));
    let m: OntologyPersonMerged = serde_json::from_value(
        call(
            c,
            method::ONTOLOGY_PERSON_MERGE,
            json!({"absorbed": tamsin, "survivor": "Marlo Quill"}),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(m.sessions, 3);
    assert!(m.survivor.handles.contains(&"name:Tamsin Reef".to_string()));
    let o = snapshot(c);
    assert_eq!(held(&o, &sid(2)), ["Marlo Quill", "pell"]);
    c.runner.ontology.forget();
    assert!(snapshot(c)
        .category(&CategoryId::parse(&tamsin).unwrap())
        .is_none());

    let u: OntologyPersonMerged = serde_json::from_value(
        call(
            c,
            method::ONTOLOGY_PERSON_MERGE,
            json!({"absorbed": tamsin, "survivor": "", "undo": true}),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert!(u.undone);
    let o = snapshot(c);
    assert_eq!(
        o.category(&CategoryId::parse(&marlo).unwrap())
            .unwrap()
            .handles,
        ["slack:U0GULL01", "name:Marlo Quill"]
    );
    assert_eq!(held(&o, &sid(2)), ["Tamsin Reef", "Marlo Quill", "pell"]);
    // A second undo has nothing to undo.
    assert!(call(
        c,
        method::ONTOLOGY_PERSON_MERGE,
        json!({"absorbed": tamsin, "survivor": "", "undo": true}),
    )
    .await
    .is_err());
}

#[tokio::test]
async fn a_person_added_with_a_held_handle_joins_that_person() {
    let rig = rig();
    let c = &rig.core;
    import(c);
    run(c, false).await;
    let before = people(&snapshot(c)).len();
    let added: theseus_protocol::OntologyCategory = serde_json::from_value(
        call(
            c,
            method::ONTOLOGY_CATEGORY_ADD,
            json!({"kind": "person", "name": "M. Quill", "handles": ["slack:U0GULL01", "email:MQ@Harbour.Example"],
                   "description": "Runs the gull survey's counts."}),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(added.name, "Marlo Quill");
    assert!(added
        .handles
        .contains(&"email:mq@harbour.example".to_string()));
    assert!(added.handles.contains(&"name:M. Quill".to_string()));
    assert_eq!(people(&snapshot(c)).len(), before);
    // A display name alone makes a person of its own.
    call(
        c,
        method::ONTOLOGY_CATEGORY_ADD,
        json!({"kind": "person", "name": "Marlo Quill", "handles": ["name:Marlo Quill"]}),
    )
    .await
    .unwrap();
    assert_eq!(people(&snapshot(c)).len(), before + 1);
}

#[tokio::test]
async fn people_from_no_private_place_are_refused() {
    let rig = rig();
    let c = &rig.core;
    import(c);
    let shared = Client::new("test", Surface::Unnamed);
    let e = call_as(c, shared, method::IMPORT_PEOPLE, json!({"tag": TAG}))
        .await
        .unwrap_err();
    assert_eq!(e.0, theseus_protocol::error_code::REFUSED, "{e:?}");
    assert!(people(&snapshot(c)).is_empty());
}
