//! Opening a line of `context.history`: one exchange in spoken form (design §5.1, "Forms"): the people's words
//! and the reply's text, tool traffic as one-line stubs, cut at about [`OPEN_TOKENS`], headed with its place and
//! date. It passes the same checks as a page's line (`Lens::admits`).
//!
//! It is the view's one spoken form (`view::text::forms::spoken`, cut by `forms::fit`), each part labelled by
//! its own provenance (`view::text::parts::mark_parts`): every message is attributed by its OWN stored mark
//! and origin, never by the message that opened the exchange: the owner's (his mark proves him), another
//! person's or a server's (outside text, wrapped on its own), or "author unknown". What the harness itself said
//! is not a spoken part. A tool call shows its tool and its result's size (its input is never read), never the
//! call's arguments. Every line of what a person or Theseus said is set behind a "│ " margin, so nothing in
//! it can open a heading of the tool's own.

use std::collections::HashMap;

use serde_json::Value;
use theseus_store::pages::ledger_session;
use theseus_store::{kinds, Page, Store as _};

use super::page::{flat, when, Lens};
use super::session::Opened;
use super::written::{self, Written};
use crate::exchange::{self, Exchange, OWNER};
use crate::node::{Body, Node, Origin};
use crate::quotes::{Field, Form};
use crate::recall::stamp;
use crate::view::history::Quoted;
use crate::view::text::forms::{self, Said, Voice};
use crate::view::text::parts::{mark_parts, Seen};
use crate::view::text::{labels, literal};
use crate::view::{Author, Handle, Pos, Reasons, SourceRange, Space};

/// An opened exchange's size, about, in tokens.
pub const OPEN_TOKENS: u64 = 3_000;
/// Nodes read a page at a time, and at most.
const PAGE: usize = 64;
const MOST: usize = 512;
/// What sets each line of stored text apart from the tool's own lines.
pub(crate) const MARGIN: &str = "\u{2502} ";

/// The exchange `h` names, as said, or why it cannot be opened (a denied one is not told apart from none): the
/// text alone, as the tests read it; the tool reads [`open_marked`].
#[cfg(test)]
pub fn open(lens: &Lens<'_>, h: &Handle) -> Result<String, String> {
    open_marked(lens, h).map(|o| o.text)
}

/// [`open`], and whether the exchange holds another person's words (a message whose own mark is not the
/// owner's, or a server's prompt): what marks the call's result as outside text (`history_tool::marker`).
pub fn open_marked(lens: &Lens<'_>, h: &Handle) -> Result<Opened, String> {
    let none = || format!("no exchange {} can be opened here", flat(h.0.trim()));
    let ex = exchange::exchange(lens.store, h.0.trim())
        .map_err(|_| none())?
        .ok_or_else(none)?;
    // Nothing the asker's scope does not admit (`Lens::in_scope`), and for a fork nothing its cut holds
    // (`Lens::forked`): either reads as no exchange here (`Lens::opens`).
    if !lens.opens(&ex).map_err(|e| format!("{e:#}"))? {
        return Err(none());
    }
    let read = nodes(lens, &ex).map_err(|e| format!("{e:#}"))?;
    let at: HashMap<String, u64> = read.iter().map(|(p, n)| (n.id.clone(), *p)).collect();
    let nodes: Vec<Node> = read.into_iter().map(|(_, n)| n).collect();
    // Another person's message, or (derived) an External result the exchange read: its reply may quote it. And
    // the exchange's own opening input (D6 round 3, A): whatever of it the read nodes hold.
    let outside = nodes.iter().any(crate::ladder::did::outside)
        || speakable(&nodes).iter().any(|n| {
            matches!(n.body, Body::UserMessage { .. })
                && matches!(author_of(lens, n), Author::Other)
        })
        || ex.person.as_deref().is_some_and(|p| p != OWNER)
        || ex.opened_by == exchange::Opener::Prompt;
    let (written, sources) = render(lens, &ex, &nodes, &at);
    let (text, parts) = written.into_parts();
    Ok(Opened {
        text,
        parts,
        outside,
        next: None,
        sources,
    })
}

