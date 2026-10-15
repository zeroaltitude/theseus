//! Splitting a thread (theseus-vt9k, T8): the turns of a thread a new thread
//! opens with, and `thread.split`, the model's tool for it.
//!
//! - **The turns.** The new thread's first compile is an arrangement
//!   (`arrangement.rs`) of the chosen messages of the source: each piece is a
//!   message's full original text (a person's words, a reply's, a tool's
//!   result), with its author, time and session, in the source's order. They
//!   are *quoted*: the source keeps every node, and an `derived_from` edge
//!   (`arrangement`) runs from the arrangement to each, so `node.reach`
//!   counts the copies. A person chooses them by position (`@` numbers of
//!   `theseus history`, an inclusive range or a list); the model by quoting
//!   the first message of the part, and the last when it is not the end.
//! - **The working set** is read from what the turns *did*, not guessed from
//!   what they say: the paths the calls named (`path`, `paths`, `file`,
//!   `files`), the directories (`cwd`, `dir`), the hosts of the URLs, and the
//!   `#123` references the messages hold. The owner edits the rest.
//! - **`thread.split`** asks for the owner's tap at every posture (the gate's
//!   layer-1 rule, `Plan::authority`), runs in the thread's own session only,
//!   and its result is the note the source keeps: appended to the tail, so
//!   the source's prefix is unchanged.

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_protocol::threads::{ThreadInfo, WorkingSet};
use theseus_tools::{parse, AsyncMediaRun, Backend, Plan, Retry, Tool, ToolCtx, ToolFailure};
use theseus_tools::{ToolClass, ToolOutput};

use crate::arrangement::{self, Piece, Role};
use crate::node::Body;
use crate::store::Transcript;
use crate::thread_join::MAX_SET;
use crate::toolrun::TurnCtx;

/// The author of the arrangement a split writes (`thread:<source>`), which
/// tells it from a task's (`session:<parent>`) to the compiler.
pub const AUTHOR_PREFIX: &str = "thread:";

/// Whether a node is the quoted turns a split thread opens with.
pub fn is_quote(n: &crate::node::Node) -> bool {
    n.author
        .as_deref()
        .is_some_and(|a| a.starts_with(AUTHOR_PREFIX))
}

pub const SPLIT: &str = "thread.split";
/// The tool's family, for the runtime's dispatch.
pub const FAMILY: &str = "thread.split";

/// The most messages a split quotes, and the most characters of them: the new
/// thread starts with them whole, so a split of a whole long thread is asked
/// to be smaller (the part that is its own concern is rarely the whole).
pub const MAX_PIECES: usize = 400;
pub const MAX_CHARS: usize = 400_000;

/// Which messages of a thread's transcript a split takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// The messages at WAL positions `first` to `last`, inclusive.
    Range(u64, u64),
    /// The messages at these positions.
    Positions(Vec<u64>),
    /// From the message `first` (a node id) through `last`, or, with none,
    /// through the end of the thread, up to but not including `stop` (the
    /// person's request that began the turn of the call that asked).
    Between {
        first: String,
        last: Option<String>,
        stop: Option<String>,
    },
}

/// What a split quotes.
#[derive(Debug, Clone)]
pub struct Chosen {
    pub pieces: Vec<Piece>,
    pub first_pos: u64,
    pub last_pos: u64,
    pub working_set: WorkingSet,
}

fn index_of(nodes: &Transcript, id: &str) -> Result<usize, String> {
    nodes
        .iter()
        .position(|(_, n)| n.id == id)
        .ok_or_else(|| format!("no message {id} in this thread"))
}

