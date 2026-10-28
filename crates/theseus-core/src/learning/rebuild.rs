//! States rebuilt from the record (M5 25d; design §2.5, §2.9): a judged
//! point's builder input read back from what the session wrote, with the
//! live point's own input function, at the event's time as `now`. A replay
//! rebuilds a judgment's state when the candidate's builder, its version, or
//! its cap differ from the judgment's; a backfill rebuilds every event in its
//! window.
//!
//! `loop.v1`'s point is rebuilt from the turn's `turn.ended` row (its loops,
//! cost, tool calls, and stop reason) and the session's nodes up to the
//! turn's end (the ask, this turn's calls and their results, and the turn's
//! reply: its assistant messages' text, joined as the turn joins them), given
//! to `judge::loop_end::input`, the function the live point calls. Every
//! other builder's input is refused with the reason ([`unrebuildable`]).

use serde_json::Value;
use theseus_judge::builders::{ContentInput, DataNames, LoopInput};
use theseus_judge::pack::Builder;
use theseus_protocol::LedgerKind;

use crate::content::ItemKind;
use crate::content_scene::{Around, Scene};
use crate::judge::loop_end::{self, LoopEnd};
use crate::ledger::LedgerRow;
use crate::node::{Body, Node};
use crate::rpc::Core;

/// Why a builder's input can't be rebuilt from the record; none when it
/// can.
pub fn unrebuildable(b: Builder) -> Option<&'static str> {
    match b {
        Builder::Loop => None,
        Builder::Security | Builder::Security2 => Some(
            "a gate state's input is the gate's decision at the call (posture and reason) and the \
             session's hold at that moment, which the record keeps only in part",
        ),
        Builder::Inbound => Some(
            "an inbound state's input reads the live tasks and the roles table as they stood at the \
             message, which the record does not keep by time",
        ),
        Builder::Continue => Some(
            "CONTINUE's input is one compile's signals beside the tail's tokens and the provider's \
             cache reads at that moment, which are not kept",
        ),
        Builder::Categorize => Some(
            "categorize's input reads the ontology's topics and memberships as they stood, which \
             the snapshot does not keep by time",
        ),
        Builder::Rerank => Some(
            "rerank's input is recall's candidates at that turn, read from the index as it stood \
             then, which is not kept",
        ),
        Builder::Probe => Some("the probe's state is synthetic: no point records it"),
        Builder::Memory | Builder::Attribution => Some(
            "the memory pass's states (a labeled node, or a reply and the notes recall admitted \
             to it) are not rebuilt from the record: only loop.v1's input is, so far",
        ),
        Builder::Citation => Some(
            "a citation check's input is a synthesis's text before it was kept, and a rejected \
             one is kept only in its row",
        ),
        Builder::People => Some(
            "a people state's candidate is an extractor's answer and the held people as they \
             stood, which the record keeps only in the judgment's context",
        ),
        Builder::Compare => Some(
            "a compare state is a replay's two replies, kept in the replay's row and not by the \
             judgment's record alone",
        ),
        Builder::RecallCross => Some(
            "a cross-stream state is the notes the thread lens dropped at that turn, read from \
             the index as it stood then, which is not kept",
        ),
        Builder::Families => Some(
            "a families state is the tool families the turn's request hid, from the catalog and \
             the board's counts as they stood then, which are not kept",
        ),
        // One item, and its state is in the judgment's blob: `content_input`.
        Builder::Content => None,
        Builder::Heard => Some(
            "a voice transcript's state is one utterance's text, the sentences it may echo and \
             its timing, which the record keeps only in the judgment's blob",
        ),
        Builder::SameThing
        | Builder::TwoThings
        | Builder::SameKind
        | Builder::NameIt => Some(
            "a sweep's state is the categories and sessions' titles as they stood on the night, \
             which the record keeps only in the judgment's blob",
        ),
        Builder::PeopleSeen => Some(
            "a people gate's state is an exchange's lines since a mark that has moved and the held \
             people as they stood",
        ),
    }
}

