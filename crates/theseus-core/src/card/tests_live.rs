//! The card's live pass (theseus-6f8t0) through a whole core: observations
//! written straight to the store (`card.obs.<id>`, as the observer will),
//! the questions the nightly pass makes of them (Add, Pattern, Stale), their
//! caps, and the answers, including End it and Edit on a Stale question.
//! Every name is invented.

use theseus_protocol::{
    CardAnswerParams, CardAsk, CardObservation, CardProposal, CardProposeResult, ObservationKind,
    ObservationState, ProposalState, StandingAddParams, StandingKind, StandingRecord,
    StandingScope,
};

use super::state::{self, State};
use super::tests_job::{answer, archive, cli, propose, rig, states, two, Rig};
use crate::standing;

const DAY: u64 = 86_400_000;

fn obs(
    id: &str,
    session: &str,
    kind: ObservationKind,
    text: &str,
    days_ago: u64,
    contradicts: Option<&str>,
) -> CardObservation {
    CardObservation {
        id: id.into(),
        session: session.into(),
        nodes: vec![format!("nod_{id}")],
        kind,
        text: text.into(),
        at_ms: theseus_protocol::now_unix_ms() - days_ago * DAY,
        contradicts: contradicts.map(String::from),
        state: ObservationState::Open,
        proposal: None,
    }
}

fn put(r: &Rig, o: &CardObservation) {
    r.core.store.put_meta(&state::obs_key(&o.id), o).unwrap();
}

fn fact(id: &str, session: &str, text: &str) -> CardObservation {
    obs(id, session, ObservationKind::Fact, text, 1, None)
}

/// Three phrasings of one preference, in three conversations (they cluster).
const METRES: [&str; 3] = [
    "keeps the tide log in metres, never feet",
    "wants the tide log kept in metres, not feet",
    "the tide log stays in metres never in feet",
];

fn metres(r: &Rig, sessions: usize) {
    for (i, t) in METRES.iter().enumerate().take(sessions) {
        let o = obs(
            &format!("cob_m{i}"),
            &format!("ses_m{i}"),
            ObservationKind::Preference,
            t,
            (3 - i) as u64,
            None,
        );
        put(r, &o);
    }
}

fn statement() -> crate::provider::Scripted {
    states(&[("K1", "logs tide heights in metres", "preference", "E1")])
}

async fn live(r: &Rig) -> (CardProposeResult, Vec<CardProposal>) {
    let mut out = CardProposeResult::default();
    let made = r.core.card_live_run(false, &mut out).await.unwrap();
    (out, made)
}

fn observation(r: &Rig, id: &str) -> CardObservation {
    r.core
        .store
        .get_meta::<CardObservation>(&state::obs_key(id))
        .unwrap()
        .unwrap()
}

fn record(r: &Rig, text: &str) -> StandingRecord {
    let add = StandingAddParams {
        kind: StandingKind::Preference,
        text: text.into(),
        scope: StandingScope::Everywhere,
        ..Default::default()
    };
    r.core.standing_add(&add, cli()).unwrap().record
}

fn live_records(r: &Rig) -> Vec<StandingRecord> {
    let board = r.core.runner.standing.all(&r.core.store).unwrap();
    standing::live(&board, None, theseus_protocol::now_unix_ms())
        .into_iter()
        .cloned()
        .collect()
}

fn all_records(r: &Rig) -> Vec<StandingRecord> {
    r.core.runner.standing.all(&r.core.store).unwrap().to_vec()
}

