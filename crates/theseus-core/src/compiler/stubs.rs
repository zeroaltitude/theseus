//! A big old attachment stubbed at a cold rewrite (theseus-ezeg). When a
//! request will write the cache whole anyway (the session's last call, its
//! last turn's end or a keep-warm read since, is older than the request's
//! conversation TTL), an attachment whose text is estimated at `[cache]
//! stub_min_tokens` or more, in a message older than the last `[cache]
//! stub_after_turns` turns, is sent as one line instead: its name and size,
//! its size in tokens, and that `file.read` reads it again. The turn's first
//! compile decides (`TurnRunner::stubbed_view`), from facts it already has:
//! the session's last call and the clock.
//!
//! **Never inside a warm window.** What a cold rewrite stubbed is kept
//! (`InForce`) and every request after it renders the same, until the next
//! cold rewrite decides again: a warm request that stubbed one more, or
//! stopped stubbing one, would change bytes the cache holds and write the
//! rest of the prefix again. A keep-warm read renders the same set too.
//! After a restart, the set is read back from the session's newest
//! `context.compiled` row (`cache.stubbed`, the record of what each compile
//! stubbed), once per session in a run.
//!
//! **The store is unchanged**: the stub is the compile's, applied to a copy of
//! the transcript's message (`apply`); the node keeps its attachment, and
//! `file.read` finds it by name.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use serde_json::json;
use theseus_protocol::StubbedFile;

use crate::attach::{Media, Spend};
use crate::catalog::TokenRates;
use crate::node::{AttachmentContent, Body};
use crate::stub::{Kind, Stub};

/// What each session's last cold rewrite stubbed, in force through the warm
/// window after it.
#[derive(Default)]
pub struct InForce(Mutex<HashMap<String, Vec<StubbedFile>>>);

impl InForce {
    pub fn get(&self, session_id: &str) -> Option<Vec<StubbedFile>> {
        self.0.lock().unwrap().get(session_id).cloned()
    }

    pub fn set(&self, session_id: &str, stubbed: Vec<StubbedFile>) {
        self.0.lock().unwrap().insert(session_id.into(), stubbed);
    }
}

/// `[cache]`'s rules for a stub.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    pub min_tokens: u64,
    pub after_turns: u32,
}

/// Whether a request now writes the cache whole anyway: the session called
/// before, and its last call is older than `ttl_ms`.
pub fn cold(now_ms: u64, last_call_ms: u64, ttl_ms: u64) -> bool {
    last_call_ms > 0 && now_ms.saturating_sub(last_call_ms) > ttl_ms
}

/// The one line a stubbed attachment is sent as: the attachment's own
/// header names it and its size, and this says the rest.
pub fn line(tokens: u64) -> String {
    format!(
        "left out of this request: its text is about {} tokens, from an earlier turn; \
         file.read with its name reads it again",
        crate::narrative::thousands(tokens)
    )
}

/// The attachments a cold rewrite stubs: in user messages older than the
/// last `after_turns` turns, each whose blocks are estimated at
/// `min_tokens` or more. Only those messages' bodies are read.
pub fn choose(
    nodes: &[(u64, Stub)],
    rules: Rules,
    media: &Media,
    rates: TokenRates,
) -> Vec<StubbedFile> {
    if rules.min_tokens == 0 {
        return Vec::new();
    }
    let mut recent: Vec<&str> = Vec::new();
    for (_, n) in nodes.iter().rev() {
        if recent.len() >= rules.after_turns as usize {
            break;
        }
        if let (Kind::UserMessage, Some(t)) = (n.kind, n.turn_id.as_deref()) {
            if !recent.contains(&t) {
                recent.push(t);
            }
        }
    }
    let mut out = Vec::new();
    for (_, n) in nodes {
        if n.kind != Kind::UserMessage || n.turn_id.as_deref().is_some_and(|t| recent.contains(&t))
        {
            continue;
        }
        let Body::UserMessage { attachments, .. } = &n.body else {
            continue;
        };
        for (i, a) in attachments.iter().enumerate() {
            if !matches!(
                a.content,
                AttachmentContent::Text { .. } | AttachmentContent::File { .. }
            ) {
                continue;
            }
            let mut spend = Spend::default();
            let blocks = crate::attach::blocks(a, n.author.as_deref(), media, &mut spend);
            let message = [json!({"role": "user", "content": blocks})];
            let tokens =
                crate::provider::Census::of_messages(&message).tokens(rates) + spend.tokens;
            if tokens >= rules.min_tokens {
                out.push(StubbedFile {
                    node_id: n.id.clone(),
                    index: i as u32,
                    name: a.name.clone(),
                    tokens,
                });
            }
        }
    }
    out
}

