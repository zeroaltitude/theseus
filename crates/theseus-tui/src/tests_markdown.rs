//! A reply in the detail pane is drawn as markdown with the CLI's renderer
//! (theseus-rn7s): the heading, the emphasis and the list are drawn, not
//! printed with their marks, as a reply streams in and as a history shows it.

use ratatui::style::Modifier;
use serde_json::json;

use crate::tests::{harbour_rig, open_dm, pane};

#[tokio::test]
async fn a_streamed_reply_is_drawn_not_printed_with_its_marks() {
    let mut rig = harbour_rig(120, 24);
    open_dm(&mut rig).await;
    let d = rig.daemon();
    d.notify(
        "turn.started",
        json!({"session_id": "ses_dm0001", "turn_id": "turn_b2", "execution_id": "exe_ses_dm0001"}),
    );
    for piece in [
        "# Port Wenlow",
        " tides\n\nA **small",
        " library**, with `code`.\n\n- one *unit",
        " test*\n- two\n",
    ] {
        d.notify(
            "model.delta",
            json!({"turn_id": "turn_b2", "loop_index": 0, "text": piece}),
        );
    }
    rig.shows("• two").await;
    let rows = pane(&rig.screen(), 120);
    let text = rows.join("\n");
    assert!(text.contains("\n Port Wenlow tides\n"), "{text}");
    assert!(text.contains("\n • one unit test\n • two"), "{text}");
    assert!(!text.contains("**") && !text.contains("# Port"), "{text}");
    // The words are drawn in their styles: bold for the heading and for **small library**.
    let buf = rig.runner.term.backend().buffer().clone();
    let side = crate::ui::sidebar_width(120);
    let mut found = (false, false);
    for y in 0..buf.area.height {
        let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
        for (word, flag) in [
            ("Port Wenlow tides", &mut found.0),
            ("small library", &mut found.1),
        ] {
            if let Some(at) = row.find(word) {
                let col = row[..at].chars().count() as u16;
                *flag |= col > side && buf[(col, y)].modifier.contains(Modifier::BOLD);
            }
        }
    }
    assert_eq!(found, (true, true), "{text}");
}
