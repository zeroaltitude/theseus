//! The chat over a scripted daemon (a JSON-RPC stream on a duplex, through
//! `Conn::over`, as the TUI's rig does), its bytes read back as a screen by
//! `vt100`, a terminal emulator that is a dev-dependency only. Time is tokio's
//! paused clock, and the chat's wall clock follows it, so the dock's timer
//! moves exactly as far as the test lets it.

use std::cell::Cell;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;
use theseus_protocol::{
    ConfirmRequest, Event, ModelDelta, ToolEnded, ToolProposed, ToolStarted, TurnSubmitResult,
};
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::env::shown_dir;
use super::run::{Connector, Runner};
use super::state::{Cfg, Chat};
use crate::tests::{daemon, Answer, Daemon, Script};

pub const T0: u64 = 1_790_000_000_000;
pub const SID: &str = "ses_0123456789abcdef";

thread_local! {
    static BASE: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// The chat's wall clock: `T0` plus the paused clock's time since the rig.
fn clock() -> u64 {
    BASE.with(|b| {
        let base = b.get().expect("a rig");
        T0 + Instant::now().duration_since(base).as_millis() as u64
    })
}

/// A terminal: what the chat wrote, parsed as a screen.
pub struct Term {
    pub parser: vt100::Parser,
    pub raw: Vec<u8>,
}

#[derive(Clone)]
pub struct Sink(pub Arc<Mutex<Term>>);

impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        let mut t = self.0.lock().unwrap();
        t.parser.process(b);
        t.raw.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The daemon's script: the chat's start, and a quiet turn.
pub fn script() -> Script {
    Arc::new(|m, _p| -> Answer {
        Ok(match m {
            "health" => json!({"name": "theseusd", "model": "claude-sonnet-5-5", "profile": "f10"}),
            "session.open" => json!({
                "session_id": SID, "kind": "conversation", "label": null,
                "created_at_unix_ms": T0, "turns": 0,
            }),
            "session.list" => json!({"sessions": []}),
            "session.watch" => json!({"watching": SID, "watchers": 1}),
            _ => json!({}),
        })
    })
}

/// The chat on a terminal of `width` by `height`, its keys, and its daemon.
pub struct Rig {
    pub runner: Runner,
    pub keys: mpsc::UnboundedSender<TermEvent>,
    pub term: Arc<Mutex<Term>>,
    pub daemon: Daemon,
    /// The daemons the loop reconnected to, kept so their connections stay up.
    pub later: Arc<Mutex<Vec<Daemon>>>,
    pub history: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

pub fn cfg(history_path: std::path::PathBuf) -> Cfg {
    Cfg {
        dir: "/home/ada/work/harbour-tides".into(),
        shown_dir: shown_dir("/home/ada/work/harbour-tides", Some("/home/ada")),
        branch: Some("main".into()),
        resume: None,
        start: super::state::Start::New,
        color: false,
        history: Vec::new(),
        history_path: Some(history_path),
    }
}

impl Rig {
    pub async fn new(width: u16, height: u16) -> Self {
        Self::with(width, height, script()).await
    }

    pub async fn with(width: u16, height: u16, script: Script) -> Self {
        Self::with_cfg(width, height, script, |_| {}).await
    }

    /// The chat as `change` configures it (how it starts, say).
    pub async fn with_cfg(
        width: u16,
        height: u16,
        script: Script,
        change: impl FnOnce(&mut Cfg),
    ) -> Self {
        BASE.with(|b| b.set(Some(Instant::now())));
        let dir = tempfile::tempdir().unwrap();
        let history = dir.path().join("history");
        let (conn, d) = daemon(script.clone());
        let later = Arc::new(Mutex::new(Vec::new()));
        let kept = later.clone();
        let connect: Connector = Box::new(move || {
            let (conn, d) = daemon(script.clone());
            kept.lock().unwrap().push(d);
            Box::pin(async move { Ok(conn) })
        });
        let (keys, events) = mpsc::unbounded_channel();
        let mut cfg = cfg(history.clone());
        change(&mut cfg);
        let mut chat = Chat::new(cfg, clock());
        chat.resized(width, height);
        let term = Arc::new(Mutex::new(Term {
            parser: vt100::Parser::new(height, width, 2000),
            raw: Vec::new(),
        }));
        let mut runner = Runner::new(chat, Some(conn), connect, events, clock);
        runner.frame = Duration::ZERO;
        runner.out = Box::new(Sink(term.clone()));
        runner.start().await;
        Self {
            runner,
            keys,
            term,
            daemon: d,
            later,
            history,
            _dir: dir,
        }
    }

    /// The visible screen, a row a string, trailing blanks off.
    pub fn screen(&self) -> Vec<String> {
        let t = self.term.lock().unwrap();
        t.parser
            .screen()
            .contents()
            .lines()
            .map(|l| l.trim_end().to_string())
            .collect()
    }

    pub fn text(&self) -> String {
        self.screen().join("\n")
    }

    /// Every byte written so far.
    pub fn bytes(&self) -> usize {
        self.term.lock().unwrap().raw.len()
    }

    pub fn written_since(&self, mark: usize) -> String {
        String::from_utf8_lossy(&self.term.lock().unwrap().raw[mark..]).into_owned()
    }

    /// Step until `cond` holds, for at most 5 s of the paused clock.
    pub async fn until(&mut self, what: &str, cond: impl Fn(&Rig) -> bool) {
        let ok = tokio::time::timeout(Duration::from_secs(5), async {
            while !cond(self) {
                self.runner.step().await.unwrap();
            }
        })
        .await;
        if ok.is_err() {
            panic!("timed out waiting for {what}; the screen:\n{}", self.text());
        }
    }

    pub async fn shows(&mut self, text: &str) {
        let t = text.to_string();
        self.until(&format!("the screen to show {text:?}"), move |r| {
            r.text().contains(&t)
        })
        .await;
    }

    pub async fn asked(&mut self, method: &str, n: usize) {
        let m = method.to_string();
        self.until(&format!("{n} {method} request(s)"), move |r| {
            r.daemon.asked(&m).len() >= n
        })
        .await;
    }

    /// Let the loop take what is ready, then run on `ms` of the paused clock.
    pub async fn advance(&mut self, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        while tokio::time::timeout_at(end, self.runner.step())
            .await
            .is_ok()
        {}
    }

    /// Let the loop take everything already sent to it.
    pub async fn settle(&mut self) {
        while tokio::time::timeout(Duration::from_millis(5), self.runner.step())
            .await
            .is_ok()
        {}
    }

    pub async fn key(&mut self, k: KeyEvent) {
        self.keys.send(TermEvent::Key(k)).unwrap();
        self.settle().await;
    }

    pub async fn press(&mut self, code: KeyCode) {
        self.key(KeyEvent::new(code, KeyModifiers::NONE)).await;
    }

    pub async fn ctrl(&mut self, c: char) {
        self.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
            .await;
    }

    pub async fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.press(KeyCode::Char(c)).await;
        }
    }

    pub async fn paste(&mut self, text: &str) {
        self.keys.send(TermEvent::Paste(text.to_string())).unwrap();
        self.settle().await;
    }

    pub async fn resize(&mut self, w: u16, h: u16) {
        self.term.lock().unwrap().parser.set_size(h, w);
        self.keys.send(TermEvent::Resize(w, h)).unwrap();
        self.settle().await;
    }

    /// The daemon tells the chat an event.
    pub fn tell(&self, e: Event) {
        let n = e.notification();
        self.daemon.notify(&n.method, n.params);
    }

    /// The chat is ready: banner printed, session open and watched.
    pub async fn ready(&mut self) {
        self.shows("? help").await;
        self.asked("session.watch", 1).await;
    }

    pub fn starts_turn(&self, turn: &str) {
        self.tell(Event::TurnStarted(theseus_protocol::TurnStarted {
            session_id: SID.into(),
            turn_id: turn.into(),
            ..Default::default()
        }));
    }

    pub fn answers(&self, turn: &str, text: &str) {
        self.tell(Event::ModelAnswered(ModelDelta {
            turn_id: turn.into(),
            loop_index: 0,
            text: text.into(),
        }));
    }

    pub fn ends_turn(&self, turn: &str, output: &str) {
        self.tell(Event::TurnEnded(TurnSubmitResult {
            session_id: SID.into(),
            turn_id: turn.into(),
            output: output.into(),
            model: "claude-sonnet-5-5".into(),
            elapsed_ms: 1234,
            cost_usd: Some(0.0042),
            session_cost_usd: Some(0.0042),
            stop_reason: "done".into(),
            ..Default::default()
        }));
    }
}

