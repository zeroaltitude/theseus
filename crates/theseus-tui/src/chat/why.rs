//! `/why` (theseus-vjzt, phase 2): what the last compiled turn's model was
//! shown and why, the lines `theseus why` prints. Four reads go at once
//! (`context.explain`, `policy.explain`, `ledger.tail` for the route row,
//! `node.list` for the tool calls) and the lines are worded when all four
//! have answered. Only `context.explain` failing is told; a side read that
//! fails or does not decode leaves its section out.

use serde_json::{json, Value};
use theseus_protocol::context::ContextExplainResult;
use theseus_protocol::why::{tools_called, turn_rows, why_lines, WhyInput};
use theseus_protocol::{method, LedgerTailResult, NodeListResult, PolicyExplainResult, RpcError};

use super::state::{call, Chat, Effect, Purpose};
use super::style::Sty;

/// The most nodes and ledger rows read, as `theseus why` reads them.
const ROWS: usize = 1000;

/// Which of the four reads an answer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhyPart {
    Context,
    Policy,
    Route,
    Nodes,
}

/// The four answers so far; `None` is not yet answered, and a side read that
/// failed is `Some(None)`.
#[derive(Debug, Default)]
pub struct WhyRead {
    context: Option<Result<Value, String>>,
    policy: Option<Option<Value>>,
    route: Option<Option<Value>>,
    nodes: Option<Option<Value>>,
}

pub(super) fn why(chat: &mut Chat, _args: &str) -> Vec<Effect> {
    let Some(sid) = chat.session.clone() else {
        chat.flash("the conversation is not open yet");
        return Vec::new();
    };
    if chat.why.is_some() {
        chat.flash("still reading it");
        return Vec::new();
    }
    chat.why = Some(WhyRead::default());
    let ask = |m, params, p| call(m, params, Purpose::Why(p));
    vec![
        ask(
            method::CONTEXT_EXPLAIN,
            json!({ "session_id": sid }),
            WhyPart::Context,
        ),
        ask(
            method::POLICY_EXPLAIN,
            json!({ "session_id": sid }),
            WhyPart::Policy,
        ),
        ask(
            method::LEDGER_TAIL,
            json!({ "n": ROWS, "kind": "route.decided", "session_id": sid }),
            WhyPart::Route,
        ),
        ask(
            method::NODE_LIST,
            json!({ "session_id": sid, "kind": "tool_call", "n": ROWS }),
            WhyPart::Nodes,
        ),
    ]
}

impl Chat {
    /// One of `/why`'s answers; the words come with the fourth.
    pub(super) fn why_got(
        &mut self,
        part: WhyPart,
        result: Result<Value, RpcError>,
    ) -> Vec<Effect> {
        let Some(read) = self.why.as_mut() else {
            return Vec::new();
        };
        let side = |r: Result<Value, RpcError>| Some(r.ok());
        match part {
            WhyPart::Context => read.context = Some(result.map_err(|e| e.message)),
            WhyPart::Policy => read.policy = side(result),
            WhyPart::Route => read.route = side(result),
            WhyPart::Nodes => read.nodes = side(result),
        }
        if read.context.is_some()
            && read.policy.is_some()
            && read.route.is_some()
            && read.nodes.is_some()
        {
            if let Some(done) = self.why.take() {
                self.say_why(done);
            }
        }
        Vec::new()
    }

    fn say_why(&mut self, read: WhyRead) {
        let x = match read.context {
            Some(Ok(v)) => serde_json::from_value::<ContextExplainResult>(v)
                .map_err(|_| "the daemon's answer about the turn could not be read".to_string()),
            Some(Err(m)) => Err(format!("could not read it: {m}")),
            None => return,
        };
        let x = match x {
            Ok(x) => x,
            Err(m) => {
                self.say_words(Sty::Yellow, &m);
                return;
            }
        };
        let decoded = |v: Option<Option<Value>>| v.flatten();
        let policy = decoded(read.policy)
            .and_then(|v| serde_json::from_value::<PolicyExplainResult>(v).ok());
        let turn_id = x.turn_id.clone().unwrap_or_default();
        let route = decoded(read.route)
            .and_then(|v| serde_json::from_value::<LedgerTailResult>(v).ok())
            .and_then(|l| turn_rows(&l, &turn_id).pop());
        let tools = decoded(read.nodes)
            .and_then(|v| serde_json::from_value::<NodeListResult>(v).ok())
            .map(|n| tools_called(&n, &turn_id))
            .unwrap_or_default();
        let lines = why_lines(&WhyInput {
            explain: &x,
            policy: policy.as_ref(),
            route: route.as_ref(),
            tools: &tools,
            hide_sources: false,
        });
        self.gap();
        for l in lines {
            let sty = if l.starts_with('`') {
                Sty::Dim
            } else {
                Sty::Plain
            };
            self.say_plain(sty, &l);
        }
        self.gap();
    }
}
