//! The exclusions (theseus-0p1r), with a fake model and a fake Jev: a held
//! "@name" folds to its bare name; the owner (his DM person, which the
//! place rule binds with no `[places] owner`) is never a candidate by his
//! first name, stays a `match` option, and a match to his person is not
//! listed; the agents the store knows (an episode's agent, the name in an
//! agent's imported `IDENTITY.md`) and the house's names are excluded
//! before any call; a role line carrying a person's pay or leave is
//! dropped; the proposals made before an exclusion are hidden at read time,
//! counted, never taken in bulk and never deleted; and the people import
//! declares no agent or bot. Every name here is invented.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::builders::PersonCandidate;
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::import::{ImportEpisodesParams, ImportLine, ImportPeopleParams};
use theseus_protocol::{
    method, OntologyCategoryAddParams, OntologyProposalAcceptAllParams, OntologyProposalsParams,
};
use theseus_store::{kinds, NewRecord};

use super::house::identity_names;
use super::tests::{answer, people_on};
use super::{fold, Line, NotPeople, SCOPE};
use crate::import::{episode, write};
use crate::ledger::LedgerRow;
use crate::store::Store;
use crate::tests_categorize::{rig, Rig, OWNER};
use crate::Core;

const TAG: &str = "skerry-2026-06";

fn cand(name: &str) -> PersonCandidate {
    PersonCandidate {
        name: name.into(),
        handles: vec![],
        role_line: String::new(),
        evidence: vec![],
    }
}

#[test]
fn a_handles_sigil_folds_away_and_software_names_are_excluded() {
    assert_eq!(fold("@Sable"), "sable");
    assert_eq!(fold("  @sable   THORN "), "sable thorn");
    assert_eq!(fold("@"), "");
    let not =
        NotPeople::of(&crate::config::Config::example(), &[]).with_names(&["gull".to_string()]);
    for name in [
        "Jev",
        "theseus",
        "OpenClaw",
        "bot-of-the-harbour",
        "Gull bot",
        "openclaw-control-ui",
    ] {
        assert!(not.excludes(&cand(name)), "{name}");
    }
    assert!(!not.excludes(&cand("Wren Halloway")));
    assert!(
        !not.excludes(&cand("Gull Halloway")),
        "a house word alone is no software"
    );
    assert_eq!(
        identity_names(
            "# IDENTITY.md\n\n- **Name:** Tern\n- **Creature:** a gull\n- **Avatar:** *(pending)*"
        ),
        ["Tern"]
    );
    assert!(identity_names("- **Name:** (pick something you like)").is_empty());
    assert!(identity_names("Names: many, of the tide").is_empty());
}

/// Session `i`'s episode: 0, a channel where the owner, an agent and two
/// authors that are no people speak (agent `gull`); 1, the agent
/// `harbour`'s identity file, which names it Tern.
fn episode_line(i: usize) -> String {
    let (place, agent, msgs): (serde_json::Value, &str, Vec<(&str, &str)>) = match i {
        0 => (
            json!({"kind": "slack-channel", "name": "skerry-log", "id": "C0SKERRY"}),
            "gull",
            vec![
                (
                    "owner",
                    "Ask Tern to send the gauge notes to Wren; Sable's are on the shelf.",
                ),
                (
                    "agent:gull",
                    "Done. Wren owns the rota; Orrin is owed the March invoice and is on leave.",
                ),
                ("person:Wren Halloway", "On it."),
                ("person:Tern", "Notes sent."),
                ("person:bot-of-the-harbour", "Tide alert."),
            ],
        ),
        _ => (
            json!({"kind": "file", "name": "agents/harbour/IDENTITY.md"}),
            "harbour",
            vec![(
                "tool",
                "# IDENTITY.md\n\n- **Name:** Tern\n- **Creature:** a gull",
            )],
        ),
    };
    let messages: Vec<serde_json::Value> = msgs
        .iter()
        .enumerate()
        .map(|(k, (a, t))| {
            json!({"idx": k, "time": "2026-06-03T09:00:00Z", "author": a,
                "integrity": if a.starts_with("agent:") || *a == "tool" { "agent" } else { "operator" },
                "text": t, "unit": format!("unit-{i}-{k}"), "sha256": "ef".repeat(32)})
        })
        .collect();
    let mut v = json!({
        "format": 1, "import_tag": TAG, "episode_id": format!("ep_{:064x}", 0x0a1b_0000 + i),
        "source": "openclaw-store", "agent": agent, "place": place,
        "as_of": {"start": "2026-06-03T09:00:00Z", "end": "2026-06-03T09:20:00Z"},
        "labels": {"sensitivity": "personal", "topic": []},
        "summary": null, "messages": messages,
    });
    v["hash"] = json!(episode::hash_of(&v));
    serde_json::to_string(&v).unwrap()
}

