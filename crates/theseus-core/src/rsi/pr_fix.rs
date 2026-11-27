//! The fix at the gate (issue 36; `join::fix`): a gate red in the change's own work is given once
//! to a fixer before the attempt ends. The fixer's commit is gated again by the same reading
//! (`judged`), and on green the builder's profile reads it alone; accepted, the attempt goes on
//! green, as any green attempt does. Anything else ends the attempt red, as today: one try, with
//! the report, the task's word and the model's result saying a fix was tried, and the tree put
//! back to the head before the fix at once (`put_back`), so the builder's next repair is made where
//! it will be built, and kept. With `[self_fix] mode = "off"`, or a
//! red that is not the change's own (a keel stop, a red main, a fault of the machine, a check that
//! kept aborting, a gate that named no phase), nothing here runs.

use std::sync::Arc;

use serde_json::Value;

use super::tree::{GateRun, Place, Tree, Verdict};
use super::{judged, Attempt, Board};
use crate::rpc::Core;
use crate::rsi::join::fix::{self, make::Job, make::Made, Point};
use crate::toolrun::TurnCtx;

/// What a fix at the gate did, for the report, the task's word and the model's result.
#[derive(Debug, Clone)]
pub(super) struct Note {
    /// The phase the gate was red in.
    phase: String,
    /// What came of it, in a sentence.
    pub(super) words: String,
    made: Option<Made>,
    approver: Option<String>,
}

impl Note {
    /// A fix at the gate for a red in `phase`.
    fn of(phase: &str, words: String, made: Option<Made>, approver: Option<String>) -> Self {
        Self {
            phase: phase.to_string(),
            words,
            made,
            approver,
        }
    }

    /// A fix of this branch that a restart cut short, before this call.
    fn cut_short(words: String) -> Self {
        Self {
            phase: "an earlier attempt of this branch".into(),
            words,
            made: None,
            approver: None,
        }
    }

    /// The report's section.
    pub(super) fn section(&self) -> String {
        let made = self.made.as_ref().map_or_else(
            || "- Commit: none\n".to_string(),
            |m| {
                format!(
                    "- Fixer: {}\n- Commit: {} ({} lines in {} files), from {}\n",
                    m.fixer, m.head, m.lines, m.files, m.before
                )
            },
        );
        format!(
            "## The fix\n\n- Point: at the gate, for a red in {}\n{made}- Approved by: {}\n\
             - Outcome: {}\n\n",
            self.phase,
            self.approver.as_deref().unwrap_or("not approved"),
            self.words
        )
    }
}

/// The seams of a fix at the gate: the fixer and main's last gate.
#[derive(Clone)]
pub struct GateSeams {
    pub fixer: Arc<dyn fix::Fixer>,
    pub main: Arc<dyn crate::rsi::pipeline::main_red::Main>,
}

impl Default for GateSeams {
    fn default() -> Self {
        Self {
            fixer: Arc::new(fix::turns::Turns),
            // A test never reads the machine's own gate log.
            main: if cfg!(test) {
                Arc::new(crate::rsi::pipeline::main_red::Unrecorded)
            } else {
                Arc::new(crate::rsi::pipeline::main_red::Recorded::default())
            },
        }
    }
}

impl Board {
    /// The fix's seams: a test's, else the daemon's.
    pub(super) fn fix_seams(&self) -> GateSeams {
        self.fix
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap_or_default()
    }

    /// A test's fixer and main.
    #[cfg(test)]
    pub fn set_fix(&self, s: GateSeams) {
        *self
            .fix
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(s);
    }
}

/// A branch that has its report is done: another attempt is a new branch, cut from where the tree
/// stands, so the builder's edits since its red are kept. A fix that did not hold was put back
/// already, when it failed (`put_back`).
pub(super) async fn next_attempt(place: &Place, mut t: Tree) -> Result<Tree, String> {
    let p = place.clone();
    super::blocking(move || {
        super::tree::next_branch(&p, &mut t)?;
        Ok(t)
    })
    .await
}

/// The builder's verb's input, once it may run for `session`: never for a session the loop
/// opened as a fixer, an approver or a reviewer (first, before anything else), and, for a session
/// that builds an item of the backlog, only for that item (the review's round-2 note 3: another
/// open item's tree is not its own to commit and gate).
pub(super) fn builder_input(
    board: &Board,
    session: &str,
    input: &Value,
) -> Result<super::Input, String> {
    not_a_role(board, session)?;
    let i = super::input_of(input)?;
    let own = board
        .core()
        .and_then(|c| crate::rsi::own_tree::item_slug(&c.store, session));
    match own {
        Some(slug) if slug != super::tree::slug(&i.item) => Err(format!(
            "Not run: this session builds the item whose tree is {slug}, and {} is another \
             item's: a session commits and gates only its own item's tree.",
            i.item.trim()
        )),
        _ => Ok(i),
    }
}

