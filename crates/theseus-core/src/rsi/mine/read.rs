//! The incremental read (§2.1): the transcript's tool calls and their results,
//! the assistant nodes' cost and `provider.call`'s timing, folded into runs
//! of shape tokens, and the META records that keep them.
//!
//! - **Cursor.** `self.mine.cursor` holds the WAL position the last night read
//!   to, the digest key, and when it ran. A night reads the nodes after it, up
//!   to ten minutes ago (so a call's result has landed), at most
//!   [`MAX_CALLS`] calls (the row cap, as backlog's `MAX_ROWS`): what is left
//!   waits for the next night.
//! - **Days.** `self.mine.shapes.<date>` holds that day's runs: token ids and
//!   keyed digests, positions as evidence, never a value. The window is the
//!   union of the last `window_days` days. The day that left the window is
//!   written empty (the record only grows; it need not keep growing).
//! - **Vocabulary.** `self.mine.vocab` names the ids the window uses.
//!
//! Raw values live here for the length of one fold, in locals. What leaves is
//! the shape's text, a digest per slot, and a constant's text only when it is
//! safe (short, clean to the scrubber, and passing the shape check).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, NewRecord, Page};

use super::shape::{self, never_mined, normalize, type_of, SlotType, Where};
use super::{digest_of, Call, Loop, Run, Vocab, AUTHORED, NEVER};
use crate::node::{Body, Node};
use crate::store::Store;

/// Calls folded in one night, at most.
pub const MAX_CALLS: usize = 20_000;
/// Nodes younger than this are left for the next night: a call's result lands
/// within it.
pub const SETTLE_MS: u64 = 10 * 60_000;
/// A constant is at most this long.
const CONSTANT_CHARS: usize = 64;
/// A result names at most this many references and paths.
const OUTS: usize = 8;
/// The part of a result that is read for what it names.
const OUT_CHARS: usize = 4096;
/// Constants kept in the vocabulary, at most.
const MAX_CONSTANTS: usize = 5000;

pub const CURSOR_KEY: &str = "self.mine.cursor";
pub const VOCAB_KEY: &str = "self.mine.vocab";

/// The key of a day's record.
pub fn day_key(date: &str) -> String {
    format!("self.mine.shapes.{date}")
}

/// Where the last night read to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
    /// The last WAL position folded.
    pub pos: u64,
    /// When the last night ran (unix ms).
    pub ran_ms: u64,
    /// The key of the value digests (hex): a digest means nothing without it,
    /// and values are compared only under one key.
    pub key: String,
}

/// A day's runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Day {
    pub date: String,
    pub runs: Vec<Run>,
}

/// The local date of a time, `2026-10-17`.
pub fn date_of(ms: u64) -> String {
    let l = crate::wake::local(ms);
    format!("{:04}-{:02}-{:02}", l.year, l.month, l.day)
}

/// What the miner is told of a session.
#[derive(Debug, Clone, Default)]
pub struct SessionInfo {
    /// Its directory, as its client sent it.
    pub dir: Option<String>,
    /// `cli`, `dm`, `channel` or `task`.
    pub place: String,
}

/// What a fold needs of the world around the store.
pub struct Env<'a> {
    pub key: Vec<u8>,
    /// Whether the scrubber would change nothing in a value.
    pub clean: &'a dyn Fn(&str) -> bool,
    /// A session's directory and place; none for a session the miner leaves
    /// alone (a shared place's, an imported one, a self step's).
    pub session: &'a dyn Fn(&str) -> Option<SessionInfo>,
}

/// What one fold read.
#[derive(Debug, Default)]
pub struct Folded {
    pub runs: Vec<Run>,
    pub vocab: Vocab,
    /// The last position read.
    pub cursor: u64,
    pub calls: usize,
}

fn micros(usd: f64) -> u64 {
    (usd.max(0.0) * 1e6).round() as u64
}

/// The git top level of `dir`, and a key for it: a hash of its canonical path
/// and its origin URL's path. A directory in no repository is its own scope.
pub fn scope_of(dir: &str) -> (String, Option<String>) {
    let canon = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.into());
    let mut at: &Path = &canon;
    loop {
        let git = at.join(".git");
        if git.exists() {
            let origin = origin_path(&git).unwrap_or_default();
            let id = digest_of(b"scope", &format!("{}\0{origin}", at.display()));
            return (format!("r:{id:016x}"), Some(at.display().to_string()));
        }
        match at.parent() {
            Some(p) => at = p,
            None => break,
        }
    }
    let id = digest_of(b"scope", &canon.display().to_string());
    (format!("d:{id:016x}"), None)
}

