//! The gate (issue 44, step 2) through the whole core: who is present, the
//! registry of decisions by place, the gate over generated stores, the
//! repairs that stand on their own (a session read by id holds out a sealed
//! thread's; a channel bound private draws on itself alone), and the
//! red-team cases, each ending with nothing admitted.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use serde_json::json;
use theseus_memory::gate::{Denied, Present, Seal, Veil};
use theseus_memory::recall::Place;
use theseus_protocol::memory::MemoryLookupParams;
use theseus_protocol::threads::{ThreadNewParams, ThreadSealParams};

use crate::approval::{Answerer, Surface};
use crate::places::BoundPlace;
use crate::tests_memory_lookup::{index_over, script};
use crate::tests_places::{rig_scripted, session, turn, Rig, LAB, OWNER};
use crate::view_gate::{self, Gate, Input, OwnerMarks, RecallPlaces};
use crate::Core;

/// A guild channel bound shared.
const PIER: u64 = 141_421_356_237_309_504;
/// What the owner said at his terminal; no other place may be shown it.
const HIS: &str = "the harbour gate code is 0451";

/// The marks of a fake ledger: a node's in-person answer, or a read error.
struct Fake(BTreeMap<&'static str, Option<bool>>);

impl OwnerMarks for Fake {
    fn in_person(&self, node: &str) -> Result<Option<bool>> {
        match self.0.get(node) {
            Some(Some(b)) => Ok(Some(*b)),
            Some(None) => anyhow::bail!("the mark of {node} cannot be read"),
            None => Ok(None),
        }
    }
}

fn fake() -> Fake {
    Fake(BTreeMap::from([
        ("his", Some(true)),
        ("agent", Some(false)),
        ("broken", None),
    ]))
}

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

fn bind(core: &Core) {
    core.bind_places(vec![
        BoundPlace {
            persona: None,
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: true,
            guild: Some("100000000000000001".into()),
            ceiling: None,
        },
        BoundPlace {
            persona: None,
            target: format!("discord:channel:{PIER}"),
            name: "#pier".into(),
            private: false,
            guild: Some("100000000000000001".into()),
            ceiling: None,
        },
    ]);
}

fn rig() -> Rig {
    let r = rig_scripted(|_| {}, script);
    bind(&r.core);
    r
}

/// The gate's point of view for a turn of `sid`, opened by `input`.
fn pov(
    core: &Core,
    sid: &str,
    input: Input<'_>,
    marks: &dyn OwnerMarks,
) -> theseus_memory::gate::PointOfView {
    let place = core.runner.place_of(sid);
    let target = core.outbox.try_target(sid).unwrap();
    theseus_memory::gate::PointOfView {
        anchor: view_gate::anchor_of(target.as_deref()),
        present: view_gate::present(&place, target.as_deref(), input, marks),
        place,
    }
}

/// What the gate admits of `sessions` for `pov`, whole.
fn admitted(
    core: &Core,
    pov: &theseus_memory::gate::PointOfView,
    asker: &str,
    sessions: &[String],
) -> Vec<String> {
    sessions
        .iter()
        .filter_map(|s| {
            let m = view_gate::session_marks(&core.runner, Some(asker), s, Veil::Clear);
            Gate::check(pov, &m).ok().map(|()| s.clone())
        })
        .collect()
}

#[test]
fn owner_alone_is_the_terminal_the_page_or_a_dm_and_his_mark() {
    let f = fake();
    let dm = "discord:dm:7";
    let lab = format!("discord:channel:{LAB}");
    let p =
        |place: &Place, target: Option<&str>, input| view_gate::present(place, target, input, &f);
    assert_eq!(
        p(&Place::Private, None, Input::Message("his")),
        Present::OwnerAlone
    );
    assert_eq!(
        p(&Place::Private, Some(dm), Input::Message("his")),
        Present::OwnerAlone
    );
    // An agent's process at the terminal is not alone; nor is a message with
    // no mark, nor one whose mark cannot be read.
    assert_eq!(
        p(&Place::Private, None, Input::Message("agent")),
        Present::PrivateChannel
    );
    assert_eq!(
        p(&Place::Private, None, Input::Message("unmarked")),
        Present::PrivateChannel
    );
    assert_eq!(
        p(&Place::Private, None, Input::Message("broken")),
        Present::Unknown
    );
    // A channel is never alone, whoever is in it.
    assert_eq!(
        p(&Place::Private, Some(&lab), Input::Message("his")),
        Present::PrivateChannel
    );
    assert_eq!(
        p(
            &Place::Shared(lab.clone()),
            Some(&lab),
            Input::Message("his")
        ),
        Present::Shared
    );
    assert_eq!(
        p(&Place::Unknown, None, Input::Message("his")),
        Present::Unknown
    );
    assert_eq!(
        p(&Place::Open(None), None, Input::Message("his")),
        Present::Open
    );
    // A wake or a report takes the presence of the turn that set it up.
    assert_eq!(
        p(
            &Place::Private,
            None,
            Input::SetUp(Some(Present::OwnerAlone))
        ),
        Present::OwnerAlone
    );
    assert_eq!(
        p(
            &Place::Private,
            None,
            Input::SetUp(Some(Present::PrivateChannel))
        ),
        Present::PrivateChannel
    );
    assert_eq!(
        p(&Place::Private, None, Input::SetUp(None)),
        Present::Unknown
    );
    assert_eq!(
        p(
            &Place::Private,
            Some(&lab),
            Input::SetUp(Some(Present::OwnerAlone))
        ),
        Present::PrivateChannel
    );
}

#[tokio::test]
async fn the_store_reads_the_owners_mark_as_step_one_writes_it() {
    let r = rig();
    let said = |node: &str, in_person: bool| {
        json!({"node": node, "in_person": in_person, "owner_handle": null, "peer": "person",
               "agent": null, "surface": "cli", "place": "local", "arrived_session": "ses_a",
               "ran_session": "ses_a", "author": null, "at_ms": 1})
    };
    r.core
        .store
        .put_meta("said.nod_his", &said("nod_his", true))
        .unwrap();
    r.core
        .store
        .put_meta("said.nod_agent", &said("nod_agent", false))
        .unwrap();
    let s = &r.core.store;
    assert_eq!(s.in_person("nod_his").unwrap(), Some(true));
    assert_eq!(s.in_person("nod_agent").unwrap(), Some(false));
    assert_eq!(s.in_person("nod_none").unwrap(), None);
}

/// The registry of decisions by place (the design's §4): each file's count
/// of the calls that decide by place, as they were when the gate landed.
/// They stay as they are; a new one goes through the gate. A count may fall.
const DECISIONS: &[(&str, usize)] = &[
    ("capture/act.rs", 2),
    ("card/noted.rs", 1),
    ("card/observe.rs", 2),
    ("chat_acts/threads.rs", 1),
    ("config_change/wall.rs", 1),
    ("consolidate/guidance.rs", 1),
    ("content_scene.rs", 1),
    ("judge/categorize.rs", 1),
    ("judge/onto/judged.rs", 1),
    ("judge/people/live.rs", 1),
    ("judge/people/sweep.rs", 1),
    ("learning/replay_onto.rs", 1),
    ("memory_lookup.rs", 2),
    ("memory_lookup/mcp.rs", 1),
    ("own_bench/replay.rs", 1),
    ("own_bench/sampler.rs", 1),
    ("push/last_line.rs", 1),
    ("recall.rs", 2),
    ("recall/cite.rs", 1),
    ("retract.rs", 2),
    ("rpc/books.rs", 1),
    ("rpc/cites.rs", 1),
    ("rpc/digest.rs", 1),
    ("rpc/fork.rs", 1),
    ("rpc/grants.rs", 1),
    ("rpc/mcp.rs", 1),
    ("rpc/memory.rs", 1),
    ("rpc/modes.rs", 1),
    ("rpc/peek.rs", 1),
    ("rpc/search_text.rs", 1),
    ("rpc/task_rehome.rs", 1),
    ("rpc/thread_flow.rs", 2),
    ("rpc/thread_home.rs", 2),
    ("rpc/thread_surface.rs", 1),
    ("rpc/threads.rs", 2),
    ("rpc/work_board.rs", 1),
    ("thread_read.rs", 2),
    ("threads.rs", 1),
    ("turn/inbound_step.rs", 1),
    ("turn/exchange_step.rs", 1),
    ("turn/recall_step.rs", 1),
    ("turn/reply_step.rs", 1),
    ("turn/route_step/correct.rs", 1),
    ("yes_fact.rs", 1),
];

/// The calls that decide by place.
const BY_PLACE: [&str; 5] = [
    "place_of(",
    ".class_of(",
    "may_draw_on(",
    "may_draw_at(",
    "owner_alone(",
];

/// Every `.rs` file under `dir`, by its path from `root`, but tests.
fn sources(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let rel = p
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with("tests") {
            continue;
        }
        if p.is_dir() {
            sources(root, &p, out);
        } else if name.ends_with(".rs") {
            out.push((rel, std::fs::read_to_string(&p).unwrap()));
        }
    }
}

