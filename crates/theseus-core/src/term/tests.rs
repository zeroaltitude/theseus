//! The terminal's own tests (theseus-n88g.4): the VT model on recorded
//! escape sequences, against golden screens; and real programs (`sh`,
//! `python3`, `cat`) driven through a pty: a prompt appears, a command's
//! output lands on the screen, Ctrl-C interrupts, a read says what changed,
//! and a close leaves no process behind.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_tools::{ToolCtx, ToolOutput};

use super::vt::Screen;
use super::*;

/// A screen fed `bytes`, as its rows joined, with `|` at each row's end so
/// trailing blanks are seen as cut.
fn screen(rows: u16, cols: u16, bytes: &[u8]) -> Screen {
    let mut s = Screen::new(rows, cols);
    s.feed(bytes);
    s
}

fn shown(s: &Screen) -> String {
    s.rows().iter().map(|r| format!("{r}|\n")).collect()
}

/// A shell's session as a terminal records it: a prompt, a command echoed,
/// its output, CR LF line ends, and a backspace that corrects a typo.
#[test]
fn a_shells_output_lands_on_rows_with_its_prompt() {
    let s = screen(
        5,
        20,
        b"$ ech\x08\x08\x08\x08\x08$ echo hi\r\nhi\r\n$ \x1b[0m",
    );
    assert_eq!(shown(&s), "$ echo hi|\nhi|\n$|\n|\n|\n");
    assert_eq!(s.cursor(), (2, 2));
}

/// A tab moves to the next stop of 8; text past the right margin wraps
/// onto the next row, and the last row scrolls the first off, into the
/// scrollback.
#[test]
fn tabs_wrap_and_scroll_keep_what_scrolled_off() {
    let s = screen(3, 10, b"a\tb\r\n0123456789AB\r\nc\r\nd");
    assert_eq!(shown(&s), "AB|\nc|\nd|\n");
    assert_eq!(s.scrolled(), 2);
    assert_eq!(s.scrollback_tail(5), vec!["a       b", "0123456789"]);
    // The deferred wrap: the tenth character leaves the cursor on its cell
    // until the next one comes.
    let s = screen(2, 4, b"abcd");
    assert_eq!((shown(&s), s.cursor()), ("abcd|\n|\n".to_string(), (0, 3)));
}

/// Cursor moves (CUP, CUU, CUF, CHA, VPA) and the erases of line and
/// display, as a full-screen program draws.
#[test]
fn cursor_moves_and_erases_draw_where_they_say() {
    let s = screen(
        4,
        12,
        b"xxxxxxxxxx\r\nyyyyyyyyyy\r\nzzzzzzzzzz\x1b[2;3Hab\x1b[K\x1b[1G>\x1b[A\x1b[3C!\x1b[4d\x1b[5`*\x1b[3;1H\x1b[1K",
    );
    assert_eq!(shown(&s), "xxxx!xxxxx|\n>yab|\n zzzzzzzzz|\n    *|\n");
    let s = screen(3, 5, b"aaaaa\r\nbbbbb\r\nccccc\x1b[2;3H\x1b[J");
    assert_eq!(shown(&s), "aaaaa|\nbb|\n|\n");
    let s = screen(3, 5, b"aaaaa\r\nbbbbb\r\nccccc\x1b[2;3H\x1b[1J");
    assert_eq!(shown(&s), "|\n   bb|\nccccc|\n");
    let s = screen(2, 5, b"abcde\x1b[2J");
    assert_eq!(shown(&s), "|\n|\n");
    // Insert and delete characters, erase characters.
    let s = screen(
        1,
        8,
        b"abcdef\x1b[1;2H\x1b[2@\x1b[1;6H\x1b[1P\x1b[1;1H\x1b[1X",
    );
    assert_eq!(shown(&s), "   bcef|\n");
}

