//! `people.v1` (theseus-wy7y), with a fake model and a fake Jev: the
//! extraction's tool answer parsed; the owner, an agent and a configured
//! name excluded before any call; the bands; a match to a held person; an
//! evaluative role line dropped; the proposals listed and accepted in bulk
//! (`--kind person`); the backfill's dry run, its stop at the cap and its
//! resume from the tag's mark; and a shared place's exchange end gets
//! nothing while a private one's proposes. Every name here is invented.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::builders::PersonCandidate;
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_ontology::Origin;
use theseus_protocol::import::{ImportEpisodesParams, ImportLine, ImportPeopleParams};
use theseus_protocol::{
    OntologyCategoryAddParams, OntologyProposalAcceptAllParams, OntologyProposalsParams, Usage,
};

use super::extract::parse;
use super::{fold, Line, NotPeople, SCOPE};
use crate::config::{JudgePackConfig, PackMode};
use crate::import::{episode, session_id_of, write};
use crate::judge::categorize::EVERY;
use crate::ledger::LedgerRow;
use crate::provider::Scripted;
use crate::store::Store;
use crate::tests_categorize::{moorings, rig, session};
use crate::Core;

const TAG: &str = "tern-2026-05";
/// The owner's name as the import writes it: a bare author.
const OWNER_NAME: &str = "Sable Thorn";

fn line(n: usize, author: &str, text: &str) -> Line {
    Line {
        node: format!("nod_{n}"),
        author: author.into(),
        text: text.into(),
    }
}

/// The extractor's answer: `people` as its tool's input.
fn answer(people: serde_json::Value) -> Scripted {
    Scripted::tools(
        "",
        &[("tu_1", super::extract::TOOL, json!({"people": people}))],
    )
}

fn cand(name: &str) -> PersonCandidate {
    PersonCandidate {
        name: name.into(),
        handles: vec![],
        role_line: String::new(),
        evidence: vec![],
    }
}

#[test]
fn the_tool_answer_is_parsed_and_checked() {
    let ls = vec![
        line(1, OWNER_NAME, "Send Wren the calibration notes."),
        line(2, "person:Wren Halloway", "I'll take the north gauge."),
    ];
    let blocks = vec![
        json!({"type": "text", "text": "Here they are."}),
        json!({"type": "tool_use", "id": "t", "name": "propose_people", "input": {"people": [
            {"name": " Wren Halloway ", "handles": ["slack:U0TIDE07", "not a handle", "name:Wren"],
             "role_line": "Takes the north gauge readings.", "evidence": ["L2", "L9", "L1", "L2"]},
            {"name": "wren  halloway", "handles": [], "role_line": "", "evidence": []},
            {"name": "", "handles": [], "role_line": "", "evidence": []},
            {"name": "Orrin Vale", "handles": [], "role_line": "", "evidence": ["L1"]}
        ]}}),
    ];
    let got = parse(&blocks, &ls).unwrap();
    assert_eq!(got.len(), 2, "one per folded name, none unnamed: {got:?}");
    let (wren, nodes) = &got[0];
    assert_eq!(wren.name, "Wren Halloway");
    assert_eq!(
        wren.handles.len(),
        1,
        "a name: handle and a bad one dropped: {:?}",
        wren.handles
    );
    assert!(wren.handles[0].starts_with("slack:"));
    assert_eq!(
        nodes,
        &["nod_2", "nod_1"],
        "evidence by label, an unknown one dropped"
    );
    assert_eq!(
        wren.evidence[0],
        "person:Wren Halloway: I'll take the north gauge."
    );
    assert_eq!(wren.role_line, "Takes the north gauge readings.");
    // No tool call at all: nothing parsed.
    assert!(parse(&[json!({"type": "text", "text": "Wren."})], &ls).is_none());
}

#[test]
fn the_owner_agents_personas_and_listed_names_are_excluded() {
    let mut cfg = crate::config::Config::example();
    cfg.places.owner = Some(vec!["discord:500000000000000042".into()]);
    cfg.people.not_people = vec!["Kestrel".into()];
    let ls = vec![
        line(1, OWNER_NAME, "Ask the gull bot."),
        line(2, "agent:Gullbot", "Noted."),
        line(3, "person:Wren Halloway", "On it."),
    ];
    let not = NotPeople::of(&cfg, &ls);
    assert!(
        not.excludes(&cand("sable  thorn")),
        "the owner's import name"
    );
    assert!(not.excludes(&cand("GULLBOT")), "an agent the session names");
    assert!(not.excludes(&cand("Kestrel")), "[people] not_people");
    assert!(not.excludes(&cand("assistant")));
    let mut by_id = cand("S.");
    by_id.handles = vec!["discord:500000000000000042".into()];
    assert!(not.excludes(&by_id), "the owner's handle");
    assert!(!not.excludes(&cand("Wren Halloway")));
    assert_eq!(fold("  Wren   HALLOWAY "), "wren halloway");
}

