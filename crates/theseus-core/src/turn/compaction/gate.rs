//! A summary in an open request (theseus-a1wvq.10): where it may go, and
//! whether it joins the run.
//!
//! A compaction's range is judged as one request is: `data::clearance` over
//! the nodes the summary call renders and the run's last. A cleared range may
//! go to a model of restricted origin with a pass of its own, and the summary
//! it makes is the loop's (`Held.summaries`). Not cleared, a restricted
//! summary profile is never sent the range: the turn's own restricted target
//! keeps the ring, a Claude turn has it written by the `[model]` default.

use std::collections::HashSet;

use super::super::data_step::DEFAULT_PROFILE;
use super::super::open_step::writer;
use super::super::{Target, Turn, TurnRunner};
use super::dropped;
use crate::compiler::Compiled;
use crate::data::{self, Carried, Carry, Clearance, DataWhy};
use crate::session::SessionRecord;
use crate::stub::Stub;

/// What a summary call is: its profile, its target when that is the turn's
/// own, the range's clearance (none outside the open view), and the words
/// for a row when the gate moved it.
pub(super) struct Route {
    pub profile: String,
    pub own: Option<Target>,
    pub cleared: Option<Clearance>,
    pub note: Option<String>,
}

impl Route {
    /// Whether the range was cleared.
    pub fn is_cleared(&self) -> bool {
        self.cleared.as_ref().is_some_and(Clearance::is_cleared)
    }
}

impl TurnRunner {
    /// The summary's route, or why the ring stands. A profile of US origin
    /// goes as asked; a restricted one only with a cleared range.
    pub(super) fn summary_route(
        &self,
        t: &Turn<'_>,
        ring: &Compiled,
        nodes: &[(u64, Stub)],
        (profile, own): (String, Option<Target>),
    ) -> Result<Route, String> {
        let cleared = open_run(t)
            .filter(|_| t.tc.open.is_some())
            .map(|run| range_clearance(t, &run, ring, nodes));
        let mut route = Route {
            profile,
            own,
            cleared,
            note: None,
        };
        if route.is_cleared() || !self.restricted_profile(&route.profile) {
            return Ok(route);
        }
        let word = route
            .cleared
            .as_ref()
            .and_then(Clearance::why)
            .unwrap_or(DataWhy::PrivateView)
            .as_str();
        let why = format!("data_gate: {word}");
        if self.restricted_profile(&t.target.profile) {
            return Err(why);
        }
        if self.restricted_profile(DEFAULT_PROFILE) {
            return Err(format!(
                "{why}; `{DEFAULT_PROFILE}` is of restricted origin or does not resolve"
            ));
        }
        route.profile = DEFAULT_PROFILE.into();
        route.own = None;
        route.note = Some(why);
        Ok(route)
    }
}

/// The run the range is judged over: the turn's own once its admission is
/// read; before that (the first loop compiles beside the verdict) the
/// session's stored run, unless it is tainted. The range is what came before
/// the turn's input, so the stored run is the one that covers it.
fn open_run(t: &Turn<'_>) -> Option<data::OpenRun> {
    match &t.route.admitted.0 {
        Ok(run) => Some(run.clone()),
        Err(DataWhy::Unchecked) => {
            let rec: SessionRecord = t.tc.store.get_session(t.tc.session_id).ok()??;
            let state = rec.data.as_deref()?;
            state.taint.is_none().then(|| state.run.clone()).flatten()
        }
        Err(_) => None,
    }
}

/// The clearance of the range `ring` dropped: the nodes the summary call
/// renders, with the run's last, which the request about to go carries too.
fn range_clearance(
    t: &Turn<'_>,
    run: &data::OpenRun,
    ring: &Compiled,
    nodes: &[(u64, Stub)],
) -> Clearance {
    let c = &ring.compilation;
    let Ok(range) = dropped(nodes, &c.includes, c.as_of) else {
        return Clearance::closed(DataWhy::Unchecked);
    };
    let mut ids: HashSet<&str> = range.nodes.iter().map(|(_, n)| n.id.as_str()).collect();
    ids.extend(range.folded.map(|n| n.id.as_str()));
    judge(
        run,
        nodes,
        &ids,
        (t.tc.turn_id, &t.route.screened, &t.data.summaries),
    )
}