/// The scrollback's last screenful and the screen, top first. vt100 reads
/// back no further than one screen's height, so a test keeps its transcript
/// within it.
fn everything(t: &mut Term) -> String {
    let rows = usize::from(t.parser.screen().size().0);
    t.parser.set_scrollback(rows);
    let reach = t.parser.screen().scrollback();
    assert!(
        reach < rows,
        "the transcript is deeper than a screen's scrollback can be read"
    );
    let top = t.parser.screen().contents();
    t.parser.set_scrollback(0);
    format!("{top}\n{}", t.parser.screen().contents())
}

fn golden(name: &str, got: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, got).unwrap();
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("no golden {name}; THESEUS_GOLDEN=write writes it"));
    assert_eq!(
        got, want,
        "{name} differs; THESEUS_GOLDEN=write rewrites it"
    );
}

// ------------------------------------------------------------ the start

#[tokio::test(start_paused = true)]
async fn the_banner_names_the_model_the_directory_and_its_branch() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    let screen = r.screen();
    assert_eq!(
        screen[0],
        "theseus · Sonnet 5.5 · ~/work/harbour-tides (main)"
    );
    assert_eq!(screen[1], "? help · / commands");
    // The prompt and the status line under it.
    assert!(screen.iter().any(|l| l == "›"), "{screen:?}");
    assert!(
        screen
            .last()
            .unwrap()
            .starts_with("default · ~/work/harbour-tides · Sonnet 5.5 · $0.00"),
        "{screen:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn the_prompt_is_drawn_before_the_daemon_answers_anything() {
    let r = Rig::new(80, 24).await;
    // `start` drew once and asked; nothing was stepped, so nothing answered.
    assert!(r.text().contains('›'), "{}", r.text());
    assert!(
        !r.text().contains("theseus ·"),
        "the banner waits for the daemon's words"
    );
}