/// The fixture's sessions, in id order: a DM with Wren (the owner and an
/// agent speak too), a channel where Orrin speaks, and one with only tool
/// output and outside text (no human-facing text).
fn episode_line(i: usize) -> String {
    let (place, msgs): (serde_json::Value, Vec<(&str, &str)>) = match i {
        0 => (
            json!({"kind": "dm", "name": "Wren Halloway", "id": "U0TIDE07"}),
            vec![
                (OWNER_NAME, "Can you send Wren the calibration notes?"),
                (
                    "person:Wren Halloway",
                    "I'll take the north gauge readings this week.",
                ),
                (
                    "agent:Gullbot",
                    "Noted: Wren owns the gauge rota. Kestrel will log it.",
                ),
            ],
        ),
        1 => (
            json!({"kind": "slack-channel", "name": "tide-survey", "id": "C0TIDE09"}),
            vec![
                ("person:Orrin Vale", "The south buoy needs a new battery."),
                (OWNER_NAME, "Orrin, can you take that?"),
            ],
        ),
        _ => (
            json!({"kind": "slack-channel", "name": "tide-log", "id": "C0TIDE10"}),
            vec![("tool", "exit 0"), ("outside", "Tide table for May.")],
        ),
    };
    let messages: Vec<serde_json::Value> = msgs
        .iter()
        .enumerate()
        .map(|(k, (a, t))| {
            json!({"idx": k, "time": "2026-05-03T09:00:00Z", "author": a,
                "integrity": match *a { "outside" => "outside", "tool" => "agent",
                    a if a.starts_with("agent:") => "agent", _ => "operator" },
                "text": t, "unit": format!("unit-{i}-{k}"), "sha256": "cd".repeat(32)})
        })
        .collect();
    let mut v = json!({
        "format": 1, "import_tag": TAG, "episode_id": format!("ep_{:064x}", 0x7e44_0000 + i),
        "source": "openclaw-store", "agent": null, "place": place,
        "as_of": {"start": "2026-05-03T09:00:00Z", "end": "2026-05-03T09:20:00Z"},
        "labels": {"sensitivity": "personal", "topic": []},
        "summary": null, "messages": messages,
    });
    v["hash"] = json!(episode::hash_of(&v));
    serde_json::to_string(&v).unwrap()
}

fn sid(i: usize) -> String {
    session_id_of(&format!("ep_{:064x}", 0x7e44_0000 + i))
}

fn import(core: &Core) {
    let lines: Vec<ImportLine> = (0..3)
        .map(|i| ImportLine {
            line: i as u64 + 1,
            text: episode_line(i),
        })
        .collect();
    let r = write::import_batch(
        &core.store,
        &ImportEpisodesParams {
            file: "terns.jsonl".into(),
            lines,
        },
        "test",
    )
    .unwrap();
    assert!(r.rejected.is_empty(), "{:?}", r.rejected);
}

/// People on, categorize off, the owner's name never a person.
fn people_on(c: &mut crate::config::Config) {
    let on = |mode| JudgePackConfig {
        mode: Some(mode),
        sample: None,
        notices: None,
    };
    c.judge.packs.insert("people.v1".into(), on(PackMode::Live));
    c.judge
        .packs
        .insert("categorize.v1".into(), on(PackMode::Off));
}

/// Jev's answers: a real person, involved, new, a role line that states a
/// role; Kestrel is a codename; Orrin is the held Orrin, his line a judgment.
fn script(jev: &FakeJev) {
    jev.script("real", Jev::Noul(0.95));
    jev.script("involved", Jev::Noul(0.93));
    jev.script("evaluative", Jev::Noul(0.05));
    jev.script(
        "match",
        Jev::Choice {
            option: "new_person".into(),
            confidence: 0.92,
        },
    );
    jev.script_when("\"Kestrel\"", None, "real", Jev::Noul(0.08));
    jev.script_when(
        "Orrin Vale",
        None,
        "match",
        Jev::Choice {
            option: "orrin-vale".into(),
            confidence: 0.94,
        },
    );
    jev.script_when("Orrin Vale", None, "evaluative", Jev::Noul(0.91));
}

