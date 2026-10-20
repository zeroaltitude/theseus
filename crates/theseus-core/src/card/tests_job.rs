//! The card's job through a whole core (theseus-slvl.6), over the invented
//! archive (`fixture`): labels respected (a partner-confidential episode and
//! an outside line never reach the model's request), proposals that cite
//! their sources, a second run that sends nothing, a declined proposal that
//! does not come back, a kept one that writes the standing record with its
//! sources, the cap and the expiry, the budget, and the owner-only acts.
//! Every name is invented.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::import::{ImportEpisodesParams, ImportEpisodesResult};
use theseus_protocol::{
    method, CardAnswerParams, CardListParams, CardProposal, CardProposeResult, ProposalState,
    StandingKind, StandingListParams,
};

use super::ask::TOOL;
use super::fixture::{self, CHATTER, OUTSIDE, SECRET};
use super::state::{self, State};
use crate::approval::{Answerer, Surface};
use crate::config::{Config, MemoryMode};
use crate::import::tests::{call, lines};
use crate::provider::{FakeProvider, Scripted};
use crate::rpc::Core;
use crate::store::Store;
use crate::test_support::ledger_rows_2k as rows;

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn rig(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.memory.mode = MemoryMode::Off;
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

async fn import(c: &Arc<Core>, eps: &[Value]) {
    let p = ImportEpisodesParams {
        file: "export.jsonl".into(),
        lines: lines(eps),
        batch: None,
        batches: None,
    };
    let got = call(c, method::IMPORT_EPISODES, serde_json::to_value(p).unwrap())
        .await
        .unwrap();
    let got: ImportEpisodesResult = serde_json::from_value(got).unwrap();
    assert_eq!(got.rejected.len(), 0, "{got:?}");
}

async fn archive(r: &Rig) {
    import(&r.core, &fixture::export(theseus_protocol::now_unix_ms())).await;
}

/// The model's answer: `(cluster, text, kind, episode label)` each.
fn states(cards: &[(&str, &str, &str, &str)]) -> Scripted {
    let cards: Vec<Value> = cards
        .iter()
        .map(|(k, t, kind, e)| {
            json!({"cluster": k, "text": t, "kind": kind, "scope": "everywhere", "episodes": [e]})
        })
        .collect();
    Scripted::tools("", &[("tu", TOOL, json!({ "cards": cards }))])
}

/// K1 is the opinion cluster (3 sources), K2 the gate one (2).
fn two() -> Scripted {
    states(&[
        (
            "K1",
            "wants your opinion when you have one",
            "preference",
            "E1",
        ),
        (
            "K2",
            "wants the gate green before any commit",
            "preference",
            "E4",
        ),
    ])
}

async fn propose(r: &Rig) -> CardProposeResult {
    r.core
        .card_propose(&Default::default(), cli())
        .await
        .unwrap()
}

async fn answer(
    r: &Rig,
    id: &str,
    option: &str,
    text: Option<&str>,
) -> theseus_protocol::CardAnswerResult {
    let p = CardAnswerParams {
        id: id.into(),
        option: option.into(),
        text: text.map(String::from),
        ..Default::default()
    };
    r.core.card_answer(&p, cli()).await.unwrap()
}

fn bodies(r: &Rig) -> Vec<String> {
    r.fake
        .requests()
        .iter()
        .map(|q| serde_json::to_string(q).unwrap())
        .collect()
}

#[test]
fn the_fixtures_partner_line_carries_the_secret() {
    assert!(fixture::SECRET_LINE.contains(SECRET));
}

/// A partner-confidential episode is counted and never read: its text is in
/// no request, and neither is an outside line or a chat the labels do not
/// make evidence. (Planted revert: skip the partner check in
/// `evidence::scan`, and the secret is in the request.)
#[tokio::test]
async fn labels_are_respected_before_any_call() {
    let r = rig(vec![two()], |_| {});
    archive(&r).await;
    let got = propose(&r).await;
    let sent = bodies(&r);
    assert_eq!(sent.len(), 1);
    for secret in [SECRET, OUTSIDE, CHATTER, "kestrel"] {
        assert!(
            !sent[0].contains(secret),
            "{secret} reached the model: {}",
            sent[0]
        );
    }
    assert!(sent[0].contains("opinion"), "the evidence did");
    assert_eq!((got.withheld, got.calls), (1, 1), "{got:?}");
}

/// Each statement is a question that cites the episodes its cluster rests on.
#[tokio::test]
async fn a_run_proposes_cited_questions() {
    let r = rig(vec![two()], |_| {});
    archive(&r).await;
    let got = propose(&r).await;
    assert_eq!(got.proposed.len(), 2, "{got:?}");
    let p = &got.proposed[0];
    assert_eq!(p.text, "wants your opinion when you have one");
    assert_eq!(
        p.question,
        "Add to your card: 'wants your opinion when you have one' (from 3 sources)? [Keep] [Edit] [Not this]"
    );
    assert_eq!(p.options, ["Keep", "Edit", "Not this"]);
    assert_eq!(
        (p.state, p.support, p.kind),
        (ProposalState::Open, 3, StandingKind::Preference)
    );
    let ids: Vec<&str> = p.sources.iter().map(|s| s.episode.as_str()).collect();
    for n in [1, 2, 3] {
        assert!(ids.contains(&fixture::id_of(n).as_str()), "{ids:?}");
    }
    assert!(!ids.contains(&fixture::id_of(7).as_str()));
    assert_eq!(p.expires_ms - p.created_ms, 14 * 86_400_000);
    // The call is a spend row; the proposals are stored and listed.
    let spent = rows(&r.core, "card.proposed");
    assert_eq!(spent.len(), 1);
    assert_eq!(spent[0].data["proposals"].as_array().unwrap().len(), 2);
    assert!(spent[0].data["cost_usd"].as_f64().is_some());
    let list = r
        .core
        .card_list(&CardListParams::default(), cli())
        .await
        .unwrap();
    assert_eq!(list.open.len(), 2);
    assert!(list.spent_today_usd > 0.0 && list.spent_total_usd == list.spent_today_usd);
    // A stored proposal is the one answered.
    let stored = state::proposals(&r.core.store).unwrap();
    assert_eq!(stored.len(), 2);
}

/// A second run over the same history proposes nothing and asks nothing.
/// (Planted revert: forget the sent episodes and the open proposals' sources
/// in `state::seen`, and the second run calls again.)
#[tokio::test]
async fn a_second_run_proposes_nothing_new() {
    let r = rig(vec![two(), two()], |_| {});
    archive(&r).await;
    assert_eq!(propose(&r).await.proposed.len(), 2);
    let again = propose(&r).await;
    assert_eq!((again.calls, again.proposed.len()), (0, 0), "{again:?}");
    assert_eq!(r.fake.requests().len(), 1);
}

/// "Not this" is remembered: the sources never come back, however the history
/// grows around them, and the same words from another cluster are dropped.
#[tokio::test]
async fn a_declined_proposal_does_not_come_back() {
    let r = rig(
        vec![
            two(),
            // After the decline a new, unrelated cluster appears; the model
            // restates the declined words for it.
            states(&[(
                "K1",
                "wants an opinion when you have one",
                "preference",
                "E1",
            )]),
        ],
        |_| {},
    );
    archive(&r).await;
    let first = propose(&r).await;
    let opinion = first.proposed[0].id.clone();
    let declined = answer(&r, &opinion, "decline", None).await;
    assert_eq!(declined.proposal.state, ProposalState::Declined);
    assert!(declined.record.is_none());
    // One more episode in the declined cluster: one of four is new, not most.
    import(
        &r.core,
        &[fixture::episode(
            theseus_protocol::now_unix_ms(),
            &fixture::Ep {
                n: 20,
                place: ("dm", None),
                sensitivity: "personal",
                partner: None,
                book_hint: None,
                triage: Some("decision_or_preference"),
                days_ago: 1,
                owner: &[
                    "Give me your opinion about the mooring layout rather than a menu of options",
                ],
                outside: &[],
            },
        )],
    )
    .await;
    let again = propose(&r).await;
    assert_eq!((again.calls, again.proposed.len()), (0, 0), "{again:?}");
    // And a different cluster whose statement repeats the declined words.
    import(
        &r.core,
        &[fixture::episode(
            theseus_protocol::now_unix_ms(),
            &fixture::Ep {
                n: 21,
                place: ("dm", None),
                sensitivity: "personal",
                partner: None,
                book_hint: None,
                triage: Some("decision_or_preference"),
                days_ago: 1,
                owner: &["Tides at the north jetty turn two hours later than the almanac says"],
                outside: &[],
            },
        )],
    )
    .await;
    let third = propose(&r).await;
    assert_eq!(third.calls, 1, "{third:?}");
    assert_eq!(
        third.proposed.len(),
        0,
        "the declined words were dropped: {third:?}"
    );
    assert_eq!(third.dropped, 1);
}

/// A kept proposal writes the standing record with its source, and the
/// proposal keeps the episodes. An edit keeps the owner's words.
/// (Planted revert: write the proposal without its sources, or the record
/// without `card:<id>`, and the assertions on both fail.)
#[tokio::test]
async fn a_kept_proposal_writes_the_record_with_its_sources() {
    let r = rig(vec![two()], |_| {});
    archive(&r).await;
    let got = propose(&r).await;
    let (a, b) = (got.proposed[0].clone(), got.proposed[1].clone());
    let kept = answer(&r, &a.id, "keep", None).await;
    let rec = kept.record.expect("a record");
    assert_eq!(rec.source, format!("card:{}", a.id));
    assert_eq!(
        (rec.kind, rec.text.as_str()),
        (StandingKind::Preference, a.text.as_str())
    );
    assert_eq!(kept.proposal.state, ProposalState::Kept);
    assert_eq!(kept.proposal.standing_id.as_deref(), Some(rec.id.as_str()));
    assert!(!kept.proposal.sources.is_empty());
    assert_eq!(kept.proposal.sources, a.sources);
    let edited = answer(
        &r,
        &b.id,
        "edit",
        Some("  wants   the whole gate green before a commit "),
    )
    .await;
    let rec2 = edited.record.unwrap();
    assert_eq!(rec2.text, "wants the whole gate green before a commit");
    assert_eq!(
        edited.proposal.kept_text.as_deref(),
        Some(rec2.text.as_str())
    );
    // Both are live standing records.
    let live = r
        .core
        .standing_list(&StandingListParams::default(), cli())
        .unwrap();
    assert_eq!(live.records.len(), 2);
    // The rows name the episodes.
    let answered = rows(&r.core, "card.answered");
    assert_eq!(answered.len(), 2);
    assert_eq!(answered[0].data["option"], "keep");
    assert_eq!(
        answered[0].data["episodes"].as_array().unwrap().len(),
        a.sources.len()
    );
    assert_eq!(answered[0].data["standing"], rec.id.as_str());
    // An answered proposal is answered once.
    let again = r
        .core
        .card_answer(
            &CardAnswerParams {
                id: a.id.clone(),
                option: "keep".into(),
                ..Default::default()
            },
            cli(),
        )
        .await
        .unwrap_err();
    assert!(again.message.contains("already kept"), "{}", again.message);
}

/// An edit gives its words; an unknown option is refused.
#[tokio::test]
async fn an_edit_needs_its_words_and_an_option_must_be_one() {
    let r = rig(vec![two()], |_| {});
    archive(&r).await;
    let id = propose(&r).await.proposed[0].id.clone();
    let p = |option: &str| CardAnswerParams {
        id: id.clone(),
        option: option.into(),
        ..Default::default()
    };
    assert!(r
        .core
        .card_answer(&p("edit"), cli())
        .await
        .unwrap_err()
        .message
        .contains("--text"));
    assert!(r
        .core
        .card_answer(&p("maybe"), cli())
        .await
        .unwrap_err()
        .message
        .contains("options"));
    // The end of an id (four characters or more) finds it; fewer does not.
    let tail = &id[id.len() - 6..];
    assert!(r
        .core
        .card_answer(
            &CardAnswerParams {
                id: id[id.len() - 2..].into(),
                option: "keep".into(),
                ..Default::default()
            },
            cli()
        )
        .await
        .is_err());
    let done = r
        .core
        .card_answer(
            &CardAnswerParams {
                id: tail.into(),
                option: "Not this".into(),
                ..Default::default()
            },
            cli(),
        )
        .await
        .unwrap();
    assert_eq!(done.proposal.state, ProposalState::Declined);
}

/// At most `max_open` wait at once; the rest are dropped, and a run with the
/// queue full asks nothing.
#[tokio::test]
async fn the_queue_is_capped() {
    let r = rig(vec![two(), two()], |c| c.card.max_open = 1);
    archive(&r).await;
    let got = propose(&r).await;
    assert_eq!(got.proposed.len(), 1, "{got:?}");
    assert_eq!(got.dropped, 1);
    let again = propose(&r).await;
    assert_eq!((again.calls, again.proposed.len()), (0, 0));
    assert!(
        again.stopped.as_deref().unwrap().starts_with("full"),
        "{again:?}"
    );
}

/// An unanswered question becomes "Not this" after `ask_days`, with a row,
/// and its sources are not asked about again.
#[tokio::test]
async fn an_unanswered_question_expires_to_not_this() {
    let r = rig(vec![two(), two()], |c| c.card.ask_days = 3);
    archive(&r).await;
    let got = propose(&r).await;
    let soon = theseus_protocol::now_unix_ms() + 2 * 86_400_000;
    assert_eq!(r.core.card_expire(soon).await.unwrap(), 0, "not yet");
    let later = theseus_protocol::now_unix_ms() + 4 * 86_400_000;
    assert_eq!(r.core.card_expire(later).await.unwrap(), 2);
    let stored = state::proposals(&r.core.store).unwrap();
    assert!(stored
        .iter()
        .all(|p| p.state == ProposalState::Declined && p.answered_by.as_deref() == Some("expiry")));
    let expired = rows(&r.core, "card.answered");
    assert_eq!(
        (expired.len(), expired[0].data["option"].clone()),
        (2, json!("expire"))
    );
    let again = propose(&r).await;
    assert_eq!(again.calls, 0, "{again:?}");
    // The list applies it by itself when it is read.
    let _ = got;
}

/// A call whose worst case would pass `[card] day_usd` is not made.
/// (Planted revert: skip the check before the call, and a request goes out.)
#[tokio::test]
async fn the_budget_stops_the_job() {
    let r = rig(vec![two()], |c| c.card.day_usd = 0.000_001);
    archive(&r).await;
    let got = propose(&r).await;
    assert_eq!((got.calls, got.proposed.len()), (0, 0));
    assert!(
        got.stopped.as_deref().unwrap().starts_with("budget"),
        "{got:?}"
    );
    assert!(r.fake.requests().is_empty());
    let off = rig(vec![two()], |c| c.card.day_usd = 0.0);
    archive(&off).await;
    assert!(propose(&off).await.stopped.unwrap().starts_with("off"));
    assert!(off.fake.requests().is_empty());
}

/// The day's line is spent across runs: what a first run used is not there
/// for the second.
#[tokio::test]
async fn the_days_spend_carries_across_runs() {
    let r = rig(vec![two()], |_| {});
    archive(&r).await;
    propose(&r).await;
    let mut st = State::read(&r.core.store).unwrap();
    assert!(st.total_usd > 0.0);
    let spent = st.day_usd;
    // The line is now just what was spent: nothing more fits.
    st.day_usd = r.core.cfg.card.day_usd;
    r.core.store.put_meta(state::STATE_KEY, &st).unwrap();
    import(
        &r.core,
        &[fixture::episode(
            theseus_protocol::now_unix_ms(),
            &fixture::Ep {
                n: 30,
                place: ("dm", None),
                sensitivity: "personal",
                partner: None,
                book_hint: None,
                triage: Some("decision_or_preference"),
                days_ago: 1,
                owner: &["The north jetty light must be checked before dusk every single evening"],
                outside: &[],
            },
        )],
    )
    .await;
    let got = propose(&r).await;
    assert_eq!(got.calls, 0);
    assert!(
        got.stopped.as_deref().unwrap().starts_with("budget"),
        "{got:?} (spent {spent})"
    );
}

/// A dry run reads and counts, makes no call and writes nothing.
#[tokio::test]
async fn a_dry_run_makes_no_call() {
    let r = rig(vec![two()], |_| {});
    archive(&r).await;
    let before = r.core.store.stats().unwrap().last_position;
    let got = r
        .core
        .card_propose(
            &theseus_protocol::CardProposeParams {
                dry_run: true,
                ..Default::default()
            },
            cli(),
        )
        .await
        .unwrap();
    assert_eq!(
        (got.calls, got.proposed.len(), got.evidence, got.withheld),
        (0, 0, 6, 1),
        "{got:?}"
    );
    assert!(got.at_most_usd > 0.0 && got.clusters >= 4);
    assert!(r.fake.requests().is_empty());
    assert_eq!(
        r.core.store.stats().unwrap().last_position,
        before,
        "a dry run writes nothing"
    );
}

/// A statement that repeats a live standing record is dropped.
#[tokio::test]
async fn a_live_record_is_not_proposed_again() {
    let r = rig(vec![two()], |_| {});
    archive(&r).await;
    let add = theseus_protocol::StandingAddParams {
        kind: StandingKind::Preference,
        text: "wants an opinion when you have one, not a menu".into(),
        ..Default::default()
    };
    r.core.standing_add(&add, cli()).unwrap();
    let got = propose(&r).await;
    assert_eq!(got.proposed.len(), 1, "{got:?}");
    assert_eq!(
        got.proposed[0].text,
        "wants the gate green before any commit"
    );
    assert_eq!(got.dropped, 1);
}

/// A call that fails writes its row and is not remembered as sent: the next
/// run reads the same clusters again.
#[tokio::test]
async fn a_failed_call_is_tried_again_next_run() {
    let r = rig(vec![Scripted::tools("", &[]), two()], |_| {});
    archive(&r).await;
    let first = propose(&r).await;
    assert_eq!(first.proposed.len(), 0);
    assert!(
        first.stopped.as_deref().unwrap().contains("state_cards"),
        "{first:?}"
    );
    assert!(rows(&r.core, "card.proposed")[0].data["failed"].is_string());
    let second = propose(&r).await;
    assert_eq!(second.proposed.len(), 2, "{second:?}");
}

/// Only the owner, from a private place, may ask, answer or list.
#[tokio::test]
async fn only_the_owner_may() {
    let r = rig(vec![two()], |_| {});
    archive(&r).await;
    let stranger = Answerer {
        label: "discord:777".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "777".into(),
            channel_id: "42".into(),
            guild_id: Some("7".into()),
        }),
        agent: None,
    };
    assert!(r
        .core
        .card_propose(&Default::default(), stranger.clone())
        .await
        .is_err());
    assert!(r
        .core
        .card_list(&CardListParams::default(), stranger.clone())
        .await
        .is_err());
    assert!(r.fake.requests().is_empty());
    let id = propose(&r).await.proposed[0].id.clone();
    let p = CardAnswerParams {
        id,
        option: "keep".into(),
        ..Default::default()
    };
    assert!(r.core.card_answer(&p, stranger).await.is_err());
    assert!(
        state::in_state(
            &state::proposals(&r.core.store).unwrap(),
            ProposalState::Open
        )
        .count()
            == 2
    );
}

