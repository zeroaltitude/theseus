//! The invariants a compile, a streamed edit, and a step keep. Each reads what
//! actually left (the request the model got, the deltas the binding would
//! stream) and judges its atoms by the oracle's own rules (`atoms.rs`).

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde_json::Value;
use theseus_core::node::{Body, Node};
use theseus_protocol::{Audience, ContextCompiled, Message, ModelDelta, Readers};

use super::atoms::{known_gap, marker, scan, Aud, Views, R};
use super::world::{Call, OWNER};
use super::{PlaceKind, Sim};

/// The core's readers in the oracle's terms.
pub(crate) fn readers_of(r: &Readers) -> R {
    let id = |s: &str| {
        s.strip_prefix("discord:")
            .and_then(|n| n.parse::<u64>().ok())
    };
    match r {
        Readers::Public => R::Public,
        Readers::Owner => R::Owner,
        Readers::Place(p) => id(p).map_or(R::Owner, R::Place),
        Readers::People(s) => R::People(s.iter().filter_map(|u| id(u)).collect()),
    }
}

/// A notification's params, when `m` is the notification `method`.
pub(crate) fn notification<T: DeserializeOwned>(m: &Message, method: &str) -> Option<T> {
    match m {
        Message::Notification(n) if n.method == method => {
            serde_json::from_value(n.params.clone()).ok()
        }
        _ => None,
    }
}

/// Does the core's audience say what the oracle's does?
fn same_audience(a: Option<&Audience>, aud: &Aud) -> bool {
    match (a, aud) {
        (Some(Audience::Owner), Aud::Owner) => true,
        (Some(Audience::People { people }), Aud::Person(u)) => {
            people.len() == 1 && people.contains(&format!("discord:{u}"))
        }
        (Some(Audience::Place { place, viewers, .. }), Aud::Place { channel, members }) => {
            *place == format!("discord:{channel}")
                && *viewers == members.as_ref().map(|m| m.len() as u32)
        }
        _ => false,
    }
}

