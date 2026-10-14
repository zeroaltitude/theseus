//! A frame's settling on the work board: the dirty entities built again, each
//! view compared with the one held, the ended children pruned, the changed
//! views' ancestors rolled up again, and the policy's notice made for each
//! changed view.

use std::collections::{BTreeSet, HashMap, HashSet};

use theseus_protocol::notices::Notice;
use theseus_protocol::work::{attention_of, Progress, Rollup, WorkKind, WorkState, WorkView};

use super::{notice_of, Dirty, Out, WorkBoard, ENDED_KEPT, RING};
use crate::push::Frame;

/// Whether two versions of a view show the same: everything but where and
/// when the frame was, and the state before it. A cost differs by a cent.
pub(super) fn same_work(a: &WorkView, b: &WorkView) -> bool {
    let cents = |x: f64| (x * 100.0).round() as i64;
    let cost = |v: &WorkView| v.cost.map(|c| (cents(c.spent_usd), cents(c.limit_usd)));
    a.id == b.id
        && a.kind == b.kind
        && a.parent == b.parent
        && a.root == b.root
        && a.session_id == b.session_id
        && a.title == b.title
        && a.place == b.place
        && a.state == b.state
        && a.attention.level == b.attention.level
        && a.attention.label == b.attention.label
        && a.started_at_ms == b.started_at_ms
        && a.ended_at_ms == b.ended_at_ms
        && doing_shown(a) == doing_shown(b)
        && a.progress == b.progress
        && a.question == b.question
        && cost(a) == cost(b)
        && a.eta == b.eta
        && a.below == b.below
        && a.why == b.why
        && a.report == b.report
        && a.woke == b.woke
        && a.flagged == b.flagged
        && a.thread == b.thread
}

/// One more descendant in a rollup.
pub(super) fn bump(r: &mut Rollup, v: &WorkView) {
    r.total += 1;
    match v.kind {
        WorkKind::Wake => {}
        WorkKind::Question if !v.state.ended() => r.needs_you += 1,
        WorkKind::Question => {}
        _ => match v.state {
            WorkState::Failed => r.failed += 1,
            WorkState::NeedsYou if v.question.is_none() => r.needs_you += 1,
            WorkState::NeedsYou => {}
            WorkState::Done if v.kind == WorkKind::Task => r.done += 1,
            WorkState::Queued if v.kind == WorkKind::Step => {}
            s if s.working() => r.working += 1,
            _ => {}
        },
    }
}

impl WorkBoard {
    /// Build the dirty entities, and settle: the frame's changed views
    /// (children before their parents' rollups), the ones that left, and the
    /// notices.
    pub(super) fn settle(&mut self, f: &Frame, d: Dirty) -> Out {
        let mut before: HashMap<String, Option<WorkView>> = HashMap::new();
        let mut changed: BTreeSet<String> = BTreeSet::new();
        let mut gone: Vec<(String, String)> = Vec::new();
        let mut built: Vec<(String, Option<WorkView>)> = Vec::new();
        for id in &d.execs {
            built.push((id.clone(), self.exec_view(id)));
        }
        for id in &d.wakes {
            built.push((id.clone(), self.wake_view(id)));
        }
        for id in &d.acts {
            built.push((id.clone(), self.act_view(id)));
        }
        for id in &d.steps {
            built.push((id.clone(), self.step_view(id)));
        }
        for (id, new) in built {
            match new {
                Some(v) => {
                    if self.put(f, &mut before, v) {
                        changed.insert(id);
                    }
                }
                None => {
                    if let Some(old) = self.views.get(&id).cloned() {
                        before
                            .entry(id.clone())
                            .or_insert_with(|| Some(old.clone()));
                        self.remove(&id);
                        gone.push((id, old.root));
                    }
                }
            }
        }
        // What the warm-up changed in place goes out with this frame.
        for id in std::mem::take(&mut self.stale) {
            if let Some(v) = self.views.get_mut(&id) {
                before.entry(id.clone()).or_insert_with(|| Some(v.clone()));
                v.position = f.position;
                v.at_ms = f.at_ms;
                changed.insert(id);
            }
        }
        self.prune(f, &changed, &mut gone, &mut before);
        let mut affected: BTreeSet<String> = BTreeSet::new();
        for id in changed.iter().chain(gone.iter().map(|(id, _)| id)) {
            let mut at = self
                .views
                .get(id)
                .and_then(|v| v.parent.clone())
                .or_else(|| before.get(id).and_then(|o| o.as_ref()?.parent.clone()));
            while let Some(p) = at {
                if !affected.insert(p.clone()) {
                    break;
                }
                at = self.views.get(&p).and_then(|v| v.parent.clone());
            }
            if self.views.get(id).is_some_and(|v| {
                matches!(
                    v.kind,
                    WorkKind::Conversation | WorkKind::Task | WorkKind::Step
                )
            }) {
                affected.insert(id.clone());
            }
        }
        for id in &affected {
            if self.roll(f, id, &mut before) {
                changed.insert(id.clone());
            }
        }
        let mut views: Vec<WorkView> = changed
            .iter()
            .filter_map(|id| self.views.get(id).cloned())
            .collect();
        views.sort_by_key(|v| (self.depth(&v.id), v.id.clone()));
        let mut notices = Vec::new();
        for v in &views {
            let prev = before.get(&v.id).and_then(|o| o.as_ref());
            if let Some(n) = notice_of(prev, v, &self.rules).filter(|n| !quiet_here(v, n)) {
                notices.push(n);
            }
        }
        for n in &notices {
            self.keep(n.clone());
        }
        Out {
            position: f.position,
            at_ms: f.at_ms,
            views,
            gone,
            notices,
            threads: Vec::new(),
        }
    }