/// The calls by place in `text`: never a definition (`fn place_of(`), and
/// never the ledger's `exchange::place_of`, which names a place to record it.
fn decisions(text: &str) -> usize {
    BY_PLACE
        .iter()
        .map(|pat| {
            text.match_indices(pat)
                .filter(|(at, _)| {
                    let before = &text[..*at];
                    let name = before.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_');
                    !name.trim_end().ends_with("fn") && !before.ends_with("exchange::")
                })
                .count()
        })
        .sum()
}

#[test]
fn no_new_decision_by_place_outside_the_gate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &root, &mut files);
    let known: BTreeMap<&str, usize> = DECISIONS.iter().copied().collect();
    let mut grown = Vec::new();
    for (rel, text) in &files {
        // The gate itself, its turn step, and the ledger, which names places
        // to record them.
        if rel == "view_gate.rs" || rel == "turn/gate_step.rs" || rel.starts_with("exchange") {
            continue;
        }
        let n = decisions(text);
        let was = known.get(rel.as_str()).copied().unwrap_or(0);
        if n > was {
            grown.push(format!("{rel}: {n}, was {was}"));
        }
    }
    assert!(
        grown.is_empty(),
        "a new decision by place outside the gate (crate::view_gate): read the place through it \
         instead. {grown:?}"
    );
}

/// The names a file gives the rule's module (`use theseus_memory::gate as x;`,
/// or the path itself), and whether it imports `check` bare.
fn rule_names(text: &str) -> (Vec<String>, bool) {
    let mut names = vec!["theseus_memory::gate".to_string()];
    let mut bare = false;
    for l in text.lines().map(str::trim) {
        if let Some(rest) = l.strip_prefix("use theseus_memory::gate") {
            if let Some(alias) = rest.strip_prefix(" as ") {
                names.push(alias.trim_end_matches(';').trim().to_string());
            }
            if rest.starts_with("::") && rest.contains("check") {
                bare = true;
            }
            if let Some(alias) = rest
                .split("self as ")
                .nth(1)
                .map(|a| a.split(|c: char| !c.is_alphanumeric() && c != '_').next())
            {
                names.extend(alias.map(str::to_string));
            }
        }
    }
    (names, bare)
}

/// Only `crate::view_gate` calls the rule, under any name a file gives it.
/// A heuristic over the sources, beside what the types enforce (the view's
/// `Admitted` is made only in `view::gate`).
#[test]
fn only_the_gate_calls_the_rule() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut callers = Vec::new();
    for e in std::fs::read_dir(crates).unwrap() {
        let src = e.unwrap().path().join("src");
        if !src.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        sources(&src, &src, &mut files);
        for (rel, text) in files {
            let (names, bare) = rule_names(&text);
            let squeezed: String = text.split_whitespace().collect();
            let calls = names
                .iter()
                .any(|n| squeezed.contains(&format!("{n}::check(")))
                || (bare && squeezed.contains("check("));
            let home = (src.ends_with("theseus-core/src") && rel == "view_gate.rs")
                || (src.ends_with("theseus-memory/src") && rel == "gate.rs");
            if calls && !home {
                callers.push(format!("{}/{rel}", src.display()));
            }
        }
    }
    assert!(
        callers.is_empty(),
        "only crate::view_gate calls the gate's rule: {callers:?}"
    );
    // The heuristic sees through an alias.
    let aliased = "use theseus_memory::gate as permits;\nfn f() { permits :: check(&p, &m); }";
    let (names, _) = rule_names(aliased);
    let squeezed: String = aliased.split_whitespace().collect();
    assert!(names
        .iter()
        .any(|n| squeezed.contains(&format!("{n}::check("))));
}

