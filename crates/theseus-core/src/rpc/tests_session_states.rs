//! Session states, supersession, retire and reopen (theseus-emqx), through
//! the core: what `session.list` derives, the one frame that moves a place,
//! and the owner's acts.

use std::sync::Arc;

use theseus_protocol::sessions::{RetiredReason, SessionRetired};
use theseus_protocol::{SessionListParams, SessionOpenParams, SessionState};

use super::tests::test_core;
use super::{Core, Parts};
use crate::approval::{Answerer, Refusal};
use crate::ledger::LedgerRow;
use crate::provider::FakeProvider;
use crate::session::SessionRecord;

const H: u64 = 3_600_000;
const PLACE: &str = "dm:4242";

fn core() -> Arc<Core> {
    test_core("the tide is out")
}

/// A second core over `c`'s store, as a restarted daemon's.
fn restarted(c: &Core) -> Arc<Core> {
    let model = Arc::new(FakeProvider::default());
    Core::build(Parts::for_tests((*c.cfg).clone(), model, c.store.clone())).unwrap()
}

fn open(core: &Core) -> String {
    core.open_session(SessionOpenParams::default())
        .unwrap()
        .session_id
}

fn rec(core: &Core, id: &str) -> SessionRecord {
    core.store.get_session(id).unwrap().unwrap()
}

/// Write `id`'s record with `f` applied, as an older turn would have left it.
fn edit(core: &Core, id: &str, f: impl FnOnce(&mut SessionRecord)) {
    let mut r = rec(core, id);
    f(&mut r);
    core.store.put_session(id, &r).unwrap();
}

fn list(core: &Core, p: SessionListParams) -> theseus_protocol::SessionListResult {
    core.session_list_of(p).unwrap()
}

fn state_of(core: &Core, id: &str) -> (Option<SessionState>, Option<SessionRetired>) {
    let r = list(
        core,
        SessionListParams {
            ids: Some(vec![id.to_string()]),
            ..Default::default()
        },
    );
    let s = &r.sessions[0];
    (s.state, s.retired.clone())
}

fn rows(core: &Core, kind: &str) -> Vec<LedgerRow> {
    core.store
        .ledger_tail::<LedgerRow>(500)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .collect()
}

fn stranger() -> Answerer {
    Answerer {
        label: "discord".into(),
        surface: crate::approval::Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "77".into(),
            channel_id: "1001".into(),
            guild_id: Some("7".into()),
        }),
    }
}

/// The window's edges, the empty grace, and a retirement, as `session.list`
/// derives them, with the windows it names.
#[test]
fn the_list_derives_each_state_at_the_windows_edges() {
    let c = core();
    let now = theseus_protocol::now_unix_ms();
    let (inside, outside, fresh, empty) = (open(&c), open(&c), open(&c), open(&c));
    edit(&c, &inside, |r| {
        (r.turns, r.created_at_unix_ms, r.last_active_ms) =
            (2, now - 30 * H, now - 24 * H + 60_000);
    });
    edit(&c, &outside, |r| {
        (r.turns, r.created_at_unix_ms, r.last_active_ms) =
            (2, now - 30 * H, now - 24 * H - 60_000);
    });
    edit(&c, &empty, |r| {
        (r.created_at_unix_ms, r.last_active_ms) = (now - H - 60_000, now - H - 60_000);
    });
    assert_eq!(state_of(&c, &inside).0, Some(SessionState::Live));
    assert_eq!(state_of(&c, &outside).0, Some(SessionState::Quiet));
    assert_eq!(
        state_of(&c, &fresh),
        (Some(SessionState::Live), None),
        "within its grace"
    );
    let (state, why) = state_of(&c, &empty);
    assert_eq!(state, Some(SessionState::Retired));
    assert_eq!(why.unwrap().reason, RetiredReason::Empty);
    let all = list(&c, SessionListParams::default());
    assert_eq!(all.live_window_ms, Some(24 * H));
    assert_eq!(all.empty_grace_ms, Some(H));
    let quiet = list(
        &c,
        SessionListParams {
            state: Some(SessionState::Quiet),
            ..Default::default()
        },
    );
    let ids: Vec<&str> = quiet
        .sessions
        .iter()
        .map(|s| s.session_id.as_str())
        .collect();
    assert_eq!(ids, [outside.as_str()]);
}

/// A session that waits on the owner reads live whatever its age: its
/// question keeps it in the default view.
#[test]
fn a_session_waiting_on_the_owner_reads_live_whatever_its_age() {
    let c = core();
    let now = theseus_protocol::now_unix_ms();
    let old = open(&c);
    edit(&c, &old, |r| {
        (r.turns, r.created_at_unix_ms, r.last_active_ms) = (3, now - 90 * H, now - 80 * H);
    });
    assert_eq!(state_of(&c, &old).0, Some(SessionState::Quiet));
    let r = rec(&c, &old);
    let ask = theseus_protocol::PendingConfirm {
        correlation_id: "act_tide".into(),
        tool: "proc.run".into(),
        reason: "run the tide chart".into(),
        floor: false,
        budget: false,
        expires_at_ms: 0,
    };
    let pending = [(r.execution_id.clone().unwrap(), vec![ask])].into();
    let shown = c.session_info(&r, &pending);
    assert_eq!(shown.pending_confirms, 1);
    assert_eq!(shown.state, Some(SessionState::Live));
}

