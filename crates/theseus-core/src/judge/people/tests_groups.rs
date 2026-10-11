//! People's proposals one row per person (theseus-fvyx), on a fixture of
//! the owner's store's shape after the backfill (about 1,551 proposals under
//! about 298 names): one name proposed 76 times, a bare first name beside
//! the one full name it is a word of, a held person proposed as held, by
//! name and by first name, and a first name two full names hold. The rows,
//! their counts and order; `min_confidence` and `limit` over rows; a row's
//! one accept (its first name joins its person, no second person made) and
//! one reject; the bulk yes a row at a time, leaving the ambiguous one with
//! why until `as_person` picks. Every name here is invented.

use serde_json::json;
use theseus_judge::fake::FakeJev;
use theseus_judge::{Answer, Judgment};
use theseus_ontology::CategoryId;
use theseus_protocol::{
    OntologyCategoryAddParams, OntologyPersonProposals, OntologyProposalAcceptAllParams,
    OntologyProposalRejectAllParams, OntologyProposalsParams, OntologyProposalsResult,
};
use theseus_store::{kinds, NewRecord};

use super::tests::until_rows;
use super::tests_sweep::shut_gate;
use super::SCOPE;
use crate::ledger::LedgerRow;
use crate::tests_categorize::{session, Rig};

/// One planted proposal: its session, the candidate's name, its least
/// probability, and whom Jev matched (`new_person` or a held local id).
struct Plant {
    sid: String,
    name: String,
    confidence: f64,
    whom: &'static str,
}

/// `people.v1` judgments made from `template` (a real one), one each,
/// written as the sink writes them: keyed by id, in the pack's scope.
fn plant(r: &Rig, template: &LedgerRow, ps: &[Plant]) {
    let mut at = template.at_unix_ms;
    for chunk in ps.chunks(100) {
        let mut recs = Vec::new();
        for p in chunk {
            at += 1;
            let mut j: Judgment = serde_json::from_value(template.data.clone()).unwrap();
            j.id = format!("jdg_{}", uuid::Uuid::now_v7().simple());
            j.context["candidate"]["name"] = json!(p.name);
            j.context["candidate"]["handles"] = json!([]);
            for a in &mut j.answers {
                match (a.question.as_str(), &mut a.answer) {
                    ("real" | "involved", Answer::Noul { noul }) => *noul = p.confidence,
                    (
                        "match",
                        Answer::Choice {
                            choice, confidence, ..
                        },
                    ) => {
                        *choice = p.whom.into();
                        *confidence = p.confidence;
                    }
                    _ => {}
                }
            }
            let row = LedgerRow {
                at_unix_ms: at,
                kind: "judge.call".into(),
                session_id: Some(p.sid.clone()),
                turn_id: None,
                data: serde_json::to_value(&j).unwrap(),
            };
            recs.push(
                NewRecord::json(kinds::LEDGER, Some(&j.id), &row)
                    .unwrap()
                    .scoped(SCOPE),
            );
        }
        r.core.store.append(&recs).unwrap();
    }
}

/// The fixture's sessions that exist (the rest are only named), by the
/// person their proposals name, and how many proposals there are.
struct Fixture {
    marlo: Vec<String>,
    tern: Vec<String>,
    orrin: Vec<String>,
    kestrel: usize,
    total: usize,
}

/// The real sessions' proposals: each `(name, whom, confidence)` once a
/// session, `n` sessions made for them.
fn real(
    r: &Rig,
    ps: &mut Vec<Plant>,
    n: usize,
    each: impl Fn(usize) -> (&'static str, &'static str, f64),
) -> Vec<String> {
    let sids: Vec<String> = (0..n).map(|_| session(&r.core, None)).collect();
    for (k, s) in sids.iter().enumerate() {
        let (name, whom, confidence) = each(k);
        ps.push(Plant {
            sid: s.clone(),
            name: name.into(),
            confidence,
            whom,
        });
    }
    sids
}