fn import(core: &Core, i: usize) {
    let r = write::import_batch(
        &core.store,
        &ImportEpisodesParams {
            file: "skerry.jsonl".into(),
            lines: vec![ImportLine {
                line: i as u64 + 1,
                text: episode_line(i),
            }],
        },
        "test",
    )
    .unwrap();
    assert!(r.rejected.is_empty(), "{:?}", r.rejected);
}

fn hold(core: &Core, name: &str, handles: &[&str]) -> String {
    let handle = handles[0];
    core.ontology_category_add(
        &OntologyCategoryAddParams {
            kind: Some("person".into()),
            name: name.into(),
            handles: handles.iter().map(|h| h.to_string()).collect(),
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    o.person_by_handle(handle).unwrap().id.local().to_string()
}

fn rows(store: &Store, kind: &str) -> Vec<LedgerRow> {
    store
        .scope_after(SCOPE, 0)
        .unwrap()
        .into_iter()
        .filter_map(|r| r.decode::<LedgerRow>().ok())
        .filter(|r| r.kind == kind)
        .collect()
}

async fn until_rows(store: &Store, kind: &str, n: usize) -> Vec<LedgerRow> {
    let t0 = Instant::now();
    loop {
        let r = rows(store, kind);
        if r.len() >= n {
            return r;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{} of {n} {kind} rows",
            r.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn script(jev: &FakeJev) {
    jev.script("real", Jev::Noul(0.95));
    jev.script("involved", Jev::Noul(0.93));
    jev.script("evaluative", Jev::Noul(0.05));
    jev.script("sensitive", Jev::Noul(0.05));
    jev.script(
        "match",
        Jev::Choice {
            option: "new_person".into(),
            confidence: 0.92,
        },
    );
    jev.script_when(
        "Orrin Vale",
        None,
        "match",
        Jev::Choice {
            option: "orrin-vale".into(),
            confidence: 0.94,
        },
    );
    jev.script_when("Orrin Vale", None, "sensitive", Jev::Noul(0.93));
}

fn listed(core: &Core) -> (Vec<theseus_protocol::OntologyProposal>, u32) {
    let r = core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap();
    (r.proposals, r.hidden)
}

/// The owner's held match as the backfill made it before the exclusions:
/// Wren's judgment again, its match the owner's person.
fn plant_owner_match(core: &Core, owner: &str) {
    let mut row = rows(&core.store, "judge.call")
        .into_iter()
        .find(|r| r.data["context"]["candidate"]["name"] == "Wren Halloway")
        .unwrap();
    let id = "jdg_0p1r0000000000000000000000000000";
    row.data["id"] = json!(id);
    row.data["context"]["candidate"]["name"] = json!("Sable Thorn");
    for a in row.data["answers"].as_array_mut().unwrap() {
        if a["question"] == "match" {
            a["answer"]["choice"] = json!(owner);
        }
    }
    let rec = NewRecord::json(kinds::LEDGER, Some(id), &row)
        .unwrap()
        .scoped(SCOPE);
    core.store.append(&[rec]).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_owner_his_agents_and_the_house_are_never_proposed_and_old_proposals_hide() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    // The owner's config names no `[places] owner`: the place rule's bound
    // DM is his, and his person is held as the transport makes it.
    let r = rig(Some(&jev), 0, |c| {
        people_on(c);
        c.places.owner = None;
    });
    // His person also holds the full name he went by somewhere (a `name:` handle).
    let owner = hold(
        &r.core,
        "@sable",
        &[&format!("discord:{OWNER}"), "name:Sable Thorn"],
    );
    let orrin = hold(&r.core, "Orrin Vale", &["slack:U0ORRIN1"]);
    import(&r.core, 0);
    the_owner_is_excluded(&r.core, &owner, orrin);
    backfill(&r, &jev, &owner).await;
    old_proposals_hide(&r.core, &owner);

    // The people import: Tern (an agent) and the harbour's bot are no people.
    let got = crate::import::tests::call(
        &r.core,
        method::IMPORT_PEOPLE,
        json!({"tag": TAG, "dry_run": true}),
    )
    .await
    .unwrap();
    assert_eq!(
        (got["people"].as_u64(), got["excluded"].as_u64()),
        (Some(1), Some(2)),
        "{got}"
    );
}

/// The owner, by the place rule's DM alone: his person and his first name
/// excluded, and the gate never lists him.
fn the_owner_is_excluded(core: &Core, owner: &str, orrin: String) {
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    let not = core.not_people(&[], &o);
    assert!(
        not.excludes_held(owner),
        "the place rule's owner, no [places] owner"
    );
    assert!(
        not.excludes(&cand("SABLE")),
        "his first name, beside his person's \"@sable\""
    );
    assert!(
        not.excludes(&cand("Thorn")),
        "a word of a name his person holds"
    );
    let said = [Line {
        node: "nod_1".into(),
        author: "owner".into(),
        text: "Sable and Orrin Vale walked the skerry.".into(),
    }];
    let gate: Vec<String> = super::seen::listed(&o, "ses_none", &said, &not)
        .into_iter()
        .map(|(h, _)| h.id)
        .collect();
    assert_eq!(gate, vec![orrin], "the gate never lists the owner");
}

/// The backfill: Sable (his name), Jev (the house's) and Gull (an
/// episode's agent) excluded before any call; Tern not known yet; the
/// owner's person among `match`'s options, so Jev can say that "Marlowe", a
/// name of his the store cannot know, is him.
async fn backfill(r: &Rig, jev: &FakeJev, owner: &str) {
    jev.script_when(
        "\"Marlowe\"",
        None,
        "match",
        Jev::Choice {
            option: owner.into(),
            confidence: 0.95,
        },
    );
    r.fake.script.lock().unwrap().push_back(answer(json!([
        {"name": "Wren Halloway", "handles": [], "role_line": "Takes the north gauge readings.", "evidence": ["L3"]},
        {"name": "Tern", "handles": [], "role_line": "", "evidence": ["L1"]},
        {"name": "Sable", "handles": [], "role_line": "", "evidence": ["L1"]},
        {"name": "Jev", "handles": [], "role_line": "", "evidence": ["L2"]},
        {"name": "Gull", "handles": [], "role_line": "", "evidence": ["L2"]},
        {"name": "Marlowe", "handles": [], "role_line": "", "evidence": ["L1"]},
        {"name": "Orrin Vale", "handles": [], "role_line": "Is owed the March invoice and is on leave next week.",
         "evidence": ["L2"]}
    ])));
    let pass = r
        .core
        .import_people_propose(&ImportPeopleParams {
            tag: TAG.into(),
            dry_run: false,
            propose: true,
            cap_usd: Some(5.0),
        })
        .await
        .unwrap()
        .propose
        .unwrap();
    assert_eq!(
        (pass.candidates, pass.excluded, pass.judged),
        (7, 3, 4),
        "{pass:?}"
    );
    let judged = until_rows(&r.core.store, "judge.call", 4).await;
    let mut names: Vec<&str> = judged
        .iter()
        .map(|j| j.data["context"]["candidate"]["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["Marlowe", "Orrin Vale", "Tern", "Wren Halloway"]);
    // `match`'s options: the held people, the owner's person among them.
    let options: Vec<Vec<String>> = jev
        .seen()
        .iter()
        .filter_map(|s| {
            s.body["questions"]["people.v1/match"]["criteria"]
                .as_object()
                .map(|c| c.keys().cloned().collect())
        })
        .collect();
    assert_eq!(options.len(), 4, "{options:?}");
    for o in &options {
        assert!(
            o.iter().any(|k| k == "orrin-vale"),
            "the options read: {o:?}"
        );
        assert!(
            o.iter().any(|k| k == owner),
            "the owner's person not offered as a match: {o:?}"
        );
    }
}

/// Read: Wren new with her line; Orrin held, his line (pay, leave)
/// dropped; Tern new; Marlowe, Jev's match to the owner, hidden. Then the
/// owner's held match the old backfill made,
/// and Tern once its identity file comes in, are hidden, counted, never
/// taken in bulk, never deleted.
fn old_proposals_hide(core: &Arc<Core>, owner: &str) {
    let (all, hidden) = listed(core);
    assert_eq!((all.len(), hidden), (3, 1), "Marlowe hidden: {all:?}");
    let line = |name: &str| {
        all.iter()
            .find(|p| p.person.as_ref().unwrap().name == name)
            .unwrap()
            .person
            .clone()
            .unwrap()
            .role_line
    };
    assert_eq!(
        line("Wren Halloway").as_deref(),
        Some("Takes the north gauge readings.")
    );
    assert_eq!(
        line("Orrin Vale"),
        None,
        "a role line of pay and leave is dropped"
    );
    plant_owner_match(core, owner);
    let (all, hidden) = listed(core);
    assert_eq!(
        (all.len(), hidden),
        (3, 2),
        "the owner's match hidden: {all:?}"
    );

    import(core, 1);
    let (all, hidden) = listed(core);
    let mut shown: Vec<String> = all
        .iter()
        .map(|p| p.person.as_ref().unwrap().name.clone())
        .collect();
    shown.sort();
    assert_eq!(
        (shown, hidden),
        (
            vec!["Orrin Vale".to_string(), "Wren Halloway".to_string()],
            3
        ),
        "Tern, an agent now, hidden too"
    );

    let done = core
        .ontology_proposal_accept_all(
            &OntologyProposalAcceptAllParams {
                kind: Some("person".into()),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert_eq!(done.accepted.len(), 2, "{done:?}");
    let (all, hidden) = listed(core);
    assert_eq!((all.len(), hidden), (0, 3));
    assert_eq!(rows(&core.store, "judge.call").len(), 5, "nothing deleted");
    assert_eq!(
        rows(&core.store, "judge.label").len(),
        2,
        "the two accepts' labels alone"
    );
}
