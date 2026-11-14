//! No self build starts while main is red (issue 6, condition 3): the record's reading, and
//! the pipeline's hold over a rig whose main's gate says red, then green.

use std::sync::{Arc, Mutex};

use super::main_red::{red_in, Main, Red};
use super::tests::rig_with;
use super::Seams;

const LOG: &str = "time,label,phase,seconds,status\n\
    2026-11-13T01:00:00+00:00,\"main 1.4-3-g1a2b3c4\",total,120,0\n\
    2026-11-13T02:00:00+00:00,\"main 1.4-4-g2b3c4d5\",total,120,1\n\
    2026-11-13T02:30:00+00:00,\"lane/x g9999999-dirty\",total,100,0\n\
    2026-11-13T02:40:00+00:00,\"main 1.4-4-g2b3c4d5\",suite,90,0\n";

/// KEY: the newest whole gate of main decides, and a lane's gate or a phase of main's is not one.
#[test]
fn the_newest_whole_gate_of_main_decides() {
    let red = red_in(LOG).expect("main's last whole gate failed");
    assert_eq!(red.commit, "2b3c4d5");
    assert_eq!(
        red.at_ms,
        crate::wake::parse_at("2026-11-13T02:00:00+00:00").unwrap()
    );
    // A later green gate of main lifts it.
    let green = format!("{LOG}2026-11-13T03:00:00+00:00,\"main 1.4-5-g3c4d5e6\",total,110,0\n");
    assert_eq!(red_in(&green), None);
    // Out of order in the file, the time decides, not the line.
    let shuffled = "time,label,phase,seconds,status\n\
        2026-11-13T05:00:00+00:00,\"main g3c4d5e6\",total,100,0\n\
        2026-11-13T02:00:00+00:00,\"main g2b3c4d5\",total,100,1\n";
    assert_eq!(red_in(shuffled), None);
    // No record, or one that cannot be read, is nothing known.
    assert_eq!(red_in(""), None);
    assert_eq!(red_in("not,a,gate,log\n"), None);
}

#[test]
fn the_hold_says_what_main_did_and_what_lifts_it() {
    let why = red_in(LOG).unwrap().why();
    assert!(why.contains("main's last gate was red at 2b3c4d5"), "{why}");
    assert!(why.contains("until a later gate of main is green"), "{why}");
}

/// A main whose gate the test sets.
struct Stand(Mutex<Option<Red>>);

impl Main for Stand {
    fn red(&self) -> Option<Red> {
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
    assert_eq!(t.hold, None);
}
