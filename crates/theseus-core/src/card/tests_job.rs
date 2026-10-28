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
    StandingKind, StandingListParams, StandingScope,
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

pub(super) struct Rig {
    pub(super) core: Arc<Core>,
    pub(super) fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

pub(super) fn rig(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    rig_with(FakeProvider::scripted(script), tweak)
}

fn rig_with(fake: FakeProvider, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.memory.mode = MemoryMode::Off;
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(fake);
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

pub(super) fn cli() -> Answerer {
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

pub(super) async fn archive(r: &Rig) {
    import(&r.core, &fixture::export(theseus_protocol::now_unix_ms())).await;
}

/// The model's answer: `(cluster, text, kind, episode label)` each.
pub(super) fn states(cards: &[(&str, &str, &str, &str)]) -> Scripted {
    let cards: Vec<Value> = cards
        .iter()
        .map(|(k, t, kind, e)| {
            json!({"cluster": k, "text": t, "kind": kind, "scope": "everywhere", "episodes": [e]})
        })
        .collect();
    Scripted::tools("", &[("tu", TOOL, json!({ "cards": cards }))])
}

/// K1 is the opinion cluster (3 sources), K2 the gate one (2).
pub(super) fn two() -> Scripted {
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

pub(super) async fn propose(r: &Rig) -> CardProposeResult {
    r.core
        .card_propose(&Default::default(), cli())
        .await
        .unwrap()
}

pub(super) async fn answer(
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

/// The owner card asks its own target model's forcing: on a Sonnet profile
/// the call is not forced (theseus-7q9wt). (Planted revert: `forcing` of
/// Haiku at the call site.)
#[tokio::test]
async fn on_a_sonnet_profile_the_card_call_is_not_forced() {
    let r = rig(vec![two()], |c| c.card.profile = "sonnet".into());
    archive(&r).await;
    propose(&r).await;
    let sent = r.fake.requests();
    assert_eq!(sent.len(), 1);
    crate::provider::one_tool::assert_unforced(&sent[0], TOOL);
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

/// A card call cut at its cap names the cap (theseus-opypg): the cut tool
/// input is empty, and "the answer held no state_cards call" hid the cause.
#[tokio::test]
async fn a_card_call_cut_at_its_cap_says_so() {
    let cut = Scripted::Blocks {
        blocks: vec![json!({"type": "tool_use", "id": "tu", "name": TOOL, "input": {}})],
        stop_reason: "max_tokens".into(),
    };
    let r = rig(vec![cut], |_| {});
    archive(&r).await;
    let got = propose(&r).await;
    let why = got.stopped.as_deref().unwrap();
    assert!(why.contains("max_tokens") && why.contains("1,500"), "{why}");
    assert!(!why.contains("held no"), "{why}");
}

/// The card call turns thinking off where the model can (theseus-opypg):
/// its 1,500 cap would be spent thinking first.
#[tokio::test]
async fn the_card_call_turns_thinking_off() {
    let r = rig(vec![two()], |_| {});
    archive(&r).await;
    propose(&r).await;
    let sent = r.fake.requests();
    let want = r.core.runner.catalog.thinking_off(&sent[0].model);
    assert!(
        want.is_some(),
        "{}: a model that turns thinking off",
        sent[0].model
    );
    assert_eq!(sent[0].thinking, want);
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
        ask: Default::default(),
        record: None,
        observations: Vec::new(),
        posted_ms: None,
    };
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(serde_json::from_value::<CardProposal>(v).unwrap(), p);
}

/// A tracing writer that keeps the journal's lines.
#[derive(Clone, Default)]
struct Log(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Log {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Log {
    type Writer = Log;
    fn make_writer(&'a self) -> Log {
        self.clone()
    }
}

impl Log {
    fn count(&self, needle: &str) -> usize {
        let text = String::from_utf8(self.0.lock().unwrap().clone()).unwrap();
        text.matches(needle).count()
    }
}

/// One proposal open under `max_open = 1`, and the last run set back past
/// the interval.
async fn full_rig() -> Rig {
    let r = rig(vec![two(), two()], |c| c.card.max_open = 1);
    archive(&r).await;
    assert_eq!(propose(&r).await.proposed.len(), 1);
    let old = State {
        last_run_ms: Some(theseus_protocol::now_unix_ms() - 30 * 86_400_000),
        ..State::read(&r.core.store).unwrap()
    };
    r.core.people_write(&[old.record().unwrap()]).await.unwrap();
    r
}

/// A run that finds the queue full is an attempt: it records itself, scans
/// nothing, and the next one is `every_days` away (theseus-15kn3).
#[tokio::test]
async fn a_full_card_records_its_attempt_and_scans_nothing() {
    let r = full_rig().await;
    let start = theseus_protocol::now_unix_ms();
    let got = r.core.card_run(false).await.unwrap();
    assert!(
        got.stopped.as_deref().unwrap().starts_with("full"),
        "{got:?}"
    );
    assert_eq!(
        (got.evidence, got.clusters, got.fresh, got.calls),
        (0, 0, 0, 0)
    );
    let last = State::read(&r.core.store).unwrap().last_run_ms.unwrap();
    assert!(last >= start, "{last} < {start}");
    let cfg = &r.core.cfg;
    let (at, _) = super::tender::next_at(
        theseus_protocol::now_unix_ms(),
        cfg.judge.learning_hour,
        cfg.card.every_days,
        Some(last),
    );
    assert!(
        at >= last + u64::from(cfg.card.every_days) * 86_400_000,
        "{at}"
    );
    // A dry run still counts, and writes nothing.
    let before = r.core.store.stats().unwrap().last_position;
    let dry = r.core.card_run(true).await.unwrap();
    assert!(dry.evidence > 0 && dry.stopped.as_deref().unwrap().starts_with("full"));
    assert_eq!(r.core.store.stats().unwrap().last_position, before);
}

/// The tender runs a full card once in its window, not again and again, and
/// the journal says so once (tokio's paused clock).
#[tokio::test(start_paused = true)]
async fn a_full_card_does_not_rerun_within_its_interval() {
    let r = full_rig().await;
    let log = Log::default();
    let _g = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_writer(log.clone())
            .with_ansi(false)
            .finish(),
    );
    r.core.card_after_serving();
    tokio::time::sleep(
        crate::learning::tender::AFTER_START + std::time::Duration::from_secs(3 * 3600),
    )
    .await;
    assert_eq!(log.count("card: the run finished"), 1);
}

/// A run that fails writes no state; it is not due again at once either.
#[tokio::test(start_paused = true)]
async fn a_failed_card_run_is_not_due_again_at_once() {
    let r = rig(vec![], |c| c.card.profile = "no-such-profile".into());
    let log = Log::default();
    let _g = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_writer(log.clone())
            .with_ansi(false)
            .finish(),
    );
    r.core.card_after_serving();
    tokio::time::sleep(
        crate::learning::tender::AFTER_START + std::time::Duration::from_secs(3 * 3600),
    )
    .await;
    assert_eq!(log.count("card: the run did not happen"), 1);
}

/// A full queue pauses the job, it does not turn it off: once the owner
/// answers one, a run no longer stops "full" and reads the archive again.
#[tokio::test]
async fn a_card_below_max_open_runs_again_on_its_normal_schedule() {
    let r = full_rig().await;
    r.core.card_run(false).await.unwrap();
    let last = State::read(&r.core.store).unwrap().last_run_ms.unwrap();
    let open = state::proposals(&r.core.store).unwrap();
    answer(&r, &open[0].id, "decline", None).await;
    let cfg = &r.core.cfg;
    let day = 86_400_000;
    let (at, why) = super::tender::next_at(
        last,
        cfg.judge.learning_hour,
        cfg.card.every_days,
        Some(last),
    );
    assert_eq!(why, "weekly");
    assert!(at >= last + 7 * day && at < last + 9 * day, "{at}");
    let got = r.core.card_run(false).await.unwrap();
    assert!(got.stopped.is_none() && got.evidence > 0, "{got:?}");
}

/// The archive with the setup and one-task additions (theseus-ly75u).
async fn archive_with_setup(r: &Rig) {
    let now = theseus_protocol::now_unix_ms();
    let mut eps = fixture::export(now);
    eps.extend(fixture::setup_export(now));
    import(&r.core, &eps).await;
}

/// An assistant's setup note gives no lines, a USER file gives only its
/// owner line, a one-task line is dropped before any call, and the run's
/// result counts both. (Planted revert: `evidence::kept` keeps every line,
/// and the setup lines reach the request and the counts are 0.)
#[tokio::test]
async fn an_assistants_setup_and_one_tasks_words_are_no_evidence() {
    let r = rig(vec![two()], |_| {});
    archive_with_setup(&r).await;
    let got = propose(&r).await;
    let sent = bodies(&r);
    assert_eq!(sent.len(), 1, "{got:?}");
    let leaked = fixture::SETUP_LINES
        .iter()
        .chain([&fixture::HELPER_LINE, &fixture::ONE_TASK_LINE]);
    for line in leaked {
        assert!(!sent[0].contains(line), "{line} reached the model");
    }
    assert!(
        sent[0].contains(fixture::OWNER_LINE),
        "the owner's line did"
    );
    assert_eq!((got.setup, got.one_task), (4, 1), "{got:?}");
}

/// The model's `one_task` mark drops a statement (an invented project's
/// merge steps); a general preference is still proposed everywhere.
/// (Planted revert: read `one_task` as everywhere in `ask::parse`.)
#[tokio::test]
async fn a_one_task_statement_is_never_proposed_everywhere() {
    let answer = Scripted::tools(
        "",
        &[(
            "tu",
            TOOL,
            json!({"cards": [
                {"cluster": "K1", "text": "wants your opinion when you have one", "kind": "preference", "scope": "everywhere", "episodes": ["E1"]},
                {"cluster": "K2", "text": "merges the quay-ledger lanes with a plain merge, then regenerates its lockfile", "kind": "preference", "scope": "one_task", "episodes": ["E4"]},
            ]}),
        )],
    );
    let r = rig(vec![answer], |_| {});
    archive(&r).await;
    let got = propose(&r).await;
    assert_eq!(got.proposed.len(), 1, "{got:?}");
    assert_eq!(got.proposed[0].scope, StandingScope::Everywhere);
    assert_eq!(got.proposed[0].text, "wants your opinion when you have one");
    assert_eq!(got.one_task, 1, "{got:?}");
}

/// Two statements of one call that conflict yield one proposal, the better
/// supported, and one noted conflict. (Planted revert: `settle_conflicts`
/// keeps every statement.)
#[tokio::test]
async fn two_conflicting_statements_yield_one_proposal_and_a_note() {
    let answer = Scripted::tools(
        "",
        &[(
            "tu",
            TOOL,
            json!({"cards": [
                {"cluster": "K1", "text": "wants to be asked before any commit is made", "kind": "preference", "scope": "everywhere", "episodes": ["E1"], "conflicts": ["K2"]},
                {"cluster": "K2", "text": "wants commits made without asking once the gate is green", "kind": "preference", "scope": "everywhere", "episodes": ["E4"]},
            ]}),
        )],
    );
    let r = rig(vec![answer], |_| {});
    archive(&r).await;
    let got = propose(&r).await;
    assert_eq!(got.proposed.len(), 1, "{got:?}");
    // K1 has three sources, K2 two: K1 stays.
    assert_eq!(
        got.proposed[0].text,
        "wants to be asked before any commit is made"
    );
    assert_eq!(got.conflicts.len(), 1, "{got:?}");
    assert!(got.conflicts[0].contains("without asking"), "{got:?}");
}

/// The owner's scope on keep and edit: the record holds there; an unknown
/// form is refused and nothing is written; a kept record moves to another
/// scope with its words and source, the old one ended. (Planted revert:
/// `card_answer` ignores `scope`, and the record is everywhere.)
#[tokio::test]
async fn the_owner_sets_a_kept_records_scope() {
    let r = rig(vec![two()], |_| {});
    archive(&r).await;
    let got = propose(&r).await;
    let (a, b) = (got.proposed[0].clone(), got.proposed[1].clone());
    let with = |id: &str, option: &str, text: Option<&str>, scope: &str| CardAnswerParams {
        id: id.into(),
        option: option.into(),
        text: text.map(String::from),
        scope: Some(scope.into()),
        ..Default::default()
    };
    // An unknown form: refused with the forms, and nothing written.
    let before = r.core.store.stats().unwrap().last_position;
    let e = r
        .core
        .card_answer(&with(&a.id, "keep", None, "work:coding"), cli())
        .await
        .unwrap_err();
    assert!(e.message.contains("surface:NAME"), "{}", e.message);
    assert_eq!(r.core.store.stats().unwrap().last_position, before);
    let e = r
        .core
        .card_answer(&with(&a.id, "keep", None, "surface:phone"), cli())
        .await
        .unwrap_err();
    assert!(e.message.contains("terminal"), "{}", e.message);
    // Keep with a surface.
    let kept = r
        .core
        .card_answer(&with(&a.id, "keep", None, "surface:terminal"), cli())
        .await
        .unwrap();
    let rec = kept.record.unwrap();
    assert_eq!(rec.scope, StandingScope::Surface("terminal".into()));
    assert_eq!(kept.proposal.scope, rec.scope);
    // Edit with a scope.
    let edited = r
        .core
        .card_answer(
            &with(
                &b.id,
                "edit",
                Some("wants the gate green before a commit"),
                "surface:cockpit",
            ),
            cli(),
        )
        .await
        .unwrap();
    let rec_b = edited.record.unwrap();
    assert_eq!(rec_b.scope, StandingScope::Surface("cockpit".into()));
    // A scope move of the edited one: its words and source, the old ended.
    let moved = r
        .core
        .card_answer(&with(&b.id, "scope", None, "everywhere"), cli())
        .await
        .unwrap();
    let rec2 = moved.record.unwrap();
    assert_eq!(rec2.scope, StandingScope::Everywhere);
    assert_eq!(rec2.text, "wants the gate green before a commit");
    assert_eq!(rec2.source, format!("card:{}", b.id));
    assert_eq!(rec2.kind, rec_b.kind);
    assert_eq!(rec2.ends.as_deref(), Some(rec_b.id.as_str()));
    assert_eq!(
        moved.proposal.standing_id.as_deref(),
        Some(rec2.id.as_str())
    );
    assert_eq!(moved.proposal.state, ProposalState::Kept);
    let live = r
        .core
        .standing_list(&StandingListParams::default(), cli())
        .unwrap();
    assert_eq!(live.records.len(), 2, "{live:?}");
    assert!(live.records.iter().all(|i| i.record.id != rec_b.id));
    // A move names its scope.
    let bare = CardAnswerParams {
        id: b.id.clone(),
        option: "scope".into(),
        ..Default::default()
    };
    let e = r.core.card_answer(&bare, cli()).await.unwrap_err();
    assert!(e.message.contains("gives the scope"), "{}", e.message);
}

/// A decline carries no scope: it is refused before anything is written, and
/// the proposal stays open (theseus-0ly2a). (Planted revert: `card_answer`
/// ignores the scope of a decline.)
#[tokio::test]
async fn a_decline_with_a_scope_is_refused() {
    let r = rig(vec![two()], |_| {});
    archive(&r).await;
    let got = propose(&r).await;
    let a = got.proposed[0].clone();
    let before = r.core.store.stats().unwrap().last_position;
    let e = r
        .core
        .card_answer(
            &CardAnswerParams {
                id: a.id.clone(),
                option: "decline".into(),
                scope: Some("everywhere".into()),
                ..Default::default()
            },
            cli(),
        )
        .await
        .unwrap_err();
    assert!(e.message.contains("takes no scope"), "{}", e.message);
    assert_eq!(r.core.store.stats().unwrap().last_position, before);
    let kept = r
        .core
        .card_answer(
            &CardAnswerParams {
                id: a.id,
                option: "keep".into(),
                ..Default::default()
            },
            cli(),
        )
        .await
        .unwrap();
    assert_eq!(kept.proposal.state, ProposalState::Kept);
}

/// The fixture's archive and six more single-line conversations, so a run
/// has ten clusters and makes two calls (the first reads eight).
async fn wide_archive(r: &Rig) {
    let now = theseus_protocol::now_unix_ms();
    let mut eps = fixture::export(now);
    let lines = [
        "Water the lantern garden before dusk every day",
        "Order fresh rope from the chandler each spring",
        "Pack the kettle and spare gloves for winter surveys",
        "Send the monthly invoice to the ferry office on Friday",
        "Label every sample jar with the cove name",
        "Book the dry dock a season ahead",
    ];
    for (i, l) in lines.iter().enumerate() {
        eps.push(fixture::episode(
            now,
            &fixture::Ep {
                n: 11 + i as u64,
                place: ("dm", None),
                sensitivity: "personal",
                partner: None,
                book_hint: None,
                triage: Some("decision_or_preference"),
                days_ago: 1 + i as u64,
                owner: &[l],
                outside: &[],
            },
        ));
    }
    import(&r.core, &eps).await;
}

/// The first call states the opinion entry; the second names it, as `P1`, in
/// the `conflicts` of its own.
fn two_calls() -> Vec<Scripted> {
    vec![
        states(&[(
            "K1",
            "wants your opinion when you have one",
            "preference",
            "E1",
        )]),
        Scripted::tools(
            "",
            &[(
                "tu",
                TOOL,
                json!({"cards": [{"cluster": "K1", "text": "wants a menu of options and no opinion", "kind": "preference", "scope": "everywhere", "episodes": ["E1"], "conflicts": ["P1"]}]}),
            )],
        ),
    ]
}

/// A statement that conflicts with an entry an earlier call of the run made
/// is not offered, and the run notes it; the later request lists the entry
/// as `P1` after its clusters and is otherwise the first's bytes.
/// (Planted revert: `parse` ignores `P` labels.)
#[tokio::test]
async fn a_conflict_across_two_calls_of_one_run_is_not_offered() {
    let r = rig(two_calls(), |_| {});
    wide_archive(&r).await;
    let got = propose(&r).await;
    assert_eq!(got.calls, 2, "{got:?}");
    assert_eq!(got.proposed.len(), 1, "{got:?}");
    assert_eq!(got.conflicts.len(), 1, "{got:?}");
    assert!(
        got.conflicts[0].starts_with("kept 'wants your opinion when you have one' over"),
        "{got:?}"
    );
    let sent = r.fake.requests();
    assert_eq!(sent.len(), 2);
    let text = |q: &crate::provider::ProviderRequest| q.messages[0].to_string();
    assert!(!text(&sent[0]).contains("P1:"));
    assert!(text(&sent[1]).contains("P1: wants your opinion when you have one"));
    assert_eq!(sent[0].system, sent[1].system);
    assert_eq!(sent[0].tools, sent[1].tools);
}

fn observation(
    id: &str,
    at_ms: u64,
    state: theseus_protocol::ObservationState,
) -> theseus_protocol::CardObservation {
    theseus_protocol::CardObservation {
        id: id.into(),
        session: "ses_lighthouse".into(),
        nodes: vec![format!("nod_{id}")],
        kind: theseus_protocol::ObservationKind::Preference,
        text: format!("keeps the {id} log in metres"),
        at_ms,
        contradicts: None,
        state,
        proposal: None,
    }
}

/// `card.observed` lists the open observations by default, newest first, each
/// state on request, and counts all three (theseus-6f8t0).
#[tokio::test]
async fn card_observed_lists_by_state_with_the_counts() {
    use theseus_protocol::{CardObservedParams, ObservationState as S};
    let r = rig(vec![], |_| {});
    for (id, at, st) in [
        ("cob_a", 10, S::Open),
        ("cob_b", 20, S::Used),
        ("cob_c", 30, S::Dropped),
        ("cob_d", 40, S::Open),
    ] {
        r.core
            .store
            .put_meta(&state::obs_key(id), &observation(id, at, st))
            .unwrap();
    }
    let ask = |state, limit| CardObservedParams {
        state,
        limit,
        ..Default::default()
    };
    let got = r.core.card_observed(&ask(None, None), cli()).unwrap();
    let ids = |g: &theseus_protocol::CardObservedResult| {
        g.observations
            .iter()
            .map(|o| o.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&got), ["cob_d", "cob_a"]);
    assert_eq!((got.open, got.used, got.dropped), (2, 1, 1));
    assert!(got.observe);
    let used = r
        .core
        .card_observed(&ask(Some(S::Used), None), cli())
        .unwrap();
    assert_eq!(ids(&used), ["cob_b"]);
    let dropped = r
        .core
        .card_observed(&ask(Some(S::Dropped), None), cli())
        .unwrap();
    assert_eq!(ids(&dropped), ["cob_c"]);
    let one = r.core.card_observed(&ask(None, Some(1)), cli()).unwrap();
    assert_eq!(ids(&one), ["cob_d"]);
    // Writing nothing: a read leaves the observations as they were.
    assert_eq!(state::observations(&r.core.store, None).unwrap().len(), 4);
}

/// An observation round-trips; a kind that is a trait is no kind at all.
#[test]
fn an_observation_round_trips_and_has_no_trait_kind() {
    let o = observation("cob_x", 5, theseus_protocol::ObservationState::Open);
    let v = serde_json::to_value(&o).unwrap();
    assert_eq!(v["kind"], "preference");
    assert_eq!(v["state"], "open");
    assert_eq!(
        serde_json::from_value::<theseus_protocol::CardObservation>(v.clone()).unwrap(),
        o
    );
    let mut bad = v;
    bad["kind"] = json!("trait");
    assert!(serde_json::from_value::<theseus_protocol::CardObservation>(bad).is_err());
}
