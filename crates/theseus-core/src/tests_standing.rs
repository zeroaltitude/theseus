//! The owner's standing records in every prompt of their scope (theseus-a7vi),
//! through the whole core: a compile carries the live records of its scope
//! and none of another's; a revoked one is gone at the next turn; a shared
//! place gets the orders' text and only a line for the rest; a compile that
//! drops one is refused naming it; the block survives compactions and the
//! prefix stays byte-identical while the records do not change; the writes
//! are the owner's, from a private place; and a plain turn writes no frame
//! of its own for them.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use theseus_protocol::{
    DiscordOrigin, SessionKind, StandingAddParams, StandingKind, StandingListParams,
    StandingRevokeParams, StandingScope,
};

use crate::approval::{Answerer, Surface};
use crate::bus::EventSink;
use crate::compiler::Compilation;
use crate::places::BoundPlace;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const OWNER: u64 = 271_828_182_845_904_523;
const ALICE: u64 = 222_222_222_222_222_222;
const LAB: u64 = 314_159_265_358_979_323;

/// A model that answers every request "Hello." and keeps each request.
#[derive(Default)]
struct Model {
    requests: Mutex<Vec<ProviderRequest>>,
}

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(req.clone());
            let f = FakeProvider::scripted(vec![Scripted::text("Hello.")]);
            f.stream_message(req, on_delta).await
        })
    }
}

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    _dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = path(dir.path());
    cfg.tools.projects_dir = Some(path(&root));
    cfg.tools.roots = vec![];
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model::default());
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    core.bind_places(vec![
        BoundPlace {
            target: format!("discord:dm:{OWNER}"),
            name: "DM @wren".into(),
            private: false,
            ..Default::default()
        },
        BoundPlace {
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: false,
            ..Default::default()
        },
    ]);
    Rig {
        core,
        model,
        _dir: dir,
    }
}

fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn session(core: &Core, place: Option<&str>) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        core.outbox.bind_place(p, &r.session_id).unwrap();
    }
    r.session_id
}

async fn try_turn(core: &Arc<Core>, sid: &str, input: &str) -> anyhow::Result<()> {
    let rec = core.store.get_session::<SessionRecord>(sid)?.unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None)?;
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: format!("discord:{OWNER}"),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .map(|_| ())
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) {
    try_turn(core, sid, input).await.unwrap();
}

fn system_text(req: &ProviderRequest) -> String {
    req.system
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn on_discord(user: u64, channel: Option<u64>) -> Answerer {
    Answerer {
        label: format!("discord:{user}"),
        surface: Surface::Discord,
        discord: Some(DiscordOrigin {
            user_id: user.to_string(),
            channel_id: channel.unwrap_or(1).to_string(),
            guild_id: channel.map(|_| "27182818".to_string()),
        }),
    }
}

fn rows_of(core: &Core, kind: &str) -> Vec<Value> {
    rows_in(core, kind, None)
}

fn rows_in(core: &Core, kind: &str, session: Option<&str>) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.is_kind(kind))
        .filter(|(_, r)| session.is_none() || r.session_id.as_deref() == session)
        .map(|(_, r)| r.data)
        .collect()
}

impl Rig {
    fn last_request(&self) -> ProviderRequest {
        self.model.requests.lock().unwrap().last().unwrap().clone()
    }

    fn compilation(&self, sid: &str) -> Compilation {
        let rec = self
            .core
            .store
            .get_session::<SessionRecord>(sid)
            .unwrap()
            .unwrap();
        self.core
            .store
            .get_compilation(rec.compilation_id.as_deref().unwrap())
            .unwrap()
            .unwrap()
    }

    fn triggers(&self, sid: &str) -> Vec<String> {
        rows_in(&self.core, "context.recompiled", Some(sid))
            .iter()
            .map(|r| r["trigger"].as_str().unwrap().to_string())
            .collect()
    }

