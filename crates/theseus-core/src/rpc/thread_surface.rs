//! What a thread is shown on, and what the Discord binding hears of it
//! (theseus-sjzm): the surfaces a thread records, the one write that changes
//! them without a person to judge (the binding's own: it creates a Discord
//! thread for an active thread, and binds a channel from its config), and the
//! `thread.changed` notification every thread fact sends.
//!
//! A person's change of a surface (`/thread bind`, `thread.edit { surfaces }`)
//! goes through `thread_edit`, judged as every thread write is. The binding's
//! (`Core::thread_surfaces`) is a system act on the owner's behalf, and says
//! so as the actor it writes.

use anyhow::{anyhow, bail};
use theseus_protocol::threads::{place_of, surface_channel, ThreadInfo};

use super::server::RpcFailure;
use super::Core;
use crate::fact;
use crate::fact::thread::{Change, ThreadEdited};

/// The most surfaces one thread is shown on.
pub(super) const MAX_SURFACES: usize = 8;

/// A surface string is `cli` or `discord:channel:<id>`.
pub(super) fn surfaces_of(list: &[String]) -> Result<Vec<String>, RpcFailure> {
    let mut out: Vec<String> = Vec::new();
    for s in list.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if s != "cli" && surface_channel(s).is_none() {
            return Err(RpcFailure::invalid(anyhow!(
                "{s:?} is not a surface: `cli` or `discord:channel:<id>`"
            )));
        }
        if !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    }
    if out.len() > MAX_SURFACES {
        return Err(RpcFailure::invalid(anyhow!(
            "a thread is shown on at most {MAX_SURFACES} surfaces"
        )));
    }
    Ok(out)
}

impl Core {
    /// Announce a thread fact to every connection that hears the wide
    /// notifications (`executions.watch`): `thread.changed`.
    pub(super) fn announce_thread<F: fact::Fact>(&self, slug: &str, f: &F) {
        let place = place_of(slug);
        fact::Rec {
            narrator: &self.narrator,
            session: None,
            turn: None,
            to: fact::To::Session(&self.bus, &place),
            store: &self.store,
        }
        .announce(f);
    }

    /// Set a thread's surfaces: `add` joins and `remove` leaves, one edit and
    /// one frame like `thread.edit`'s. The Discord binding's own act, not a
    /// person's (no judge): it shows an active thread on a Discord thread it
    /// made, or on a channel the config binds. `who` is the actor written.
    pub fn thread_surfaces(
        &self,
        slug: &str,
        add: Option<&str>,
        remove: Option<&str>,
        who: &str,
    ) -> anyhow::Result<ThreadInfo> {
        let changed = ["surfaces"];
        let written = self.runner.threads.write(
            &self.store,
            |board| {
                let cur = board.get(slug).ok_or_else(|| anyhow!("no thread {slug}"))?;
                let mut next = cur.clone();
                next.surfaces.retain(|s| Some(s.as_str()) != remove);
                if let Some(a) = add.filter(|a| !next.surfaces.iter().any(|s| s == a)) {
                    if next.surfaces.len() >= MAX_SURFACES {
                        bail!("a thread is shown on at most {MAX_SURFACES} surfaces");
                    }
                    next.surfaces.push(a.to_string());
                }
                Ok(match next.surfaces == cur.surfaces {
                    true => vec![],
                    false => vec![next],
                })
            },
            |_, ts| {
                ts.iter()
                    .map(|t| {
                        let c = Change {
                            thread: t,
                            who,
                            via: "discord",
                            changed: &changed,
                        };
                        fact::row(&ThreadEdited(c), None, None)
                    })
                    .collect()
            },
        )?;
        match written.into_iter().next() {
            Some(t) => {
                let c = Change {
                    thread: &t,
                    who,
                    via: "discord",
                    changed: &changed,
                };
                self.announce_thread(slug, &ThreadEdited(c));
                Ok(t)
            }
            None => self
                .threads()
                .get(slug)
                .cloned()
                .ok_or_else(|| anyhow!("no thread {slug}")),
        }
    }
}