/// A scroll region, as `less` and `vim` set it: a line feed at its bottom
/// scrolls only the region; insert and delete line move lines within it;
/// reverse index at its top scrolls it down.
#[test]
fn a_scroll_region_scrolls_only_its_rows() {
    let s = screen(5, 6, b"1\r\n2\r\n3\r\n4\r\nstat\x1b[2;4r\x1b[4;1H\nnew");
    assert_eq!(shown(&s), "1|\n3|\n4|\nnew|\nstat|\n");
    assert_eq!(s.scrolled(), 0, "a region's scroll keeps nothing");
    let s = screen(4, 4, b"a\r\nb\r\nc\r\nd\x1b[2;4r\x1b[2;1H\x1bM");
    assert_eq!(shown(&s), "a|\n|\nb|\nc|\n");
    let s = screen(4, 4, b"a\r\nb\r\nc\r\nd\x1b[2;1H\x1b[L");
    assert_eq!(shown(&s), "a|\n|\nb|\nc|\n");
    let s = screen(4, 4, b"a\r\nb\r\nc\r\nd\x1b[2;1H\x1b[2M");
    assert_eq!(shown(&s), "a|\nd|\n|\n|\n");
}

/// The alternate screen (`?1049h`): a full-screen program draws on a blank
/// one, and leaving it restores the shell's rows and its cursor.
#[test]
fn the_alternate_screen_is_left_as_it_was_found() {
    let mut s = screen(3, 10, b"$ vim x\r\n");
    s.feed(b"\x1b[?1049h\x1b[22;0;0t\x1b[1;3r\x1b[H\x1b[2J~\r\n~\r\n\x1b[3;1H\"x\" [New]");
    assert!(s.alternate());
    assert_eq!(shown(&s), "~|\n~|\n\"x\" [New]|\n");
    s.feed(b"\x1b[?1049l\x1b[23;0;0t$ ");
    assert!(!s.alternate());
    assert_eq!(shown(&s), "$ vim x|\n$|\n|\n");
}

/// What the model never sees: colours, titles (OSC), character sets, a
/// cursor's shape, bracketed paste, and keypad modes are read whole and
/// dropped. UTF-8 is a character a cell, split across feeds or not, and a
/// broken sequence is U+FFFD.
#[test]
fn escapes_it_does_not_draw_are_dropped_whole() {
    let mut s = Screen::new(2, 20);
    s.feed(b"\x1b]0;title\x07\x1b(B\x1b[1;31mred\x1b[0m \x1b[2 q\x1b[?2004h\x1b=\x1bP+q\x1b\\ok");
    s.feed("é".as_bytes()[..1].as_ref());
    s.feed(&"é".as_bytes()[1..]);
    s.feed(b"\xff!");
    assert_eq!(s.rows()[0], "red ok\u{e9}\u{fffd}!");
}

/// A program that asks where the cursor is, or what the terminal is, gets
/// its answer to read (Python's REPL and vim ask).
#[test]
fn a_programs_questions_are_answered() {
    let mut s = screen(3, 10, b"ab\x1b[6n\x1b[c\x1b[>c\x1b[5n");
    assert_eq!(
        s.take_replies(),
        b"\x1b[1;3R\x1b[?1;2c\x1b[>0;0;0c\x1b[0n".to_vec()
    );
    assert!(s.take_replies().is_empty());
}

/// `less` on a file, as it drew it, recorded: the alternate screen, a
/// scroll region, the status line in reverse video, and its quit.
#[test]
fn a_recorded_less_session_gives_its_golden_screens() {
    let mut s = Screen::new(4, 16);
    s.feed(b"$ less f\r\n");
    s.feed(b"\x1b[?1049h\x1b[22;0;0t\x1b[?1h\x1b=\rline 1\r\nline 2\r\nline 3\r\n\x1b[7mf (END)\x1b[27m\x1b[K");
    assert_eq!(shown(&s), "line 1|\nline 2|\nline 3|\nf (END)|\n");
    s.feed(b"\r\x1b[K\x1b[?1l\x1b>\x1b[?1049l\x1b[23;0;0t$ ");
    assert_eq!(shown(&s), "$ less f|\n$|\n|\n|\n");
}

