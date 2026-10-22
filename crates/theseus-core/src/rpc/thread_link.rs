//! `thread.link` and `thread.unlink` (theseus-98bz.11): the owner's word that two threads belong
//! together. A link makes each thread's notes reach the other's recall with no distance
//! (`ThreadScene` takes a thread's links into `waived`); it never crosses a seal. Threads are the
//! owner's, so each is judged as the other thread acts are, and no model tool reaches them.
//!
//! One frame holds both records and the `thread.linked` or `thread.unlinked` row. A link that
//! holds, and an unlink of one that is absent, write nothing.

use std::collections::BTreeSet;

use anyhow::{anyhow, bail, Result};
use theseus_protocol::method;
use theseus_protocol::threads::{ThreadInfo, ThreadLinkParams, ThreadRow};

use super::server::{route, Conn, RpcFailure};
use super::thread_join::no_thread;
use super::Core;
use crate::approval::Answerer;
use crate::fact;
use crate::fact::thread::{ThreadLinked, ThreadUnlinked};
use crate::threads::Board;

/// The most threads one thread may be linked with.
const MAX_LINKS: usize = 8;

impl Core {
    /// The two methods, each as the connection's answerer.
    pub(super) fn rpc_thread_links(
        &self,
        name: &str,
        params: serde_json::Value,
        conn: Conn<'_>,
    ) -> Result<serde_json::Value, RpcFailure> {
        route(params, |p: ThreadLinkParams| {
            let who = conn.answerer(p.author.clone(), p.discord.clone());
            if name == method::THREAD_LINK {
                self.thread_link(&p, who)
            } else {
                self.thread_unlink(&p, who)
            }
        })
    }

    /// `thread.link`: `slug` and `other` hold each other in their `links`.
    pub fn thread_link(
        &self,
        p: &ThreadLinkParams,
        who: impl Into<Answerer>,
    ) -> Result<ThreadRow, RpcFailure> {
        self.link_change(p, who.into(), method::THREAD_LINK)
    }

    /// `thread.unlink`: neither holds the other, whatever the seal (it only tightens).
    pub fn thread_unlink(
        &self,
        p: &ThreadLinkParams,
        who: impl Into<Answerer>,
    ) -> Result<ThreadRow, RpcFailure> {
        self.link_change(p, who.into(), method::THREAD_UNLINK)
    }

    fn link_change(
        &self,
        p: &ThreadLinkParams,
        who: Answerer,
        method: &'static str,
    ) -> Result<ThreadRow, RpcFailure> {
        self.judge_thread_pair(&who, method, &p.slug, &p.other)?;
        self.threads_on()?;
        let linking = method == method::THREAD_LINK;
        let (a, b) = (p.slug.trim().to_string(), p.other.trim().to_string());
        if a == b {
            return Err(RpcFailure::invalid(anyhow!(
                "a thread cannot be linked to itself"
            )));
        }
        let (who_s, via) = (who.who(), who.via());
        let written = self
            .runner
            .threads
            .write(
                &self.store,
                |board| {
                    let (ta, tb) = (
                        board.get(&a).ok_or_else(|| no_thread(&a))?,
                        board.get(&b).ok_or_else(|| no_thread(&b))?,
                    );
                    if linking {
                        link_ends(ta, tb)
                    } else {
                        Ok(unlink_ends(board, ta, tb))
                    }
                },
                |_, ts| {
                    let (x, y) = (&ts[0], &ts[1]);
                    Ok(vec![if linking {
                        fact::row(
                            &ThreadLinked {
                                thread: x,
                                other: &y.slug,
                                who: &who_s,
                                via: &via,
                            },
                            None,
                            None,
                        )?
                    } else {
                        fact::row(
                            &ThreadUnlinked {
                                thread: x,
                                other: &y.slug,
                                who: &who_s,
                                via: &via,
                            },
                            None,
                            None,
                        )?
                    }])
                },
            )
            .map_err(RpcFailure::invalid)?;
        let Some(first) = written.first().cloned() else {
            // Nothing differed: the thread as it is, and no frame.
            let t = self.thread_named(&a)?;
            return Ok(self.thread_rows(&mut [t]).pop().expect("one row"));
        };
        for (t, other) in [(&written[0], &written[1]), (&written[1], &written[0])] {
            let other = other.slug.as_str();
            if linking {
                self.announce_thread(
                    &t.slug,
                    &ThreadLinked {
                        thread: t,
                        other,
                        who: &who_s,
                        via: &via,
                    },
                );
            } else {
                self.announce_thread(
                    &t.slug,
                    &ThreadUnlinked {
                        thread: t,
                        other,
                        who: &who_s,
                        via: &via,
                    },
                );
            }
        }
        Ok(self.thread_rows(&mut [first]).pop().expect("one row"))
    }
}

