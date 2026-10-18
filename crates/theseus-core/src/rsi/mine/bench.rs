//! The bench's trials (§2.1): Harbor's `agent/trajectory.json` (ATIF-v1.7,
//! written by `bench/harbor/theseus_atif.py`) and the `theseus-history.json`
//! it is made from, read into the same runs the ledger gives, in the scope
//! `bench:<suite>`, never mixed with a repository's.
//!
//! A trial is a run; its task is the part of the trial directory's name before
//! `__`. An agent step is a model loop: its `metrics` are the loop's cost
//! (`prompt_tokens` is input plus cache reads plus cache writes, as Harbor
//! counts it; the parts come back out through `cached_tokens` and
//! `extra.cache_creation_input_tokens`), and the results of its calls are its
//! `observation`. The bench has no model time per loop and no cwd: its
//! shapes are the same, its paths are absolute and so no constant.
//!
//! A bench-born motif must be generic (`motif.rs`): its constants only
//! program, subcommand and flag tokens, with no task path or string.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::read::{date_of, Env};
use super::shape::{normalize, result_class, Where};
use super::{Loop, Run, Vocab};

/// Trajectories read in one pass, at most.
pub const MAX_FILES: usize = 2000;
/// How deep a bench directory is searched for trials.
const DEPTH: usize = 5;

/// A canonical tool name from the wire name a trajectory holds: `proc_run`
/// is `proc.run`; a name with a dot already is canonical.
pub fn canonical(name: &str) -> String {
    if name.contains('.') {
        return name.to_string();
    }
    match name.split_once('_') {
        Some((a, b)) => format!("{a}.{}", b.replace('_', ".")),
        None => name.to_string(),
    }
}

/// One agent step, as either file gives it.
struct Step {
    calls: Vec<(String, String, Value)>,
    /// Uncached input, cache reads, cache writes, output; dollars.
    tokens: [u64; 4],
    usd: f64,
    results: Vec<(String, String, String, bool, u64)>,
}

fn u(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

fn atif(t: &Value) -> Vec<Step> {
    let mut out = Vec::new();
    for s in t["steps"].as_array().into_iter().flatten() {
        if s["source"] != "agent" {
            continue;
        }
        let calls = s["tool_calls"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                (
                    c["tool_call_id"].as_str().unwrap_or("").to_string(),
                    canonical(c["function_name"].as_str().unwrap_or("")),
                    c["arguments"].clone(),
                )
            })
            .collect();
        let m = &s["metrics"];
        let (read, written) = (
            u(&m["cached_tokens"]),
            u(&m["extra"]["cache_creation_input_tokens"]),
        );
        let results = s["observation"]["results"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| {
                let x = &r["extra"];
                (
                    r["source_call_id"].as_str().unwrap_or("").to_string(),
                    r["content"].as_str().unwrap_or("").to_string(),
                    x["status"].as_str().unwrap_or("ok").to_string(),
                    x["is_error"].as_bool().unwrap_or(false),
                    u(&x["duration_ms"]),
                )
            })
            .collect();
        out.push(Step {
            calls,
            tokens: [
                u(&m["prompt_tokens"]).saturating_sub(read + written),
                read,
                written,
                u(&m["completion_tokens"]),
            ],
            usd: m["cost_usd"].as_f64().unwrap_or(0.0),
            results,
        });
    }
    out
}