/// One fact observation is one Add question citing its message, with no
/// model call, with the card done and the archive's interval not due.
/// (Planted revert: gate the live pass on `done` in the tender, or here.)
#[tokio::test]
async fn one_fact_gives_one_add_citing_its_message_even_when_done() {
    let r = rig(vec![], |_| {});
    let st = State {
        done: true,
        last_run_ms: Some(theseus_protocol::now_unix_ms()),
        ..State::default()
    };
    r.core.people_write(&[st.record().unwrap()]).await.unwrap();
    put(
        &r,
        &fact(
            "cob_f1",
            "ses_a",
            "keeps the survey boat at the north jetty",
        ),
    );
    let (_, made) = live(&r).await;
    assert_eq!(made.len(), 1, "{made:?}");
    let p = &made[0];
    assert_eq!(p.ask, CardAsk::Add);
    assert_eq!(p.kind, StandingKind::Fact);
    assert_eq!(p.observations, ["cob_f1"]);
    assert_eq!(
        (
            p.sources.len(),
            p.sources[0].node.as_str(),
            p.sources[0].kind.as_str()
        ),
        (1, "nod_cob_f1", "live")
    );
    assert_eq!(
        p.question,
        "Add to your card: 'keeps the survey boat at the north jetty' (from 1 source)? [Keep] [Edit] [Not this]"
    );
    assert!(r.fake.requests().is_empty(), "a lone fact needs no call");
    let o = observation(&r, "cob_f1");
    assert_eq!(
        (o.state, o.proposal.as_deref()),
        (ObservationState::Used, Some(p.id.as_str()))
    );
    assert_eq!(state::proposals(&r.core.store).unwrap().len(), 1);
}

/// A correction counts at support 1, as a fact does.
#[tokio::test]
async fn a_correction_counts_at_support_one() {
    let r = rig(vec![], |_| {});
    let c = obs(
        "cob_c1",
        "ses_a",
        ObservationKind::Correction,
        "uses metres for every tide height now",
        1,
        None,
    );
    put(&r, &c);
    let (_, made) = live(&r).await;
    assert_eq!((made.len(), made[0].support), (1, 1));
    assert_eq!(made[0].kind, StandingKind::Preference);
}

/// Preferences in two conversations give no Pattern (an Add of both at
/// most); in three they give one Pattern, support 3, worded as the owner
/// asked; with `pattern_sessions = 4` none. (Planted revert: `plan` takes a
/// Pattern at two sessions, and the first assertion fails.)
#[tokio::test]
async fn a_pattern_needs_its_distinct_conversations() {
    let r = rig(vec![statement()], |_| {});
    metres(&r, 2);
    let (_, made) = live(&r).await;
    assert!(made.iter().all(|p| p.ask != CardAsk::Pattern), "{made:?}");
    assert_eq!(made.len(), 1);
    assert_eq!((made[0].ask, made[0].support), (CardAsk::Add, 2));

    let r = rig(vec![statement()], |_| {});
    metres(&r, 3);
    let (_, made) = live(&r).await;
    assert_eq!(made.len(), 1, "{made:?}");
    let p = &made[0];
    assert_eq!((p.ask, p.support), (CardAsk::Pattern, 3));
    assert_eq!(p.observations.len(), 3);
    assert_eq!(p.sources.len(), 3);
    assert!(
        p.question
            .starts_with("You've said or chosen this in 3 conversations since ")
            && p.question.ends_with(
                ": 'logs tide heights in metres'. Add it to your card? [Keep] [Edit] [Not this]"
            ),
        "{}",
        p.question
    );
    for id in &p.observations {
        assert_eq!(observation(&r, id).state, ObservationState::Used);
    }

    let r = rig(vec![statement()], |c| c.card.pattern_sessions = 4);
    metres(&r, 3);
    let (_, made) = live(&r).await;
    assert!(made.iter().all(|p| p.ask != CardAsk::Pattern), "{made:?}");
}

/// A Pattern's statement call is the existing one: one call, one frame, its
/// `card.proposed` row, and no source that is not an observation.
#[tokio::test]
async fn the_statement_call_is_the_cards_one_and_one_frame() {
    let r = rig(vec![statement()], |_| {});
    metres(&r, 3);
    let (out, made) = live(&r).await;
    assert_eq!((out.calls, made.len()), (1, 1));
    assert_eq!(r.fake.requests().len(), 1);
    let sent = serde_json::to_string(&r.fake.requests()[0]).unwrap();
    assert!(sent.contains("tide log"), "{sent}");
    // The archive's own mark is not touched by the live pass.
    assert_eq!(State::read(&r.core.store).unwrap().last_run_ms, None);
}

