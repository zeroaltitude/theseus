//! A dispatch's mark in the turn's trace (M5 23b; design §2.5, "The
//! trace"): every judgment a turn dispatches is decided, and its id minted,
//! before the turn's last frame, which carries the trace, so the trace says
//! where each judgment was taken and names the row it will be. The mark is a
//! zero-length span named `judge`, of kind `mark`, with the pack, its point,
//! its mode, and the judgment's id. Deciding is pure (the mode and the
//! sample), so the mark costs the turn no frame, no read, and no wait; the
//! judgment itself is spawned after the frame, as before.
//!
//! One convention for every point that dispatches inside a turn: the gate,
//! inbound, compile, and the loop's end each mark the same way
//! ([`Dispatch::mark`]), and each spawns with the id it marked.

use serde_json::{json, Value};
use theseus_judge::Mode;
use theseus_protocol::Span;

use crate::trace::Trace;

/// The mark's name, and the kind a live judgment's span takes (26b).
pub const SPAN: &str = "judge";
/// A shadow dispatch's span kind: a mark.
pub const MARK: &str = "mark";

/// A judgment decided at a dispatch: its pack, its point, its mode, and the
/// id its row will carry.
#[derive(Debug, Clone, PartialEq)]
pub struct Dispatch {
    pub pack: String,
    pub point: String,
    pub mode: Mode,
    pub id: String,
}

impl Dispatch {
    /// A dispatch of `pack` at `point`, its id minted now.
    pub fn new(pack: &str, point: &str, mode: Mode) -> Self {
        Self {
            pack: pack.into(),
            point: point.into(),
            mode,
            id: theseus_judge::new_id(),
        }
    }

    /// Its mark, at this instant, under the trace's innermost open span,
    /// with any attributes the point adds (`extra`, an object).
    pub fn mark(&self, trace: &mut Trace, extra: Value) {
        let mut attrs = json!({
            "pack": self.pack,
            "point": self.point,
            "mode": mode_str(self.mode),
            "judgment": self.id,
        });
        if let (Some(a), Value::Object(e)) = (attrs.as_object_mut(), extra) {
            a.extend(e);
        }
        trace.mark(SPAN, MARK, attrs);
    }
}

pub fn mode_str(m: Mode) -> &'static str {
    match m {
        Mode::Shadow => "shadow",
        Mode::Canary => "canary",
        Mode::Live => "live",
    }
}

/// Every judgment a finished trace marks, in the trace's order.
pub fn marks(span: &Span) -> Vec<Dispatch> {
    let mut out = Vec::new();
    walk(span, &mut out);
    out
}

fn walk(span: &Span, out: &mut Vec<Dispatch>) {
    if span.name == SPAN && span.kind == MARK {
        let s = |k: &str| span.attrs.get(k).and_then(Value::as_str);
        let mode = match s("mode") {
            Some("canary") => Mode::Canary,
            Some("live") => Mode::Live,
            _ => Mode::Shadow,
        };
        if let (Some(pack), Some(id)) = (s("pack"), s("judgment")) {
            out.push(Dispatch {
                pack: pack.into(),
                point: s("point").unwrap_or_default().into(),
                mode,
                id: id.into(),
            });
        }
    }
    for c in &span.children {
        walk(c, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mark_is_read_back_as_the_dispatch_it_marked() {
        let mut t = Trace::start("turn", "turn", Value::Null);
        let d = Dispatch::new("loop.v1", "loop_end", Mode::Shadow);
        assert!(d.id.starts_with("jdg_"), "{}", d.id);
        d.mark(&mut t, json!({"loop": 0}));
        let root = t.finish(Value::Null);
        let m = &root.children[0];
        assert_eq!((m.name.as_str(), m.kind.as_str()), (SPAN, MARK));
        assert_eq!(m.start_us, m.end_us.unwrap(), "zero-length");
        assert_eq!(m.attrs["loop"], 0);
        assert_eq!(m.attrs["mode"], "shadow");
        assert_eq!(marks(&root), vec![d]);
    }
}
