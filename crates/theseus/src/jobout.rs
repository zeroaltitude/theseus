//! `theseus peek` and `theseus attach` (theseus-e1hu4): a running job's
//! output, once and live. Both name the job as a work view does (`act_…`, or a
//! unique end of it of at least four characters, as `theseus tasks` prints).
//!
//! - `peek` is one `work.peek`: the size of the output and its last lines.
//! - `attach` peeks, then follows (`action.output`) from where the peek
//!   ended and prints each chunk as it comes, until the job ends. Ctrl-C
//!   detaches (`action.unfollow`) and the job goes on; it never stops the
//!   job. A stream that fell behind (`events.lost`) says so, peeks again and
//!   follows from there; a connection that closes says so and exits 1.

use std::future::Future;
use std::io::Write;

use anyhow::{anyhow, bail, Result};
use clap::Args;
use serde_json::{json, Value};
use theseus_client::Conn;
use theseus_protocol::job_follow::job_words;
use theseus_protocol::work::{WorkKind, WorkView};
use theseus_protocol::{
    method, notify, ActionOutputParams, ActionOutputResult, Event, Message, WorkPeekParams,
    WorkPeekResult, WorkWatchParams, WorkWatchResult,
};

#[derive(Args, Debug)]
pub struct PeekArgs {
    /// The job: its id (`act_…`) or a unique end of it, at least four characters.
    #[arg(value_name = "JOB")]
    job: String,
}

#[derive(Args, Debug)]
pub struct AttachArgs {
    /// The job: its id (`act_…`) or a unique end of it, at least four characters.
    #[arg(value_name = "JOB")]
    job: String,
}

/// The job a person named: a whole id as it is, else the one running job of
/// the work board whose id ends with it.
async fn resolve(conn: &mut Conn, given: &str) -> Result<String> {
    let given = theseus_client::resolve::trimmed(given);
    if theseus_client::resolve::whole(given) {
        return Ok(given.to_string());
    }
    theseus_client::resolve::long_enough(given, "job")?;
    let v = conn
        .request(
            method::WORK_WATCH,
            WorkWatchParams {
                limit: Some(0),
                ..Default::default()
            },
        )
        .await?;
    let _ = conn.request(method::WORK_UNWATCH, json!({})).await;
    let snap: WorkWatchResult = serde_json::from_value(v)?;
    one_job(given, &snap.views)
}

