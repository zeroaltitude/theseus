//! The view record's readers through the whole core (issue 44): `view.show` answers the owner in person
//! alone, over real protocol connections; a seal proposed after a view was kept keeps its piece out of the
//! owner's inspection and of the model's own look; and the look reads this turn's presence alone.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::json;
use theseus_memory::gate::Present;
use theseus_protocol::threads::ThreadNewParams;
use theseus_protocol::{error_code, method};

use crate::approval::{Answerer, Client, Surface};
use crate::exchange::{self, Exchange, Opener};
use crate::peer_proc::{Behind, JOB};
use crate::tests_m3::{rpc_as, surface};
use crate::tests_places::{rig_scripted, session, Rig, LAB};
use crate::view::budget::{Shown, View};
use crate::view::record;
use crate::view::select::Band;
use crate::view::{
    Anchor, Audience, Author, Form, Handle, Marks, Piece, PointOfView, Pos, Reason, Sizes,
    SourceRange, Space,
};
use crate::view_gate::{self, Asking};
use crate::Core;

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

fn rig() -> Rig {
    rig_scripted(|_| {}, |_| crate::provider::Scripted::text("ok"))
}

/// An exchange of `session` at `place`, as the ledger writes it.
fn exchange_of(core: &Core, turn: &str, session: &str, place: &str) {
    let e = Exchange {
        turn_id: turn.into(),
        session_id: session.into(),
        place: place.into(),
        private: true,
        opened_by: Opener::Message,
        person: None,
        start_ms: 1_000,
        end_ms: Some(2_000),
        ask: None,
        reply: None,
        messages: Vec::new(),
        stop: None,
        rebuilt: false,
    };
    core.store
        .append(&[exchange::exchange_record(&e).unwrap()])
        .unwrap();
}

fn piece(place: &str, turn: &str) -> Piece {
    let pos = Pos {
        at_ms: 1_000,
        id: turn.into(),
    };
    Piece {
        range: SourceRange {
            space: Space::Exchanges {
                place: place.into(),
            },
            first: pos.clone(),
            last: pos,
        },
        band: Band::Recall,
        slot: Band::Recall.row().slot,
        reasons: BTreeSet::from([Reason::Recalled]),
        form: Form::Line,
        marks: Marks {
            place: Some(place.into()),
            audience: Audience::Private,
            author: Author::His {
                may_authorise: true,
            },
            outside: false,
            sealed: false,
            veiled: false,
        },
        sizes: Sizes {
            whole: 30,
            spoken: 20,
            line: 10,
        },
        handle: Handle(format!("x:{turn}")),
        unit: None,
    }
}

/// A view at `anchor` holding `pieces`, kept for `turn` under `compilation`.
fn keep(core: &Core, turn: &str, compilation: &str, anchor: &str, pieces: Vec<Piece>) {
    let shown: Vec<Shown> = pieces
        .into_iter()
        .map(|p| Shown {
            form: p.form,
            tokens: p.tokens(),
            piece: p,
        })
        .collect();
    let view = View {
        pov: PointOfView {
            anchor: Anchor::Place(anchor.into()),
            person: None,
            present: crate::view::Present::OwnerAlone,
            at_hand: None,
            now_ms: 5_000,
        },
        tokens: shown.iter().map(|s| s.tokens).sum(),
        shown,
        dropped: Vec::new(),
        residue: Vec::new(),
    };
    record::keep(&core.store, turn, compilation, &view).unwrap();
}

async fn show(
    core: &Arc<Core>,
    client: Client,
    params: serde_json::Value,
) -> Result<String, String> {
    match rpc_as(core, client, method::VIEW_SHOW, params).await {
        Ok(v) => Ok(v.to_string()),
        Err(e) => {
            assert_eq!(e.code, error_code::REFUSED, "{e:?}");
            Err(e.message)
        }
    }
}