/// Named keys are the bytes an xterm sends; text's newlines are Enter.
#[test]
fn named_keys_are_an_xterms_bytes() {
    for (k, b) in [
        ("Enter", &b"\r"[..]),
        ("ctrl-c", b"\x03"),
        ("C-d", b"\x04"),
        ("^[", b"\x1b"),
        ("Escape", b"\x1b"),
        ("Up", b"\x1b[A"),
        ("Left", b"\x1b[D"),
        ("Tab", b"\t"),
        ("F5", b"\x1b[15~"),
        ("Backspace", b"\x7f"),
    ] {
        assert_eq!(keys::bytes(k).as_deref(), Some(b), "{k}");
    }
    assert_eq!(keys::bytes("Hyper-X"), None);
    assert_eq!(keys::bytes("Ctrl-"), None);
    assert_eq!(keys::text("a\nb\r\n"), b"a\rb\r".to_vec());
}

// ------------------------------------------------------------ real ptys

pub(super) fn ctx(root: &Path) -> ToolCtx {
    ToolCtx::for_tests(root)
}

/// A registry whose programs get a plain environment.
pub(super) fn terms(external: &[&str]) -> Arc<Terms> {
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    Arc::new(Terms::new(
        vec![("PATH".into(), path), ("HOME".into(), "/tmp".into())],
        external.iter().map(|s| s.to_string()).collect(),
    ))
}

pub(super) async fn call(
    terms: &Arc<Terms>,
    session: &str,
    tool: &str,
    input: Value,
    root: &Path,
) -> (ToolOutput, Option<theseus_tools::External>) {
    match terms.run(tool, session, &input, &ctx(root)).await {
        Ok(o) => o,
        Err(f) => panic!("{tool} {input}: {}", f.message),
    }
}

pub(super) async fn fails(
    terms: &Arc<Terms>,
    session: &str,
    tool: &str,
    input: Value,
    root: &Path,
) -> String {
    match terms.run(tool, session, &input, &ctx(root)).await {
        Ok((o, _)) => panic!("{tool} {input} ran: {}", o.text),
        Err(f) => f.message,
    }
}

/// Read until `text` shows, bounded.
pub(super) async fn read_until(
    terms: &Arc<Terms>,
    sid: &str,
    id: &str,
    text: &str,
    root: &Path,
) -> String {
    let (o, _) = call(
        terms,
        sid,
        READ,
        json!({"terminal": id, "until": text, "timeout_ms": 15_000}),
        root,
    )
    .await;
    let cut = text.trim_end_matches(' ');
    assert!(
        o.text.contains(&format!("{cut:?} appeared")),
        "no {text:?}:\n{}",
        o.text
    );
    o.text
}

/// Whether a process with `pid` lives (a zombie does not).
pub(super) fn lives(pid: u32) -> bool {
    theseus_kernel::tree::stat(pid).is_some_and(|s| !matches!(s.state, 'Z' | 'X'))
}

/// The pids of live processes whose command line holds `marker`.
pub(super) fn marked(marker: &str) -> Vec<u32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&p| {
            std::fs::read(format!("/proc/{p}/cmdline"))
                .map(|c| String::from_utf8_lossy(&c).contains(marker))
                .unwrap_or(false)
        })
        .filter(|&p| lives(p))
        .collect()
}

/// A process's time on a CPU so far, in nanoseconds: the scheduler's own
/// count (`/proc/<pid>/stat`'s ticks are sampled, and can jump by two at once).
pub(super) fn cpu_ns(pid: u32) -> u64 {
    let s = std::fs::read_to_string(format!("/proc/{pid}/schedstat")).unwrap();
    s.split_whitespace().next().unwrap().parse().unwrap()
}