/// A recall in a channel bound private draws on the channel alone; at the
/// terminal it draws on every place, as before.
#[tokio::test]
async fn a_channel_bound_private_recalls_its_own_place_alone() {
    let r = rig();
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    let desk = session(&r.core, None);
    turn(&r.core, &desk, HIS).await;
    turn(&r.core, &lab, "the lab's own note on the harbour").await;
    assert_eq!(
        RecallPlaces::of(&r.core.runner, Some(&lab)).channel(),
        Some(format!("discord:channel:{LAB}").as_str())
    );
    assert_eq!(
        RecallPlaces::of(&r.core.runner, Some(&desk)).channel(),
        None
    );
    r.core
        .runner
        .memory
        .set_ask(index_over(&r.core, vec![desk.clone(), lab.clone()]));
    let words = json!({"words": "harbour"});
    let got = crate::memory_lookup::call(r.core.clone(), &lab, words.clone())
        .await
        .unwrap()
        .text;
    assert!(!got.contains(HIS), "{got}");
    let other = session(&r.core, None);
    view_gate::seat_owner(&r.core.runner, &other);
    let got = crate::memory_lookup::call(r.core.clone(), &other, words)
        .await
        .unwrap()
        .text;
    assert!(
        got.contains(HIS),
        "the terminal draws on every place, as before: {got}"
    );
}

/// Red team: a guest speaks in a channel bound private. Its turn is not the
/// owner alone, whoever wrote, and nothing from elsewhere is admitted: not by
/// the gate, not by a read by id, not in a listing.
#[tokio::test]
async fn red_team_a_guest_in_a_channel_bound_private_gets_nothing_from_elsewhere() {
    let r = rig();
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    let desk = session(&r.core, None);
    let dm = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &desk, HIS).await;
    let f = Fake(BTreeMap::from([
        ("guest", Some(false)),
        ("his", Some(true)),
    ]));
    for input in ["guest", "his"] {
        let p = pov(&r.core, &lab, Input::Message(input), &f);
        assert_eq!(p.present, Present::PrivateChannel, "{input}");
        let got = admitted(&r.core, &p, &lab, &[desk.clone(), dm.clone()]);
        assert!(got.is_empty(), "{input}: {got:?}");
    }
    let read = crate::memory_lookup::call(r.core.clone(), &lab, json!({"session": desk})).await;
    assert!(
        read.as_ref()
            .is_err_and(|e| e.contains("is not this channel's")),
        "{read:?}"
    );
    let listed = crate::memory_lookup::call(r.core.clone(), &lab, json!({"when": "since 2020"}))
        .await
        .unwrap()
        .text;
    assert!(!listed.contains(&desk), "{listed}");
    let opened = crate::thread_read::call(r.core.clone(), &lab, json!({"thread": "any"})).await;
    assert!(opened.is_err_and(|e| e.contains("channel bound private")));
}

/// Red team: an agent's process at the terminal asks for the owner's words
/// from elsewhere. Its input's mark says it was not the owner in person, so
/// the gate admits nothing from another place; the owner's own input, marked,
/// is admitted the same pieces.
#[tokio::test]
async fn red_team_an_agent_at_the_terminal_gets_none_of_the_owners_words_from_elsewhere() {
    let r = rig();
    let desk = session(&r.core, None);
    let dm = session(&r.core, Some(&format!("dm:{OWNER}")));
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    turn(&r.core, &dm, HIS).await;
    let f = fake();
    let agent = pov(&r.core, &desk, Input::Message("agent"), &f);
    assert_eq!(agent.present, Present::PrivateChannel);
    assert!(admitted(&r.core, &agent, &desk, &[dm.clone(), lab.clone()]).is_empty());
    let m = view_gate::session_marks(&r.core.runner, Some(&desk), &dm, Veil::Clear);
    assert_eq!(Gate::check(&agent, &m).err(), Some(Denied::Alone));
    let his = pov(&r.core, &desk, Input::Message("his"), &f);
    assert_eq!(his.present, Present::OwnerAlone);
    assert_eq!(
        admitted(&r.core, &his, &desk, &[dm.clone(), lab.clone()]),
        [dm, lab]
    );
}

