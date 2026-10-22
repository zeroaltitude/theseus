//! The dream writes guidance (theseus-olt3h), through a whole core and a
//! scripted provider: a category five sessions hold gets its first guidance,
//! four get none, an answer over 400 characters is refused and never cut, the
//! night stops at `[memory] synth_limit_usd_per_day`, and the undo restores the
//! empty guidance.

use std::sync::Arc;

use theseus_ontology::{
    Category, CategoryId, MemberList, Membership, Ontology, Origin, Record, DREAM_GUIDANCE_MAX,
};
use theseus_protocol::{now_unix_ms, SessionKind};

use super::guidance::{INSTRUCTIONS, MARK, PER_NIGHT};
use crate::node::Node;
use crate::provider::FakeProvider;
use crate::rpc::Parts;
use crate::session::{SessionRecord, TargetRef};
use crate::store::Store;
use crate::{Config, Core};

const ANSWER: &str = "The harbour survey logs tides at the north gauge every Monday.";

struct Rig {
    core: Arc<Core>,
    model: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn rig(reply: &str, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(FakeProvider {
        reply: reply.into(),
        ..FakeProvider::default()
    });
    let core = Core::build(Parts::for_tests(cfg, model.clone(), store)).unwrap();
    Rig {
        core,
        model,
        _dir: dir,
    }
}

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

/// A person's session that last used the live profile, with two messages.
fn session(core: &Core, sid: &str, title: &str, first: &str, last: &str, active: u64) {
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.session_id = sid.into();
    rec.title = Some(title.into());
    rec.last_active_ms = active;
    let (live, _) = core.live_profile();
    let t = core.runner.resolve_target(&live, None, None, None).unwrap();
    rec.last_target = Some(TargetRef {
        profile: t.profile,
        provider: t.provider,
        model: t.model,
    });
    core.store.put_session(sid, &rec).unwrap();
    for text in [first, last] {
        let n = Node::user(sid, None, "test", text);
        core.store.append(&[n.record().unwrap()]).unwrap();
    }
}

/// A category `name` a machine wrote, held by `n` sessions.
fn plant(core: &Core, slug: &str, by: &str, n: usize) {
    let category = format!("topic:{slug}");
    let mut records = vec![Record::Category(Category::new(id(&category), slug, by))];
    let at = now_unix_ms();
    for i in 0..n {
        let sid = format!("{slug}-{i}");
        session(
            core,
            &sid,
            &format!("{slug} session {i}"),
            &format!("first words of {slug} {i}"),
            &format!("last words of {slug} {i}"),
            at - i as u64,
        );
        records.push(Record::Members(MemberList {
            session: sid,
            kind: "topic".into(),
            members: vec![Membership {
                category: id(&category),
                origin: Origin::Jev,
                confidence: Some(0.9),
                as_of_ms: at,
            }],
        }));
    }
    if by.starts_with("jev") {
        core.ontology_machine_write(Origin::Jev, "j-0001", records)
            .unwrap();
    } else {
        core.runner
            .ontology
            .write(&core.store, records, Origin::Operator, |_| Ok(vec![]))
            .unwrap();
    }
}

fn snapshot(core: &Core) -> Arc<Ontology> {
    core.runner.ontology.snapshot(&core.store).unwrap()
}

fn asked(model: &FakeProvider) -> usize {
    model
        .requests()
        .iter()
        .filter(|r| r.system.iter().any(|b| b["text"] == INSTRUCTIONS))
        .count()
}

#[tokio::test]
async fn the_dream_writes_guidance_for_a_category_of_five() {
    let r = rig(ANSWER, |_| {});
    plant(&r.core, "harbour", "jev:j-0001", 5);
    let before = snapshot(&r.core);
    let done = r.core.dream_guidance(now_unix_ms()).await.unwrap();
    assert_eq!((done.written.len(), done.refused, done.waiting), (1, 0, 0));
    let write = done.written[0].clone();
    assert!(write.starts_with("dream-"), "{write}");
    let o = snapshot(&r.core);
    let g = o.guidance(&id("topic:harbour")).expect("guidance");
    assert_eq!(g.text, ANSWER);
    assert_eq!(g.added_by, format!("dream:{write}"));
    // The request names the category and each session's title and two messages.
    let request = r
        .model
        .requests()
        .into_iter()
        .find(|q| q.system.iter().any(|b| b["text"] == INSTRUCTIONS))
        .expect("the call");
    let content = request.messages[0]["content"][0]["text"].as_str().unwrap();
    assert!(content.contains("Category: harbour"), "{content}");
    assert!(content.contains("harbour session 0"), "{content}");
    assert!(
        content.contains("First: first words of harbour 0"),
        "{content}"
    );
    assert!(
        content.contains("Last: last words of harbour 4"),
        "{content}"
    );
    // The undo restores the empty guidance, one version on.
    r.core.ontology_undo(&write, "test", "test").unwrap();
    let back = snapshot(&r.core);
    assert!(back
        .guidance(&id("topic:harbour"))
        .is_none_or(|g| g.is_empty()));
    assert_ne!(*back, *before, "the guidance's version moved");
    // The spend is kept where a restart reads it back.
    let mark: super::guidance::DreamMark = r.core.store.get_meta(MARK).unwrap().unwrap();
    assert!(mark.spent_usd > 0.0, "{mark:?}");
}

#[tokio::test]
async fn four_sessions_get_none() {
    let r = rig(ANSWER, |_| {});
    plant(&r.core, "harbour", "jev:j-0001", 4);
    let done = r.core.dream_guidance(now_unix_ms()).await.unwrap();
    assert_eq!((done.written.len(), asked(&r.model)), (0, 0));
    assert!(snapshot(&r.core).guidance(&id("topic:harbour")).is_none());
}

#[tokio::test]
async fn guidance_over_400_characters_is_refused_not_cut() {
    let long = "x".repeat(DREAM_GUIDANCE_MAX + 1);
    let r = rig(&long, |_| {});
    plant(&r.core, "harbour", "jev:j-0001", 5);
    let done = r.core.dream_guidance(now_unix_ms()).await.unwrap();
    assert_eq!((done.written.len(), done.refused), (0, 1));
    assert_eq!(asked(&r.model), 1, "it was asked");
    assert!(snapshot(&r.core).guidance(&id("topic:harbour")).is_none());
    // The call was paid for, and counted.
    assert!(done.spent > 0);
}

#[tokio::test]
async fn the_dream_stops_at_synth_limit() {
    let r = rig(ANSWER, |c| c.memory.synth_limit_usd_per_day = 0.0);
    plant(&r.core, "harbour", "jev:j-0001", 5);
    let done = r.core.dream_guidance(now_unix_ms()).await.unwrap();
    assert_eq!((done.written.len(), asked(&r.model)), (0, 0));
    assert!(done.stopped.is_some(), "{done:?}");
}

/// The operator's category, a category four hold, and a fourth category past
/// the night's three are left alone; the most held come first.
#[tokio::test]
async fn the_operators_categories_are_left_and_three_a_night_are_written() {
    let r = rig(ANSWER, |_| {});
    plant(&r.core, "alpha", "operator", 9);
    for (slug, n) in [("bravo", 5), ("charlie", 8), ("delta", 6), ("echo", 7)] {
        plant(&r.core, slug, "jev:j-0001", n);
    }
    let done = r.core.dream_guidance(now_unix_ms()).await.unwrap();
    assert_eq!(done.written.len(), PER_NIGHT);
    let o = snapshot(&r.core);
    for slug in ["charlie", "echo", "delta"] {
        assert!(
            o.guidance(&id(&format!("topic:{slug}"))).is_some(),
            "{slug}"
        );
    }
    for slug in ["alpha", "bravo"] {
        assert!(
            o.guidance(&id(&format!("topic:{slug}"))).is_none(),
            "{slug}"
        );
    }
    // The next night takes the next one.
    let next = r.core.dream_guidance(now_unix_ms() + 1).await.unwrap();
    assert_eq!(next.written.len(), 1);
    assert!(snapshot(&r.core).guidance(&id("topic:bravo")).is_some());
}

/// Sessions that last used different profiles make the category wait.
#[tokio::test]
async fn a_category_whose_sessions_last_used_two_profiles_waits() {
    let r = rig(ANSWER, |_| {});
    plant(&r.core, "harbour", "jev:j-0001", 5);
    let mut rec: SessionRecord = r.core.store.get_session("harbour-0").unwrap().unwrap();
    rec.last_target.as_mut().unwrap().profile = "another".into();
    r.core.store.put_session("harbour-0", &rec).unwrap();
    let done = r.core.dream_guidance(now_unix_ms()).await.unwrap();
    assert_eq!(
        (done.written.len(), done.waiting, asked(&r.model)),
        (0, 1, 0)
    );
}