#[tokio::test(start_paused = true)]
async fn one_session_is_opened_in_this_directory_and_watched_once() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.settle().await;
    let methods = r.daemon.methods();
    assert_eq!(
        methods.iter().filter(|m| *m == "session.open").count(),
        1,
        "{methods:?}"
    );
    assert_eq!(methods.iter().filter(|m| *m == "session.watch").count(), 1);
    assert_eq!(
        r.daemon.asked("session.open")[0]["dir"],
        "/home/ada/work/harbour-tides"
    );
    assert!(
        !methods
            .iter()
            .any(|m| m == "turn.submit" || m == "session.history"),
        "a new conversation reads no history: {methods:?}"
    );
}

// ------------------------------------------------------------ a turn

#[tokio::test(start_paused = true)]
async fn a_question_and_its_answer_land_in_the_transcript_once() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.type_text("what does this project do?").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    let sent = &r.daemon.asked("turn.submit")[0];
    assert_eq!(sent["input"], "what does this project do?");
    assert_eq!(sent["session_id"], SID);
    assert!(
        r.text().contains("› what does this project do?"),
        "{}",
        r.text()
    );

    r.starts_turn("trn_1");
    r.shows("◐ thinking · 0:00").await;
    r.answers("trn_1", "It predicts tides for a station.");
    r.ends_turn("trn_1", "It predicts tides for a station.");
    r.shows("1.2 s · Sonnet 5.5 · $0.0042 total ($0.0042 this reply)")
        .await;
    let text = r.text();
    assert_eq!(
        text.matches("It predicts tides for a station.").count(),
        1,
        "{text}"
    );
    assert!(
        !text.contains("thinking"),
        "the live line goes with the turn:\n{text}"
    );
    assert!(
        r.screen().last().unwrap().contains("$0.0042"),
        "the status line carries the cost"
    );
}