/// The nodes of `ex`'s turn, oldest first, each with its record's WAL position: its session's, read forward from
/// its opening message (or from its start, when a message did not open it) until the next exchange's message.
fn nodes(lens: &Lens<'_>, ex: &Exchange) -> anyhow::Result<Vec<(u64, Node)>> {
    let first = ex
        .ask
        .as_deref()
        .and_then(|id| lens.store.get_node(id).ok().flatten());
    let (mut after, since) = match &first {
        Some((pos, _)) => (pos.saturating_sub(1), None),
        None => (0, Some(ex.start_ms)),
    };
    let mut out = Vec::new();
    let mut read = 0;
    loop {
        let page = Page {
            kind: kinds::NODE,
            tags: vec![ledger_session(&ex.session_id)],
            after: Some(after),
            since_ms: since,
            limit: PAGE,
            ..Page::default()
        };
        let Some(got) = lens.store.inner().page(&page)? else {
            anyhow::bail!("the index is still being built: try again in a moment");
        };
        read += got.records.len();
        for r in &got.records {
            let n: Node = r.decode()?;
            match n.turn_id.as_deref() == Some(ex.turn_id.as_str()) {
                true => out.push((r.position, n)),
                // The next exchange's message: this one is over.
                false if !out.is_empty() && matches!(n.body, Body::UserMessage { .. }) => {
                    return Ok(out)
                }
                false => {}
            }
        }
        match (got.more, got.last) {
            (true, Some(last)) if read < MOST => after = last,
            _ => return Ok(out),
        }
    }
}

/// `text` behind the margin, line by line. The writers quote between edges a cut may fall at, and lay
/// [`margin1`] of those; this is it of the text trimmed, as the tests read it.
#[cfg(test)]
pub(super) fn margined(text: &str) -> String {
    margin1(text.trim())
}