/// Both records with each holding the other, or none when they already do. A merged thread, a
/// sealed one and one whose seal is proposed refuse: a seal's notes never reach another thread.
fn link_ends(a: &ThreadInfo, b: &ThreadInfo) -> Result<Vec<ThreadInfo>> {
    for (t, o) in [(a, b), (b, a)] {
        if let Some(m) = &t.merged_into {
            bail!("the thread {} was merged into {m}: link that one", t.slug);
        }
        if t.sealed.is_some() {
            bail!(
                "the thread {} is sealed: a sealed thread's notes never reach another thread, so it cannot be linked",
                t.slug
            );
        }
        if t.seal_pending.is_some() {
            bail!(
                "a seal is proposed for the thread {} and waits for the owner: a sealed thread's notes never reach another thread, so it cannot be linked until the owner answers",
                t.slug
            );
        }
        if !t.links.contains(&o.slug) && t.links.len() >= MAX_LINKS {
            bail!(
                "the thread {} holds {MAX_LINKS} links already: `theseus thread unlink` one first",
                t.slug
            );
        }
    }
    if a.links.contains(&b.slug) && b.links.contains(&a.slug) {
        return Ok(vec![]);
    }
    Ok(vec![with_link(a, &b.slug), with_link(b, &a.slug)])
}

fn unlink_ends(board: &Board, a: &ThreadInfo, b: &ThreadInfo) -> Vec<ThreadInfo> {
    let pair = match a.links.contains(&b.slug) || b.links.contains(&a.slug) {
        true => Some((a, b)),
        false => merged_pair(board, a, b),
    };
    pair.map_or_else(Vec::new, |(x, y)| {
        vec![without_link(x, &y.slug), without_link(y, &x.slug)]
    })
}

/// The pair a link really sits on, when `b` (or `a`) was merged into the
/// thread named: a link of `a` to a thread whose live end is `b`, or the
/// other way round (theseus-t8fow).
fn merged_pair<'a>(
    board: &'a Board,
    a: &'a ThreadInfo,
    b: &'a ThreadInfo,
) -> Option<(&'a ThreadInfo, &'a ThreadInfo)> {
    let through = |t: &'a ThreadInfo, live: &str| {
        t.links
            .iter()
            .find(|l| board.live_of(l).is_some_and(|e| e.slug == live))
            .and_then(|l| board.get(l))
    };
    through(a, &b.slug)
        .map(|m| (a, m))
        .or_else(|| through(b, &a.slug).map(|m| (m, b)))
}

fn with_link(t: &ThreadInfo, other: &str) -> ThreadInfo {
    let links: BTreeSet<String> = t.links.iter().cloned().chain([other.to_string()]).collect();
    ThreadInfo {
        links: links.into_iter().collect(),
        ..t.clone()
    }
}

fn without_link(t: &ThreadInfo, other: &str) -> ThreadInfo {
    ThreadInfo {
        links: t.links.iter().filter(|l| *l != other).cloned().collect(),
        ..t.clone()
    }
}