/// The indexes into `nodes` that `pick` names, in order.
fn indexes(nodes: &Transcript, pick: &Pick) -> Result<Vec<usize>, String> {
    match pick {
        Pick::Range(a, b) => {
            if a > b {
                return Err(format!("the range @{a} to @{b} ends before it begins"));
            }
            Ok((0..nodes.len())
                .filter(|i| (*a..=*b).contains(&nodes[*i].0))
                .collect())
        }
        Pick::Positions(ps) => {
            let mut out = Vec::new();
            for p in ps {
                let i = nodes
                    .iter()
                    .position(|(pos, _)| pos == p)
                    .ok_or_else(|| format!("no message at @{p} in this thread"))?;
                if !out.contains(&i) {
                    out.push(i);
                }
            }
            out.sort_unstable();
            Ok(out)
        }
        Pick::Between { first, last, stop } => {
            let from = index_of(nodes, first)?;
            let to = match (last, stop) {
                (Some(l), _) => index_of(nodes, l)?,
                (None, Some(s)) => index_of(nodes, s)?.saturating_sub(1),
                (None, None) => nodes.len().saturating_sub(1),
            };
            if from > to {
                return Err("the last message comes before the first".into());
            }
            Ok((from..=to).collect())
        }
    }
}

/// A list kept to `MAX_SET`, none twice.
fn push(v: &mut Vec<String>, x: String) {
    if !x.is_empty() && v.len() < MAX_SET && !v.contains(&x) {
        v.push(x);
    }
}

