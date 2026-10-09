//! A recall's two queries on one connection (theseus-zo1y). A recall that
//! asks for vectors asks twice: its word sources alone, which answer in a few
//! ms, and the whole query, whose embedding may take hundreds (`recall::race`).
//! Each on a connection of its own cost two of the tender's slots, and the
//! whole one's stayed held for its embedding after the recall had gone. Here
//! both requests go out at once on one connection, the words' first: the
//! tender answers a connection's requests in order, so the words' answer comes
//! as soon as it is ready, and the whole one after it. Once no one waits for
//! an answer (the recall's deadline passed, or its turn ended), the
//! connection closes, and the tender stops embedding at its next layer.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::time::Duration;

use theseus_protocol::index::{method, IndexQueryParams, IndexQueryResult};
use theseus_protocol::{Id, Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;

use super::{down_why, CallError, IndexTender, QUERY_DEADLINE};

/// One of the two answers.
pub type Answer = Pin<Box<dyn Future<Output = Result<IndexQueryResult, String>> + Send>>;

impl IndexTender {
    /// `first` and `second` (a recall's words' query, then its whole one),
    /// both on one connection, each answered as it comes, under
    /// [`QUERY_DEADLINE`] and the longer `wait_ms`. Asked only of a tender
    /// that runs, so a recall never waits on a socket no tender serves.
    pub fn query_two(
        &self,
        first: &IndexQueryParams,
        second: &IndexQueryParams,
    ) -> (Answer, Answer) {
        let tender = match self.status() {
            None => return both_fail("the index is off: [index] enabled = false"),
            Some(t) if t.state != "running" => {
                return both_fail(&format!(
                    "the index tender is {}{}",
                    t.state,
                    t.why.as_ref().map(|w| format!(": {w}")).unwrap_or_default()
                ))
            }
            Some(t) => t,
        };
        let deadline =
            QUERY_DEADLINE + Duration::from_millis(first.wait_ms.max(second.wait_ms).min(60_000));
        let (a, b) = call_two(&self.socket(), first, second, deadline);
        let said = move |r: Result<IndexQueryResult, CallError>,
                         tender: &theseus_protocol::TenderStatus| match r {
            Ok(r) => Ok(r),
            Err(CallError::Answered { message, .. }) => Err(message),
            Err(e) => Err(down_why(tender, Some(&e))),
        };
        let t2 = tender.clone();
        (
            Box::pin(async move { said(a.await, &tender) }),
            Box::pin(async move { said(b.await, &t2) }),
        )
    }
}

/// Both answers the same failure.
fn both_fail(why: &str) -> (Answer, Answer) {
    let (a, b) = (why.to_string(), why.to_string());
    (
        Box::pin(async move { Err(a) }),
        Box::pin(async move { Err(b) }),
    )
}

type Reply = Result<IndexQueryResult, CallError>;

/// `index.query` twice on one connection to `socket`: `first` (id 1), then
/// `second` (id 2), written at once. Each future is its request's answer;
/// the connection lives in a task of its own, which closes it once both
/// answers are read, `deadline` has passed, or no one waits for either.
pub fn call_two(
    socket: &Path,
    first: &IndexQueryParams,
    second: &IndexQueryParams,
    deadline: Duration,
) -> (
    impl Future<Output = Reply> + Send + 'static,
    impl Future<Output = Reply> + Send + 'static,
) {
    let no = |e: String| CallError::NoAnswer(e);
    let (tx1, rx1) = oneshot::channel::<Reply>();
    let (tx2, rx2) = oneshot::channel::<Reply>();
    let lines = [(1, first), (2, second)]
        .iter()
        .try_fold(Vec::new(), |mut out, (id, p)| {
            out.extend(serde_json::to_vec(&Request::new(
                Id::Num(*id),
                method::QUERY,
                p,
            ))?);
            out.push(b'\n');
            Ok::<_, serde_json::Error>(out)
        });
    let socket = socket.to_path_buf();
    tokio::spawn(async move {
        let mut waiting = [Some(tx1), Some(tx2)];
        let run = async {
            let lines = lines.map_err(|e| no(e.to_string()))?;
            let s = tokio::net::UnixStream::connect(&socket)
                .await
                .map_err(|e| no(format!("connecting to {}: {e}", socket.display())))?;
            let (r, mut w) = s.into_split();
            w.write_all(&lines).await.map_err(|e| no(e.to_string()))?;
            let mut r = BufReader::new(r);
            while waiting.iter().any(Option::is_some) {
                let mut line = String::new();
                let read = tokio::select! {
                    read = r.read_line(&mut line) => read,
                    // No one waits for an answer: close the connection, so
                    // the tender stops its embedding.
                    () = unheard(&mut waiting) => return Ok(()),
                };
                if read.map_err(|e| no(e.to_string()))? == 0 {
                    return Err(no("the tender closed the connection".into()));
                }
                let resp: Response = serde_json::from_str(&line).map_err(|e| no(e.to_string()))?;
                let slot = match resp.id {
                    Id::Num(1) => 0,
                    Id::Num(2) => 1,
                    other => return Err(no(format!("an answer to another request ({other:?})"))),
                };
                let reply = match resp.error {
                    Some(e) => Err(CallError::Answered {
                        code: e.code,
                        message: e.message,
                    }),
                    None => serde_json::from_value(resp.result.unwrap_or_default())
                        .map_err(|e| no(e.to_string())),
                };
                if let Some(tx) = waiting[slot].take() {
                    let _ = tx.send(reply);
                }
            }
            Ok(())
        };
        let ended = match tokio::time::timeout(deadline, run).await {
            Ok(r) => r,
            Err(_) => Err(no(format!("no answer within {} ms", deadline.as_millis()))),
        };
        if let Err(e) = ended {
            for tx in waiting.iter_mut().filter_map(Option::take) {
                let _ = tx.send(Err(no(e.to_string())));
            }
        }
    });
    let gone = || CallError::NoAnswer("the call's task ended".into());
    (
        async move { rx1.await.unwrap_or_else(|_| Err(gone())) },
        async move { rx2.await.unwrap_or_else(|_| Err(gone())) },
    )
}

/// Resolves once every answer still owed has no one waiting for it.
async fn unheard(waiting: &mut [Option<oneshot::Sender<Reply>>; 2]) {
    for tx in waiting.iter_mut().flatten() {
        tx.closed().await;
    }
}