pub(super) fn id_of(o: &ToolOutput) -> String {
    o.meta["terminal"].as_str().unwrap().to_string()
}

/// A command that would run for minutes, and Ctrl-C: the command ends, the
/// rest of its line does not run, and the shell takes the next. The sleep's
/// seconds carry this run's pid, so another run's sleep (this test in
/// another tree at once) is never taken for this one's: with a bare `sleep
/// 4242`, a neighbour's outlived this run's Ctrl-C and failed it
/// (theseus-fps6), as in `a_close_leaves_no_child_behind` (theseus-d006).
async fn ctrl_c_interrupts_a_command(terms: &Arc<Terms>, id: &str, dir: &Path) {
    // Seven digits, so no pid's marker holds another's.
    let sleep = format!("4242.{:07}", std::process::id());
    call(
        terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": format!("sleep {sleep}; echo after\n")}),
        dir,
    )
    .await;
    let t0 = Instant::now();
    while marked(&sleep).is_empty() {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "sleep never started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    call(
        terms,
        "s1",
        SEND,
        json!({"terminal": id, "keys": ["Ctrl-C"]}),
        dir,
    )
    .await;
    let t0 = Instant::now();
    while !marked(&sleep).is_empty() {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "Ctrl-C did not interrupt the sleep"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // The prompt the interrupt draws, under the tty's `^C`, before the next
    // line is typed: typed ahead, the tty echoes it before that prompt and
    // the shell prints `back` after it, on the prompt's row (theseus-ynia).
    read_until(terms, "s1", id, "echo after\n^C\nok> ", dir).await;
    call(
        terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "echo back\n"}),
        dir,
    )
    .await;
    let text = read_until(terms, "s1", id, "\nback\n", dir).await;
    assert!(
        !text.contains("|after\n"),
        "Ctrl-C ended the line, not only the sleep:\n{text}"
    );
}

/// Another session cannot reach a terminal; its own sees its health line.
async fn another_session_cannot_reach_it(terms: &Arc<Terms>, id: &str, dir: &Path) {
    let e = fails(terms, "s2", READ, json!({"terminal": id}), dir).await;
    assert!(
        e.contains(&format!("this session has no terminal {id}")),
        "{e}"
    );
    // Health's line.
    let info = terms.get(id).unwrap().info();
    assert_eq!(
        (
            info.program.as_str(),
            info.session_id.as_str(),
            info.running
        ),
        ("sh", "s1", true)
    );
    assert!(info.bytes_in > 0 && info.bytes_out > 0);
}