/// Red team: a seal is set after a thread's session was read. The next read
/// by id is refused, the listing leaves it out, the MCP read answers as if
/// it were not there, and the gate denies it; the thread itself still reads
/// its own.
#[tokio::test]
async fn red_team_a_seal_set_after_holds_the_session_out_everywhere() {
    let r = rig();
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
    turn(&r.core, &kelp, HIS).await;
    let desk = session(&r.core, None);
    view_gate::seat_owner(&r.core.runner, &desk);
    let read = |sid: String| {
        let c = r.core.clone();
        let kelp = kelp.clone();
        async move { crate::memory_lookup::call(c, &sid, json!({"session": kelp})).await }
    };
    assert!(read(desk.clone()).await.unwrap().text.contains(HIS));
    let f = fake();
    let his = pov(&r.core, &desk, Input::Message("his"), &f);
    assert_eq!(
        admitted(&r.core, &his, &desk, std::slice::from_ref(&kelp)),
        std::slice::from_ref(&kelp)
    );

    r.core
        .thread_seal(
            &ThreadSealParams {
                slug: "kelp".into(),
                reason: Some("embargoed".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    let refused = read(desk.clone()).await;
    assert!(
        refused.as_ref().is_err_and(|e| e.contains("sealed thread")),
        "{refused:?}"
    );
    let words = crate::memory_lookup::call(
        r.core.clone(),
        &desk,
        json!({"session": kelp, "words": "harbour"}),
    )
    .await;
    assert!(words.is_err(), "{words:?}");
    let listed = crate::memory_lookup::call(r.core.clone(), &desk, json!({"when": "since 2020"}))
        .await
        .unwrap()
        .text;
    assert!(!listed.contains(&kelp), "{listed}");
    let p = MemoryLookupParams {
        session: Some(kelp.clone()),
        ..Default::default()
    };
    let mcp = crate::memory_lookup::mcp::get(&r.core, &p).await;
    assert!(mcp.is_err(), "{mcp:?}");
    assert_eq!(
        view_gate::seal_of(&r.core.runner, Some(&desk), &kelp),
        Seal::Held
    );
    assert!(admitted(&r.core, &his, &desk, std::slice::from_ref(&kelp)).is_empty());
    // The sealed thread reads its own session.
    assert_eq!(
        view_gate::seal_of(&r.core.runner, Some(&kelp), &kelp),
        Seal::Clear
    );
    assert!(read(kelp.clone()).await.unwrap().text.contains(HIS));
}

/// The places a generated store's sessions speak in.
const WHERE: [Option<&str>; 4] = [
    None,
    Some("dm:271828182845904523"),
    Some("channel:314159265358979323"),
    Some("channel:141421356237309504"),
];

/// Over generated stores: for each point of view that is not the owner
/// alone, what the gate admits is the same when every other place's sessions
/// are left out of the store.
#[tokio::test]
async fn other_places_change_nothing_the_gate_admits_over_generated_stores() {
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    let f = fake();
    for case in 0..8 {
        let anchor = next(WHERE.len());
        let input = ["agent", "unmarked", "his"][next(3)];
        // A place's latest session is the one that speaks there, so each
        // bound place has one session and the asker is made last; the
        // terminal has any number.
        let mut places: Vec<usize> = (0..next(4)).map(|_| 0).collect();
        places.extend((1..WHERE.len()).filter(|w| *w != anchor && next(2) == 0));
        // The whole store, then only the anchor's place.
        let mut seen = Vec::new();
        for only in [false, true] {
            let r = rig();
            let mut made: Vec<(String, String)> = places
                .iter()
                .enumerate()
                .filter(|(_, w)| !only || **w == anchor)
                .map(|(i, w)| (i.to_string(), session(&r.core, WHERE[*w])))
                .collect();
            let asker = session(&r.core, WHERE[anchor]);
            made.push(("asker".into(), asker.clone()));
            let p = pov(&r.core, &asker, Input::Message(input), &f);
            if p.present == Present::OwnerAlone {
                break;
            }
            let ids: Vec<String> = made.iter().map(|(_, s)| s.clone()).collect();
            let got = admitted(&r.core, &p, &asker, &ids);
            let labels: Vec<String> = made
                .iter()
                .filter(|(_, s)| got.contains(s))
                .map(|(l, _)| l.clone())
                .collect();
            assert!(
                labels.contains(&"asker".to_string()),
                "case {case}: {labels:?}"
            );
            seen.push((p.present, labels));
        }
        if let [whole, own] = seen.as_slice() {
            assert_eq!(whole, own, "case {case}: {places:?} at {anchor}, {input}");
        }
    }
}

/// A shared place's recall, unchanged: it draws on its own sessions alone.
#[tokio::test]
async fn a_shared_place_keeps_todays_rule() {
    let r: Rig = rig();
    let pier = session(&r.core, Some(&format!("channel:{PIER}")));
    let places = RecallPlaces::of(&r.core.runner, Some(&pier));
    assert_eq!(places.channel(), None);
    let today = r.core.runner.place_of(&pier);
    assert_eq!(places.asker(today.clone()), today);
}

/// A turn of `sid` whose input `turn.submit` marked as typed in person or by
/// an agent's process.
async fn turn_as(core: &std::sync::Arc<Core>, sid: &str, input: &str, in_person: bool) {
    use crate::session::SessionRecord;
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let mark = view_gate::test_mark(&core.runner, sid, in_person);
    let sink = crate::bus::EventSink::new(core.bus.clone(), sid, None).with_arrival(Some(mark));
    core.runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "cli".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

/// Red team, through the paths that run today: an agent's process at the
/// terminal runs a turn that looks up the owner's words. Its presence is not
/// the owner alone and its lookup draws on its own place alone; the owner in
/// person, at the same terminal, still draws on his other places.
#[tokio::test]
async fn an_agent_turn_at_the_terminal_looks_up_nothing_from_elsewhere() {
    let r = rig();
    let dm = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &dm, HIS).await;
    r.core
        .runner
        .memory
        .set_ask(index_over(&r.core, vec![dm.clone()]));
    let lookup = r#"LOOKUP {"words": "harbour gate"}"#;

    let agent = session(&r.core, None);
    turn_as(&r.core, &agent, lookup, false).await;
    let a = r.core.runner.presence.get(&agent).expect("its presence");
    assert_eq!(a.present(), Present::PrivateChannel);
    assert_eq!(a.place(), "local");
    let got = crate::tests_places::result_of(&r.core, &agent, "m1");
    assert!(!got.contains(HIS), "{got}");
    // Its memory.lookup by id, and its listing, keep to its own place too.
    let by_id = crate::memory_lookup::call(r.core.clone(), &agent, json!({"session": dm})).await;
    assert!(by_id.is_err(), "{by_id:?}");

    let owner = session(&r.core, None);
    turn_as(&r.core, &owner, lookup, true).await;
    assert_eq!(
        r.core.runner.presence.get(&owner).unwrap().present(),
        Present::OwnerAlone
    );
    let got = crate::tests_places::result_of(&r.core, &owner, "m1");
    assert!(
        got.contains(HIS),
        "the owner in person draws on his DM: {got}"
    );
}

/// With `[context] ledger` off, nothing is marked and nothing is computed:
/// the terminal reads places as before, agent or not.
#[tokio::test]
async fn with_the_ledger_off_the_terminal_reads_places_as_before() {
    let r = rig_scripted(|c| c.context.ledger = Some(false), script);
    bind(&r.core);
    let dm = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &dm, HIS).await;
    r.core
        .runner
        .memory
        .set_ask(index_over(&r.core, vec![dm.clone()]));
    let agent = session(&r.core, None);
    turn_as(
        &r.core,
        &agent,
        r#"LOOKUP {"words": "harbour gate"}"#,
        false,
    )
    .await;
    assert!(r.core.runner.presence.get(&agent).is_none());
    let got = crate::tests_places::result_of(&r.core, &agent, "m1");
    assert!(got.contains(HIS), "as before the ledger: {got}");
}

/// A wake takes the presence of the turn that set it: the call that set it,
/// in its session, names that turn, whose ask's mark the ledger wrote.
#[tokio::test]
async fn a_set_up_is_read_through_the_call_that_made_it() {
    let r = rig();
    let desk = session(&r.core, None);
    turn_as(&r.core, &desk, "plain words", false).await;
    let a = r.core.runner.presence.get(&desk).unwrap();
    // A call of that turn, as `wake.at` writes one.
    let body: crate::node::Body = serde_json::from_value(json!({"kind": "tool_call",
        "tool_use_id": "wk1", "tool": "wake.at", "wire_name": "wake_at", "input": {},
        "assistant_node": "msg_x"}))
    .unwrap();
    let call = crate::node::Node::tool_call(&desk, Some(a.turn_id()), Some(0), body);
    r.core.store.append(&[call.record().unwrap()]).unwrap();
    let class = Place::Private;
    let got = view_gate::set_up(&r.core.store, &class, &desk, "wk1");
    assert_eq!(got, Some(Present::PrivateChannel));
    assert_eq!(
        view_gate::set_up(&r.core.store, &class, &desk, "nobody"),
        None
    );
    // The owner's own turn sets up a wake that inherits his presence.
    let mine = session(&r.core, None);
    turn_as(&r.core, &mine, "plain words", true).await;
    let a = r.core.runner.presence.get(&mine).unwrap();
    let body: crate::node::Body = serde_json::from_value(json!({"kind": "tool_call",
        "tool_use_id": "wk2", "tool": "wake.at", "wire_name": "wake_at", "input": {},
        "assistant_node": "msg_y"}))
    .unwrap();
    let call = crate::node::Node::tool_call(&mine, Some(a.turn_id()), Some(0), body);
    r.core.store.append(&[call.record().unwrap()]).unwrap();
    let got = view_gate::set_up(&r.core.store, &class, &mine, "wk2");
    assert_eq!(got, Some(Present::OwnerAlone));
    assert_eq!(
        view_gate::narrowest([Some(Present::OwnerAlone), None]),
        Some(Present::Unknown)
    );
    assert_eq!(
        view_gate::narrowest([Some(Present::OwnerAlone), Some(Present::PrivateChannel)]),
        Some(Present::PrivateChannel)
    );
}

/// A thread's record and its binding, written before the core is built, as
/// the board reads them at its load; the bound session's id is returned.
fn with_thread(store: &crate::store::Store, slug: &str, seal: &str) -> String {
    use crate::session::SessionRecord;
    let r = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    store.put_session(&r.session_id, &r).unwrap();
    let mut t = json!({"slug": slug, "title": slug});
    match seal {
        "sealed" => t["sealed"] = json!({"by": "cli", "at_ms": 1, "reason": "embargo"}),
        "pending" => t["seal_pending"] = json!("holds a password"),
        "corrupt" => t = json!("not a thread"),
        _ => {}
    }
    // "absent": a binding to a thread with no record at all (F7).
    if seal != "absent" {
        store.put_meta(&format!("thread:{slug}"), &t).unwrap();
    }
    store
        .put_meta(&format!("discord.session.thread:{slug}"), &r.session_id)
        .unwrap();
    r.session_id
}

/// A core with threads `enabled` or not, built over a store that holds the
/// thread `kelp` sealed as `seal` says; and kelp's session.
fn threaded(enabled: bool, seal: &str) -> (Rig, String) {
    let kelp = std::cell::RefCell::new(String::new());
    let r = crate::tests_places::rig_setup(
        |c| c.threads.enabled = enabled,
        |s| *kelp.borrow_mut() = with_thread(s, "kelp", seal),
    );
    bind(&r.core);
    (r, kelp.into_inner())
}

/// Seals hold with `[threads] enabled = false`, on the paths that ask by
/// words alone: memory.lookup's words and the MCP server's search, for an
/// established seal, a pending one, and a thread whose record does not read.
#[tokio::test]
async fn seals_hold_on_every_search_with_threads_off() {
    // With the thread lens on too: a held hit is taken out before the lens,
    // whatever its memo says (F6).
    for (enabled, seal) in [
        (false, "sealed"),
        (false, "pending"),
        (false, "corrupt"),
        (false, "absent"),
        (true, "sealed"),
        (true, "pending"),
    ] {
        let seal = seal.to_string();
        let seal = seal.as_str();
        let (r, kelp) = threaded(enabled, seal);
        let open = session(&r.core, None);
        turn(&r.core, &kelp, HIS).await;
        turn(&r.core, &open, "the open harbour note").await;
        r.core
            .runner
            .memory
            .set_ask(index_over(&r.core, vec![kelp.clone(), open.clone()]));
        let desk = session(&r.core, None);
        view_gate::seat_owner(&r.core.runner, &desk);
        let got = crate::memory_lookup::call(r.core.clone(), &desk, json!({"words": "harbour"}))
            .await
            .unwrap()
            .text;
        assert!(!got.contains(HIS), "{seal}: {got}");
        assert!(got.contains("the open harbour note"), "{seal}: {got}");
        let p = MemoryLookupParams {
            words: Some("harbour".into()),
            ..Default::default()
        };
        let hits = crate::memory_lookup::mcp::search(&r.core, &p)
            .await
            .unwrap();
        assert!(
            hits.hits.iter().all(|h| !h.text.contains(HIS)),
            "{seal}: {hits:?}"
        );
        assert_ne!(
            view_gate::seal_of(&r.core.runner, Some(&desk), &kelp),
            Seal::Clear,
            "{seal}"
        );
        // The thread itself reads its own, unless its record does not read.
        let own = view_gate::seal_of(&r.core.runner, Some(&kelp), &kelp);
        let unread = seal == "corrupt" || seal == "absent";
        assert_eq!(own == Seal::Clear, !unread, "{seal}: {own:?}");
    }
}

/// What cannot be proven is never clear: a session whose record does not
/// decode (its nodes still readable), a thread's binding that does not
/// decode, and a session bound to a thread whose record does not decode.
#[tokio::test]
async fn unreadable_seal_provenance_is_unread() {
    use theseus_store::{kinds, NewRecord};
    let (r, _) = threaded(true, "none");
    let old = session(&r.core, None);
    turn(&r.core, &old, HIS).await;
    let bad = NewRecord::json(kinds::SESSION, Some(&old), &json!("not a session")).unwrap();
    r.core.store.append(&[bad]).unwrap();
    assert!(!r.core.store.session_nodes(&old).unwrap().is_empty());
    let desk = session(&r.core, None);
    assert_eq!(
        view_gate::seal_of(&r.core.runner, Some(&desk), &old),
        Seal::Unread
    );
    view_gate::seat_owner(&r.core.runner, &desk);
    let read = crate::memory_lookup::call(r.core.clone(), &desk, json!({"session": old})).await;
    assert!(read.is_err(), "{read:?}");
    assert_eq!(view_gate::seal_of(&r.core.runner, None, &desk), Seal::Clear);

    // A binding that does not decode: no session can be proven clear.
    let r = crate::tests_places::rig_setup(
        |_| {},
        |s| {
            s.put_meta("discord.session.thread:gone", &json!({"no": 1}))
                .unwrap()
        },
    );
    let plain = session(&r.core, None);
    assert_eq!(
        view_gate::seal_of(&r.core.runner, None, &plain),
        Seal::Unread
    );

    // A session bound to a thread whose record does not decode.
    let (r, orphan) = threaded(true, "corrupt");
    assert_eq!(
        view_gate::seal_of(&r.core.runner, None, &orphan),
        Seal::Unread
    );
    let fine = session(&r.core, None);
    assert_eq!(view_gate::seal_of(&r.core.runner, None, &fine), Seal::Clear);
}

/// The view's gate is made from the turn's verified presence: a turn in a
/// channel bound private whose arrival calls itself owner-marked passes
/// nothing from another place, and a session with no computed presence
/// passes nothing at all.
#[tokio::test]
async fn the_views_permit_reads_the_turns_verified_presence() {
    use crate::view::gate::Gate as _;
    let r = rig();
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    turn_as(&r.core, &lab, "plain words", true).await;
    let lab_place = format!("discord:channel:{LAB}");
    assert_eq!(
        r.core.runner.presence.get(&lab).unwrap().present(),
        Present::PrivateChannel
    );
    let permit = crate::view::permit::Permit::for_turn(&r.core.runner, &lab);
    let claimed = crate::view::PointOfView {
        anchor: crate::view::Anchor::Place(lab_place.clone()),
        person: Some("owner".into()),
        present: crate::view::Present::OwnerAlone,
        at_hand: None,
        now_ms: 1,
    };
    let piece = |place: &str| crate::view::permit::tests_piece(place);
    assert!(permit.admit(&claimed, piece("local")).is_none());
    assert!(permit.admit(&claimed, piece(&lab_place)).is_some());
    let never = session(&r.core, None);
    let none = crate::view::permit::Permit::for_turn(&r.core.runner, &never);
    let mut at_desk = claimed;
    at_desk.anchor = crate::view::Anchor::Place("local".into());
    assert!(none.admit(&at_desk, piece("local")).is_none());
}

/// A session with stored nodes and no record of its own cannot be proven
/// unsealed (F7).
#[tokio::test]
async fn a_native_session_with_no_record_is_unread() {
    let r = rig();
    let ghost = "ses_ghost_with_nodes";
    let n = crate::node::Node::user(ghost, None, "cli", HIS);
    r.core.store.append(&[n.record().unwrap()]).unwrap();
    assert_eq!(
        view_gate::seal_of(&r.core.runner, None, ghost),
        Seal::Unread
    );
    assert!(
        r.core.ladder_hidden(ghost),
        "one rule: the ladder's is the gate's"
    );
}

/// A node a split moved into a sealed thread is held out of a search and of a
/// read of its own session, by id or by session, whatever `[threads] enabled`
/// says; a `moved.of` that does not decode, or names a thread the board does
/// not hold, is held out too (F5).
#[tokio::test]
async fn a_node_moved_into_a_sealed_thread_is_held_out_everywhere() {
    for (enabled, moved) in [
        (false, "sealed"),
        (true, "sealed"),
        (false, "corrupt"),
        (false, "missing"),
    ] {
        let (r, _kelp) = threaded(enabled, "sealed");
        let home = session(&r.core, None);
        turn(&r.core, &home, HIS).await;
        turn(&r.core, &home, "the open harbour note").await;
        let secret = r
            .core
            .store
            .session_nodes(&home)
            .unwrap()
            .into_iter()
            .find(|(_, n)| crate::recall::text_of(n).contains(HIS))
            .map(|(_, n)| n.id)
            .unwrap();
        let key = crate::moved::moved_of_key(&secret);
        match moved {
            "sealed" => r
                .core
                .store
                .put_meta(&key, &json!({"thread": "kelp", "split": "spl_x"})),
            "corrupt" => r.core.store.put_meta(&key, &json!(17)),
            _ => r
                .core
                .store
                .put_meta(&key, &json!({"thread": "gone", "split": "spl_x"})),
        }
        .unwrap();
        r.core
            .runner
            .memory
            .set_ask(index_over(&r.core, vec![home.clone()]));
        let desk = session(&r.core, None);
        view_gate::seat_owner(&r.core.runner, &desk);
        let words = json!({"words": "harbour"});
        let got = crate::memory_lookup::call(r.core.clone(), &desk, words)
            .await
            .unwrap()
            .text;
        assert!(!got.contains(HIS), "{moved}: {got}");
        assert!(got.contains("the open harbour note"), "{moved}: {got}");
        let read = json!({"session": home});
        let got = crate::memory_lookup::call(r.core.clone(), &desk, read)
            .await
            .unwrap()
            .text;
        assert!(!got.contains(HIS), "{moved}: {got}");
        let p = MemoryLookupParams {
            id: Some(secret.clone()),
            ..Default::default()
        };
        let by_id = crate::memory_lookup::mcp::get(&r.core, &p).await;
        assert!(by_id.is_err(), "{moved}: {by_id:?}");
    }
}

/// `thread.read` checks the seal again when it runs: a thread sealed after the
/// call was planned is not read without the owner's approval of the call (F9).
#[tokio::test]
async fn thread_read_checks_the_seal_again_at_execution() {
    let (r, kelp) = threaded(true, "sealed");
    turn(&r.core, &kelp, HIS).await;
    let desk = session(&r.core, None);
    view_gate::seat_owner(&r.core.runner, &desk);
    let got = crate::thread_read::call(r.core.clone(), &desk, json!({"thread": "kelp"})).await;
    assert!(
        got.as_ref().is_err_and(|e| e.contains("not approved")),
        "{got:?}"
    );
}

/// An agent's steer into the owner's running turn at the terminal narrows the
/// turn at once: memory.lookup after it reads this place only, and what the
/// turn sets up after it inherits the narrowed presence (F2).
#[tokio::test]
async fn an_agents_steer_narrows_the_running_owners_turn() {
    use crate::exchange::{exchange_record, mark_record, Exchange, Opener};
    let r = rig();
    let dm = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &dm, HIS).await;
    r.core
        .runner
        .memory
        .set_ask(index_over(&r.core, vec![dm.clone()]));
    let desk = session(&r.core, None);
    let owner = view_gate::test_mark(&r.core.runner, &desk, true);
    let agent = view_gate::test_mark(&r.core.runner, &desk, false);
    let place = owner.place.clone();
    view_gate::begin(
        &r.core.runner,
        &desk,
        "turn_s",
        place,
        view_gate::Proof::Mark(&owner),
    );
    let words = json!({"words": "harbour gate"});
    let lookup = |input: serde_json::Value| {
        let c = r.core.clone();
        let desk = desk.clone();
        async move {
            let places = RecallPlaces::of(&c.runner, Some(&desk));
            assert_eq!(places.narrowed().is_some(), {
                c.runner.presence.get(&desk).unwrap().present() != Present::OwnerAlone
            });
            crate::memory_lookup::call(c, &desk, input)
                .await
                .unwrap()
                .text
        }
    };
    assert!(
        lookup(words.clone()).await.contains(HIS),
        "the owner alone, before the steer"
    );
    view_gate::steered(&r.core.runner, &desk, Some(&agent));
    assert_eq!(
        r.core.runner.presence.get(&desk).unwrap().present(),
        Present::PrivateChannel
    );
    assert!(
        !lookup(words.clone()).await.contains(HIS),
        "narrowed at once"
    );
    // An owner's steer after it never widens it again.
    view_gate::steered(&r.core.runner, &desk, Some(&owner));
    assert_eq!(
        r.core.runner.presence.get(&desk).unwrap().present(),
        Present::PrivateChannel
    );

    // What the turn set up after the steer inherits the narrowed presence.
    let mark = |node: &str, m: &crate::exchange::OwnerMark| {
        let mut m = m.clone();
        m.node = node.into();
        mark_record(&m).unwrap()
    };
    let ex = Exchange {
        turn_id: "turn_s".into(),
        session_id: desk.clone(),
        place: "local".into(),
        private: true,
        opened_by: Opener::Message,
        person: None,
        start_ms: 1,
        end_ms: Some(2),
        ask: Some("nod_ask".into()),
        reply: None,
        messages: vec!["nod_ask".into(), "nod_steer".into()],
        stop: None,
        rebuilt: false,
    };
    let body: crate::node::Body = serde_json::from_value(json!({"kind": "tool_call",
        "tool_use_id": "wk9", "tool": "wake.at", "wire_name": "wake_at", "input": {},
        "assistant_node": "msg_z"}))
    .unwrap();
    let call = crate::node::Node::tool_call(&desk, Some("turn_s"), Some(0), body);
    r.core
        .store
        .append(&[
            exchange_record(&ex).unwrap(),
            mark("nod_ask", &owner),
            mark("nod_steer", &agent),
            call.record().unwrap(),
        ])
        .unwrap();
    let got = view_gate::set_up(&r.core.store, &Place::Private, &desk, "wk9");
    assert_eq!(got, Some(Present::PrivateChannel));
}

/// A task started where the owner was alone reports to a parent that was
/// rebound to a shared channel since: its report's body is not written as that
/// turn's input; a content-free notice is (F8).
#[tokio::test]
async fn a_report_into_a_place_that_narrowed_is_a_notice() {
    use crate::exchange::{exchange_record, mark_record, Exchange, Opener};
    let r = rig();
    let parent = session(&r.core, None);
    let owner = view_gate::test_mark(&r.core.runner, &parent, true);
    let mut m = owner.clone();
    m.node = "nod_ask".into();
    let ex = Exchange {
        turn_id: "turn_p".into(),
        session_id: parent.clone(),
        place: "local".into(),
        private: true,
        opened_by: Opener::Message,
        person: None,
        start_ms: 1,
        end_ms: Some(2),
        ask: Some("nod_ask".into()),
        reply: None,
        messages: vec!["nod_ask".into()],
        stop: None,
        rebuilt: false,
    };
    let body: crate::node::Body = serde_json::from_value(json!({"kind": "tool_call",
        "tool_use_id": "tc_task", "tool": "task.create", "wire_name": "task_create",
        "input": {}, "assistant_node": "msg_t"}))
    .unwrap();
    let call = crate::node::Node::tool_call(&parent, Some("turn_p"), Some(0), body);
    r.core
        .store
        .append(&[
            exchange_record(&ex).unwrap(),
            mark_record(&m).unwrap(),
            call.record().unwrap(),
        ])
        .unwrap();
    let mut task =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    task.task = Some(
        serde_json::from_value(
            json!({"parent_session": parent, "parent_execution": "exe_p",
            "by": "tc_task"}),
        )
        .unwrap(),
    );
    r.core.store.put_session(&task.session_id, &task).unwrap();
    assert!(view_gate::report_shown(
        &r.core.runner,
        &parent,
        &task.session_id
    ));
    r.core
        .outbox
        .bind_place(&format!("channel:{PIER}"), &parent)
        .unwrap();
    assert!(!view_gate::report_shown(
        &r.core.runner,
        &parent,
        &task.session_id
    ));
    let notice = view_gate::withheld_report("a1b2c3");
    assert!(notice.contains("a1b2c3") && notice.contains("where the owner is alone"));
}

/// An earlier `Recall` node's sources are admitted again under the current
/// turn's presence: a note admitted when the owner was alone does not ride
/// into an agent's turn at the terminal, nor into a channel bound private
/// (item 8d).
#[tokio::test]
async fn an_earlier_recall_is_admitted_again_under_the_current_turn() {
    let r = rig();
    let dm = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &dm, HIS).await;
    let (_, source) = r.core.store.session_nodes(&dm).unwrap().remove(0);
    let sources = || {
        let mut s = crate::recall::render::Sources::new();
        s.insert(source.id.clone(), std::sync::Arc::new(source.clone()));
        s
    };
    let desk = session(&r.core, None);
    let owner = view_gate::test_mark(&r.core.runner, &desk, true);
    let agent = view_gate::test_mark(&r.core.runner, &desk, false);
    let begin = |m: &crate::exchange::OwnerMark| {
        let place = m.place.clone();
        view_gate::begin(
            &r.core.runner,
            &desk,
            "turn_r",
            place,
            view_gate::Proof::Mark(m),
        );
    };
    begin(&owner);
    let mut s = sources();
    view_gate::readmit_sources(&r.core.runner, &desk, &mut s);
    assert_eq!(s.len(), 1, "the owner alone keeps it");
    begin(&agent);
    let mut s = sources();
    view_gate::readmit_sources(&r.core.runner, &desk, &mut s);
    assert!(s.is_empty(), "an agent's turn does not carry it");
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    let mark = view_gate::test_mark(&r.core.runner, &lab, true);
    let place = mark.place.clone();
    view_gate::begin(
        &r.core.runner,
        &lab,
        "turn_l",
        place,
        view_gate::Proof::Mark(&mark),
    );
    let mut s = sources();
    view_gate::readmit_sources(&r.core.runner, &lab, &mut s);
    assert!(s.is_empty(), "a channel bound private does not carry it");
}

