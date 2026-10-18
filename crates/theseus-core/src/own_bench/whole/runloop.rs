//! The whole-turn loop (own-benchmark part (c)): the replay profile's model
//! called again and again, its tool calls answered by `hands.rs`, until it
//! answers without one or a cap ends it. Reads the original turn from the
//! session's nodes; writes nothing but the `turn.replayed` row the caller
//! writes.
//!
//! Caps, each a ceiling on one replay (`[route_bench] whole_loops`, `whole_seconds`,
//! `whole_usd`): the loop that would pass one is not made, and the row says
//! which stopped it. Every call is reserved before it is sent (the caller's
//! own cap and the day ceiling too, as a first-loop replay's call is), so the
//! cap is checked before the spend, never after.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde_json::{json, Value};
use theseus_protocol::own_bench::{TestRun, TurnReplayParams, WholeReplay};

use super::compare::{self, Facts};
use super::hands::{Hands, InL1, Programs, Recorded};
use super::scratch;
use super::undo::{self, Recorded as Call};
use crate::node::Body;
use crate::own_bench::rebuild::{Original, Said};
use crate::own_bench::replay::{said_of_response, CapReached};
use crate::rpc::Core;

/// What a whole-turn replay gave back.
pub struct Done {
    pub replay: Said,
    pub micros: u64,
    pub whole: WholeReplay,
    /// The original turn's final reply, and what the whole turn cost.
    pub original: Said,
}

/// The original turn as the session recorded it.
struct Turn {
    original: Said,
    facts: Facts,
    recorded: Vec<Recorded>,
    /// Every call from the turn's start to the end of the session.
    span: Vec<(String, Value, bool, Value)>,
}

#[cfg(test)]
pub(crate) static TEST_PROGRAMS: std::sync::Mutex<Option<Arc<dyn Programs>>> =
    std::sync::Mutex::new(None);

fn programs() -> Arc<dyn Programs> {
    #[cfg(test)]
    if let Some(p) = TEST_PROGRAMS.lock().unwrap().clone() {
        return p;
    }
    Arc::new(InL1)
}

/// A `proc.run` call's words: its argv, or its line run by bash, or its last
/// step's.
fn argv_of(input: &Value) -> Vec<String> {
    let strs = |v: &Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    if let Some(a) = input.get("argv") {
        return strs(a);
    }
    if let Some(c) = input.get("command").and_then(Value::as_str) {
        return vec!["bash".into(), "-c".into(), c.into()];
    }
    input
        .get("steps")
        .and_then(Value::as_array)
        .and_then(|s| s.last())
        .map(|s| s.get("argv").map_or_else(Vec::new, strs))
        .unwrap_or_default()
}

/// The row's account of a finished replay, with the deterministic comparison.
fn whole_row(
    (loops, capped): (u32, Option<String>),
    hands: &Hands<'_>,
    (original, scratch): (&Facts, &scratch::Scratch),
    caveats: Vec<String>,
) -> WholeReplay {
    let replay = Facts {
        changed: hands.changed.clone(),
        runs: hands
            .ran
            .iter()
            .map(|r| TestRun {
                argv: r.argv.clone(),
                exit: r.exit,
            })
            .collect(),
    };
    WholeReplay {
        loops,
        capped,
        calls: hands.calls.clone(),
        changed: replay.changed.iter().cloned().collect(),
        original_changed: original.changed.iter().cloned().collect(),
        tests: replay.tests(),
        original_tests: original.tests(),
        same: compare::same(original, &replay),
        caveats,
        scratch_files: scratch.files,
        scratch_bytes: scratch.bytes,
        label: None,
    }
}

/// Each call's result by its id: whether it succeeded, its meta and its text.
fn results_by_id(
    transcript: &crate::store::Transcript,
) -> std::collections::HashMap<String, (bool, Value, String)> {
    let mut results = std::collections::HashMap::new();
    for (_, n) in transcript {
        if let Body::ToolResult {
            tool_use_id,
            is_error,
            meta,
            content,
            status,
            ..
        } = &n.body
        {
            let ok = !is_error && *status == crate::node::ResultStatus::Ok;
            results.insert(tool_use_id.clone(), (ok, meta.clone(), content.clone()));
        }
    }
    results
}

