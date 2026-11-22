//! `exchange.adopt` (issue 44): old turns of a DM-bound session and of a
//! session bound elsewhere, rebuilt as `unknown`; the DM adopted, so its
//! window, its ladder's dry run and the history tool hold them, author
//! unknown, and the other session's stay as they were; the undo; a sealed
//! session; `/new`; a session tied to two places; turns the rebuild has not
//! reached; and the refusal of anyone but the owner in person.

use super::*;
use crate::exchange::adopt::{self as adoption, Asker};
use crate::history_tool::page::Lens;
use crate::view::history::HistoryArgs;
use crate::view::Present;
use theseus_protocol::exchange::{ExchangeAdoptParams, ExchangeAdoptResult};

const DM: &str = "discord:dm:42";
const LOBBY: &str = "discord:channel:7";

/// Old turns, the ledger off: two in a session bound to the DM, one in a
/// session bound to the lobby. Their exchanges rebuilt, all `unknown`.
struct Old {
    r: Rig,
    dm: String,
    lobby: String,
    dm_turns: Vec<String>,
    lobby_turn: String,
}

async fn old() -> Old {
    let r = rig(3, |c| c.context.ledger = Some(false));
    let c = &r.core;
    let mut sids = Vec::new();
    for place in ["dm:42", "channel:7"] {
        let s =
            crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        c.store.put_session(&s.session_id, &s).unwrap();
        c.outbox.bind_place(place, &s.session_id).unwrap();
        sids.push(s.session_id);
    }
    let a = turn(c, owner_cli(), Some(&sids[0]), "an old direct message").await;
    let b = turn(c, owner_cli(), Some(&sids[0]), "another old one").await;
    let l = turn(c, owner_cli(), Some(&sids[1]), "an old lobby message").await;
    assert_eq!(rebuilt(c, None, false).await.exchanges, 3);
    Old {
        dm: sids[0].clone(),
        lobby: sids[1].clone(),
        dm_turns: vec![a.turn_id, b.turn_id],
        lobby_turn: l.turn_id,
        r,
    }
}

async fn adopt_rpc(c: &Arc<Core>, client: Client, p: Value) -> Response {
    call(c, client, "exchange.adopt", p).await
}

fn result(r: Response) -> ExchangeAdoptResult {
    assert!(r.error.is_none(), "{:?}", r.error);
    serde_json::from_value(r.result.unwrap()).unwrap()
}

fn place(c: &Core, turn: &str) -> String {
    exchange(&c.store, turn).unwrap().unwrap().place
}

fn window(c: &Core, place: &str) -> Vec<String> {
    let since = crate::ladder::new_mark(&c.store, place).unwrap();
    let l = crate::ladder::read::ladder(&c.store, place, since.as_ref()).unwrap();
    l.window.into_iter().map(|e| e.turn_id).collect()
}

fn clear(_: &str) -> bool {
    false
}

fn private(_: &str) -> bool {
    true
}

/// What the history tool pages at `here`, the owner alone.
fn history(c: &Core, here: &str) -> Vec<String> {
    let lens = Lens {
        store: &c.store,
        here: here.into(),
        present: Present::OwnerAlone,
        hidden: &clear,
        private: &private,
        in_view: None,
        words: None,
    };
    let a = HistoryArgs {
        place: Some(here.into()),
        ..HistoryArgs::default()
    };
    let page = lens.walk(&a, None, 0).unwrap().page;
    page.lines.into_iter().map(|l| l.handle.0).collect()
}