/// A rig with the fixture planted: a real people.v1 judgment (the sweep's,
/// of Wren) to plant from, Orrin Vale held, 290 names proposed 1 to 9 times
/// each in sessions only named (1,450), and the rows a test answers.
async fn fixture() -> (FakeJev, Rig, Fixture) {
    let (jev, r, _private, _shared) = shut_gate(2.0).await;
    r.fake
        .script
        .lock()
        .unwrap()
        .push_back(super::tests::answer(json!([
            {"name": "Wren Halloway", "handles": [], "role_line": "", "evidence": ["L1"]}
        ])));
    r.core.people_sweep("nightly").await.unwrap();
    let template = until_rows(&r.core.store, "judge.call", 1).await.remove(0);
    let orrin_vale = OntologyCategoryAddParams {
        kind: Some("person".into()),
        name: "Orrin Vale".into(),
        ..Default::default()
    };
    r.core
        .ontology_category_add(&orrin_vale, "the CLI")
        .unwrap();
    let mut ps = Vec::new();
    for i in 0..290 {
        for k in 0..(i % 9 + 1) {
            ps.push(Plant {
                sid: format!("ses_fixture_{i}_{k}"),
                name: format!("Skerry{i} Holm{i}"),
                confidence: if i % 2 == 0 { 0.95 } else { 0.7 },
                whom: "new_person",
            });
        }
    }
    let new = "new_person";
    let pell = |k: usize| {
        (
            ["pell garrow", "Pell Garrow", "Pell Garrow"][k % 3],
            new,
            0.92,
        )
    };
    real(&r, &mut ps, 76, pell);
    let marlo = real(&r, &mut ps, 14, |k| {
        (if k < 10 { "Marlo Quill" } else { "Marlo" }, new, 0.8)
    });
    let terns = [
        "Tern Ashby",
        "Tern Ashby",
        "Tern Mallow",
        "Tern Mallow",
        "Tern",
    ];
    let tern = real(&r, &mut ps, 9, |k| (terns[k % 5], new, 0.9));
    let orrin = real(&r, &mut ps, 8, |k| match k {
        0..=3 => ("Orrin Vale", "orrin-vale", 0.93),
        4 | 5 => ("Orrin Vale", new, 0.93),
        _ => ("Orrin", new, 0.93),
    });
    real(&r, &mut ps, 3, |_| ("Kestrel", new, 0.75));
    plant(&r, &template, &ps);
    // The template's own proposal of Wren Halloway is listed too.
    let total = ps.len() + 1;
    assert!((1_550..1_700).contains(&total), "{total}");
    let f = Fixture {
        marlo,
        tern,
        orrin,
        kestrel: 3,
        total,
    };
    (jev, r, f)
}

fn row<'a>(people: &'a [OntologyPersonProposals], name: &str) -> &'a OntologyPersonProposals {
    people
        .iter()
        .find(|g| g.name == name)
        .unwrap_or_else(|| panic!("no row {name}"))
}

fn by_person(r: &Rig, min: Option<f64>, limit: u32) -> OntologyProposalsResult {
    r.core
        .ontology_proposals(&OntologyProposalsParams {
            by_person: true,
            min_confidence: min,
            limit: Some(limit),
            ..Default::default()
        })
        .unwrap()
}

