//! A paste is one event, never keys (theseus-8hcg): in the input line it is
//! text, sent on enter; anywhere else it opens the input line, and quits,
//! answers, stops, and arms nothing. Bracketed paste is turned on when the
//! loop starts and off on every way out.

use std::sync::Mutex;
use std::time::Duration;

use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::{Backend, ClearType, TestBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Rect, Size};
use ratatui::{Terminal, TerminalOptions, Viewport};
use serde_json::json;
use theseus_protocol::utc_hm;
use tokio::sync::mpsc;

use crate::app::{App, Mode};
use crate::run::{Connector, Runner};
use crate::tests::{
    clock, focused, gate_question, harbour_rig, harbour_world, open_dm, script, Captured, Rig, NOW,
    T0,
};

/// Bracketed paste on, and off.
const PASTE_ON: &str = "\x1b[?2004h";
const PASTE_OFF: &str = "\x1b[?2004l";

/// One paste, handled before this returns.
async fn paste(rig: &mut Rig, text: &str) {
    let n = rig.runner.terminal_events + 1;
    rig.keys.send(TermEvent::Paste(text.to_string())).unwrap();
    rig.until("a paste to be handled", move |r| {
        r.runner.terminal_events >= n
    })
    .await;
}

/// The harbour on a wide screen, the spec review's question on its card.
async fn card_rig() -> Rig {
    let world = harbour_world();
    {
        let mut w = world.lock().unwrap();
        let q = gate_question("ses_spec01", "cor_spec01", 98, 98 + 252);
        w.board["confirms"] = json!([q]);
    }
    NOW.with(|n| n.set(T0 + 98_000));
    let mut rig = Rig::new(120, 20, script(world));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab]).await;
    rig.shows("[y] approve  [n] decline").await;
    rig
}

/// In the input line, a paste is text with its line break kept; the line
/// is not sent until enter, and then as one message. A terminal's `\r` is a
/// line break.
#[tokio::test]
async fn a_paste_in_the_input_line_is_one_message_sent_on_enter() {
    let mut rig = harbour_rig(80, 24);
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('i')]).await;
    paste(&mut rig, "line\r\nquit").await;
    rig.settle().await;
    assert!(
        !rig.runner.quitting(),
        "a pasted line break and q quit nothing"
    );
    assert_eq!(rig.runner.app.mode, Mode::Input);
    assert_eq!(rig.runner.app.input, "line\nquit");
    assert_eq!(rig.screen()[23], " > line↵quit");
    assert!(
        rig.daemon().asked("turn.submit").is_empty(),
        "nothing is sent before enter"
    );
    rig.press(&[KeyCode::Enter]).await;
    rig.asked("turn.submit", 1).await;
    assert_eq!(
        rig.daemon().asked("turn.submit"),
        [json!({"session_id": "ses_dm0001", "input": "line\nquit", "author": "the TUI"})]
    );
    assert!(!rig.runner.quitting());
}

/// On an open card, outside the input line, a paste of the card's keys and
/// the quit key answers nothing, stops nothing, and quits nothing: it opens
/// the input line with the text in it, and the question still waits. Typed,
/// the same keys act as before.
#[tokio::test]
async fn a_paste_on_a_card_answers_nothing_and_quits_nothing() {
    let mut rig = card_rig().await;
    paste(&mut rig, "y\nq\nt\nn\ns\ns").await;
    rig.settle().await;
    let methods = rig.daemon().methods();
    assert!(
        !methods
            .iter()
            .any(|m| m == "action.confirm" || m == "execution.stop" || m == "task.cancel"),
        "a paste answered or stopped: {methods:?}"
    );
    assert!(!rig.runner.quitting(), "a pasted q is not the quit key");
    assert_eq!(rig.runner.app.mode, Mode::Input, "the input line is open");
    assert_eq!(rig.runner.app.input, "y\nq\nt\nn\ns\ns");
    assert!(
        rig.runner
            .app
            .card()
            .is_some_and(|(q, _)| q.correlation_id() == "cor_spec01"),
        "the question still waits"
    );
    assert!(rig.screen().iter().any(|l| l.contains("[y] approve")));
    // Typed keys are kept as they were: esc leaves the line, y approves.
    rig.press(&[KeyCode::Esc, KeyCode::Char('y')]).await;
    rig.asked("action.confirm", 1).await;
    assert_eq!(
        rig.daemon().asked("action.confirm"),
        [json!({"correlation_id": "cor_spec01", "approve": true, "author": "the TUI"})]
    );
    // And q quits.
    rig.press(&[KeyCode::Char('q')]).await;
    assert!(rig.runner.quitting());
}

