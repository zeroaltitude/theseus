//! Check tasks (M5 step 28a, theseus-vug.3; M5 §2.11): independence as a
//! compiler property. `task.create { check_of, profile? }` opens a task that
//! checks another task's work by its claim, never its working, and records
//! why it is independent of it.
//!
//! - **What it names.** `check_of` resolves among the tasks the calling
//!   session started (the place rule: as a quote resolves only in its own
//!   transcript), by id or the end of it (`task::resolve`). A task with no
//!   report (running, waiting, failed, or cancelled) is refused with the
//!   reason. A check takes its parent's class, as every task does.
//! - **The model.** `profile` names the check's model, and only a check's:
//!   every other task runs on its parent's target. Without it, the check
//!   runs on its parent's target too.
//! - **27's rule.** The checked task's standing `objective` and `acceptance`
//!   pieces (from its `Arrangement` node) stand for the check's own, so a
//!   check needs no arrangement of its own when they hold an objective; it
//!   may add pieces, which resolve as 27's do. The fidelity check does not
//!   apply: a check's work is the claim, not the parent's discussion.
//! - **The admission.** The check's session gets its brief, then one
//!   `Arrangement` node: the inherited pieces, its own, and the claim (the
//!   checked task's report, "claimed by task a1b2c3, as of …"), written in
//!   `open_task`'s frame, with a `derived_from` edge to each piece's node and
//!   one to the report (`VIA_CLAIM`). Since `compile()` reads only the
//!   check's own nodes, nothing else of the checked task's session reaches
//!   it but through what the parent writes: its brief, and its own pieces.
//! - **The exclusion** is enforced on those. A piece of the check's own that
//!   resolves to a copy of the report (the parent's relayed report node)
//!   renders as the claim; one whose node derives, by `derived_from` edges,
//!   from any other node of the checked task's session is refused, naming the
//!   exclusion.
//! - **The overlap flag.** The brief, or a piece of the check's own, that
//!   shares a span of `OVERLAP_WORDS` words or more with a node of the
//!   checked task's session is flagged; the check still runs, and each flag
//!   is part of the basis. The report, the brief, and the `Arrangement` node
//!   of that session are left out of the comparison: the report is admitted,
//!   and the other two are the parent's own words. A word is a maximal run of
//!   letters and digits, compared without case, so punctuation and spacing
//!   neither make nor break a match.
//! - **The basis** (`theseus_protocol::TaskCheck`) is on the check's session
//!   record (`TaskOf.check`): the checked task, the excluded sessions, the
//!   admitted pieces, the model and profile, and the overlap flags; its row
//!   is `task.check_opened`, and a refusal's `task.check_refused`. Its line
//!   (`TaskCheck::line`) shows beside the check's report, on `theseus
//!   tasks`, and in the cockpit's task view.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_kernel::ExecState;
use theseus_protocol::{CheckOverlap, CheckPiece, TaskCheck};
use theseus_store::kinds;

use crate::arrangement::{self as arr, Piece, Refused, Role};
use crate::graph::{Edge, EdgeKind};
use crate::node::{Body, Node};
use crate::session::{SessionRecord, TargetRef};
use crate::toolrun::TurnCtx;

/// The shortest span, in words, the overlap flag counts.
pub const OVERLAP_WORDS: usize = 12;
/// The most of a flagged span its basis keeps, in characters.
pub const SPAN_CHARS: usize = 200;
/// The most nodes the exclusion's walk of `derived_from` edges visits.
pub const WALK_NODES: usize = 20_000;

/// A check's claim, as its `Arrangement` node keeps it: the checked task's
/// report, which the check's compilation renders after the pieces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    /// The checked task's session, and how people name it.
    pub task: String,
    pub short: String,
    /// Its report: its last message, in its own session, and when.
    pub node: String,
    pub at_ms: u64,
    /// The report's text, capped at `arrangement::PIECE_CHARS`.
    pub text: String,
}

