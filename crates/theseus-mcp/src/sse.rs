//! Server-sent events, framed as the HTML standard's event-stream parser
//! frames them, for streamable HTTP. Bytes go in as they arrive, in any
//! split: a frame, a line, a CRLF, or a UTF-8 character may be cut across
//! two reads.
//!
//! - Lines end in CRLF, LF, or CR; a UTF-8 BOM at the start is dropped.
//! - A blank line dispatches the event; a line starting with `:` is a
//!   comment; `field: value` loses one space after the colon.
//! - `data` lines join with LF; `event` names the event (`message` when
//!   none does); `id` sets the stream's last event id (one holding NUL is
//!   ignored); `retry` sets the reconnection time when it is all digits.
//! - An event with no data dispatches nothing, but its `id` still counts.
//! - At the end of the stream an unfinished event is dropped.

use std::time::Duration;

/// One dispatched event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// `message` unless the frame named another type.
    pub event: String,
    pub data: String,
    /// The stream's last event id when this event was dispatched.
    pub id: Option<String>,
}

/// A frame (a line, or an event's data) grew past the parser's limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TooLarge(pub usize);

impl std::fmt::Display for TooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a server-sent event larger than {} bytes", self.0)
    }
}

#[derive(Debug)]
pub struct Parser {
    max: usize,
    line: Vec<u8>,
    /// The last line ended in CR: an LF that comes next belongs to it.
    after_cr: bool,
    /// No line has ended yet: a BOM there is dropped.
    first_line: bool,
    data: String,
    event: String,
    last_id: Option<String>,
    retry: Option<Duration>,
}

