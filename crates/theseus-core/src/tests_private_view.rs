//! The private view, held byte for byte (theseus-9ymz0). The open view these
//! tests were written for (theseus-a1wvq.7, `tests_open_view`) and the draft's
//! narrower one are gone; the golden of a private session's request is not.
//!
//! The rig is `tests_places`'s: a work tree with the owner's notes (a private
//! context file) and a public tree (its README a context file marked public),
//! here with memory live, a standing order, preference and fact, a person in
//! the ontology, and a recalled episode.

use std::path::Path;
use std::sync::Arc;

use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};
use theseus_protocol::standing::{StandingAddParams, StandingKind, StandingScope};
use theseus_protocol::{
    OntologyCategoryAddParams, OntologyGuidanceSetParams, OntologyMembershipSetParams,
};

use crate::config::MemoryMode;
use crate::provider::ProviderRequest;
use crate::recall::{Ask, AskFuture};
use crate::tests_places::{rig_with, session, turn, Rig, OWNER, SECRET};
use crate::Core;

/// `tests/golden/private_requests.txt`: the first requests of a CLI session
/// and of the owner's DM. `THESEUS_GOLDEN=write` writes it again.
const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/private_requests.txt"
);

/// The markers of the owner's material: none may reach an open request.
pub(crate) const PREF: &str = "the owner likes replies in limericks";
pub(crate) const FACT: &str = "the owner keeps bees at Hollowmere";
pub(crate) const PERSON: &str = "Wendell Widgetco";
pub(crate) const EPISODE: &str = "the Hollowmere hives swarmed on Tuesday";
/// What every view keeps: an order.
pub(crate) const ORDER: &str = "never push to main";
/// What the public README says (`tests_places`).
pub(crate) const OPEN: &str = "the open tide table";
/// The message already in each session's transcript, and the turn's.
pub(crate) const EARLIER: &str = "Is the tide table in the open tree current?";
pub(crate) const ASKED: &str = "What happened to the hives?";

/// When the episode was said: a fixed noon, so its stamp names one day.
const EPISODE_MS: u64 = 1_767_787_200_000;

/// The rig with memory live, and the owner's material: the records, the
/// person (with guidance naming them), and an earlier session that holds the
/// episode, which the stand-in index answers; and the ids a request names
/// (the records', that session's, the episode's).
pub(crate) fn owners_rig(tweak: impl FnOnce(&mut crate::Config)) -> (Rig, Vec<String>) {
    let r = rig_with(|c| {
        c.memory.mode = MemoryMode::Live;
        tweak(c);
    });
    let c = &r.core;
    let mut ids = Vec::new();
    for (kind, text) in [
        (StandingKind::Order, ORDER),
        (StandingKind::Preference, PREF),
        (StandingKind::Fact, FACT),
    ] {
        let added = c.standing_add(
            &StandingAddParams {
                kind,
                text: text.into(),
                scope: StandingScope::Everywhere,
                ..Default::default()
            },
            "the CLI",
        );
        ids.push(added.unwrap().record.id);
    }
    let earlier = session(c, None);
    let mut n = crate::node::Node::user(&earlier, None, "test", EPISODE);
    n.created_at_ms = EPISODE_MS;
    c.store.append(&[n.record().unwrap()]).unwrap();
    c.runner.memory.set_ask(index_of(c, vec![earlier.clone()]));
    ids.extend([earlier, n.id]);
    (r, ids)
}