    /// The CLI adds a record; its id.
    fn add(&self, kind: StandingKind, scope: StandingScope, text: &str) -> String {
        self.core
            .standing_add(
                &StandingAddParams {
                    kind,
                    text: text.into(),
                    scope,
                    ..Default::default()
                },
                "the CLI",
            )
            .unwrap()
            .record
            .id
    }

    fn revoke(&self, id: &str) {
        self.core
            .standing_revoke(
                &StandingRevokeParams {
                    id: id.into(),
                    ..Default::default()
                },
                "the CLI",
            )
            .unwrap();
    }
}

const NO_PUSH: &str = "never push to main";
const SHORT: &str = "answer in two sentences";
const GARDEN: &str = "keeps a vegetable garden";

/// A compile carries every live record of its scope and none of another's:
/// the order everywhere, the preference of this session, the fact of this
/// place; not another session's, not another place's. The manifest lists
/// each by id.
#[tokio::test]
async fn a_compile_carries_every_live_record_of_its_scope_and_none_of_anothers() {
    let r = rig();
    let a = session(&r.core, Some(&format!("dm:{OWNER}")));
    let b = session(&r.core, None);
    let everywhere = r.add(StandingKind::Order, StandingScope::Everywhere, NO_PUSH);
    let mine = r.add(
        StandingKind::Preference,
        StandingScope::Session(a.clone()),
        SHORT,
    );
    let here = r.add(
        StandingKind::Fact,
        StandingScope::Place(format!("discord:dm:{OWNER}")),
        GARDEN,
    );
    let theirs = r.add(
        StandingKind::Preference,
        StandingScope::Session(b.clone()),
        "only for the other session",
    );
    let elsewhere = r.add(
        StandingKind::Fact,
        StandingScope::Place(format!("discord:channel:{LAB}")),
        "only for the lab",
    );

    turn(&r.core, &a, "hi").await;
    let system = system_text(&r.last_request());
    for (id, text) in [(&everywhere, NO_PUSH), (&mine, SHORT), (&here, GARDEN)] {
        assert!(system.contains(&format!("- {id}: {text}")), "{system}");
    }
    for text in ["only for the other session", "only for the lab"] {
        assert!(!system.contains(text), "{system}");
    }
    let listed: Vec<String> = r
        .compilation(&a)
        .manifest
        .standing
        .iter()
        .map(|u| u.id.clone())
        .collect();
    let mut want = vec![everywhere.clone(), mine, here];
    want.sort();
    let mut got = listed;
    got.sort();
    assert_eq!(got, want);

    // The other session sees the order and its own preference alone.
    turn(&r.core, &b, "hi").await;
    let system = system_text(&r.last_request());
    assert!(
        system.contains(&format!("- {everywhere}: {NO_PUSH}")),
        "{system}"
    );
    assert!(
        system.contains(&format!("- {theirs}: only for the other session")),
        "{system}"
    );
    assert!(
        !system.contains(SHORT) && !system.contains(GARDEN),
        "{system}"
    );
    assert!(!system.contains(&elsewhere), "{system}");
}