impl Parser {
    /// A parser whose lines and events stay under `max_bytes`.
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max: max_bytes,
            line: Vec::new(),
            after_cr: false,
            first_line: true,
            data: String::new(),
            event: String::new(),
            last_id: None,
            retry: None,
        }
    }

    /// Feed the next bytes; the events they complete come out in order.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Event>, TooLarge> {
        let mut out = Vec::new();
        let mut rest = bytes;
        while !rest.is_empty() {
            if self.after_cr {
                self.after_cr = false;
                if rest[0] == b'\n' {
                    rest = &rest[1..];
                    continue;
                }
            }
            match rest.iter().position(|&b| b == b'\n' || b == b'\r') {
                Some(i) => {
                    self.push(&rest[..i])?;
                    self.after_cr = rest[i] == b'\r';
                    rest = &rest[i + 1..];
                    self.end_line(&mut out)?;
                }
                None => {
                    self.push(rest)?;
                    rest = &[];
                }
            }
        }
        Ok(out)
    }

    /// A new stream from the same source (a reconnect): its framing starts
    /// afresh, and the last event id and the retry time carry over.
    pub fn restart(&mut self) {
        self.line.clear();
        self.after_cr = false;
        self.first_line = true;
        self.data.clear();
        self.event.clear();
    }

    /// The stream's last event id, for `Last-Event-ID` on a reconnect.
    pub fn last_event_id(&self) -> Option<&str> {
        self.last_id.as_deref().filter(|id| !id.is_empty())
    }

    /// The reconnection time the server asked for, if it did.
    pub fn retry(&self) -> Option<Duration> {
        self.retry
    }

    fn push(&mut self, bytes: &[u8]) -> Result<(), TooLarge> {
        if self.line.len() + bytes.len() > self.max {
            return Err(TooLarge(self.max));
        }
        self.line.extend_from_slice(bytes);
        Ok(())
    }

    fn end_line(&mut self, out: &mut Vec<Event>) -> Result<(), TooLarge> {
        let mut bytes = std::mem::take(&mut self.line);
        if std::mem::replace(&mut self.first_line, false) && bytes.starts_with(b"\xEF\xBB\xBF") {
            bytes.drain(..3);
        }
        let line = String::from_utf8_lossy(&bytes);
        if line.is_empty() {
            self.dispatch(out);
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(());
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line.as_ref(), ""),
        };
        match field {
            "event" => self.event = value.to_string(),
            "data" => {
                if self.data.len() + value.len() + 1 > self.max {
                    return Err(TooLarge(self.max));
                }
                self.data.push_str(value);
                self.data.push('\n');
            }
            "id" if !value.contains('\0') => self.last_id = Some(value.to_string()),
            "retry" if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
                if let Ok(ms) = value.parse::<u64>() {
                    self.retry = Some(Duration::from_millis(ms));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn dispatch(&mut self, out: &mut Vec<Event>) {
        let event = std::mem::take(&mut self.event);
        if self.data.is_empty() {
            return;
        }
        let mut data = std::mem::take(&mut self.data);
        if data.ends_with('\n') {
            data.pop();
        }
        out.push(Event {
            event: if event.is_empty() {
                "message".into()
            } else {
                event
            },
            data,
            id: self.last_id.clone().filter(|id| !id.is_empty()),
        });
    }
}

/// One event as a server writes it: an optional id, then the data, one
/// `data:` line per line of it, then a blank line.
pub fn frame(id: Option<&str>, data: &str) -> String {
    let mut s = String::new();
    if let Some(id) = id {
        s.push_str("id: ");
        s.push_str(id);
        s.push('\n');
    }
    for line in data.split('\n') {
        s.push_str("data: ");
        s.push_str(line);
        s.push('\n');
    }
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(input: &[u8]) -> Vec<Event> {
        Parser::new(1 << 20).feed(input).unwrap()
    }

    fn ev(data: &str, id: Option<&str>) -> Event {
        Event {
            event: "message".into(),
            data: data.into(),
            id: id.map(String::from),
        }
    }

    /// The same stream, cut at every byte, and fed a byte at a time, gives
    /// the same events: split frames, split CRLFs, split characters.
    #[test]
    fn every_split_gives_the_same_events() {
        let stream = "\u{FEFF}: a comment first\r\n\r\nid: 1\r\ndata: {\"a\":\r\ndata:  \"é\"}\r\n\r\n\
                      event: endpoint\rdata: /x\r\r: keepalive\n\nretry: 250\nid: 2\ndata\n\ndata: tail";
        let want = vec![
            ev("{\"a\":\n \"é\"}", Some("1")),
            Event {
                event: "endpoint".into(),
                data: "/x".into(),
                id: Some("1".into()),
            },
            ev("", Some("2")),
        ];
        let bytes = stream.as_bytes();
        assert_eq!(all(bytes), want);
        for cut in 0..=bytes.len() {
            let mut p = Parser::new(1 << 20);
            let mut got = p.feed(&bytes[..cut]).unwrap();
            got.extend(p.feed(&bytes[cut..]).unwrap());
            assert_eq!(got, want, "cut at {cut}");
            assert_eq!(p.last_event_id(), Some("2"));
            assert_eq!(p.retry(), Some(Duration::from_millis(250)));
        }
        let mut p = Parser::new(1 << 20);
        let mut got = Vec::new();
        for b in bytes {
            got.extend(p.feed(std::slice::from_ref(b)).unwrap());
        }
        assert_eq!(got, want);
    }

    #[test]
    fn the_fields_rules() {
        // No space after the colon; a second space is kept; an unknown field
        // and a bad retry are ignored; an id with NUL is ignored.
        let got = all(b"data:x\ndata:  y\nfoo: bar\nretry: 12a\nid: a\0b\n\n");
        assert_eq!(got, vec![ev("x\n y", None)]);
        // An id alone dispatches nothing, but counts for the next event; an
        // empty id clears it.
        let mut p = Parser::new(1 << 20);
        assert!(p.feed(b"id: 7\n\n").unwrap().is_empty());
        assert_eq!(p.last_event_id(), Some("7"));
        assert_eq!(p.feed(b"data: z\n\n").unwrap(), vec![ev("z", Some("7"))]);
        assert_eq!(p.feed(b"id\ndata: w\n\n").unwrap(), vec![ev("w", None)]);
        assert_eq!(p.last_event_id(), None);
        // An event type with no data is dropped, and does not leak into the
        // next event.
        assert_eq!(all(b"event: ping\n\ndata: q\n\n"), vec![ev("q", None)]);
        // A BOM only at the start: later, it is part of the line, whose
        // field is then unknown.
        assert_eq!(
            all("data: a\n\n\u{FEFF}data: b\n\n".as_bytes()),
            vec![ev("a", None)]
        );
    }

    #[test]
    fn a_frame_round_trips_and_size_is_bounded() {
        let data = "{\"jsonrpc\":\"2.0\",\"id\":1}\nsecond line";
        assert_eq!(
            all(frame(Some("e1"), data).as_bytes()),
            vec![ev(data, Some("e1"))]
        );
        let mut p = Parser::new(16);
        assert_eq!(p.feed(b"data: 0123456789abcdef\n"), Err(TooLarge(16)));
        let mut p = Parser::new(16);
        assert_eq!(
            p.feed(b"data: 01234567\ndata: 89abcdef\n"),
            Err(TooLarge(16))
        );
    }
}