/// The person, with guidance, and `sid` a member of them.
pub(crate) fn with_person(c: &Core, sid: &str) {
    let p = c
        .ontology_category_add(
            &OntologyCategoryAddParams {
                kind: Some("person".into()),
                name: PERSON.into(),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    c.ontology_guidance_set(
        &OntologyGuidanceSetParams {
            category: p.id.clone(),
            text: format!("{PERSON} is the owner's accountant."),
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
    c.ontology_membership_set(
        &OntologyMembershipSetParams {
            session_id: sid.into(),
            add: vec![p.id],
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
}

/// A stand-in index over `sessions`: each of their nodes, newest first, at
/// the time it was written.
pub(crate) fn index_of(core: &Arc<Core>, sessions: Vec<String>) -> Ask {
    let store = core.store.clone();
    Arc::new(move |p| -> AskFuture {
        let mut hits = Vec::new();
        for sid in &sessions {
            for (position, n) in store.session_nodes(sid).unwrap() {
                if p.as_of.is_none_or(|a| position < a) {
                    hits.push((position, n));
                }
            }
        }
        hits.sort_by_key(|h| std::cmp::Reverse(h.0));
        let hits = hits
            .into_iter()
            .enumerate()
            .map(|(i, (position, n))| IndexHit {
                text: crate::recall::text_of(&n),
                time_ms: n.created_at_ms,
                node_id: n.id,
                chunk: 0,
                session_id: n.session_id,
                position,
                kind: "user_message".into(),
                origin: "operator".into(),
                author: None,
                place: None,
                tool: None,
                external: false,
                entities_matched: vec![],
                entities: Vec::new(),
                sources: [(
                    "bm25".to_string(),
                    IndexSourceRank {
                        rank: i + 1,
                        score: 1.0,
                    },
                )]
                .into(),
                fused: 1.0 / (61 + i) as f64,
            })
            .collect();
        Box::pin(async move {
            Ok(IndexQueryResult {
                hits,
                indexed_through: 0,
                lag: Default::default(),
                timings: Default::default(),
                skipped: Default::default(),
                weights: Default::default(),
            })
        })
    })
}

/// The owner's earlier message in `sid`, written before its first turn.
pub(crate) fn said_earlier(c: &Core, sid: &str) {
    let n = crate::node::Node::user(sid, None, "test", EARLIER);
    c.store.append(&[n.record().unwrap()]).unwrap();
}

/// A session at `place` working in `dir`.
pub(crate) fn session_in(c: &Core, place: Option<&str>, dir: &Path) -> String {
    let sid = session(c, place);
    c.store
        .update_session(&sid, |r| {
            r.dir = Some(dir.to_string_lossy().into_owned());
            Ok(vec![])
        })
        .unwrap();
    sid
}

/// The request's body, as the golden holds it: its directories `<dir>`, the
/// ids it names aliased (whole, then the six characters a handle keeps), the
/// time zone a stamp names `<zone>`, every run of digits `#`, the machine's
/// CPU count `<cpus>`;
/// the tools by name and the digest of their definitions, so a request's
/// every byte is held without 100 KB of schemas in the file.
pub(crate) fn body(req: &ProviderRequest, dir: &Path, ids: &[String]) -> String {
    let mut v = serde_json::to_value(req).unwrap();
    let tools = std::mem::take(v["tools"].as_array_mut().unwrap());
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    let mut text = serde_json::to_string_pretty(&v).unwrap();
    let defs = serde_json::to_string(&tools).unwrap();
    let scrub = |s: &str| {
        let mut s = s.replace(dir.to_str().unwrap(), "<dir>");
        for (i, id) in ids.iter().enumerate() {
            s = s.replace(id, &format!("<id{}>", (b'A' + i as u8) as char));
            s = s.replace(
                &id[id.len() - 6..],
                &format!("<id{}>", (b'A' + i as u8) as char),
            );
        }
        let s: String = s
            .chars()
            .fold((String::new(), false), |(mut out, digit), ch| {
                let d = ch.is_ascii_digit();
                if !d {
                    out.push(ch);
                } else if !digit {
                    out.push('#');
                }
                (out, d)
            })
            .0;
        // `said Wed #:# MST`: the zone is the machine's.
        let mut parts = s.split("#:# ");
        let mut out = parts.next().unwrap_or_default().to_string();
        for p in parts {
            let rest = p
                .find([' ', '(', ')', '"', ';', '\\'])
                .map_or("", |i| &p[i..]);
            out.push_str("#:# <zone>");
            out.push_str(rest);
        }
        // `CPUs available to commands: # (cgroup quota; …).`: the machine's.
        let cpus = "CPUs available to commands: ";
        match out.split_once(cpus) {
            Some((head, tail)) => {
                let rest = tail.find('.').map_or("", |i| &tail[i..]);
                format!("{head}{cpus}<cpus>{rest}")
            }
            None => out,
        }
    };
    text = scrub(&text);
    let digest = crate::config_copy::sha256(&scrub(&defs));
    format!(
        "{text}\ntools: {}\ntools sha256: {digest}\n",
        names.join(" ")
    )
}

/// The first request of (a) a CLI session and (b) the owner's DM, each with
/// an earlier message, a private and a public context file, the standing records, a person, a
/// directory and a recalled episode, as it is today: byte for byte the
/// golden. It is the guard of the view the open view's deletion must not move:
/// nothing theseus-9ymz0 removed or narrowed reaches a private session's
/// request.
#[tokio::test]
async fn a_private_sessions_request_is_byte_identical() {
    let (r, ids) = owners_rig(|_| {});
    let c = &r.core;
    let tmp = r.root.parent().unwrap().to_path_buf();
    let mut got = String::new();
    for (name, place) in [("cli", None), ("dm", Some(format!("dm:{OWNER}")))] {
        let sid = session_in(c, place.as_deref(), &r.root);
        with_person(c, &sid);
        said_earlier(c, &sid);
        turn(c, &sid, ASKED).await;
        let req = r.requests().pop().unwrap();
        let text = serde_json::to_string(&req).unwrap();
        for marker in [SECRET, OPEN, ORDER, PREF, FACT, PERSON, EPISODE] {
            assert!(
                text.contains(marker),
                "{name}: the private view carries {marker:?}"
            );
        }
        got.push_str(&format!(
            "== {name}\n{}",
            body(&req, &tmp, &[&[sid], &ids[..]].concat())
        ));
    }
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        let to = std::env::var("THESEUS_GOLDEN_TO").unwrap_or_else(|_| GOLDEN.to_string());
        std::fs::write(to, &got).unwrap();
    }
    let want = std::fs::read_to_string(GOLDEN).unwrap_or_default();
    assert!(
        got == want,
        "a private session's request moved from tests/golden/private_requests.txt \
         (THESEUS_GOLDEN=write writes it again):\n{got}"
    );
}