/// What the daemon says about an item now, for a state that holds none of v3's
/// fields: the config, the session's place and directory, and the paths the
/// call behind it named.
pub struct Now<'a> {
    pub around: &'a Around,
    pub scene: &'a Scene,
    pub paths: &'a [String],
}

/// A content state's input read back from its blob (theseus-gyjyo.1): the
/// item's kind, source, piece and text, and the operator's names, as the live
/// point put them in. The text is what the state held, scrubbed and capped, so
/// building from it again gives the same state when the candidate's cap is the
/// same, and a state under the candidate's own cap when it is not.
///
/// The five fields `content.v3` adds (`author`, `place`, `project`, `public`,
/// `context`, theseus-gyjyo.11) are taken from the blob when it holds any, as
/// judged. A blob that holds none (a v1 or v2 judgment's) gets them from `now`:
/// the author from its kind, and the rest as the daemon would state them for
/// that item today, from the session's record and the config. **Those are read
/// at replay time, not as they stood when the item was judged**: a session that
/// has since moved directory, or a config that has since changed, is asked
/// about. `None` leaves them unset, which is what a v2 candidate wants.
pub fn content_input(state_json: &str, now: Option<Now<'_>>) -> Result<ContentInput, String> {
    let v: Value = serde_json::from_str(state_json)
        .map_err(|e| format!("its state's blob is not JSON: {e}"))?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let n = |k: &str| v.get(k).and_then(Value::as_u64).map(|n| n as u32);
    let names = |k: &str| -> Vec<String> {
        v["data_names"][k]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|n| n.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut input = ContentInput {
        text: s("text").ok_or("its state's blob holds no text")?,
        kind: s("kind").ok_or("its state's blob names no kind")?,
        source: s("source").unwrap_or_default(),
        piece: n("piece").zip(n("pieces")),
        data_names: DataNames {
            open: names("open"),
            private: names("private"),
        },
        ..ContentInput::default()
    };
    let judged_with_fields = ["author", "place", "project", "public", "context"]
        .iter()
        .any(|k| v.get(*k).is_some());
    if judged_with_fields {
        input.author = s("author");
        input.place = s("place");
        input.project = s("project");
        input.public = v.get("public").and_then(Value::as_bool).unwrap_or(false);
        input.context = s("context");
    } else if let Some(now) = now {
        let kind: ItemKind = serde_json::from_value(Value::String(input.kind.clone()))
            .map_err(|_| format!("its state's kind {:?} is not an item kind", input.kind))?;
        let mut paths = crate::content_scene::paths_in(&input.source);
        paths.extend(now.paths.iter().cloned());
        now.around.apply(&mut input, kind, now.scene, &paths);
    }
    Ok(input)
}

/// A turn's end, as its `turn.ended` row records it.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnEnd {
    pub session_id: String,
    pub turn_id: String,
    /// When its row was written: the event's time.
    pub at_ms: u64,
    pub loops: u32,
    pub cost_usd: Option<f64>,
    pub tool_calls: u32,
    pub stop_reason: String,
    /// The model that ended it.
    pub model: String,
}

impl TurnEnd {
    /// From a `turn.ended` row, when it is one and names its turn.
    pub fn of(row: &LedgerRow) -> Option<TurnEnd> {
        if row.kind != LedgerKind::TurnEnded.as_str() {
            return None;
        }
        let d = &row.data;
        Some(TurnEnd {
            session_id: row.session_id.clone()?,
            turn_id: row.turn_id.clone()?,
            at_ms: row.at_unix_ms,
            loops: d["loops"].as_u64().unwrap_or(0) as u32,
            cost_usd: d["cost_usd"].as_f64(),
            tool_calls: d["tool_calls"].as_u64().unwrap_or(0) as u32,
            stop_reason: d["stop_reason"].as_str().unwrap_or_default().to_string(),
            model: d["model"].as_str().unwrap_or_default().to_string(),
        })
    }
}

