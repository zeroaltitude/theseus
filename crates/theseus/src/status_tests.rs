use super::*;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// A clock for the tests: seconds into a day as `HH:MM`, UTC.
fn hm(ms: u64) -> String {
    theseus_protocol::utc_hm(ms)
}

const T0: u64 = 1_759_300_000_000; // 2025-10-01 06:26:40 UTC

fn view(
    id: &str,
    state: &str,
    level: &str,
    label: &str,
    since: u64,
    position: u64,
) -> ExecutionView {
    serde_json::from_value(json!({
        "position": position, "at_ms": since,
        "execution_id": format!("exe_{id}"), "session_id": format!("ses_{id}"),
        "kind": "task", "state": state, "pending": [], "turns": 1,
        "attention": {"level": level, "label": label, "since_ms": since},
    }))
    .unwrap()
}

fn asking(id: &str, since: u64, position: u64) -> ExecutionView {
    let mut v = view(
        id,
        "waiting",
        "needs_you",
        "confirm fs.write: write regatta/paint-log.md",
        since,
        position,
    );
    v.pending = serde_json::from_value(json!([{
        "correlation_id": format!("act_{id}"), "tool": "fs.write",
        "reason": "write regatta/paint-log.md"}]))
    .unwrap();
    v
}

fn board(views: Vec<ExecutionView>) -> Board {
    let mut b = Board::default();
    b.load(ExecutionsWatchResult {
        position: 10,
        total: views.len() as u64,
        executions: views,
        confirms: vec![],
    });
    b
}

fn five() -> Board {
    board(vec![
        view("e527d2", "running", "working", "turn 5", T0 - 134_000, 11),
        asking("2a329e", T0 - 240_000, 12),
        view(
            "5a877c",
            "failed",
            "needs_you",
            "failed: the provider refused the request (400)",
            T0 - 60_000,
            13,
        ),
        view("87e9c6", "running", "working", "turn 2", T0 - 31_000, 14),
    ])
}

#[test]
fn the_short_form_counts_what_is_not_zero() {
    let s = five().summary(T0, 0);
    assert_eq!(short_line(&s), "●1 ◐2 ✗1");
    let s = Summary { new: 2, ..s };
    assert_eq!(short_line(&s), "●1 ◐2 ✗1 ◆2");
}

#[test]
fn the_short_form_is_empty_when_nothing_works_or_waits() {
    let b = board(vec![view("q1", "waiting", "ready", "ready", T0, 11)]);
    assert_eq!(short_line(&b.summary(T0, 0)), "");
    assert_eq!(short_line(&Board::default().summary(T0, 0)), "");
}