/// `card.done` stops the nightly run's reads, and is the owner's.
#[tokio::test]
async fn done_is_kept_and_listed() {
    let r = rig(vec![], |_| {});
    let got = r
        .core
        .card_done(
            &theseus_protocol::CardDoneParams {
                done: true,
                ..Default::default()
            },
            cli(),
        )
        .await
        .unwrap();
    assert!(got.done);
    assert!(
        r.core
            .card_list(&CardListParams::default(), cli())
            .await
            .unwrap()
            .done
    );
    r.core
        .card_done(
            &theseus_protocol::CardDoneParams {
                done: false,
                ..Default::default()
            },
            cli(),
        )
        .await
        .unwrap();
    assert!(
        !r.core
            .card_list(&CardListParams::default(), cli())
            .await
            .unwrap()
            .done
    );
}

/// The nightly run is paced by the interval from the last run.
#[test]
fn the_nightly_run_waits_out_the_interval() {
    use super::tender::next_at;
    let day = 86_400_000;
    let now = 1_791_633_600_000; // 2026-10-10T12:00:00Z
                                 // Never run: due now.
    assert_eq!(next_at(now, 3, 7, None).0, now);
    // Ran an hour ago: not before a week is out, and then at the learning hour.
    let (at, why) = next_at(now, 3, 7, Some(now - 3_600_000));
    assert_eq!(why, "weekly");
    assert!(
        at >= now - 3_600_000 + 7 * day && at < now + 9 * day,
        "{at}"
    );
    // Ran eight days ago: due as the learning tender says.
    let (at, _) = next_at(now, 3, 7, Some(now - 8 * day));
    assert!(at <= now, "{at}");
}

/// The wire shapes round-trip.
#[test]
fn a_proposal_round_trips() {
    let p = CardProposal {
        id: "cdp_x".into(),
        kind: StandingKind::Fact,
        scope: Default::default(),
        text: "t".into(),
        question: "q".into(),
        options: vec![],
        sources: vec![],
        support: 1,
        created_ms: 1,
        expires_ms: 2,
        state: ProposalState::Open,
        run: "crd_x".into(),
        answered_ms: None,
        answered_by: None,
        kept_text: None,
        standing_id: None,
    };
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(serde_json::from_value::<CardProposal>(v).unwrap(), p);
}