impl Core {
    /// The original turn: its final reply, what it changed and ran, its
    /// recorded results, and the calls from its start on.
    fn read_turn(&self, session: &str, turn: &str, first: &Said) -> anyhow::Result<Turn> {
        let transcript = self.store.transcript(session)?;
        let results = results_by_id(&transcript);
        let start = transcript
            .iter()
            .position(|(_, n)| n.turn_id.as_deref() == Some(turn))
            .context("the turn has no nodes")?;
        let mut out = Turn {
            original: Said {
                model: first.model.clone(),
                ..Said::default()
            },
            facts: Facts::default(),
            recorded: Vec::new(),
            span: Vec::new(),
        };
        let orig_cwd = self
            .store
            .get_session::<crate::session::SessionRecord>(session)?
            .and_then(|r| r.dir)
            .map(|d| self.runner.tools.cwd_for(Some(&d)))
            .unwrap_or_else(|| self.runner.tools.cwd_for(None));
        let mut turn_calls: Vec<(String, Value, bool, Value)> = Vec::new();
        for (_, n) in &transcript[start..] {
            let in_turn = n.turn_id.as_deref() == Some(turn);
            match &n.body {
                Body::ToolCall {
                    tool_use_id,
                    tool,
                    input,
                    ..
                } => {
                    let (ok, meta, content) = results.get(tool_use_id).cloned().unwrap_or((
                        false,
                        Value::Null,
                        String::new(),
                    ));
                    out.span
                        .push((tool.clone(), input.clone(), ok, meta.clone()));
                    if in_turn {
                        turn_calls.push((tool.clone(), input.clone(), ok, meta.clone()));
                        out.recorded.push(Recorded {
                            tool: tool.clone(),
                            content,
                        });
                        if tool == "proc.run" {
                            out.facts.runs.push(TestRun {
                                argv: argv_of(input),
                                exit: meta
                                    .get("exit_code")
                                    .and_then(Value::as_i64)
                                    .map(|c| c as i32),
                            });
                        }
                    }
                }
                Body::AssistantMessage { .. } if in_turn => {
                    if let Some(s) = crate::own_bench::rebuild::said_of(n) {
                        out.original.text = s.text.clone();
                        out.original.tool_calls.extend(s.tool_calls);
                        out.original.input_tokens += s.input_tokens;
                        out.original.output_tokens += s.output_tokens;
                        out.original.cost_usd += s.cost_usd;
                        out.original.model = s.model;
                    }
                }
                _ => {}
            }
        }
        // What the original turn changed: its own calls' paths, as the
        // directory's.
        let span = undo::collect(
            turn_calls.iter().map(|(tool, input, ok, meta)| Call {
                tool,
                input,
                ok: *ok,
                meta,
            }),
            |_| false,
        );
        if let Ok(span) = span {
            for w in span.written {
                let p = w.path();
                out.facts
                    .changed
                    .insert(p.strip_prefix(&orig_cwd).unwrap_or(p).display().to_string());
            }
        }
        Ok(out)
    }