#[test]
fn the_long_form_has_a_header_and_a_row_for_each_task() {
    let mut b = five();
    b.views.get_mut("exe_87e9c6").unwrap().wake_at_ms = Some(T0 + 240_000);
    let sum = b.summary(T0, 0);
    let titles: HashMap<String, String> = [
        ("ses_2a329e", "Paint the south buoy red and log it."),
        (
            "ses_5a877c",
            "Read the tide gauge at the pier and write down every reading, carefully",
        ),
        ("ses_87e9c6", "Fetch Saturday's forecast for the harbour."),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let got = long_lines(&b, &sum, &titles, (T0, 100), &hm).join("\n");
    let want = "\
theseus 06:26 · ● 1 needs you · ◐ 2 working · ✗ 1 failed · ⏰ 06:30
● 2a329e  Paint the south buoy red and log it.  confirm fs.write: write regatta/paint-log.md · 4:00
         theseus confirm act_2a329e
✗ 5a877c  Read the tide gauge at the pier and…  failed 06:25: the provider refused the request (400)
◐ e527d2  (task)                                turn 5 · 2:14
◐ 87e9c6  Fetch Saturday's forecast for the h…  turn 2 · 0:31";
    assert_eq!(got, want);
}

#[test]
fn the_long_form_fits_a_narrow_terminal_cutting_the_title_first() {
    let b = five();
    let sum = b.summary(T0, 0);
    let titles: HashMap<String, String> = [(
        "ses_87e9c6".to_string(),
        "Fetch Saturday's forecast for the harbour.".to_string(),
    )]
    .into();
    for line in long_lines(&b, &sum, &titles, (T0, 60), &hm).iter().skip(1) {
        if !line.starts_with("         ") {
            assert!(line.chars().count() <= 60, "{line}");
        }
    }
}

#[test]
fn the_long_form_of_nothing_says_so() {
    let b = Board::default();
    assert_eq!(
        long_lines(&b, &b.summary(T0, 0), &HashMap::new(), (T0, 80), &hm),
        ["theseus 06:26 · nothing needs you or works"]
    );
}

#[test]
fn the_tab_sequences_are_osc_9_4_and_pass_through_tmux() {
    assert_eq!(tab_seq(TabState::Working, false), "\x1b]9;4;3;0\x07");
    assert_eq!(tab_seq(TabState::NeedsYou, false), "\x1b]9;4;2;100\x07");
    assert_eq!(tab_seq(TabState::Idle, false), "\x1b]9;4;0;0\x07");
    assert_eq!(
        tab_seq(TabState::Working, true),
        "\x1bPtmux;\x1b\x1b]9;4;3;0\x07\x1b\\"
    );
    assert_eq!(
        tab_seq(TabState::Idle, true),
        "\x1bPtmux;\x1b\x1b]9;4;0;0\x07\x1b\\"
    );
}

#[test]
fn a_change_is_applied_only_if_it_is_newer() {
    let mut b = board(vec![view("q1", "running", "working", "turn 1", T0, 11)]);
    // In the snapshot already.
    assert!(!b.apply(view("q1", "waiting", "ready", "ready", T0, 11)));
    assert!(!b.apply(view("zz", "running", "working", "turn 1", T0, 9)));
    assert_eq!(b.summary(T0, 0).working, 1);
    assert!(b.apply(view("q1", "waiting", "ready", "ready", T0, 12)));
    assert_eq!(b.summary(T0, 0).working, 0);
    // A late copy of the working view does not bring it back.
    assert!(!b.apply(view("q1", "running", "working", "turn 1", T0, 11)));
}

/// A scripted daemon over a duplex pipe: it answers `executions.watch` with
/// `snapshot`, then sends each notification after its pause, then closes.
fn daemon(snapshot: Value, events: Vec<(Duration, ExecutionView)>) -> Conn {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(client);
    let conn = Conn::over(cr, cw);
    tokio::spawn(async move {
        let (sr, mut sw) = tokio::io::split(server);
        let mut lines = BufReader::new(sr).lines();
        let req: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(req["method"], "executions.watch");
        assert_eq!(req["params"]["limit"], 0);
        let answer = json!({"jsonrpc": "2.0", "id": req["id"], "result": snapshot});
        sw.write_all(format!("{answer}\n").as_bytes())
            .await
            .unwrap();
        for (pause, v) in events {
            tokio::time::sleep(pause).await;
            let n = json!({"jsonrpc": "2.0", "method": "execution.changed", "params": v});
            sw.write_all(format!("{n}\n").as_bytes()).await.unwrap();
        }
        sw.shutdown().await.unwrap();
        // Hold the pipe's read side open until the client is done.
        let _ = lines.next_line().await;
    });
    conn
}

fn snapshot(views: &[ExecutionView]) -> Value {
    json!({"position": 10, "executions": views, "confirms": [], "total": views.len()})
}

#[tokio::test(start_paused = true)]
async fn watch_prints_a_line_for_each_change_of_a_count_or_of_the_first_item() {
    let mut conn = daemon(
        snapshot(&[view("a", "running", "working", "turn 1", T0, 11)]),
        vec![
            // A second task: the count changes.
            (
                Duration::from_secs(1),
                view("b", "running", "working", "turn 1", T0, 12),
            ),
            // The same counts, the first still `a`: no line.
            (
                Duration::from_secs(1),
                view("b", "running", "working", "turn 2", T0, 13),
            ),
            // `a` asks: counts change.
            (Duration::from_secs(1), asking("a", T0, 14)),
            // `b` fails: ● stays, ✗ appears.
            (
                Duration::from_secs(1),
                view("b", "failed", "needs_you", "failed: no", T0 + 5, 15),
            ),
            // `a` is answered and ends: the first item changes to `b`.
            (
                Duration::from_secs(1),
                view("a", "complete", "idle", "complete", T0 + 6, 16),
            ),
            // Nothing works or waits but the failure; then it is gone too.
            (
                Duration::from_secs(1),
                view("b", "complete", "idle", "complete", T0 + 7, 17),
            ),
        ],
    );
    let mut out = Vec::new();
    let mut sink = Sink::new(&mut out, None, true, false);
    watch_conn(&mut conn, &mut sink).await.unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "◐1\n◐2\n●1 ◐1\n●1 ✗1\n✗1\n\n"
    );
}