/// `nodes` with each stubbed attachment's content its line: a copy of its
/// message, at its position, the rest untouched.
pub fn apply(nodes: Vec<(u64, Stub)>, stubbed: &[StubbedFile]) -> Vec<(u64, Stub)> {
    if stubbed.is_empty() {
        return nodes;
    }
    let ids: HashSet<&str> = stubbed.iter().map(|s| s.node_id.as_str()).collect();
    nodes
        .into_iter()
        .map(|(pos, n)| {
            if !ids.contains(n.id.as_str()) {
                return (pos, n);
            }
            let mut node = (*n).clone();
            if let Body::UserMessage { attachments, .. } = &mut node.body {
                for s in stubbed.iter().filter(|s| s.node_id == node.id) {
                    if let Some(a) = attachments.get_mut(s.index as usize) {
                        a.content = AttachmentContent::NotRead {
                            reason: line(s.tokens),
                        };
                    }
                }
            }
            (pos, node.into())
        })
        .collect()
}

impl crate::turn::TurnRunner {
    /// The transcript a loop's compile renders, with the stubs in force:
    /// decided again at a turn's first compile when it is cold, else what
    /// the last cold rewrite decided (read back from the session's newest
    /// `context.compiled` row once a run). `cold` is `None` past a turn's
    /// first compile and for a keep-warm read, which never decide.
    pub fn stubbed_view(
        &self,
        session_id: &str,
        model: &str,
        hidden: &[crate::session::NotShown],
        nodes: Vec<(u64, Stub)>,
        cold: Option<bool>,
    ) -> (Vec<(u64, Stub)>, Vec<StubbedFile>) {
        let stubbed = match cold {
            Some(true) => {
                let entry = self.catalog.get(model);
                let media = Media {
                    vision: entry.is_some_and(|e| e.vision),
                    pdf: entry.is_some_and(|e| e.pdf),
                    pdf_pages: entry.map_or(0, |e| crate::attach::page_limit(e.context_window)),
                    model,
                    also: None,
                    blobs: Some(self.store.blobs()),
                    hidden,
                };
                let rules = Rules {
                    min_tokens: self.cfg.cache.stub_min_tokens,
                    after_turns: self.cfg.cache.stub_after_turns,
                };
                let chosen = choose(&nodes, rules, &media, TokenRates::of(model));
                self.stubs.set(session_id, chosen.clone());
                chosen
            }
            _ => match self.stubs.get(session_id) {
                Some(s) => s,
                None => {
                    let s = self.stubbed_before(session_id);
                    self.stubs.set(session_id, s.clone());
                    s
                }
            },
        };
        (apply(nodes, &stubbed), stubbed)
    }

    /// What the session's newest compile stubbed, from its `context.compiled`
    /// row through the index (none while its shape is built).
    fn stubbed_before(&self, session_id: &str) -> Vec<StubbedFile> {
        use theseus_store::{kinds, Page, Store as _};
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![theseus_store::pages::ledger_kind_session(
                theseus_protocol::LedgerKind::ContextCompiled.as_str(),
                session_id,
            )],
            limit: 1,
            ..Page::default()
        };
        let row = (|| {
            let got = self.store.inner().page(&page).ok()??;
            let row: crate::ledger::LedgerRow = got.records.first()?.decode().ok()?;
            serde_json::from_value::<theseus_protocol::ContextCompiled>(row.data).ok()
        })();
        row.map(|c| c.cache.stubbed).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{Attachment, Node};

