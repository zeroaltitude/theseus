//! The client against the scripted fake: in this process over pipes, and as
//! the `theseus-lsp-fake` binary where a real process matters (the stop's
//! kill, a crash).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_lsp::fake::{Config, Diagnostics, Fake};
use theseus_lsp::spawn::{self, Process};
use theseus_lsp::types::{DocumentSymbols, Position};
use theseus_lsp::{locate, Client, Error, Event, Events, Freshness, Options, Server};

const BIN: &str = env!("CARGO_BIN_EXE_theseus-lsp-fake");

fn options(root: &Path) -> Options {
    let mut o = Options::new(root);
    o.request_timeout = Duration::from_secs(10);
    o
}

async fn in_process(cfg: Config, opts: Options) -> (Client, Events) {
    let (ours, theirs) = tokio::io::duplex(1 << 20);
    let (cr, cw) = tokio::io::split(ours);
    let (sr, sw) = tokio::io::split(theirs);
    tokio::spawn(Fake::serve(cfg, sr, sw));
    Client::start(Server::pipes(cr, cw), opts)
        .await
        .expect("the fake starts")
}

async fn process(args: &[&str], root: &Path) -> (Client, Events, Process) {
    let mut argv = vec![BIN.to_string()];
    argv.extend(args.iter().map(|a| (*a).to_string()));
    let (server, proc) = spawn::spawn(&argv, root, &[], Stdio::null()).expect("the fake spawns");
    let (c, e) = Client::start(server, options(root))
        .await
        .expect("the fake starts");
    (c, e, proc)
}

async fn seen(c: &Client) -> Value {
    c.request_within("fake/seen", Value::Null, Duration::from_secs(5))
        .await
        .expect("the fake says what it saw")
}

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p
}

fn uri(p: &Path) -> String {
    theseus_lsp::uri::from_path(p).unwrap()
}

#[tokio::test]
async fn answers_go_to_their_callers_however_they_interleave() {
    let dir = tempfile::tempdir().unwrap();
    let words: [&'static str; 6] = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"];
    let text = words.join(" ");
    let a = write(dir.path(), "a.fake", &text);
    let cfg = Config {
        slow_ms: 30,
        ..Config::default()
    };
    let (c, _e) = in_process(cfg, options(dir.path())).await;
    c.open(&a).await.unwrap();
    let asks = words.into_iter().map(|w| {
        let c = c.clone();
        let a = a.clone();
        let pos = locate(&text, 1, w, None).unwrap();
        async move { (w, c.hover(&a, pos).await.unwrap().unwrap().text()) }
    });
    for (w, hover) in futures_join(asks).await {
        assert_eq!(hover, format!("`{w}`: a fake symbol"));
    }
    assert_eq!(c.in_flight(), 0);
}

/// Run the futures at once, in order.
async fn futures_join<F: std::future::Future + Send + 'static>(
    fs: impl Iterator<Item = F>,
) -> Vec<F::Output>
where
    F::Output: Send + 'static,
{
    let handles: Vec<_> = fs.map(tokio::spawn).collect();
    let mut out = Vec::new();
    for h in handles {
        out.push(h.await.unwrap());
    }
    out
}