/// A statement that cites none of its cluster is dropped, and its
/// observations stay open.
#[tokio::test]
async fn an_uncited_statement_is_dropped_and_the_observations_wait() {
    let bad = states(&[("K1", "logs tide heights in metres", "preference", "E9")]);
    let r = rig(vec![bad], |_| {});
    metres(&r, 3);
    let (_, made) = live(&r).await;
    assert!(made.is_empty());
    assert_eq!(
        state::observations(&r.core.store, Some(ObservationState::Open))
            .unwrap()
            .len(),
        3
    );
}

/// A Stale question per record, the owner's words in it; a second
/// observation of the same record makes no second question.
#[tokio::test]
async fn a_contradiction_asks_stale_once_per_record() {
    let r = rig(vec![], |_| {});
    let rec = record(&r, "prefers the survey boat named Heron");
    put(
        &r,
        &obs(
            "cob_s1",
            "ses_a",
            ObservationKind::Correction,
            "now uses the survey boat named Egret",
            2,
            Some(&rec.id),
        ),
    );
    put(
        &r,
        &obs(
            "cob_s2",
            "ses_b",
            ObservationKind::Preference,
            "says the Heron is laid up for the winter",
            1,
            Some(&rec.id),
        ),
    );
    let (_, made) = live(&r).await;
    assert_eq!(made.len(), 1, "{made:?}");
    let p = &made[0];
    assert_eq!(p.ask, CardAsk::Stale);
    assert_eq!(p.record.as_deref(), Some(rec.id.as_str()));
    assert_eq!(p.options, ["Keep", "Edit", "End it"]);
    assert_eq!(p.observations, ["cob_s1"]);
    assert!(
        p.question.starts_with("Your card says 'prefers the survey boat named Heron' (kept ")
            && p.question.ends_with(
                "you said 'now uses the survey boat named Egret'. Still true? [Keep] [Edit] [End it]"
            ),
        "{}",
        p.question
    );
    // Not an Add or a Pattern, and it stays open for another night.
    assert_eq!(observation(&r, "cob_s2").state, ObservationState::Open);
    assert!(r.fake.requests().is_empty());
    let (_, again) = live(&r).await;
    assert!(again.is_empty(), "one open Stale question a record");
    // The record is untouched by asking.
    assert_eq!(live_records(&r).len(), 1);
}

async fn stale_rig() -> (Rig, StandingRecord, CardProposal) {
    let r = rig(vec![], |_| {});
    let rec = record(&r, "prefers the survey boat named Heron");
    put(
        &r,
        &obs(
            "cob_s1",
            "ses_a",
            ObservationKind::Correction,
            "now uses the survey boat named Egret",
            2,
            Some(&rec.id),
        ),
    );
    let (_, made) = live(&r).await;
    let p = made[0].clone();
    (r, rec, p)
}

/// End it revokes the record, and the observation is used.
#[tokio::test]
async fn end_it_revokes_the_record() {
    let (r, rec, p) = stale_rig().await;
    let got = answer(&r, &p.id, "end", None).await;
    assert_eq!(got.proposal.state, ProposalState::Declined);
    assert!(live_records(&r).iter().all(|x| x.id != rec.id));
    assert!(live_records(&r).is_empty());
    assert_eq!(observation(&r, "cob_s1").state, ObservationState::Used);
}

/// Keep leaves the record as it is and sets the observation aside.
#[tokio::test]
async fn keep_leaves_the_record_and_drops_the_observation() {
    let (r, rec, p) = stale_rig().await;
    let got = answer(&r, &p.id, "keep", None).await;
    assert_eq!(got.proposal.state, ProposalState::Kept);
    assert_eq!(got.proposal.standing_id.as_deref(), Some(rec.id.as_str()));
    assert_eq!(live_records(&r), [rec]);
    assert_eq!(observation(&r, "cob_s1").state, ObservationState::Dropped);
    // Never asked again for it.
    let (_, again) = live(&r).await;
    assert!(again.is_empty());
}

