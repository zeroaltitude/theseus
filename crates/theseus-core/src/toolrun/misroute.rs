//! The misroute guard (theseus-rha2). A message the router placed in a thread
//! below its act bar (a visible tag, a guess) may have gone to the wrong one,
//! and a turn might then write into the wrong working set. So the thread's
//! session is armed when the turn is placed, and the turn's **first write or
//! program** is looked at once: when a path it names is outside every root of
//! the thread's working set, that one call waits for the owner, at the
//! approve posture, with the question "This edits ~/projects/x, which isn't
//! in the pr round thread. Go on, or move to the security fix?". The guard
//! then lets go: a call inside the roots shows the placement was right, and
//! an answered one was the owner's to give. A turn placed at or above the act
//! bar, an addressed one, and a thread with no roots are never armed.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use theseus_protocol::{Access, Plan};
use theseus_tools::Tool;

use super::ToolRuntime;
use super::TurnCtx;
use crate::policy::{Decision, Posture};

/// What the guard knows of the thread a turn was placed in.
#[derive(Debug, Clone)]
pub struct Armed {
    pub title: String,
    pub roots: Vec<PathBuf>,
    /// The thread the router thought next likeliest, if any: the question
    /// offers a move to it.
    pub alt: Option<String>,
}

/// A call the guard holds: the path and what the call does to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub path: String,
    pub verb: &'static str,
    pub thread: String,
    pub alt: Option<String>,
}

impl Held {
    /// The question the card asks.
    pub fn why(&self) -> String {
        let go = match &self.alt {
            Some(alt) => format!("Go on, or move to the {alt}?"),
            None => "Go on, or move it?".to_string(),
        };
        format!(
            "This {} {}, which isn't in the {} thread. {go}",
            self.verb, self.path, self.thread
        )
    }
}

/// The armed sessions.
#[derive(Default)]
pub struct Guards(Mutex<HashMap<String, Armed>>);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// `path` with `~` expanded and `.`/`..` folded, no filesystem read.
pub(crate) fn normal(path: &str, home: Option<&Path>) -> PathBuf {
    let expanded = match (path.strip_prefix("~/"), path == "~", home) {
        (Some(rest), _, Some(h)) => h.join(rest),
        (_, true, Some(h)) => h.to_path_buf(),
        _ => PathBuf::from(path),
    };
    fold(&expanded)
}

