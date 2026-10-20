//! `onto.v1`'s labelled replay (theseus-mqqc2): sessions the import labelled
//! go through the pack with the labels hidden, and each kind's precision and
//! recall are scored against them.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::band::Band;
use crate::breaker::BreakerConfig;
use crate::builders::{InboundInput, OntoInput, OntoItem, OntoKind, OntoTable};
use crate::client::{ClientConfig, JevClient, StaticKey};
use crate::fake::{FakeJev, Scripted};
use crate::judge::{JevJudge, MemorySink, Recording};
use crate::pack::by_name;
use crate::price;
use crate::replay::{
    hide_labels, onto_markdown, onto_report, replay_onto, score_onto, Chosen, Floor, LabelledCase,
    Pick, Scored,
};

fn item(id: &str, name: &str, session: bool) -> OntoItem {
    OntoItem {
        id: id.into(),
        name: name.into(),
        description: String::new(),
        session,
        thread: false,
        used_ms: 0,
    }
}

fn kind(name: &str, ids: &[&str]) -> OntoKind {
    OntoKind {
        kind: name.into(),
        meaning: format!("what a {name} is"),
        pool: ids
            .iter()
            .map(|id| item(&format!("{name}:{id}"), id, false))
            .collect(),
        mined_from: Vec::new(),
    }
}

/// A session of three messages' worth: a unique token in the message so the
/// fake can answer each differently, a pool of three topics and three
/// projects, and the import's labels.
fn case(n: usize, topic: &str, project: &str) -> LabelledCase {
    let mut input: InboundInput =
        serde_json::from_str(include_str!("../fixtures/inputs/inbound.json")).unwrap();
    input.message = format!("replay marker zz{n}zz about the work");
    input.onto = Some(Box::new(OntoInput {
        kinds: vec![
            kind("topic", &["alpha", "beta", "gamma"]),
            kind("project", &["north", "south", "east"]),
        ],
        table: vec![OntoTable {
            name: "topic".into(),
            meaning: "a subject of work".into(),
        }],
        ..OntoInput::default()
    }));
    let set = |k: &str, v: &str| (k.to_string(), BTreeSet::from([format!("{k}:{v}")]));
    LabelledCase {
        session: format!("s{n}"),
        input,
        labels: BTreeMap::from([set("topic", topic), set("project", project)]),
    }
}

fn cases() -> Vec<LabelledCase> {
    vec![
        case(1, "alpha", "north"),
        case(2, "beta", "south"),
        case(3, "gamma", "east"),
    ]
}

/// The option a scripted model picks to answer with `category`: its place in
/// the candidates the builder offers for this session's message.
fn option_for(c: &LabelledCase, kind: &str, category: &str) -> String {
    let o = c.input.onto.as_ref().unwrap();
    let k = o.kinds.iter().find(|k| k.kind == kind).unwrap();
    let at = k
        .pick(&c.input.message)
        .candidates
        .iter()
        .position(|i| i.id == format!("{kind}:{category}"))
        .unwrap();
    format!("c{}", at + 1)
}

/// Scripts the model to answer every session's kinds with the import's own
/// labels, except where `wrong` names a (session, kind) to answer with
/// another category.
fn script(fake: &FakeJev, cases: &[LabelledCase], wrong: &[(&str, &str, &str)]) {
    for c in cases {
        let marker = format!("zz{}zz", &c.session[1..]);
        for (kind, gold) in &c.labels {
            let label = gold.iter().next().unwrap().split(':').nth(1).unwrap();
            let category = wrong
                .iter()
                .find(|(s, k, _)| *s == c.session && k == kind)
                .map_or(label, |(_, _, other)| other);
            fake.script_when(
                &marker,
                None,
                &format!("onto.v1/{kind}"),
                Scripted::Choice {
                    option: option_for(c, kind, category),
                    confidence: 0.9,
                },
            );
        }
    }
}

fn judge_for(fake: &FakeJev) -> Recording<JevJudge, Arc<MemorySink>> {
    let client = JevClient::new(
        ClientConfig {
            api_base: fake.base(),
            ..ClientConfig::default()
        },
        Arc::new(StaticKey::new(
            "apik-test-0123456789-abcdefghijklmnop".into(),
        )),
    )
    .unwrap();
    Recording::new(
        JevJudge::new(client, price::builtin(), BreakerConfig::default()),
        Arc::new(MemorySink::default()),
    )
}

fn by_kind(rows: &[crate::replay::KindScore], kind: &str) -> crate::replay::KindScore {
    rows.iter().find(|k| k.kind == kind).unwrap().clone()
}

/// The key test: a scripted model that answers the import's labels scores
/// 1.0 on every kind, at both floors.
#[tokio::test]
async fn a_model_that_answers_the_labels_scores_one() {
    let fake = FakeJev::start().unwrap();
    let cs = cases();
    script(&fake, &cs, &[]);
    let pack = by_name("onto.v1").unwrap();
    let scored = replay_onto(&judge_for(&fake), pack, &cs).await.unwrap();
    assert_eq!(scored.len(), 3);
    let report = onto_report("onto.v1", cs.len(), &scored);
    assert_eq!(report.unanswered, 0);
    for rows in [&report.at_act, &report.at_confirm] {
        assert_eq!(rows.len(), 2);
        for k in rows {
            assert_eq!(
                (k.precision, k.recall, k.hits, k.wrong, k.missed),
                (Some(1.0), Some(1.0), 3, 0, 0),
                "{}",
                k.kind
            );
        }
    }
}