/// `sh` on a pty: its prompt appears, a command's output lands on the
/// screen, a read says which rows changed, Ctrl-C interrupts a command that
/// would run for minutes, and the close leaves nothing of it running.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sh_runs_a_command_and_ctrl_c_interrupts_one() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms(&[]);
    let (o, ext) = call(
        &terms,
        "s1",
        OPEN,
        json!({"argv": ["sh"], "rows": 10, "cols": 60}),
        d.path(),
    )
    .await;
    assert!(ext.is_none());
    let id = id_of(&o);
    assert!(
        o.text.contains(&format!("Terminal {id} (sh), 10x60")),
        "{}",
        o.text
    );
    assert!(o.meta["opened"]["pid"].as_u64().unwrap() > 0);
    let pid = terms.get(&id).unwrap().pty.pid();
    // The prompt: sh prints one once it reads its terminal.
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "PS1='o''k> '\n"}),
        d.path(),
    )
    .await;
    read_until(&terms, "s1", &id, "ok> ", d.path()).await;
    call(&terms, "s1", READ, json!({"terminal": id}), d.path()).await;
    let (o, _) = call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "echo $((6*7))"}),
        d.path(),
    )
    .await;
    assert!(
        o.text
            .starts_with(&format!("Sent 13 characters of text to terminal {id} (sh)")),
        "{}",
        o.text
    );
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "keys": ["Enter"]}),
        d.path(),
    )
    .await;
    // Until the prompt after the output, not the output alone: the shell draws
    // `ok>` on the next row a moment after `42`, and a read that returned
    // between the two would see that prompt as a change (theseus-y6zr).
    let text = read_until(&terms, "s1", &id, "42\nok>", d.path()).await;
    assert!(text.contains("|ok> echo $((6*7))\n"), "{text}");
    assert!(text.contains("|42\n"), "{text}");
    assert!(text.contains("Changed since the last read: rows"), "{text}");
    // Nothing more comes: the next read says so.
    let (o, _) = call(
        &terms,
        "s1",
        READ,
        json!({"terminal": id, "quiet_ms": 200}),
        d.path(),
    )
    .await;
    assert!(
        o.text.contains("Nothing changed since the last read."),
        "{}",
        o.text
    );
    assert!(o.text.contains("Waited: quiet for 200 ms"), "{}", o.text);
    ctrl_c_interrupts_a_command(&terms, &id, d.path()).await;
    another_session_cannot_reach_it(&terms, &id, d.path()).await;
    let (o, _) = call(&terms, "s1", CLOSE, json!({"terminal": id}), d.path()).await;
    assert!(
        o.text.starts_with(&format!("Closed terminal {id} (sh): ")),
        "{}",
        o.text
    );
    assert_eq!(o.meta["closed"]["by"], "term.close");
    assert!(!lives(pid), "the shell outlived its close");
    assert!(terms.get(&id).is_none());
    let e = fails(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "x"}),
        d.path(),
    )
    .await;
    assert!(e.contains("no terminal"), "{e}");
}

/// Python's REPL computes, and its `>>>` prompt is on the screen.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn python3s_repl_computes_on_the_screen() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms(&[]);
    let (o, _) = call(
        &terms,
        "s1",
        OPEN,
        json!({"argv": ["python3", "-q"]}),
        d.path(),
    )
    .await;
    let id = id_of(&o);
    read_until(&terms, "s1", &id, ">>> ", d.path()).await;
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "sum(range(101)) * 3\n"}),
        d.path(),
    )
    .await;
    let text = read_until(&terms, "s1", &id, "15150", d.path()).await;
    assert!(text.contains("|>>> sum(range(101)) * 3\n"), "{text}");
    // A line that never ends, and Ctrl-C: Python says KeyboardInterrupt. Each
    // key goes at the state it is meant for, never after a sleep: a key typed
    // ahead can land before Python reads it as meant (theseus-1n2y).
    let pid = terms.get(&id).unwrap().pty.pid();
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "while True: pass\n"}),
        d.path(),
    )
    .await;
    read_until(&terms, "s1", &id, "while True: pass\n... ", d.path()).await;
    let before = cpu_ns(pid);
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "keys": ["Enter"]}),
        d.path(),
    )
    .await;
    // The loop runs: Python's time on a CPU rises by 20 ms, more than its read
    // of a line takes.
    let t0 = Instant::now();
    while cpu_ns(pid) < before + 20_000_000 {
        assert!(t0.elapsed() < Duration::from_secs(30), "the loop never ran");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "keys": ["Ctrl-C"]}),
        d.path(),
    )
    .await;
    read_until(&terms, "s1", &id, "KeyboardInterrupt", d.path()).await;
    // The fresh prompt after it: a Ctrl-D typed before Python reads its next
    // line reaches it as nothing, and Python waits on.
    read_until(&terms, "s1", &id, "KeyboardInterrupt\n>>> ", d.path()).await;
    // Ctrl-D ends it: the read says its program exited.
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "keys": ["Ctrl-D"]}),
        d.path(),
    )
    .await;
    let (o, _) = call(
        &terms,
        "s1",
        READ,
        json!({"terminal": id, "quiet_ms": 5_000, "timeout_ms": 10_000}),
        d.path(),
    )
    .await;
    assert!(o.text.contains("Waited: its program ended"), "{}", o.text);
    let t0 = Instant::now();
    loop {
        let (o, _) = call(&terms, "s1", READ, json!({"terminal": id}), d.path()).await;
        // Python ends by SIGINT once a KeyboardInterrupt was its last.
        if o.text.contains("its program exited (0)") || o.text.contains("ended by signal 2") {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "{}", o.text);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let e = fails(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "1\n"}),
        d.path(),
    )
    .await;
    assert!(e.contains("program has ended"), "{e}");
    let closed = terms.close_session("s1", BY_SESSION_END);
    assert_eq!(closed.len(), 1);
    assert!(
        closed[0].exit == Some(0) || closed[0].signal == Some(2),
        "{:?}",
        closed[0]
    );
}