    pub(super) fn keep(&mut self, n: Notice) {
        if self.ring.len() == RING {
            if let Some(old) = self.ring.pop_front() {
                self.evicted = self.evicted.max(old.position);
            }
        }
        self.ring.push_back(n);
    }

    fn depth(&self, id: &str) -> usize {
        let mut n = 0;
        let mut at = self.views.get(id).and_then(|v| v.parent.clone());
        while let Some(p) = at {
            n += 1;
            if n > 32 {
                break;
            }
            at = self.views.get(&p).and_then(|v| v.parent.clone());
        }
        n
    }

    /// Put a built view on the board: compared with the held one, given its
    /// frame's position, the state before, and when its state began. True
    /// when it shows something new.
    fn put(
        &mut self,
        f: &Frame,
        before: &mut HashMap<String, Option<WorkView>>,
        mut new: WorkView,
    ) -> bool {
        let old = self.views.get(&new.id).cloned();
        // What only the held view knows: the rollup and the plan's progress,
        // until the rollup pass makes them again.
        if let Some(o) = &old {
            new.below = o.below;
            new.progress = o.progress.clone();
        }
        new.position = f.position;
        new.at_ms = f.at_ms;
        new.thread = self.thread_for_root(&new.root);
        if let Some(o) = &old {
            new.previous = Some(o.state);
            let level = attention_of(&new).level;
            if o.state == new.state && o.attention.level == level {
                new.state_since_ms = o.state_since_ms;
            } else if new.state_since_ms <= o.state_since_ms {
                new.state_since_ms = f.at_ms;
            }
        }
        new.attention = attention_of(&new);
        if let Some(o) = &old {
            if same_work(o, &new) {
                return false;
            }
        }
        before.entry(new.id.clone()).or_insert_with(|| old.clone());
        self.install(new);
        true
    }

    /// Install a view and its place in the tree.
    pub(super) fn install(&mut self, v: WorkView) {
        if let Some(old) = self.views.get(&v.id) {
            if old.parent != v.parent {
                if let Some(p) = &old.parent {
                    if let Some(k) = self.kids.get_mut(p) {
                        k.remove(&v.id);
                    }
                }
            }
        }
        if let Some(p) = &v.parent {
            self.kids.entry(p.clone()).or_default().insert(v.id.clone());
        }
        self.views.insert(v.id.clone(), v);
    }

    fn remove(&mut self, id: &str) {
        if let Some(v) = self.views.remove(id) {
            if let Some(p) = &v.parent {
                if let Some(k) = self.kids.get_mut(p) {
                    k.remove(id);
                }
            }
        }
        self.kids.remove(id);
    }

    /// The descendants of `id`, nearest first.
    fn below_ids(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![id.to_string()];
        while let Some(at) = stack.pop() {
            if let Some(k) = self.kids.get(&at) {
                for c in k {
                    out.push(c.clone());
                    stack.push(c.clone());
                }
            }
        }
        out
    }

    /// Roll a view up again: what is under it by level, and a plan's steps.
    /// True when either changed.
    pub(super) fn roll(
        &mut self,
        f: &Frame,
        id: &str,
        before: &mut HashMap<String, Option<WorkView>>,
    ) -> bool {
        let Some(v) = self.views.get(id) else {
            return false;
        };
        let is_root = v.parent.is_none();
        let kind = v.kind;
        let mut r = if is_root {
            self.pruned.get(id).copied().unwrap_or_default()
        } else {
            Rollup::default()
        };
        let mut steps: Vec<&WorkView> = Vec::new();
        let below = self.below_ids(id);
        for c in &below {
            if let Some(c) = self.views.get(c) {
                bump(&mut r, c);
                if c.kind == WorkKind::Step && self.owned_by(c, id) {
                    steps.push(c);
                }
            }
        }
        let new_below = (r.total > 0).then_some(r);
        let progress = (kind != WorkKind::Question && !steps.is_empty()).then(|| {
            let counted: Vec<&&WorkView> = steps
                .iter()
                .filter(|s| s.state != WorkState::Cancelled)
                .collect();
            let done = counted
                .iter()
                .filter(|s| s.state == WorkState::Done)
                .count() as u32;
            let mut open: Vec<&&&WorkView> = counted
                .iter()
                .filter(|s| s.state != WorkState::Done)
                .collect();
            open.sort_by_key(|s| (s.state != WorkState::Running, s.started_at_ms, s.id.clone()));
            Progress {
                steps_done: done,
                steps_total: counted.len() as u32,
                current: open.first().map(|s| s.title.clone()),
                said: None,
                at_ms: steps.iter().map(|s| s.at_ms).max().unwrap_or(f.at_ms),
            }
        });
        let v = self.views.get(id).expect("read above");
        let progress = match (progress, &v.progress) {
            (Some(mut p), Some(o))
                if p.steps_done == o.steps_done
                    && p.steps_total == o.steps_total
                    && p.current == o.current =>
            {
                p.at_ms = o.at_ms;
                Some(p)
            }
            (p, _) => p,
        };
        if v.below == new_below && v.progress == progress {
            return false;
        }
        let old = v.clone();
        before.entry(id.to_string()).or_insert_with(|| Some(old));
        let v = self.views.get_mut(id).expect("read above");
        v.below = new_below;
        v.progress = progress;
        v.position = f.position;
        v.at_ms = f.at_ms;
        true
    }