/// `session.list` with no parameters answers every session, in the order it
/// always did, retired and quiet ones included: the CLI, the TUI and the
/// Discord binding see what they saw.
#[test]
fn the_list_with_no_parameters_is_every_session_as_before() {
    let c = core();
    let now = theseus_protocol::now_unix_ms();
    let ids: Vec<String> = (0..6).map(|_| open(&c)).collect();
    for (i, id) in ids.iter().enumerate() {
        edit(&c, id, |r| {
            r.turns = 1;
            r.created_at_unix_ms = now - 200 * H;
            r.last_active_ms = now - (i as u64) * 20 * H;
        });
    }
    c.retire_session(&ids[2], &Answerer::from("cli")).unwrap();
    let all = list(&c, SessionListParams::default());
    let got: Vec<&str> = all.sessions.iter().map(|s| s.session_id.as_str()).collect();
    let want: Vec<&str> = ids.iter().map(String::as_str).collect();
    assert_eq!(got, want, "every session, the most recently active first");
    let states: Vec<_> = all.sessions.iter().map(|s| s.state.unwrap()).collect();
    use SessionState::*;
    assert_eq!(states, [Live, Live, Retired, Quiet, Quiet, Quiet]);
}

/// A place's move is one frame: the place's record, both sessions' links,
/// the old one's retirement and the row. After a restart the records, the
/// place's record, the row and the outbox's targets agree.
#[test]
fn a_supersession_is_one_frame_and_reads_the_same_after_a_restart() {
    let c = core();
    let (old, new) = (open(&c), open(&c));
    c.outbox.bind_place(PLACE, &old).unwrap();
    let f0 = theseus_store::frames_written_here();
    let was = c.bind_place_to(PLACE, &new).unwrap();
    assert_eq!(theseus_store::frames_written_here() - f0, 1, "one frame");
    assert_eq!(was.as_deref(), Some(old.as_str()));
    let check = |c: &Core| {
        let (o, n) = (rec(c, &old), rec(c, &new));
        let by = o
            .superseded_by
            .as_ref()
            .expect("the old names its successor");
        assert_eq!(
            (by.session_id.as_str(), by.place.as_deref()),
            (new.as_str(), Some(PLACE))
        );
        assert_eq!(
            o.retired.as_ref().unwrap().reason,
            RetiredReason::Superseded
        );
        assert_eq!(n.supersedes.as_ref().unwrap().session_id, old);
        assert_eq!(by.at_ms, n.supersedes.as_ref().unwrap().at_ms);
        assert_eq!(
            c.outbox.place_session(PLACE).unwrap().as_deref(),
            Some(new.as_str())
        );
        assert_eq!(c.outbox.target(&new).as_deref(), Some("discord:dm:4242"));
        assert_eq!(
            c.outbox.target(&old),
            None,
            "the old posts nothing more there"
        );
        let r = rows(c, "session.superseded");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].session_id.as_deref(), Some(old.as_str()));
        assert_eq!(r[0].data["superseded_by"], new.as_str());
        assert_eq!(state_of(c, &old).0, Some(SessionState::Retired));
    };
    check(&c);
    let again = restarted(&c);
    check(&again);
}

/// A first bind writes the place's record alone, and a move onto the same
/// session nothing more.
#[test]
fn a_first_bind_supersedes_nothing() {
    let c = core();
    let new = open(&c);
    assert_eq!(c.bind_place_to(PLACE, &new).unwrap(), None);
    assert_eq!(c.bind_place_to(PLACE, &new).unwrap(), None);
    assert!(rec(&c, &new).supersedes.is_none());
    assert!(rows(&c, "session.superseded").is_empty());
}

