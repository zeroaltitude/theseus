//! The live questions in the owner's DM (theseus-6f8t0): what posts, when,
//! how many a day, and that a post and its `posted_ms` are one frame. The
//! clock is the tick's argument, so a test names the hour and the day.

use std::sync::Arc;

use theseus_protocol::{
    CardAsk, CardProposal, CardSource, ProposalState, StandingKind, StandingScope,
};

use super::state;
use crate::config::{Config, MemoryMode};
use crate::learning::local_midnight;
use crate::provider::FakeProvider;
use crate::rpc::Core;
use crate::store::Store;
use crate::tests_day_ceiling::{bind_dm, posts};

const HOUR: u64 = 3_600_000;
const DAY: u64 = 24 * HOUR;

struct Rig {
    core: Arc<Core>,
    _dir: tempfile::TempDir,
}

fn rig(tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.memory.mode = MemoryMode::Off;
    cfg.initiative.morning_hour = 8;
    cfg.card.dm_per_day = 3;
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    bind_dm(&core);
    Rig { core, _dir: dir }
}

/// Ten in the morning, a few days on: after every post the pacer has seen
/// (they are stamped with the real clock) and a day the tests can step by 24
/// hours.
fn morning() -> u64 {
    local_midnight(theseus_protocol::now_unix_ms()) + 3 * DAY + 10 * HOUR
}

fn proposal(n: u64, ask: CardAsk, live: bool) -> CardProposal {
    let text = format!("Moors at the north jetty, case {n}");
    CardProposal {
        id: format!("cdp_{n:08}"),
        kind: StandingKind::Fact,
        scope: StandingScope::Everywhere,
        question: match ask {
            CardAsk::Stale => format!("Is this still true? '{text}'"),
            _ => theseus_protocol::card_question(&text, &StandingScope::Everywhere, 2),
        },
        text,
        options: match ask {
            CardAsk::Stale => theseus_protocol::CARD_STALE_OPTIONS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            _ => theseus_protocol::card_options(),
        },
        sources: vec![CardSource {
            episode: format!("ep_{n}"),
            node: format!("nod_{n:0>10}"),
            kind: "decision".into(),
            at_ms: 1,
        }],
        support: 2,
        created_ms: n,
        expires_ms: morning() + 30 * DAY,
        state: ProposalState::Open,
        run: "crd_1".into(),
        answered_ms: None,
        answered_by: None,
        kept_text: None,
        standing_id: None,
        ask,
        record: matches!(ask, CardAsk::Stale).then(|| "std_00000001".to_string()),
        observations: if live {
            vec![format!("cob_{n:08}")]
        } else {
            vec![]
        },
        posted_ms: None,
    }
}

fn put(r: &Rig, ps: &[CardProposal]) {
    let recs: Vec<_> = ps.iter().map(|p| state::record(p).unwrap()).collect();
    r.core.store.append(&recs).unwrap();
}

fn read(r: &Rig) -> Vec<CardProposal> {
    state::proposals(&r.core.store).unwrap()
}

fn marked(r: &Rig) -> usize {
    read(r).iter().filter(|p| p.posted_ms.is_some()).count()
}

fn live(n: std::ops::RangeInclusive<u64>) -> Vec<CardProposal> {
    n.map(|n| proposal(n, CardAsk::Add, true)).collect()
}

#[tokio::test]
async fn three_open_live_questions_go_as_three_posts_each_marked() {
    let r = rig(|_| {});
    put(&r, &live(1..=3));
    let now = morning();
    assert_eq!(r.core.card_dm_tick(now).await.unwrap(), 3);
    let sent = posts(&r.core, "card_question");
    assert_eq!(sent.len(), 3);
    let mut ids: Vec<&str> = sent
        .iter()
        .map(|b| b["card"]["id"].as_str().unwrap())
        .collect();
    ids.sort();
    assert_eq!(ids, ["cdp_00000001", "cdp_00000002", "cdp_00000003"]);
    assert_eq!(sent[0]["card"]["ask"], "add");
    assert_eq!(marked(&r), 3);
    assert!(read(&r).iter().all(|p| p.posted_ms == Some(now)));
    assert_eq!(
        r.core.card_dm_tick(now + HOUR).await.unwrap(),
        0,
        "a second tick"
    );
    assert_eq!(posts(&r.core, "card_question").len(), 3);
}

#[tokio::test]
async fn five_open_questions_are_three_today_and_two_the_next_day() {
    let r = rig(|_| {});
    put(&r, &live(1..=5));
    let now = morning();
    assert_eq!(r.core.card_dm_tick(now).await.unwrap(), 3);
    assert_eq!(
        r.core.card_dm_tick(now + HOUR).await.unwrap(),
        0,
        "the day's three are posted"
    );
    assert_eq!(marked(&r), 3);
    // The oldest first.
    let rest: Vec<String> = read(&r)
        .into_iter()
        .filter(|p| p.posted_ms.is_none())
        .map(|p| p.id)
        .collect();
    assert_eq!(rest, ["cdp_00000004", "cdp_00000005"]);
    assert_eq!(r.core.card_dm_tick(now + DAY).await.unwrap(), 2);
    assert_eq!(marked(&r), 5);
    assert_eq!(posts(&r.core, "card_question").len(), 5);
}