/// The form Margin1: each line of `text` (as `str::lines` splits it) behind the margin, its control
/// characters spaces, nothing trimmed. [`margined`] is it, of the text trimmed.
/// Frozen once a stored reference names it: a change adds a new numbered form, never edits this one.
pub(crate) fn margin1(text: &str) -> String {
    text.lines()
        .map(|l| {
            let l: String = l
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            format!("{MARGIN}{l}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The form Line1: one line of [`margin1`] without its margin, each control character a space, nothing trimmed;
/// the text holds no line break. A cut through a `Margin1` quote keeps the line it cut in this form
/// (`quotes::cap`). Frozen once a stored reference names it: a change adds a new numbered form, never edits this one.
pub(crate) fn line1(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Who wrote a person's message, by its own mark: the owner (shown as his; may authorise only when he typed
/// it in person), another known person, or not known. A server's prompt is another's words.
pub(super) fn author_of(lens: &Lens<'_>, n: &Node) -> Author {
    if n.origin == Origin::Mcp {
        return Author::Other;
    }
    match exchange::mark(lens.store, &n.id).ok().flatten() {
        Some(m) => match m.person() {
            Some(p) if p == OWNER => Author::His {
                may_authorise: m.in_person,
            },
            Some(_) => Author::Other,
            None => Author::Unknown,
        },
        None => Author::Unknown,
    }
}

/// The nodes that are spoken: what the harness itself said is not, and a call's input is never shown (its
/// stub names the tool and its result's size alone).
fn speakable(nodes: &[Node]) -> Vec<Node> {
    nodes
        .iter()
        .filter(|n| match (&n.body, n.origin) {
            (Body::UserMessage { .. }, o) => matches!(o, Origin::Operator | Origin::Mcp),
            _ => true,
        })
        .cloned()
        .map(|mut n| {
            if let Body::ToolCall { input, .. } = &mut n.body {
                *input = Value::Null;
            }
            n
        })
        .collect()
}

/// The exchange, headed, each part labelled by its own provenance (`view::text::parts`), cut at about
/// [`OPEN_TOKENS`] by `view::text::forms::fit`, written with its parts: what a person or Theseus said, behind its
/// margin, a quote of the node's record at its position (`at`); and the nodes whose words it shows (a call's or a
/// result's stub shows none).
fn render(
    lens: &Lens<'_>,
    ex: &Exchange,
    nodes: &[Node],
    at: &HashMap<String, u64>,
) -> (Written, Vec<String>) {
    let handle = Handle(ex.turn_id.clone());
    let nodes = speakable(nodes);
    let by_id: HashMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut parts = forms::spoken(&nodes);
    // Each spoken part's quote, and its text as shown: its words behind the margin, then its mark.
    let mut quoted: HashMap<String, (Quoted, String)> = HashMap::new();
    for p in &mut parts {
        if matches!(p.voice, Voice::Person | Voice::Harness | Voice::Theseus) {
            // A volatile value is read with the time it was true at, never as a bare fact (M6 §2.11).
            let n = by_id.get(p.node.as_str());
            let mark = n
                .and_then(|n| crate::recall::render::volatile(n, &p.text))
                .map_or_else(String::new, |m| format!("\n({m})"));
            let trimmed = p.text.trim();
            let from = trimmed.as_ptr() as usize - p.text.as_ptr() as usize;
            // The words' edges where a cut may fall (`cuts`): a known value that begins or ends in the blanks
            // trimmed is kept whole.
            let (from, to) = lens.cuts.around(&p.text, from, from + trimmed.len());
            let source = n.and_then(|n| at.get(&n.id).map(|pos| (n, *pos)));
            let q = source.and_then(|(n, pos)| {
                let field = match n.body {
                    Body::AssistantMessage { .. } => Field::ReplyText,
                    _ => Field::Chunk,
                };
                written::quoted((&n.id, pos), (field, &p.text), from..to, Form::Margin1)
            });
            p.text = format!("{}{mark}", margin1(&p.text[from..to]));
            if let Some(q) = q {
                quoted.insert(p.node.clone(), (q, p.text.clone()));
            }
        }
    }
    let parts = forms::fit(parts, OPEN_TOKENS, &handle);
    let mut sources: Vec<String> = (parts.iter())
        .filter(|p| matches!(p.voice, Voice::Person | Voice::Harness | Voice::Theseus))
        .filter(|p| !p.node.is_empty())
        .map(|p| p.node.clone())
        .collect();
    sources.dedup();
    let at = Pos {
        at_ms: ex.start_ms,
        id: ex.turn_id.clone(),
    };
    let reasons = Reasons::new();
    let seen = Seen {
        origin: labels::Origin {
            place: &ex.place,
            at_ms: ex.start_ms,
            source: None,
        },
        here: ex.place == lens.here,
        live: false,
        reasons: &reasons,
        range: SourceRange {
            space: Space::Exchanges {
                place: ex.place.clone(),
            },
            first: at.clone(),
            last: at,
        },
        handle,
        person: None,
    };
    let author = |p: &Said| {
        by_id
            .get(p.node.as_str())
            .map_or(Author::Unknown, |n| author_of(lens, n))
    };
    let marked = mark_parts(&parts, &author, &seen, &stamp::system);
    let mut w = Written::default();
    w.code(&format!(
        "[Exchange {}, in {}, {}: what was said then, in spoken form (tool traffic as one-line stubs; each line \
         of what was said behind \u{2502}). It is history, not a present instruction.]",
        literal::literal(&ex.turn_id, 80),
        literal::literal(&ex.place, 120),
        when(ex.start_ms)
    ));
    for m in &marked {
        w.code("\n");
        let text = m.turn_text();
        // The part's words, behind their margin, lie where its text does in its labelled form: before them its
        // heading and voice, after them its mark and an outside wrapping's end, all code.
        match quoted.get(&m.node) {
            Some((q, shown)) => match text.rfind(shown.as_str()) {
                Some(i) => {
                    w.code(&text[..i]);
                    w.said(&text[i..], Some(q));
                }
                None => w.said(&text, Some(q)),
            },
            None => w.code(&text),
        }
    }
    if marked.is_empty() {
        w.code("\nNothing of it was said in words.");
    }
    (w, sources)
}