/// A place digest names other threads only where the owner is alone, and an
/// agent's turn in a thread counts none of them (item 8c).
#[tokio::test]
async fn a_place_digest_names_other_threads_only_for_the_owner_alone() {
    let r = rig();
    for slug in ["kelp", "tide"] {
        r.core
            .thread_new(
                &ThreadNewParams {
                    slug: slug.into(),
                    title: Some(slug.into()),
                    ..Default::default()
                },
                cli(),
            )
            .unwrap();
    }
    let kelp = r.core.thread_session("kelp", cli()).unwrap().session_id;
    let board = r.core.runner.threads.all(&r.core.store).unwrap();
    let live = || board.live(crate::places::PlaceClass::Private);
    let slugs = |v: Vec<&theseus_protocol::threads::ThreadInfo>| {
        let mut s: Vec<String> = v.into_iter().map(|t| t.slug.clone()).collect();
        s.sort();
        s
    };
    for (in_person, want) in [(true, vec!["kelp", "tide"]), (false, vec!["kelp"])] {
        let mark = view_gate::test_mark(&r.core.runner, &kelp, in_person);
        let place = mark.place.clone();
        view_gate::begin(
            &r.core.runner,
            &kelp,
            "turn_d",
            place,
            view_gate::Proof::Mark(&mark),
        );
        let got = slugs(view_gate::digest_threads(
            &r.core.runner,
            &kelp,
            "kelp",
            live(),
        ));
        assert_eq!(got, want, "in person: {in_person}");
    }
    // A session with no computed presence names none but its own.
    let quiet = r.core.thread_session("tide", cli()).unwrap().session_id;
    let got = slugs(view_gate::digest_threads(
        &r.core.runner,
        &quiet,
        "tide",
        live(),
    ));
    assert_eq!(got, ["tide"]);
}