/// `cat` echoes what it is sent, and Ctrl-C ends it; a terminal whose
/// program `[policy] external_programs` lists is outside text, every read
/// marked, and one sent text that names a listed program becomes so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cat_echoes_and_a_listed_programs_screen_is_outside_text() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms(&["cat", "gh"]);
    let (o, ext) = call(&terms, "s1", OPEN, json!({"argv": ["/bin/cat"]}), d.path()).await;
    let id = id_of(&o);
    assert_eq!(
        ext.map(|e| e.url),
        Some("cat".to_string()),
        "its open's screen is outside text"
    );
    assert_eq!(o.meta[crate::external::PROGRAM_KEY], "cat");
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "hello there\n"}),
        d.path(),
    )
    .await;
    let text = read_until(&terms, "s1", &id, "hello there\nhello there", d.path()).await;
    assert!(
        text.starts_with("[it runs cat, which [policy] external_programs lists"),
        "{text}"
    );
    let (_, ext) = call(&terms, "s1", READ, json!({"terminal": id}), d.path()).await;
    assert!(ext.is_some(), "every read of it is marked");
    assert_eq!(
        terms.get(&id).unwrap().info().external.as_deref(),
        Some("cat")
    );
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "keys": ["Ctrl-C"]}),
        d.path(),
    )
    .await;
    let (o, _) = call(
        &terms,
        "s1",
        READ,
        json!({"terminal": id, "quiet_ms": 5_000, "timeout_ms": 10_000}),
        d.path(),
    )
    .await;
    assert!(o.text.contains("its program ended"), "{}", o.text);
    // A shell is not listed, until it is sent a listed program's name.
    let (o, ext) = call(&terms, "s1", OPEN, json!({"argv": ["sh"]}), d.path()).await;
    let sh = id_of(&o);
    assert!(ext.is_none());
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": sh, "text": "echo plain\n"}),
        d.path(),
    )
    .await;
    let (_, ext) = call(&terms, "s1", READ, json!({"terminal": sh}), d.path()).await;
    assert!(ext.is_none());
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": sh, "text": "gh issue view 1 || true\n"}),
        d.path(),
    )
    .await;
    let (o, ext) = call(&terms, "s1", READ, json!({"terminal": sh}), d.path()).await;
    assert!(ext.is_some(), "{}", o.text);
    assert_eq!(o.meta[crate::external::PROGRAM_KEY], "gh");
    assert_eq!(terms.close_session("s1", BY_SESSION_END).len(), 2);
}

