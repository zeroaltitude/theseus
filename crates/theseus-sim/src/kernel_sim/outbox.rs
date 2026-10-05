//! kernel-sim's outbox (theseus-q4v, §3.16; theseus-celu.35): posts staged in
//! a turn's end frame (`end_turn_with`) and planned outside a turn
//! (`outbox_plan`), dispatched and settled by a fake binding that sends each
//! under its downstream key to a fake channel, with crashes after each
//! transition: after the stage, after the dispatch before the send, after
//! the send before the settle, and after the settle. Every staged post is
//! delivered once the binding runs; the channel keeps one copy per post; a
//! second settle changes nothing; no execution waits on a post; nothing but
//! the binding touches one (a cancel, a stop, its execution's end, the
//! reconciler).

use std::collections::HashMap;

use anyhow::{bail, Result};
use rand::Rng;
use serde_json::json;
use theseus_kernel::*;

use super::World;

/// The downstream key a post is sent under: its own id, as the Discord
/// binding's nonce is the post's.
fn key_of(post: &Action) -> String {
    post.correlation_id.clone()
}

impl World {
    /// Crashes around a post's transitions are twice as likely as
    /// between two other frames, so every one of them is crossed.
    fn maybe_crash_post(&mut self, where_: &str) -> Result<bool> {
        if !self.chance(self.p.p_crash * 2.0) {
            return Ok(false);
        }
        self.rep.sim2.post_crashes += 1;
        self.crash(where_)?;
        Ok(true)
    }

    fn post_for(&mut self, e: Option<&Execution>, kind: &str) -> Post {
        let n: u32 = self.rng.random_range(0..1000);
        Post {
            session_id: e.map(|e| e.session_id.clone()).unwrap_or_default(),
            execution_id: e.map(|e| e.id.clone()).unwrap_or_default(),
            target: match e {
                Some(_) => "discord:dm:sim".into(),
                None => "discord:operator".into(),
            },
            body: json!({"kind": kind, "text": format!("sim {kind} {n}")}),
            retry_class: RetryClass::IdempotentWithKey {
                key: "discord.nonce".into(),
            },
        }
    }

    /// The binding (or a turn's end) wrote `a`: as the post now is, and the
    /// next post record the WAL scan must find.
    fn post_wrote(&mut self, a: Action) {
        self.s2.posts.insert(a.correlation_id.clone(), a.clone());
        self.s2.post_writes.push_back(a);
    }

    /// A post record, as the check's WAL scan reads it: the next one the sim
    /// wrote, and no other. A cancel, a stop, an execution's end, or the
    /// reconciler that wrote a post would be found here.
    pub(super) fn post_record(&mut self, at: &str, r: &theseus_store::Record) -> Result<()> {
        let got: Action = r.decode()?;
        match self.s2.post_writes.pop_front() {
            Some(want) if json!(want) == json!(got) => Ok(()),
            want => bail!(
                "{at}: post {} was written at {} outside its binding: {got:?}, the binding wrote {want:?}",
                got.correlation_id,
                r.position
            ),
        }
    }

    /// End the turn `g` holds, and two times in five stage its reply in the
    /// frame that ends it, as the core's turn does. A crash may come after.
    pub(super) fn end_turn_posting(&mut self, g: TurnGuard, end: TurnEnd) -> Result<Execution> {
        if !self.chance(0.4) {
            return self.kernel.end_turn(g, end);
        }
        let e = self.kernel.execution(&g.execution_id)?.unwrap();
        let post = self.post_for(Some(&e), "reply");
        let mut staged = None;
        let k = &self.kernel;
        let e = k.end_turn_with(g, end, |_| {
            let (a, records) = k.outbox_stage(post)?;
            staged = Some(a);
            Ok(records)
        })?;
        if let Some(a) = staged {
            self.rep.sim2.posts_staged += 1;
            self.post_wrote(a);
            // The crash comes after the frame, so the caller's turn has ended.
            self.maybe_crash_post("after a post's stage")?;
        }
        Ok(e)
    }