/// The owner retires and reopens; anyone else is refused and nothing is
/// written. A reopen keeps both links as history.
#[test]
fn the_owner_retires_and_reopens_and_no_one_else_can() {
    let c = core();
    let (old, new) = (open(&c), open(&c));
    c.outbox.bind_place(PLACE, &old).unwrap();
    c.bind_place_to(PLACE, &new).unwrap();
    for e in [
        c.retire_session(&new, &stranger()).unwrap_err(),
        c.reopen_session(&old, &stranger()).unwrap_err(),
    ] {
        assert!(e.downcast_ref::<Refusal>().is_some(), "{e:#}");
    }
    assert!(rec(&c, &new).retired.is_none(), "a refusal writes nothing");
    assert_eq!(rows(&c, "approval.refused").len(), 2);
    let owner = Answerer::from("cli");
    let r = c.retire_session(&new, &owner).unwrap();
    assert_eq!(r.retired.as_ref().unwrap().reason, RetiredReason::ByHand);
    assert_eq!(state_of(&c, &new).0, Some(SessionState::Retired));
    assert!(c.session_retired(&new));
    let r = c.reopen_session(&old, &owner).unwrap();
    assert!(r.retired.is_none());
    assert_eq!(
        r.superseded_by.as_ref().unwrap().session_id,
        new,
        "the history stays"
    );
    assert_ne!(state_of(&c, &old).0, Some(SessionState::Retired));
    assert_eq!(
        c.outbox.place_session(PLACE).unwrap().as_deref(),
        Some(new.as_str()),
        "the place stays on its successor"
    );
    assert_eq!(rows(&c, "session.retired").len(), 1);
    let reopened = rows(&c, "session.reopened");
    assert_eq!(reopened[0].data["was"], "superseded");
    // Nothing is deleted.
    assert!(c
        .store
        .get_session::<SessionRecord>(&new)
        .unwrap()
        .is_some());
}

/// A session opened and never used reads retired (empty) after its grace,
/// and a reopen brings it back for good.
#[test]
fn an_empty_session_reopened_stays_open() {
    let c = core();
    let now = theseus_protocol::now_unix_ms();
    let id = open(&c);
    edit(&c, &id, |r| {
        (r.created_at_unix_ms, r.last_active_ms) = (now - 5 * H, now - 5 * H);
    });
    assert_eq!(state_of(&c, &id).0, Some(SessionState::Retired));
    c.reopen_session(&id, &Answerer::from("cli")).unwrap();
    assert_eq!(state_of(&c, &id), (Some(SessionState::Live), None));
}

/// A record written at format 24 reads as before, with no state of its
/// own, and its derived state is right: a bind's empty session is live
/// within its grace and retired (empty) past it; once used, it is live,
/// then quiet.
#[test]
fn a_record_from_before_the_states_derives_them() {
    let r: SessionRecord =
        serde_json::from_str(crate::tests_layouts::SESSION_BEFORE_STATES).unwrap();
    assert!(r.retired.is_none() && r.superseded_by.is_none() && r.supersedes.is_none());
    assert!(r.reopened_ms.is_none() && r.title_was.is_empty());
    let rule = theseus_protocol::sessions::StateRule::default();
    let born = r.created_at_unix_ms;
    assert_eq!(r.state_at(rule, born + H - 1, false).0, SessionState::Live);
    let (state, why) = r.state_at(rule, born + H, false);
    assert_eq!(
        (state, why.unwrap().reason),
        (SessionState::Retired, RetiredReason::Empty)
    );
    let used = SessionRecord { turns: 1, ..r };
    assert_eq!(
        used.state_at(rule, born + 24 * H - 1, false).0,
        SessionState::Live
    );
    assert_eq!(
        used.state_at(rule, born + 24 * H, false).0,
        SessionState::Quiet
    );
}

/// `session.list` over 2,000 sessions of the owner's, a third quiet and a
/// third retired by hand, through the method (each state derived): the
/// whole list and a page of 20, each timed 20 times, with the index rows
/// and records each visits. A measure, not a check: run it with
/// `--ignored --nocapture`. The state is derived from the record each list
/// already reads, so the records a list visits are the sessions it lists.
#[test]
#[ignore = "a measure: 2,000 sessions, printed"]
fn the_session_list_with_states_timed() {
    use theseus_store::{index_rows_here, records_read_here};
    let c = core();
    let now = theseus_protocol::now_unix_ms();
    for i in 0..2_000u64 {
        let id = open(&c);
        edit(&c, &id, |r| {
            r.turns = 1 + i % 5;
            r.last_active_ms = now - (i % 3) * 30 * H;
            if i % 3 == 2 {
                r.retired = Some(SessionRetired {
                    reason: RetiredReason::ByHand,
                    at_ms: now,
                });
            }
        });
    }
    let time = |what: &str, f: &dyn Fn() -> usize| {
        f();
        let (rows, read) = (index_rows_here(), records_read_here());
        let n = f();
        let (rows, read) = (index_rows_here() - rows, records_read_here() - read);
        let mut ms: Vec<f64> = (0..20)
            .map(|_| {
                let t = std::time::Instant::now();
                f();
                t.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        ms.sort_by(f64::total_cmp);
        println!(
            "{what}: {n} sessions, p50 {:.2} ms, max {:.2} ms, {rows} index rows, {read} records",
            ms[10], ms[19]
        );
    };
    time("whole list", &|| {
        list(&c, SessionListParams::default()).sessions.len()
    });
    time("page of 20", &|| {
        let p = SessionListParams {
            n: Some(20),
            ..Default::default()
        };
        list(&c, p).sessions.len()
    });
}