/// The turn's reply as the turn made it: each of its assistant messages'
/// text that is not empty, in order, joined by a blank line.
pub fn reply_of(nodes: &[Node], turn_id: &str) -> String {
    nodes
        .iter()
        .filter(|n| n.turn_id.as_deref() == Some(turn_id))
        .filter_map(|n| match &n.body {
            Body::AssistantMessage { blocks, .. } => {
                Some(crate::provider::text_of(blocks)).filter(|t| !t.is_empty())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `loop.v1`'s input for a turn, from its session's nodes (oldest first)
/// and its end, with the end's time as `now`: what the live point built at
/// the turn's end.
pub fn loop_input(nodes: &[Node], end: &TurnEnd, task: bool) -> LoopInput {
    let upto: Vec<Node> = nodes
        .iter()
        .filter(|n| n.created_at_ms <= end.at_ms)
        .cloned()
        .collect();
    let live = LoopEnd {
        session_id: end.session_id.clone(),
        execution_id: String::new(),
        turn_id: end.turn_id.clone(),
        task,
        output: reply_of(&upto, &end.turn_id),
        loops: end.loops,
        cost_usd: end.cost_usd,
        tool_calls: end.tool_calls,
    };
    loop_end::input(&upto, &live, end.at_ms)
}

impl Core {
    /// A session's `turn.ended` rows, oldest first, by one ledger page per
    /// hundred rows through the index. `Err` while the index's shape is
    /// built after a start.
    pub(crate) fn turn_ends(
        &self,
        session: Option<&str>,
        since_ms: u64,
    ) -> anyhow::Result<Vec<TurnEnd>> {
        let tags = crate::rpc::ledger_tags(Some(LedgerKind::TurnEnded.as_str()), session);
        let mut after = None;
        let mut out = Vec::new();
        loop {
            let page = theseus_store::Page {
                kind: theseus_store::kinds::LEDGER,
                tags: tags.clone(),
                after: Some(after.unwrap_or(0)),
                before: None,
                since_ms: Some(since_ms),
                until_ms: None,
                limit: 100,
            };
            let Some(got) = self.store.ledger_page(&page)? else {
                anyhow::bail!(
                    "the ledger's index is still being built after the start; try again in a minute"
                );
            };
            for r in &got.records {
                if let Ok(row) = r.decode::<LedgerRow>() {
                    out.extend(TurnEnd::of(&row));
                }
            }
            if !got.more || got.last.is_none() {
                return Ok(out);
            }
            after = got.last;
        }
    }

    /// Whether a session is a task's (its turns' class is `task`).
    pub(crate) fn is_task_session(&self, session: &str) -> bool {
        self.store
            .get_session::<crate::session::SessionRecord>(session)
            .ok()
            .flatten()
            .is_some_and(|r| r.task.is_some())
    }

    /// `loop.v1`'s input for a judged turn, rebuilt from the record: the
    /// turn's `turn.ended` row and its session's nodes. `Err`: why not.
    pub(crate) fn rebuild_loop(&self, context: &Value) -> Result<LoopInput, String> {
        let s = |k: &str| context.get(k).and_then(Value::as_str);
        let (Some(session), Some(turn)) = (s("session"), s("turn")) else {
            return Err("its judgment names no session and turn".into());
        };
        let end = self
            .turn_ends(Some(session), 0)
            .map_err(|e| format!("{e:#}"))?
            .into_iter()
            .find(|e| e.turn_id == turn)
            .ok_or_else(|| format!("no turn.ended row is recorded for turn {turn}"))?;
        let nodes: Vec<Node> = self
            .store
            .session_nodes(session)
            .map_err(|e| format!("its session's nodes were not read: {e:#}"))?
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        Ok(loop_input(&nodes, &end, self.is_task_session(session)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_judge::pack::by_name;
    use theseus_judge::{prepare, Input, NoScrub};

    /// A content state read back from its blob and built again is the stored
    /// state, field for field: kind, source, piece, the operator's names and
    /// the text. Checked on an item of one piece and on one of several that
    /// carries names.
    #[test]
    fn a_rebuilt_content_state_is_the_stored_one() {
        let pack = by_name("content.v2").unwrap();
        for (kind, piece, names) in [
            ("fetched_page", None, DataNames::default()),
            (
                "tool_result",
                Some((2, 3)),
                DataNames {
                    open: vec!["Widgetco".into()],
                    private: vec!["Acme".into()],
                },
            ),
        ] {
            let input = ContentInput {
                text: "A page about herons.\nSecond line.".into(),
                kind: kind.into(),
                source: "fetch https://example.test/page".into(),
                piece,
                data_names: names,
                ..ContentInput::default()
            };
            let stored = prepare(&pack, &Input::Content(input.clone()), &NoScrub).unwrap();
            let back = content_input(&stored.state.json, None).unwrap();
            assert_eq!(back, input, "{kind}");
            let again = prepare(&pack, &Input::Content(back), &NoScrub).unwrap();
            assert_eq!(again.state.json, stored.state.json, "{kind}");
            assert_eq!(again.dynamic, stored.dynamic, "{kind}");
        }
    }

    /// A v3 state's five fields come back from its blob as they were judged, and
    /// building from them again gives the same state.
    #[test]
    fn a_v3_state_is_rebuilt_with_the_fields_it_was_judged_with() {
        let pack = by_name("content.v3").unwrap();
        let input = ContentInput {
            text: "A page about herons.".into(),
            kind: "tool_result".into(),
            source: "fs.grep".into(),
            author: Some("tool".into()),
            place: Some("shared".into()),
            project: Some("~/projects/lantern".into()),
            public: true,
            context: Some("As judged.".into()),
            ..ContentInput::default()
        };
        let stored = prepare(&pack, &Input::Content(input.clone()), &NoScrub).unwrap();
        // The session and the config have changed since: they are not asked.
        let around = Around::new(vec![], "Now.", None);
        let scene = Scene {
            place: Some("private"),
            dir: Some("/elsewhere".into()),
        };
        let back = content_input(
            &stored.state.json,
            Some(Now {
                around: &around,
                scene: &scene,
                paths: &[],
            }),
        )
        .unwrap();
        assert_eq!(back, input);
        let again = prepare(&pack, &Input::Content(back), &NoScrub).unwrap();
        assert_eq!(again.state.json, stored.state.json);
    }

    /// A v2 state has none: they are the daemon's reading now, the author from
    /// the kind; with nothing given they stay unset, which a v2 candidate wants.
    #[test]
    fn a_v2_state_gets_the_fields_from_now_or_none() {
        let v2 = by_name("content.v2").unwrap();
        let input = ContentInput {
            text: "Count the herons.".into(),
            kind: "operator_message".into(),
            source: "message".into(),
            ..ContentInput::default()
        };
        let stored = prepare(&v2, &Input::Content(input.clone()), &NoScrub).unwrap();
        assert_eq!(content_input(&stored.state.json, None).unwrap(), input);
        let around = Around::new(
            vec!["/home/owner/projects/lantern".into()],
            "Now.",
            Some("/home/owner".into()),
        );
        let scene = Scene {
            place: Some("private"),
            dir: Some("/home/owner/projects/lantern".into()),
        };
        let got = content_input(
            &stored.state.json,
            Some(Now {
                around: &around,
                scene: &scene,
                paths: &[],
            }),
        )
        .unwrap();
        assert_eq!(got.author.as_deref(), Some("owner"));
        assert_eq!(got.place.as_deref(), Some("private"));
        assert_eq!(got.project.as_deref(), Some("~/projects/lantern"));
        assert!(got.public);
        assert_eq!(got.context.as_deref(), Some("Now."));
        // A kind that is no item kind is refused with the reason, not guessed.
        assert!(content_input(
            r#"{"text":"x","kind":"carrier_pigeon"}"#,
            Some(Now {
                around: &around,
                scene: &scene,
                paths: &[]
            })
        )
        .unwrap_err()
        .contains("not an item kind"));
    }

    #[test]
    fn a_blob_that_is_not_a_content_state_is_refused_with_the_reason() {
        assert!(content_input("not json", None)
            .unwrap_err()
            .contains("not JSON"));
        assert!(content_input(r#"{"kind":"tool_result"}"#, None)
            .unwrap_err()
            .contains("no text"));
        assert!(content_input(r#"{"text":"x"}"#, None)
            .unwrap_err()
            .contains("no kind"));
    }
}