#[tokio::test(start_paused = true)]
async fn the_live_line_moves_while_a_turn_runs_and_the_scrollback_does_not() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.type_text("run the slow survey").await;
    r.press(KeyCode::Enter).await;
    r.starts_turn("trn_1");
    r.shows("thinking · 0:00").await;
    let before = r.screen();
    r.advance(1_000).await;
    let after = r.screen();
    assert!(r.text().contains("thinking · 0:01"), "{}", r.text());
    let moved: Vec<_> = before.iter().zip(&after).filter(|(a, b)| a != b).collect();
    assert_eq!(moved.len(), 1, "only the live line changed: {moved:?}");
    // Nothing above the dock was written again: the bytes of the last second
    // hold no transcript text.
    let mark = r.bytes();
    r.advance(1_000).await;
    let new = r.written_since(mark);
    assert!(
        !new.contains("run the slow survey"),
        "the scrollback was redrawn: {new:?}"
    );
    assert!(
        !new.contains("theseus ·"),
        "the banner was redrawn: {new:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn an_idle_chat_draws_nothing_on_a_timer() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.type_text("hi").await;
    r.settle().await;
    let (frames, bytes) = (r.runner.frames, r.bytes());
    r.advance(30_000).await;
    assert_eq!(
        r.runner.frames, frames,
        "the dock was redrawn with nothing to show"
    );
    assert_eq!(r.bytes(), bytes);
}

#[tokio::test(start_paused = true)]
async fn at_most_thirty_frames_a_second() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.runner.frame = Duration::from_millis(33);
    r.starts_turn("trn_1");
    r.settle().await;
    let frames = r.runner.frames;
    // A burst of events and keys in one second.
    for i in 0..200 {
        r.answers("trn_1", &format!("line {i}"));
    }
    for _ in 0..100 {
        r.press(KeyCode::Char('x')).await;
    }
    r.advance(1_000).await;
    assert!(
        r.runner.frames - frames <= 31,
        "{} frames in a second",
        r.runner.frames - frames
    );
}

#[tokio::test(start_paused = true)]
async fn a_call_is_a_dock_row_while_it_runs_and_a_line_when_it_ends() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.starts_turn("trn_1");
    r.tell(Event::ToolProposed(ToolProposed {
        session_id: SID.into(),
        turn_id: "trn_1".into(),
        tool_use_id: "tu_1".into(),
        tool: "proc.run".into(),
        input: json!({"argv": ["python3", "slow_survey.py"]}),
        ..Default::default()
    }));
    r.tell(Event::ToolStarted(ToolStarted {
        session_id: SID.into(),
        turn_id: "trn_1".into(),
        tool_use_id: "tu_1".into(),
        tool: "proc.run".into(),
        ..Default::default()
    }));
    r.shows("Run `python3 slow_survey.py` · 0:00").await;
    r.advance(34_000).await;
    assert!(
        r.text().contains("Run `python3 slow_survey.py` · 0:34"),
        "{}",
        r.text()
    );
    assert_eq!(
        r.text().matches("slow_survey.py").count(),
        1,
        "one row, not one a second"
    );
    r.tell(Event::ToolEnded(ToolEnded {
        session_id: SID.into(),
        turn_id: "trn_1".into(),
        tool_use_id: "tu_1".into(),
        tool: "proc.run".into(),
        status: "ok".into(),
        duration_ms: Some(34_100),
        ..Default::default()
    }));
    r.shows("● Run `python3 slow_survey.py`").await;
    let line = r
        .screen()
        .into_iter()
        .find(|l| l.starts_with("● Run"))
        .unwrap();
    assert!(line.ends_with("✓ 34.1 s"), "{line:?}");
    assert!(
        !r.text().contains("Run `python3 slow_survey.py` ·"),
        "the row left the dock:\n{}",
        r.text()
    );
}