/// The path of the origin remote's URL, from the repository's config (a
/// worktree's `.git` file points at the common directory).
fn origin_path(git: &Path) -> Option<String> {
    let dir = if git.is_file() {
        let t = std::fs::read_to_string(git).ok()?;
        let gd = std::path::PathBuf::from(t.strip_prefix("gitdir:")?.trim());
        let common = std::fs::read_to_string(gd.join("commondir")).ok()?;
        gd.join(common.trim())
    } else {
        git.to_path_buf()
    };
    let cfg = std::fs::read_to_string(dir.join("config")).ok()?;
    let mut in_origin = false;
    for line in cfg.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_origin = l == "[remote \"origin\"]";
        } else if in_origin {
            if let Some(u) = l.strip_prefix("url") {
                let u = u.trim_start().strip_prefix('=')?.trim();
                // Only the path: the host and any credentials stay out.
                let path = match u.split_once("://") {
                    Some((_, r)) => r.split_once('/').map_or("", |x| x.1),
                    None => u.split_once(':').map_or(u, |x| x.1),
                };
                return Some(path.trim_end_matches(".git").to_string());
            }
        }
    }
    None
}

/// The prefixes of the secrets the scrubber knows by shape, repeated here so
/// that the shape check stands on its own.
const SECRET_PREFIXES: &[&str] = &[
    "sk-",
    "sk_",
    "ghp_",
    "gho_",
    "ghs_",
    "github_pat_",
    "xox",
    "AKIA",
    "ASIA",
    "AIza",
    "eyJ",
    "-----BEGIN",
    "glpat-",
    "npm_",
    "rk_live",
    "pk_live",
];

/// Whether `v` has the shape of a secret: a known prefix, or a long run of
/// token characters mixing letters and digits that is not a commit id.
pub fn secret_shaped(v: &str) -> bool {
    if SECRET_PREFIXES.iter().any(|p| v.contains(p)) {
        return true;
    }
    let git_id = (7..=40).contains(&v.len()) && v.chars().all(|c| c.is_ascii_hexdigit());
    if git_id {
        return false;
    }
    v.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '/' | '=')))
        .any(|run| {
            run.len() >= 24
                && run.chars().any(|c| c.is_ascii_digit())
                && run.chars().any(|c| c.is_ascii_alphabetic())
                // A path or a hyphenated name has separators between short words.
                && !run.contains('/')
                && run.split(['-', '_']).all(|w| w.len() >= 8 || w.is_empty())
        })
}

/// Whether `v` looks like a person: an address, a handle, or two capitalized
/// words.
pub fn person_like(v: &str) -> bool {
    if v.contains('@') && !v.starts_with("git@") {
        return true;
    }
    let words: Vec<&str> = v.split_whitespace().collect();
    words.windows(2).any(|w| {
        w.iter().all(|x| {
            x.len() >= 2
                && x.chars().next().is_some_and(char::is_uppercase)
                && x.chars().skip(1).all(char::is_lowercase)
        })
    })
}

/// The shape check on a constant (§2.5, hygiene): short, no absolute path
/// outside the repository, no person-like string, no secret shape.
pub fn constant_ok(v: &str, class: &str) -> bool {
    !v.is_empty()
        && v.chars().count() <= CONSTANT_CHARS
        && !v.contains('\n')
        && class != "$ABS"
        && !person_like(v)
        && !secret_shaped(v)
}

/// A value's digest, or 0 when the miner would keep nothing of it.
fn digest(env: &Env<'_>, v: &str) -> u64 {
    if secret_shaped(v) || !(env.clean)(v) {
        0
    } else {
        digest_of(&env.key, v)
    }
}

/// The references and paths a result named, as digests.
fn outs_of(env: &Env<'_>, text: &str, w: &Where) -> Vec<u64> {
    let head: String = text.chars().take(OUT_CHARS).collect();
    let mut out: Vec<u64> = Vec::new();
    let mut take = |word: &str| {
        let word =
            word.trim_matches(|c: char| matches!(c, '\'' | '"' | ',' | ':' | '(' | ')' | ';'));
        if word.len() < 2 || word.len() > 100 {
            return;
        }
        let d = digest(env, word);
        if d != 0 && !out.contains(&d) && out.len() < OUTS {
            out.push(d);
        }
    };
    let trimmed = head.trim();
    if !trimmed.is_empty() && !trimmed.contains(char::is_whitespace) {
        take(trimmed);
    }
    for word in head.split_whitespace() {
        let t = type_of(
            word.trim_matches(|c: char| matches!(c, '\'' | '"' | ',' | ':' | '(' | ')')),
            w,
        );
        if t.ty == SlotType::Path {
            take(word);
        }
    }
    out
}

