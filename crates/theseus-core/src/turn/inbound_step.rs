//! The inbound point's dispatch (M5 step 25a; `crate::judge::inbound`): a
//! turn whose input is a person's message hands it to the judge once its
//! node is written. The judge marks the turn's trace and spawns the rest;
//! the turn waits on nothing. The owner's correction is read here too
//! (theseus-q31l, `route_step::correct`).

use super::{Turn, TurnRunner};
use crate::judge::inbound::{author_of, place_kind, Inbound};
use crate::node::{AttachmentContent, Body, Node};

impl TurnRunner {
    /// A turn's id: the router's, minted when it began this message's inbound
    /// judgment early (theseus-rha2), else a new one (out of `run_inner_body`,
    /// within clippy's 100 lines).
    pub(super) fn turn_id_for(&self, sid: &str) -> String {
        self.threads
            .route
            .turn_id(sid)
            .unwrap_or_else(|| crate::new_id("turn"))
    }

    /// `node` is this turn's input, written; `author` the client that sent
    /// it (`sock#3`, `web#1`, or the binding's person). With the judge off,
    /// nothing is read.
    ///
    /// `steered`: the message joined a running turn at a loop boundary
    /// (theseus-qu1lc). Its judgments run and are recorded as a new turn's
    /// do, but the turn's route stays the running one: the route verdict and
    /// the owner's words about the route apply from the next turn.
    pub(super) fn inbound_point(&self, t: &mut Turn<'_>, node: &Node, author: &str, steered: bool) {
        let Body::UserMessage { text, attachments } = &node.body else {
            return;
        };
        // The owner's own message, for the card's observer (theseus-6f8t0):
        // marked with the judge off too.
        if self.standing.expected.take_owner(t.tc.session_id, text) {
            self.standing.expected.mark_owner(&node.id);
        }
        // What the turn's tools are chosen by, with the judge off too.
        if !steered {
            t.route.ask = text
                .chars()
                .take(crate::tools_defer::want::ASK_CHARS)
                .collect();
            t.route.asker = author.to_string();
        }
        // A message the router placed (theseus-rha2): its point was begun
        // before the choice, and the row of its choice rides this turn's frame.
        let begun = self
            .threads
            .route
            .take_handoff(t.tc.session_id, text)
            .and_then(|h| {
                if let Some(row) = &h.row {
                    t.record(&crate::fact::thread_route::ThreadRouted(row));
                }
                h.early
            });
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
        // message the router judged early (thread-route) asked `directive.v1`
        // in that call, by the same rule (`Core::capture_wanted`).
        let noted = self.standing.expected.take(t.tc.session_id, text);
        let capture = noted
            && self.cfg.standing.capture
            && t.tc.task.is_none()
            && crate::capture::owner_alone(class, place.as_deref());
        let early = match begun {
            Some(early) => Some((early, Some((t.tc.session_id.to_string(), node.id.clone())))),
            None => self
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
                    threads: Vec::new(),
                    in_thread: !steered && self.scope_in_thread(t.tc.session_id),
                    persona: self.persona_ctx(t.tc.session_id, class),
                    capture,
                    data_names: self.cfg.privacy.data_names(),
                    attachments: attachments.clone(),
                    restricted: theseus_judge::builders::RestrictedModels::allowed_if(
                        self.cn_read(t) == super::data_step::Cn::Allowed,
                    ),
                })
                .map(|e| (e, None)),
        };
        // classify.v4's `scope` answer, for the place digest (theseus-98bz.9):
        // a steered message's is not the running turn's.
        let wait = early.and_then(|(mut e, said)| {
            if !steered {
                t.digest.scope = e.scope.take();
            }
            e.adopt(&mut t.trace, said)
        });
        if steered {
            return;
        }
        let asked = wait.is_some();
        Self::route_asked(t, wait, route, images);
        // The owner's correction, or the layer (theseus-q31l): a turn that
        // asked the route pack can be steered; any turn's words can label.
        self.correction_point(t, &node.id, text, author, place.as_deref(), route, asked);
        // Then the persona, which a correction may have placed (theseus-c1uz).
        self.engage_start(t, text, asked);
    }
}