/// The builder's verb refuses a session the loop opened as a fixer, an approver or a reviewer,
/// before anything else (`crate::rsi::roles`; a fork of one too); a builder's role is the one it serves. A
/// role that cannot be read refuses it too.
fn not_a_role(board: &Board, session: &str) -> Result<(), String> {
    let role = match board.core() {
        Some(c) => board.role_in(&c.store, session),
        None => Err(anyhow::anyhow!("the core is not attached")),
    };
    let role = role.map_err(|e| {
        format!(
            "Not run: the job the self loop opened this session for cannot be read ({e:#}), and only \
             the builder of an item calls {}.",
            super::PROPOSE
        )
    })?;
    match role {
        Some(role) if role != crate::rsi::roles::Role::Builder => Err(format!(
            "Not run: this session is {} of a self branch, and only the builder of an item calls \
             {}.",
            role.words(),
            super::PROPOSE
        )),
        _ => Ok(()),
    }
}

/// The tree an attempt works in: a branch that has its report is done, so the attempt is a new
/// branch ([`next_attempt`]); otherwise the same one, put back first when a restart cut its fix
/// short ([`resumed`]), with the words that say so.
pub(super) async fn prepared(
    core: &Core,
    board: &Board,
    place: &Place,
    t: Tree,
    item: &str,
) -> Result<(Tree, Option<Note>), String> {
    if super::report_path(place, &t.branch).exists() {
        let owed = owed(core, board, place, &t, item).await;
        return Ok((next_attempt(place, t).await?, owed));
    }
    let cut = resumed(core, board, place, &t).await?;
    Ok((t, cut))
}

/// A tree resumed after a restart whose branch's newest fix at the gate has no end (`started`, or
/// `made` and never approved): the fix is ended (`self.fix.none`) and the tree put back to the head
/// before it, before anything is committed or gated, so no fixer's change reaches a gate as the
/// builder's. The words the attempt says; none when there was no such fix. A fix this process
/// runs now is no restart's (`Board::live_fixes`).
pub(super) async fn resumed(
    core: &Core,
    board: &Board,
    place: &Place,
    t: &Tree,
) -> Result<Option<Note>, String> {
    if board.fix_is_live(&t.branch) {
        return Ok(None);
    }
    let Some(rows) = (!board.index_unread())
        .then(|| fix::rows_of(&core.store, &t.branch, Point::Gate))
        .transpose()
        .map_err(|e| format!("{e:#}"))?
        .flatten()
    else {
        // With the mode off no fix runs, and a build waits on no index, as before issue 36: a
        // committed fixer's commit, if one was ever left, is still caught by the green rule from
        // the commits' own trailers (`fix::unapproved`).
        if !core.cfg.self_fix.on() {
            return Ok(None);
        }
        return Err(
            "Not run: the record is still being read after a start, so whether a fix of this \
             branch was cut short cannot be told yet. Call again in a minute."
                .into(),
        );
    };
    let Some(at) = rows.iter().rposition(|r| r.kind == "self.fix.started") else {
        return Ok(None);
    };
    if rows[at + 1..].iter().any(|r| fix::ends(&r.kind)) {
        return Ok(None);
    }
    let d = &rows[at].data;
    let head = d["head"].as_str().unwrap_or_default().to_string();
    if head.is_empty() {
        return Ok(None);
    }
    let why = "a restart cut the fix short";
    let m = crate::fact::rsi_fix::Moved::new(
        d["item"].as_str().unwrap_or_default(),
        format!("no fix of {} at the gate", t.branch),
    )
    .with("branch", t.branch.clone())
    .with("head", head.clone())
    .with("point", Point::Gate.name())
    .with("fixer", d["fixer"].clone())
    .why(why);
    core.rec(None).record(&crate::fact::rsi_fix::NoFix(m));
    let (p, tr, h) = (place.clone(), t.clone(), head.clone());
    super::blocking(move || super::tree::reset_to(&p, &tr, &h)).await?;
    Ok(Some(Note::cut_short(format!(
        "{why}, so the tree was put back to {}, the commit before the fix, and what was not \
         committed since is gone",
        &head[..head.len().min(12)]
    ))))
}

