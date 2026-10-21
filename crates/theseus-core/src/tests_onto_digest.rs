//! The morning note lists the night's ontology writes (theseus-0umqz): one
//! line a write of the sweep's or the dream's with the id `ontology undo`
//! takes, an undone write says so, a day with no write adds no line, and the
//! note carries the lines verbatim after its words, voiced or not.

use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_ontology::{Category, CategoryId, Guidance, MemberList, Membership, Origin, Record};
use theseus_protocol::now_unix_ms;

use crate::config::PersonaConfig;
use crate::places::BoundPlace;
use crate::provider::FakeProvider;
use crate::store::Store;
use crate::test_support::rows_100k as rows;
use crate::{Config, Core};

const DAY: u64 = 24 * 3_600_000;
const OWNER: u64 = 271_828_182_845_904_523;

struct Rig {
    core: Arc<Core>,
    _dir: tempfile::TempDir,
}

fn rig(tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg.personas.insert(
        "ondine".into(),
        PersonaConfig {
            name: Some("Ondine".into()),
            emoji: Some("🌊".into()),
            initiative: theseus_protocol::Initiative::Normal,
            ..Default::default()
        },
    );
    cfg.context.default_persona = Some("ondine".into());
    cfg.initiative.morning_hour = 0;
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(FakeProvider::default());
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap();
    core.bind_places(vec![BoundPlace {
        target: format!("discord:dm:{OWNER}"),
        name: "DM @wren".into(),
        ..Default::default()
    }]);
    Rig { core, _dir: dir }
}

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn held(session: &str, category: &str, origin: Origin, as_of_ms: u64) -> Record {
    Record::Members(MemberList {
        session: session.into(),
        kind: "topic".into(),
        members: vec![Membership {
            category: id(category),
            origin,
            confidence: Some(0.9),
            as_of_ms,
        }],
    })
}

/// A night's writes: a merge and a decay by the sweep, and a guidance by the
/// dream, which the owner undid.
fn plant_night(core: &Core, now: u64) -> (Vec<String>, String) {
    core.ontology_machine_write(
        Origin::Jev,
        "j-0001",
        vec![
            Record::Category(Category::new(
                id("topic:rust-harness"),
                "Rust Harness",
                "jev:j-0001",
            )),
            Record::Category(Category::new(
                id("topic:rust-harness-2"),
                "rust-harness",
                "jev:j-0001",
            )),
            Record::Category(Category::new(
                id("topic:old-news"),
                "old news",
                "jev:j-0001",
            )),
            held("s1", "topic:rust-harness", Origin::Jev, now),
            held("s2", "topic:rust-harness-2", Origin::Jev, now),
            // Unconfirmed for 40 days: the sweep drops it.
            held("s3", "topic:old-news", Origin::Jev, now - 40 * DAY),
        ],
    )
    .unwrap();
    let swept = core.onto_sweep(now).unwrap();
    assert_eq!((swept.merged, swept.decayed), (1, 1), "{swept:?}");
    let dream = format!("dream-{now}-0");
    core.ontology_machine_write(
        Origin::Dream,
        &dream,
        vec![Record::Guidance(Guidance::new(
            id("topic:rust-harness"),
            "Builds are run through the gate.",
            1,
            &format!("dream:{dream}"),
        ))],
    )
    .unwrap();
    (swept.writes, dream)
}

#[test]
fn the_morning_note_lists_each_write_with_its_undo_id() {
    let r = rig(|_| {});
    let now = now_unix_ms();
    let (writes, dream) = plant_night(&r.core, now);
    let merge = writes.iter().find(|w| w.ends_with("-merge")).unwrap();
    let decay = writes.iter().find(|w| w.ends_with("-decay")).unwrap();
    let lines = r.core.onto_night_lines(now + 1_000);
    assert_eq!(
        lines,
        vec![
            format!(
                "- dropped 1 membership unconfirmed for 30 days · undo: theseus ontology undo \
                 {decay}"
            ),
            format!(
                "- merged topic \"rust-harness\" into \"Rust Harness\" (1 session) · undo: theseus \
                 ontology undo {merge}"
            ),
            format!(
                "- wrote guidance for topic \"Rust Harness\" · undo: theseus ontology undo {dream}"
            ),
        ]
    );

    // An undone write's line says it was undone, and no longer offers the undo.
    r.core.ontology_undo(&dream, "test", "test").unwrap();
    let lines = r.core.onto_night_lines(now + 2_000);
    let last = lines.last().unwrap();
    assert_eq!(last, "- wrote guidance for topic \"Rust Harness\" · undone");
    assert!(!last.contains("theseus ontology undo"));
}

#[test]
fn a_day_with_no_ontology_write_adds_no_line() {
    let r = rig(|_| {});
    let now = now_unix_ms();
    // The operator's own write, and Jev's in-turn filing, are no night's.
    r.core
        .ontology_machine_write(
            Origin::Jev,
            "j-0001",
            vec![
                Record::Category(Category::new(id("topic:harbour"), "harbour", "jev:j-0001")),
                held("s1", "topic:harbour", Origin::Jev, now),
            ],
        )
        .unwrap();
    assert!(r.core.onto_night_lines(now + 1_000).is_empty());
    // And nothing happened at all: no facts, so no note.
    assert!(r.core.morning_facts(now + 1_000, 0).is_none());
    // A day a week later reads none of the night before's.
    let (_, _) = plant_night(&r.core, now);
    assert!(r.core.onto_night_lines(now + 3 * DAY).is_empty());
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(20), "no {what} in 20 s");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The note counts the writes as one more item, and appends their lines
/// verbatim after its words: by the model's voice, and by the stand-in when
/// the model is off.
#[tokio::test]
async fn the_note_carries_the_lines_after_its_words_voiced_or_not() {
    for (voiced, day_usd) in [(true, 0.5), (false, 0.0)] {
        let r = rig(move |c| c.initiative.day_usd = day_usd);
        let now = now_unix_ms();
        let (_, dream) = plant_night(&r.core, now);
        let lines = r.core.onto_night_lines(now_unix_ms());
        assert_eq!(lines.len(), 3);
        let (brief, n) = r
            .core
            .morning_facts(now_unix_ms(), lines.len() as u64)
            .unwrap();
        assert!(
            brief.contains("3 ontology changes by the night's sweep and dream"),
            "{brief}"
        );
        assert_eq!(n, 3);
        assert!(r.core.post_morning_if_due(), "the day is claimed");
        until("the morning note", || {
            !rows(&r.core, "initiative.spoke").is_empty()
        })
        .await;
        let spoke = rows(&r.core, "initiative.spoke");
        assert_eq!(spoke[0]["kind"], "morning");
        let text = spoke[0]["text"].as_str().unwrap();
        assert!(text.ends_with(&lines.join("\n")), "voiced {voiced}: {text}");
        assert!(
            text.contains(&format!("undo: theseus ontology undo {dream}")),
            "{text}"
        );
        let words = text.trim_end_matches(&lines.join("\n")).trim();
        assert!(!words.is_empty(), "the words come first: {text}");
    }
}