    fn text_file(name: &str, bytes: usize) -> Attachment {
        Attachment {
            name: name.into(),
            media_type: "text/plain".into(),
            size: bytes as u64,
            content: AttachmentContent::Text {
                text: "word ".repeat(bytes / 5),
                cut: false,
            },
        }
    }

    fn message(turn: &str, files: Vec<Attachment>) -> Node {
        Node::user_with("ses_t", Some(turn), "test", "here", files)
    }

    fn nodes(ns: Vec<Node>) -> Vec<(u64, Stub)> {
        ns.into_iter()
            .enumerate()
            .map(|(i, n)| (i as u64 + 1, n.into()))
            .collect()
    }

    /// Only an attachment over the floor, older than the last turns, is
    /// stubbed; `apply` leaves its name and size, and says its tokens.
    #[test]
    fn a_big_old_attachment_is_chosen_and_nothing_else() {
        let ns = nodes(vec![
            message(
                "turn_1",
                vec![text_file("big.txt", 400_000), text_file("small.txt", 4_000)],
            ),
            message("turn_2", vec![]),
            message("turn_3", vec![]),
            message("turn_4", vec![text_file("recent.txt", 400_000)]),
        ]);
        let rules = Rules {
            min_tokens: 20_000,
            after_turns: 3,
        };
        let got = choose(&ns, rules, &Media::none(), TokenRates::CLAUDE);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!((got[0].name.as_str(), got[0].index), ("big.txt", 0));
        assert!(got[0].tokens > 100_000, "{got:?}");
        let off = Rules {
            min_tokens: 0,
            ..rules
        };
        assert!(choose(&ns, off, &Media::none(), TokenRates::CLAUDE).is_empty());
        let out = apply(ns, &got);
        let Body::UserMessage { attachments, .. } = &out[0].1.body else {
            panic!()
        };
        assert_eq!(attachments[0].name, "big.txt");
        let AttachmentContent::NotRead { reason } = &attachments[0].content else {
            panic!("{:?}", attachments[0].content)
        };
        assert!(
            reason.contains("file.read") && reason.contains("tokens"),
            "{reason}"
        );
        assert!(matches!(
            attachments[1].content,
            AttachmentContent::Text { .. }
        ));
        assert!(cold(10_000_000, 1, 3_600_000) && !cold(10_000_000, 9_000_000, 3_600_000));
        assert!(
            !cold(10_000_000, 0, 3_600_000),
            "a session that never called"
        );
    }

    /// The arithmetic (theseus-ezeg): a 626,000-token prompt rewritten cold
    /// on Fable 5.1's 5-minute cache costs about $7.83; with its 426,000-token
    /// attachment a stub of a few dozen tokens, about $2.50.
    #[test]
    fn a_cold_rewrite_of_626k_tokens_costs_about_2_50_with_its_big_file_stubbed() {
        let catalog = crate::catalog::Catalog::builtin();
        let fable = catalog.get("claude-fable-5-1").unwrap();
        // 426,000 tokens of text at Claude's 3.3 bytes a token.
        let big = text_file("contract.txt", (426_000.0 * 3.3) as usize);
        let ns = nodes(vec![
            message("turn_1", vec![big]),
            message("turn_2", vec![]),
            message("turn_3", vec![]),
            message("turn_4", vec![]),
        ]);
        let rules = Rules {
            min_tokens: 20_000,
            after_turns: 3,
        };
        let got = choose(&ns, rules, &Media::none(), TokenRates::CLAUDE);
        let stubbed = got[0].tokens;
        assert!((420_000..432_000).contains(&stubbed), "{stubbed}");
        let write = |tokens: u64| {
            fable.cost_usd(&theseus_protocol::Usage {
                cache_creation_input_tokens: tokens,
                ..Default::default()
            })
        };
        let line_tokens = (line(stubbed).len() as f64 / 3.3).ceil() as u64 + 20;
        let (before, after) = (write(626_000), write(626_000 - stubbed + line_tokens));
        assert!((before - 7.825).abs() < 0.01, "{before}");
        assert!((2.35..2.65).contains(&after), "{after}");
    }
}