#[tokio::test(start_paused = true)]
async fn a_question_shows_in_the_dock_and_y_answers_it() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.starts_turn("trn_1");
    r.tell(Event::ConfirmRequested(ConfirmRequest {
        correlation_id: "act_77".into(),
        session_id: SID.into(),
        tool: "proc.run".into(),
        input: json!({"argv": ["python3", "-m", "unittest"]}),
        reason: "run python3 -m unittest".into(),
        ..Default::default()
    }));
    r.shows("y yes · n no").await;
    assert!(
        r.text().contains("? Run `python3 -m unittest`"),
        "{}",
        r.text()
    );
    assert!(r.text().contains("waiting for you"), "{}", r.text());
    r.press(KeyCode::Char('y')).await;
    r.asked("action.confirm", 1).await;
    let a = &r.daemon.asked("action.confirm")[0];
    assert_eq!(
        (
            a["correlation_id"].as_str(),
            a["approve"].as_bool(),
            a["author"].as_str()
        ),
        (Some("act_77"), Some(true), Some("theseus chat"))
    );
    r.tell(Event::ConfirmResolved(theseus_protocol::ConfirmResolved {
        correlation_id: "act_77".into(),
        session_id: SID.into(),
        option: Some("approve".into()),
        ..Default::default()
    }));
    r.shows("approved").await;
    assert!(!r.text().contains("y yes"), "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn y_is_text_when_the_line_is_not_empty() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.tell(Event::ConfirmRequested(ConfirmRequest {
        correlation_id: "act_77".into(),
        session_id: SID.into(),
        tool: "proc.run".into(),
        reason: "r".into(),
        ..Default::default()
    }));
    r.shows("y yes").await;
    r.type_text("why").await;
    assert!(r.daemon.asked("action.confirm").is_empty());
    assert!(r.text().contains("› why"), "{}", r.text());
}

// ------------------------------------------------------------ the editor

fn log() -> String {
    (1..=60)
        .map(|n| format!("{n:02} 12:{n:02}:07 reading\n"))
        .collect()
}

#[tokio::test(start_paused = true)]
async fn a_long_paste_folds_and_goes_with_the_question_as_one_message() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.paste(&log()).await;
    r.type_text("what went wrong?").await;
    let screen = r.text();
    assert!(
        screen.contains("› [pasted 60 lines · 1."),
        "the paste is a chip and the prompt is readable:\n{screen}"
    );
    assert!(
        screen.contains("ctrl+o shows it]what went wrong?") || screen.contains("what went wrong?")
    );
    assert!(
        !screen.contains("12:30:07"),
        "the log is not on the screen:\n{screen}"
    );
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    let sent = r.daemon.asked("turn.submit");
    assert_eq!(sent.len(), 1, "one message, not one a line");
    assert_eq!(sent[0]["input"], format!("{}what went wrong?", log()));
}