/// An `Arrangement` node's text as a compilation renders it: its pieces,
/// then a check's claim.
pub fn render(pieces: &[Piece], claim: Option<&Claim>) -> String {
    let Some(c) = claim else {
        return arr::render(pieces);
    };
    let mut out = String::new();
    if !pieces.is_empty() {
        out.push_str(&arr::render(pieces));
        out.push_str("\n\n");
    }
    out.push_str(&format!(
        "[Check: you check task {s}'s work by its claim, never by its working: you see nothing \
         of its session but its report, below. The pieces above, if any, say what it was asked \
         to do. Establish for yourself, from evidence you gather now, whether the claim holds, \
         and report what you found: it holds, it does not, or it could not be established, and \
         why.]\n\n--- Claim: claimed by task {s}, as of {} ---\n{}",
        arr::utc(c.at_ms),
        c.text,
        s = c.short,
    ));
    out
}

/// What `task.create { check_of }` asks.
pub struct Ask<'a> {
    pub name: &'a str,
    pub profile: Option<&'a str>,
    pub arrangement: Option<&'a arr::Input>,
    pub brief: &'a str,
}

/// A check, resolved: what its session is given, and its basis.
pub struct Prepared {
    /// The inherited pieces, then its own.
    pub pieces: Vec<Piece>,
    pub claim: Claim,
    pub basis: TaskCheck,
    /// What it runs on, when the call named a profile.
    pub target: Option<TargetRef>,
}

/// Resolve a check against the calling session's transcript `nodes`
/// (leaving out `holder`, the reply that holds the call) and the profiles
/// the config names. A refusal is ledgered (`task.check_refused`), and the
/// model reads why.
pub fn prepare(
    tc: &TurnCtx<'_>,
    ask: &Ask<'_>,
    nodes: &crate::store::Transcript,
    holder: Option<&str>,
    profiles: &BTreeMap<String, TargetRef>,
) -> Result<Prepared, String> {
    let refuse = |r: Refused| {
        tc.record(&crate::fact::check::TaskCheckRefused {
            class: r.class,
            checked: ask.name,
            reason: &r.message,
        });
        r.message
    };
    let no = |class: &'static str, message: String| refuse(Refused { class, message });
    let (e, claim) = claimed(tc, ask.name, &no)?;
    let (short, node) = (claim.short.clone(), claim.node.clone());
    let target = match ask.profile {
        Some(p) => Some(profile(profiles, p, &no)?),
        None => None,
    };
    let runs_on = target.clone().or_else(|| {
        tc.target.map(|t| TargetRef {
            profile: t.profile.clone(),
            provider: t.provider.clone(),
            model: t.model.clone(),
        })
    });
    let rec: Option<SessionRecord> = tc
        .store
        .get_session(&e.session_id)
        .map_err(|e| format!("Not started: {e:#}"))?;
    let inherited = inherited(tc.store, rec.as_ref());
    let checked: Vec<Node> = tc
        .store
        .session_nodes(&e.session_id)
        .map_err(|e| format!("Not started: {e:#}"))?
        .into_iter()
        .map(|(_, n)| n)
        .collect();
    let defines = inherited.iter().any(|p| p.role == Role::Objective);
    let own = match ask.arrangement {
        Some(a) => {
            match a.check() {
                Err(m) if m.starts_with(arr::REFUSAL) && defines => {}
                Err(m) => return Err(no("invalid", m)),
                Ok(()) => {}
            }
            if a.pieces.is_empty() {
                vec![]
            } else {
                arr::resolve(a, nodes, holder).map_err(&refuse)?
            }
        }
        None => vec![],
    };
    if !defines
        && !own
            .iter()
            .any(|p| p.superseded_by.is_none() && matches!(p.role, Role::Objective | Role::Design))
    {
        return Err(no(
            "no_objective",
            format!(
                "{} Task {short}'s arrangement has no objective piece to stand for the check's: \
                 add `arrangement.pieces`, quoting what the task was asked to do (role \
                 `objective`).",
                arr::REFUSAL
            ),
        ));
    }
    let (pieces, admitted) = admit(tc.store, &inherited, &own, &claim, &checked, &no)?;
    let overlaps = flagged(
        ask.brief,
        &pieces[inherited.len()..],
        inherited.len(),
        &checked,
        &node,
    );
    let basis = TaskCheck {
        checked_task: e.session_id.clone(),
        checked_short: short,
        report_node: node,
        report_at_ms: claim.at_ms,
        excluded_sessions: vec![e.session_id],
        admitted,
        profile: runs_on
            .as_ref()
            .map(|t| t.profile.clone())
            .unwrap_or_default(),
        provider: runs_on
            .as_ref()
            .map(|t| t.provider.clone())
            .unwrap_or_default(),
        model: runs_on.map(|t| t.model).unwrap_or_default(),
        overlaps,
        at_ms: theseus_protocol::now_unix_ms(),
    };
    Ok(Prepared {
        pieces,
        claim,
        basis,
        target,
    })
}

