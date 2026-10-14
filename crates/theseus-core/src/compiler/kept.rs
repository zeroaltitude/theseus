//! Each session's render, kept across turns (theseus-0l3g): an append to the
//! same compilation renders only the nodes written since, taking up the
//! render where the last one stopped.
//!
//! A request is the compilation's prefix and its tail, rendered in order,
//! and the tail only grows while the compilation stands. So the render of a
//! loop is the last render's state (`render::State`: the messages so far,
//! the last of which may still take blocks of its role, what the images
//! used, the efforts, where the tail began) with the new nodes rendered into
//! it. **The rule that matters**: that is byte for byte what a render of
//! every node makes. It holds because a kept state is taken up only when
//! - its key is this render's: the compilation (its prefix, its cleared
//!   results, its thinking stripped or not), the provider, the model and
//!   the fallback (whose thinking goes back), how images and PDFs show, the
//!   images the provider refused, and the caller's view (`view`: the old
//!   attachments a cold rewrite stubbed);
//! - the nodes it rendered are the first of this render's, by id and in
//!   order (a node never changes; a recall's renders from frozen sources),
//!   with the prefix the same length;
//! - it was closed when kept: every call it rendered found its result among
//!   its nodes (no repair), no answer cut at the window or refused waited to
//!   be replaced, and nothing was retried (`retrying`);
//! - no new node is a result for, or a call by, an id it rendered a result
//!   for: the full render would read that result anew.
//!
//! Anything else renders every node, as before, and keeps that. Bounded by
//! `[compiler] kept_mb` across sessions, the least recently used going
//! first; 0 keeps nothing. `compiler/tests_kept.rs` holds the rule over
//! generated sessions: every request the same with the kept render and
//! without.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use super::render::{cut_answers, render_messages, results_of, Ctx, Messages, State};
use crate::attach::Media;
use crate::node::{Body, Node};
use crate::provider::tool_uses_in;
use serde_json::Value;

/// What a kept render was made for: a render under another key starts over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Key {
    pub compilation: String,
    pub strip: bool,
    pub provider: String,
    pub model: String,
    pub also: Option<String>,
    pub vision: bool,
    pub pdf: bool,
    pub pdf_pages: u32,
    /// The images the provider refused, by their digest of ids.
    pub hidden: String,
    /// The caller's view of the nodes (`CompileInput::kept`).
    pub view: u64,
}

impl Key {
    pub(super) fn of(
        compilation: &str,
        strip: bool,
        provider: &str,
        media: &Media,
        view: u64,
    ) -> Key {
        Key {
            compilation: compilation.into(),
            strip,
            provider: provider.into(),
            model: media.model.into(),
            also: media.also.map(str::to_string),
            vision: media.vision,
            pdf: media.pdf,
            pdf_pages: media.pdf_pages,
            hidden: format!("{:?}", media.hidden),
            view,
        }
    }
}

struct Entry {
    key: Key,
    /// The nodes rendered, in order.
    ids: Vec<String>,
    /// How many of them were the prefix.
    prefix: usize,
    /// The calls they hold a result for.
    answered: HashSet<String>,
    state: State,
    bytes: usize,
    used: u64,
}

/// The kept renders, by session.
pub struct Kept {
    inner: Mutex<Inner>,
    cap: usize,
}

#[derive(Default)]
struct Inner {
    by: HashMap<String, Entry>,
    bytes: usize,
    tick: u64,
    /// Renders taken up, and renders made whole (tests, the debug log).
    hits: u64,
    misses: u64,
}

impl Kept {
    /// Kept renders up to `cap` bytes of their messages' content.
    pub fn new(cap: usize) -> Self {
        Kept {
            inner: Mutex::default(),
            cap,
        }
    }

    /// Renders taken up and renders made whole, so far.
    pub fn counts(&self) -> (u64, u64) {
        let i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        (i.hits, i.misses)
    }

    /// The bytes kept now, and the sessions.
    pub fn size(&self) -> (usize, usize) {
        let i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        (i.bytes, i.by.len())
    }

    fn take(&self, session: &str) -> Option<Entry> {
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let e = i.by.remove(session)?;
        i.bytes -= e.bytes;
        Some(e)
    }