    /// Whether a step is counted by `id`: no task or conversation between.
    fn owned_by(&self, step: &WorkView, id: &str) -> bool {
        let mut at = step.parent.clone();
        for _ in 0..32 {
            match at {
                Some(p) if p == id => return true,
                Some(p) => match self.views.get(&p) {
                    Some(v) if v.kind == WorkKind::Step => at = v.parent.clone(),
                    _ => return false,
                },
                None => return false,
            }
        }
        false
    }

    /// The 50 newest ended children of each touched root stay; the older
    /// leave, counted in the root's rollup.
    pub(super) fn prune(
        &mut self,
        f: &Frame,
        changed: &BTreeSet<String>,
        gone: &mut Vec<(String, String)>,
        before: &mut HashMap<String, Option<WorkView>>,
    ) {
        let roots: HashSet<String> = changed
            .iter()
            .filter_map(|id| self.views.get(id))
            .filter(|v| v.parent.is_some() && v.state.ended())
            .map(|v| v.root.clone())
            .collect();
        for root in roots {
            let mut ended: Vec<(u64, String)> = self
                .below_ids(&root)
                .into_iter()
                .filter_map(|id| self.views.get(&id))
                .filter(|v| prunable(v))
                .map(|v| (v.ended_at_ms.unwrap_or(v.at_ms), v.id.clone()))
                .collect();
            if ended.len() <= ENDED_KEPT {
                continue;
            }
            ended.sort();
            let cut = ended.len() - ENDED_KEPT;
            for (_, id) in ended.into_iter().take(cut) {
                self.forget(&root, &id, f, gone, before);
            }
        }
    }

    /// An ended view and what hangs from it leave the board, counted.
    fn forget(
        &mut self,
        root: &str,
        id: &str,
        f: &Frame,
        gone: &mut Vec<(String, String)>,
        before: &mut HashMap<String, Option<WorkView>>,
    ) {
        let mut ids = self.below_ids(id);
        ids.insert(0, id.to_string());
        let parent = self.views.get(id).and_then(|v| v.parent.clone());
        for gid in ids {
            if let Some(v) = self.views.get(&gid).cloned() {
                bump(self.pruned.entry(root.to_string()).or_default(), &v);
                before.entry(gid.clone()).or_insert_with(|| Some(v.clone()));
                self.remove(&gid);
                self.acts.remove(&gid);
                for set in self.acts_by_exec.values_mut() {
                    set.remove(&gid);
                }
                self.ended_wakes.remove(&gid);
                self.steps.remove(&gid);
                if let Some(ei) = self.execs.remove(&gid) {
                    self.by_session.remove(&ei.x.session_id);
                }
                gone.push((gid, root.to_string()));
            }
        }
        let _ = (f, parent);
    }
}

/// A notice another view of the same event already makes. A question has a
/// view of its own, and the execution that asks carries it in brief: the
/// question's view speaks for it, so the owner is told once. A wake that
/// turned a conversation on is told by that conversation's `woke`, and a
/// reminder reaches every surface as `wake.noticed`.
fn quiet_here(v: &WorkView, n: &Notice) -> bool {
    use theseus_protocol::notices::Why;
    v.kind == WorkKind::Wake
        || (v.kind != WorkKind::Question
            && matches!(
                n.why,
                Why::Asked | Why::Answered | Why::Expired | Why::Reminder
            ))
}

/// An ended view that may leave its root: done, cancelled, expired, and a
/// job that failed. A failed task stays until someone looks.
fn prunable(v: &WorkView) -> bool {
    match v.state {
        WorkState::Done | WorkState::Cancelled | WorkState::Expired => true,
        WorkState::Failed => v.kind == WorkKind::Job,
        _ => false,
    }
}

/// What a surface shows of what a view is doing: the kind, the words and the
/// turn, not the loop it is in or since when (those ride along with the next
/// change, so a loop's end alone sends nothing).
fn doing_shown(v: &WorkView) -> Option<(theseus_protocol::work::DoingKind, &str, u64)> {
    v.doing.as_ref().map(|d| (d.what, d.label.as_str(), d.turn))
}