/// The task `name` names among those this conversation started, and its
/// report as a claim; refused (`no`) when it has none.
fn claimed(
    tc: &TurnCtx<'_>,
    name: &str,
    no: &dyn Fn(&'static str, String) -> String,
) -> Result<(theseus_kernel::Execution, Claim), String> {
    // Only the tasks this conversation started (the place rule).
    let mine = tc
        .kernel
        .tasks(Some(tc.execution_id))
        .map_err(|e| format!("Not started: {e:#}"))?;
    // 39a's record id (`tsk_…`, which the task graph's view and
    // `task.create`'s result show) names a task session's task too.
    let by_record = mine
        .iter()
        .find(|e| crate::task_graph::of_session(&e.session_id) == name.trim());
    let e = match by_record {
        Some(e) => e,
        None => crate::task::resolve(&mine, name).map_err(|m| {
            no(
                "unknown",
                format!("Not started: {m} among the tasks this conversation started; a check reads only those"),
            )
        })?,
    };
    let short = crate::task::short(&e.session_id);
    if !e.state.is_terminal() {
        return Err(no(
            "no_report",
            format!(
                "Not started: task {short} is {} and has not reported. A check reads a task's \
                 report, so start it once the task has finished.",
                e.state.as_str()
            ),
        ));
    }
    let report = crate::task::load_report(tc.store, tc.kernel, &e.id)
        .map_err(|e| format!("Not started: {e:#}"))?;
    let (node, text) = match report {
        Some(crate::task::Report {
            node: Some(n),
            text: Some(t),
            ..
        }) if e.state == ExecState::Complete => (n, t),
        r => {
            let why = r
                .and_then(|r| r.reason)
                .map(|r| format!(" ({r})"))
                .unwrap_or_default();
            return Err(no(
                "no_report",
                format!(
                    "Not started: task {short} {}{why}, and has no report to check. A check \
                     reads a finished task's report.",
                    e.state.as_str()
                ),
            ));
        }
    };
    let at_ms = tc
        .store
        .get_node(&node)
        .ok()
        .flatten()
        .map_or(e.updated_at_ms, |(_, n)| n.created_at_ms);
    let (capped, _) = crate::toolrun::cap(&text, arr::PIECE_CHARS, |_| {
        format!("the whole report stays in task {short}'s session")
    });
    let claim = Claim {
        task: e.session_id.clone(),
        short,
        node,
        at_ms,
        text: capped,
    };
    Ok((e.clone(), claim))
}

/// The profile `name` names; refused (`no`) when none does.
fn profile(
    profiles: &BTreeMap<String, TargetRef>,
    name: &str,
    no: &dyn Fn(&'static str, String) -> String,
) -> Result<TargetRef, String> {
    profiles.get(name).cloned().ok_or_else(|| {
        no(
            "profile",
            format!(
                "Not started: no profile is named `{name}`; configured: {}",
                profiles.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
        )
    })
}

/// The overlap flag: the brief and the check's own `pieces` (from index
/// `first`), against the `checked` session's nodes but its report and brief.
fn flagged(
    brief: &str,
    pieces: &[Piece],
    first: usize,
    checked: &[Node],
    report: &str,
) -> Vec<CheckOverlap> {
    let mut sources = vec![("brief".to_string(), brief.to_string())];
    for (i, p) in pieces.iter().enumerate() {
        if let Some(t) = &p.text {
            sources.push((format!("piece {}", first + i), t.clone()));
        }
    }
    let against: Vec<(&str, String)> = checked
        .iter()
        .filter(|n| n.id != report && !crate::task::is_brief(n))
        .filter_map(|n| working_text(n).map(|t| (n.id.as_str(), t)))
        .collect();
    overlaps(&sources, &against)
}

/// The check's pieces (the inherited, then its own) and what it was
/// admitted, the exclusion enforced on its own pieces: one that copies the
/// report reads as the claim, and one that derives from any other node of
/// the `checked` session is refused (`no`).
fn admit(
    store: &crate::store::Store,
    inherited: &[Piece],
    own: &[Piece],
    claim: &Claim,
    checked: &[Node],
    no: &dyn Fn(&'static str, String) -> String,
) -> Result<(Vec<Piece>, Vec<CheckPiece>), String> {
    let (node, short) = (&claim.node, &claim.short);
    // The exclusion, on the check's own pieces.
    let copies = copies_of_report(store, node).map_err(|e| format!("Not started: {e:#}"))?;
    let roots = checked.iter().map(|n| n.id.clone()).filter(|id| id != node);
    let derived = derived_from(store, roots).map_err(|e| format!("Not started: {e:#}"))?;
    let mut admitted: Vec<CheckPiece> = inherited.iter().map(|p| piece_of(p, "checked")).collect();
    let mut keep = Vec::with_capacity(own.len());
    for (i, p) in own.iter().enumerate() {
        if copies.contains(&p.node) {
            admitted.push(CheckPiece {
                node_id: p.node.clone(),
                session_id: p.session_id.clone(),
                role: "claim".into(),
                from: "own".into(),
            });
            continue;
        }
        if let Some(src) = derived.get(&p.node) {
            return Err(no(
                "excluded",
                format!(
                    "Not started (the exclusion): piece {i} quotes node {}, which derives from \
                     node {src} of task {short}'s session. A check reads nothing of the task \
                     it checks but its report: quote the report, or the messages that defined \
                     the work, and leave its working out.",
                    p.node
                ),
            ));
        }
        if inherited.iter().any(|q| q.node == p.node) {
            continue;
        }
        keep.push(i);
    }
    // Its own pieces after the inherited ones, their `supersedes` moved with them.
    let at: HashMap<usize, u32> = keep
        .iter()
        .enumerate()
        .map(|(k, i)| (*i, (inherited.len() + k) as u32))
        .collect();
    let mut pieces = inherited.to_vec();
    for i in &keep {
        let mut p = own[*i].clone();
        p.superseded_by = p.superseded_by.and_then(|b| at.get(&(b as usize)).copied());
        pieces.push(p);
    }
    for p in &pieces[inherited.len()..] {
        admitted.push(piece_of(p, "own"));
    }
    admitted.push(CheckPiece {
        node_id: node.clone(),
        session_id: claim.task.clone(),
        role: "claim".into(),
        from: "claim".into(),
    });
    Ok((pieces, admitted))
}

/// `task.create`'s result for a check: its basis's line, each flagged span,
/// and the basis in its meta.
pub fn said(text: String, mut meta: Value, basis: &TaskCheck) -> (String, Value) {
    let mut out = format!(
        "{text}\n{}. It reads its brief, {}, and that task's report as a claim, and nothing \
         else of its session.",
        basis.line(),
        crate::narrative::count(
            basis.admitted.len().saturating_sub(1) as u64,
            "piece",
            "pieces"
        )
    );
    for o in &basis.overlaps {
        out.push_str(&format!(
            "\nFlagged: the {} shares {} words with node {} of that session: \"{}\"",
            o.source, o.words, o.node_id, o.span
        ));
    }
    if !basis.overlaps.is_empty() {
        out.push_str(
            "\nThe check runs anyway, and the flags are part of its basis: words copied from \
             the task's working are not independent of it.",
        );
    }
    meta["check"] = serde_json::json!(basis);
    (out, meta)
}

/// The checked task's standing `objective` and `acceptance` pieces, read
/// from its `Arrangement` node: none for a task from before 27.
fn inherited(store: &crate::store::Store, rec: Option<&SessionRecord>) -> Vec<Piece> {
    let Some(id) = rec.and_then(|r| r.task.as_ref()?.arrangement.clone()) else {
        return vec![];
    };
    let Ok(Some((_, n))) = store.get_node(&id) else {
        return vec![];
    };
    let Body::Arrangement { pieces, .. } = n.body else {
        return vec![];
    };
    pieces
        .into_iter()
        .filter(|p| {
            p.superseded_by.is_none()
                && p.text.is_some()
                && matches!(p.role, Role::Objective | Role::Acceptance)
        })
        .collect()
}

fn piece_of(p: &Piece, from: &str) -> CheckPiece {
    CheckPiece {
        node_id: p.node.clone(),
        session_id: p.session_id.clone(),
        role: p.role.as_str().into(),
        from: from.into(),
    }
}

/// The `derived_from` edges into `id`.
fn edges_into(store: &crate::store::Store, id: &str) -> anyhow::Result<Vec<Edge>> {
    let mut out = Vec::new();
    for r in store.scope_after(&Edge::scope_into(id), 0)? {
        if r.kind != kinds::EDGE {
            continue;
        }
        let e: Edge = r.decode()?;
        match EdgeKind::named(&e.kind) {
            Some(EdgeKind::DerivedFrom) => out.push(e),
            // The memory pass's edges (31a) say two nodes are alike, or that one
            // corrects the other: never that one derives from the other.
            Some(EdgeKind::SameEntity | EdgeKind::Supersedes) | None => {}
        }
    }
    Ok(out)
}

/// The nodes that copy the report: its relayed node in the parent.
fn copies_of_report(store: &crate::store::Store, report: &str) -> anyhow::Result<HashSet<String>> {
    Ok(edges_into(store, report)?
        .into_iter()
        .map(|e| e.from)
        .collect())
}

/// Every node that derives, by `derived_from` edges followed back from
/// `roots`, from one of them: each with the root it derives from. The walk
/// visits at most `WALK_NODES` nodes.
pub fn derived_from(
    store: &crate::store::Store,
    roots: impl Iterator<Item = String>,
) -> anyhow::Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    let mut queue: VecDeque<(String, String)> = roots.map(|r| (r.clone(), r)).collect();
    let mut seen = HashSet::new();
    while let Some((id, root)) = queue.pop_front() {
        if seen.len() >= WALK_NODES {
            break;
        }
        if !seen.insert(id.clone()) {
            continue;
        }
        for e in edges_into(store, &id)? {
            out.entry(e.from.clone()).or_insert_with(|| root.clone());
            queue.push_back((e.from, root.clone()));
        }
    }
    Ok(out)
}

/// A node's words as the overlap flag reads them: a message's text, a
/// reply's text and thinking and its calls' inputs, a call's input, a
/// result's content, a summary's text (its range's working, 30c). None for
/// an arrangement or a recall, whose words are others'.
pub fn working_text(n: &Node) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    match &n.body {
        Body::UserMessage { text, .. } => parts.push(text),
        Body::AssistantMessage { blocks, .. } => {
            for b in blocks {
                for k in ["text", "thinking"] {
                    if let Some(t) = b[k].as_str() {
                        parts.push(t);
                    }
                }
                strings(&b["input"], &mut parts);
            }
        }
        Body::ToolCall { input, .. } => strings(input, &mut parts),
        Body::ToolResult { content, .. } => parts.push(content),
        Body::Summary { text, .. } | Body::Synthesis { text, .. } => parts.push(text),
        Body::Arrangement { .. } | Body::Recall { .. } => return None,
    }
    let t = parts.join("\n");
    (!t.trim().is_empty()).then_some(t)
}

/// Every string in a JSON value.
fn strings<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    match v {
        Value::String(s) => out.push(s),
        Value::Array(a) => a.iter().for_each(|v| strings(v, out)),
        Value::Object(m) => m.values().for_each(|v| strings(v, out)),
        _ => {}
    }
}

/// A word: its byte range in its text, and its lower case.
struct Word {
    start: usize,
    end: usize,
    lower: String,
}

/// The words of `text`: maximal runs of letters and digits.
fn words(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push(Word {
                    start: s,
                    end: i,
                    lower: text[s..i].to_lowercase(),
                });
                start = None;
            }
            _ => {}
        }
    }
    out
}