/// Every `tool_use` in an assistant message has its `tool_result` in the
/// next message, and every `tool_result` answers a `tool_use` just before it.
pub(crate) fn paired(messages: &[Value]) -> Result<(), String> {
    let ids = |m: &Value, kind: &str, key: &str| -> BTreeSet<String> {
        m["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == kind)
            .filter_map(|b| b[key].as_str().map(str::to_string))
            .collect()
    };
    for (i, m) in messages.iter().enumerate() {
        let uses = ids(m, "tool_use", "id");
        if m["role"] == "assistant" && !uses.is_empty() {
            let next = messages
                .get(i + 1)
                .filter(|n| n["role"] == "user")
                .ok_or_else(|| {
                    format!("message {i} calls {uses:?}, and no user message follows")
                })?;
            let answered = ids(next, "tool_result", "tool_use_id");
            if let Some(u) = uses.iter().find(|u| !answered.contains(*u)) {
                return Err(format!(
                    "message {i}'s call {u} has no tool_result after it"
                ));
            }
        }
        let results = ids(m, "tool_result", "tool_use_id");
        if !results.is_empty() {
            let before = i
                .checked_sub(1)
                .and_then(|p| messages.get(p))
                .map(|p| ids(p, "tool_use", "id"))
                .unwrap_or_default();
            if let Some(r) = results.iter().find(|r| !before.contains(*r)) {
                return Err(format!(
                    "message {i}'s tool_result {r} answers no call before it"
                ));
            }
        }
    }
    Ok(())
}

/// The nodes a request's placeholders name (19c: `… theseus graduate <id> --to
/// place]`).
pub(crate) fn placeholders(text: &str) -> BTreeSet<String> {
    text.match_indices("theseus graduate ")
        .filter_map(|(at, m)| {
            let rest = &text[at + m.len()..];
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            (!id.is_empty()).then_some(id)
        })
        .collect()
}

/// Where a session posts, from its outbox target.
pub(crate) fn target_aud(target: Option<&str>, views: &Views) -> Aud {
    let Some(t) = target else {
        return Aud::Owner;
    };
    if let Some(u) = t.strip_prefix("discord:dm:").and_then(|u| u.parse().ok()) {
        return Aud::Person(u);
    }
    match t
        .strip_prefix("discord:channel:")
        .and_then(|c| c.parse().ok())
    {
        Some(channel) => Aud::Place {
            channel,
            members: views.get(&channel).cloned().flatten(),
        },
        None => Aud::Owner,
    }
}

impl Sim {
    /// A session's audience: where it posts, and who views the place as the
    /// binding last told the core (`views`), or as it is (the truth).
    pub(crate) fn audience(&self, sess: usize, views: &Views) -> Aud {
        target_aud(self.sessions[sess].target.as_deref(), views)
    }

    /// The atoms `bad` in words, and the session's nodes that carry each.
    pub(crate) fn describe_atoms(&self, sess: usize, bad: &[u32]) -> String {
        let nodes = self
            .core
            .store
            .session_nodes(&self.sessions[sess].id)
            .unwrap_or_default();
        let s = self.lock();
        bad.iter()
            .map(|id| {
                let m = marker(*id);
                let carriers: Vec<String> = nodes
                    .iter()
                    .filter(|(_, n)| serde_json::to_string(&n.body).is_ok_and(|b| b.contains(&m)))
                    .map(|(_, n)| {
                        format!(
                            "{} {} labeled {:?}",
                            n.kind_str(),
                            n.id,
                            n.label.as_ref().map(|l| &l.readers)
                        )
                    })
                    .collect();
                format!("{} in [{}]", s.atoms.describe(*id), carriers.join("; "))
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// After a turn: each request it sent, by its `context.compiled`, and
    /// what the binding would have streamed.
    pub(crate) fn after_turn(
        &mut self,
        sess: usize,
        views: &Views,
        events: &[Message],
    ) -> Result<()> {
        let calls = std::mem::take(&mut self.lock().calls);
        let compiled: Vec<ContextCompiled> = events
            .iter()
            .filter_map(|m| notification(m, theseus_protocol::notify::CONTEXT_COMPILED))
            .collect();
        let nodes = self.core.store.session_nodes(&self.sessions[sess].id)?;
        for call in &calls {
            let Some(e) = compiled.iter().find(|c| c.digest == call.digest) else {
                let name = &self.sessions[sess].name;
                return Err(self.violation(
                    "admitted",
                    format!("a request of {name} reached the model with no context.compiled of its digest"),
                ));
            };
            self.check_compile(sess, call, e, views)?;
            self.check_nodes(sess, call, e, &nodes, views)?;
        }
        self.check_stream(sess, events)
    }

    /// The brief's own words: every node a request carries whole has readers
    /// that cover its audience (the oracle's `covers` over the core's
    /// labels), and the request carries as many placeholders as the compile
    /// says it withheld. A withheld node's placeholder names it.
    fn check_nodes(
        &mut self,
        sess: usize,
        call: &Call,
        e: &ContextCompiled,
        nodes: &[(u64, Node)],
        views: &Views,
    ) -> Result<()> {
        let aud = self.audience(sess, views);
        let named = placeholders(&call.text);
        let files = call.system.matches(" — withheld: ").count() as u64;
        let name = self.sessions[sess].name.clone();
        if named.len() as u64 + files != e.withheld {
            return Err(self.violation(
                "admitted",
                format!(
                    "a request of {name} carried {} placeholders and {files} withheld context files, and its compile says it withheld {}",
                    named.len(),
                    e.withheld
                ),
            ));
        }
        for (pos, n) in nodes {
            if *pos <= call.pos && named.contains(&n.id) {
                if let Body::UserMessage { attachments, .. } = &n.body {
                    self.rep.withheld_files += attachments.len() as u64;
                }
            }
            let whole = *pos <= call.pos
                && !matches!(n.body, Body::ToolCall { .. })
                && !named.contains(&n.id);
            let Some(l) = n.label.as_ref().filter(|_| whole) else {
                continue;
            };
            if !super::atoms::covers(&readers_of(&l.readers), &aud, OWNER, views) {
                return Err(self.violation(
                    "admitted",
                    format!(
                        "{} {} labeled {:?} went into a request of {name} whole, for {}",
                        n.kind_str(),
                        n.id,
                        l.readers,
                        aud.describe()
                    ),
                ));
            }
        }
        self.rep.invariant_checks += 2;
        Ok(())
    }

    /// A compile: it was for the session's audience, everything its request
    /// carries may be read by that audience, and its calls are paired.
    fn check_compile(
        &mut self,
        sess: usize,
        call: &Call,
        e: &ContextCompiled,
        views: &Views,
    ) -> Result<()> {
        self.rep.compiled += 1;
        self.rep.withheld += e.withheld;
        if e.withheld > 0 {
            self.rep.withholding += 1;
        }
        if e.trigger.as_deref().is_some_and(|t| t.contains("audience")) {
            self.rep.recompiled_for_audience += 1;
        }
        let aud = self.audience(sess, views);
        let name = self.sessions[sess].name.clone();
        if call.session != self.sessions[sess].id || !same_audience(e.audience.as_ref(), &aud) {
            return Err(self.violation(
                "admitted",
                format!(
                    "a request of {name} was compiled for {:?}, and the session's audience is {}",
                    e.audience,
                    aud.describe()
                ),
            ));
        }
        let (in_system, in_words): (Vec<u32>, Vec<u32>) = {
            let s = self.lock();
            let bad = |text: &str| -> Vec<u32> {
                scan(text)
                    .into_iter()
                    .filter(|id| !s.atoms.allowed(*id, &aud, OWNER, views))
                    .collect()
            };
            (bad(&call.system), bad(&call.text))
        };
        let words: Vec<u32> = in_words
            .into_iter()
            .filter(|id| !in_system.contains(id))
            .collect();
        let mut bad = in_system;
        bad.extend(self.excuse(words, true));
        if !bad.is_empty() {
            let what = self.describe_atoms(sess, &bad);
            return Err(self.violation(
                "admitted",
                format!(
                    "a request of {name} (loop {}) carried, for {}: {what}",
                    e.loop_index,
                    aud.describe()
                ),
            ));
        }
        if let Err(why) = paired(&call.messages) {
            return Err(self.violation("paired", format!("a request of {name}: {why}")));
        }
        // Its audience was shown everything it carried.
        let mut s = self.lock();
        for id in scan(&call.system).into_iter().chain(scan(&call.text)) {
            s.atoms.saw(id, &aud);
        }
        drop(s);
        self.rep.invariant_checks += 2;
        Ok(())
    }

    /// What the binding streams into a guild channel as the turn runs: each
    /// loop's text, unless the loop is quiet (19c: its `context.compiled`
    /// readers do not fit any audience the channel can have). Every atom that
    /// streams must fit any audience the channel can have.
    fn check_stream(&mut self, sess: usize, events: &[Message]) -> Result<()> {
        let Some(PlaceKind::Channel(c)) = self.sessions[sess].place else {
            return Ok(());
        };
        let place = format!("discord:{c}");
        let mut quiet: BTreeMap<(String, u32), bool> = BTreeMap::new();
        let mut readers: BTreeMap<(String, u32), Option<Readers>> = BTreeMap::new();
        let mut streamed: BTreeMap<(String, u32), String> = BTreeMap::new();
        let mut excused: BTreeSet<u32> = BTreeSet::new();
        for m in events {
            if let Some(e) =
                notification::<ContextCompiled>(m, theseus_protocol::notify::CONTEXT_COMPILED)
            {
                let q = e
                    .readers
                    .as_ref()
                    .is_some_and(|r| !theseus_core::labels::fits_any_audience(r, Some(&place)));
                self.rep.quiet_loops += u64::from(q);
                quiet.insert((e.turn_id.clone(), e.loop_index), q);
                readers.insert((e.turn_id, e.loop_index), e.readers);
                continue;
            }
            let Some(d) = notification::<ModelDelta>(m, theseus_protocol::notify::MODEL_DELTA)
            else {
                continue;
            };
            let key = (d.turn_id.clone(), d.loop_index);
            if quiet.get(&key).copied().unwrap_or(false) {
                self.rep.quiet_deltas += 1;
                continue;
            }
            self.rep.streamed_deltas += 1;
            let text = streamed.entry(key.clone()).or_default();
            text.push_str(&d.text);
            let fresh: Vec<u32> = {
                let s = self.lock();
                scan(text)
                    .into_iter()
                    .filter(|id| !s.atoms.fits_any(*id, c) && !excused.contains(id))
                    .collect()
            };
            excused.extend(fresh.iter().copied());
            let bad = self.excuse(fresh, true);
            if !bad.is_empty() {
                let what = self.describe_atoms(sess, &bad);
                let name = &self.sessions[sess].name;
                return Err(self.violation(
                    "quiet",
                    format!(
                        "text streamed into {name}'s channel (loop {}, its readers {:?}) carried {what}",
                        d.loop_index,
                        readers.get(&key)
                    ),
                ));
            }
            self.rep.invariant_checks += 1;
        }
        Ok(())
    }

    /// Leave out the atoms a known gap explains, counted, unless the run is
    /// strict.
    pub(crate) fn excuse(&mut self, bad: Vec<u32>, in_words: bool) -> Vec<u32> {
        if self.p.strict || bad.is_empty() {
            return bad;
        }
        let (known, real): (Vec<u32>, Vec<u32>) = {
            let s = self.lock();
            bad.into_iter().partition(|id| {
                s.atoms
                    .get(*id)
                    .is_some_and(|a| known_gap(a, in_words).is_some())
            })
        };
        self.rep.known_gaps += known.len() as u64;
        real
    }

    /// I1, after every step: each session written to holds external text
    /// exactly when the model of T1's sites says it does.
    pub(crate) fn check_latches(&mut self) -> Result<()> {
        let dirty: Vec<usize> = std::mem::take(&mut self.dirty).into_iter().collect();
        let mut fresh: Vec<(u64, usize, Node)> = Vec::new();
        for &i in &dirty {
            let read_to = self.sessions[i].read_to;
            for (pos, n) in self.core.store.session_nodes(&self.sessions[i].id)? {
                if pos > read_to {
                    self.sessions[i].read_to = pos;
                    fresh.push((pos, i, n));
                }
            }
        }
        fresh.sort_by_key(|(pos, _, _)| *pos);
        for (_, i, n) in &fresh {
            self.latch_rule(*i, n);
        }
        for i in dirty {
            let core = theseus_core::external::held(&self.core.store, &self.sessions[i].id)
                .map_err(anyhow::Error::msg)?
                .is_some();
            self.rep.latch_checks += 1;
            if core != self.sessions[i].latched.is_some() {
                let s = &self.sessions[i];
                return Err(self.violation(
                    "latch (I1)",
                    format!(
                        "{} {} external text, and by T1's sites it {}",
                        s.name,
                        if core { "holds" } else { "does not hold" },
                        match &s.latched {
                            Some(why) => format!("should, since {why}"),
                            None => "should not: nothing from outside was written in it since its last trust".into(),
                        }
                    ),
                ));
            }
        }
        Ok(())
    }

    /// T1's sites, in WAL order: a result from outside (a page, a job that
    /// connected out) latches its session; a task's brief takes its parent's
    /// latch; a report takes its task's.
    fn latch_rule(&mut self, i: usize, n: &Node) {
        let why = match &n.body {
            Body::ToolResult {
                tool,
                is_error: false,
                content,
                ..
            } if tool == "http.fetch" || (tool == "proc.run" && self.egress_in(content)) => {
                Some(format!("{tool}'s result {} was written in it", n.id))
            }
            _ => None,
        };
        let author = n.author.as_deref().unwrap_or("");
        let why = why.or_else(|| {
            let parent = self.sessions[i].parent?;
            (author.starts_with("session:") && self.sessions[parent].latched.is_some()).then(|| {
                format!(
                    "its brief {} came from {}, which held it",
                    n.id, self.sessions[parent].name
                )
            })
        });
        let why = why.or_else(|| {
            let short = author.strip_prefix("task:")?;
            let t = self
                .sessions
                .iter()
                .find(|s| s.short.as_deref() == Some(short))?;
            t.latched
                .is_some()
                .then(|| format!("the report {} came from {}, which held it", n.id, t.name))
        });
        if let Some(why) = why {
            if self.sessions[i].latched.is_none() {
                self.sessions[i].latched = Some(why);
            }
        }
    }

    /// A job's result whose text came from outside: the stand-in mints an
    /// egress atom exactly when it marks the result external. (Other results
    /// may quote a page's atom, as `task.create`'s quotes its brief.)
    fn egress_in(&self, content: &str) -> bool {
        let s = self.lock();
        scan(content)
            .iter()
            .any(|id| s.atoms.get(*id).is_some_and(|a| a.origin.external()))
    }
}