#[tokio::test(start_paused = true)]
async fn a_pasted_y_or_q_is_text_and_a_pasted_newline_sends_nothing() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.tell(Event::ConfirmRequested(ConfirmRequest {
        correlation_id: "act_77".into(),
        session_id: SID.into(),
        tool: "proc.run".into(),
        reason: "r".into(),
        ..Default::default()
    }));
    r.shows("y yes").await;
    r.paste("y\nyes\nsure").await;
    r.settle().await;
    assert!(
        r.daemon.asked("action.confirm").is_empty(),
        "a paste answered the question"
    );
    assert!(
        r.daemon.asked("turn.submit").is_empty(),
        "a paste sent a message"
    );
    assert!(r.text().contains("› y"), "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn ctrl_o_shows_the_folded_paste_in_the_scrollback() {
    let mut r = Rig::new(80, 100).await;
    r.ready().await;
    r.paste(&log()).await;
    r.ctrl('o').await;
    r.shows("pasted text:").await;
    assert!(r.text().contains("  60 12:60:07 reading"), "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn multi_line_input_by_ctrl_j_and_backslash_enter() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.type_text("one").await;
    r.ctrl('j').await;
    r.type_text("two\\").await;
    r.press(KeyCode::Enter).await;
    r.type_text("three").await;
    assert!(r.daemon.asked("turn.submit").is_empty());
    let text = r.text();
    assert!(text.contains("› one\n  two\n  three"), "{text}");
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    assert_eq!(r.daemon.asked("turn.submit")[0]["input"], "one\ntwo\nthree");
}

#[tokio::test(start_paused = true)]
async fn up_recalls_this_directorys_messages_and_the_history_is_kept_on_disk() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.type_text("first thing").await;
    r.press(KeyCode::Enter).await;
    r.type_text("second thing").await;
    r.press(KeyCode::Enter).await;
    r.settle().await;
    r.press(KeyCode::Up).await;
    assert!(
        r.screen().iter().any(|l| l == "› second thing"),
        "{}",
        r.text()
    );
    r.press(KeyCode::Up).await;
    assert!(r.screen().iter().any(|l| l == "› first thing"));
    r.press(KeyCode::Down).await;
    assert!(r.screen().iter().any(|l| l == "› second thing"));
    // On disk, per directory.
    r.until("the history file", |r| r.history.exists()).await;
    tokio::task::yield_now().await;
    let saved = super::env::load_history(&r.history, "/home/ada/work/harbour-tides");
    assert_eq!(saved, ["first thing", "second thing"]);
}

#[tokio::test(start_paused = true)]
async fn ctrl_c_clears_the_line_and_twice_leaves_saying_the_conversation_goes_on() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.type_text("half a thought").await;
    r.ctrl('c').await;
    assert!(!r.text().contains("half a thought"), "{}", r.text());
    assert!(!r.runner.quitting());
    r.ctrl('c').await;
    r.shows("ctrl+c again to leave").await;
    assert!(!r.runner.quitting());
    r.ctrl('c').await;
    assert!(r.runner.quitting());
    r.shows("left · the conversation goes on · theseus -c · theseus chat abcdef")
        .await;
}

// ------------------------------------------------------------ commands

#[tokio::test(start_paused = true)]
async fn a_slash_lists_the_commands_filtered_as_you_type() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.press(KeyCode::Char('/')).await;
    assert!(r.text().contains("/help"), "{}", r.text());
    assert!(r.text().contains("/quit"));
    r.press(KeyCode::Char('q')).await;
    assert!(!r.text().contains("/help  "), "filtered:\n{}", r.text());
    assert!(r.text().contains("/quit"));
    r.press(KeyCode::Tab).await;
    assert!(r.text().contains("› /quit"), "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn help_prints_the_keys_and_the_commands_and_question_mark_does_too() {
    let mut r = Rig::new(100, 30).await;
    r.ready().await;
    r.type_text("/help").await;
    r.press(KeyCode::Enter).await;
    r.shows("ctrl+c clears the line").await;
    assert!(
        r.text().contains("/quit") && r.text().contains("leave; the conversation goes on"),
        "{}",
        r.text()
    );
    assert!(
        r.daemon.asked("turn.submit").is_empty(),
        "a command is not a message"
    );
    let mark = r.bytes();
    r.press(KeyCode::Char('?')).await;
    assert!(
        r.written_since(mark).contains("ctrl+j"),
        "? printed the help"
    );
}