/// Only the owner in person reads a turn's view, the record as well as its text: an agent's process at the
/// terminal, a Theseus job (its process, or a shell carrying its marker), an agent behind the web UI and the
/// Discord binding are each refused in words, for another place's turn, and none is told to try without
/// `--text`.
#[tokio::test]
async fn view_show_answers_the_owner_in_person_alone() {
    let r = rig();
    let lab_place = format!("discord:channel:{LAB}");
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    exchange_of(&r.core, "t-lab", &lab, &lab_place);
    keep(
        &r.core,
        "t-lab",
        "cmp_lab",
        &lab_place,
        vec![piece(&lab_place, "t-lab")],
    );
    let named = json!({"turn_id": "t-lab"});
    let agent = surface("sock#2", Surface::Cli).behind(Behind::Agent("claude".into()));
    let job = surface("sock#3", Surface::Cli).behind(Behind::Agent(JOB.into()));
    let web_agent = surface("web#2", Surface::Web).behind(Behind::Agent("claude".into()));
    let discord = Client::new("discord", Surface::Discord);
    for (who, client, params) in [
        ("agent", agent.clone(), named.clone()),
        (
            "agent, text",
            agent,
            json!({"turn_id": "t-lab", "text": true}),
        ),
        ("job", job, named.clone()),
        (
            "job's marker",
            surface("sock#4", Surface::Cli),
            json!({"turn_id": "t-lab", "from_job": "s1"}),
        ),
        ("web agent", web_agent, named.clone()),
        ("discord", discord, named.clone()),
    ] {
        let said = show(&r.core, client, params).await.unwrap_err();
        assert!(said.starts_with("not shown:"), "{who}: {said}");
        assert!(
            !said.contains("--text") && !said.contains("without"),
            "{who}: {said}"
        );
        assert!(!said.contains("x:t-lab"), "{who}: {said}");
    }
    let mine = show(&r.core, surface("sock#1", Surface::Cli), named.clone())
        .await
        .unwrap();
    assert!(mine.contains("x:t-lab"), "{mine}");
    let web = show(
        &r.core,
        surface("web#1", Surface::Web),
        json!({"turn_id": "t-lab", "tally": true}),
    )
    .await
    .unwrap();
    assert!(
        web.contains("x:t-lab") && web.contains("\"tallies\":[{"),
        "{web}"
    );
}