/// A paste in the list, with no session open, quits nothing: it waits in
/// the input line, and the footer says how to reach it. A paste disarms a
/// waiting stop, as any other key would.
#[tokio::test]
async fn a_paste_in_the_list_waits_in_the_input_line() {
    let mut rig = harbour_rig(80, 24);
    rig.shows("check the tide tables").await;
    paste(&mut rig, "q").await;
    assert!(!rig.runner.quitting());
    assert_eq!(rig.runner.app.mode, Mode::Normal);
    assert_eq!(rig.runner.app.input, "q");
    assert!(
        rig.screen()[23].contains("pasted into the input line"),
        "{:?}",
        rig.screen()
    );
    // A conversation's stop, armed: the paste is not its second key.
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('s')]).await;
    paste(&mut rig, "s").await;
    rig.settle().await;
    assert!(rig.daemon().asked("execution.stop").is_empty());
    assert_eq!(rig.runner.app.mode, Mode::Input);
    assert_eq!(rig.runner.app.input, "qs");
}

/// Enter on a fresh TUI, nothing under the cursor, opens the first row.
#[tokio::test]
async fn enter_at_start_opens_the_first_row() {
    let mut rig = harbour_rig(80, 24);
    rig.shows("check the tide tables").await;
    assert_eq!(rig.runner.app.selected, None);
    rig.press(&[KeyCode::Enter]).await;
    assert_eq!(focused(&rig).as_deref(), Some("ses_spec01"));
    assert_eq!(rig.runner.app.selected.as_deref(), Some("ses_spec01"));
    rig.asked("session.history", 1).await;
    assert_eq!(
        rig.daemon().asked("session.history")[0]["session_id"],
        "ses_spec01"
    );
}

/// Run the loop to its end, for at most 5 s.
async fn run(rig: &mut Rig) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(5), rig.runner.run())
        .await
        .expect("the loop ends")
}

/// What the loop wrote: bracketed paste on first, and off last.
fn on_then_off(out: &Captured, way: &str) {
    let text = out.text();
    assert!(
        text.starts_with("\x1b[?1004h\x1b[?2004h"),
        "{way}: {text:?}"
    );
    assert!(text.ends_with("\x1b[?2004l\x1b[?1004l"), "{way}: {text:?}");
    assert_eq!(text.matches(PASTE_ON).count(), 1, "{way}: {text:?}");
    assert_eq!(text.matches(PASTE_OFF).count(), 1, "{way}: {text:?}");
}

/// A way the loop ends, set up before it runs.
type End = fn(&mut Rig);

/// Bracketed paste is on while the loop runs, and off on every way out: q,
/// ctrl-c, the terminal's reader ending, and SIGTERM or SIGHUP.
#[tokio::test]
async fn bracketed_paste_is_off_on_every_way_out() {
    let ways: [(&str, End); 4] = [
        ("q", |r| {
            let k = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
            r.keys.send(TermEvent::Key(k)).unwrap();
        }),
        ("ctrl-c", |r| {
            let k = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
            r.keys.send(TermEvent::Key(k)).unwrap();
        }),
        ("the reader ended", |r| r.keys = mpsc::unbounded_channel().0),
        ("a signal", |r| {
            let (tx, rx) = mpsc::unbounded_channel();
            tx.send(()).unwrap();
            r.runner.signals = Some(rx);
        }),
    ];
    for (way, end) in ways {
        let mut rig = harbour_rig(80, 24);
        let out = Captured::default();
        rig.runner.out = Box::new(out.clone());
        end(&mut rig);
        run(&mut rig).await.unwrap();
        on_then_off(&out, way);
    }
}

/// A terminal whose size can't be read: the loop's first draw fails.
struct Broken(TestBackend);

impl Backend for Broken {
    type Error = std::io::Error;
    fn draw<'a, I>(&mut self, content: I) -> std::io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let Ok(()) = self.0.draw(content);
        Ok(())
    }
    fn hide_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn show_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn get_cursor_position(&mut self) -> std::io::Result<Position> {
        Ok(Position::ORIGIN)
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, _: P) -> std::io::Result<()> {
        Ok(())
    }
    fn clear(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn clear_region(&mut self, _: ClearType) -> std::io::Result<()> {
        Ok(())
    }
    fn size(&self) -> std::io::Result<Size> {
        Err(std::io::Error::other("the terminal went away"))
    }
    fn window_size(&mut self) -> std::io::Result<WindowSize> {
        Err(std::io::Error::other("the terminal went away"))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// An error ends the loop with bracketed paste off too.
#[tokio::test]
async fn an_error_turns_bracketed_paste_off() {
    let connect: Connector =
        Box::new(|| Box::pin(async { Err(anyhow::anyhow!("nothing listens")) }));
    let (_keys, events) = mpsc::unbounded_channel();
    let term = Terminal::with_options(
        Broken(TestBackend::new(80, 24)),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 80, 24)),
        },
    )
    .unwrap();
    let mut runner = Runner::new(App::new(utc_hm), term, connect, events, clock);
    runner.frame = Duration::ZERO;
    let out = Captured::default();
    runner.out = Box::new(out.clone());
    let ran = tokio::time::timeout(Duration::from_secs(5), runner.run())
        .await
        .expect("the loop ends");
    assert!(ran.is_err(), "the draw failed");
    on_then_off(&out, "an error");
}