#[tokio::test(start_paused = true)]
async fn quit_leaves_and_an_unknown_command_is_said_not_sent() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.type_text("/nope").await;
    r.press(KeyCode::Enter).await;
    r.shows("no command /nope").await;
    assert!(r.daemon.asked("turn.submit").is_empty());
    r.type_text("/etc/hosts is odd").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    r.type_text("/quit").await;
    r.press(KeyCode::Enter).await;
    assert!(r.runner.quitting());
}

// ------------------------------------------------------------ the terminal

#[tokio::test(start_paused = true)]
async fn a_resize_redraws_the_dock_and_not_the_transcript() {
    let mut r = Rig::new(100, 30).await;
    r.ready().await;
    r.type_text("hello").await;
    r.press(KeyCode::Enter).await;
    r.starts_turn("trn_1");
    r.answers("trn_1", "Hello from the other side of the tide table.");
    r.ends_turn("trn_1", "");
    r.shows("1.2 s · Sonnet 5.5").await;
    let mark = r.bytes();
    r.resize(60, 30).await;
    let wrote = r.written_since(mark);
    assert!(!wrote.is_empty(), "the dock was redrawn");
    for transcript in ["theseus ·", "Hello from the other side", "› hello", "1.2 s"] {
        assert!(
            !wrote.contains(transcript),
            "{transcript:?} was written again on a resize: {wrote:?}"
        );
    }
    assert!(wrote.contains("default"), "the status line is the dock's");
}

#[tokio::test(start_paused = true)]
async fn the_dock_stays_at_the_bottom_when_the_transcript_scrolls() {
    let mut r = Rig::new(80, 12).await;
    r.ready().await;
    for i in 0..3 {
        r.type_text(&format!("message {i}")).await;
        r.press(KeyCode::Enter).await;
        r.starts_turn(&format!("trn_{i}"));
        r.answers(&format!("trn_{i}"), &format!("Reply number {i}."));
        r.ends_turn(&format!("trn_{i}"), "");
        r.shows(&format!("Reply number {i}")).await;
    }
    let screen = r.screen();
    assert!(
        screen.last().unwrap().starts_with("default · "),
        "{screen:?}"
    );
    assert_eq!(screen.len(), 12, "the screen is full: {screen:?}");
    assert!(screen[screen.len() - 2] == "›", "{screen:?}");
    // The scrollback holds the earliest lines, once.
    let all = everything(&mut r.term.lock().unwrap());
    assert!(
        all.contains("theseus · Sonnet 5.5"),
        "the banner is in the scrollback:\n{all}"
    );
    assert_eq!(
        all.matches("Reply number 0").count(),
        1,
        "written once:\n{all}"
    );
}

