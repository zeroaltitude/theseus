//! The view record's readers through the whole core (issue 44): `view.show` answers the owner in person
//! alone, over real protocol connections; a seal proposed after a view was kept keeps its piece out of the
//! owner's inspection and of the model's own look; and the look reads this turn's presence alone.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::json;
use theseus_protocol::{error_code, method};

use crate::approval::{Client, Surface};
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
use crate::Core;

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
    keep_at(core, turn, compilation, anchor, pieces, 5_000);
}

/// [`keep`], built at `now_ms`.
fn keep_at(
    core: &Core,
    turn: &str,
    compilation: &str,
    anchor: &str,
    pieces: Vec<Piece>,
    now_ms: u64,
) {
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
            now_ms,
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

/// A turn's view is a read (anything goes, 2026-11-22): an agent's process at the terminal, a Theseus job (its
/// process, or a shell carrying its marker), an agent behind the web UI and the Discord binding are each
/// answered, the record and its text alike, for another place's turn.
#[tokio::test]
async fn view_show_answers_every_reader() {
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
        ("owner", surface("sock#1", Surface::Cli), named.clone()),
    ] {
        let shown = show(&r.core, client, params)
            .await
            .unwrap_or_else(|e| panic!("{who}: {e}"));
        assert!(shown.contains("x:t-lab"), "{who}: {shown}");
    }
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

/// Of a turn's many compilations, the look reads the newest.
#[tokio::test]
async fn the_look_reads_this_turns_newest_view() {
    let r = rig();
    let desk = session(&r.core, None);
    exchange_of(&r.core, "t-here", &desk, "local");
    for i in 0..100u64 {
        let c = format!("cmp_{i:03}");
        keep_at(
            &r.core,
            "t-desk",
            &c,
            "local",
            vec![piece("local", "t-here")],
            5_000 + i,
        );
    }
    let said = crate::look::view::words(&r.core, &desk, "t-desk");
    assert!(
        said.contains("view cmp_099") && !said.contains("cmp_098"),
        "{said}"
    );
    assert_eq!(said.matches("x:t-here").count(), 1, "{said}");
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
    let said = crate::look::view::words(&r.core, &desk, "t-desk");
    assert!(said.contains("no view record"), "{said}");
}