/// The gate's reading of the tree (`judged`), and a red given once to a fixer when it may be: the
/// run the attempt goes on with and the branch's commits, or the way out of a halt. A head that
/// would be green and holds a fixer's commit no other model approved is red (`fix::unapproved`).
pub(super) async fn gated(
    core: &Arc<Core>,
    board: &Board,
    place: &Place,
    tc: &TurnCtx<'_>,
    a: &mut Attempt<'_>,
    commits: Vec<(String, String)>,
) -> Result<(GateRun, Vec<(String, String)>), Result<(String, Value), String>> {
    let run = judged(a, place).await?;
    let mut run = after_red(core, board, place, tc, a, run).await?;
    if run.verdict == Verdict::Green {
        let (s, p, dir) = (core.store.clone(), place.clone(), a.tree.dir.clone());
        let found =
            super::blocking(move || fix::unapproved(&s, &p, &dir, "origin/main..HEAD")).await;
        let why = match found {
            Ok(w) => w,
            Err(e) => Some(format!(
                "whether the branch holds an unapproved fix could not be read: {e}"
            )),
        };
        if let Some(why) = why {
            run.verdict = Verdict::Red;
            run.tail = format!("fix: {why}\n{}", run.tail);
        }
    }
    if a.fix.is_none() {
        return Ok((run, commits));
    }
    let (p, t) = (place.clone(), a.tree.clone());
    match super::blocking(move || super::tree::commits(&p, &t)).await {
        Ok(c) => Ok((run, c)),
        Err(e) => Err(Err(e)),
    }
}

/// A red gate, once to a fixer when it may be: the run the attempt goes on with (green when the
/// fix held), or the way out of a halt.
async fn after_red(
    core: &Arc<Core>,
    board: &Board,
    place: &Place,
    tc: &TurnCtx<'_>,
    a: &mut Attempt<'_>,
    run: GateRun,
) -> Result<GateRun, Result<(String, Value), String>> {
    // A fix this call put back after a restart is this attempt's one (`resumed`).
    if run.verdict != Verdict::Red
        || !core.cfg.self_fix.on()
        || super::stopped(core).is_some()
        || a.fix.is_some()
    {
        return Ok(run);
    }
    let Ok(phase) = fix::own_work(&run) else {
        return Ok(run);
    };
    let seams = board.fix_seams();
    if seams.main.red(place).is_some() {
        return Ok(run);
    }
    let Ok(main) = crate::rsi::join::git::main_of(place, "origin") else {
        return Ok(run);
    };
    let (tree, item, why) = (a.tree.clone(), a.i.item.trim().to_string(), a.i.why.clone());
    let job = Job {
        core,
        place,
        tree: &tree,
        item: &item,
        point: Point::Gate,
        builder: Some(tc.session_id),
        why: &why,
        fault: phase.clone(),
        failure: run.tail.clone(),
        main,
        fixer: seams.fixer,
    };
    let _live = board.live_fix(&tree.branch);
    let before = {
        let (p, t) = (place.clone(), tree.clone());
        super::blocking(move || p.git_ok(&t.dir, &["rev-parse", "HEAD"])).await
    };
    let Ok(before) = before else {
        return Ok(run);
    };
    let (run, note, held) = match job.make().await {
        Ok(made) => after_made(&job, a, place, made).await?,
        Err(why) => (run, Note::of(&phase, why, None, None), false),
    };
    a.fix = Some(note);
    // A fix that did not hold leaves nothing of the fixer's in the tree (review round 2, R2-1):
    // it is put back to the head before the fix now, before the builder reads the red, so the
    // repair the builder makes next is made in the tree it will build on, and kept.
    let mut run = run;
    if !held {
        if let Err(e) = put_back(board, place, a, &before).await {
            let words = format!(
                "the tree could not be put back to {}, the commit before the fix ({e}); the \
                 fixer's commit is still in it, and the next call puts it back",
                &before[..before.len().min(12)]
            );
            // The red's head is the one the tree holds, the fixer's commit in it.
            let (p, t) = (place.clone(), a.tree.clone());
            if let Ok(h) = super::blocking(move || p.git_ok(&t.dir, &["rev-parse", "HEAD"])).await {
                a.head = Some(h);
            }
            run.verdict = Verdict::Red;
            run.tail = format!("fix: {words}\n{}", run.tail);
            if let Some(f) = a.fix.as_mut() {
                f.words = format!("{}; {words}", f.words);
            }
        }
    }
    Ok(run)
}

