//! Turn traces: a nested tree of timed spans recorded while a turn runs.
//!
//! `turn > loop[n] > { hook sites, provider.call > { first_byte, first_token },
//! advancer } > …`. Every span has a start and end in microseconds relative to
//! the turn's start, a kind, and free-form attributes. Marks are zero-length
//! spans. The finished tree rides on the turn result, the `turn.ended` ledger
//! row, and the error payload of a failed turn, so timing is never something
//! you wish you had recorded.

use std::time::Instant;

use serde_json::Value;
use theseus_protocol::Span;

pub struct Trace {
    origin: Instant,
    stack: Vec<Span>,
}

impl Trace {
    /// Start a trace whose root span is `name`; the clock starts now.
    pub fn start(name: &str, kind: &str, attrs: Value) -> Self {
        let mut t = Self {
            origin: Instant::now(),
            stack: Vec::new(),
        };
        t.stack.push(Span {
            name: name.into(),
            kind: kind.into(),
            start_us: 0,
            end_us: None,
            attrs,
            children: Vec::new(),
        });
        t
    }

    /// Start a trace whose clock began at `origin` (e.g. before a lock wait).
    /// The root records `origin_unix_ms` so exporters can place it in wall time.
    pub fn start_at(origin: Instant, name: &str, kind: &str, attrs: Value) -> Self {
        let mut t = Self::start(name, kind, attrs);
        t.origin = origin;
        let origin_unix_ms =
            theseus_protocol::now_unix_ms().saturating_sub(origin.elapsed().as_millis() as u64);
        if let Some(root) = t.stack.first_mut() {
            if let Value::Object(m) = &mut root.attrs {
                m.insert("origin_unix_ms".into(), Value::from(origin_unix_ms));
            } else if root.attrs.is_null() {
                root.attrs = serde_json::json!({"origin_unix_ms": origin_unix_ms});
            }
        }
        t
    }

    pub fn now_us(&self) -> u64 {
        self.origin.elapsed().as_micros() as u64
    }

    /// Open a child span under the innermost open span.
    pub fn enter(&mut self, name: &str, kind: &str, attrs: Value) {
        let start_us = self.now_us();
        self.stack.push(Span {
            name: name.into(),
            kind: kind.into(),
            start_us,
            end_us: None,
            attrs,
            children: Vec::new(),
        });
    }

    /// Close the innermost open span, merging `attrs` into it.
    pub fn exit(&mut self, attrs: Value) {
        if self.stack.len() <= 1 {
            return; // never close the root here
        }
        let end = self.now_us();
        let mut span = self.stack.pop().expect("span");
        span.end_us = Some(end);
        merge(&mut span.attrs, attrs);
        self.stack.last_mut().expect("parent").children.push(span);
    }

    /// A zero-length span at the current instant.
    pub fn mark(&mut self, name: &str, kind: &str, attrs: Value) {
        let now = self.now_us();
        self.mark_at(now, name, kind, attrs);
    }

    /// A zero-length span at an explicit offset (e.g. a provider's first byte).
    pub fn mark_at(&mut self, at_us: u64, name: &str, kind: &str, attrs: Value) {
        self.stack
            .last_mut()
            .expect("open span")
            .children
            .push(Span {
                name: name.into(),
                kind: kind.into(),
                start_us: at_us,
                end_us: Some(at_us),
                attrs,
                children: Vec::new(),
            });
    }

    /// A span with explicit bounds, recorded after the fact.
    pub fn record(&mut self, name: &str, kind: &str, start_us: u64, end_us: u64, attrs: Value) {
        self.stack
            .last_mut()
            .expect("open span")
            .children
            .push(Span {
                name: name.into(),
                kind: kind.into(),
                start_us,
                end_us: Some(end_us),
                attrs,
                children: Vec::new(),
            });
    }

    /// Close everything and return the root.
    pub fn finish(mut self, attrs: Value) -> Span {
        while self.stack.len() > 1 {
            self.exit(Value::Null);
        }
        let end = self.now_us();
        let mut root = self.stack.pop().expect("root");
        root.end_us = Some(end);
        merge(&mut root.attrs, attrs);
        root
    }
}

fn merge(into: &mut Value, extra: Value) {
    match (into, extra) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                a.insert(k, v);
            }
        }
        (slot, Value::Null) => {
            let _ = slot;
        }
        (slot @ Value::Null, other) => *slot = other,
        (slot, other) => {
            *slot = serde_json::json!({"value": slot.clone(), "extra": other});
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nests_and_closes_in_order() {
        let mut t = Trace::start("turn", "turn", json!({"a": 1}));
        t.enter("loop 0", "loop", Value::Null);
        t.mark("hook", "hook", json!({"event": "x"}));
        t.enter("provider.call", "provider", Value::Null);
        t.mark_at(5, "first_byte", "mark", Value::Null);
        t.exit(json!({"tokens": 3}));
        t.exit(Value::Null);
        let root = t.finish(json!({"b": 2}));
        assert_eq!(root.name, "turn");
        assert_eq!(root.attrs["a"], 1);
        assert_eq!(root.attrs["b"], 2);
        assert!(root.end_us.is_some());
        assert_eq!(root.children.len(), 1);
        let lp = &root.children[0];
        assert_eq!(lp.name, "loop 0");
        assert_eq!(lp.children.len(), 2);
        assert_eq!(lp.children[0].kind, "hook");
        let pc = &lp.children[1];
        assert_eq!(pc.name, "provider.call");
        assert_eq!(pc.attrs["tokens"], 3);
        assert_eq!(pc.children[0].start_us, 5);
        assert_eq!(pc.children[0].end_us, Some(5));
        assert!(pc.end_us.unwrap() >= pc.start_us);
    }

    #[test]
    fn finish_closes_dangling_spans() {
        let mut t = Trace::start("turn", "turn", Value::Null);
        t.enter("loop 0", "loop", Value::Null);
        t.enter("provider.call", "provider", Value::Null);
        let root = t.finish(Value::Null);
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].children.len(), 1);
        assert!(root.children[0].children[0].end_us.is_some());
    }
}