fn window(w: &[Word]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for x in w {
        x.lower.hash(&mut h);
    }
    h.finish()
}

/// The spans of `OVERLAP_WORDS` words or more that each source (its name,
/// its text) shares with a text of `against` (a node's id, its text): each
/// maximal run of shared windows is one span, with the node that holds its
/// first window.
pub fn overlaps(sources: &[(String, String)], against: &[(&str, String)]) -> Vec<CheckOverlap> {
    let mut held: HashMap<u64, usize> = HashMap::new();
    for (k, (_, text)) in against.iter().enumerate() {
        for w in words(text).windows(OVERLAP_WORDS) {
            held.entry(window(w)).or_insert(k);
        }
    }
    let mut out = Vec::new();
    for (name, text) in sources {
        let w = words(text);
        let mut i = 0;
        while i + OVERLAP_WORDS <= w.len() {
            let Some(&k) = held.get(&window(&w[i..i + OVERLAP_WORDS])) else {
                i += 1;
                continue;
            };
            let mut j = i;
            while j + 1 + OVERLAP_WORDS <= w.len()
                && held.contains_key(&window(&w[j + 1..j + 1 + OVERLAP_WORDS]))
            {
                j += 1;
            }
            let last = j + OVERLAP_WORDS - 1;
            let span = &text[w[i].start..w[last].end];
            let mut cut: String = span.chars().take(SPAN_CHARS).collect();
            if span.chars().count() > SPAN_CHARS {
                cut.push('…');
            }
            out.push(CheckOverlap {
                source: name.clone(),
                node_id: against[k].0.to_string(),
                words: (last + 1 - i) as u32,
                span: cut,
            });
            i = last + 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKING: &str = "The harbour master's log says: the north pier holds forty two \
        herring gulls at low tide, counted from the lamp gallery at dawn.";

    fn flags(source: &str) -> Vec<CheckOverlap> {
        overlaps(
            &[("brief".into(), source.into())],
            &[("trs_log", WORKING.into())],
        )
    }

    /// Twelve shared words are a span; eleven are not. Case and punctuation
    /// neither make nor break a match.
    #[test]
    fn a_span_of_twelve_words_is_flagged_and_eleven_is_not() {
        // "the north pier holds forty two herring gulls at low tide, counted": 12.
        let twelve =
            "Check this: THE NORTH PIER holds forty-two herring gulls, at low tide; counted.";
        let got = flags(twelve);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].words, 12);
        assert_eq!(got[0].node_id, "trs_log");
        assert_eq!(got[0].source, "brief");
        assert_eq!(
            got[0].span,
            "THE NORTH PIER holds forty-two herring gulls, at low tide; counted"
        );
        let eleven = "Check this: the north pier holds forty two herring gulls at low tide.";
        assert!(flags(eleven).is_empty(), "{:?}", flags(eleven));
        // A longer run is one span, with its length.
        let all = format!("Re-count: {WORKING}");
        let got = flags(&all);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].words, 24);
        // Two runs apart are two spans.
        let two = "the harbour master's log says: the north pier holds forty two herring \
            gulls. Then: at low tide, counted from the lamp gallery at dawn, as the harbour \
            master's log says";
        let got = flags(two);
        assert_eq!(
            got.len(),
            1,
            "the second run is under twelve words: {got:?}"
        );
    }

    #[test]
    fn words_are_runs_of_letters_and_digits() {
        let w: Vec<String> = words("Forty-two gulls, at 06:00 — café!")
            .into_iter()
            .map(|w| w.lower)
            .collect();
        assert_eq!(w, ["forty", "two", "gulls", "at", "06", "00", "café"]);
    }

    /// A claim renders after the pieces, naming the task and its time.
    #[test]
    fn a_claim_renders_after_the_pieces() {
        let c = Claim {
            task: "ses_maker".into(),
            short: "a1b2c3".into(),
            node: "msg_report".into(),
            at_ms: 1_790_000_000_000,
            text: "There are 42 gulls.".into(),
        };
        let out = render(&[], Some(&c));
        assert!(
            out.starts_with("[Check: you check task a1b2c3's work"),
            "{out}"
        );
        assert!(
            out.ends_with(
                "--- Claim: claimed by task a1b2c3, as of 2026-09-21 14:13 UTC ---\nThere are 42 gulls."
            ),
            "{out}"
        );
        assert_eq!(render(&[], None), arr::render(&[]));
    }
}