#[tokio::test]
async fn a_later_question_the_same_day_takes_what_the_allowance_left() {
    let r = rig(|_| {});
    put(&r, &live(1..=1));
    let now = morning();
    assert_eq!(r.core.card_dm_tick(now).await.unwrap(), 1);
    put(&r, &live(2..=5));
    // The pacer's window (30 minutes) has passed.
    assert_eq!(r.core.card_dm_tick(now + HOUR).await.unwrap(), 2);
    assert_eq!(marked(&r), 3);
}

#[tokio::test]
async fn nothing_goes_when_a_condition_holds_it_back() {
    let now = morning();
    for (what, r) in [
        ("dm_per_day = 0", rig(|c| c.card.dm_per_day = 0)),
        ("observe = false", rig(|c| c.card.observe = false)),
        (
            "before morning_hour",
            rig(|c| c.initiative.morning_hour = 11),
        ),
    ] {
        put(&r, &live(1..=3));
        assert_eq!(r.core.card_dm_tick(now).await.unwrap(), 0, "{what}");
        assert!(posts(&r.core, "card_question").is_empty(), "{what}");
        assert_eq!(marked(&r), 0, "{what}");
    }
    // No DM bound: nowhere to post.
    let r = rig(|_| {});
    r.core.runner.place_rule.bind(vec![]);
    put(&r, &live(1..=3));
    assert_eq!(r.core.card_dm_tick(now).await.unwrap(), 0, "no DM");
}

#[tokio::test]
async fn a_closed_pacer_window_holds_the_questions_and_never_posts_them_silently() {
    let r = rig(|_| {});
    put(&r, &live(1..=3));
    let now = morning();
    r.core.outbox.pacer.took(now - 5 * 60_000);
    assert_eq!(r.core.card_dm_tick(now).await.unwrap(), 0);
    assert!(posts(&r.core, "card_question").is_empty());
    assert_eq!(marked(&r), 0);
    // The window passes: they go, and not silent.
    assert_eq!(r.core.card_dm_tick(now + HOUR).await.unwrap(), 3);
    assert!(posts(&r.core, "card_question")
        .iter()
        .all(|b| b["silent"] != true));
}

#[tokio::test]
async fn an_archive_question_an_answered_one_and_a_posted_one_are_never_posted() {
    let r = rig(|_| {});
    let mut kept = proposal(2, CardAsk::Add, true);
    kept.state = ProposalState::Kept;
    let mut declined = proposal(3, CardAsk::Add, true);
    declined.state = ProposalState::Declined;
    let mut already = proposal(4, CardAsk::Add, true);
    already.posted_ms = Some(morning() - 3 * DAY);
    let mut expired = proposal(5, CardAsk::Add, true);
    expired.expires_ms = morning() - 1;
    put(
        &r,
        &[
            proposal(1, CardAsk::Add, false),
            kept,
            declined,
            already,
            expired,
        ],
    );
    assert_eq!(r.core.card_dm_tick(morning()).await.unwrap(), 0);
    assert!(posts(&r.core, "card_question").is_empty());
}

#[tokio::test]
async fn a_question_answered_in_the_cli_first_is_not_posted() {
    let r = rig(|_| {});
    put(&r, &live(1..=2));
    let mut answered = proposal(1, CardAsk::Add, true);
    answered.state = ProposalState::Declined;
    put(&r, &[answered]);
    let now = morning();
    assert_eq!(r.core.card_dm_tick(now).await.unwrap(), 1);
    let sent = posts(&r.core, "card_question");
    assert_eq!(sent[0]["card"]["id"], "cdp_00000002");
    // And the answer is still the answer.
    let all = read(&r);
    assert_eq!(all[0].state, ProposalState::Declined);
    assert_eq!(all[0].posted_ms, None);
}

#[tokio::test]
async fn the_posts_and_the_marks_are_one_frame() {
    let r = rig(|_| {});
    put(&r, &live(1..=3));
    let frames = || r.core.store.stats().unwrap().frames_appended;
    let before = frames();
    assert_eq!(r.core.card_dm_tick(morning()).await.unwrap(), 3);
    assert_eq!(frames() - before, 1, "three posts and three marks");
    // Nothing due writes nothing.
    let before = frames();
    assert_eq!(r.core.card_dm_tick(morning() + HOUR).await.unwrap(), 0);
    assert_eq!(frames(), before);
}

#[tokio::test]
async fn the_text_is_a_decision_note_with_the_question_and_its_messages() {
    let r = rig(|_| {});
    put(
        &r,
        &[
            proposal(1, CardAsk::Add, true),
            proposal(2, CardAsk::Pattern, true),
            proposal(3, CardAsk::Stale, true),
        ],
    );
    r.core.card_dm_tick(morning()).await.unwrap();
    let sent = posts(&r.core, "card_question");
    assert_eq!(sent.len(), 3);
    for b in &sent {
        let t = b["text"].as_str().unwrap();
        assert!(crate::owner_note::is_headed(t), "{t}");
        assert!(t.starts_with("❓ Decision · "), "{t}");
        assert!(t.contains("Moors at the north jetty"), "{t}");
        assert!(t.contains("From m:"), "{t}");
    }
    let by = |id: &str| sent.iter().find(|b| b["card"]["id"] == id).unwrap();
    assert_eq!(by("cdp_00000002")["card"]["ask"], "pattern");
    assert_eq!(by("cdp_00000003")["card"]["ask"], "stale");
    println!(
        "{}",
        sent.iter()
            .map(|b| b["text"].as_str().unwrap())
            .collect::<Vec<_>>()
            .join("\n---\n")
    );
}
