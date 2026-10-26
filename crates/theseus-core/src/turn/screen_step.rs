//! The tool screen (theseus-a1wvq.9): a tool's result joins an open request's
//! run only once `data::screen` passes it. A result that fails ends the run
//! there, never the session: it stays written, it is not in the turn's
//! screened set, so the next loop's clearance refuses it (`tool_result`) and
//! `data_move` takes that loop to the `[model]` default, and the turn stores
//! no run past it. An open session (`data.opened`) records the failure as its
//! taint, bookkeeping as every taint is; a private-view session records
//! no taint. Either way the failure's word is a `session.data` `screened`
//! row (theseus-lm38z).

use super::*;
use crate::data::{self, DataWhy, Taint};
use crate::provider::ToolUse;
use crate::stub::Kind;

impl TurnRunner {
    /// Screens the results the loop's calls wrote, once, right after its calls ran.
    /// It runs only for an open request's loop: the turn admitted, its target
    /// of restricted origin, and its request compiled in the open view; any
    /// other turn pays this one branch. A restricted, admitted turn with no
    /// open view is a bug, and fails closed as if every result failed.
    ///
    /// A call with no result node yet (one waiting for a confirm, a
    /// background job's later result) is not screened here: another writer
    /// writes its result later, and the run's chain ends at it.
    pub(super) fn screen_loop(&self, t: &mut Turn<'_>, session: &SessionRecord, uses: &[ToolUse]) {
        if t.route.admitted.0.is_err() || !self.restricted_profile(&t.target.profile) {
            return;
        }
        let Ok(nodes) = t.tc.store.transcript(t.tc.session_id) else {
            return;
        };
        let turn = t.tc.turn_id;
        let names = self.cfg.privacy.names();
        // What a program's provenance is read against (theseus-ouqru).
        let seal = self.seal.lock().unwrap().clone();
        let seal = seal.unwrap_or_else(|| crate::seal::of().clone());
        let public_reach = data::public_reach(&self.cfg.routing.data.public_reach);
        let must_cover = crate::sandbox::config_covers();
        let mut failed = None;
        for (_, n) in nodes
            .iter()
            .rev()
            .take_while(|(_, n)| n.turn_id.as_deref() == Some(turn))
        {
            let (
                Kind::ToolResult,
                Body::ToolResult {
                    tool_use_id,
                    content,
                    meta,
                    ..
                },
            ) = (n.kind, &n.body)
            else {
                continue;
            };
            let Some(u) = uses.iter().find(|u| &u.id == tool_use_id) else {
                continue;
            };
            let redactions = meta.get("redactions").and_then(|r| r.as_u64()).unwrap_or(0);
            let final_url = meta.get("final_url").and_then(|u| u.as_str());
            let mut via = None;
            let verdict = match t.tc.open {
                None => Err(data::Unscreened::Tool),
                Some(open) => {
                    self.tools
                        .tool_by_wire(&u.name)
                        .map_or(Err(data::Unscreened::Tool), |tool| {
                            let plan = tool.plan(&u.input, &self.tools.ctx_in(&t.tc)).ok();
                            let s = data::Screening {
                                tool: tool.name(),
                                plan: plan.as_ref(),
                                redactions,
                                final_url,
                                content,
                                names: &names,
                                proof: data::Proof {
                                    detail: meta.get("detail"),
                                    steps: meta.get("steps"),
                                    seal: &seal,
                                    public_reach: &public_reach,
                                    must_cover: &must_cover,
                                },
                                open,
                            };
                            let passed = data::screen(&s);
                            if passed.is_ok() && crate::open_view::runs_tool(tool.name()) {
                                via = data::provenance::via(&s);
                            }
                            passed
                        })
                }
            };
            match verdict {
                Ok(()) => {
                    t.route.screened.push(n.id.clone());
                    // A program's result passed on provenance: say which.
                    if let Some(via) = via {
                        t.record(&crate::fact::data::SessionDataProgram {
                            node: &n.id,
                            via: via.as_str(),
                            seal: seal.reason(),
                        });
                    }
                }
                // The transcript is read newest first: keep the earliest failure.
                Err(why) => failed = Some((why, n.id.clone())),
            }
        }
        let Some((why, node)) = failed else {
            return;
        };
        tracing::info!(turn = %turn, node = %node, word = why.as_str(), "a tool result failed the screen");
        t.record(&crate::fact::data::SessionDataScreened {
            node: &node,
            word: why.as_str(),
        });
        if self.open_of(session).is_some() {
            t.data.taint.get_or_insert_with(|| Taint {
                why: DataWhy::ToolResult,
                node: Some(node),
                at_ms: theseus_protocol::now_unix_ms(),
            });
        }
    }
}