    fn put(&self, session: &str, mut e: Entry) {
        if e.bytes > self.cap {
            return;
        }
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        i.tick += 1;
        e.used = i.tick;
        i.bytes += e.bytes;
        if let Some(old) = i.by.insert(session.to_string(), e) {
            i.bytes -= old.bytes;
        }
        while i.bytes > self.cap {
            let Some(oldest) =
                i.by.iter()
                    .min_by_key(|(_, e)| e.used)
                    .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some(e) = i.by.remove(&oldest) {
                i.bytes -= e.bytes;
            }
        }
    }

    fn counted(&self, hit: bool) {
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match hit {
            true => i.hits += 1,
            false => i.misses += 1,
        }
    }
}

/// What a kept render reads besides its nodes.
pub(super) struct Kept1<'a> {
    pub kept: &'a Kept,
    pub session: &'a str,
    pub key: Key,
}

/// The rest of `render_messages`' arguments.
pub(super) struct Args<'a> {
    pub strip_prefix_thinking: bool,
    pub provider: &'a str,
    pub media: &'a Media<'a>,
    pub retrying: Option<&'a str>,
    pub sources: &'a crate::recall::render::Sources,
    pub cleared: &'a HashSet<&'a str>,
}

/// `render_messages`, taking up the session's kept render where it holds,
/// and keeping this one when it is closed.
pub(super) fn render(k: Kept1<'_>, prefix: &[&Node], tail: &[&Node], a: Args<'_>) -> Messages {
    if a.retrying.is_some() || k.kept.cap == 0 {
        return render_messages(
            prefix,
            tail,
            a.strip_prefix_thinking,
            a.provider,
            a.media,
            a.retrying,
            (a.sources, a.cleared),
        );
    }
    let items: Vec<(&Node, bool)> = prefix
        .iter()
        .map(|n| (*n, true))
        .chain(tail.iter().map(|n| (*n, false)))
        .collect();
    let hit = k.kept.take(k.session).filter(|e| {
        e.key == k.key
            && e.prefix == prefix.len()
            && e.ids.len() <= items.len()
            && e.ids.iter().zip(&items).all(|(id, (n, _))| *id == n.id)
            && !items[e.ids.len()..]
                .iter()
                .any(|(n, _)| reads(n, &e.answered))
    });
    k.kept.counted(hit.is_some());
    let (mut st, from, mut answered) = match hit {
        Some(e) => (e.state, e.ids.len(), e.answered),
        None => (State::default(), 0, HashSet::new()),
    };
    let new = &items[from..];
    let results = results_of(new.iter().map(|(n, _)| *n));
    let (replaced, open) = cut_answers(new.iter().map(|(n, _)| *n), None);
    let ctx = Ctx {
        results: &results,
        replaced: &replaced,
        strip_prefix_thinking: a.strip_prefix_thinking,
        provider: a.provider,
        media: a.media,
        sources: a.sources,
        cleared: a.cleared,
    };
    for (n, in_prefix) in new {
        st.item(n, *in_prefix, &ctx);
    }
    if st.repairs.is_empty() && !open {
        answered.extend(results.keys().map(|id| id.to_string()));
        let entry = Entry {
            key: k.key,
            ids: items.iter().map(|(n, _)| n.id.clone()).collect(),
            prefix: prefix.len(),
            answered,
            bytes: bytes_of(&st),
            state: st.clone(),
            used: 0,
        };
        k.kept.put(k.session, entry);
    }
    st.finish()
}

/// Whether a new node reads a result the kept render rendered: a result for
/// one of its calls, or a call by its id.
fn reads(n: &Node, answered: &HashSet<String>) -> bool {
    match &n.body {
        Body::ToolResult {
            tool_use_id,
            late: false,
            ..
        } => answered.contains(tool_use_id),
        Body::AssistantMessage { blocks, .. } => tool_uses_in(blocks)
            .iter()
            .any(|u| answered.contains(&u.id)),
        _ => false,
    }
}

/// A state's size, near enough for the bound: its strings' bytes, and a
/// little for each value.
fn bytes_of(st: &State) -> usize {
    fn of(v: &Value) -> usize {
        16 + match v {
            Value::String(s) => s.len(),
            Value::Array(a) => a.iter().map(of).sum(),
            Value::Object(m) => m.iter().map(|(k, v)| k.len() + of(v)).sum(),
            _ => 0,
        }
    }
    st.out
        .iter()
        .map(|(_, blocks)| 64 + blocks.iter().map(of).sum::<usize>())
        .sum()
}