/// The fix's commit made: the gate again, then the builder's profile. The run, the fix's note, and
/// whether the fix held.
async fn after_made(
    job: &Job<'_>,
    a: &mut Attempt<'_>,
    place: &Place,
    made: Made,
) -> Result<(GateRun, Note, bool), Result<(String, Value), String>> {
    let phase = job.fault.clone();
    a.head = Some(made.head.clone());
    let (p, t) = (place.clone(), a.tree.clone());
    if let Ok(d) = super::blocking(move || super::tree::diffstat(&p, &t)).await {
        a.diff = d;
    }
    let again = judged(a, place).await?;
    if again.verdict != Verdict::Green {
        let why = job.red_again(&made, &again.tail);
        return Ok((again, Note::of(&phase, why, Some(made), None), false));
    }
    match job.approve(&made).await {
        Ok(approver) => {
            let words = format!(
                "the gate's red in {phase} was repaired by {}, the gate is green again, and {approver} \
                 accepted the fix",
                made.fixer
            );
            Ok((
                again,
                Note::of(&phase, words, Some(made), Some(approver)),
                true,
            ))
        }
        Err(why) => {
            let mut red = again;
            red.verdict = Verdict::Red;
            red.tail = format!(
                "fix: the gate was green on the fix, but {why}\n{}",
                red.tail
            );
            Ok((red, Note::of(&phase, why, Some(made), None), false))
        }
    }
}

/// The tree back to `before`, the head before a fix that did not hold, on its branch: the fix's
/// commit and anything of the fixer's gone, and the attempt's head and size said again. When the
/// reset fails, the attempt's head stays the one the tree holds, and the next call puts it back
/// (`owed`).
async fn put_back(
    board: &Board,
    place: &Place,
    a: &mut Attempt<'_>,
    before: &str,
) -> Result<(), String> {
    let (p, t, b) = (place.clone(), a.tree.clone(), before.to_string());
    let d = reset(board, move || {
        super::tree::reset_to(&p, &t, &b)?;
        super::tree::diffstat(&p, &t)
    })
    .await
    .inspect_err(|e| {
        tracing::warn!(error = %e, branch = %a.tree.branch, "self fix: the tree was not put back");
    })?;
    a.diff = d;
    a.head = Some(before.to_string());
    Ok(())
}

/// A reset of a self tree, which a test can make fail (`Board::set_resets_fail`).
async fn reset<T: Send + 'static>(
    board: &Board,
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    match board.reset_fails() {
        true => Err("a test's refusal".into()),
        false => super::blocking(f).await,
    }
}

/// The words of a tree the last call could not put back (`put_back`), put back now, before
/// anything is committed: a commit of the item's fixer at the gate that was never accepted is
/// still under the tree's head, on this branch or an earlier one. The tree goes back to the
/// commit before the oldest such fix; like a restart's put-back (`resumed`), what was made since
/// is gone, none of which could go green on top of that fix. When it fails again, the fixer's
/// commit stays and the attempt is red by the green rule (`fix::unapproved`): a try, so the item
/// is held for the owner when its tries run out (`backlog::attempts`), and every call tries again.
/// None when nothing is owed, or the record cannot be read yet (the green rule still holds).
async fn owed(core: &Core, board: &Board, place: &Place, t: &Tree, item: &str) -> Option<Note> {
    if board.index_unread() {
        return None;
    }
    let rows = fix::item_rows(&core.store, item, Point::Gate)
        .ok()
        .flatten()?;
    let accepted: Vec<String> = rows
        .iter()
        .filter(|r| r.kind == "self.fix.verdict" && r.data["verdict"] == "accept")
        .filter_map(|r| r.data["head"].as_str().map(str::to_string))
        .collect();
    let made: Vec<(String, String)> = rows
        .iter()
        .filter(|r| r.kind == "self.fix.made" || r.kind == "self.fix.refused")
        .filter_map(|r| {
            Some((
                r.data["head"].as_str()?.into(),
                r.data["from"].as_str()?.into(),
            ))
        })
        .filter(|(head, _)| !accepted.contains(head))
        .collect();
    if made.is_empty() {
        return None;
    }
    let (p, tr) = (place.clone(), t.clone());
    let under = super::blocking(move || {
        Ok(made.into_iter().find(|(head, _)| {
            p.git_ok(&tr.dir, &["merge-base", "--is-ancestor", head, "HEAD"])
                .is_ok()
        }))
    })
    .await
    .ok()
    .flatten()?;
    let before = under.1;
    let short = before[..before.len().min(12)].to_string();
    let (p, tr) = (place.clone(), t.clone());
    let words = match reset(board, move || super::tree::reset_to(&p, &tr, &before)).await {
        Ok(()) => format!(
            "a fixer's commit that did not hold was still in the tree, which an earlier call \
             could not put back, so it is put back now to {short}, the commit before that fix, \
             and what was made since is gone"
        ),
        Err(e) => format!(
            "a fixer's commit that did not hold is still in the tree, which could not be put \
             back to {short}, the commit before that fix, again ({e}): this attempt is red and \
             counts as a try, the item waits for the owner when its tries run out, and the next \
             call tries again"
        ),
    };
    Some(Note::cut_short(words))
}
