//! The inbound point's dispatch (M5 step 25a; `crate::judge::inbound`): a
//! turn whose input is a person's message hands it to the judge once its
//! node is written. The judge marks the turn's trace and spawns the rest;
//! the turn waits on nothing. The owner's correction is read here too
//! (theseus-q31l, `route_step::correct`).

use super::{Turn, TurnRunner};
use crate::judge::inbound::{author_of, place_kind, Inbound};
use crate::node::{AttachmentContent, Body, Node, Origin};

impl TurnRunner {
    /// `node` is this turn's input, written; `author` the client that sent
    /// it (`sock#3`, `web#1`, or the binding's person). With the judge off,
    /// nothing is read.
    ///
    /// `steered`: the message joined a running turn at a loop boundary
    /// (theseus-qu1lc). Its judgments run and are recorded as a new turn's
    /// do, but the turn's route stays the running one: the route verdict and
    /// the owner's words about the route apply from the next turn.
    pub(super) fn inbound_point(&self, t: &mut Turn<'_>, node: &Node, author: &str, steered: bool) {
        let Body::UserMessage {
            text, attachments, ..
        } = &node.body
        else {
            return;
        };
        // The owner's own message, for the card's observer (theseus-6f8t0):
        // marked with the judge off too.
        if self.standing.expected.take_owner(t.tc.session_id, text) {
            self.standing.expected.mark_owner(&node.id);
        }
        // What the turn's tools are chosen by, with the judge off too.
        if !steered {
            // The owner's own words that began this turn, for a settings question
            // of it to put to rest (`capture::settled`): not a report or a wake
            // the turn read first, not a message steered in.
            if node.origin == Origin::Operator {
                self.standing.expected.note_input(t.tc.turn_id, &node.id);
            }
            t.route.ask = text
                .chars()
                .take(crate::tools_defer::want::ASK_CHARS)
                .collect();
        }
        if !self.judge.config().enabled {
            if !steered {
                self.engage_start(t, text, false);
            }
            return;
        }
        let class = t.tc.class;
        let place = self.outbox.try_target(t.tc.session_id).ok().flatten();
        // route.v1 (25e): live, or shadow for a turn whose profile the
        // owner chose; its verdict's channel waits for the first compile. A
        // routed session's turn read it at its start (theseus-9yyr).
        let read = match steered {
            true => None,
            false => t.route.read.take(),
        };
        let route = read.unwrap_or_else(|| self.route_mode(t.target, t.tc.session_id));
        // Jev's judgment of the tool families, beside the rest (theseus-xlri5).
        if !steered {
            self.families_start(t, route);
        }
        // A pin after a routed turn counts on route.v1's ladder (26a).
        if t.target.chosen.is_some() && !steered {
            self.judge.pinned(t.tc.session_id, &t.target.profile);
        }
        let images = attachments
            .iter()
            .any(|a| matches!(a.content, AttachmentContent::Image { .. }));
        // The owner's own words, in a place only the owner writes in: the
        // note `turn.submit` made is spent either way (theseus-slvl.5). A
        // message the router judged early (thread-route) asked `directive.v2`
        // in that call, by the same rule (`Core::capture_wanted`).
        let noted = self.standing.expected.take(t.tc.session_id, text);
        let capture = noted
            && self.cfg.standing.capture
            && t.tc.task.is_none()
            && crate::capture::owner_alone(class, place.as_deref());
        let early = self
            .judge
            .inbound_early(Inbound {
                session_id: t.tc.session_id.into(),
                execution_id: t.tc.execution_id.into(),
                turn_id: t.tc.turn_id.into(),
                node_id: node.id.clone(),
                text: text.clone(),
                place_kind: place_kind(place.as_deref(), author, class),
                author: author_of(class).into(),
                kernel: self.kernel.clone(),
                route,
                chosen: t.target.chosen.clone(),
                task: t.tc.task.is_some(),
                own: super::defer_step::is_self(author),
                persona: self.persona_ctx(t.tc.session_id, class),
                capture,
                data_names: self.cfg.privacy.data_names(),
                attachments: attachments.clone(),
            })
            .map(|e| (e, None));
        let mut breaker_open = false;
        let wait = early.and_then(|(e, said)| {
            breaker_open = e.breaker_open;
            e.adopt(&mut t.trace, said)
        });
        if steered {
            return;
        }
        let asked = wait.is_some();
        Self::route_asked(t, wait, route, images, breaker_open);
        // The owner's correction by a press or a reaction (theseus-q31l): a
        // turn that asked the route pack can be steered by it.
        self.correction_point(t, asked);
        // Then the persona, which a correction may have placed (theseus-c1uz).
        self.engage_start(t, text, asked);
    }
}
