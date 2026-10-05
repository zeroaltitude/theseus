//! The detail pane's text (design `stage2` §2.9): the focused session's
//! history (`session.history`), then its events as they come (`session.watch`),
//! as lines with the CLI's words and tags (`theseus_client::render`). The
//! reply streams into its open line, as the CLI's printer lays it out; every
//! other line is whole.

use std::collections::HashMap;

use theseus_client::render::{self, Show, Tag};
use theseus_protocol::{Event, NodeInfo, SessionHistoryResult};

/// The most lines the pane keeps; the oldest go first.
const KEPT: usize = 5_000;

/// What the pane shows of a session's events: the reply and each turn's
/// start and end; the thinking stays out, as `theseus watch` leaves it out
/// unless asked.
const SHOW: Show = Show {
    reply: true,
    thinking: false,
    turns: true,
};

#[derive(Debug, Default)]
pub struct Detail {
    pub session_id: String,
    /// Each line: its tag and its text, one row before wrapping.
    lines: Vec<(Tag, String)>,
    /// The reply's last line is open: the next piece goes on in it.
    open: bool,
    /// `session.history` has answered.
    pub loaded: bool,
    /// Rows scrolled up from the bottom; 0 follows the end.
    pub scroll: usize,
    /// Which session each turn belongs to, from the events that say: a delta
    /// names only its turn.
    turns: HashMap<String, String>,
    /// Messages this TUI sent whose `node.written` has not come yet.
    own: u32,
    /// Another surface's messages being read, in the order their
    /// `node.written` came: each node's id and its place, the line its text
    /// goes above (theseus-v6yc). `None`: its place was dropped past `KEPT`,
    /// so it goes at the end.
    places: Vec<(String, Option<usize>)>,
}

impl Detail {
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            ..Self::default()
        }
    }

    pub fn lines(&self) -> &[(Tag, String)] {
        &self.lines
    }

    /// The session's history, as `theseus history` prints it: it replaces
    /// what the pane held.
    pub fn history(&mut self, h: &SessionHistoryResult) {
        self.lines.clear();
        self.places.clear();
        self.open = false;
        self.loaded = true;
        for n in &h.nodes {
            self.node(n);
        }
    }

    /// One more node, after the history: an operator's message written by
    /// another surface. It goes in the place its `node.written` marked, above
    /// what came after (a reply that streamed while it was read), or at the
    /// end when nothing marked it.
    pub fn node(&mut self, n: &NodeInfo) {
        let at = self.places.iter().position(|(id, _)| *id == n.node_id);
        let place = at.and_then(|i| self.places[i].1);
        let Some(p) = place.filter(|p| *p < self.lines.len()) else {
            if let Some(i) = at {
                self.places.remove(i);
            }
            self.close();
            for l in render::node_lines(n, false) {
                self.push(l.tag, l.text);
            }
            return;
        };
        let i = at.expect("a place is a mark's");
        self.places.remove(i);
        let new: Vec<(Tag, String)> = render::node_lines(n, false)
            .into_iter()
            .map(|l| (l.tag, l.text.replace('\t', "    ")))
            .collect();
        let k = new.len();
        self.lines.splice(p..p, new);
        // The marks after it move down by its lines: a later mark at the same
        // place too, so the earlier message stays above it.
        for (j, (_, q)) in self.places.iter_mut().enumerate() {
            if let Some(q) = q {
                if *q > p || (*q == p && j >= i) {
                    *q += k;
                }
            }
        }
        self.trim();
    }

    /// Another surface's message `node_id` is being read: its place is here,
    /// after every line so far. The reply's open line closes, so what streams
    /// next starts below the place, never in a line the message splits.
    fn mark(&mut self, node_id: &str) {
        self.close();
        self.places
            .push((node_id.to_string(), Some(self.lines.len())));
    }

    /// The read of `node_id` found no node, or failed: its mark goes, and
    /// leaves nothing behind.
    pub fn unmark(&mut self, node_id: &str) {
        self.places.retain(|(id, _)| id != node_id);
    }

    /// The operator's own message, sent from the input line.
    pub fn sent(&mut self, input: &str) {
        self.own += 1;
        self.close();
        for (i, l) in input.split('\n').enumerate() {
            let text = if i == 0 {
                format!("you: {l}")
            } else {
                format!("     {l}")
            };
            self.push(Tag::Plain, text);
        }
    }

    /// A line of the TUI's own: an answer's refusal, a stop's result.
    pub fn note(&mut self, tag: Tag, text: &str) {
        self.close();
        for l in text.split('\n') {
            self.push(tag, l.to_string());
        }
    }

    /// One of the session's events. An event of another session is dropped:
    /// a turn this connection asked for still reports to it after the focus
    /// moved on.
    pub fn event(&mut self, e: &Event) {
        if let (Some(turn), Some(sid)) = (e.turn_id(), e.session_id()) {
            self.turns.insert(turn.to_string(), sid.to_string());
        }
        let ours = match (e.session_id(), e.turn_id()) {
            (Some(sid), _) => sid == self.session_id,
            (None, Some(turn)) => self
                .turns
                .get(turn)
                .is_none_or(|sid| *sid == self.session_id),
            (None, None) => true,
        };
        if !ours {
            return;
        }
        let question = matches!(e, Event::ConfirmRequested(_));
        for l in render::event(e, SHOW) {
            match l.tag {
                Tag::Reply => self.stream(&l.text),
                // A question's answers are the CLI's commands; the card says
                // the TUI's keys instead.
                Tag::Dim if question => {}
                tag => {
                    self.close();
                    self.push(tag, l.text);
                }
            }
        }
    }

    /// An operator's message was written in the session: false if it is one
    /// this TUI sent (its `you:` line shows it), so only another surface's is
    /// read, and its place in the pane is marked now.
    pub fn others_message(&mut self, node_id: &str) -> bool {
        if self.own > 0 {
            self.own -= 1;
            return false;
        }
        self.mark(node_id);
        true
    }

    /// A piece of the reply: it goes on in the open line, and each newline in
    /// it opens the next.
    fn stream(&mut self, text: &str) {
        for (i, piece) in text.split('\n').enumerate() {
            if i > 0 || !self.open {
                self.push(Tag::Reply, String::new());
            }
            if let Some((_, last)) = self.lines.last_mut() {
                last.push_str(piece);
            }
            self.open = true;
        }
    }

    fn close(&mut self) {
        // A reply's open line that holds nothing (it ended with a newline)
        // goes, so the next line follows its text.
        if self.open && self.lines.last().is_some_and(|(_, t)| t.is_empty()) {
            self.lines.pop();
        }
        self.open = false;
    }

    fn push(&mut self, tag: Tag, text: String) {
        self.lines.push((tag, text.replace('\t', "    ")));
        self.trim();
    }

    /// The oldest lines past `KEPT` go, and each mark moves up with what it
    /// stood above; one whose line went goes to the end.
    fn trim(&mut self) {
        if self.lines.len() > KEPT {
            let extra = self.lines.len() - KEPT;
            self.lines.drain(..extra);
            for (_, q) in &mut self.places {
                *q = q.and_then(|q| q.checked_sub(extra));
            }
        }
    }
}
