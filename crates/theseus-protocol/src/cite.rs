//! A reply's citations (theseus-ejdk, provable memory): the handle each
//! recalled item's header carries (`[m:a1b2c3]`), the handles a reply holds,
//! and what a surface needs to show one's source. String work only: no
//! clock, no store, no model. The core resolves a handle against the turn's
//! `Recall` node; the surfaces draw the result.

use serde::{Deserialize, Serialize};

/// What a handle starts with, inside its brackets.
pub const HANDLE_PREFIX: &str = "m:";

/// The shortest tail of a node's id a handle holds: the characters
/// `theseus history` prints after the prefix (`msg·a1b2c3`), the id's random
/// end.
pub const MIN_TAIL: usize = 6;

/// A handle as the text writes it: `[m:a1b2c3]`.
pub fn bracketed(handle: &str) -> String {
    format!("[{handle}]")
}

/// The handle each id gets in one pack: the last `MIN_TAIL` characters, or
/// the fewest more that make every distinct id's tail its own (a pack of a
/// dozen ids needs no more than six in practice; the rule holds for any).
/// The same id twice is the same handle. In the order given.
pub fn handles_for(ids: &[&str]) -> Vec<String> {
    let mut distinct: Vec<&str> = Vec::new();
    for id in ids {
        if !distinct.contains(id) {
            distinct.push(id);
        }
    }
    let longest = distinct
        .iter()
        .map(|i| i.chars().count())
        .max()
        .unwrap_or(0);
    let mut n = MIN_TAIL;
    while n < longest {
        let mut tails: Vec<String> = distinct.iter().map(|i| tail(i, n)).collect();
        tails.sort();
        let before = tails.len();
        tails.dedup();
        if tails.len() == before {
            break;
        }
        n += 1;
    }
    ids.iter()
        .map(|i| format!("{HANDLE_PREFIX}{}", tail(i, n)))
        .collect()
}

/// `id`'s last `n` characters.
fn tail(id: &str, n: usize) -> String {
    let len = id.chars().count();
    id.chars().skip(len.saturating_sub(n)).collect()
}

/// A frozen header with its handle at the front: `[m:a1b2c3] a message from
/// …`. A header recalled before handles has none and is left as it is.
pub fn header_with(handle: &str, header: &str) -> String {
    format!("{} {header}", bracketed(handle))
}

/// The handle a frozen header starts with, and the header after it.
pub fn split_header(header: &str) -> Option<(&str, &str)> {
    let rest = header.strip_prefix('[')?;
    let (handle, after) = rest.split_once("] ")?;
    is_handle(handle).then_some((handle, after))
}

