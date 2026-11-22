//! The gate (issue 44) through the whole core, after "anything goes" (2026-11-22): who is present withholds
//! nothing now, and these are what stays of it. The owner's mark as the store reads it, and the reading of
//! it that a task's objective change requires (`task_graph::words::present_in_person`: authority, not
//! privacy); a refused submission lends its mark to nothing; and the registry of decisions by place.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use serde_json::json;
use theseus_memory::gate::Present;
use theseus_memory::recall::Place;

use crate::places::BoundPlace;
use crate::tests_memory_lookup::{index_over, script};
use crate::tests_places::{rig_scripted, session, turn, Rig, LAB, OWNER};
use crate::view_gate::{self, OwnerMarks};
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

/// The owner alone is the terminal, the web page or a direct conversation, and his in-person mark: the
/// reading a task's objective change requires (`task_graph::words::present_in_person`, `Why::NotAlone`).
#[test]
fn owner_alone_is_the_terminal_the_page_or_a_dm_and_his_mark() {
    use crate::task_graph::words::present_in_person;
    let f = fake();
    let dm = "discord:dm:7";
    let lab = format!("discord:channel:{LAB}");
    let p = |place: &Place, target: Option<&str>, node| present_in_person(place, target, node, &f);
    assert_eq!(p(&Place::Private, None, "his"), Present::OwnerAlone);
    assert_eq!(p(&Place::Private, Some(dm), "his"), Present::OwnerAlone);
    // An agent's process at the terminal is not alone; nor is a message with
    // no mark, nor one whose mark cannot be read.
    assert_eq!(p(&Place::Private, None, "agent"), Present::PrivateChannel);
    assert_eq!(
        p(&Place::Private, None, "unmarked"),
        Present::PrivateChannel
    );
    assert_eq!(p(&Place::Private, None, "broken"), Present::Unknown);
    // A channel is never alone, whoever is in it.
    assert_eq!(
        p(&Place::Private, Some(&lab), "his"),
        Present::PrivateChannel
    );
    assert_eq!(
        p(&Place::Shared(lab.clone()), Some(&lab), "his"),
        Present::Shared
    );
    assert_eq!(p(&Place::Unknown, None, "his"), Present::Unknown);
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
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

/// With `[context] ledger` off the terminal reads places as before, agent or not.
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
    let got = crate::tests_places::result_of(&r.core, &agent, "m1");
    assert!(got.contains(HIS), "as before the ledger: {got}");
}

/// The ledger's F1: an owner's submission that is refused before it runs leaves nothing behind, so an
/// agent's identical text after it carries the agent's own mark, and is not read as the owner alone.
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
    let node = (r.core.store.session_nodes(&desk).unwrap().into_iter())
        .find_map(|(_, n)| match &n.body {
            crate::node::Body::UserMessage { text: t, .. } if t == text => Some(n.id.clone()),
            _ => None,
        })
        .expect("the agent's message");
    assert_eq!(r.core.store.in_person(&node).unwrap(), Some(false));
    let place = r.core.runner.place_of(&desk);
    assert_eq!(
        crate::task_graph::words::present_in_person(&place, None, &node, &r.core.store),
        Present::PrivateChannel
    );
}