/// A normalized call as the miner stores it: its shape and result unnamed
/// (`unknown`), a digest for each slot, and a constant's text in `vocab` only
/// when it is safe. The caller sets the position, the loop, the result and
/// the time. `constants` counts the constants kept, against a cap.
pub fn call_of(env: &Env<'_>, vocab: &mut Vocab, n: &shape::Norm, constants: &mut usize) -> Call {
    let mut slots = Vec::with_capacity(n.values.len());
    for (v, s) in n.values.iter().zip(&n.shape.slots) {
        let d = digest(env, v.as_str());
        if d != 0 && *constants < MAX_CONSTANTS && constant_ok(v.as_str(), &s.class) {
            vocab.add_constant(d, v.as_str());
            *constants += 1;
        }
        slots.push(d);
    }
    Call {
        shape: vocab.add_shape(&n.shape),
        res: vocab.add_result("unknown"),
        slots,
        outs: vec![],
        pos: 0,
        loop_ix: 0,
        ms: 0,
        flags: if n.shape.authored { AUTHORED } else { 0 } | if never_mined(n) { NEVER } else { 0 },
    }
}

/// A run, while a fold builds it.
struct Building {
    run: Run,
    /// Assistant node id -> loop index.
    loops: HashMap<String, u32>,
}

/// What a fold knows of a call it has not seen the result of.
struct Pending {
    root: Option<String>,
    run: usize,
    first: usize,
    count: usize,
}

/// The model's time for each call of a provider, by the assistant node,
/// from `provider.call` rows after `from`.
fn timings(
    store: &Store,
    from: u64,
    since_ms: Option<u64>,
    until_ms: u64,
) -> Result<HashMap<String, u64>> {
    let mut timing = HashMap::new();
    let mut after = from;
    loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![ledger_kind("provider.call")],
            after: Some(after),
            since_ms,
            until_ms: Some(until_ms),
            limit: 256,
            ..Page::default()
        };
        let Some(out) = store.ledger_page(&page)? else {
            bail!("the ledger's index is still being built");
        };
        for r in &out.records {
            if let Ok(row) = r.decode::<crate::ledger::LedgerRow>() {
                if let (Some(n), Some(ms)) = (
                    row.data["node_id"].as_str(),
                    row.data["timing"]["total_ms"].as_u64(),
                ) {
                    timing.insert(n.to_string(), ms);
                }
            }
        }
        match (out.more, out.last) {
            (true, Some(l)) => after = l,
            _ => return Ok(timing),
        }
    }
}

/// A fold in progress: everything it has seen of the pages so far.
struct Folding<'a> {
    env: &'a Env<'a>,
    timing: HashMap<String, u64>,
    vocab: Vocab,
    runs: Vec<Building>,
    by_key: HashMap<(String, String, String, String), usize>,
    pending: HashMap<(String, String), Pending>,
    assistant: HashMap<String, ([u64; 4], u64)>,
    sessions: HashMap<String, Option<SessionInfo>>,
    scopes: HashMap<String, (String, Option<String>)>,
    calls: usize,
    constants: usize,
}