#[tokio::test]
async fn the_servers_own_requests_are_answered() {
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(dir.path());
    opts.settings = json!({ "fake": { "check": { "strict": true } } });
    let cfg = Config {
        ask: true,
        ..Config::default()
    };
    let (c, _e) = in_process(cfg, opts).await;
    // The fake asks after `initialized`, then begins a progress it never
    // ends: once it has its five answers, the progress has arrived too.
    let mut answers = Value::Null;
    for _ in 0..100 {
        answers = seen(&c).await["answers"].clone();
        if answers.as_object().is_some_and(|a| a.len() == 5) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let r = c.wait_ready(Duration::from_millis(100)).await;
    let r = r.expect_err("a progress still running is not ready");
    assert_eq!(
        r.running.get("indexing").map(String::as_str),
        Some("Indexing")
    );
    assert_eq!(
        answers["workspace/configuration"],
        json!([{ "strict": true }, null, { "fake": { "check": { "strict": true } } }])
    );
    assert_eq!(answers["client/registerCapability"], Value::Null);
    assert_eq!(answers["window/workDoneProgress/create"], Value::Null);
    assert_eq!(answers["workspace/applyEdit"]["applied"], json!(false));
    assert_eq!(answers["fake/unknownRequest"], json!({ "error": -32601 }));
}

#[tokio::test]
async fn pushed_diagnostics_follow_the_version_and_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    let a = write(dir.path(), "a.fake", "fn total\nERROR here\n");
    let (c, _e) = in_process(Config::default(), options(dir.path())).await;
    let d = c.diagnostics(&a, Duration::from_secs(5)).await.unwrap();
    assert_eq!((d.version, d.freshness), (1, Freshness::Pushed));
    assert_eq!(d.errors().count(), 1);
    assert_eq!(d.items[0].range.start, Position::new(1, 0));
    // Fixed on disk by someone else: the next call sends it again first.
    std::fs::write(&a, "fn total\nfine now, longer\n").unwrap();
    let d = c.diagnostics(&a, Duration::from_secs(5)).await.unwrap();
    assert_eq!(
        (d.version, d.freshness, d.items.len()),
        (2, Freshness::Pushed, 0)
    );
    let versions = seen(&c).await["versions"].clone();
    assert_eq!(versions, json!([[uri(&a), 1], [uri(&a), 2]]));
    assert_eq!(c.pushed(&a).map(|p| p.0), Some(Some(2)));
}

#[tokio::test]
async fn a_push_with_no_version_counts_when_it_came_after_the_change() {
    let dir = tempfile::tempdir().unwrap();
    let a = write(dir.path(), "a.fake", "ERROR\n");
    let cfg = Config {
        versions: false,
        push_delay_ms: 100,
        ..Config::default()
    };
    let (c, _e) = in_process(cfg, options(dir.path())).await;
    let d = c.diagnostics(&a, Duration::from_secs(5)).await.unwrap();
    assert_eq!((d.freshness, d.items.len()), (Freshness::Pushed, 1));
    let v = c.change(&a, "clean\n".into()).await.unwrap();
    assert_eq!(v, 2);
    // The list for version 1 is there, but came before the change.
    let d = c.diagnostics(&a, Duration::from_secs(5)).await.unwrap();
    assert_eq!((d.freshness, d.items.len()), (Freshness::Pushed, 0));
    assert!(
        d.waited >= Duration::from_millis(90),
        "it waited for the push: {:?}",
        d.waited
    );
}

#[tokio::test]
async fn a_pull_only_server_is_pulled_and_unchanged_reuses_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let a = write(dir.path(), "a.fake", "ok\nERROR one\nERROR two\n");
    for mode in [Diagnostics::Pull, Diagnostics::PullRegistered] {
        let cfg = Config {
            diagnostics: mode,
            ..Config::default()
        };
        let (c, _e) = in_process(cfg, options(dir.path())).await;
        // PullRegistered: the registration comes 200 ms after `initialized`,
        // while this call already waits for a push that never comes.
        let d = c.diagnostics(&a, Duration::from_secs(5)).await.unwrap();
        assert_eq!(
            (d.freshness, d.items.len()),
            (Freshness::Pulled, 2),
            "{mode:?}"
        );
        let again = c.diagnostics(&a, Duration::from_secs(5)).await.unwrap();
        assert_eq!(
            (again.freshness, again.items.len()),
            (Freshness::Pulled, 2),
            "{mode:?}: unchanged, reused"
        );
        let requests = seen(&c).await["requests"].clone();
        let pulls = requests
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| *m == "textDocument/diagnostic")
            .count();
        assert_eq!(pulls, 2, "{mode:?}");
        assert!(c.pushed(&a).is_none(), "nothing was pushed");
    }
}

#[tokio::test]
async fn the_wait_is_bounded_and_says_when_it_ran_out() {
    let dir = tempfile::tempdir().unwrap();
    let a = write(dir.path(), "a.fake", "ERROR\n");
    let cfg = Config {
        push_delay_ms: 1500,
        ..Config::default()
    };
    let (c, _e) = in_process(cfg, options(dir.path())).await;
    let d = c.diagnostics(&a, Duration::from_millis(200)).await.unwrap();
    assert_eq!(d.freshness, Freshness::Stale);
    assert!(d.items.is_empty(), "nothing known yet");
    assert!(d.waited < Duration::from_millis(1000), "{:?}", d.waited);
    let d = c.diagnostics(&a, Duration::from_secs(5)).await.unwrap();
    assert_eq!((d.freshness, d.items.len()), (Freshness::Pushed, 1));
}