/// The rows: one per person, their counts and order, a first name inside
/// its person, the ambiguous one flagged; `min_confidence` and `limit`.
fn the_rows(r: &Rig, total: usize) {
    let each = r
        .core
        .ontology_proposals(&OntologyProposalsParams {
            limit: Some(u32::MAX),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        each.proposals.len(),
        total,
        "every proposal, one by one (`--each`)"
    );
    assert!(each.people.is_empty());
    // 290 + Pell, Marlo Quill (Marlo inside), Tern Ashby, Tern Mallow, Tern
    // (flagged), Orrin Vale (by id, by name, Orrin inside), Kestrel, Wren.
    let all = by_person(r, None, u32::MAX);
    let names: Vec<&String> = all.people.iter().map(|g| &g.name).collect();
    assert!(all.proposals.is_empty(), "no topic's proposal");
    assert_eq!(all.people.len(), 290 + 8, "{names:?}");
    let n: usize = all.people.iter().map(|g| g.judgments.len()).sum();
    assert_eq!(n, total);
    let first = &all.people[0];
    let top = (first.name.as_str(), first.sessions, first.judgments.len());
    assert_eq!(top, ("Pell Garrow", 76, 76));
    assert!(first.new && first.key == "name:pell garrow" && first.as_person == "Pell Garrow");
    let m = row(&all.people, "Marlo Quill");
    assert_eq!(
        (m.sessions, m.first_names.clone()),
        (14, vec!["Marlo".to_string()])
    );
    assert!(
        names.iter().all(|n| *n != "Marlo"),
        "Marlo is inside Marlo Quill"
    );
    let o = row(&all.people, "Orrin Vale");
    assert_eq!(
        (o.key.as_str(), o.new, o.sessions),
        ("person:orrin-vale", false, 8)
    );
    assert_eq!(o.as_person, "person:orrin-vale");
    assert_eq!(o.first_names, ["Orrin"]);
    let t = row(&all.people, "Tern");
    assert_eq!(t.ambiguous, ["Tern Ashby", "Tern Mallow"]);
    assert!(t.first_names.is_empty() && t.judgments.len() == 1);
    let k = row(&all.people, "Kestrel");
    assert!(k.ambiguous.is_empty() && k.first_names.is_empty(), "{k:?}");
    let s7 = row(&all.people, "Skerry7 Holm7");
    let range = (s7.sessions, s7.confidence_min, s7.confidence_max);
    assert_eq!(range, (8, 0.7, 0.7));
    assert_eq!(s7.bands, ["confirm"]);
    let sure = by_person(r, Some(0.9), u32::MAX);
    assert!(sure.people.iter().all(|g| g.confidence_max >= 0.9));
    assert!(sure.people.iter().any(|g| g.name == "Skerry8 Holm8"));
    assert!(sure
        .people
        .iter()
        .all(|g| g.name != "Skerry7 Holm7" && g.name != "Kestrel"));
    let page = by_person(r, None, 50);
    assert_eq!((page.people.len(), page.people_more), (50, 298 - 50));
}

/// A row's one reject, and its one accept: Marlo joins Marlo Quill, one
/// person made.
fn a_rows_answers(r: &Rig, f: &Fixture) {
    let all = by_person(r, None, u32::MAX);
    let reject = OntologyProposalRejectAllParams {
        judgments: row(&all.people, "Kestrel").judgments.clone(),
        ..Default::default()
    };
    let no = r
        .core
        .ontology_proposal_reject_all(&reject, "the CLI")
        .unwrap();
    assert_eq!((no.rejected.len(), no.left.len()), (f.kestrel, 0), "{no:?}");
    let no = r
        .core
        .ontology_proposal_reject_all(&reject, "the CLI")
        .unwrap();
    assert_eq!((no.rejected.len(), no.left.len()), (0, f.kestrel), "{no:?}");
    assert!(no.left[0].contains("not listed now"), "{no:?}");
    let m = row(&all.people, "Marlo Quill");
    let accept = OntologyProposalAcceptAllParams {
        judgments: m.judgments.clone(),
        as_person: Some(m.as_person.clone()),
        ..Default::default()
    };
    let yes = r
        .core
        .ontology_proposal_accept_all(&accept, "the CLI")
        .unwrap();
    assert_eq!((yes.accepted.len(), yes.left.len()), (14, 0), "{yes:?}");
    let onto = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let people: Vec<&str> = onto
        .categories()
        .filter(|c| c.kind() == theseus_ontology::person::KIND && c.name.contains("Marlo"))
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        people,
        ["Marlo Quill"],
        "no second person for the first name"
    );
    let mq = CategoryId::new("person", "marlo-quill").unwrap();
    for s in &f.marlo {
        assert!(onto.memberships(s).iter().any(|x| x.category == mq), "{s}");
    }
}

/// The bulk yes, a row at a time: the ambiguous Tern is left with why,
/// Orrin joins Orrin Vale, 76 proposals make one Pell Garrow; then Tern is
/// picked with `as_person`.
fn the_bulk_yes(r: &Rig, f: &Fixture) {
    let bulk = OntologyProposalAcceptAllParams {
        kind: Some("person".into()),
        min_confidence: 0.9,
        ..Default::default()
    };
    let bulk = r
        .core
        .ontology_proposal_accept_all(&bulk, "the CLI")
        .unwrap();
    let tern_left: Vec<&String> = bulk.left.iter().filter(|l| l.contains("--as")).collect();
    assert_eq!(tern_left.len(), 1, "{:?}", bulk.left);
    assert!(
        tern_left[0].contains("Tern Ashby, Tern Mallow"),
        "{tern_left:?}"
    );
    let onto = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let ov = CategoryId::new("person", "orrin-vale").unwrap();
    for s in &f.orrin {
        assert!(onto.memberships(s).iter().any(|x| x.category == ov), "{s}");
    }
    assert!(onto
        .categories()
        .all(|c| c.name != "Orrin" && c.name != "Tern"));
    let pg = onto
        .categories()
        .filter(|c| c.name.eq_ignore_ascii_case("Pell Garrow"));
    assert_eq!(pg.count(), 1, "one Pell Garrow from 76 proposals");
    let left = by_person(r, None, u32::MAX);
    let pick = OntologyProposalAcceptAllParams {
        judgments: row(&left.people, "Tern").judgments.clone(),
        as_person: Some("Tern Mallow".into()),
        ..Default::default()
    };
    let pick = r
        .core
        .ontology_proposal_accept_all(&pick, "the CLI")
        .unwrap();
    assert_eq!(pick.accepted.len(), 1, "{pick:?}");
    let onto = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let tm = CategoryId::new("person", "tern-mallow").unwrap();
    assert!(onto
        .memberships(&f.tern[4])
        .iter()
        .any(|x| x.category == tm));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn proposals_are_one_row_per_person_with_first_names_inside() {
    let (_jev, r, f) = fixture().await;
    the_rows(&r, f.total);
    a_rows_answers(&r, &f);
    the_bulk_yes(&r, &f);
}
