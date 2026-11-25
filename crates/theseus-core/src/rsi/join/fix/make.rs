//! One fix, at either point: the fixer resolved and refused as the review is, once a point, its
//! turn in the item's tree, the code's one commit, the bounds on it, and, once the caller's gate
//! is green again, the builder's profile reading that commit alone. Every step writes its row.
//!
//! The caller owns the tree and the gate: `rsi::pr` at the gate (the builder's own tree, and its
//! gate's reading, `judged`), [`super::at_review`] at review (the tree reopened on the branch).

use std::sync::Arc;

use super::bounds::{self, Change, Refusal, State};
use super::{ApproveAsk, FixAsk, Fixer, Point};
use crate::fact::rsi_fix::{Made as MadeRow, Moved, NoFix, Refused, Started, Verdict};
use crate::rpc::Core;
use crate::rsi::join::reviewing::{builder_target, not_independent, reviewer_for};
use crate::rsi::join::{blocking, git};
use crate::rsi::tree::{self, Place, Tree};

/// A fix to make: where, of what, and who.
pub(crate) struct Job<'a> {
    pub core: &'a Arc<Core>,
    pub place: &'a Place,
    pub tree: &'a Tree,
    pub item: &'a str,
    pub point: Point,
    /// The session that built the branch: its model is the one the fixer must not be, and the
    /// one that approves the fix.
    pub builder: Option<&'a str>,
    /// The change's own words.
    pub why: &'a str,
    /// The fault in a few words (`clippy`, `the review's finding`), for the commit's subject.
    pub fault: String,
    /// What failed, whole: the gate's last lines, or the review's reasons.
    pub failure: String,
    /// The remote's main the branch left.
    pub main: String,
    pub fixer: Arc<dyn Fixer>,
}

/// A fix made and within its bounds, its gate not yet run again.
#[derive(Debug, Clone)]
pub(crate) struct Made {
    /// The head before it.
    pub before: String,
    /// Its commit.
    pub head: String,
    pub lines: u64,
    pub files: u64,
    /// Who made it, `fable (claude-fable-5-2)`, and that model.
    pub fixer: String,
    pub fixer_model: String,
    /// Its sentence of why.
    pub reason: String,
    /// The fixer's session.
    pub session: Option<String>,
}