async fn ladder_dry(c: &Arc<Core>, place: &str) -> u64 {
    let money = crate::ladder::build::Allowance::default();
    c.ladder_build(place, true, money).await.unwrap().exchanges
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_owner_adopts_a_dms_old_history_and_takes_it_back() {
    let o = old().await;
    let c = &o.r.core;
    assert!(window(c, DM).is_empty() && history(c, DM).is_empty());
    let dry = result(adopt_rpc(c, owner_cli(), json!({"place": DM, "dry_run": true})).await);
    assert_eq!(
        (dry.sessions, dry.exchanges, dry.touches, dry.unledgered),
        (1, 2, 2, 0)
    );
    assert!(dry.id.is_none() && dry.left.is_empty(), "{dry:?}");
    assert_eq!(
        place(c, &o.dm_turns[0]),
        rebuild::UNKNOWN,
        "a dry run writes nothing"
    );

    let done = result(adopt_rpc(c, owner_cli(), json!({"place": DM})).await);
    let id = done.id.clone().expect("the adoption's id");
    assert_eq!((done.sessions, done.exchanges), (1, 2));
    for t in &o.dm_turns {
        let e = exchange(&c.store, t).unwrap().unwrap();
        assert_eq!(
            (e.place.as_str(), e.person.as_deref()),
            (DM, None),
            "a place, never a person"
        );
        assert!(
            mark(&c.store, e.ask.as_deref().unwrap()).unwrap().is_none(),
            "no mark is made"
        );
    }
    assert_eq!(
        place(c, &o.lobby_turn),
        rebuild::UNKNOWN,
        "the other session's are untouched"
    );
    assert_eq!(
        window(c, DM),
        o.dm_turns,
        "the window holds them, oldest first"
    );
    assert_eq!(
        ladder_dry(c, DM).await,
        2,
        "the ladder's dry run counts them"
    );
    let mut paged = history(c, DM);
    paged.reverse();
    assert_eq!(paged, o.dm_turns, "the history tool pages them");
    assert!(window(c, LOBBY).is_empty() && history(c, LOBBY).is_empty());
    assert!(newest(&c.store, Root::Person(OWNER), 8).unwrap().is_empty());

    let again = result(adopt_rpc(c, owner_cli(), json!({"place": DM, "dry_run": true})).await);
    assert_eq!(again.exchanges, 0);
    assert_eq!(again.left.get("exchanges_adopted_already"), Some(&2));

    let undone = result(adopt_rpc(c, owner_cli(), json!({"place": DM, "undo": id})).await);
    assert_eq!(
        (undone.undone, undone.exchanges, undone.touches),
        (true, 2, 2)
    );
    assert!(
        window(c, DM).is_empty() && history(c, DM).is_empty(),
        "back to empty"
    );
    assert_eq!(ladder_dry(c, DM).await, 0);
    assert_eq!(place(c, &o.dm_turns[0]), rebuild::UNKNOWN);
    let twice = adopt_rpc(c, owner_cli(), json!({"place": DM, "undo": id})).await;
    assert!(twice.error.is_some(), "one undo per adoption");
    let _ = &o.dm;
}

/// Adopted words are author unknown in their own place, and never the
/// owner's words to another place: the history tool from elsewhere, and the
/// past's "his words", leave them out.
#[tokio::test]
async fn adopted_words_are_author_unknown_and_never_his_elsewhere() {
    use crate::view::select::sources::Exchanges as _;
    use crate::view::text::labels::part_label;
    use crate::view::Label;
    let o = old().await;
    let c = &o.r.core;
    result(adopt_rpc(c, owner_cli(), json!({"place": DM})).await);
    let sessions = crate::view::sources_store::Sessions::default();
    let src = crate::view::sources_store::StoreSources {
        store: &c.store,
        mark: None,
        recalled: Vec::new(),
        sealed: &clear,
        sessions: &sessions,
    };
    let f = src.facts(&o.dm_turns[0]).unwrap().unwrap();
    assert_eq!((f.place.as_str(), f.person.as_deref()), (DM, None));
    assert_eq!(f.marks.author, crate::view::Author::Unknown);
    assert_eq!(
        part_label(f.marks.author, false, true, false),
        Label::AuthorUnknown
    );
    let ex = exchange(&c.store, &o.dm_turns[0]).unwrap().unwrap();
    let from = |here: &str| Lens {
        store: &c.store,
        here: here.into(),
        present: Present::OwnerAlone,
        hidden: &clear,
        private: &private,
        in_view: None,
        words: None,
    };
    assert!(from(DM).admits(&ex), "its own place reads it");
    assert!(!from("local").admits(&ex), "never his words elsewhere");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adopted_history_sits_before_new_and_the_history_tool_pages_past_it() {
    let o = old().await;
    let c = &o.r.core;
    result(adopt_rpc(c, owner_cli(), json!({"place": DM})).await);
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    c.ladder_mark_new(DM).unwrap();
    assert!(window(c, DM).is_empty(), "the window begins after /new");
    assert_eq!(ladder_dry(c, DM).await, 0, "so does the ladder");
    assert_eq!(
        history(c, DM).len(),
        2,
        "the history tool pages back past it"
    );
}

fn asker<'a>(sealed: &'a dyn Fn(&str) -> bool) -> Asker<'a> {
    Asker {
        surface: "cli",
        private: true,
        sealed,
        now_ms: 1,
    }
}

fn params(place: &str, dry_run: bool) -> ExchangeAdoptParams {
    ExchangeAdoptParams {
        place: place.into(),
        dry_run,
        undo: None,
    }
}

#[tokio::test]
async fn a_sealed_sessions_history_is_never_adopted() {
    let o = old().await;
    let c = &o.r.core;
    let dm = o.dm.clone();
    let sealed = move |s: &str| s == dm;
    let r = adoption::adopt(&c.store, &asker(&sealed), &params(DM, false)).unwrap();
    assert_eq!((r.sessions, r.exchanges, r.id), (0, 0, None));
    assert_eq!(r.left.get("sessions_sealed_or_unproven"), Some(&1));
    assert_eq!(place(c, &o.dm_turns[0]), rebuild::UNKNOWN);
}

#[tokio::test]
async fn a_session_tied_to_two_places_is_left_and_counted() {
    let o = old().await;
    let c = &o.r.core;
    c.outbox.bind_place("channel:9", &o.dm).unwrap();
    let r = adoption::adopt(&c.store, &asker(&clear), &params(DM, true)).unwrap();
    assert_eq!(r.exchanges, 0);
    assert_eq!(r.left.get("sessions_tied_to_another_place_too"), Some(&1));
}

#[tokio::test]
async fn turns_the_rebuild_has_not_reached_stop_the_adoption() {
    let r = rig(1, |c| c.context.ledger = Some(false));
    let c = &r.core;
    let s = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    c.store.put_session(&s.session_id, &s).unwrap();
    c.outbox.bind_place("dm:42", &s.session_id).unwrap();
    turn(c, owner_cli(), Some(&s.session_id), "not rebuilt").await;
    let dry = adoption::adopt(&c.store, &asker(&clear), &params(DM, true)).unwrap();
    assert_eq!((dry.unledgered, dry.exchanges), (1, 0));
}

#[tokio::test]
async fn only_the_owner_in_person_adopts() {
    let o = old().await;
    let c = &o.r.core;
    let agent = Client::new("sock#9", Surface::Cli).behind(Behind::Agent("claude".into()));
    let job =
        Client::new("sock#8", Surface::Cli).behind(Behind::Agent(crate::peer_proc::JOB.into()));
    let dc = Client::new("discord#1", Surface::Discord);
    for who in [agent, job, dc] {
        let r = adopt_rpc(c, who, json!({"place": DM})).await;
        assert!(r.error.is_some(), "refused");
    }
    assert_eq!(place(c, &o.dm_turns[0]), rebuild::UNKNOWN);
    let _ = &o.lobby;
}