#[tokio::test]
async fn a_dropped_request_is_cancelled_and_a_slow_one_times_out() {
    let dir = tempfile::tempdir().unwrap();
    let a = write(dir.path(), "a.fake", "fn total\n");
    let mut opts = options(dir.path());
    opts.request_timeout = Duration::from_millis(300);
    let cfg = Config {
        slow_ms: 5_000,
        ..Config::default()
    };
    let (c, _e) = in_process(cfg, opts).await;
    let pos = Position::new(0, 4);
    // Dropped once it is in flight: the task that waits for it is aborted.
    let task = tokio::spawn({
        let (c, a) = (c.clone(), a.clone());
        async move { c.hover(&a, pos).await }
    });
    while c.in_flight() == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    task.abort();
    assert!(
        task.await.is_err_and(|e| e.is_cancelled()),
        "the caller stopped waiting"
    );
    assert_eq!((c.cancels_sent(), c.in_flight()), (1, 0));
    let timed = c.definition(&a, pos).await;
    assert!(
        matches!(timed, Err(Error::Timeout { ref method, .. }) if method == "textDocument/definition"),
        "{timed:?}"
    );
    assert_eq!((c.cancels_sent(), c.in_flight()), (2, 0));
    let s = seen(&c).await;
    assert_eq!(s["cancels"].as_array().unwrap().len(), 2, "{s}");
}

#[tokio::test]
async fn navigation_across_files_and_a_rename_that_is_not_applied() {
    let dir = tempfile::tempdir().unwrap();
    let a_text = "use b\nlet n = total(items)\nlet m = total(more)\n";
    let a = write(dir.path(), "a.fake", a_text);
    let b = write(dir.path(), "b.fake", "fn total(items)\n");
    let (c, _e) = in_process(Config::default(), options(dir.path())).await;
    let pos = locate(a_text, 2, "total", None).unwrap();
    let defs = c.definition(&a, pos).await.unwrap();
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].uri, uri(&b));
    assert_eq!(defs[0].range.start, Position::new(0, 3));
    let refs = c.references(&a, pos, true).await.unwrap();
    assert_eq!(refs.len(), 3);
    let syms = c.document_symbols(&b).await.unwrap();
    assert!(matches!(&syms, DocumentSymbols::Nested(s) if s[0].name == "total"));
    let ws = c.workspace_symbols("tot").await.unwrap();
    assert_eq!(ws.len(), 1);
    let edit = c.rename(&a, pos, "sum").await.unwrap().unwrap();
    let edits = edit.text_edits();
    assert_eq!(edits.len(), 2);
    for (u, version, e) in &edits {
        assert!(e.iter().all(|t| t.new_text == "sum"));
        // Both were opened (b by its symbols), at version 1.
        assert_eq!(*version, Some(1), "{u}");
    }
    assert_eq!(
        std::fs::read_to_string(&a).unwrap(),
        a_text,
        "never applied"
    );
}

#[tokio::test]
async fn a_written_file_is_sent_again_saved_and_watched() {
    let dir = tempfile::tempdir().unwrap();
    let a = write(dir.path(), "a.fake", "one\n");
    let (c, _e) = in_process(Config::default(), options(dir.path())).await;
    c.open(&a).await.unwrap();
    std::fs::write(&a, "one two\n").unwrap();
    c.file_changed(&a).await.unwrap();
    let fresh = write(dir.path(), "new.fake", "fn fresh\n");
    c.file_changed(&fresh).await.unwrap();
    std::fs::remove_file(&a).unwrap();
    c.file_changed(&a).await.unwrap();
    let s = seen(&c).await;
    assert_eq!(s["versions"], json!([[uri(&a), 1], [uri(&a), 2]]));
    assert_eq!(s["saves"], json!([uri(&a)]));
    assert_eq!(s["closes"], json!([uri(&a)]));
    assert_eq!(
        s["watched"],
        json!([[uri(&a), 2], [uri(&fresh), 1], [uri(&a), 3]])
    );
    assert_eq!(c.open_version(&a), None);
}

