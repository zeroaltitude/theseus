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
    assert!(r.core.morning_facts(now + 1_000, None, 0).is_none());
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
            .morning_facts(now_unix_ms(), None, lines.len() as u64)
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

/// Two writes in one millisecond keep the order they were made in: the
/// sweep's, then the dream's, though `dream-` sorts before `sweep-` (a fast
/// host's sweep and dream share a millisecond).
#[test]
fn writes_in_one_millisecond_are_listed_in_the_ledgers_order() {
    let row = |kind: &str, write: &str| crate::ledger::LedgerRow {
        at_unix_ms: 5,
        kind: kind.into(),
        session_id: None,
        turn_id: None,
        data: serde_json::json!({"undo": {"write": write}, "category": "topic:x"}),
    };
    let rows = [
        row("ontology.kind", "sweep-5-1-kind"),
        row("ontology.guidance", "dream-5-0"),
    ];
    let lines = crate::judge::onto::digest::lines(&rows, &|_| None);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].ends_with("undo sweep-5-1-kind"), "{lines:?}");
    assert!(lines[1].ends_with("undo dream-5-0"), "{lines:?}");
}

/// A rename (theseus-63tfk.1) is told with both names and the id that undoes it.
#[test]
fn a_rename_is_a_line_with_both_names() {
    let r = rig(|_| {});
    let now = now_unix_ms();
    r.core
        .ontology_machine_write(
            Origin::Jev,
            "j-0001",
            vec![Record::Category(Category::new(
                id("topic:scratch-notes"),
                "scratch notes",
                "jev:j-0001",
            ))],
        )
        .unwrap();
    let write = format!("sweep-{now}-0-rename");
    let mut o = (*r.core.runner.ontology.snapshot(&r.core.store).unwrap()).clone();
    let step = o
        .plan_rename(&id("topic:scratch-notes"), "Lantern Ledger", &write)
        .unwrap();
    r.core
        .ontology_machine_write(Origin::Sweep, &write, step.records().to_vec())
        .unwrap();
    assert_eq!(
        r.core.onto_night_lines(now + 1_000),
        vec![format!(
            "- renamed topic \"scratch notes\" to \"Lantern Ledger\" · undo: theseus ontology \
             undo {write}"
        )]
    );
}

/// A kind Jev named (theseus-e3pxa) says so in its line; one the most shared
/// name named does not.
#[test]
fn a_kind_jev_named_says_so_in_its_line() {
    use crate::judge::onto::mint::{plan_with, Hints, Said};
    let now = now_unix_ms();
    let said: Vec<Said> = ["s1", "s2", "s3"]
        .iter()
        .map(|s| Said {
            session: s.to_string(),
            names: vec!["Lantern Ledger".into()],
            confidence: 0.9,
        })
        .collect();
    for (pick, tail) in [(Some("Ledger Works"), ", named by Jev"), (None, "")] {
        let r = rig(|_| {});
        let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
        let mut hints = Hints::default();
        hints
            .named
            .extend(pick.map(|p| ("lanternledger".to_string(), p.to_string())));
        let plan = plan_with(&o, &said, now, |n| format!("sweep-{now}-{n}-kind"), &hints);
        let m = &plan.minted[0];
        r.core
            .ontology_machine_write(Origin::Sweep, &m.write, m.records.clone())
            .unwrap();
        let lines = r.core.onto_night_lines(now + 1_000);
        let want = format!("from 3 sessions{tail} · undo");
        assert!(lines[0].contains(&want), "{pick:?}: {lines:?}");
    }
}
