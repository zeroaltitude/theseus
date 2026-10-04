//! Step 38a's ceilings through the whole core (theseus-ext.3): a place's
//! floor makes a call wait, its tools narrow what the model is offered and
//! the gate refuses the rest in words naming the ceiling, it never widens a
//! shared place's set, a task inherits its parent's, its spend limit is the
//! lower of its own and the config's, and its profile is the turn's when the
//! turn names none. The rig is the place rule's (`tests_places`).

use theseus_protocol::{PlaceCeiling, PlaceClass};

use crate::places::BoundPlace;
use crate::session::SessionRecord;
use crate::tests_places::{
    answers_a_call, offered, refused, result_of, rig, rows, session, system_text, turn, LAB, OWNER,
    SECRET, SHARED_TOOLS,
};

/// A guild channel bound shared, with a ceiling.
const PIER: u64 = 141_421_356_237_309_504;

fn bind(core: &crate::Core, lab: Option<PlaceCeiling>, pier: Option<PlaceCeiling>) {
    core.bind_places(vec![
        BoundPlace {
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: true,
            guild: Some("100000000000000001".into()),
            ceiling: lab,
        },
        BoundPlace {
            target: format!("discord:channel:{PIER}"),
            name: "#pier".into(),
            private: false,
            guild: Some("100000000000000002".into()),
            ceiling: pier,
        },
        BoundPlace {
            target: format!("discord:dm:{OWNER}"),
            name: "DM @owner".into(),
            ..Default::default()
        },
    ]);
}

fn tools(t: &[&str]) -> Option<PlaceCeiling> {
    Some(PlaceCeiling {
        tools: Some(t.iter().map(|t| t.to_string()).collect()),
        ..Default::default()
    })
}

fn floor(f: &str) -> Option<PlaceCeiling> {
    Some(PlaceCeiling {
        posture_floor: Some(f.into()),
        ..Default::default()
    })
}