/// A standing record kept from a message of another conversation is held
/// from a turn the gate does not admit that conversation for: an agent's at
/// the terminal, a channel bound private; the owner alone is shown it, and a
/// record from no conversation is shown to every turn (item 8b).
#[tokio::test]
async fn a_standing_record_keeps_its_source_place() {
    let r = rig();
    let dm = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &dm, HIS).await;
    let (_, said) = r.core.store.session_nodes(&dm).unwrap().remove(0);
    let source = format!("capture:{}", said.id);
    let records = [("std_dm", source.as_str()), ("std_cli", "the CLI")];
    let held = |sid: &str| {
        let mut h: Vec<String> =
            view_gate::standing_held(&r.core.runner, sid, records.iter().copied())
                .into_iter()
                .collect();
        h.sort();
        h
    };
    let desk = session(&r.core, None);
    let seat = |sid: &str, in_person: bool| {
        let m = view_gate::test_mark(&r.core.runner, sid, in_person);
        let place = m.place.clone();
        view_gate::begin(
            &r.core.runner,
            sid,
            "turn_st",
            place,
            view_gate::Proof::Mark(&m),
        );
    };
    seat(&desk, true);
    assert!(held(&desk).is_empty());
    seat(&desk, false);
    assert_eq!(held(&desk), ["std_dm"]);
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    seat(&lab, true);
    assert_eq!(held(&lab), ["std_dm"]);
    // Where it was said, it is its own.
    seat(&dm, false);
    assert!(held(&dm).is_empty());
}

/// The ledger's F1, through the gate: an owner's submission that is refused
/// before it runs leaves nothing behind, so an agent's identical text after
/// it computes a narrow presence (the mark rides its own submission).
#[tokio::test]
async fn a_refused_owners_submission_lends_nothing_to_an_agents_identical_text() {
    let r = rig();
    let desk = session(&r.core, None);
    let text = "look up the harbour gate in my other conversations";
    // The owner's submission, marked, refused before its turn ran: its sink
    // is dropped with its mark.
    let owner = view_gate::test_mark(&r.core.runner, &desk, true);
    drop(crate::bus::EventSink::new(r.core.bus.clone(), &desk, None).with_arrival(Some(owner)));
    turn_as(&r.core, &desk, text, false).await;
    let a = r.core.runner.presence.get(&desk).unwrap();
    assert_eq!(a.present(), Present::PrivateChannel);
}