    /// The whole turn on `target`: see the module's docs. `left` is what the
    /// caller's own cap still allows; `spent` is set to what the replay cost.
    pub(in crate::own_bench) async fn replay_whole(
        self: &Arc<Self>,
        p: &TurnReplayParams,
        original: &Original,
        (provider, request): (
            &dyn crate::provider::Provider,
            &crate::provider::ProviderRequest,
        ),
        target: &crate::turn::Target,
        left: Option<u64>,
        spent: &mut u64,
    ) -> anyhow::Result<Done> {
        let route = &self.cfg.route_bench;
        let t0 = Instant::now();
        let rt = self.runner.tools.clone();
        let orig_cwd = rt.cwd_for(original.session.dir.as_deref());
        let read = self.read_turn_blocking(p, original).await?;
        let (scratch, caveats) = self.copy_as_it_was(p, original, &read).await?;

        // The loop.
        let mut hands = Hands::new(
            &rt,
            (orig_cwd.clone(), scratch.dir.clone()),
            read.recorded.clone(),
            programs(),
        );
        let deadline = t0 + Duration::from_secs(route.whole_seconds);
        let usd_cap = theseus_kernel::usd_to_micros(route.whole_usd);
        let mut request = request.clone();
        let mut total = 0u64;
        let mut replay = Said::default();
        let mut capped: Option<String> = None;
        let mut loops = 0u32;
        let mut final_text = String::new();
        loop {
            if loops >= route.whole_loops {
                capped = Some("loops".into());
                break;
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                capped = Some("seconds".into());
                break;
            };
            hands.seconds_left = remaining.as_secs().max(1);
            let room = usd_cap.saturating_sub(total);
            let allowed = left.map_or(room, |l| l.min(room));
            let mut one = 0u64;
            let called = tokio::time::timeout(
                remaining,
                self.call_model(provider, &request, target, Some(allowed), &mut one),
            )
            .await;
            let (resp, cost) = match called {
                Err(_) => {
                    capped = Some("seconds".into());
                    break;
                }
                Ok(Err(e)) if e.downcast_ref::<CapReached>().is_some() && loops > 0 => {
                    capped = Some("usd".into());
                    break;
                }
                Ok(Err(e)) => {
                    *spent = total + one;
                    return Err(e);
                }
                Ok(Ok(r)) => r,
            };
            loops += 1;
            total += cost;
            let said = said_of_response(&resp, cost);
            replay.model = said.model.clone();
            replay.input_tokens += said.input_tokens;
            replay.output_tokens += said.output_tokens;
            replay.tool_calls.extend(said.tool_calls.clone());
            final_text = said.text;
            let uses: Vec<&Value> = resp
                .content
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
                .collect();
            if uses.is_empty() || resp.stop_reason.as_deref() != Some("tool_use") {
                break;
            }
            let mut results = Vec::new();
            for u in uses {
                let id = u.get("id").and_then(Value::as_str).unwrap_or_default();
                let name = u.get("name").and_then(Value::as_str).unwrap_or_default();
                let input = u.get("input").cloned().unwrap_or(Value::Null);
                let a = hands.call(name, &input).await;
                results.push(json!({"type": "tool_result", "tool_use_id": id,
                    "content": a.text, "is_error": a.is_error}));
            }
            request
                .messages
                .push(json!({"role": "assistant", "content": resp.content}));
            request
                .messages
                .push(json!({"role": "user", "content": results}));
        }
        *spent = total;
        replay.text = final_text;
        replay.cost_usd = theseus_kernel::micros_to_usd(total);

        let whole = whole_row((loops, capped), &hands, (&read.facts, &scratch), caveats);
        drop(hands);
        drop(scratch);
        Ok(Done {
            replay,
            micros: total,
            whole,
            original: read.original,
        })
    }
}

impl Core {
    /// The original turn, read on the blocking pool.
    async fn read_turn_blocking(
        self: &Arc<Self>,
        p: &TurnReplayParams,
        original: &Original,
    ) -> anyhow::Result<Turn> {
        let me = self.clone();
        let (session, turn, first) = (p.session.clone(), p.turn.clone(), original.said.clone());
        tokio::task::spawn_blocking(move || me.read_turn(&session, &turn, &first))
            .await
            .context("the turn was not read")?
    }

    /// The directory as it was at the turn's start, and what the copy could
    /// not be.
    async fn copy_as_it_was(
        self: &Arc<Self>,
        p: &TurnReplayParams,
        original: &Original,
        read: &Turn,
    ) -> anyhow::Result<(scratch::Scratch, Vec<String>)> {
        let rt = self.runner.tools.clone();
        let reg = rt.clone();
        let span = undo::collect(
            read.span.iter().map(|(tool, input, ok, meta)| Call {
                tool,
                input,
                ok: *ok,
                meta,
            }),
            |t| {
                reg.registry
                    .get(t)
                    .is_some_and(|t| t.class() == theseus_tools::ToolClass::Write)
            },
        )?;
        let cwd = rt.cwd_for(original.session.dir.as_deref());
        let max = self.cfg.route_bench.whole_tree_mb * 1024 * 1024;
        let tag = p.turn.clone();
        let skip: Vec<std::path::PathBuf> = rt
            .policy
            .approve_paths
            .iter()
            .chain(&rt.policy.floor_paths)
            .cloned()
            .collect();
        let copied = tokio::task::spawn_blocking(move || {
            let s = scratch::copy(&cwd, &std::env::temp_dir(), &tag, (max, &skip))?;
            let notes = undo::undo(&cwd, &s.dir, &span.written)?;
            Ok::<_, crate::own_bench::rebuild::NotRebuildable>((s, notes, span.programs))
        })
        .await
        .context("the copy did not finish")??;
        let (scratch, notes, programs_unrecorded) = copied;
        let mut caveats = scratch.caveats.clone();
        caveats.extend(notes);
        if programs_unrecorded > 0 {
            caveats.push(format!(
                "{programs_unrecorded} programs ran from this turn on whose file changes are not \
                 recorded, so the copy may differ from the directory as it was"
            ));
        }
        Ok((scratch, caveats))
    }
}