impl Folding<'_> {
    /// One node: an answer's cost, a call, or a call's result.
    fn node(&mut self, position: u64, node: &Node) {
        let env = self.env;
        let info = self
            .sessions
            .entry(node.session_id.clone())
            .or_insert_with(|| (env.session)(&node.session_id))
            .clone();
        let Some(info) = info else { return };
        match &node.body {
            Body::AssistantMessage {
                usage, cost_usd, ..
            } => {
                let tokens = [
                    usage.input_tokens,
                    usage.cache_read_input_tokens,
                    usage.cache_creation_input_tokens,
                    usage.output_tokens,
                ];
                self.assistant
                    .insert(node.id.clone(), (tokens, micros(cost_usd.unwrap_or(0.0))));
            }
            Body::ToolCall {
                tool_use_id,
                tool,
                input,
                assistant_node,
                gate,
                ..
            } => {
                let cwd = gate
                    .as_ref()
                    .and_then(|g| g.proposal.policy_context.get("cwd"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| info.dir.clone());
                self.call(
                    position,
                    node,
                    &info,
                    (tool_use_id, tool, input, assistant_node),
                    cwd,
                );
            }
            Body::ToolResult {
                tool_use_id,
                status,
                is_error,
                content,
                duration_ms,
                meta,
                ..
            } => {
                let Some(p) = self
                    .pending
                    .remove(&(node.session_id.clone(), tool_use_id.clone()))
                else {
                    return;
                };
                let status = serde_json::to_value(status)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default();
                let exit = meta.get("exit_code").and_then(Value::as_i64);
                let class = shape::result_class(&status, *is_error, exit, content);
                let res = self.vocab.add_result(&class);
                let outs = outs_of(
                    self.env,
                    content,
                    &Where {
                        root: p.root.clone(),
                    },
                );
                let ms = duration_ms.unwrap_or(0).min(u64::from(u32::MAX)) as u32;
                for c in &mut self.runs[p.run].run.calls[p.first..p.first + p.count] {
                    c.res = res;
                    c.ms = ms;
                    c.outs = outs.clone();
                }
            }
            _ => {}
        }
    }

    /// A tool call: its run, its loop, and a call for each shape it has.
    fn call(
        &mut self,
        position: u64,
        node: &Node,
        info: &SessionInfo,
        (tool_use_id, tool, input, assistant_node): (&String, &String, &Value, &String),
        cwd: Option<String>,
    ) {
        let (scope, root) = match &cwd {
            Some(d) => self
                .scopes
                .entry(d.clone())
                .or_insert_with(|| scope_of(d))
                .clone(),
            None => ("d:none".to_string(), None),
        };
        let w = Where { root: root.clone() };
        let day = date_of(node.created_at_ms);
        let turn = node.turn_id.clone().unwrap_or_default();
        let key = (
            node.session_id.clone(),
            turn.clone(),
            day.clone(),
            scope.clone(),
        );
        let runs = &mut self.runs;
        let ix = *self.by_key.entry(key).or_insert_with(|| {
            runs.push(Building {
                run: Run {
                    session: node.session_id.clone(),
                    turn,
                    task: String::new(),
                    day,
                    scope,
                    place: info.place.clone(),
                    calls: vec![],
                    loops: vec![],
                    last_ms: 0,
                },
                loops: HashMap::new(),
            });
            runs.len() - 1
        });
        let b = &mut self.runs[ix];
        let (assistant, timing) = (&self.assistant, &self.timing);
        let loop_ix = *b.loops.entry(assistant_node.clone()).or_insert_with(|| {
            let (tokens, usd_micros) = assistant.get(assistant_node).copied().unwrap_or_default();
            b.run.loops.push(Loop {
                tokens,
                usd_micros,
                model_ms: timing.get(assistant_node).copied().unwrap_or(0),
                calls: 0,
            });
            (b.run.loops.len() - 1) as u32
        });
        let first = b.run.calls.len();
        for n in normalize(tool, input, &w) {
            let mut c = call_of(self.env, &mut self.vocab, &n, &mut self.constants);
            c.pos = position;
            c.loop_ix = loop_ix;
            b.run.calls.push(c);
            b.run.loops[loop_ix as usize].calls += 1;
            self.calls += 1;
        }
        b.run.last_ms = b.run.last_ms.max(node.created_at_ms);
        let count = b.run.calls.len() - first;
        self.pending.insert(
            (node.session_id.clone(), tool_use_id.clone()),
            Pending {
                root,
                run: ix,
                first,
                count,
            },
        );
    }
}