/// At most four terminals a session; a fifth is refused with why, and
/// another session still opens its own. A bad size or key is invalid input.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_holds_four_terminals() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms(&[]);
    for _ in 0..PER_SESSION {
        call(
            &terms,
            "s1",
            OPEN,
            json!({"argv": ["cat"], "quiet_ms": 0}),
            d.path(),
        )
        .await;
    }
    let e = fails(&terms, "s1", OPEN, json!({"argv": ["cat"]}), d.path()).await;
    assert!(
        e.contains("this session has 4 terminals running, the most it may"),
        "{e}"
    );
    call(
        &terms,
        "s2",
        OPEN,
        json!({"argv": ["cat"], "quiet_ms": 0}),
        d.path(),
    )
    .await;
    let t = tools::Open;
    assert!(
        theseus_tools::Tool::plan(&t, &json!({"argv": ["cat"], "rows": 1}), &ctx(d.path()))
            .is_err()
    );
    let send = tools::Send(terms.clone());
    let e = theseus_tools::Tool::plan(
        &send,
        &json!({"terminal": "t1", "keys": ["Hyper-X"]}),
        &ctx(d.path()),
    )
    .unwrap_err();
    assert!(e.contains("unknown key \"Hyper-X\""), "{e}");
    let e = fails(
        &terms,
        "s3",
        OPEN,
        json!({"argv": ["no-such-program-x"]}),
        d.path(),
    )
    .await;
    assert!(e.contains("could not start no-such-program-x"), "{e}");
    let pids: Vec<u32> = terms.all().iter().map(|t| t.pty.pid()).collect();
    assert_eq!(pids.len(), 5);
    let closed = terms.close_where(BY_DAEMON, |_| true);
    assert_eq!(closed.len(), 5);
    assert!(
        pids.iter().all(|&p| !lives(p)),
        "a program outlived the daemon's stop"
    );
}

/// A close reaches what the program started, its own process group or not:
/// a background child, and one that left with a session of its own. The
/// sleeps' seconds carry this run's pid, so no other run's sleep (another
/// tree's, or one an earlier run left, which lives 72 minutes) is taken for
/// this one's, and a survivor is named with where it came from (theseus-d006).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_close_leaves_no_child_behind() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms(&[]);
    let (o, _) = call(&terms, "s1", OPEN, json!({"argv": ["sh"]}), d.path()).await;
    let id = id_of(&o);
    // Seven digits, so no pid's marker holds another's.
    let run = format!("{:07}", std::process::id());
    let (ignores, own) = (format!("4343.{run}"), format!("4344.{run}"));
    // One ignores the hang-up and SIGTERM; one runs in a session of its own.
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id,
        "text": format!("trap '' HUP TERM; sleep {ignores} & setsid sleep {own} & echo started\n")}),
        d.path(),
    )
    .await;
    read_until(&terms, "s1", &id, "started\n", d.path()).await;
    let t0 = Instant::now();
    while marked(&ignores).is_empty() || marked(&own).is_empty() {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "the sleeps never started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let closed = tokio::task::spawn_blocking({
        let terms = terms.clone();
        move || terms.close_session("s1", BY_CANCEL)
    })
    .await
    .unwrap();
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].by, BY_CANCEL);
    let t0 = Instant::now();
    loop {
        let left: Vec<u32> = marked(&ignores).into_iter().chain(marked(&own)).collect();
        if left.is_empty() {
            break;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "a child outlived its terminal:\n{}",
            left.iter()
                .map(|&p| described(p))
                .collect::<Vec<_>>()
                .join("\n")
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A process as a failure names it: its pid, parent, session, process group,
/// state, start (clock ticks after boot), cgroup, and command line.
pub(super) fn described(pid: u32) -> String {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    let f: Vec<&str> = stat
        .rfind(')')
        .map(|i| stat[i + 1..].split_whitespace().collect())
        .unwrap_or_default();
    let field = |i: usize| f.get(i).copied().unwrap_or("?");
    let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).unwrap_or_default();
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    format!(
        "pid {pid} ppid {} session {} pgrp {} state {} start {} cgroup {} cmdline {:?}",
        field(1),
        field(3),
        field(2),
        field(0),
        field(19),
        cgroup.trim(),
        String::from_utf8_lossy(&cmdline).replace('\0', " ")
    )
}
