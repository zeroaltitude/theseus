//! An edit's question and its finished call (theseus-z6au): the card shows the
//! call in words and the hunk the daemon sent, numbered; the changed words of
//! a paired line are drawn in a style of their own; a finished edit's diff and
//! a failed command's last lines stay in the conversation.

use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use serde_json::json;
use theseus_client::render::diffview::{MARK_OFF, MARK_ON};
use theseus_client::render::Tag;
use theseus_protocol::ConfirmPreview;

use crate::tests::*;
use crate::ui;

const HUNK: &str = "--- tides.py\n+++ tides.py\n@@ -48,3 +48,3 @@\n     highest = max(heights)\n-    return lowest - highest\n+    return highest - lowest\n \n";

#[tokio::test]
async fn an_edits_card_shows_the_call_in_words_and_its_hunk() {
    let world = harbour_world();
    {
        let mut w = world.lock().unwrap();
        let mut q = gate_question("ses_spec01", "cor_spec01", 98, 98 + 252);
        q.tool = "fs.edit".into();
        q.input = json!({"path": "tides.py", "old_string": "lowest - highest", "new_string": "highest - lowest"});
        q.preview = Some(ConfirmPreview {
            kind: "diff".into(),
            body: HUNK.into(),
        });
        w.board["confirms"] = json!([q]);
    }
    NOW.with(|n| n.set(T0 + 98_000));
    let mut rig = Rig::new(120, 30, script(world));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab]).await;
    rig.shows("tides.py:49").await;
    let screen = rig.screen().join("\n");
    assert!(
        screen.contains("⏸ confirm fs.edit: Edit tides.py"),
        "{screen}"
    );
    assert!(
        screen.contains("49 -     return lowest - highest"),
        "{screen}"
    );
    assert!(
        screen.contains("49 +     return highest - lowest"),
        "{screen}"
    );
    assert!(
        !screen.contains(MARK_ON) && !screen.contains(MARK_OFF),
        "marks are drawn, never shown"
    );
}

/// The changed words of a −/+ pair are drawn on a background of their own.
#[test]
fn the_changed_words_are_drawn_on_their_own_background() {
    use theseus_client::render::diffview::diff_lines;
    let lines = diff_lines(HUNK, "");
    let del = lines.iter().find(|l| l.tag == Tag::Del).unwrap();
    let area = Rect::new(0, 0, 60, 1);
    let mut buf = Buffer::empty(area);
    ui::put(&mut buf, 0, 0, &del.text, 60, del.tag);
    let row: String = (0..60).map(|x| buf[(x, 0)].symbol().to_string()).collect();
    let at = row.find("lowest").unwrap() as u16;
    assert_eq!(buf[(at, 0)].bg, Color::Red, "{row}");
    assert_ne!(buf[(2, 0)].bg, Color::Red);
    assert_eq!(buf[(2, 0)].fg, Color::Red, "the line itself is red");
}

/// A wrap that cuts a marked word keeps its mark on the next row.
#[test]
fn a_wrap_keeps_a_marked_word_marked_on_both_rows() {
    let text = format!("abc {MARK_ON}def ghi jkl{MARK_OFF} mno");
    let rows = ui::wrap(&text, 9);
    assert!(rows.len() >= 2);
    for r in &rows {
        let on = r.matches(MARK_ON).count();
        let off = r.matches(MARK_OFF).count();
        assert_eq!(on, off, "every row closes what it opens: {rows:?}");
    }
}

/// The conversation names each call in words, keeps a failed command's last
/// lines, and keeps an edit's diff after the approval.
#[test]
fn the_conversation_keeps_the_words_the_failure_and_the_diff() {
    use crate::detail::Detail;
    use theseus_protocol::{Event, ToolDiff, ToolEnded, ToolProposed};
    let mut d = Detail::new("ses_a");
    let proposed = |id: &str, tool: &str, input| {
        Event::ToolProposed(ToolProposed {
            session_id: "ses_a".into(),
            tool_use_id: id.into(),
            tool: tool.into(),
            input,
            ..ToolProposed::default()
        })
    };
    d.event(&proposed(
        "t1",
        "proc.run",
        json!({"argv": ["python3", "-m", "unittest", "-v"]}),
    ));
    d.event(&Event::ToolEnded(ToolEnded {
        session_id: "ses_a".into(),
        tool_use_id: "t1".into(),
        tool: "proc.run".into(),
        status: "ok".into(),
        exit_code: Some(1),
        preview: "[exit code 1 · in /app]\ntest_tidal_range ... FAIL\n\nFAIL: test_tidal_range\nAssertionError: -2.8 != 2.8\n".into(),
        ..ToolEnded::default()
    }));
    d.event(&proposed(
        "t2",
        "fs.edit",
        json!({"path": "tides.py", "old_string": "a", "new_string": "b"}),
    ));
    d.event(&Event::ToolEnded(ToolEnded {
        session_id: "ses_a".into(),
        tool_use_id: "t2".into(),
        tool: "fs.edit".into(),
        status: "ok".into(),
        diff: Some(ToolDiff {
            path: "tides.py".into(),
            added: 1,
            removed: 1,
            unified: HUNK.into(),
            truncated: false,
        }),
        ..ToolEnded::default()
    }));
    let text: Vec<String> = d.lines().iter().map(|(_, t)| t.clone()).collect();
    let all = text.join("\n");
    assert!(all.contains("→ Run `python3 -m unittest -v`"), "{all}");
    assert!(
        all.contains("AssertionError: -2.8 != 2.8") && all.contains("FAIL: test_tidal_range"),
        "{all}"
    );
    assert!(
        all.contains("→ Edit tides.py") && !all.contains("→ fs.edit"),
        "{all}"
    );
    assert!(all.contains("49 - ") && all.contains("49 + "), "{all}");
    let tags: Vec<Tag> = d.lines().iter().map(|(t, _)| *t).collect();
    assert!(tags.contains(&Tag::Del) && tags.contains(&Tag::Add));
}