/// Edit ends the old record and adds the owner's words in one frame: the new
/// record carries `ends`, and no record of an empty text (a revoke) is
/// written. (Planted revert: revoke, then add without `replaces`.)
#[tokio::test]
async fn edit_ends_the_old_record_and_adds_the_new_in_one_frame() {
    let (r, rec, p) = stale_rig().await;
    let before = r.core.store.stats().unwrap().last_position;
    let got = answer(
        &r,
        &p.id,
        "edit",
        Some("prefers the survey boat named Egret"),
    )
    .await;
    let after = r.core.store.stats().unwrap().last_position;
    assert_eq!(got.proposal.state, ProposalState::Kept);
    let new = got.record.unwrap();
    assert_eq!(new.ends.as_deref(), Some(rec.id.as_str()));
    assert_eq!(new.source, format!("card:{}", p.id));
    let live = live_records(&r);
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].id, new.id);
    // The record and its end are one write: nothing was written alone.
    assert_eq!(all_records(&r).len(), 2);
    assert!(all_records(&r).iter().all(|x| !x.text.is_empty()));
    // The standing frame (the record and its row) and the proposal's (it and
    // its row): four records; ending and adding apart would be six.
    assert_eq!(after - before, 4, "{before} {after}");
    assert_eq!(observation(&r, "cob_s1").state, ObservationState::Used);
}

/// An edit retried after a crash between the record and the proposal finds
/// the record the answer already made.
#[tokio::test]
async fn an_edit_is_finished_by_answering_again() {
    let (r, rec, p) = stale_rig().await;
    let add = StandingAddParams {
        kind: StandingKind::Preference,
        text: "prefers the survey boat named Egret".into(),
        scope: StandingScope::Everywhere,
        source: Some(format!("card:{}", p.id)),
        replaces: Some(rec.id.clone()),
        ..Default::default()
    };
    let made = r.core.standing_add(&add, cli()).unwrap().record;
    let got = answer(
        &r,
        &p.id,
        "edit",
        Some("prefers the survey boat named Egret"),
    )
    .await;
    assert_eq!(got.record.unwrap().id, made.id);
    assert_eq!(live_records(&r).len(), 1);
}

/// `end` answers a Stale question alone.
#[tokio::test]
async fn end_on_an_add_is_refused() {
    let r = rig(vec![], |_| {});
    put(
        &r,
        &fact(
            "cob_f1",
            "ses_a",
            "keeps the survey boat at the north jetty",
        ),
    );
    let (_, made) = live(&r).await;
    let p = CardAnswerParams {
        id: made[0].id.clone(),
        option: "end".into(),
        ..Default::default()
    };
    let e = r.core.card_answer(&p, cli()).await.unwrap_err();
    assert!(
        e.message.contains("not about a record you keep"),
        "{}",
        e.message
    );
    // Nothing moved.
    assert_eq!(observation(&r, "cob_f1").state, ObservationState::Used);
    assert_eq!(
        state::proposals(&r.core.store).unwrap()[0].state,
        ProposalState::Open
    );
}

/// Not this on a live question sets its observations aside: the same ones
/// are never proposed again, and a new one that repeats its words is set
/// aside too.
#[tokio::test]
async fn not_this_is_remembered_for_the_observations() {
    let r = rig(vec![], |_| {});
    put(
        &r,
        &fact(
            "cob_f1",
            "ses_a",
            "keeps the survey boat at the north jetty",
        ),
    );
    let (_, made) = live(&r).await;
    answer(&r, &made[0].id, "decline", None).await;
    assert_eq!(observation(&r, "cob_f1").state, ObservationState::Dropped);
    let (_, again) = live(&r).await;
    assert!(again.is_empty());
    put(
        &r,
        &fact(
            "cob_f2",
            "ses_b",
            "keeps the survey boat at the north jetty",
        ),
    );
    let (_, again) = live(&r).await;
    assert!(again.is_empty());
    assert_eq!(observation(&r, "cob_f2").state, ObservationState::Dropped);
    assert_eq!(state::proposals(&r.core.store).unwrap().len(), 1);
}

