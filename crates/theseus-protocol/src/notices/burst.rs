//! At most one Interrupt per root per window (`Rules::burst_ms`, 10 s): the
//! first pings with its own line; the rest in the window become badges that
//! read `3 questions need you`. A client's memory, not a wire type: it takes
//! each notice with its `at_ms`, and reads no clock.

use std::collections::HashMap;

use super::{Notice, Rules, Urgency, Why};

/// A root's open window: when it began, how many Interrupts it took, and
/// whether every one was a question.
#[derive(Debug, Clone, Copy)]
struct Window {
    start_ms: u64,
    count: u32,
    questions: bool,
}

/// The burst rule's memory, by root.
#[derive(Debug, Clone, Default)]
pub struct Burst {
    window_ms: u64,
    open: HashMap<String, Window>,
}

impl Burst {
    pub fn new(rules: &Rules) -> Self {
        Self {
            window_ms: rules.burst_ms,
            open: HashMap::new(),
        }
    }

    /// The notice as it should be delivered: an Interrupt in its root's open
    /// window becomes an Inform with the burst's line; any other passes as
    /// it is. Windows that closed are forgotten.
    pub fn fold(&mut self, n: Notice) -> Notice {
        self.fold_one(n).0
    }

    /// The notice as delivered, and whether the burst took it.
    fn fold_one(&mut self, mut n: Notice) -> (Notice, bool) {
        let window = self.window_ms;
        self.open.retain(|_, w| n.at_ms < w.start_ms + window);
        if n.urgency != Urgency::Interrupt {
            return (n, false);
        }
        let asked = matches!(n.why, Why::Asked | Why::Reminder);
        match self.open.get_mut(&n.root.id) {
            None => {
                self.open.insert(
                    n.root.id.clone(),
                    Window {
                        start_ms: n.at_ms,
                        count: 1,
                        questions: asked,
                    },
                );
                (n, false)
            }
            Some(w) => {
                w.count += 1;
                w.questions &= asked;
                let what = if w.questions { "questions" } else { "things" };
                n.urgency = Urgency::Inform;
                n.line = format!("{} {what} need you", w.count);
                (n, true)
            }
        }
    }

    /// Notices that reach a surface together, folded: one per root, the
    /// most urgent, with the burst's line once the burst took one, so a
    /// burst inside one delivery pings once and reads `3 questions need
    /// you`. A quiet notice (a retraction) is never merged.
    pub fn fold_all(&mut self, notices: Vec<Notice>) -> Vec<Notice> {
        let mut out: Vec<Notice> = Vec::new();
        for n in notices {
            let (n, burst) = self.fold_one(n);
            let same = |o: &&mut Notice| {
                o.root.id == n.root.id && o.urgency != Urgency::Quiet && n.urgency != Urgency::Quiet
            };
            match out.iter_mut().find(same) {
                Some(o) if burst || n.urgency > o.urgency => {
                    let urgency = o.urgency.max(n.urgency);
                    let why = if o.urgency >= n.urgency { o.why } else { n.why };
                    *o = Notice { urgency, why, ..n };
                }
                Some(_) => {}
                None => out.push(n),
            }
        }
        out
    }
}
