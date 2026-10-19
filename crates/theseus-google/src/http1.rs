//! The least HTTP/1.1 the loopback consent listener and the fake Google
//! need: one request in, one response out, `Connection: close`.

use std::collections::BTreeMap;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The most a request may hold, head and body: the loopback listener is
/// reachable by any local process.
const MAX: usize = 1 << 20;

#[derive(Debug, Clone)]
pub struct Req {
    pub method: String,
    /// The path and query, as sent.
    pub target: String,
    /// Header names lowercased.
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Req {
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }

    pub fn query(&self) -> Vec<(String, String)> {
        match self.target.split_once('?') {
            Some((_, q)) => url::form_urlencoded::parse(q.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn form(&self) -> BTreeMap<String, String> {
        url::form_urlencoded::parse(&self.body)
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

/// One request from `stream`, or None when it closed or sent nonsense.
pub async fn read_request<S: AsyncReadExt + Unpin>(stream: &mut S) -> Option<Req> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        if buf.len() > MAX {
            return None;
        }
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    let (method, target) = (first.next()?.to_string(), first.next()?.to_string());
    let headers: BTreeMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if len > MAX {
        return None;
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    Some(Req {
        method,
        target,
        headers,
        body,
    })
}

/// One response, then the connection is done.
pub async fn write_response<S: AsyncWriteExt + Unpin>(
    stream: &mut S,
    status: u16,
    content_type: &str,
    extra: &[(&str, String)],
    body: &[u8],
) {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        _ if status >= 500 => "Server Error",
        _ => "Status",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body).await;
    let _ = stream.shutdown().await;
}