/// An Add kept or edited moves its observations to used and writes the
/// standing record with `card:<id>`.
#[tokio::test]
async fn keeping_a_live_add_writes_the_record() {
    let r = rig(vec![], |_| {});
    put(
        &r,
        &fact(
            "cob_f1",
            "ses_a",
            "keeps the survey boat at the north jetty",
        ),
    );
    let (_, made) = live(&r).await;
    let got = answer(&r, &made[0].id, "keep", None).await;
    let rec = got.record.unwrap();
    assert_eq!(rec.source, format!("card:{}", made[0].id));
    assert_eq!(rec.kind, StandingKind::Fact);
    assert_eq!(observation(&r, "cob_f1").state, ObservationState::Used);
}

/// An unanswered live question expires to Not this, and its observations
/// are set aside.
#[tokio::test]
async fn an_unanswered_live_question_expires_to_not_this() {
    let r = rig(vec![], |_| {});
    put(
        &r,
        &fact(
            "cob_f1",
            "ses_a",
            "keeps the survey boat at the north jetty",
        ),
    );
    let (_, made) = live(&r).await;
    let now = theseus_protocol::now_unix_ms() + 15 * DAY;
    assert_eq!(r.core.card_expire(now).await.unwrap(), 1);
    let p = &state::proposals(&r.core.store).unwrap()[0];
    assert_eq!(
        (p.state, p.answered_by.as_deref()),
        (ProposalState::Declined, Some("expiry"))
    );
    assert_eq!(made[0].id, p.id);
    assert_eq!(observation(&r, "cob_f1").state, ObservationState::Dropped);
}

/// `live_per_night` ends the night's questions, the rest stay open;
/// `max_open` does the same; and with three contradictions and
/// `stale_per_night = 2`, two Stale questions.
#[tokio::test]
async fn the_nights_caps_leave_the_rest_open() {
    let r = rig(vec![], |c| c.card.live_per_night = 2);
    for i in 0..3 {
        put(
            &r,
            &fact(
                &format!("cob_f{i}"),
                "ses_a",
                [
                    "keeps the survey boat at the north jetty",
                    "files tide tables by harbour",
                    "mows the lawn on sundays",
                ][i],
            ),
        );
    }
    let (_, made) = live(&r).await;
    assert_eq!(made.len(), 2);
    let (_, more) = live(&r).await;
    assert!(more.is_empty(), "the night's line was reached");
    assert_eq!(
        state::observations(&r.core.store, Some(ObservationState::Open))
            .unwrap()
            .len(),
        1
    );

    let r = rig(vec![], |c| c.card.max_open = 1);
    for i in 0..2 {
        put(
            &r,
            &fact(
                &format!("cob_g{i}"),
                "ses_a",
                [
                    "keeps the survey boat at the north jetty",
                    "files tide tables by harbour",
                ][i],
            ),
        );
    }
    let (_, made) = live(&r).await;
    assert_eq!(made.len(), 1);
    let (_, more) = live(&r).await;
    assert!(more.is_empty());

    let r = rig(vec![], |c| c.card.stale_per_night = 2);
    let recs = [
        record(&r, "prefers the survey boat named Heron"),
        record(&r, "files tide tables by harbour name"),
        record(&r, "mows the lawn on sundays only"),
    ];
    for (i, rec) in recs.iter().enumerate() {
        put(
            &r,
            &obs(
                &format!("cob_t{i}"),
                "ses_a",
                ObservationKind::Correction,
                [
                    "says the Heron is sold",
                    "files tide tables by date now",
                    "mows the lawn on saturdays",
                ][i],
                (3 - i) as u64,
                Some(&rec.id),
            ),
        );
    }
    let (_, made) = live(&r).await;
    assert_eq!(made.iter().filter(|p| p.ask == CardAsk::Stale).count(), 2);
    assert_eq!(
        state::observations(&r.core.store, Some(ObservationState::Open))
            .unwrap()
            .len(),
        1
    );
}