fn billed(then: Scripted) -> Scripted {
    Scripted::Billed {
        usage: Usage {
            input_tokens: 1000,
            output_tokens: 1_000_000,
            ..Usage::default()
        },
        then: Box::new(then),
    }
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

async fn propose(
    core: &Arc<Core>,
    dry_run: bool,
    cap: f64,
) -> theseus_protocol::import::PeopleProposeReport {
    core.import_people_propose(&ImportPeopleParams {
        tag: TAG.into(),
        dry_run,
        propose: true,
        cap_usd: Some(cap),
    })
    .await
    .unwrap()
    .propose
    .unwrap()
}

/// Orrin, held already, with his Slack id.
fn hold_orrin(core: &Core) {
    core.ontology_category_add(
        &OntologyCategoryAddParams {
            kind: Some("person".into()),
            name: "Orrin Vale".into(),
            handles: vec!["slack:U0ORRIN1".into()],
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
}

/// The extractor's two answers: the first session's, billed at $0.50; the
/// second's.
fn script_extractions(fake: &crate::provider::FakeProvider) {
    {
        let mut s = fake.script.lock().unwrap();
        s.push_back(billed(answer(json!([
            {"name": "Wren Halloway", "handles": ["slack:U0TIDE07"],
             "role_line": "Takes the north gauge readings and owns the gauge rota.", "evidence": ["L2", "L3"]},
            {"name": OWNER_NAME, "handles": [], "role_line": "", "evidence": ["L1"]},
            {"name": "Gullbot", "handles": [], "role_line": "Logs readings.", "evidence": ["L3"]},
            {"name": "Kestrel", "handles": [], "role_line": "", "evidence": ["L3"]}
        ]))));
        s.push_back(answer(json!([
            {"name": "Orrin Vale", "handles": [], "role_line": "A brilliant but careless engineer.",
             "evidence": ["L1", "L2"]}
        ])));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_backfill_proposes_people_under_its_cap_and_they_are_accepted_in_bulk() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    let r = rig(Some(&jev), 0, people_on);
    import(&r.core);
    hold_orrin(&r.core);

    // The dry run: the sessions, their tokens and the projected cost, and no call.
    let dry = propose(&r.core, true, 5.0).await;
    assert_eq!(
        (dry.sessions, dry.read, dry.no_text, dry.done_before),
        (3, 2, 1, 0),
        "{dry:?}"
    );
    assert!(dry.tokens > 0 && dry.projected_usd > 0.0, "{dry:?}");
    assert!(r.fake.requests().is_empty(), "a dry run calls no model");
    assert_eq!(jev.connections(), 0);

    // A cap that holds one session's worst case but not two: the first
    // session is passed and the run stops before the second.
    script_extractions(&r.fake);
    // The first costs $0.50 (billed so): past the cap's room for a second.
    let first = propose(&r.core, false, 0.4).await;
    assert_eq!((first.read, first.left), (1, 2), "{first:?}");
    assert!(
        first
            .stopped
            .as_deref()
            .unwrap()
            .contains("stopped at the cap"),
        "{first:?}"
    );
    assert!(
        first
            .stopped
            .as_deref()
            .unwrap()
            .contains("--propose --cap"),
        "{first:?}"
    );
    assert_eq!(
        (first.candidates, first.excluded, first.judged),
        (4, 2, 2),
        "{first:?}"
    );
    assert_eq!(r.fake.requests().len(), 1);
    let sent = serde_json::to_string(&r.fake.requests()[0]).unwrap();
    assert!(
        sent.contains("propose_people") && sent.contains("Wren"),
        "the tool and the text"
    );

    // The resume goes on from the tag's mark.
    let rest = propose(&r.core, false, 5.0).await;
    assert_eq!(
        (rest.done_before, rest.read, rest.no_text, rest.left),
        (1, 1, 1, 0),
        "{rest:?}"
    );
    assert!(rest.stopped.is_none(), "{rest:?}");
    assert_eq!(
        r.fake.requests().len(),
        2,
        "the no-text session is never sent"
    );
    let again = propose(&r.core, false, 5.0).await;
    assert_eq!((again.done_before, again.read), (3, 0), "{again:?}");

    // The ledger: each extraction's row, with its cost; three judgments.
    let extracted = rows(&r.core.store, "people.extracted");
    assert_eq!(extracted.len(), 2);
    assert!(extracted[0].data["cost_usd"].as_f64().unwrap() > 0.0);
    assert_eq!(
        extracted[0].data["excluded"],
        json!([OWNER_NAME, "Gullbot"])
    );
    // Excluded before any call: Jev judged Wren, Kestrel and Orrin alone.
    let mut judged: Vec<String> = until_rows(&r.core.store, "judge.call", 3)
        .await
        .iter()
        .map(|j| {
            j.data["context"]["candidate"]["name"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    judged.sort();
    assert_eq!(judged, ["Kestrel", "Orrin Vale", "Wren Halloway"]);
    // And the states Jev read name no other candidate.
    let mut seen: Vec<String> = jev
        .seen()
        .iter()
        .filter_map(|s| {
            let st = &s.body["state"];
            let v: serde_json::Value = match st.as_str() {
                Some(text) => serde_json::from_str(text).ok()?,
                None => st.clone(),
            };
            Some(v["candidate_name"].as_str()?.to_string())
        })
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        ["Kestrel", "Orrin Vale", "Wren Halloway"],
        "the owner and the agent never reach Jev"
    );

    proposals_are_listed_then_accepted_in_bulk(&r.core);
}

/// The backfill's proposals: Wren new, Orrin held, Kestrel none; then the
/// bulk accept of people.
fn proposals_are_listed_then_accepted_in_bulk(core: &Arc<Core>) {
    // The proposals: Wren, new, with her role line; Orrin, held, his line
    // dropped as a judgment of him; Kestrel, no person, none.
    let all = core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
        .proposals;
    assert_eq!(all.len(), 2, "{all:?}");
    let wren = all.iter().find(|p| p.session_id == sid(0)).unwrap();
    let who = wren.person.as_ref().unwrap();
    assert!(
        who.new && who.name == "Wren Halloway" && wren.topic.is_none(),
        "{wren:?}"
    );
    assert_eq!(
        who.role_line.as_deref(),
        Some("Takes the north gauge readings and owns the gauge rota.")
    );
    assert_eq!(wren.band, "act");
    let orrin = all.iter().find(|p| p.session_id == sid(1)).unwrap();
    assert_eq!(orrin.topic.as_deref(), Some("person:orrin-vale"));
    assert_eq!(
        orrin.person.as_ref().unwrap().role_line,
        None,
        "the evaluative line is dropped"
    );
    assert!(!orrin.person.as_ref().unwrap().new);

    // Accepted in bulk, by kind: Wren declared with her handles and her
    // role line, each session joined by the operator.
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
    assert!(done.left.is_empty(), "{done:?}");
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    let w = o
        .categories()
        .find(|c| c.name == "Wren Halloway")
        .expect("Wren declared");
    assert_eq!(
        w.description,
        "Takes the north gauge readings and owns the gauge rota."
    );
    assert!(
        w.handles
            .iter()
            .any(|h| h.eq_ignore_ascii_case("slack:U0TIDE07")),
        "{:?}",
        w.handles
    );
    let orrin_id = theseus_ontology::CategoryId::new("person", "orrin-vale").unwrap();
    for (s, who) in [(sid(0), w.id.clone()), (sid(1), orrin_id)] {
        let held = o.memberships(&s);
        assert!(
            held.iter()
                .any(|m| m.category == who && m.origin == Origin::Operator),
            "{s}: {held:?}"
        );
    }
    assert!(core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
        .proposals
        .is_empty());
}

/// A shared place's exchange end proposes nothing; a private one's, due,
/// extracts and judges, on its own mark.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shared_place_gets_nothing_and_a_private_one_proposes() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    let r = rig(Some(&jev), 2 * EVERY, people_on);
    let shared = session(&r.core, Some("channel:314159265358979323"));
    moorings(&r.core, &shared, EVERY).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(rows(&r.core.store, "people.extracted").is_empty());
    assert_eq!(
        r.fake.requests().len(),
        EVERY,
        "no extraction for a shared place"
    );

    r.fake.script.lock().unwrap().push_back(answer(json!([
        {"name": "Wren Halloway", "handles": [], "role_line": "", "evidence": ["L1"]}
    ])));
    let private = session(&r.core, None);
    moorings(&r.core, &private, EVERY).await;
    let x = until_rows(&r.core.store, "people.extracted", 1).await;
    assert_eq!(x[0].session_id.as_deref(), Some(private.as_str()));
    assert_eq!(x[0].data["purpose"], "live");
    until_rows(&r.core.store, "judge.call", 1).await;
    let mark: Option<crate::judge::categorize::Mark> = r
        .core
        .store
        .get_meta(&format!("{}{private}", super::live::MARK_PREFIX))
        .unwrap();
    assert!(mark.is_some(), "the mark moves in the extraction's frame");
}