/// The host of a URL, when it has one.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// `#123` references in a text: a `#` straight before digits, not part of a
/// word or a longer run.
fn refs_in(text: &str, out: &mut Vec<String>) {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let before = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if b[i] == b'#' && before {
            let j = (i + 1..b.len())
                .find(|k| !b[*k].is_ascii_digit())
                .unwrap_or(b.len());
            if j > i + 1 && j - i <= 8 {
                push(out, text[i..j].to_string());
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
}

/// What the calls named, as the working set of the thread that takes them.
fn working_set_of(nodes: &Transcript, idx: &[usize], texts: &[&str]) -> WorkingSet {
    let mut w = WorkingSet::default();
    for i in idx {
        let Body::ToolCall { input, .. } = &nodes[*i].1.body else {
            continue;
        };
        let Some(o) = input.as_object() else { continue };
        for (k, v) in o {
            let strs: Vec<&str> = match v {
                Value::String(s) => vec![s.as_str()],
                Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
                _ => continue,
            };
            for s in strs {
                match k.as_str() {
                    "path" | "paths" | "file" | "files" => push(&mut w.files, s.into()),
                    "cwd" | "dir" => push(&mut w.roots, s.into()),
                    "url" => {
                        if let Some(h) = host_of(s) {
                            push(&mut w.hosts, h);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    for t in texts {
        refs_in(t, &mut w.refs);
    }
    w
}

/// The messages `pick` names, as the pieces of an arrangement, and what the
/// turns touched. Only messages with text are quoted (a call is read with its
/// result); an empty choice, or one past the limits, is said.
pub fn choose(nodes: &Transcript, pick: &Pick) -> Result<Chosen, String> {
    let idx = indexes(nodes, pick)?;
    let mut pieces = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    let (mut first_pos, mut last_pos) = (u64::MAX, 0);
    for i in &idx {
        let (pos, n) = &nodes[*i];
        let Some(text) = arrangement::text_of(n) else {
            continue;
        };
        first_pos = first_pos.min(*pos);
        last_pos = last_pos.max(*pos);
        let (body, _) = crate::toolrun::cap(&text, arrangement::PIECE_CHARS, |_| {
            format!(
                "the whole message is node {} of session {}",
                n.id, n.session_id
            )
        });
        pieces.push(Piece {
            role: Role::Context,
            node: n.id.clone(),
            session_id: n.session_id.clone(),
            origin: n.origin,
            author: n.author.clone(),
            at_ms: n.created_at_ms,
            first_line: crate::session::title_from(&text),
            text: Some(body),
            trusted: false,
            superseded_by: None,
        });
        texts.push(text);
    }
    if pieces.is_empty() {
        return Err("no message with text is in the part chosen: nothing to split".into());
    }
    let chars: usize = pieces
        .iter()
        .filter_map(|p| p.text.as_ref())
        .map(|t| t.len())
        .sum();
    if pieces.len() > MAX_PIECES || chars > MAX_CHARS {
        return Err(format!(
            "the part chosen is {} messages and {chars} characters, over the {MAX_PIECES} messages \
             or {MAX_CHARS} characters a new thread opens with: split a smaller part",
            pieces.len()
        ));
    }
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    Ok(Chosen {
        pieces,
        first_pos,
        last_pos,
        working_set: working_set_of(nodes, &idx, &refs),
    })
}

/// The first message of a new thread's session: what it is, where its turns
/// came from, and that they are quotes. The arrangement follows it.
pub fn brief(new: &ThreadInfo, from: &str, quoted: u32, first: u64, last: u64) -> String {
    format!(
        "This thread, {} ({}), was split out of the thread {from} on {}. The {} that follow are \
         the part of {from} that is this thread's concern, quoted verbatim from @{first} to \
         @{last} of it: that is all you have of {from}'s conversation. Nothing was moved; {from} \
         goes on without them. Continue from where they stop.",
        new.slug,
        new.title,
        crate::thread_join::day(theseus_protocol::now_unix_ms()),
        crate::narrative::count(quoted as u64, "message", "messages"),
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SplitInput {
    slug: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    goal: Option<String>,
    from: String,
    #[serde(default)]
    to: Option<String>,
}

/// `thread.split`: part of this thread becomes a thread of its own.
pub struct ThreadSplitTool;

impl Tool for ThreadSplitTool {
    fn name(&self) -> &'static str {
        SPLIT
    }

    fn family(&self) -> &'static str {
        FAMILY
    }

    fn description(&self) -> &'static str {
        "Move a part of this thread into a new thread of its own, when it has become a separate \
         concern. Quote the first message of that part exactly (at least 20 characters), and its \
         last message unless it runs to the end of the thread. The new thread opens with those \
         messages quoted; nothing here is moved or deleted. The owner confirms. Only in a thread."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "slug": {"type": "string", "description": "The new thread's name: lower-case letters, digits and hyphens."},
                "title": {"type": "string"},
                "goal": {"type": "string", "description": "One sentence."},
                "from": {"type": "string", "description": "A quote of the part's first message, exactly as written."},
                "to": {"type": "string", "description": "A quote of its last message; none: through the end of the thread."}
            },
            "required": ["slug", "from"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Write
    }

    fn backend(&self) -> Backend {
        Backend::Async
    }

    fn retry(&self) -> Retry {
        // Run again after a crash, the thread would exist twice.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i: SplitInput = parse(input)?;
        Ok(Plan {
            summary: format!(
                "split part of this thread into the thread {}",
                i.slug.trim()
            ),
            authority: Some("a new thread opens with turns of this one".into()),
            ..Default::default()
        })
    }

    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "thread.split runs through the runtime, which reads the thread",
        ))))
    }
}

fn fail(why: impl Into<String>) -> AsyncMediaRun {
    let why = why.into();
    Box::pin(std::future::ready(Err(ToolFailure::new(why))))
}

/// Run `thread.split` for the turn `tc`'s call `correlation_id`.
pub fn run(tc: &TurnCtx<'_>, correlation_id: &str, input: &Value) -> AsyncMediaRun {
    let i: SplitInput = match parse(input) {
        Ok(i) => i,
        Err(e) => return fail(format!("Not split: {e}")),
    };
    let Some(core) = tc.threads.core() else {
        return fail("Not split: the core is not built");
    };
    let nodes = match tc.store.transcript(tc.session_id) {
        Ok(n) => n,
        Err(e) => return fail(format!("Not split: {e:#}")),
    };
    let holder = crate::task::holder_of(&nodes, correlation_id);
    let mut pieces = vec![arrangement::PieceRef {
        quote: Some(i.from.clone()),
        node: None,
        role: Role::Context,
    }];
    let same =
        i.to.as_deref()
            .is_some_and(|t| arrangement::normalize(t) == arrangement::normalize(&i.from));
    if let Some(t) = i.to.as_deref().filter(|_| !same) {
        pieces.push(arrangement::PieceRef {
            quote: Some(t.to_string()),
            node: None,
            role: Role::Context,
        });
    }
    let want = arrangement::Input {
        pieces,
        trust: vec![],
        supersedes: vec![],
    };
    let found = match arrangement::resolve(&want, &nodes, holder.as_deref()) {
        Ok(p) => p,
        Err(r) => {
            return fail(format!(
                "Not split: {}",
                r.message
                    .replace("piece 0", "`from`")
                    .replace("piece 1", "`to`")
            ))
        }
    };
    let first = found[0].node.clone();
    let last = match (same, found.get(1)) {
        (_, Some(l)) => Some(l.node.clone()),
        (true, None) => Some(first.clone()),
        (false, None) => None,
    };
    // Through the end of the thread, less the request that asked for this
    // call: the person's message that began the turn, and what followed it.
    let stop = holder.as_deref().and_then(|h| {
        let at = nodes.iter().position(|(_, n)| n.id == h)?;
        nodes[..at]
            .iter()
            .rev()
            .find(|(_, n)| {
                n.origin == crate::node::Origin::Operator
                    && n.kind == crate::stub::Kind::UserMessage
            })
            .map(|(_, n)| n.id.clone())
            .or_else(|| Some(h.to_string()))
    });
    let pick = Pick::Between { first, last, stop };
    let session = tc.session_id.to_string();
    Box::pin(async move {
        theseus_store::blocking(|| {
            core.split_from_session(
                &session,
                &i.slug,
                i.title.as_deref(),
                i.goal.as_deref(),
                pick,
            )
        })
        .map(|(text, meta)| (ToolOutput { text, meta }, None, None))
        .map_err(ToolFailure::new)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{Node, ResultStatus};

    fn at(mut n: Node, pos: u64) -> (u64, crate::stub::Stub) {
        n.created_at_ms = 1_790_000_000_000 + pos;
        (pos, n.into())
    }

    fn call(tool: &str, input: Value) -> Node {
        Node::tool_call(
            "ses_harbour",
            Some("turn_1"),
            Some(0),
            Body::ToolCall {
                tool_use_id: "tu_1".into(),
                tool: tool.into(),
                wire_name: tool.replace('.', "_"),
                input,
                assistant_node: "msg_x".into(),
                correlation_id: None,
                gate: None,
            },
        )
    }

    fn result(tool: &str, content: &str) -> Node {
        Node::tool_result(
            "ses_harbour",
            None,
            None,
            Body::ToolResult {
                tool_use_id: "tu_1".into(),
                tool: tool.into(),
                status: ResultStatus::Ok,
                is_error: false,
                content: content.into(),
                correlation_id: None,
                bytes_total: 0,
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta: Value::Null,
                image: None,
                external: None,
            },
        )
    }

    /// Two concerns in one thread: the tides, then the disk pressure.
    fn transcript() -> Transcript {
        vec![
            at(
                Node::user(
                    "ses_harbour",
                    None,
                    "cli",
                    "What is the tide table for Friday?",
                ),
                10,
            ),
            at(
                Node::user(
                    "ses_harbour",
                    None,
                    "cli",
                    "Why is the log disk filling, see #41?",
                ),
                20,
            ),
            at(call("fs.read", json!({"path": "/var/log/gauge.log"})), 21),
            at(result("fs.read", "gauge.log: 91% of the volume"), 22),
            at(
                call(
                    "http.fetch",
                    json!({"url": "https://Metrics.example.org:9090/q?x=1"}),
                ),
                23,
            ),
            at(
                Node::user(
                    "ses_harbour",
                    None,
                    "cli",
                    "Rotate it weekly, as in #41 and #7.",
                ),
                30,
            ),
        ]
    }

    fn texts(c: &Chosen) -> Vec<&str> {
        c.pieces
            .iter()
            .map(|p| p.text.as_deref().unwrap())
            .collect()
    }

    /// A range is inclusive at both ends, in the source's order; a call has
    /// no text of its own and is not quoted, its result is.
    #[test]
    fn a_range_quotes_exactly_the_messages_in_it() {
        let t = transcript();
        let c = choose(&t, &Pick::Range(20, 30)).unwrap();
        assert_eq!(
            texts(&c),
            [
                "Why is the log disk filling, see #41?",
                "gauge.log: 91% of the volume",
                "Rotate it weekly, as in #41 and #7."
            ]
        );
        assert_eq!((c.first_pos, c.last_pos), (20, 30));
        assert!(c
            .pieces
            .iter()
            .all(|p| p.role == Role::Context && p.superseded_by.is_none()));
        let one = choose(&t, &Pick::Range(10, 10)).unwrap();
        assert_eq!(texts(&one), ["What is the tide table for Friday?"]);
    }

    #[test]
    fn positions_quote_those_messages_and_a_missing_one_is_said() {
        let t = transcript();
        let c = choose(&t, &Pick::Positions(vec![30, 10])).unwrap();
        assert_eq!(
            texts(&c),
            [
                "What is the tide table for Friday?",
                "Rotate it weekly, as in #41 and #7."
            ],
            "the source's order, not the order asked"
        );
        let e = choose(&t, &Pick::Positions(vec![11])).unwrap_err();
        assert!(e.contains("no message at @11"), "{e}");
        let e = choose(&t, &Pick::Range(30, 20)).unwrap_err();
        assert!(e.contains("ends before it begins"), "{e}");
        let e = choose(&t, &Pick::Range(21, 21)).unwrap_err();
        assert!(
            e.contains("nothing to split"),
            "a call alone has no text: {e}"
        );
    }

    /// The model's way: from a message through another's, or through the
    /// end of the thread before the reply that holds the call.
    #[test]
    fn between_runs_from_a_message_to_the_end_or_to_another() {
        let t = transcript();
        let id = |i: usize| t[i].1.id.clone();
        let c = choose(
            &t,
            &Pick::Between {
                first: id(1),
                last: None,
                stop: Some(id(5)),
            },
        )
        .unwrap();
        assert_eq!(c.pieces.len(), 2, "through the message before the stop");
        let c = choose(
            &t,
            &Pick::Between {
                first: id(1),
                last: None,
                stop: None,
            },
        )
        .unwrap();
        assert_eq!(c.pieces.len(), 3);
        let c = choose(
            &t,
            &Pick::Between {
                first: id(1),
                last: Some(id(1)),
                stop: None,
            },
        )
        .unwrap();
        assert_eq!(c.pieces.len(), 1);
        let e = choose(
            &t,
            &Pick::Between {
                first: id(5),
                last: Some(id(1)),
                stop: None,
            },
        )
        .unwrap_err();
        assert!(e.contains("comes before"), "{e}");
    }

    /// The working set is what the turns did: the files and hosts their
    /// calls named and the `#` references their messages hold.
    #[test]
    fn the_working_set_is_what_the_chosen_turns_touched() {
        let t = transcript();
        let c = choose(&t, &Pick::Range(20, 30)).unwrap();
        let w = c.working_set;
        assert_eq!(w.files, ["/var/log/gauge.log"]);
        assert_eq!(w.hosts, ["metrics.example.org"]);
        assert_eq!(w.refs, ["#41", "#7"]);
        let none = choose(&t, &Pick::Range(10, 10)).unwrap().working_set;
        assert!(none.is_empty(), "{none:?}");
    }

    #[test]
    fn a_part_past_the_limits_is_asked_to_be_smaller() {
        let big = "x".repeat(arrangement::PIECE_CHARS);
        let t: Transcript = (0..30)
            .map(|i| {
                at(
                    Node::user("ses_harbour", None, "cli", &format!("{big}{i}")),
                    i + 1,
                )
            })
            .collect();
        let e = choose(&t, &Pick::Range(1, 30)).unwrap_err();
        assert!(e.contains("split a smaller part"), "{e}");
        assert!(choose(&t, &Pick::Range(1, 5)).is_ok());
    }

    #[test]
    fn refs_are_hash_digits_not_inside_words() {
        let mut v = Vec::new();
        refs_in("fixed in #123 and #45; not a#6 or #x or #123456789", &mut v);
        assert_eq!(v, ["#123", "#45"]);
    }

    #[test]
    fn a_hosts_name_is_the_part_between_the_slashes() {
        assert_eq!(
            host_of("https://user@Example.org:8080/a/b?x=1").as_deref(),
            Some("example.org")
        );
        assert_eq!(host_of("no url"), None);
    }
}