pub(crate) fn fold(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

impl Guards {
    /// A turn was placed in `session`'s thread below the act bar: its first
    /// write or program is looked at. A thread with no roots arms nothing.
    pub fn arm(&self, session: &str, title: &str, roots: &[String], alt: Option<&str>) {
        if roots.is_empty() {
            self.disarm(session);
            return;
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        lock(&self.0).insert(
            session.to_string(),
            Armed {
                title: title.to_string(),
                roots: roots.iter().map(|r| normal(r, home.as_deref())).collect(),
                alt: alt.map(str::to_string),
            },
        );
    }

    pub fn disarm(&self, session: &str) {
        lock(&self.0).remove(session);
    }

    pub fn armed(&self, session: &str) -> bool {
        lock(&self.0).contains_key(session)
    }

    /// The call `plan` makes, in `session`: the guard lets go at the first
    /// write or program, and says what it holds when a path is outside every
    /// root. A read never counts, nor a call that names no path.
    pub fn check(&self, session: &str, plan: &Plan) -> Option<Held> {
        let mut g = lock(&self.0);
        let acts: Vec<&theseus_protocol::Resource> = plan
            .resources
            .iter()
            .filter(|r| matches!(r.access, Access::Write | Access::Exec))
            .collect();
        if acts.is_empty() {
            return None;
        }
        let armed = g.remove(session)?;
        acts.into_iter().find_map(|r| {
            let p = fold(&r.path);
            let inside = armed.roots.iter().any(|root| p.starts_with(root));
            (!inside).then(|| Held {
                path: crate::places::home_shown(&p),
                verb: match r.access {
                    Access::Exec => "runs a program in",
                    _ => "edits",
                },
                thread: armed.title.clone(),
                alt: armed.alt.clone(),
            })
        })
    }
}

impl ToolRuntime {
    /// The gate's decision for a planned call, no looser than the guard's
    /// question when the turn was placed on a guess and this is its first
    /// write or program outside the thread's roots. Nothing for a session the
    /// guard has not armed (one lookup in a map that is empty but for a turn
    /// placed on a guess).
    pub(super) fn misroute_hold(
        &self,
        tc: &TurnCtx<'_>,
        tool: &dyn Tool,
        plan: &Plan,
        d: Decision,
    ) -> Decision {
        let Some(held) = self.misroute.check(tc.session_id, plan) else {
            return d;
        };
        tc.record(&crate::fact::thread_route::MisrouteHeld {
            thread: &held.thread,
            tool: tool.name(),
            path: &held.path,
        });
        d.at_least(
            Posture::Approve,
            &held.why(),
            "the thread misroute guard",
            tool.name(),
            &plan.summary,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::Resource;

    fn plan(access: Access, path: &str) -> Plan {
        Plan {
            resources: vec![Resource {
                path: path.into(),
                access,
            }],
            summary: "x".into(),
            ..Default::default()
        }
    }

    /// The first write or program is looked at once: outside the roots it is
    /// held with the question; inside, or a read, it is not; and the guard
    /// lets go after the first.
    #[test]
    fn the_first_write_outside_the_roots_is_held_once() {
        let g = Guards::default();
        g.arm(
            "ses_a",
            "pr round",
            &["/work/tidewatch".into()],
            Some("security fix"),
        );
        assert_eq!(
            g.check("ses_a", &plan(Access::Read, "/work/vault/a.rs")),
            None,
            "a read is never held"
        );
        assert!(g.armed("ses_a"), "and does not use the guard up");
        let held = g
            .check("ses_a", &plan(Access::Write, "/work/vault/src/a.rs"))
            .unwrap();
        assert_eq!(
            held.why(),
            "This edits /work/vault/src/a.rs, which isn't in the pr round thread. Go on, or move to the security fix?"
        );
        assert_eq!(
            g.check("ses_a", &plan(Access::Write, "/work/vault/src/b.rs")),
            None,
            "once"
        );
        // A call inside the roots lets go too.
        g.arm("ses_b", "pr round", &["/work/tidewatch".into()], None);
        assert_eq!(
            g.check(
                "ses_b",
                &plan(Access::Write, "/work/tidewatch/src/../lib.rs")
            ),
            None
        );
        assert!(!g.armed("ses_b"));
        // A program is a call that acts; a sibling directory is outside.
        g.arm("ses_c", "pr round", &["/work/tidewatch".into()], None);
        let held = g
            .check("ses_c", &plan(Access::Exec, "/work/tidewatch-old"))
            .unwrap();
        assert!(
            held.why()
                .starts_with("This runs a program in /work/tidewatch-old"),
            "{}",
            held.why()
        );
        assert!(held.why().ends_with("Go on, or move it?"));
    }

    #[test]
    fn a_thread_with_no_roots_and_a_session_never_armed_hold_nothing() {
        let g = Guards::default();
        g.arm("ses_a", "pr round", &[], None);
        assert!(!g.armed("ses_a"));
        assert_eq!(g.check("ses_a", &plan(Access::Write, "/anywhere")), None);
        assert_eq!(
            g.check("ses_never", &plan(Access::Write, "/anywhere")),
            None
        );
    }

    #[test]
    fn home_is_expanded_in_a_root_and_shown_in_the_question() {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return;
        };
        let g = Guards::default();
        g.arm("ses_a", "t", &["~/projects/x".into()], None);
        assert_eq!(
            g.check(
                "ses_a",
                &plan(
                    Access::Write,
                    &format!("{}/projects/x/a.rs", home.display())
                )
            ),
            None
        );
        g.arm("ses_a", "t", &["~/projects/x".into()], None);
        let held = g
            .check(
                "ses_a",
                &plan(
                    Access::Write,
                    &format!("{}/projects/y/a.rs", home.display()),
                ),
            )
            .unwrap();
        assert_eq!(held.path, "~/projects/y/a.rs");
    }
}
