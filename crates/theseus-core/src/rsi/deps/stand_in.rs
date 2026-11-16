//! A registry a test owns: a loopback HTTP server that answers each path with
//! what the test said, and counts every request. The real registries are never
//! contacted by a test.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::hosts::Endpoints;

/// What a path answers.
#[derive(Clone)]
pub(super) enum Reply {
    Body(Vec<u8>),
    Status(u16),
    /// A redirect to this URL.
    Redirect(String),
}

pub(super) struct Registry {
    pub base: String,
    routes: Arc<Mutex<HashMap<String, Reply>>>,
    seen: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Registry {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Registry {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes: Arc<Mutex<HashMap<String, Reply>>> = Arc::default();
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let (r, s) = (routes.clone(), seen.clone());
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let (r, s) = (r.clone(), s.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match sock.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&buf).into_owned();
                    let path = head
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/")
                        .to_string();
                    s.lock().unwrap().push(path.clone());
                    let reply = r.lock().unwrap().get(&path).cloned();
                    let out = match reply {
                        Some(Reply::Body(b)) => {
                            let mut o = format!(
                                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                                b.len()
                            )
                            .into_bytes();
                            o.extend(b);
                            o
                        }
                        Some(Reply::Status(c)) => format!(
                            "HTTP/1.1 {c} X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        )
                        .into_bytes(),
                        Some(Reply::Redirect(to)) => format!(
                            "HTTP/1.1 302 Found\r\nlocation: {to}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        )
                        .into_bytes(),
                        None => b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                            .to_vec(),
                    };
                    let _ = sock.write_all(&out).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Self {
            base,
            routes,
            seen,
            task,
        }
    }

    pub fn serve(&self, path: &str, reply: Reply) {
        self.routes.lock().unwrap().insert(path.into(), reply);
    }

    /// Every path asked of it, in order.
    pub fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    /// The endpoints that point at this server: crates under `/crates`, npm at its root.
    pub fn endpoints(&self) -> Endpoints {
        Endpoints {
            crates: format!("{}/crates", self.base),
            npm: self.base.clone(),
        }
    }
}