    /// The binding's turn: now and then a notice planned outside any turn
    /// (about an execution, open or ended, or about the daemon), and then one
    /// open post delivered.
    pub(super) fn run_binding(&mut self) -> Result<bool> {
        if self.chance(0.3) {
            let about = if self.execs.is_empty() || self.chance(0.2) {
                None
            } else {
                let id = self.execs[self.rng.random_range(0..self.execs.len())].clone();
                self.kernel.execution(&id)?
            };
            let post = self.post_for(about.as_ref(), "notice");
            let a = self.kernel.outbox_plan(post)?;
            self.rep.sim2.posts_planned += 1;
            self.post_wrote(a);
            if self.maybe_crash_post("after a post's plan")? {
                return Ok(true);
            }
        }
        let open: Vec<String> = self
            .s2
            .posts
            .iter()
            .filter(|(_, a)| !a.state.is_settled())
            .map(|(c, _)| c.clone())
            .collect();
        if open.is_empty() {
            return Ok(false);
        }
        let corr = open[self.rng.random_range(0..open.len())].clone();
        self.deliver_post(&corr, true)
    }

    /// The binding delivers one post: dispatched (or found dispatched by an
    /// earlier attempt, which writes nothing), sent under its key, settled
    /// with the channel's answer, and now and then settled a second time,
    /// which changes nothing. Returns whether a crash stopped it.
    fn deliver_post(&mut self, corr: &str, crashes: bool) -> Result<bool> {
        let before = self.s2.posts[corr].clone();
        let at = self.kernel.store().last_position();
        let d = self.kernel.outbox_dispatch(corr)?;
        match before.state {
            ActionState::Authorized => {
                if d.state != ActionState::Dispatched || d.dispatched_at_ms != Some(self.now()) {
                    bail!(
                        "post {corr} dispatched as {:?} at {:?}",
                        d.state,
                        d.dispatched_at_ms
                    );
                }
            }
            _ if d.dispatched_at_ms != before.dispatched_at_ms
                || self.kernel.store().last_position() != at =>
            {
                bail!("post {corr}, dispatched before, was dispatched again: {d:?}");
            }
            _ => {}
        }
        if before.state == ActionState::Authorized {
            self.post_wrote(d.clone());
        }
        if crashes && self.maybe_crash_post("after a post's dispatch, before its send")? {
            return Ok(true);
        }
        // The channel refuses a post now and then: a refusal no retry can
        // change, so only on its first send.
        let key = key_of(&d);
        let first = !self.s2.channel.messages.iter().any(|(k, _, _)| k == &key);
        let refused = first && self.chance(0.1);
        let sent = if refused {
            self.rep.sim2.posts_refused += 1;
            None
        } else {
            let (id, new) = self.s2.channel.send(&key, corr);
            self.rep.sim2.posts_sent += u64::from(new);
            self.rep.sim2.posts_resent += u64::from(!new);
            Some(id)
        };
        if crashes && self.maybe_crash_post("after a post's send, before its settle")? {
            return Ok(true);
        }
        let done = |id: Option<&str>, at: u64| Completion {
            correlation_id: corr.to_string(),
            outcome: if id.is_some() {
                Outcome::Succeeded
            } else {
                Outcome::Failed
            },
            result_ref: None,
            external_op_id: id.map(str::to_string),
            started_at_ms: at,
            finished_at_ms: at,
            producer: "sim-binding".into(),
            signature: None,
            cost_micros: None,
            detail: Some(json!({"messages": id.map(|id| vec![json!({"key": "reply", "id": id})])})),
        };
        let settled = match self
            .kernel
            .outbox_settle(&done(sent.as_deref(), self.now()))?
        {
            Settled::Now(a) => a,
            Settled::Already(a) => bail!("post {corr}, open, settled as settled before: {a:?}"),
        };
        if settled.external_op_id != sent {
            bail!(
                "post {corr} settled with {:?}, sent as {sent:?}",
                settled.external_op_id
            );
        }
        self.rep.sim2.posts_settled += 1;
        self.post_wrote(settled.clone());
        if crashes && self.maybe_crash_post("after a post's settle")? {
            return Ok(true);
        }
        if self.chance(0.3) {
            let at = self.kernel.store().last_position();
            match self
                .kernel
                .outbox_settle(&done(Some("msg_again"), self.now() + 1))?
            {
                Settled::Already(a) if json!(a) == json!(settled) => {}
                other => bail!("a second settle of post {corr} changed it: {other:?}"),
            }
            if self.kernel.store().last_position() != at {
                bail!("a second settle of post {corr} wrote");
            }
            self.rep.sim2.second_settles += 1;
        }
        Ok(false)
    }