#[tokio::test(start_paused = true)]
async fn the_status_line_drops_its_least_needed_parts_on_a_narrow_terminal() {
    let mut r = Rig::new(40, 24).await;
    r.ready().await;
    let last = r.screen().last().unwrap().clone();
    assert!(last.chars().count() <= 40, "{last:?}");
    assert!(
        last.contains("$0.00") && last.contains("Sonnet 5.5"),
        "{last:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn the_dock_is_a_golden_at_80_and_120_columns() {
    for (w, name) in [(80u16, "chat_80.txt"), (120, "chat_120.txt")] {
        let mut r = Rig::new(w, 40).await;
        r.ready().await;
        r.type_text("what does this project do? Please read the README and tell me in a few sentences what the project is for.").await;
        r.press(KeyCode::Enter).await;
        r.starts_turn("trn_1");
        r.tell(Event::ToolProposed(ToolProposed {
            session_id: SID.into(),
            turn_id: "trn_1".into(),
            tool_use_id: "tu_1".into(),
            tool: "fs.read".into(),
            input: json!({"path": "README.md"}),
            ..Default::default()
        }));
        r.tell(Event::ToolEnded(ToolEnded {
            session_id: SID.into(),
            turn_id: "trn_1".into(),
            tool_use_id: "tu_1".into(),
            tool: "fs.read".into(),
            status: "ok".into(),
            duration_ms: Some(12),
            ..Default::default()
        }));
        r.answers(
            "trn_1",
            "Harbour Tides predicts high and low water for a station from its harmonic constants. It reads a station file, sums the constituents for each minute of the day, and prints the extremes.\n\n- tides.py: the predictor\n- test_tides.py: its tests",
        );
        r.ends_turn("trn_1", "");
        r.shows("this reply)").await;
        r.type_text("and the tests?").await;
        golden(name, &(r.screen().join("\n") + "\n"));
    }
}

// ------------------------------------------------------------ the connection

#[tokio::test(start_paused = true)]
async fn a_lost_connection_is_said_once_and_the_session_is_watched_again() {
    let mut r = Rig::new(80, 24).await;
    r.ready().await;
    r.daemon.close();
    r.shows("connection to theseusd lost; reconnecting").await;
    // The retry connects to a fresh scripted daemon; the session is known, so
    // it is watched again and not opened again.
    r.until("the status line to say offline", |r| {
        r.text().contains("offline")
    })
    .await;
    r.advance(1_000).await;
    r.until("the dock to leave offline", |r| {
        !r.text().contains("offline")
    })
    .await;
    assert_eq!(r.text().matches("connection to theseusd lost").count(), 1);
    let later = r.later.lock().unwrap();
    assert_eq!(later.len(), 1, "one reconnect");
    assert_eq!(later[0].asked("session.watch")[0]["session_id"], SID);
    assert!(
        later[0].asked("session.open").is_empty(),
        "the same conversation, not a new one"
    );
}

#[tokio::test(start_paused = true)]
async fn a_message_typed_before_the_session_opens_is_sent_once_it_does() {
    let mut r = Rig::new(80, 24).await;
    // Typed at once, before the daemon has answered `session.open`.
    r.type_text("hurry").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    assert_eq!(r.daemon.asked("turn.submit")[0]["session_id"], SID);
    assert_eq!(r.daemon.asked("turn.submit").len(), 1);
}

#[tokio::test(start_paused = true)]
async fn the_status_line_counts_the_open_tasks_it_has_seen() {
    let mut r = Rig::new(100, 24).await;
    r.ready().await;
    let task = |id: &str, state| {
        Event::TaskChanged(theseus_protocol::tasks::TaskChanged {
            session_id: SID.into(),
            verb: "update".into(),
            task: theseus_protocol::tasks::TaskRecord {
                id: id.into(),
                state,
                ..Default::default()
            },
        })
    };
    use theseus_protocol::tasks::TaskState::{Done, InProgress};
    r.tell(task("tsk_1", InProgress));
    r.tell(task("tsk_2", InProgress));
    r.until("two tasks", |r| r.text().contains("◐ 2 tasks"))
        .await;
    r.tell(task("tsk_1", Done));
    r.until("one task", |r| {
        r.text().contains("◐ 1 task") && !r.text().contains("2 tasks")
    })
    .await;
}

/// The turn's link in the cockpit (theseus-x9swu) sits under its line, and
/// `/inspect` says it again; before any turn ended it says there is none.
#[tokio::test(start_paused = true)]
async fn a_turn_ends_with_its_link_and_inspect_says_it_again() {
    let url = "http://localhost:17433/session/ses_reef42/turn/trn_1";
    let mut r = Rig::new(120, 30).await;
    r.ready().await;
    r.type_text("/inspect").await;
    r.press(KeyCode::Enter).await;
    r.shows("no turn has ended here yet").await;
    r.starts_turn("trn_1");
    r.tell(Event::TurnEnded(TurnSubmitResult {
        session_id: SID.into(),
        turn_id: "trn_1".into(),
        model: "claude-sonnet-5-5".into(),
        elapsed_ms: 1234,
        stop_reason: "done".into(),
        inspect_url: Some(url.into()),
        ..Default::default()
    }));
    r.shows(&format!("inspect {url}")).await;
    r.type_text("/inspect").await;
    r.press(KeyCode::Enter).await;
    let text = r.text();
    assert_eq!(text.matches(&format!("inspect {url}")).count(), 2, "{text}");
}