/// A thread `kelp` with one exchange; its session and the place it speaks in.
fn kelp(r: &Rig) -> (String, String) {
    r.core
        .thread_new(
            &ThreadNewParams {
                slug: "kelp".into(),
                title: Some("Kelp".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    let kelp = r.core.thread_session("kelp", cli()).unwrap().session_id;
    let place = view_gate::place_name(&r.core.runner, &kelp).expect("a place");
    exchange_of(&r.core, "t-kelp", &kelp, &place);
    (kelp, place)
}

/// Seat `session`'s turn `turn` with `present` at `place`, as the turn's own start does.
fn seat(core: &Core, session: &str, turn: &str, present: Present, place: &str) {
    core.runner.presence.set(
        session,
        Asking {
            turn_id: turn.into(),
            present,
            place: place.into(),
        },
    );
}

/// A seal proposed after the view was kept: the owner's inspection and the model's own look, the owner's
/// alone and a reader in company at the same place, all leave the piece out from then on.
#[tokio::test]
async fn a_seal_proposed_after_keep_holds_the_piece_out_of_every_reader() {
    let r = rig();
    let (_kelp, place) = kelp(&r);
    let desk = session(&r.core, None);
    keep(
        &r.core,
        "t-desk",
        "cmp_desk",
        &place,
        vec![piece(&place, "t-kelp")],
    );
    let owner = || surface("sock#1", Surface::Cli);
    let named = json!({"turn_id": "t-desk"});
    let look = |present| {
        seat(&r.core, &desk, "t-desk", present, &place);
        crate::look::view::words(&r.core, &desk, "t-desk")
    };
    assert!(show(&r.core, owner(), named.clone())
        .await
        .unwrap()
        .contains("x:t-kelp"));
    assert!(look(Present::OwnerAlone).contains("x:t-kelp"));
    let company = look(Present::PrivateChannel);
    assert!(company.contains("x:t-kelp"), "{company}");
    assert!(
        company.contains(crate::view::record::show::RESTRICTED),
        "{company}"
    );

    assert!(r
        .core
        .thread_recommend_seal("kelp", "holds a password")
        .unwrap());
    let shown = show(&r.core, owner(), json!({"turn_id": "t-desk", "text": true}))
        .await
        .unwrap();
    assert!(!shown.contains("x:t-kelp"), "{shown}");
    for present in [Present::OwnerAlone, Present::PrivateChannel] {
        let said = look(present);
        assert!(!said.contains("x:t-kelp"), "{present:?}: {said}");
        assert!(said.contains("cmp_desk"), "{present:?}: {said}");
    }
}

/// The look reads this turn's presence alone: one kept for another turn of the session, or none, shows
/// nothing; and of a turn's many compilations it reads the newest.
#[tokio::test]
async fn the_look_reads_this_turns_presence_and_its_newest_view() {
    let r = rig();
    let desk = session(&r.core, None);
    exchange_of(&r.core, "t-here", &desk, "local");
    for i in 0..100 {
        keep(
            &r.core,
            "t-desk",
            &format!("cmp_{i:03}"),
            "local",
            vec![piece("local", "t-here")],
        );
    }
    let unknown = crate::look::view::words(&r.core, &desk, "t-desk");
    assert!(
        unknown.contains("cannot be read") && !unknown.contains("x:t-here"),
        "{unknown}"
    );
    seat(&r.core, &desk, "t-earlier", Present::OwnerAlone, "local");
    let other = crate::look::view::words(&r.core, &desk, "t-desk");
    assert!(
        other.contains("cannot be read") && !other.contains("x:t-here"),
        "{other}"
    );
    seat(&r.core, &desk, "t-desk", Present::OwnerAlone, "local");
    let said = crate::look::view::words(&r.core, &desk, "t-desk");
    assert!(
        said.contains("view cmp_099") && !said.contains("cmp_098"),
        "{said}"
    );
    assert_eq!(said.matches("x:t-here").count(), 1, "{said}");
}

/// A piece's text is the owner's at his own terminal alone: the web UI, his own, is refused it, and is
/// still shown the references.
#[tokio::test]
async fn text_is_for_the_owners_own_shell_alone() {
    let r = rig();
    let desk = session(&r.core, None);
    exchange_of(&r.core, "t-here", &desk, "local");
    keep(
        &r.core,
        "t-desk",
        "cmp_desk",
        "local",
        vec![piece("local", "t-here")],
    );
    let text = json!({"turn_id": "t-desk", "text": true});
    let web = show(&r.core, surface("web#1", Surface::Web), text.clone())
        .await
        .unwrap_err();
    assert!(web.contains("own terminal"), "{web}");
    let refs = show(
        &r.core,
        surface("web#1", Surface::Web),
        json!({"turn_id": "t-desk"}),
    )
    .await;
    assert!(refs.unwrap().contains("x:t-here"));
    let mine = show(&r.core, surface("sock#1", Surface::Cli), text)
        .await
        .unwrap();
    assert!(
        mine.contains("x:t-here") && !mine.contains("withheld"),
        "{mine}"
    );
}

/// The owner alone is shown every piece of this turn's view, and never a piece's text.
#[tokio::test]
async fn the_owner_alone_is_shown_every_piece_and_no_text() {
    let r = rig();
    let desk = session(&r.core, None);
    exchange_of(&r.core, "t-here", &desk, "local");
    exchange_of(&r.core, "t-two", &desk, "local");
    keep(
        &r.core,
        "t-desk",
        "cmp_desk",
        "local",
        vec![piece("local", "t-here"), piece("local", "t-two")],
    );
    seat(&r.core, &desk, "t-desk", Present::OwnerAlone, "local");
    let said = crate::look::view::words(&r.core, &desk, "t-desk");
    assert!(
        said.contains("x:t-here") && said.contains("x:t-two"),
        "{said}"
    );
    assert!(said.contains("view cmp_desk"), "{said}");
    assert!(!said.contains(" | "), "no text: {said}");
    let restricted = crate::view::record::show::RESTRICTED;
    assert!(!said.contains(restricted), "{said}");
}

/// A turn with no view says so.
#[tokio::test]
async fn a_turn_with_no_view_says_so() {
    let r = rig();
    let desk = session(&r.core, None);
    seat(&r.core, &desk, "t-desk", Present::OwnerAlone, "local");
    let said = crate::look::view::words(&r.core, &desk, "t-desk");
    assert!(said.contains("no view record"), "{said}");
}
