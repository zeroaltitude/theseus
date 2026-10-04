//! LSP's base protocol: each message is a header part, then a JSON body.
//!
//! ```text
//! Content-Length: 52\r\n
//! \r\n
//! {"jsonrpc":"2.0","id":1,"method":"shutdown"}
//! ```
//!
//! The header part is ASCII lines ended by `\r\n`, and an empty line ends it.
//! `Content-Length` is required; `Content-Type` is optional and, if given,
//! must say UTF-8 (the spec's `utf8` is accepted too). Unknown headers are
//! skipped. A read takes exactly one message from a buffered stream however
//! the bytes arrive: split across reads, or several in one.

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The longest header line read before the message is refused.
const MAX_HEADER_LINE: usize = 8 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("reading the stream failed: {0}")]
    Io(#[from] std::io::Error),
    /// The stream broke the framing; nothing after it can be trusted.
    #[error("a malformed header: {0}")]
    Header(String),
    /// The body is larger than the limit; the stream is not read further.
    #[error("a message of {len} bytes, over the {max}-byte limit")]
    TooLarge { len: usize, max: usize },
}

/// Read one message's body. `Ok(None)` is a clean end of the stream before
/// any byte of a message; an end inside one is an error.
pub async fn read_message<R>(r: &mut R, max: usize) -> Result<Option<Vec<u8>>, FrameError>
where
    R: AsyncBufRead + Unpin,
{
    let mut len: Option<usize> = None;
    let mut first = true;
    loop {
        let mut line = Vec::new();
        let n = (&mut *r)
            .take(MAX_HEADER_LINE as u64 + 1)
            .read_until(b'\n', &mut line)
            .await?;
        if n == 0 {
            if first {
                return Ok(None);
            }
            return Err(FrameError::Header(
                "the stream ended inside a header".into(),
            ));
        }
        first = false;
        if line.last() != Some(&b'\n') {
            return Err(FrameError::Header(if line.len() > MAX_HEADER_LINE {
                format!("a header line over {MAX_HEADER_LINE} bytes")
            } else {
                "the stream ended inside a header".into()
            }));
        }
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.is_empty() {
            break;
        }
        header(&line, &mut len)?;
    }
    let len = len.ok_or_else(|| FrameError::Header("no Content-Length".into()))?;
    if len > max {
        return Err(FrameError::TooLarge { len, max });
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).await?;
    Ok(Some(body))
}

fn header(line: &[u8], len: &mut Option<usize>) -> Result<(), FrameError> {
    let text = std::str::from_utf8(line)
        .map_err(|_| FrameError::Header("a header line that is not text".into()))?;
    let Some((name, value)) = text.split_once(':') else {
        return Err(FrameError::Header(format!("no colon in {text:?}")));
    };
    let value = value.trim();
    if name.trim().eq_ignore_ascii_case("content-length") {
        let n = value
            .parse()
            .map_err(|_| FrameError::Header(format!("a Content-Length of {value:?}")))?;
        *len = Some(n);
    } else if name.trim().eq_ignore_ascii_case("content-type") {
        let charset = value
            .split(';')
            .filter_map(|p| p.trim().strip_prefix("charset="))
            .next();
        if let Some(c) = charset {
            if !matches!(c.to_ascii_lowercase().as_str(), "utf-8" | "utf8") {
                return Err(FrameError::Header(format!("the charset {c:?}")));
            }
        }
    }
    Ok(())
}

/// Write one message: its header, then its body, then a flush.
pub async fn write_message<W>(w: &mut W, body: &[u8]) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let head = format!("Content-Length: {}\r\n\r\n", body.len());
    w.write_all(head.as_bytes()).await?;
    w.write_all(body).await?;
    w.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, BufReader, ReadBuf};

    /// A reader that hands out its bytes in the pieces it was given, one
    /// piece a read: a message split across reads, or several in one.
    struct Pieces(std::collections::VecDeque<Vec<u8>>);

    impl AsyncRead for Pieces {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            if let Some(mut p) = self.0.pop_front() {
                let n = p.len().min(buf.remaining());
                buf.put_slice(&p[..n]);
                if n < p.len() {
                    self.0.push_front(p.split_off(n));
                }
            }
            Poll::Ready(Ok(()))
        }
    }

    fn framed(body: &str) -> Vec<u8> {
        format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
    }

    async fn read_all(pieces: Vec<Vec<u8>>) -> Vec<Result<Option<Vec<u8>>, String>> {
        let mut r = BufReader::with_capacity(4, Pieces(pieces.into()));
        let mut out = Vec::new();
        loop {
            let m = read_message(&mut r, 1 << 20).await;
            let done = !matches!(m, Ok(Some(_)));
            out.push(m.map_err(|e| e.to_string()));
            if done {
                return out;
            }
        }
    }

    #[tokio::test]
    async fn a_message_split_byte_by_byte_reads_whole() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":"é"}"#;
        let pieces = framed(body).into_iter().map(|b| vec![b]).collect();
        let got = read_all(pieces).await;
        assert_eq!(got[0].as_ref().unwrap().as_deref(), Some(body.as_bytes()));
        assert!(matches!(got[1], Ok(None)), "then a clean end: {got:?}");
    }

    #[tokio::test]
    async fn two_messages_in_one_read_come_out_as_two() {
        let mut joined = framed(r#"{"a":1}"#);
        joined.extend(
            b"Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: 7\r\n\r\n{\"b\":2}",
        );
        let got = read_all(vec![joined]).await;
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].as_ref().unwrap().as_deref(), Some(&b"{\"a\":1}"[..]));
        assert_eq!(got[1].as_ref().unwrap().as_deref(), Some(&b"{\"b\":2}"[..]));
    }

    #[tokio::test]
    async fn bad_framing_is_refused_not_guessed_at() {
        for (bytes, says) in [
            (&b"Content-Type: x\r\n\r\n{}"[..], "no Content-Length"),
            (b"Content-Length: ten\r\n\r\n", "a Content-Length of"),
            (
                b"Content-Length: 2\r\nContent-Type: a; charset=latin1\r\n\r\n{}",
                "charset",
            ),
            (b"Content-Length: 2\r\n", "ended inside a header"),
            (b"just text\r\n\r\n", "no colon"),
        ] {
            let got = read_all(vec![bytes.to_vec()]).await;
            let e = got.last().unwrap().as_ref().unwrap_err();
            assert!(e.contains(says), "{e} should say {says}");
        }
        let got = read_all(vec![framed(r#"{"big":"0123456789"}"#)]).await;
        assert!(got[0].is_ok());
        let mut r = BufReader::new(Pieces(vec![framed(r#"{"big":"0123456789"}"#)].into()));
        assert!(matches!(
            read_message(&mut r, 8).await,
            Err(FrameError::TooLarge { len: 20, max: 8 })
        ));
        // A body cut short is an error, not a message.
        let mut cut = framed(r#"{"a":1}"#);
        cut.truncate(cut.len() - 2);
        let got = read_all(vec![cut]).await;
        assert!(got[0].is_err());
    }

    #[tokio::test]
    async fn a_written_message_reads_back() {
        let mut buf = Vec::new();
        write_message(&mut buf, "{\"x\":\"ü\"}".as_bytes())
            .await
            .unwrap();
        assert!(buf.starts_with(b"Content-Length: 10\r\n\r\n"));
        let got = read_all(vec![buf]).await;
        assert_eq!(
            got[0].as_ref().unwrap().as_deref(),
            Some("{\"x\":\"ü\"}".as_bytes())
        );
    }
}