/// The client watches no files for a server, so it never offers to
/// (theseus-m9hj): a server offered dynamic registration of watched files
/// stops watching for itself, as rust-analyzer does, and then misses what a
/// job writes. The client's own writes it still announces (above).
#[tokio::test]
async fn the_client_never_offers_to_watch_files_for_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let (c, _e) = in_process(Config::default(), options(dir.path())).await;
    let s = seen(&c).await;
    let watched = &s["capabilities"]["workspace"]["didChangeWatchedFiles"];
    assert!(watched.is_object(), "{s}");
    assert_ne!(watched["dynamicRegistration"], json!(true), "{watched}");
}

#[tokio::test]
async fn a_change_on_disk_is_sent_before_the_next_request() {
    let dir = tempfile::tempdir().unwrap();
    let a = write(dir.path(), "a.fake", "fn total\n");
    let (c, _e) = in_process(Config::default(), options(dir.path())).await;
    c.open(&a).await.unwrap();
    std::fs::write(&a, "\nfn total\n").unwrap();
    let h = c.hover(&a, Position::new(1, 4)).await.unwrap().unwrap();
    assert_eq!(
        h.text(),
        "`total`: a fake symbol",
        "the hover read the new text"
    );
    assert_eq!(c.open_version(&a), Some(2));
}

#[tokio::test]
async fn a_clean_stop_needs_no_kill() {
    let dir = tempfile::tempdir().unwrap();
    let (c, _e, mut proc) = process(&[], dir.path()).await;
    let s = c.stop().await;
    assert!(s.shutdown_answered && s.exited && !s.killed, "{s:?}");
    let status = tokio::time::timeout(
        Duration::from_secs(5),
        proc.status.wait_for(Option::is_some),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    assert!(status.success(), "{status}");
    assert!(c.closed().is_some());
}

#[tokio::test]
async fn a_server_that_ignores_exit_is_killed_after_the_grace() {
    let dir = tempfile::tempdir().unwrap();
    let (c, _e, mut proc) = process(&["--ignore-exit"], dir.path()).await;
    let s = c.stop().await;
    assert!(s.shutdown_answered && !s.exited && s.killed, "{s:?}");
    assert!(s.took >= Duration::from_secs(1), "the grace: {s:?}");
    let status = tokio::time::timeout(
        Duration::from_secs(5),
        proc.status.wait_for(Option::is_some),
    )
    .await
    .expect("the server is gone")
    .unwrap()
    .unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(libc_sigkill()), "{status}");
}

fn libc_sigkill() -> i32 {
    9
}

#[tokio::test]
async fn a_crash_fails_the_waiting_request_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let a = write(dir.path(), "a.fake", "fn total\n");
    let (c, mut events, mut proc) = process(&["--crash-after", "1"], dir.path()).await;
    let r = c.definition(&a, Position::new(0, 4)).await;
    assert!(matches!(r, Err(Error::Closed(_))), "{r:?}");
    let mut closed = None;
    while let Ok(Some(e)) = tokio::time::timeout(Duration::from_secs(5), events.recv()).await {
        if let Event::Closed { reason } = e {
            closed = Some(reason);
            break;
        }
    }
    // Its stdout's end (between messages, or inside one it was writing when
    // it exited), or a write to its closed stdin, whichever comes first.
    let closed = closed.expect("the end is an event");
    assert!(
        closed.starts_with("the server closed its stdout")
            || closed.starts_with("writing to the server failed"),
        "{closed}"
    );
    let status = proc
        .status
        .wait_for(Option::is_some)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.code(), Some(3));
    // A request after the end fails at once.
    let r = c.hover(&a, Position::new(0, 4)).await;
    assert!(matches!(r, Err(Error::Closed(_))), "{r:?}");
    let s = c.stop().await;
    assert!(!s.killed && !s.shutdown_answered, "{s:?}");
}

#[tokio::test]
async fn a_dropped_client_kills_its_server() {
    let dir = tempfile::tempdir().unwrap();
    let (c, _e, mut proc) = process(&["--ignore-exit"], dir.path()).await;
    drop(c);
    let status = tokio::time::timeout(
        Duration::from_secs(5),
        proc.status.wait_for(Option::is_some),
    )
    .await
    .expect("the server is gone")
    .unwrap()
    .unwrap();
    assert!(!status.success(), "{status}");
}