impl Job<'_> {
    /// A row of this fix, its fields set.
    fn moved(&self, head: &str, what: String) -> Moved {
        Moved::new(self.item, what)
            .with("branch", self.tree.branch.clone())
            .with("head", head.to_string())
            .with("point", self.point.name())
    }

    /// A bound refused it: its row, and the words the branch's way back carries.
    fn refused(&self, head: &str, r: &Refusal) -> String {
        self.refused_from(head, None, r)
    }

    /// [`Self::refused`] of a commit made from `from`: the row says where the branch was before it.
    fn refused_from(&self, head: &str, from: Option<&str>, r: &Refusal) -> String {
        let mut m = self
            .moved(
                head,
                format!(
                    "a fix of {} {} was refused: {}",
                    self.tree.branch,
                    self.point.words(),
                    r.bound
                ),
            )
            .with("bound", r.bound)
            .why(r.why.clone());
        if let Some(f) = from {
            m = m.with("from", f.to_string());
        }
        self.core.rec(None).record(&Refused(m));
        format!(
            "a fix was tried {} and refused: {}",
            self.point.words(),
            r.why
        )
    }

    /// No fix: its row (with the fixer's session, when it had one), and the words.
    fn none(&self, head: &str, fixer: &str, session: Option<&str>, why: &str) -> String {
        let mut m = self
            .moved(
                head,
                format!("no fix of {} {}", self.tree.branch, self.point.words()),
            )
            .with("fixer", fixer.to_string())
            .why(why.to_string());
        if let Some(s) = session {
            m = m.with("session", s.to_string());
        }
        self.core.rec(None).record(&NoFix(m));
        format!(
            "a fix was tried {} by {fixer} and made nothing: {why}",
            self.point.words()
        )
    }

    /// The fixer is the builder's model: said once while nothing else of a fix is said, as the
    /// review's refusal is, and never tried.
    fn not_independent(&self, head: &str, why: String) -> String {
        let r = Refusal {
            bound: "not-independent",
            why,
        };
        let said = crate::rsi::join::rows_of(&self.core.store, &super::KINDS)
            .ok()
            .flatten()
            .and_then(|rows| {
                rows.into_iter()
                    .rev()
                    .find(|row| row.kind != "self.fix.refused" || row.data["bound"] == r.bound)
            })
            .is_some_and(|row| row.kind == "self.fix.refused" && row.data["why"] == r.why.as_str());
        if said {
            return format!("no fix was tried: {}", r.why);
        }
        self.refused(head, &r)
    }

    /// The tree back to `before` on its branch, nothing of the fixer's left in it.
    fn restore(&self, before: &str) {
        let (p, dir, b) = (self.place, &self.tree.dir, &self.tree.branch);
        let back = p
            .git_ok(dir, &["checkout", "-q", "-f", "-B", b, before])
            .and_then(|_| p.git_ok(dir, &["clean", "-fdq"]));
        if let Err(e) = back {
            tracing::warn!(error = %format!("{e:#}"), branch = %b, "self fix: the tree was not put back");
        }
    }

    /// Make the fix: its commit within its bounds, or the words of why not (its row written).
    pub(crate) async fn make(&self) -> Result<Made, String> {
        let before = self
            .place
            .git_ok(&self.tree.dir, &["rev-parse", "HEAD"])
            .map_err(|e| format!("a fix could not start: {e:#}"))?;
        let who = reviewer_for(self.core, self.builder, self.why).await;
        if let Some(why) = who.refusal.clone() {
            return Err(self.not_independent(&before, why));
        }
        let Some(target) = who.target.clone() else {
            return Err(self.not_independent(&before, "no fixer could be resolved".into()));
        };
        let earlier = super::rows_of(&self.core.store, &self.tree.branch, self.point)
            .ok()
            .flatten();
        if earlier.is_none_or(|rows| rows.iter().any(|r| r.kind == "self.fix.started")) {
            let why = format!(
                "a fix {} of this branch was tried already (or the record cannot show it was \
                 not): a branch is fixed once at each point",
                self.point.words()
            );
            return Err(self.refused(
                &before,
                &Refusal {
                    bound: "second",
                    why,
                },
            ));
        }
        let m = self
            .moved(
                &before,
                format!(
                    "{} given to a fixer {}",
                    self.tree.branch,
                    self.point.words()
                ),
            )
            .with("fixer", who.name.clone())
            .why(self.fault.clone());
        self.core.rec(None).record(&Started(m));
        let snap = State::of(self.place, self.tree).map_err(|e| format!("{e:#}"))?;
        let ask = self.ask(&before, target).await?;
        let f = match self.fixer.fix(self.core, &ask).await {
            Ok(f) => f,
            Err(e) => {
                self.restore(&before);
                let why = format!("the fixer did not answer: {e:#}");
                return Err(self.none(&before, &who.name, None, &why));
            }
        };
        let builder = who.builder.as_deref().unwrap_or_default();
        self.answered(before, builder, &snap, f).await
    }

    /// What the fixer answered, judged: its commit within its bounds, or the words of why not.
    async fn answered(
        &self,
        before: String,
        builder: &str,
        snap: &State,
        f: super::Fixed,
    ) -> Result<Made, String> {
        let name = match f.profile.as_str() {
            "" => f.model.clone(),
            p => format!("{p} ({})", f.model),
        };
        if let Some(why) = not_independent(builder, &f.model) {
            self.restore(&before);
            return Err(self.refused(
                &before,
                &Refusal {
                    bound: "not-independent",
                    why,
                },
            ));
        }
        if !f.done {
            self.restore(&before);
            return Err(self.none(&before, &name, f.session.as_deref(), &f.why));
        }
        let now = State::of(self.place, self.tree).map_err(|e| format!("{e:#}"))?;
        if let Some(r) = snap.moved(&now) {
            self.restore(&before);
            return Err(self.refused(&before, &r));
        }
        let head = match self.commit(&f.model, &f.why).await {
            Ok(Some(h)) => h,
            Ok(None) => {
                let why = "the fixer said done and changed nothing".to_string();
                return Err(self.refused(
                    &before,
                    &Refusal {
                        bound: "empty",
                        why,
                    },
                ));
            }
            Err(e) => {
                self.restore(&before);
                let why = format!("its change could not be committed: {e:#}");
                return Err(self.none(&before, &name, f.session.as_deref(), &why));
            }
        };
        let change = self.bounded(&before, &head).await?;
        let made = Made {
            before,
            head,
            lines: change.lines(),
            files: change.files.len() as u64,
            fixer: name,
            fixer_model: f.model,
            reason: f.why,
            session: f.session,
        };
        let m = self
            .moved(
                &made.head,
                format!(
                    "{} fixed {} by {}: {} lines in {} files",
                    self.tree.branch,
                    self.point.words(),
                    made.fixer,
                    made.lines,
                    made.files
                ),
            )
            .with("fixer", made.fixer.clone())
            .with("lines", made.lines)
            .with("files", made.files)
            .with("from", made.before.clone())
            .with("session", made.session.clone())
            .why(made.reason.clone())
            .numbers(serde_json::json!({"lines": made.lines, "files": made.files,
                "cost_usd": f.cost_usd}));
        self.core.rec(None).record(&MadeRow(m));
        Ok(made)
    }

    /// What the fixer is given.
    async fn ask(&self, before: &str, target: crate::turn::Target) -> Result<FixAsk, String> {
        let (p, main, h) = (self.place.clone(), self.main.clone(), before.to_string());
        let diff = blocking(move || git::diff(&p, &main, &h, 3))
            .await
            .map_err(|e| format!("the branch's diff could not be read: {e:#}"))?;
        let evidence = crate::task_graph::get(&self.core.store, self.item)
            .ok()
            .flatten()
            .map(|t| {
                let mut e = format!("{}\n\n{}\n", t.title, t.objective);
                for ev in &t.evidence {
                    let note = ev.note.as_deref().map(|n| format!(": {n}"));
                    e.push_str(&format!("- {}{}\n", ev.identity, note.unwrap_or_default()));
                }
                e
            })
            .unwrap_or_else(|| format!("(item {} is not on the backlog)", self.item));
        let cfg = &self.core.cfg.self_fix;
        Ok(FixAsk {
            item: self.item.to_string(),
            branch: self.tree.branch.clone(),
            head: before.to_string(),
            point: self.point,
            tree: self.tree.dir.display().to_string(),
            slug: self.tree.slug.clone(),
            diff,
            failure: self.failure.clone(),
            evidence,
            target,
            lines: cfg.lines,
            files: cfg.files,
        })
    }

    /// The code's one commit of what the fixer left: formatted first, as a build's is.
    async fn commit(&self, model: &str, why: &str) -> anyhow::Result<Option<String>> {
        let subject = format!(
            "self: fix {} by {model} for {} ({})",
            self.point.words(),
            self.fault,
            self.item
        );
        let subject = crate::scrub::safe::head(&subject, 120).0.to_string();
        let body = format!(
            "{}\n\nFixed {} by {model}, for: {}\nItem: {}\n\n{} {}\nCo-Authored-By: Theseus \
             (self) <noreply@anthropic.com>",
            why.trim(),
            self.point.words(),
            self.fault,
            self.item,
            super::TRAILER,
            self.point.name()
        );
        let (p, t) = (self.place.clone(), self.tree.clone());
        blocking(move || {
            let body = match crate::rsi::tree_fmt::format_tree(&p, &t).line() {
                Some(line) => format!("{body}\n{line}"),
                None => body,
            };
            tree::commit(&p, &t, &subject, &body)
        })
        .await
    }

    /// The bounds on the commit `head` against `before`: its change, or the refusal's words.
    async fn bounded(&self, before: &str, head: &str) -> Result<Change, String> {
        let cfg = &self.core.cfg.self_fix;
        let (lines, files) = (cfg.lines, cfg.files);
        let (p, dir, main) = (self.place.clone(), self.tree.dir.clone(), self.main.clone());
        let (b, h) = (before.to_string(), head.to_string());
        let judged = blocking(move || {
            let c = Change::of(&p, &dir, &b, &h)?;
            let base = git::merge_base(&p, &main, &b)?;
            let refusal = bounds::size(&c, lines, files)
                .or_else(|| bounds::guarded(&c))
                .or_else(|| bounds::tests(&p, &dir, &base, &b, &c))
                .or_else(|| bounds::guard(&p, &dir, &main, &b, &h));
            Ok((c, refusal))
        })
        .await;
        match judged {
            Ok((c, None)) => Ok(c),
            Ok((_, Some(r))) => Err(self.refused_from(head, Some(before), &r)),
            Err(e) => Err(self.refused_from(
                head,
                Some(before),
                &Refusal {
                    bound: "unread",
                    why: format!("the fix's commit could not be read: {e:#}"),
                },
            )),
        }
    }

    /// The gate was red again on the fix: its row and its words.
    pub(crate) fn red_again(&self, m: &Made, tail: &str) -> String {
        let last = tail
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        self.none(
            &m.head,
            &m.fixer,
            m.session.as_deref(),
            &format!("the gate was red again after the fix ({})", last.trim()),
        )
    }

    /// The builder's profile, with the model it ran on: a profile renamed since is the live
    /// profile's settings with that model, which is the model that must approve. Refused, with its
    /// row, when the builder is not known or cannot run.
    fn approver(&self, m: &Made) -> Result<crate::turn::Target, String> {
        let refused = |why: String| {
            let r = Refusal {
                bound: "not-independent",
                why,
            };
            self.refused(&m.head, &r)
        };
        let Some(built) = builder_target(self.core, self.builder) else {
            return Err(refused(
                "the model that built the branch is not known, so nobody can approve the fix"
                    .into(),
            ));
        };
        let live = self.core.live_profile().0;
        let runner = &self.core.runner;
        let model = (Some(built.provider.as_str()), Some(built.model.as_str()));
        runner
            .resolve_target(&live, Some(&built.profile), model.0, model.1)
            .or_else(|_| runner.resolve_target(&live, None, model.0, model.1))
            .map_err(|e| {
                refused(format!(
                    "the builder's profile {} cannot run: {e:#}",
                    built.profile
                ))
            })
    }

    /// The builder's profile reads the fix's commit alone: who accepted it, or the words of why
    /// it did not hold.
    pub(crate) async fn approve(&self, m: &Made) -> Result<String, String> {
        let target = self.approver(m)?;
        let (p, dir) = (self.place.clone(), self.tree.dir.clone());
        let range = format!("{}..{}", m.before, m.head);
        let diff = blocking(move || p.git_ok(&dir, &["diff", "--no-color", "-U3", &range]))
            .await
            .unwrap_or_default();
        let ask = ApproveAsk {
            item: self.item.to_string(),
            branch: self.tree.branch.clone(),
            head: m.head.clone(),
            point: self.point,
            diff,
            reason: m.reason.clone(),
            failure: self.failure.clone(),
            fixer: m.fixer.clone(),
            target,
        };
        let r = self.fixer.approve(self.core, &ask).await;
        let (verdict, approver, reasons, cost) = match r {
            Ok(r) => {
                if let Some(why) = not_independent(&m.fixer_model, &r.model) {
                    return Err(self.refused(
                        &m.head,
                        &Refusal {
                            bound: "not-independent",
                            why,
                        },
                    ));
                }
                let name = match r.profile.as_str() {
                    "" => r.model.clone(),
                    p => format!("{p} ({})", r.model),
                };
                let v = if r.accept { "accept" } else { "reject" };
                (v, name, r.reasons, r.cost_usd)
            }
            Err(e) => (
                "reject",
                ask.target.profile.clone(),
                vec![format!(
                    "the model that built the branch did not answer: {e:#}"
                )],
                0.0,
            ),
        };
        let m2 = self
            .moved(
                &m.head,
                format!(
                    "the fix of {} {} {}ed by {approver}",
                    self.tree.branch,
                    self.point.words(),
                    verdict
                ),
            )
            .with("fixer", m.fixer.clone())
            .with("approver", approver.clone())
            .with("verdict", verdict)
            .with("failure", self.fault.clone())
            .with("reasons", reasons.clone())
            .why(reasons.first().cloned().unwrap_or_default())
            .numbers(serde_json::json!({"cost_usd": cost, "lines": m.lines, "files": m.files}));
        self.core.rec(None).record(&Verdict(m2));
        match verdict {
            "accept" => Ok(approver),
            _ => Err(format!(
                "a fix was made {} by {} and the model that built the branch, {approver}, \
                 rejected it: {}",
                self.point.words(),
                m.fixer,
                reasons.join("; ")
            )),
        }
    }
}