    /// At the end, the binding runs until no post is open: every post the
    /// sim wrote is settled, and each one sent is in the channel once.
    pub(super) fn deliver_every_post(&mut self) -> Result<()> {
        let open: Vec<String> = self
            .s2
            .posts
            .iter()
            .filter(|(_, a)| !a.state.is_settled())
            .map(|(c, _)| c.clone())
            .collect();
        for corr in open {
            self.deliver_post(&corr, false)?;
        }
        for (corr, a) in &self.s2.posts {
            let copies = self.s2.channel.copies(corr);
            match (a.state, copies.as_slice()) {
                (ActionState::Succeeded, [one]) if a.external_op_id.as_deref() == Some(*one) => {}
                (ActionState::Failed, []) => {}
                (state, copies) => {
                    bail!("after the binding ran, post {corr} is {state:?} with copies {copies:?}")
                }
            }
        }
        Ok(())
    }

    /// Every post, at every check: each post record the WAL gained since the
    /// last check is one the binding wrote (`post_record`); none among the
    /// kernel's actions; none an execution waits on; at most one copy in the
    /// channel, the one its settle names. Every twentieth check, and at the
    /// end, each post is read back: there, as the binding last left it, and
    /// no stray post.
    pub(super) fn check_outbox(
        &self,
        at: &str,
        execs: &[Execution],
        actions: &[Action],
    ) -> Result<()> {
        if let Some(a) = self.s2.post_writes.front() {
            bail!("{at}: the WAL holds no record of the binding's write {a:?}");
        }
        let posts = &self.s2.posts;
        if let Some(a) = actions
            .iter()
            .find(|a| a.tool == OUTBOX_TOOL || posts.contains_key(&a.correlation_id))
        {
            bail!(
                "{at}: post {} is among the kernel's actions",
                a.correlation_id
            );
        }
        for e in execs {
            let waits = match &e.wake {
                Some(Wake::Actions { correlation_ids }) => correlation_ids.clone(),
                _ => vec![],
            };
            if let Some(c) = e
                .outstanding
                .iter()
                .chain(&e.queued_results)
                .chain(&waits)
                .find(|c| posts.contains_key(*c))
            {
                bail!("{at}: {} waits on post {c}", e.id);
            }
        }
        let mut copies: HashMap<&str, Vec<&str>> = HashMap::new();
        for (_, post, id) in &self.s2.channel.messages {
            copies.entry(post.as_str()).or_default().push(id.as_str());
        }
        for (corr, a) in posts {
            let copies = copies.get(corr.as_str()).map(Vec::as_slice).unwrap_or(&[]);
            if copies.len() > 1 {
                bail!(
                    "{at}: post {corr} is in the channel {} times: {copies:?}",
                    copies.len()
                );
            }
            if a.state == ActionState::Succeeded
                && a.external_op_id.as_deref() != copies.first().copied()
            {
                bail!(
                    "{at}: post {corr} settled as {:?}, the channel has {copies:?}",
                    a.external_op_id
                );
            }
        }
        if self.rep.invariant_checks.is_multiple_of(20) || at == "after quiesce" {
            self.read_back_posts(at)?;
        }
        Ok(())
    }

    /// Each post the sim wrote, read back from the store: as the binding
    /// last left it, and no other.
    fn read_back_posts(&self, at: &str) -> Result<()> {
        let stored: HashMap<String, Action> = self
            .kernel
            .outbox_actions()?
            .into_iter()
            .map(|a| (a.correlation_id.clone(), a))
            .collect();
        if stored.len() != self.s2.posts.len() {
            bail!(
                "{at}: {} posts in the store, and the sim wrote {}",
                stored.len(),
                self.s2.posts.len()
            );
        }
        for (corr, want) in &self.s2.posts {
            match stored.get(corr) {
                None => bail!("{at}: post {corr} is gone"),
                Some(got) if json!(got) != json!(want) => {
                    bail!("{at}: post {corr} changed outside its binding: {want:?} -> {got:?}")
                }
                Some(_) => {}
            }
        }
        Ok(())
    }
}