/// The one job among `views` whose id ends with `given`.
fn one_job(given: &str, views: &[WorkView]) -> Result<String> {
    let found: Vec<&WorkView> = views
        .iter()
        .filter(|v| v.kind == WorkKind::Job && v.id.ends_with(given))
        .collect();
    match found.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => bail!("no job on the work board ends with {given:?} (`theseus tasks` lists them)"),
        many => bail!(
            "{given:?} ends {} jobs: {}",
            many.len(),
            many.iter()
                .map(|v| v.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

async fn peek_of(conn: &mut Conn, id: &str) -> Result<(Value, WorkPeekResult)> {
    let v = conn
        .request(method::WORK_PEEK, WorkPeekParams { id: id.into() })
        .await?;
    let r: WorkPeekResult = serde_json::from_value(v.clone())?;
    Ok((v, r))
}

/// A peek in words: the size, whether the job has ended, then its last lines.
pub fn peek_lines(p: &WorkPeekResult) -> Vec<String> {
    let state = match (p.complete, p.started_at_ms) {
        (true, _) => "ended".to_string(),
        (false, Some(s)) => job_words(p.at_ms.saturating_sub(s), p.usual_ms),
        (false, None) => "running".to_string(),
    };
    let mut out = vec![format!(
        "{} · {} · last {} of {}",
        p.id,
        state,
        lines_word(p.lines.len()),
        bytes(p.bytes)
    )];
    out.extend(p.lines.iter().map(|l| format!("  {l}")));
    out
}

fn lines_word(n: usize) -> String {
    match n {
        1 => "1 line".to_string(),
        n => format!("{n} lines"),
    }
}

fn bytes(n: u64) -> String {
    match n {
        0..=1023 => format!("{n} B"),
        1024..=1_048_575 => format!("{:.1} KiB", n as f64 / 1024.0),
        _ => format!("{:.1} MiB", n as f64 / 1_048_576.0),
    }
}

/// The work board's commands: `work`, `peek` and `attach`, from one arm of
/// the CLI's `run`, which stays within clippy's length that way.
pub async fn board(conn: &mut Conn, json: bool, cmd: crate::Cmd) -> Result<()> {
    match cmd {
        crate::Cmd::Work(a) => crate::tasks_tree::run(conn, json, a.into_tasks()).await,
        crate::Cmd::Peek(a) => peek(conn, json, a).await,
        crate::Cmd::Attach(a) => attach(conn, json, a).await,
        _ => unreachable!("routed by `run`"),
    }
}

/// `theseus peek JOB`.
async fn peek(conn: &mut Conn, json: bool, a: PeekArgs) -> Result<()> {
    let id = resolve(conn, &a.job).await?;
    let (v, p) = peek_of(conn, &id).await?;
    if json {
        println!("{}", json!({"result": v}));
    } else {
        for l in peek_lines(&p) {
            println!("{l}");
        }
    }
    Ok(())
}

/// `theseus attach JOB`: follow until the job ends or Ctrl-C.
async fn attach(conn: &mut Conn, json: bool, a: AttachArgs) -> Result<()> {
    let id = resolve(conn, &a.job).await?;
    let mut out = std::io::stdout();
    let end = follow(conn, &id, &mut out, json, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    if end == Ended::Dropped {
        std::process::exit(1);
    }
    Ok(())
}

/// How a follow ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Ended {
    /// The job ended, with its outcome.
    Job(String),
    /// The reader detached: the job goes on.
    Detached,
    /// The connection closed.
    Dropped,
}

/// Peek, follow from the peek's end, and print until `detach` resolves or the
/// job ends.
pub async fn follow(
    conn: &mut Conn,
    id: &str,
    out: &mut impl Write,
    json: bool,
    detach: impl Future<Output = ()>,
) -> Result<Ended> {
    tokio::pin!(detach);
    let (_, p) = peek_of(conn, id).await?;
    if !json {
        for l in peek_lines(&p) {
            writeln!(out, "{l}")?;
        }
        writeln!(out, "-- following (Ctrl-C detaches; the job goes on) --")?;
    }
    let mut next = p.bytes;
    let mut early: Vec<Message> = Vec::new();
    start(conn, id, next, &mut early).await?;
    for m in early {
        if let Some(e) = show(m, id, &mut next, out, json)? {
            return Ok(e);
        }
    }
    loop {
        tokio::select! {
            () = &mut detach => {
                let _ = conn
                    .request(method::ACTION_UNFOLLOW, json!({"id": id}))
                    .await;
                if !json {
                    writeln!(out, "\n-- detached; the job goes on --")?;
                }
                return Ok(Ended::Detached);
            }
            m = conn.next() => {
                let Some(m) = m? else {
                    if !json {
                        writeln!(out, "\n-- the connection closed; `theseus attach` joins again --")?;
                    }
                    return Ok(Ended::Dropped);
                };
                if let Message::Notification(n) = &m {
                    if n.method == notify::EVENTS_LOST && lost_ours(&n.params, id) {
                        // Fell behind: say so, read what it printed since, follow on.
                        if !json {
                            writeln!(out, "\n-- fell behind; reading the job again --")?;
                        }
                        let (_, p) = peek_of(conn, id).await?;
                        if !json {
                            for l in peek_lines(&p) {
                                writeln!(out, "{l}")?;
                            }
                        }
                        next = p.bytes;
                        let mut early = Vec::new();
                        start(conn, id, next, &mut early).await?;
                        for m in early {
                            if let Some(e) = show(m, id, &mut next, out, json)? {
                                return Ok(e);
                            }
                        }
                        continue;
                    }
                }
                if let Some(e) = show(m, id, &mut next, out, json)? {
                    return Ok(e);
                }
            }
        }
    }
}

/// `action.output` from `from`, keeping the notifications that arrive before
/// its answer.
async fn start(conn: &mut Conn, id: &str, from: u64, early: &mut Vec<Message>) -> Result<()> {
    let v = conn
        .call(
            method::ACTION_OUTPUT,
            serde_json::to_value(ActionOutputParams {
                id: id.into(),
                from: Some(from),
            })?,
            |m, p| {
                early.push(Message::Notification(theseus_protocol::Notification::new(
                    m,
                    p.clone(),
                )))
            },
        )
        .await?;
    let _: ActionOutputResult = serde_json::from_value(v)?;
    Ok(())
}

/// Whether an `events.lost` names this job's stream.
fn lost_ours(params: &Value, id: &str) -> bool {
    let want = format!("output:{id}");
    params["streams"]
        .as_array()
        .is_some_and(|s| s.iter().any(|x| x.as_str() == Some(want.as_str())))
}

/// One message: a chunk of this job is printed (a gap before it said), its end
/// finishes the follow.
fn show(
    m: Message,
    id: &str,
    next: &mut u64,
    out: &mut impl Write,
    json: bool,
) -> Result<Option<Ended>> {
    let Message::Notification(n) = m else {
        return Ok(None);
    };
    if json && (n.method == notify::ACTION_OUTPUT_CHUNK || n.method == notify::ACTION_OUTPUT_END) {
        println!("{}", json!({"method": n.method, "params": n.params}));
    }
    match Event::from_notification(&n.method, &n.params)
        .map_err(|e| anyhow!("a bad {}: {e}", n.method))?
    {
        Some(Event::ActionOutputChunk(c)) if c.id == id => {
            // A chunk the old follower queued before a re-peek begins before
            // the peek's end: what was shown is not shown again.
            let end = c.offset + c.text.len() as u64;
            if end <= *next {
                return Ok(None);
            }
            let text = match c.offset < *next {
                true => c.text.get((*next - c.offset) as usize..).unwrap_or(""),
                false => c.text.as_str(),
            };
            if !json {
                if c.offset > *next {
                    writeln!(out, "-- {} skipped --", bytes(c.offset - *next))?;
                }
                write!(out, "{text}")?;
                out.flush()?;
            }
            *next = end;
            Ok(None)
        }
        Some(Event::ActionOutputEnd(e)) if e.id == id => {
            if !json {
                writeln!(out, "-- the job ended: {} --", e.outcome)?;
            }
            Ok(Some(Ended::Job(e.outcome)))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    fn job(id: &str, kind: &str) -> WorkView {
        serde_json::from_value(json!({
            "id": id, "kind": kind, "position": 1, "at_ms": 1, "root": "exe_1",
            "title": "", "state": "running",
            "attention": {"level": "working", "label": "running", "since_ms": 1},
            "started_at_ms": 1, "state_since_ms": 1
        }))
        .unwrap()
    }

    #[test]
    fn a_job_is_named_whole_or_by_a_unique_end() {
        let views = vec![
            job("act_aaaa1111", "job"),
            job("act_bbbb2222", "job"),
            job("act_cccc2222", "question"),
        ];
        assert_eq!(one_job("1111", &views).unwrap(), "act_aaaa1111");
        // A question is no job; an end that names none is refused.
        assert!(one_job("cccc2222", &views).is_err());
        // An end that names two is refused, naming both.
        let two = vec![job("act_aaaa2222", "job"), job("act_bbbb2222", "job")];
        let e = one_job("2222", &two).unwrap_err().to_string();
        assert!(
            e.contains("act_aaaa2222") && e.contains("act_bbbb2222"),
            "{e}"
        );
    }

    #[test]
    fn a_peek_reads_as_its_size_and_its_last_lines() {
        let p = WorkPeekResult {
            id: "act_k5".into(),
            bytes: 2048,
            lines: vec!["leg 7".into(), "leg 8".into()],
            at_ms: 1,
            complete: false,
            started_at_ms: None,
            usual_ms: None,
        };
        assert_eq!(
            peek_lines(&p),
            vec![
                "act_k5 · running · last 2 lines of 2.0 KiB",
                "  leg 7",
                "  leg 8"
            ]
        );
        // With its dispatch and its program's usual time, it reads as every surface does.
        let p = WorkPeekResult {
            lines: vec!["leg 7".into()],
            bytes: 12,
            at_ms: 6_000,
            started_at_ms: Some(1_000),
            usual_ms: Some(31_000),
            ..p
        };
        assert_eq!(
            peek_lines(&p)[0],
            "act_k5 · job 0:05 of usual 0:31 · last 1 line of 12 B"
        );
    }

    /// A chunk that begins before what was shown is not shown again, nor is
    /// its overlap: a re-peek leaves the old follower's chunks in flight.
    #[test]
    fn a_chunk_the_peek_already_covered_is_not_printed_again() {
        let chunk = |offset: u64, text: &str| {
            Message::Notification(theseus_protocol::Notification::new(
                notify::ACTION_OUTPUT_CHUNK,
                json!({"id": "act_k5", "offset": offset, "text": text}),
            ))
        };
        let mut out = Vec::new();
        let mut next = 12u64;
        for m in [
            chunk(0, "leg 1\nleg 2\n"),
            chunk(6, "leg 2\nleg 3\n"),
            chunk(18, "leg 4\n"),
        ] {
            assert!(show(m, "act_k5", &mut next, &mut out, false)
                .unwrap()
                .is_none());
        }
        assert_eq!(String::from_utf8(out).unwrap(), "leg 3\nleg 4\n");
        assert_eq!(next, 24);
    }

    /// A scripted daemon: it answers the peek and the follow, sends two
    /// chunks, and records every method it is asked.
    async fn daemon(
        stream: tokio::io::DuplexStream,
        asked: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        end: bool,
    ) {
        let (r, mut w) = tokio::io::split(stream);
        let mut lines = BufReader::new(r).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let req: Value = serde_json::from_str(&line).unwrap();
            let m = req["method"].as_str().unwrap().to_string();
            asked.lock().unwrap().push(m.clone());
            let result = match m.as_str() {
                "work.peek" => json!({"id": "act_k5", "bytes": 12, "lines": ["leg 1", "leg 2"],
                    "at_ms": 1, "complete": false}),
                "action.output" => json!({"id": "act_k5", "offset": 12, "bytes": 12,
                    "complete": false}),
                _ => json!({"was_following": true}),
            };
            let mut out = String::new();
            if m == "action.output" {
                for (off, text) in [(12, "leg 3\n"), (18, "leg 4\n")] {
                    out += &json!({"jsonrpc": "2.0", "method": "action.output.chunk",
                        "params": {"id": "act_k5", "offset": off, "text": text}})
                    .to_string();
                    out.push('\n');
                }
                if end {
                    out += &json!({"jsonrpc": "2.0", "method": "action.output.end",
                        "params": {"id": "act_k5", "outcome": "succeeded"}})
                    .to_string();
                    out.push('\n');
                }
            }
            out += &json!({"jsonrpc": "2.0", "id": req["id"], "result": result}).to_string();
            out.push('\n');
            w.write_all(out.as_bytes()).await.unwrap();
        }
    }

    /// Attach prints the peek, then each chunk in order, then the end.
    #[tokio::test]
    async fn attach_prints_the_peek_then_the_chunks_in_order_then_the_end() {
        let (a, b) = tokio::io::duplex(1 << 16);
        let asked = Default::default();
        tokio::spawn(daemon(b, std::sync::Arc::clone(&asked), true));
        let (r, w) = tokio::io::split(a);
        let mut conn = Conn::over(r, w);
        let mut out = Vec::new();
        let end = follow(&mut conn, "act_k5", &mut out, false, std::future::pending())
            .await
            .unwrap();
        assert_eq!(end, Ended::Job("succeeded".into()));
        let text = String::from_utf8(out).unwrap();
        let at = |s: &str| text.find(s).unwrap_or_else(|| panic!("{s:?} in {text}"));
        assert!(at("leg 2") < at("-- following") && at("-- following") < at("leg 3\n"));
        assert!(at("leg 3\n") < at("leg 4\n") && at("leg 4\n") < at("the job ended: succeeded"));
    }

    /// Ctrl-C detaches: the follow is ended, and nothing that stops the job
    /// is asked.
    #[tokio::test]
    async fn attach_detaches_without_cancelling_the_job() {
        let (a, b) = tokio::io::duplex(1 << 16);
        let asked: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
        tokio::spawn(daemon(b, std::sync::Arc::clone(&asked), false));
        let (r, w) = tokio::io::split(a);
        let mut conn = Conn::over(r, w);
        let mut out = Vec::new();
        // The signal comes once the first chunk is out.
        let (go, signal) = tokio::sync::oneshot::channel::<()>();
        let mut go = Some(go);
        let detach = async {
            let _ = signal.await;
        };
        let mut w2 = ChunkWatch {
            inner: &mut out,
            go: &mut go,
        };
        let end = follow(&mut conn, "act_k5", &mut w2, false, detach)
            .await
            .unwrap();
        assert_eq!(end, Ended::Detached);
        let asked = asked.lock().unwrap().clone();
        assert_eq!(asked, ["work.peek", "action.output", "action.unfollow"]);
        assert!(
            asked
                .iter()
                .all(|m| !m.contains("cancel") && !m.contains("stop")),
            "{asked:?}"
        );
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("detached; the job goes on"));
    }

    /// A writer that sends the signal when the first chunk's text is written.
    struct ChunkWatch<'a> {
        inner: &'a mut Vec<u8>,
        go: &'a mut Option<tokio::sync::oneshot::Sender<()>>,
    }

    impl Write for ChunkWatch<'_> {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.inner.extend_from_slice(b);
            if b.starts_with(b"leg 3") {
                if let Some(go) = self.go.take() {
                    let _ = go.send(());
                }
            }
            Ok(b.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
