//! A wake's or a watch's note in the line its fact narrates stops before a
//! value the live board knows, as its title is cut: a wake that came due, a
//! reminder that fired, and a watch that ended. All values invented.

use theseus_kernel::{FiredWake, PendingWake};

use super::{Fact, Rec, Say, To};
use crate::narrative::Narrator;
use crate::secrets::{Secret, SecretBoard};
use crate::tests_cut_sites::{whole_or_absent, V};

/// 50 characters, then V across a title's cut at 60.
fn note() -> String {
    format!("{}{V} and after", "n".repeat(50))
}

fn fired() -> FiredWake {
    FiredWake {
        wake: PendingWake {
            id: "wak_0000c0ffee".into(),
            due_at_ms: 5_000,
            note: note(),
            set_at_ms: 1,
            by: "act_x".into(),
            ..Default::default()
        },
        late_ms: 0,
        while_down: false,
        missed: 0,
        next_due_at_ms: None,
    }
}

/// The lines `f` narrates, joined.
fn narrated(f: &impl Fact) -> String {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let narrator = Narrator::new(true);
    let rec = Rec {
        narrator: &narrator,
        session: Some("ses_a"),
        turn: None,
        to: To::Nobody,
        store: &store,
    };
    f.narrate(&mut Say { rec: &rec });
    let lines: Vec<String> = narrator.tail().into_iter().map(|l| l.text).collect();
    assert!(!lines.is_empty(), "the fact narrates a line");
    lines.join("\n")
}

#[test]
fn a_wakes_note_in_its_narrated_line_stops_before_a_known_value() {
    let board = SecretBoard::empty();
    board.hold("cut_test", Secret::new(V.into()));
    let titled = format!("\"{}…\"", "n".repeat(50));
    let f = fired();
    let due = narrated(&super::turn::WakeCameDue { fired: &f });
    let reminder = narrated(&super::wake::WakeFired {
        fired: &f,
        session_id: "ses_a",
        execution_id: "exe_a",
        posted_to: None,
        home: false,
    });
    let text = note();
    let watch = narrated(&super::watch::WatchEnded {
        ending: super::watch::Ending::Met,
        wake_id: "wak_0000c0ffee",
        session_id: "ses_a",
        execution_id: "exe_a",
        text: &text,
        at_ms: 5_000,
        posted_to: None,
        home: false,
    });
    for (what, line) in [("due", &due), ("reminder", &reminder)] {
        assert!(line.contains(&titled), "{what}: {line}");
        whole_or_absent(what, line, V);
    }
    whole_or_absent("watch", &watch, V);
}