/// A panic turns bracketed paste off, before the hook it replaced runs.
#[test]
fn a_panic_turns_bracketed_paste_off() {
    static OUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());
    struct Out;
    impl std::io::Write for Out {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            OUT.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    crate::term::on_panic(|| Box::new(Out));
    let caught = std::panic::catch_unwind(|| panic!("a planted panic"));
    // The default hook again.
    drop(std::panic::take_hook());
    assert!(caught.is_err());
    let text = String::from_utf8_lossy(&OUT.lock().unwrap()).into_owned();
    assert_eq!(text, "\x1b[?2004l\x1b[?1004l");
}

/// A pasted escape sequence reaches neither the input line nor the screen
/// raw: ESC, BEL and C1 controls are dropped, a `\r` is a line break, and a
/// tab is kept (review of theseus-8hcg).
#[tokio::test]
async fn a_pasted_escape_reaches_neither_the_input_line_nor_the_footer() {
    let mut rig = harbour_rig(80, 24);
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('i')]).await;
    paste(&mut rig, "\x1b[2J\x1b]0;moored\x07red\u{9b}31m\ttab\rnext").await;
    rig.settle().await;
    let input = rig.runner.app.input.clone();
    assert_eq!(input, "[2J]0;mooredred31m\ttab\nnext");
    assert!(
        !input
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t')),
        "{input:?}"
    );
    let footer = &rig.screen()[23];
    assert!(footer.starts_with(" > "), "{footer:?}");
    assert!(footer.ends_with("31m tab↵next"), "{footer:?}");
    assert!(!footer.chars().any(char::is_control), "{footer:?}");
    assert!(rig.daemon().asked("turn.submit").is_empty());
}

/// In the filter a paste is one line, and enter then opens the one row it
/// shows in two presses: one ends the filter, one opens (review of
/// theseus-8hcg; client-names' review counted three on main).
#[tokio::test]
async fn a_paste_in_the_filter_is_one_line_and_two_enters_open_its_row() {
    let mut rig = harbour_rig(80, 24);
    rig.shows("check the tide tables").await;
    rig.press(&[KeyCode::Char('/')]).await;
    paste(&mut rig, "old\r\nnotes").await;
    assert_eq!(rig.runner.app.filter, "old notes");
    assert_eq!(rig.runner.app.mode, Mode::Filter);
    rig.press(&[KeyCode::Enter, KeyCode::Enter]).await;
    assert_eq!(focused(&rig).as_deref(), Some("ses_old001"));
    assert!(!rig.runner.quitting());
}

/// In a decline's note a paste is the note's text, line breaks kept; it is
/// sent with the decline on enter, never before.
#[tokio::test]
async fn a_paste_in_a_declines_note_is_sent_with_the_decline() {
    let mut rig = card_rig().await;
    rig.press(&[KeyCode::Char('n')]).await;
    assert!(matches!(rig.runner.app.mode, Mode::Note(_)));
    paste(&mut rig, "after the\r\ntide turns").await;
    rig.settle().await;
    assert!(rig.daemon().asked("action.confirm").is_empty());
    rig.press(&[KeyCode::Enter]).await;
    rig.asked("action.confirm", 1).await;
    assert_eq!(
        rig.daemon().asked("action.confirm"),
        [
            json!({"correlation_id": "cor_spec01", "approve": false, "author": "the TUI",
                "note": "after the\ntide turns"})
        ]
    );
}

/// A terminal that refuses the first write (the modes' `enter`): the loop
/// still runs, and bracketed paste is still turned off on the way out.
#[tokio::test]
async fn a_failed_first_write_still_ends_with_paste_off() {
    #[derive(Clone, Default)]
    struct FailsFirst(Captured, std::sync::Arc<std::sync::atomic::AtomicBool>);
    impl std::io::Write for FailsFirst {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if !self.1.swap(true, std::sync::atomic::Ordering::SeqCst) {
                return Err(std::io::Error::other("the terminal refused it"));
            }
            self.0.write(b)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut rig = harbour_rig(80, 24);
    let out = FailsFirst::default();
    rig.runner.out = Box::new(out.clone());
    let k = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
    rig.keys.send(TermEvent::Key(k)).unwrap();
    run(&mut rig).await.unwrap();
    let text = out.0.text();
    assert!(!text.contains(PASTE_ON), "{text:?}");
    assert!(text.ends_with("\x1b[?2004l\x1b[?1004l"), "{text:?}");
}

/// A long paste shows its end in the footer, cut with `…`, its line breaks
/// as `↵`, read from the end only (review of theseus-8hcg).
#[tokio::test]
async fn a_long_paste_shows_its_end_in_the_footer() {
    let mut rig = harbour_rig(80, 24);
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('i')]).await;
    let text = "the north quay's tide table.\n".repeat(40_000);
    paste(&mut rig, &text).await;
    rig.settle().await;
    let footer = rig.screen()[23].clone();
    assert!(footer.starts_with(" > …"), "{footer:?}");
    assert!(footer.ends_with("tide table.↵"), "{footer:?}");
    assert_eq!(rig.runner.app.input.len(), text.len());
}
