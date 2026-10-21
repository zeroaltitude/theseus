//! The board's rough edges (theseus-ikw4p): where `N` puts a task, outside text drawn cell for cell, the one-turn
//! conversations folded, and a dismissed failure never read as done.

use crossterm::event::KeyCode;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};
use serde_json::json;
use theseus_protocol::SessionKind;

use crate::tests::{harbour_world, info, open_dm, replace_view, script, task, view, Rig, T0};
use crate::ui;

/// What crossterm wrote, kept to read back.
#[derive(Clone, Default)]
struct Wire(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Wire {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// `N` says where the task goes: Home with no conversation under the cursor, the conversation's title with one.
#[tokio::test]
async fn n_says_where_the_task_goes() {
    let mut rig = Rig::new(100, 24, script(harbour_world()));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Char('N')]).await;
    rig.shows("new task under Home: the brief (enter starts it)>")
        .await;
    rig.press(&[KeyCode::Esc]).await;
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('N')]).await;
    rig.shows("new task under DM: the brief (enter starts it)>")
        .await;
}

/// A filter puts the cursor on the first row that matches, so `N` after it goes under that conversation.
#[tokio::test]
async fn a_filter_puts_the_cursor_on_its_first_row() {
    let mut rig = Rig::new(100, 24, script(harbour_world()));
    rig.shows("spec rev").await;
    rig.press(&[KeyCode::Char('/')]).await;
    rig.type_text("spec").await;
    rig.press(&[KeyCode::Enter]).await;
    assert_eq!(rig.runner.app.selected.as_deref(), Some("ses_spec01"));
    rig.press(&[KeyCode::Char('N')]).await;
    rig.shows("new task under spec review: the brief").await;
}

/// Read the board back through crossterm's bytes into a terminal emulator, as tmux draws it: each row is what the
/// app meant, none shifted by a glyph the two count differently (`⚙️` is U+2699 U+FE0F).
#[tokio::test]
async fn outside_text_draws_cell_for_cell() {
    let world = harbour_world();
    world.lock().unwrap().titles.insert(
        "ses_spec01".into(),
        serde_json::to_value(info(
            "ses_spec01",
            SessionKind::Conversation,
            None,
            Some("⚙️ a1b2c3 · scan the quay for the install checks"),
        ))
        .unwrap(),
    );
    let mut rig = Rig::new(180, 50, script(world));
    rig.shows("scan the quay").await;
    let meant = rig.screen();
    // The width a terminal draws `⚙️` at is its own: tmux counts the glyph one cell and ratatui two, so the
    // text never holds the selector (the emulator below agrees either way; tmux does not).
    assert!(meant.iter().any(|l| l.contains('⚙')), "{meant:#?}");
    assert!(
        meant.iter().all(|l| !l.contains('\u{FE0F}')),
        "a variation selector reached the screen: {meant:#?}"
    );
    assert!(
        meant.iter().any(|l| l.contains("no session open")),
        "{meant:#?}"
    );
    let area = Rect::new(0, 0, 180, 50);
    let wire = Wire::default();
    let mut term = Terminal::with_options(
        CrosstermBackend::new(wire.clone()),
        TerminalOptions {
            viewport: Viewport::Fixed(area),
        },
    )
    .unwrap();
    term.draw(|f| {
        ui::draw(&rig.runner.app, f.buffer_mut());
    })
    .unwrap();
    let mut screen = vt100::Parser::new(50, 180, 0);
    screen.process(&wire.0.lock().unwrap());
    let got: Vec<String> = screen
        .screen()
        .rows(0, 180)
        .map(|r| r.trim_end().to_string())
        .collect();
    assert_eq!(got, meant);
}

/// Idle one-turn conversations last active over an hour ago fold into one row at the foot; a conversation with a
/// question never does, and enter on the fold row opens it.
#[tokio::test]
async fn one_turn_conversations_fold_and_a_question_never_does() {
    let world = harbour_world();
    {
        let mut w = world.lock().unwrap();
        // The harbour's old notes and the spec review (a question waits there) are the same age and turns.
        for sid in ["ses_old001", "ses_spec01"] {
            let t = w.titles.get_mut(sid).unwrap();
            t["turns"] = json!(1);
            t["last_active_ms"] = json!(T0 - 2 * 3_600_000);
        }
        for (n, sid) in ["ses_ok0001", "ses_ok0002", "ses_ok0003"]
            .into_iter()
            .enumerate()
        {
            let mut v =
                crate::tests::conv(60 + n as u64, sid, "waiting", crate::tests::input(), 0.0);
            v.turns = 1;
            replace_view(&mut w.board, &v);
            let mut i = serde_json::to_value(info(
                sid,
                SessionKind::Conversation,
                None,
                Some("Reply with: ok"),
            ))
            .unwrap();
            i["turns"] = json!(1);
            i["last_active_ms"] = json!(T0 - 2 * 3_600_000);
            w.titles.insert(sid.into(), i);
        }
    }
    let mut rig = Rig::new(120, 40, script(world));
    rig.shows("▸ 4 one-turn conversations").await;
    let screen = rig.screen().join("\n");
    assert!(!screen.contains("Reply with"), "{screen}");
    assert!(
        screen.contains("confirm proc.run"),
        "a question never folds: {screen}"
    );
    // Enter on the fold row opens it.
    rig.runner.app.selected = Some(crate::one_turn::FOLD_ID.into());
    rig.press(&[KeyCode::Enter]).await;
    rig.shows("▾ 4 one-turn conversations").await;
    assert!(rig.screen().join("\n").contains("Reply with"));
}

/// A failed task the owner dismissed reads `✗ … dismissed`, never `◆ done`.
#[tokio::test]
async fn a_dismissed_failure_never_reads_done() {
    let world = harbour_world();
    {
        let mut w = world.lock().unwrap();
        let mut v = task(96, "ses_lost01", "ses_dm0001", "failed", 0.2);
        v.dismissed = true;
        replace_view(&mut w.board, &v);
        w.titles.insert(
            "ses_lost01".into(),
            serde_json::to_value(info(
                "ses_lost01",
                SessionKind::Task,
                None,
                Some("chart the reef"),
            ))
            .unwrap(),
        );
    }
    let mut rig = Rig::new(120, 30, script(world));
    rig.shows("dismissed").await;
    let v = view(
        96,
        "ses_lost01",
        SessionKind::Task,
        Some("ses_dm0001"),
        "failed",
        None,
        vec![],
        2,
        0.2,
    );
    let mut dismissed = v;
    dismissed.dismissed = true;
    assert!(!rig.runner.app.is_done(&dismissed));
    let rows = rig.runner.app.rows();
    let row = rows.iter().find(|r| r.session_id == "ses_lost01");
    let line = rig
        .screen()
        .into_iter()
        .find(|l| l.contains("dismissed"))
        .unwrap_or_default();
    assert!(
        row.is_none_or(|r| r.dismissed && !r.done) && !line.contains("◆") && !line.contains("done"),
        "{line}"
    );
}