/// `data::clearance` over the transcript's nodes in `ids` and the run's last.
fn judge(
    run: &data::OpenRun,
    nodes: &[(u64, Stub)],
    ids: &HashSet<&str>,
    (turn, screened, summaries): (&str, &[String], &[String]),
) -> Clearance {
    let carried = Carried {
        open_view: true,
        from: nodes
            .iter()
            .find(|(_, n)| n.id == run.from)
            .map(|(p, _)| *p),
        nodes: nodes
            .iter()
            .filter(|(_, n)| n.id == run.through || ids.contains(n.id.as_str()))
            .map(|(p, n)| Carry {
                id: n.id.clone(),
                at: *p,
                writer: writer(n, turn, screened, summaries),
            })
            .collect(),
    };
    data::clearance(run, &carried)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::OpenRun;
    use crate::node::{Body, Node, ResultStatus};
    use serde_json::json;

    const TURN: &str = "turn_1";

    fn user() -> Node {
        Node::user("ses_1", Some("turn_0"), "cli", "hello")
    }

    fn own() -> Node {
        Node::recall("ses_1", TURN, "rcl_1", "baseline", vec![])
    }

    fn tool() -> Node {
        let body = Body::ToolResult {
            tool_use_id: "t1".into(),
            tool: "fs.read".into(),
            status: ResultStatus::Ok,
            is_error: false,
            content: "a page".into(),
            correlation_id: None,
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: None,
            late: false,
            meta: json!({}),
            image: None,
            external: None,
        };
        Node::tool_result("ses_1", Some(TURN), None, body)
    }

    /// The range of positions 2 to 4 (`a` to `c`, the run) and the nodes
    /// after it, as the summary request carries them: the answer's why.
    fn why(before: Option<Node>, after: Option<Node>) -> Option<DataWhy> {
        let mut nodes: Vec<Node> = before.into_iter().collect();
        let first = nodes.len();
        nodes.extend([user(), user(), user()]);
        nodes.extend(after);
        let stubs: Vec<(u64, Stub)> = nodes
            .into_iter()
            .enumerate()
            .map(|(i, n)| (i as u64 + 1, n.into()))
            .collect();
        let run = OpenRun {
            from: stubs[first].1.id.clone(),
            through: stubs[first + 2].1.id.clone(),
        };
        let ids: HashSet<&str> = stubs.iter().map(|(_, n)| n.id.as_str()).collect();
        judge(&run, &stubs, &ids, (TURN, &[], &[])).why()
    }

    /// Every case of a range's clearance, as a request's is judged.
    #[test]
    fn range_clearance_matrix() {
        assert_eq!(why(None, None), None, "a range wholly in the run");
        assert_eq!(
            why(Some(user()), None),
            Some(DataWhy::NotInRun),
            "a node before the run's first"
        );
        assert_eq!(why(None, Some(tool())), Some(DataWhy::ToolResult));
        assert_eq!(why(None, Some(user())), Some(DataWhy::Unvetted));
        assert_eq!(
            why(None, Some(own())),
            None,
            "this loop's nodes past the run"
        );
    }

    /// A summary is the loop's own only when its range was cleared: one of an
    /// uncleared range stays another writer's, though a Claude profile wrote
    /// it, so a run is never stored through it.
    #[test]
    fn a_summary_of_an_uncleared_range_never_joins_the_run() {
        let n = Node::summary(
            "ses_1",
            TURN,
            Body::Summary {
                first: 1,
                last: 2,
                nodes: 2,
                text: "the gist".into(),
                profile: "sonnet".into(),
                model: "claude-sonnet-5-5".into(),
                cost_usd: None,
                header: String::new(),
            },
        );
        let stub: Stub = n.into();
        assert_eq!(writer(&stub, TURN, &[], &[]), data::Writer::Other);
        assert_eq!(
            writer(&stub, "turn_2", &[], std::slice::from_ref(&stub.id)),
            data::Writer::Loop,
            "listed by its turn's compaction"
        );
        assert_eq!(
            writer(&stub, TURN, &[], &["sum_other".into()]),
            data::Writer::Other
        );
    }

    /// Outside the open view nothing is cleared, whatever the range holds.
    #[test]
    fn a_range_outside_the_open_view_is_private() {
        let run = OpenRun {
            from: "a".into(),
            through: "a".into(),
        };
        let c = Carried {
            open_view: false,
            from: Some(1),
            nodes: vec![Carry {
                id: "a".into(),
                at: 1,
                writer: data::Writer::Other,
            }],
        };
        assert_eq!(data::clearance(&run, &c).why(), Some(DataWhy::PrivateView));
    }
}