/// One planted wrong answer lowers exactly its kind's precision and recall.
#[tokio::test]
async fn a_wrong_answer_lowers_exactly_that_kinds_precision() {
    let fake = FakeJev::start().unwrap();
    let cs = cases();
    script(&fake, &cs, &[("s2", "project", "east")]);
    let pack = by_name("onto.v1").unwrap();
    let scored = replay_onto(&judge_for(&fake), pack, &cs).await.unwrap();
    let rows = score_onto(&scored, Floor::Act);
    let topic = by_kind(&rows, "topic");
    assert_eq!((topic.precision, topic.recall), (Some(1.0), Some(1.0)));
    let project = by_kind(&rows, "project");
    assert_eq!((project.hits, project.wrong, project.missed), (2, 1, 1));
    assert_eq!(project.precision, Some(2.0 / 3.0));
    assert_eq!(project.recall, Some(2.0 / 3.0));
}

/// The labels never reach Jev: a label's category is not the session's own,
/// in the input or in the request.
#[tokio::test]
async fn the_labels_are_hidden_from_the_request() {
    let fake = FakeJev::start().unwrap();
    let mut cs = cases();
    cs.truncate(1);
    for k in &mut cs[0].input.onto.as_mut().unwrap().kinds {
        // The import's own membership: it would show as the session's.
        k.pool[0].session = true;
        k.pool[0].name = "Labelled Name Here".into();
    }
    assert!(hide_labels(&cs[0])
        .onto
        .unwrap()
        .kinds
        .iter()
        .all(|k| !k.pool[0].session));
    script(&fake, &cs, &[]);
    let pack = by_name("onto.v1").unwrap();
    replay_onto(&judge_for(&fake), pack, &cs).await.unwrap();
    for seen in fake.seen() {
        let state = seen.body.to_string();
        assert!(!state.contains("memberships"), "{state}");
    }
}

fn scored(picks: &[(&str, Pick, Band)], labels: &[(&str, &[&str])]) -> Scored {
    Scored {
        session: "s".into(),
        labels: labels
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    v.iter().map(|c| c.to_string()).collect::<BTreeSet<_>>(),
                )
            })
            .collect(),
        picks: picks
            .iter()
            .map(|(k, p, b)| {
                (
                    k.to_string(),
                    Chosen {
                        pick: p.clone(),
                        band: *b,
                    },
                )
            })
            .collect(),
    }
}

/// Floors, `new`, nothing, and a second label: each lands where the doc says.
#[test]
fn the_floor_and_the_misses_are_counted_as_documented() {
    let cand = |c: &str| Pick::Candidate(c.into());
    let cases = vec![
        // Hit at act.
        scored(&[("topic", cand("t:a"), Band::Act)], &[("topic", &["t:a"])]),
        // Right but only a confirm: counts at Confirm, a miss at Act.
        scored(
            &[("topic", cand("t:b"), Band::Confirm)],
            &[("topic", &["t:b"])],
        ),
        // `new` where a label was listed: a miss, minted.
        scored(&[("topic", Pick::New, Band::Act)], &[("topic", &["t:c"])]),
        // Two labels, one found.
        scored(
            &[("topic", cand("t:d"), Band::Act)],
            &[("topic", &["t:d", "t:e"])],
        ),
        // Nothing said, nothing labelled: no count.
        scored(&[("topic", Pick::Nothing, Band::Act)], &[]),
        // A pick with no label at all is wrong.
        scored(&[("topic", cand("t:f"), Band::Act)], &[]),
    ];
    let act = by_kind(&score_onto(&cases, Floor::Act), "topic");
    assert_eq!(
        (act.picked, act.hits, act.wrong, act.missed, act.minted),
        (3, 2, 1, 3, 1)
    );
    assert_eq!(act.recall, Some(2.0 / 5.0));
    let confirm = by_kind(&score_onto(&cases, Floor::Confirm), "topic");
    assert_eq!((confirm.picked, confirm.hits, confirm.missed), (4, 3, 2));
    // Nothing picked at the floor: no precision is claimed.
    let none = score_onto(
        &[scored(
            &[("topic", cand("t:a"), Band::Escalate)],
            &[("topic", &["t:a"])],
        )],
        Floor::Confirm,
    );
    assert_eq!((none[0].precision, none[0].recall), (None, Some(0.0)));
}

#[test]
fn the_markdown_names_each_kind_and_prints_n_a_for_no_number() {
    let s = scored(
        &[("topic", Pick::Nothing, Band::Act)],
        &[("topic", &["t:a"])],
    );
    let md = onto_markdown(&onto_report("onto.v1", 2, &[s]));
    assert!(md.contains("1 unanswered (left out)"), "{md}");
    assert!(
        md.contains("| topic | 1 | 1 | 0 | 0 | 0 | 1 | 0 | n/a | 0.000 |"),
        "{md}"
    );
}