/// A proposal written before live learning (no `ask`) reads and answers as
/// an Add.
#[tokio::test]
async fn an_old_proposal_answers_as_an_add() {
    let r = rig(vec![], |_| {});
    let now = theseus_protocol::now_unix_ms();
    let old = format!(
        r#"{{"id":"cdp_old1","kind":"fact","scope":{{"kind":"everywhere"}},"text":"keeps tide tables in a blue binder","question":"q","options":["Keep","Edit","Not this"],"sources":[],"support":1,"created_ms":{now},"expires_ms":{},"state":"open","run":"crd_1"}}"#,
        now + 14 * DAY
    );
    let v: serde_json::Value = serde_json::from_str(&old).unwrap();
    r.core.store.put_meta("card.proposal.cdp_old1", &v).unwrap();
    let p = &state::proposals(&r.core.store).unwrap()[0];
    assert_eq!(p.ask, CardAsk::Add);
    let got = answer(&r, "cdp_old1", "keep", None).await;
    assert_eq!(got.proposal.state, ProposalState::Kept);
    assert_eq!(
        got.record.unwrap().text,
        "keeps tide tables in a blue binder"
    );
}

/// Off (`observe`, or no day line): no pass, no call, nothing moved.
#[tokio::test]
async fn off_means_no_pass_and_no_call() {
    for tweak in [
        (|c: &mut crate::config::Config| c.card.observe = false) as fn(&mut _),
        |c| c.card.day_usd = 0.0,
    ] {
        let r = rig(vec![statement()], tweak);
        metres(&r, 3);
        put(
            &r,
            &fact(
                "cob_f1",
                "ses_a",
                "keeps the survey boat at the north jetty",
            ),
        );
        let (_, made) = live(&r).await;
        assert!(made.is_empty());
        assert!(r.fake.requests().is_empty());
        assert_eq!(
            state::observations(&r.core.store, Some(ObservationState::Open))
                .unwrap()
                .len(),
            4
        );
    }
}

/// A dry run proposes nothing in either pass.
#[tokio::test]
async fn a_dry_run_writes_no_live_question() {
    let r = rig(vec![], |_| {});
    put(
        &r,
        &fact(
            "cob_f1",
            "ses_a",
            "keeps the survey boat at the north jetty",
        ),
    );
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
    assert!(got.proposed.is_empty());
    assert_eq!(r.core.store.stats().unwrap().last_position, before);
}

/// `card.propose` runs the archive's pass and then the live one, and the
/// archive's questions are what they were without any observation.
#[tokio::test]
async fn propose_runs_both_and_the_archive_is_unmoved() {
    let plain = rig(vec![two()], |_| {});
    archive(&plain).await;
    let base = propose(&plain).await;

    let r = rig(vec![two()], |_| {});
    archive(&r).await;
    put(
        &r,
        &fact(
            "cob_f1",
            "ses_a",
            "keeps the survey boat at the north jetty",
        ),
    );
    let both = propose(&r).await;
    let key = |p: &CardProposal| (p.text.clone(), p.question.clone(), p.support, p.ask);
    let archived: Vec<_> = both
        .proposed
        .iter()
        .filter(|p| p.observations.is_empty())
        .map(key)
        .collect();
    assert_eq!(archived, base.proposed.iter().map(key).collect::<Vec<_>>());
    assert_eq!(both.proposed.len(), base.proposed.len() + 1);
    assert_eq!(
        (both.evidence, both.clusters, both.fresh, both.calls),
        (base.evidence, base.clusters, base.fresh, base.calls)
    );
}

/// The nightly tender runs the live pass with the card done (the archive's
/// stop), on tokio's paused clock.
#[tokio::test(start_paused = true)]
async fn the_nightly_tender_runs_the_live_pass_after_done() {
    let r = rig(vec![], |_| {});
    let st = State {
        done: true,
        ..State::default()
    };
    r.core.people_write(&[st.record().unwrap()]).await.unwrap();
    put(
        &r,
        &fact(
            "cob_f1",
            "ses_a",
            "keeps the survey boat at the north jetty",
        ),
    );
    r.core.card_after_serving();
    tokio::time::sleep(
        crate::learning::tender::AFTER_START + std::time::Duration::from_secs(3 * 3600),
    )
    .await;
    let all = state::proposals(&r.core.store).unwrap();
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].observations, ["cob_f1"]);
    // The archive's mark was not touched.
    assert_eq!(State::read(&r.core.store).unwrap().last_run_ms, None);
}