/// A revoked record is gone at the session's next turn, which is one
/// `system_changed` recompile; with no change to the records the system and
/// the tools are byte-identical across turns, so the cache holds.
#[tokio::test]
async fn a_revoked_record_is_gone_and_an_unchanged_set_keeps_the_prefix_byte_identical() {
    let r = rig();
    let sid = session(&r.core, None);
    let keep = r.add(StandingKind::Order, StandingScope::Everywhere, NO_PUSH);
    let drop = r.add(StandingKind::Preference, StandingScope::Everywhere, SHORT);
    turn(&r.core, &sid, "one").await;
    let first = r.last_request();
    turn(&r.core, &sid, "two").await;
    let second = r.last_request();
    assert_eq!(
        serde_json::to_string(&(&first.system, &first.tools)).unwrap(),
        serde_json::to_string(&(&second.system, &second.tools)).unwrap(),
        "the prefix is byte-identical with no change to the records"
    );
    assert_eq!(r.triggers(&sid), ["new_session"]);

    r.revoke(&drop);
    turn(&r.core, &sid, "three").await;
    let system = system_text(&r.last_request());
    assert!(
        system.contains(NO_PUSH) && !system.contains(SHORT),
        "{system}"
    );
    assert_eq!(r.triggers(&sid), ["new_session", "system_changed"]);
    let m = r.compilation(&sid).manifest;
    assert_eq!(
        m.standing.iter().map(|u| u.id.as_str()).collect::<Vec<_>>(),
        [keep.as_str()]
    );

    // A replacement ends the old record and takes its place.
    let new = r
        .core
        .standing_add(
            &StandingAddParams {
                text: "never push to main or to release".into(),
                replaces: Some(keep.clone()),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert_eq!(new.ended.as_deref(), Some(keep.as_str()));
    turn(&r.core, &sid, "four").await;
    let system = system_text(&r.last_request());
    assert!(
        system.contains("never push to main or to release"),
        "{system}"
    );
    assert!(!system.contains(&keep), "{system}");
    let listed = r
        .core
        .standing_list(
            &StandingListParams {
                all: true,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    let states: Vec<(&str, &str)> = listed
        .records
        .iter()
        .map(|i| (i.record.id.as_str(), i.state.as_str()))
        .collect();
    assert!(states.contains(&(keep.as_str(), "replaced")), "{states:?}");
    assert!(states.contains(&(drop.as_str(), "revoked")), "{states:?}");
}

/// A shared place gets the orders of its scope with their text, and for the
/// preferences and facts only the line that says they are withheld: never
/// their text, in the system and in the manifest's note of what was shown.
#[tokio::test]
async fn a_shared_place_shows_orders_and_hides_the_text_of_preferences_and_facts() {
    let r = rig();
    let order = r.add(StandingKind::Order, StandingScope::Everywhere, NO_PUSH);
    let pref = r.add(StandingKind::Preference, StandingScope::Everywhere, SHORT);
    let fact = r.add(StandingKind::Fact, StandingScope::Everywhere, GARDEN);
    let shared = session(&r.core, Some(&format!("channel:{LAB}")));
    turn(&r.core, &shared, "hi").await;
    let system = system_text(&r.last_request());
    assert!(
        system.contains(&format!("- {order}: {NO_PUSH}")),
        "{system}"
    );
    assert!(system.contains(crate::standing::WITHHELD), "{system}");
    for text in [SHORT, GARDEN, pref.as_str(), fact.as_str()] {
        assert!(!system.contains(text), "{text} leaked: {system}");
    }
    let m = r.compilation(&shared).manifest;
    let shown: Vec<(&str, bool)> = m
        .standing
        .iter()
        .map(|u| (u.id.as_str(), u.shown))
        .collect();
    assert!(shown.contains(&(order.as_str(), true)), "{shown:?}");
    assert!(
        shown.contains(&(pref.as_str(), false)) && shown.contains(&(fact.as_str(), false)),
        "{shown:?}"
    );

    // The owner's DM carries all three.
    let dm = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &dm, "hi").await;
    let system = system_text(&r.last_request());
    assert!(
        system.contains(SHORT) && system.contains(GARDEN),
        "{system}"
    );
    assert!(!system.contains(crate::standing::WITHHELD), "{system}");
}

/// The list gives a preference's or a fact's text to the owner in a private
/// place alone: anyone else, or the owner in a shared place, reads
/// `(private)`.
#[tokio::test]
async fn the_list_gives_private_text_to_the_owner_in_a_private_place_alone() {
    let r = rig();
    r.add(StandingKind::Order, StandingScope::Everywhere, NO_PUSH);
    r.add(StandingKind::Fact, StandingScope::Everywhere, GARDEN);
    let texts = |who: Answerer| -> Vec<String> {
        r.core
            .standing_list(&StandingListParams::default(), who)
            .unwrap()
            .records
            .into_iter()
            .map(|i| i.record.text)
            .collect()
    };
    assert_eq!(texts(on_discord(OWNER, None)), [NO_PUSH, GARDEN]);
    for who in [on_discord(ALICE, None), on_discord(OWNER, Some(LAB))] {
        assert_eq!(texts(who), [NO_PUSH, "(private)"]);
    }
}

/// The writes are the owner's, from a private place: the CLI and the owner's
/// DM count; someone else's, a shared place's, and an unnamed connection's
/// are refused, ledgered, and write nothing. A record's text is capped, an
/// identical live one is not added twice, and a session scope names a session.
#[tokio::test]
async fn a_write_counts_only_from_the_owner_in_a_private_place() {
    let r = rig();
    let add = |who: Answerer| {
        r.core.standing_add(
            &StandingAddParams {
                text: NO_PUSH.into(),
                ..Default::default()
            },
            who,
        )
    };
    let unnamed = Answerer {
        label: "sock#9".into(),
        surface: Surface::Unnamed,
        discord: None,
    };
    for who in [
        unnamed,
        on_discord(ALICE, None),
        on_discord(OWNER, Some(LAB)),
    ] {
        let e = add(who).unwrap_err();
        assert!(
            format!("{e:?}").contains("does not count")
                || format!("{e:?}").contains("shared")
                || format!("{e:?}").contains("owner"),
            "{e:?}"
        );
    }
    assert!(r
        .core
        .runner
        .standing
        .all(&r.core.store)
        .unwrap()
        .is_empty());
    let refused = rows_of(&r.core, "approval.refused");
    assert_eq!(refused.len(), 3);
    assert!(
        refused.iter().all(|row| row["act"] == "standing.add"),
        "{refused:?}"
    );

    let ok = add(on_discord(OWNER, None)).unwrap();
    let again = add(on_discord(OWNER, None)).unwrap();
    assert_eq!(
        ok.record.id, again.record.id,
        "an identical live record is not added twice"
    );
    assert_eq!(rows_of(&r.core, "standing.added").len(), 1);

    let long = "x".repeat(crate::standing::MAX_TEXT + 1);
    let e = r
        .core
        .standing_add(
            &StandingAddParams {
                text: long,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap_err();
    assert!(e.message.contains("at most"), "{}", e.message);
    let e = r
        .core
        .standing_add(
            &StandingAddParams {
                text: "x".into(),
                scope: StandingScope::Session("ses_nope".into()),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap_err();
    assert!(e.message.contains("no session"), "{}", e.message);

    // A revoke is judged the same way.
    let revoke = |who: Answerer| {
        r.core.standing_revoke(
            &StandingRevokeParams {
                id: ok.record.id.clone(),
                ..Default::default()
            },
            who,
        )
    };
    assert!(revoke(on_discord(ALICE, None)).is_err());
    assert!(
        r.core
            .standing_list(&StandingListParams::default(), "the CLI")
            .unwrap()
            .records
            .len()
            == 1
    );
    revoke(on_discord(OWNER, None)).unwrap();
    assert!(r
        .core
        .standing_list(&StandingListParams::default(), "the CLI")
        .unwrap()
        .records
        .is_empty());
    assert_eq!(rows_of(&r.core, "standing.revoked").len(), 1);
}

/// A compile that drops a live record of its scope is refused before anything
/// is sent, naming it: `standing <id> missing`, a `context.unadmitted` row.
#[tokio::test]
async fn a_compile_that_drops_a_record_is_refused_naming_it() {
    let r = rig();
    let sid = session(&r.core, None);
    let id = r.add(StandingKind::Order, StandingScope::Everywhere, NO_PUSH);
    turn(&r.core, &sid, "fine").await;
    let sent = r.model.requests.lock().unwrap().len();

    crate::standing::DROP_THE_BLOCK.store(true, std::sync::atomic::Ordering::SeqCst);
    r.core
        .request_recompile(&sid, crate::compiler::Recompile::Transcript, "the CLI")
        .unwrap();
    let e = try_turn(&r.core, &sid, "dropped").await;
    crate::standing::DROP_THE_BLOCK.store(false, std::sync::atomic::Ordering::SeqCst);
    let e = format!("{:?}", e.unwrap_err());
    assert!(e.contains(&id) && e.contains("context_unadmitted"), "{e}");
    assert_eq!(
        r.model.requests.lock().unwrap().len(),
        sent,
        "nothing was sent"
    );
    let rows = rows_of(&r.core, "context.unadmitted");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["why"], "standing_missing");
    assert_eq!(rows[0]["piece"], format!("standing {id} missing"));
}

/// The list tallies the manifests: every compilation of the records' scope
/// since they began listed them, none lacked one.
#[tokio::test]
async fn the_list_tallies_the_manifests_of_every_compile_of_their_scope() {
    let r = rig();
    let sid = session(&r.core, None);
    let id = r.add(StandingKind::Order, StandingScope::Everywhere, NO_PUSH);
    turn(&r.core, &sid, "one").await;
    r.add(StandingKind::Fact, StandingScope::Everywhere, GARDEN);
    turn(&r.core, &sid, "two").await;
    let l = r
        .core
        .standing_list(&StandingListParams::default(), "the CLI")
        .unwrap();
    assert_eq!((l.compiles, l.missing), (2, 0), "{l:?}");
    let order = l.records.iter().find(|i| i.record.id == id).unwrap();
    assert_eq!((order.compiles, order.missing), (2, 0));
    // A session that took no turn since the record began adds nothing.
    let _idle = session(&r.core, None);
    let l = r
        .core
        .standing_list(&StandingListParams::default(), "the CLI")
        .unwrap();
    assert_eq!(l.compiles, 2);
}

/// A plain turn writes the frames it wrote before: the records are read
/// from memory, and the block rides the compile.
#[tokio::test]
async fn a_plain_turn_with_records_writes_no_frame_of_its_own_for_them() {
    let r = rig();
    let sid = session(&r.core, None);
    r.add(StandingKind::Order, StandingScope::Everywhere, NO_PUSH);
    r.add(StandingKind::Preference, StandingScope::Everywhere, SHORT);
    turn(&r.core, &sid, "warm up").await;
    let before = r.core.store.stats().unwrap().frames_appended;
    turn(&r.core, &sid, "hi").await;
    let frames = r.core.store.stats().unwrap().frames_appended - before;
    assert!(frames <= 5, "a plain turn wrote {frames} frames");
}

/// The records a restart reads, by one META prefix scan, are the ones kept in
/// memory, in the same order.
#[tokio::test]
async fn the_records_rebuilt_after_a_restart_equal_the_ones_kept_in_memory() {
    let r = rig();
    let a = r.add(StandingKind::Order, StandingScope::Everywhere, NO_PUSH);
    r.add(StandingKind::Fact, StandingScope::Everywhere, GARDEN);
    r.revoke(&a);
    let held = r.core.runner.standing.all(&r.core.store).unwrap();
    let fresh = crate::standing::Board::default();
    assert_eq!(*fresh.all(&r.core.store).unwrap(), *held);
    assert_eq!(held.len(), 3, "two records and one revoke");
}

/// A place scope names a bound place's target: the CLI and the web UI have
/// none, so a scope of `cli` is refused with why rather than holding nowhere.
#[tokio::test]
async fn a_place_scope_names_a_bound_places_target() {
    let r = rig();
    for place in ["cli", "web", ""] {
        let e = r
            .core
            .standing_add(
                &StandingAddParams {
                    text: NO_PUSH.into(),
                    scope: StandingScope::Place(place.into()),
                    ..Default::default()
                },
                "the CLI",
            )
            .unwrap_err();
        assert!(
            e.message.contains("discord:dm:<user id>"),
            "{place}: {}",
            e.message
        );
    }
    assert!(r
        .core
        .runner
        .standing
        .all(&r.core.store)
        .unwrap()
        .is_empty());
}

mod surface;
