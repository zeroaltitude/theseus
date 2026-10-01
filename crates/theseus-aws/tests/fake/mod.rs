//! A local fake AWS endpoint: a loopback HTTP/1.1 server that answers each
//! request with the next reply of a script, and records what it was sent.
//! Its listener is bound before any client connects, so no connect can hang
//! (on this machine a connect to a port nothing listens on does).

#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Clone, Debug)]
pub enum Reply {
    Answer {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    /// Read the request, then say nothing for this long.
    Hang(Duration),
    /// Read the request, then close the connection without a word.
    Drop,
}

impl Reply {
    pub fn new(status: u16, body: &str) -> Reply {
        Reply::Answer {
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn json(status: u16, body: &str) -> Reply {
        Reply::new(status, body).header("Content-Type", "application/x-amz-json-1.1")
    }

    pub fn header(mut self, k: &str, v: &str) -> Reply {
        if let Reply::Answer { headers, .. } = &mut self {
            headers.push((k.to_owned(), v.to_owned()));
        }
        self
    }
}

/// A request as the fake saw it.
#[derive(Clone, Debug)]
pub struct Seen {
    pub method: String,
    /// The path and query, as sent.
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

pub struct Fake {
    pub url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn read_request(sock: &mut TcpStream) -> Option<Seen> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut chunk))
            .await
            .ok()?
            .ok()?;
        if n == 0 || buf.len() > 1 << 20 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let first = lines.next()?;
    let mut parts = first.split(' ');
    let method = parts.next()?.to_owned();
    let target = parts.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let len: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut chunk))
            .await
            .ok()?
            .ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some(Seen {
        method,
        target,
        headers,
        body,
    })
}

async fn answer(sock: &mut TcpStream, head_request: bool, reply: Reply) {
    match reply {
        Reply::Answer {
            status,
            headers,
            body,
        } => {
            let mut out = format!("HTTP/1.1 {status} Fake\r\nConnection: close\r\n");
            let given_len = headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-length"));
            for (k, v) in &headers {
                // A canned length is only true of a HEAD's answer.
                if k.eq_ignore_ascii_case("content-length") && !head_request {
                    continue;
                }
                out.push_str(&format!("{k}: {v}\r\n"));
            }
            if !(head_request && given_len) {
                out.push_str(&format!("Content-Length: {}\r\n", body.len()));
            }
            out.push_str("\r\n");
            let _ = sock.write_all(out.as_bytes()).await;
            if !head_request {
                let _ = sock.write_all(&body).await;
            }
            let _ = sock.shutdown().await;
        }
        Reply::Hang(d) => tokio::time::sleep(d).await,
        Reply::Drop => {}
    }
}

impl Fake {
    /// Answers each request with the next reply; the last one repeats.
    pub async fn start(replies: Vec<Reply>) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let task = tokio::spawn(async move {
            let mut n = 0;
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let log = log.clone();
                let reply = replies.get(n).or(replies.last()).cloned();
                n += 1;
                tokio::spawn(async move {
                    let Some(req) = read_request(&mut sock).await else {
                        return;
                    };
                    let head = req.method == "HEAD";
                    log.lock().unwrap().push(req);
                    if let Some(reply) = reply {
                        answer(&mut sock, head, reply).await;
                    }
                });
            }
        });
        Fake {
            url: format!("http://{addr}"),
            seen,
            task,
        }
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}