/// Whether `s` is a handle: `m:` and at least `MIN_TAIL` characters of an id's
/// end, which are letters, digits and `_` (an imported message's id ends
/// `_<index>`).
pub fn is_handle(s: &str) -> bool {
    s.strip_prefix(HANDLE_PREFIX).is_some_and(|t| {
        t.len() >= MIN_TAIL
            && t.len() <= 64
            && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

/// The handles in `text`, each once, in the order they first appear. Only
/// the bracketed form counts; a handle in a code span or a quote is still a
/// handle the model wrote.
pub fn handles_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("[m:") {
        let after = &rest[at + 1..];
        match after.find(']') {
            Some(end) if is_handle(&after[..end]) => {
                let h = after[..end].to_string();
                if !out.contains(&h) {
                    out.push(h);
                }
                rest = &after[end..];
            }
            _ => rest = after,
        }
    }
    out
}

/// `text` with each handle `f` answers replaced by what it returns, the
/// bracketed handle and no more.
pub fn rewrite(text: &str, mut f: impl FnMut(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("[m:") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        match after.find(']') {
            Some(end) if is_handle(&after[..end]) => {
                match f(&after[..end]) {
                    Some(r) => out.push_str(&r),
                    None => out.push_str(&rest[at..at + end + 2]),
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `text` with the cited handles as `[1]`, `[2]`, in the order they first
/// appear, and the citations in that order. A handle with no citation is left.
pub fn mark_handles<'a>(text: &str, cites: &'a [Citation]) -> (String, Vec<&'a Citation>) {
    let mut order = Vec::new();
    let marked = mark_into(text, cites, &mut order);
    (marked, order)
}

/// `mark_handles` for a reply in several texts: the numbers go on from the
/// citations `order` already holds.
pub fn mark_into<'a>(text: &str, cites: &'a [Citation], order: &mut Vec<&'a Citation>) -> String {
    rewrite(text, |h| {
        let c = cites.iter().find(|c| c.handle == h)?;
        let n = match order.iter().position(|o| o.handle == h) {
            Some(i) => i,
            None => {
                order.push(c);
                order.len() - 1
            }
        };
        Some(format!("[{}]", n + 1))
    })
}

/// A cited source: what a surface shows under a reply, resolved by the core
/// against the turn's `Recall` node. A handle the turn was not given has none,
/// and neither does one a shared place may not show: it stays text.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Citation {
    /// `m:a1b2c3`, as the reply wrote it between brackets.
    pub handle: String,
    pub node_id: String,
    pub session_id: String,
    /// Where the source sits in the WAL.
    pub position: u64,
    /// The item's frozen header, its handle left off: whose, where, when,
    /// and as of which position.
    pub header: String,
    /// When the source was written, in Unix ms.
    pub at_unix_ms: u64,
    /// The source's text, over the range recall showed, cut short.
    pub excerpt: String,
    /// The source was a Discord message: where it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordSource>,
}

/// A source that was a Discord message: its place (`discord:<channel>`, as
/// the outbox names it) and its id there.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DiscordSource {
    pub target: String,
    pub message_id: String,
}

/// `memory.cites`: the sources of the handles a session's replies hold, read
/// from the session's `Recall` nodes, newest first. A session in a shared
/// place is answered only with sources a shared place may show.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MemoryCitesParams {
    pub session_id: String,
    /// Handles as `handles_in` reads them (`m:a1b2c3`), at most 64.
    pub handles: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MemoryCitesResult {
    pub session_id: String,
    /// The handles that resolved, in the order asked.
    pub cites: Vec<Citation>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A handle is the id's last six characters, longer only as far as the
    /// pack needs to tell its ids apart.
    #[test]
    fn a_handle_is_the_ids_tail_and_unique_in_its_pack() {
        let a = "msg_0198a0b1c2d3e4f5a6b7c8d9e0f1a2b3";
        let b = "msg_0198a0b1c2d3e4f5a6b7c8d9e5f1a2b3";
        assert_eq!(handles_for(&[a]), ["m:f1a2b3"]);
        assert_eq!(handles_for(&[a, a]), ["m:f1a2b3", "m:f1a2b3"]);
        // The same last six: each takes the digit that parts them.
        assert_eq!(handles_for(&[a, b]), ["m:0f1a2b3", "m:5f1a2b3"]);
        assert_eq!(
            handles_for(&[a, "msg_xx00000a2b3c4"]),
            ["m:f1a2b3", "m:a2b3c4"]
        );
        assert!(handles_for(&[]).is_empty());
    }

    /// The header's handle splits off; a header with none is left alone.
    #[test]
    fn a_header_holds_its_handle_in_front() {
        let h = header_with("m:a1b2c3", "a message from cli in #harbor");
        assert_eq!(h, "[m:a1b2c3] a message from cli in #harbor");
        assert_eq!(
            split_header(&h),
            Some(("m:a1b2c3", "a message from cli in #harbor"))
        );
        assert_eq!(split_header("a message from cli"), None);
        assert_eq!(split_header("[x] a message"), None);
    }

    /// A reply's handles: bracketed, hex, each once, in order.
    #[test]
    fn a_reply_holds_the_handles_it_wrote() {
        let text = "You chose oak [m:a1b2c3] and later pine [m:ffeedd][m:a1b2c3]; see [m:xyz123] \
                    and [m:abc] and [mm:abcdef] and [m:abcdef and [m:12_345_0] and [m:no way].";
        assert_eq!(
            handles_in(text),
            ["m:a1b2c3", "m:ffeedd", "m:xyz123", "m:12_345_0"]
        );
        assert!(handles_in("nothing here [1] [m:]").is_empty());
    }
}