/// The calls of `sid` that wait for approval, with the card's reason.
fn waiting(core: &crate::Core, sid: &str) -> Vec<String> {
    rows(core, "tool.confirm_requested")
        .into_iter()
        .filter(|r| r["session_id"] == sid)
        .map(|r| r["reason"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// A floor of `approve` makes a call in its place wait, though the config
/// runs it at notify, and its card says why; the owner's DM, with no
/// ceiling, runs the same calls at once with a notice. The tools note says
/// the floor, and the postures it lists are the floor's.
#[tokio::test]
async fn a_floor_makes_a_notify_tool_wait() {
    let r = rig();
    bind(&r.core, floor("approve"), None);
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    turn(&r.core, &lab, "look around").await;
    let asked = waiting(&r.core, &lab);
    assert_eq!(
        asked.len(),
        1,
        "the first call waits, and the turn with it: {asked:?}"
    );
    assert!(
        asked[0].contains("proc.run — approve (#lab's ceiling sets a floor of approve)"),
        "{}",
        asked[0]
    );
    let system = system_text(&r.requests()[0]);
    assert!(
        system.contains("no call here runs looser than approve"),
        "{system}"
    );
    assert!(
        !system.contains("notify: "),
        "every tool is listed at approve: {system}"
    );

    let dm = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &dm, "look around").await;
    assert!(waiting(&r.core, &dm).is_empty(), "the DM has no floor");
    let notified = rows(&r.core, "tool.notified");
    assert!(
        notified.iter().any(|n| n["session_id"] == dm.as_str()),
        "{notified:?}"
    );
    // A floor looser than the config's posture changes nothing.
    bind(&r.core, floor("open"), None);
    let open = session(&r.core, Some(&format!("channel:{LAB}")));
    turn(&r.core, &open, "look around").await;
    assert!(waiting(&r.core, &open).is_empty());
}

/// A private place whose ceiling lists `fs` is offered the file tools alone,
/// and its program is refused, never run, in words naming the ceiling; its
/// reads run.
#[tokio::test]
async fn a_tool_outside_the_ceiling_is_not_offered_and_a_call_naming_it_is_refused() {
    let r = rig();
    bind(&r.core, tools(&["fs"]), None);
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    let res = turn(&r.core, &lab, "look around").await;
    assert_eq!(res.loops, 2, "{res:?}");
    let req = &r.requests()[0];
    let names = offered(req);
    assert!(names.iter().all(|n| n.starts_with("fs_")), "{names:?}");
    assert!(names.contains(&"fs_read".to_string()));
    let system = system_text(req);
    assert!(
        system.contains("only these tool families here: fs"),
        "{system}"
    );
    let program = result_of(&r.core, &lab, "p1");
    assert!(
        program.starts_with(
            "Not run: proc.run is not offered in #lab: its ceiling in the bindings file offers only fs"
        ),
        "{program}"
    );
    assert!(
        result_of(&r.core, &lab, "p2").contains(SECRET),
        "a private read runs"
    );
    let refused = refused(&r.core);
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(refused[0]
        .1
        .starts_with("place: proc.run is not offered in #lab"));
}

/// A shared place's ceiling that lists the private families offers it
/// nothing more than the place rule does, and its program is still refused
/// as a shared place's; one that lists `web` alone narrows it to the search,
/// and its wake is refused in the ceiling's words.
#[tokio::test]
async fn a_ceiling_never_offers_a_shared_place_a_private_tool() {
    let r = rig();
    let every = [
        "fs", "git", "text", "proc", "aws", "web", "http", "wake", "task",
    ];
    bind(&r.core, None, tools(&every));
    let pier = session(&r.core, Some(&format!("channel:{PIER}")));
    assert_eq!(r.core.runner.class_of(&pier), PlaceClass::Shared);
    turn(&r.core, &pier, "look around").await;
    assert_eq!(
        offered(&r.requests()[0]),
        SHARED_TOOLS,
        "the place rule's set"
    );
    let program = result_of(&r.core, &pier, "p1");
    assert!(
        program.starts_with("Not run: proc.run is not offered in a shared place"),
        "{program}"
    );

    bind(&r.core, None, tools(&["web"]));
    let narrow = session(&r.core, Some(&format!("channel:{PIER}")));
    let before = r.requests().len();
    turn(&r.core, &narrow, "WAKE in a second").await;
    assert_eq!(offered(&r.requests()[before]), ["web_search"]);
    let wake = result_of(&r.core, &narrow, "w1");
    assert!(
        wake.starts_with(
            "Not run: wake.at is not offered in #pier: its ceiling in the bindings file offers only web"
        ),
        "{wake}"
    );
    assert!(rows(&r.core, "wake.set").is_empty(), "no wake was set");
}

/// A task speaks in its parent's place, so it has its parent's ceiling: in
/// `#lab`, whose ceiling offers `fs` and `task`, the task is offered no
/// program, and its call to one is refused in `#lab`'s words.
#[tokio::test]
async fn a_task_inherits_its_parents_ceiling() {
    let r = rig();
    bind(&r.core, tools(&["fs", "task"]), None);
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    turn(&r.core, &lab, "start a task").await;
    r.until("the task's answer", 15, |r| {
        r.asked("count the files").iter().any(answers_a_call)
    })
    .await;
    let task = r.asked("count the files");
    assert!(!offered(&task[0]).contains(&"proc_run".to_string()));
    let refused = refused(&r.core);
    assert!(
        refused.iter().any(
            |(id, why)| id == "k1" && why.starts_with("place: proc.run is not offered in #lab")
        ),
        "the task's program was refused by the ceiling: {refused:?}"
    );
}

/// The spend limit of a place's session is the lower of its ceiling's and
/// the config's, and follows either: a higher ceiling leaves the config's,
/// and none returns the session to the config's, followed again. Each change
/// is one `budget.limit_changed` row that says the place chose it.
#[tokio::test]
async fn the_spend_limit_is_the_lower_of_the_two_and_follows_changes() {
    let r = rig();
    let pier = session(&r.core, Some(&format!("channel:{PIER}")));
    turn(&r.core, &pier, "hello").await;
    let exec = || {
        let s: SessionRecord = r.core.store.get_session(&pier).unwrap().unwrap();
        let id = s.execution_id.unwrap();
        r.core.kernel.execution(&id).unwrap().unwrap().budget
    };
    let config = r.core.kernel.config().spend_limit_micros;
    assert_eq!((exec().limit_micros, exec().pinned), (config, false));

    r.core.place_spend(&pier, "#pier", Some(1.0));
    assert_eq!((exec().limit_micros, exec().pinned), (1_000_000, true));
    r.core.place_spend(&pier, "#pier", Some(1.0));
    let changed = rows(&r.core, "budget.limit_changed");
    assert_eq!(
        changed.len(),
        1,
        "the same cap again writes nothing: {changed:?}"
    );
    assert_eq!(changed[0]["why"], "place");
    assert_eq!(changed[0]["to_usd"], 1.0);

    // A ceiling above the config's: the config's is the lower.
    r.core.place_spend(&pier, "#pier", Some(1_000_000.0));
    assert_eq!((exec().limit_micros, exec().pinned), (config, true));
    // No ceiling: the config's, followed as any session's is.
    r.core.place_spend(&pier, "#pier", None);
    assert_eq!((exec().limit_micros, exec().pinned), (config, false));
    assert_eq!(rows(&r.core, "budget.limit_changed").len(), 2);
}

/// A place's profile is a turn's when the turn names none.
#[tokio::test]
async fn a_places_profile_is_the_turns_unless_it_names_one() {
    let r = rig();
    let profile = Some(PlaceCeiling {
        profile: Some("deep".into()),
        ..Default::default()
    });
    bind(&r.core, profile, None);
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    let pier = session(&r.core, Some(&format!("channel:{PIER}")));
    assert_eq!(r.core.place_profile(&lab, "live".into()), "deep");
    assert_eq!(r.core.place_profile(&pier, "live".into()), "live");
    assert_eq!(r.core.place_profile("ses_nowhere", "live".into()), "live");
}

/// Health's places carry each one's guild and ceiling.
#[tokio::test]
async fn health_shows_each_places_guild_and_ceiling() {
    let r = rig();
    bind(&r.core, floor("approve"), tools(&["web"]));
    let h = r.core.health().places.unwrap();
    let got: Vec<(String, Option<String>, Option<PlaceCeiling>)> = h
        .places
        .into_iter()
        .map(|p| (p.name, p.guild, p.ceiling))
        .collect();
    assert_eq!(
        got[2],
        (
            "#lab".into(),
            Some("100000000000000001".into()),
            floor("approve")
        )
    );
    assert_eq!(
        got[3],
        (
            "#pier".into(),
            Some("100000000000000002".into()),
            tools(&["web"])
        )
    );
    assert_eq!(got[4], ("DM @owner".into(), None, None));
}