/// Read the nodes after `from` (or, from the start, those written since
/// `since_ms`) and before `until_ms`, at most `cap` calls.
pub fn fold(
    store: &Store,
    env: &Env<'_>,
    from: u64,
    since_ms: Option<u64>,
    until_ms: u64,
    cap: usize,
) -> Result<Folded> {
    let mut f = Folding {
        env,
        timing: timings(store, from, since_ms, until_ms)?,
        vocab: Vocab::default(),
        runs: Vec::new(),
        by_key: HashMap::new(),
        pending: HashMap::new(),
        assistant: HashMap::new(),
        sessions: HashMap::new(),
        scopes: HashMap::new(),
        calls: 0,
        constants: 0,
    };
    let mut cursor = from;
    let mut after = from;
    'pages: loop {
        let page = Page {
            kind: kinds::NODE,
            tags: ["assistant_message", "tool_call", "tool_result"]
                .map(ledger_kind)
                .to_vec(),
            after: Some(after),
            since_ms,
            until_ms: Some(until_ms),
            limit: 256,
            ..Page::default()
        };
        let Some(out) = theseus_store::Store::page(&**store.inner(), &page)? else {
            bail!("the store's index is still being built");
        };
        for r in &out.records {
            if f.calls >= cap {
                break 'pages;
            }
            cursor = r.position;
            if let Ok(node) = r.decode::<Node>() {
                f.node(r.position, &node);
            }
        }
        match (out.more, out.last) {
            (true, Some(l)) => {
                after = l;
                cursor = cursor.max(l);
            }
            _ => {
                cursor = cursor.max(out.last.unwrap_or(cursor));
                break;
            }
        }
    }
    Ok(Folded {
        runs: f
            .runs
            .into_iter()
            .map(|b| b.run)
            .filter(|r| !r.calls.is_empty())
            .collect(),
        vocab: f.vocab,
        cursor,
        calls: f.calls,
    })
}

/// What the records hold: the cursor, the vocabulary, the window's runs.
#[derive(Debug, Default)]
pub struct Window {
    pub cursor: Cursor,
    pub vocab: Vocab,
    pub days: BTreeMap<String, Day>,
}

impl Window {
    pub fn runs(&self) -> Vec<Run> {
        self.days.values().flat_map(|d| d.runs.clone()).collect()
    }
}

/// The dates of the window ending at `now_ms`, newest first.
pub fn window_dates(now_ms: u64, days: u32) -> Vec<String> {
    (0..u64::from(days))
        .map(|k| date_of(now_ms.saturating_sub(k * 86_400_000)))
        .collect()
}

/// Read the cursor, the vocabulary and the window's days.
pub fn load(store: &Store, now_ms: u64, window_days: u32) -> Result<Window> {
    let cursor = store.get_meta::<Cursor>(CURSOR_KEY)?.unwrap_or_default();
    let vocab = store.get_meta::<Vocab>(VOCAB_KEY)?.unwrap_or_default();
    let mut days = BTreeMap::new();
    for d in window_dates(now_ms, window_days) {
        if let Some(day) = store.get_meta::<Day>(&day_key(&d))? {
            if !day.runs.is_empty() {
                days.insert(d, day);
            }
        }
    }
    Ok(Window {
        cursor,
        vocab,
        days,
    })
}

impl Window {
    /// Fold a night's runs into the days. Returns the dates that changed.
    pub fn add(&mut self, f: Folded) -> Vec<String> {
        self.vocab.merge(&f.vocab);
        let mut changed = std::collections::BTreeSet::new();
        for run in f.runs {
            let day = self.days.entry(run.day.clone()).or_insert_with(|| Day {
                date: run.day.clone(),
                runs: vec![],
            });
            changed.insert(run.day.clone());
            match day.runs.iter_mut().find(|r| r.same(&run)) {
                Some(r) => {
                    let off = r.loops.len() as u32;
                    r.calls.extend(run.calls.into_iter().map(|mut c| {
                        c.loop_ix += off;
                        c
                    }));
                    r.loops.extend(run.loops);
                    r.last_ms = r.last_ms.max(run.last_ms);
                }
                None => day.runs.push(run),
            }
        }
        changed.into_iter().collect()
    }

    /// The frame's records: the cursor, the days that changed, the day that
    /// left the window written empty, the vocabulary.
    pub fn records(
        &mut self,
        changed: &[String],
        expired: Option<String>,
    ) -> Result<Vec<NewRecord>> {
        let mut out = Vec::new();
        let runs = self.runs();
        self.vocab.retain(&runs);
        out.push(NewRecord::json(
            kinds::META,
            Some(CURSOR_KEY),
            &self.cursor,
        )?);
        out.push(NewRecord::json(kinds::META, Some(VOCAB_KEY), &self.vocab)?);
        for d in changed {
            if let Some(day) = self.days.get(d) {
                out.push(NewRecord::json(kinds::META, Some(&day_key(d)), day)?);
            }
        }
        if let Some(d) = expired {
            let empty = Day {
                date: d.clone(),
                runs: vec![],
            };
            out.push(NewRecord::json(kinds::META, Some(&day_key(&d)), &empty)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