/// `theseus --json history`'s nodes, as steps: an answer's calls, usage and
/// cost, then the results that follow.
fn history(h: &Value) -> Vec<Step> {
    let mut out: Vec<Step> = Vec::new();
    for n in h["nodes"].as_array().into_iter().flatten() {
        let d = &n["detail"];
        match n["kind"].as_str() {
            Some("assistant_message") => {
                let calls = d["tool_calls"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| {
                        (
                            c["id"].as_str().unwrap_or("").to_string(),
                            canonical(c["name"].as_str().unwrap_or("")),
                            c["input"].clone(),
                        )
                    })
                    .collect();
                let us = &d["usage"];
                out.push(Step {
                    calls,
                    tokens: [
                        u(&us["input_tokens"]),
                        u(&us["cache_read_input_tokens"]),
                        u(&us["cache_creation_input_tokens"]),
                        u(&us["output_tokens"]),
                    ],
                    usd: d["cost_usd"].as_f64().unwrap_or(0.0),
                    results: vec![],
                });
            }
            Some("tool_result") => {
                if let Some(s) = out.last_mut() {
                    s.results.push((
                        d["tool_use_id"].as_str().unwrap_or("").to_string(),
                        n["text"].as_str().unwrap_or("").to_string(),
                        d["status"].as_str().unwrap_or("ok").to_string(),
                        d["is_error"].as_bool().unwrap_or(false),
                        u(&d["duration_ms"]),
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

/// A trial's run from a trajectory or a history (`file` as read), in
/// `suite`; none when it holds no call.
pub fn run_of(
    file: &Value,
    suite: &str,
    trial: &str,
    task: &str,
    when_ms: u64,
    env: &Env<'_>,
    vocab: &mut Vocab,
) -> Option<Run> {
    let steps = if file["schema_version"].is_string() {
        atif(file)
    } else {
        history(file)
    };
    let w = Where::default();
    let mut constants = 0usize;
    let mut run = Run {
        session: trial.to_string(),
        turn: String::new(),
        task: task.to_string(),
        day: date_of(when_ms),
        scope: format!("bench:{suite}"),
        place: "bench".into(),
        calls: vec![],
        loops: vec![],
        last_ms: when_ms,
    };
    for s in steps {
        if s.calls.is_empty() {
            continue;
        }
        let loop_ix = run.loops.len() as u32;
        let mut emitted = 0u32;
        for (id, tool, input) in &s.calls {
            let result = s.results.iter().find(|r| &r.0 == id);
            let class = match result {
                Some((_, text, status, is_error, _)) => result_class(status, *is_error, None, text),
                None => "unknown".into(),
            };
            let ms = result.map_or(0, |r| r.4.min(u64::from(u32::MAX)) as u32);
            for n in normalize(tool, input, &w) {
                let mut c = super::read::call_of(env, vocab, &n, &mut constants);
                c.res = vocab.add_result(&class);
                c.loop_ix = loop_ix;
                c.ms = ms;
                run.calls.push(c);
                emitted += 1;
            }
        }
        run.loops.push(Loop {
            tokens: s.tokens,
            usd_micros: (s.usd.max(0.0) * 1e6).round() as u64,
            model_ms: 0,
            calls: emitted,
        });
    }
    (!run.calls.is_empty()).then_some(run)
}

/// The trials under a bench directory: every `agent/trajectory.json` (or the
/// `theseus-history.json` beside it) within [`DEPTH`], the suite the
/// directory's name, at most [`MAX_FILES`], newest first, those newer than
/// `since_ms`.
pub fn read_dir(dir: &Path, since_ms: u64, env: &Env<'_>, vocab: &mut Vocab) -> Vec<Run> {
    let suite = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "bench".into());
    let mut files: Vec<(u64, PathBuf)> = Vec::new();
    walk(dir, 0, &mut files);
    files.sort_by(|a, b| b.cmp(a));
    files.truncate(MAX_FILES);
    let mut runs = Vec::new();
    for (ms, f) in files {
        if ms < since_ms {
            continue;
        }
        // A trial's directory holds `agent/trajectory.json`; its history is
        // beside the `agent` directory.
        let trial_dir = if f.file_name().is_some_and(|n| n == "trajectory.json") {
            f.parent().and_then(Path::parent)
        } else {
            f.parent()
        };
        let trial = trial_dir
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let task = trial.split("__").next().unwrap_or(&trial).to_string();
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if let Some(r) = run_of(&v, &suite, &trial, &task, ms, env, vocab) {
            runs.push(r);
        }
    }
    runs
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<(u64, PathBuf)>) {
    if depth > DEPTH || out.len() > MAX_FILES * 4 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<_> = rd.flatten().collect();
    names.sort_by_key(std::fs::DirEntry::file_name);
    for e in names {
        let p = e.path();
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            walk(&p, depth + 1, out);
        } else if p
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|n| n == "agent")
            && matches!(
                p.file_name().and_then(|n| n.to_str()),
                Some("trajectory.json")
            )
        {
            let ms = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            out.push((ms, p));
        } else if p.file_name().is_some_and(|n| n == "theseus-history.json")
            && !p.with_file_name("agent").join("trajectory.json").exists()
        {
            let ms = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            out.push((ms, p));
        }
    }
}

#[cfg(test)]
mod tests;