#[tokio::test(start_paused = true)]
async fn watch_prints_nothing_on_a_timer() {
    // One line at the start, then an hour of nothing, then the close.
    let mut conn = daemon(
        snapshot(&[view("a", "running", "working", "turn 1", T0, 11)]),
        vec![(
            Duration::from_secs(3600),
            view("a", "running", "working", "turn 2", T0, 12),
        )],
    );
    let mut out = Vec::new();
    let mut sink = Sink::new(&mut out, None, true, false);
    watch_conn(&mut conn, &mut sink).await.unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), "◐1\n");
}

#[tokio::test(start_paused = true)]
async fn the_tab_follows_the_board_and_clears_at_exit() {
    let mut conn = daemon(
        snapshot(&[]),
        vec![
            (
                Duration::from_secs(1),
                view("a", "running", "working", "turn 1", T0, 11),
            ),
            (
                Duration::from_secs(1),
                view("b", "running", "working", "turn 1", T0, 12),
            ),
            (Duration::from_secs(1), asking("a", T0, 13)),
            (
                Duration::from_secs(1),
                view("a", "complete", "idle", "complete", T0, 14),
            ),
            (
                Duration::from_secs(1),
                view("b", "complete", "idle", "complete", T0, 15),
            ),
        ],
    );
    let (mut out, mut tab) = (Vec::new(), Vec::new());
    let mut sink = Sink::new(&mut out, Some((&mut tab, true)), true, false);
    watch_conn(&mut conn, &mut sink).await.unwrap();
    sink.finish().unwrap();
    let want: String = [
        TabState::Idle,
        TabState::Working,
        TabState::NeedsYou,
        TabState::Working,
        TabState::Idle,
    ]
    .iter()
    .map(|s| tab_seq(*s, true))
    .collect();
    assert_eq!(String::from_utf8(tab).unwrap(), want);
}

#[tokio::test(start_paused = true)]
async fn wait_any_returns_on_the_first_view_that_needs_you() {
    let mut conn = daemon(
        snapshot(&[view("a", "running", "working", "turn 1", T0, 11)]),
        vec![
            (
                Duration::from_secs(5),
                view("b", "running", "working", "turn 1", T0, 12),
            ),
            (Duration::from_secs(5), asking("b", T0, 13)),
            (Duration::from_secs(5), asking("a", T0, 14)),
        ],
    );
    let r = find_blocked(&mut conn).await.unwrap();
    assert_eq!((r.reached.as_str(), r.already), ("blocked", false));
    assert_eq!(r.execution.unwrap().execution_id, "exe_b");
}

#[tokio::test(start_paused = true)]
async fn wait_any_answers_at_once_when_something_already_needs_you() {
    let mut conn = daemon(snapshot(&[asking("a", T0, 11)]), vec![]);
    let r = find_blocked(&mut conn).await.unwrap();
    assert_eq!((r.reached.as_str(), r.already), ("blocked", true));
}
