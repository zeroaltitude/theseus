//! No self build starts while main is red (issue 6, condition 3): the record's reading over the
//! shapes a real log has, and the pipeline's hold over a rig whose main's gate says red, then green.

use std::sync::{Arc, Mutex};

use super::main_red::{red_in, Main, Red};
use super::tests::rig_with;
use super::Seams;
use crate::rsi::tree::Place;

/// A log as the gate writes it: branch and describe in the label, the status as `ok` or `failed`,
/// a lane's gate and a dirty tree's among main's, a detached merge's `HEAD`, and several commits.
const LOG: &str = "time,label,phase,seconds,status\n\
    2026-11-13T01:00:00+00:00,\"main release-1.4-3-g1a2b3c4\",total,120,ok\n\
    2026-11-13T02:00:00+00:00,\"main release-1.4-4-g2b3c4d5\",suite,90,failed\n\
    2026-11-13T02:10:00+00:00,\"main release-1.4-4-g2b3c4d5\",total,120,failed\n\
    2026-11-13T02:30:00+00:00,\"lane/x release-1.4-9-g9999999\",total,100,ok\n\
    2026-11-13T02:40:00+00:00,\"main release-1.4-4-g2b3c4d5-dirty\",total,100,ok\n";

const HEAD: &str = "2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e";

/// KEY: the newest whole gate of main's head decides; a dirty tree's, a phase's and a lane's do
/// not; `ok` is a pass and `failed` is a failure, as the log writes them.
#[test]
fn the_newest_whole_gate_of_mains_head_decides() {
    let red = red_in(LOG, HEAD).expect("main's head failed its whole gate");
    assert_eq!(red.commit, "2b3c4d5");
    assert_eq!(
        red.at_ms,
        crate::wake::parse_at("2026-11-13T02:10:00+00:00").unwrap()
    );
    // A later green gate of the same commit lifts it, whatever branch it ran on: a self-join's
    // gate of a detached merge is labelled `HEAD`.
    let green =
        format!("{LOG}2026-11-13T03:00:00+00:00,\"HEAD release-1.4-4-g2b3c4d5\",total,110,ok\n");
    assert_eq!(red_in(&green, HEAD), None);
}

/// A real log's trap (the first review's F2): a red row of an old commit says nothing once main
/// has moved on, and `ok` is never read as a failure.
#[test]
fn a_red_row_of_an_old_commit_says_nothing_once_main_moved_on() {
    let moved = "3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f";
    assert_eq!(
        red_in(LOG, moved),
        None,
        "no row of that commit: nothing known"
    );
    // Only greens: nothing is red.
    let greens = "time,label,phase,seconds,status\n\
        2026-11-13T01:00:00+00:00,\"main g2b3c4d5\",total,120,ok\n\
        2026-11-13T02:00:00+00:00,\"main g2b3c4d5\",total,120,0\n";
    assert_eq!(red_in(greens, HEAD), None);
    // A numeric failure, as the auto-revert's tests write it, is read too; any other word is not.
    let numeric = "time,label,phase,seconds,status\n\
        2026-11-13T01:00:00+00:00,\"main g2b3c4d5\",total,120,1\n";
    assert!(red_in(numeric, HEAD).is_some());
    let odd = "time,label,phase,seconds,status\n\
        2026-11-13T01:00:00+00:00,\"main g2b3c4d5\",total,120,skipped\n";
    assert_eq!(red_in(odd, HEAD), None);
    // Out of order in the file, the time decides, not the line.
    let shuffled = "time,label,phase,seconds,status\n\
        2026-11-13T05:00:00+00:00,\"main g2b3c4d5\",total,100,ok\n\
        2026-11-13T02:00:00+00:00,\"main g2b3c4d5\",total,100,failed\n";
    assert_eq!(red_in(shuffled, HEAD), None);
    // No record, or one that cannot be read, is nothing known.
    assert_eq!(red_in("", HEAD), None);
    assert_eq!(red_in("not,a,gate,log\n", HEAD), None);
}

#[test]
fn the_hold_says_what_main_did_and_what_lifts_it() {
    let why = red_in(LOG, HEAD).unwrap().why();
    assert!(why.contains("main's last gate was red at 2b3c4d5"), "{why}");
    assert!(
        why.contains("until a later gate of main is green or main moves on"),
        "{why}"
    );
}

/// A main whose gate the test sets.
struct Stand(Mutex<Option<Red>>);

impl Main for Stand {
    fn red(&self, _: &Place) -> Option<Red> {
        self.0.lock().unwrap().clone()
    }
}

/// KEY: with main red nothing starts, the hold is said once and shows in the view; the first
/// tick after a green gate starts the top item.
#[tokio::test]
async fn no_self_build_starts_while_main_is_red() {
    let r = rig_with(2, 30.0);
    let main = Arc::new(Stand(Mutex::new(Some(Red {
        commit: "2b3c4d5".into(),
        at_ms: crate::wake::parse_at("2026-11-13T02:00:00+00:00").unwrap(),
    }))));
    let seams = Seams {
        main: main.clone(),
        ..r.seams()
    };
    for _ in 0..2 {
        let t = r.core.pipeline_tick(&seams).await.unwrap();
        assert!(t.started.is_empty(), "{t:?}");
        assert_eq!(t.hold.as_deref(), Some("main_red"));
    }
    assert!(r.fake.starts.lock().unwrap().is_empty());
    let held = r.rows("self.pipeline.held");
    assert_eq!(held.len(), 1, "said once, however often it ticks: {held:?}");
    assert!(held[0]["why"]
        .as_str()
        .unwrap()
        .contains("main's last gate was red"));
    let view = r
        .core
        .self_pipeline_with(&Default::default(), &seams)
        .unwrap();
    assert_eq!(view.room.hold.as_deref(), Some("main_red"));
    // A green gate of main: the next tick starts.
    *main.0.lock().unwrap() = None;
    let t = r.core.pipeline_tick(&seams).await.unwrap();
    assert_eq!(t.started.len(), 2, "{t:?}");
    assert_ne!(t.hold.as_deref(), Some("main_red"), "{t:?}");
}

/// The real reader, over a real repository: the log's red row is main's only while its commit is
/// the repo's `origin/main`.
#[tokio::test]
async fn the_recorded_reader_judges_by_the_repos_main() {
    let r = rig_with(1, 30.0);
    let place = &r.f.place;
    let head = place
        .git_ok(&place.repo, &["rev-parse", "refs/remotes/origin/main"])
        .unwrap();
    let log = crate::scratch::tempdir();
    let path = log.path().join("gate-times.csv");
    let short = &head[..12];
    std::fs::write(
        &path,
        format!(
            "time,label,phase,seconds,status\n\
             2026-11-13T02:10:00+00:00,\"main v1-4-g{short}\",total,120,failed\n\
             2026-11-13T02:10:00+00:00,\"main v1-4-gabcdef0\",total,120,failed\n"
        ),
    )
    .unwrap();
    let recorded = super::main_red::Recorded {
        gate_times: path.clone(),
    };
    let red = recorded
        .red(place)
        .expect("the repo's main failed its gate");
    assert_eq!(red.commit, short);
    // The same log with the red row at another commit: main is not red.
    std::fs::write(
        &path,
        "time,label,phase,seconds,status\n\
         2026-11-13T02:10:00+00:00,\"main v1-4-gabcdef0\",total,120,failed\n",
    )
    .unwrap();
    assert_eq!(recorded.red(place), None);
}
