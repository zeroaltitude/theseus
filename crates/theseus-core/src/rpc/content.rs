//! Removing a content label (theseus-9ymz0, design §6.3): the operator clears
//! one flag from a session (Discord's `/trust` is this removal for `injection`). A label is raised
//! by what entered the session and removed only by the operator, so a session
//! that carries `injection` stays frozen (under `injection = "refuse"`), and
//! one that carries `secret`, `confidential` or `personal` stays marked, until
//! this.
//!
//! It loosens, so it is judged as an answer is (`judge_act(Act::Unlabel)`):
//! only from the owner, from a private place, never from a job's process. It
//! is `Act::Unlabel` and not `Act::Trust` because a trust's refusal row, its
//! method and its words are about a hold on external text; this refusal is
//! the session's row naming `content.unlabel` and the flag. Every other part
//! is `trust.rs`'s: the record and the `content.removed` row in one frame
//! under the session's lock, a sentence in the narrative, and the event to the
//! session's watchers. Removing a flag the session does not carry is an error
//! and writes nothing.

use anyhow::{anyhow, bail, Result};
use serde_json::json;
use theseus_protocol::{
    error_code, ContentUnlabelParams, Event, LabelRaised, LabelView, Message, UnlabelResult,
};
use theseus_store::{kinds, NewRecord};

use super::confirms::Act;
use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::content::{Flag, InjectionMode, ItemKind, Label};
use crate::fact::{self, content::ContentRemoved};
use crate::session::SessionRecord;

/// A label as the protocol shows it.
pub(crate) fn label_view(l: &Label) -> LabelView {
    LabelView {
        flag: l.flag.as_str().into(),
        node: l.node.clone(),
        kind: l.kind.as_str().into(),
        source: l.source.clone(),
        confidence: l.confidence,
        band: match l.band {
            theseus_judge::band::Band::Act => "act",
            theseus_judge::band::Band::Confirm => "confirm",
            theseus_judge::band::Band::Escalate => "escalate",
        }
        .into(),
        at_ms: l.at_ms,
        judgment: Some(l.judgment.clone()).filter(|j| !j.is_empty()),
    }
}

/// What entered the session, as the operator is told of it: `a tool result
/// from `http.fetch https://example.com/x``.
fn what_raised(l: &Label) -> String {
    let what = match l.kind {
        ItemKind::OperatorMessage => "your message",
        ItemKind::PersonMessage => "a message from someone else here",
        ItemKind::ToolResult => "a tool result",
        ItemKind::FetchedPage => "a fetched page",
        ItemKind::SearchResult => "a search result",
        ItemKind::TaskBrief => "a task brief",
        ItemKind::TaskReport => "a task report",
        ItemKind::Reply => "my own reply",
    };
    format!("{what} from `{}`", l.source)
}

/// The post kind of the injection flag's notice under `notify`: an FYI the
/// owner's notices place takes (issue 54).
pub const FLAG_FYI: &str = "content_flag";

/// The one message in the session's place when a label is raised (design
/// §6.3). `injection` says the session is holding, and what stops until the
/// operator clears it (under `notify`, **the default since 1.0.6**, it says
/// nothing stops — which is what the operator is told); the three that
/// disqualify a model of restricted origin say so, with no urgency, since
/// nothing else is blocked.
pub(crate) fn label_notice(l: &Label, mode: InjectionMode) -> String {
    let what = what_raised(l);
    match (l.flag, mode) {
        (Flag::Injection, InjectionMode::Refuse) => format!(
            "⚑ **injection** — this session is holding: {what} tried to steer me, so writes, \
             programs and sends are refused until you clear it."
        ),
        (Flag::Injection, InjectionMode::Notify) => format!(
            "⚑ **injection** — {what} tried to steer me. Nothing is refused here \
             (`[content_based_security] injection = \"notify\"`); you can clear the flag."
        ),
        (Flag::Secret, _) => format!("⚑ **secret** — {what} showed the value of a credential."),
        (Flag::Confidential, _) => {
            format!("⚑ **confidential** — {what} showed an organisation's non-public matter.")
        }
        (Flag::Personal, _) => {
            format!("⚑ **personal** — {what} showed a person's private information.")
        }
    }
}

impl Core {
    /// Tell the operator a label was raised, once: `session.labelled` to the
    /// session's watchers, and a `content_label` post in the session's place,
    /// which the Discord binding renders with its one Remove button
    /// (`content:remove:<session>:<flag>`). No post when the session has no
    /// place (a CLI session, a task with no thread): the label shows on every
    /// other surface. The caller is whatever raised the label,
    /// and calls this only when `ContentFlags::raise` returned true, so a
    /// second raise of a flag says nothing.
    ///
    /// An `injection` label holds every granted repository first
    /// (`project_context::change::hold_all`, S31-1's act key), so a write the
    /// session made before its label landed is held as one made after it.
    /// The judge's landing has already written those holds in the label's
    /// own frame; a repository already held is left as it is, so this is a
    /// second try and the hold does not depend on it.
    pub fn tell_label(&self, session_id: &str, l: &Label) -> Result<bool> {
        let rec = self
            .store
            .get_session::<SessionRecord>(session_id)?
            .ok_or_else(|| anyhow!("no session is named {session_id}"))?;
        if l.flag == Flag::Injection {
            let held = crate::project_context::change::hold_all(&self.store, session_id);
            let stored = match held {
                Ok(r) if r.is_empty() => Ok(()),
                Ok(r) => self.store.append(&r).map(drop),
                Err(e) => Err(e),
            };
            if let Err(e) = stored {
                tracing::warn!(error = %format!("{e:#}"), "the granted repositories were not held");
            }
        }
        self.bus.publish(
            session_id,
            &Message::from(Event::SessionLabelled(LabelRaised {
                session_id: session_id.into(),
                label: label_view(l),
            })),
            None,
        );
        let mode = self.cfg.content_based_security.injection;
        let mut body = json!({
            "kind": "content_label",
            "session_id": session_id,
            "flag": l.flag.as_str(),
            "text": label_notice(l, mode),
        });
        // Under `notify` the injection flag refuses nothing, so its notice only informs: an FYI
        // of its own kind, which goes to the owner's notices place when one is bound, with its
        // Remove button. Under `refuse` it needs him and keeps the session's place.
        if (l.flag, mode) == (Flag::Injection, InjectionMode::Notify) {
            let title = rec
                .label
                .clone()
                .filter(|t| !t.is_empty())
                .or(rec.title.clone());
            body["kind"] = json!(FLAG_FYI);
            body["text"] = json!(crate::owner_note::note(
                crate::owner_note::Kind::Fyi,
                "Theseus flagged what it read as trying to steer it",
                &crate::judge::onto::words::in_conversation(title.as_deref()),
                &label_notice(l, mode),
            ));
        }
        let post =
            self.outbox
                .post_for(session_id, rec.execution_id.as_deref().unwrap_or(""), body)?;
        Ok(post.is_some())
    }

    /// Tell the operator that asks about a session's content failed twice
    /// (theseus-gyjyo.7): one `notice` post in the session's place, with the
    /// words the judge wrote. No post when the session has no place; the
    /// `content.unjudged` row is the record either way.
    pub fn tell_unjudged(&self, session_id: &str, text: &str) -> Result<bool> {
        let rec = self
            .store
            .get_session::<SessionRecord>(session_id)?
            .ok_or_else(|| anyhow!("no session is named {session_id}"))?;
        let body = json!({"kind": "notice", "text": text});
        let post =
            self.outbox
                .post_for(session_id, rec.execution_id.as_deref().unwrap_or(""), body)?;
        Ok(post.is_some())
    }

    /// `content.unlabel`: the session no longer carries `flag`. An unknown
    /// session, a word that is not a flag, and a flag the session does not
    /// carry are each an error, and nothing is written.
    pub fn unlabel_session(
        &self,
        session_id: &str,
        flag: &str,
        by: impl Into<Answerer>,
    ) -> Result<UnlabelResult> {
        let who = by.into();
        let flag = Flag::parse(flag).ok_or_else(|| {
            anyhow!("`{flag}` is not a content flag (injection, secret, confidential or personal)")
        })?;
        self.store
            .get_session::<SessionRecord>(session_id)?
            .ok_or_else(|| anyhow!("no session is named {session_id}"))?;
        let what = format!("Removing {} from session {session_id}", flag.as_str());
        self.judge_act(
            &who,
            Act::Unlabel {
                session: session_id,
                what: &what,
            },
        )?;
        let out = self
            .store
            .with_session(session_id, |rec| {
                let Some((r, frame)) = removed(
                    rec,
                    session_id,
                    flag,
                    &who,
                    theseus_protocol::method::CONTENT_UNLABEL,
                    None,
                    (
                        &|judgment: &str| self.judgment_pack(judgment),
                        &|node: &str| crate::item_labels::read(&self.store, node),
                    ),
                )?
                else {
                    return Ok(None);
                };
                self.store.append(&frame)?;
                Ok(Some(r))
            })?
            .flatten();
        let Some(r) = out else {
            bail!(
                "session {session_id} carries no {} label, so there is nothing to remove",
                flag.as_str()
            );
        };
        self.announce_removal(session_id, &r);
        Ok(r)
    }

    /// A removal written: narrated, and published to the session's watchers.
    fn announce_removal(&self, session_id: &str, r: &UnlabelResult) {
        fact::Rec {
            narrator: &self.narrator,
            session: Some(session_id),
            turn: None,
            to: fact::To::Session(&self.bus, session_id),
            store: &self.store,
        }
        .announce(&ContentRemoved { result: r });
    }

    pub(super) fn content_unlabel(
        &self,
        p: ContentUnlabelParams,
        conn: Conn<'_>,
    ) -> Result<UnlabelResult, RpcFailure> {
        // A removal names the surface or the person, as a trust does:
        // `Conn::answerer`.
        let who = conn.answerer(p.author, p.discord);
        self.unlabel_session(&p.session_id, &p.flag, who)
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: format!(
                        "removing a content label from {} does not count: {}. The session keeps \
                         its labels.",
                        r.who, r.why
                    ),
                    data: json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })
    }
}

/// The record of an item, by its node (`item_labels::read`).
type ItemOf<'a> = dyn Fn(&str) -> Result<Option<crate::item_labels::ItemLabels>> + 'a;

/// The frame that removes `flag` from `rec`: the `content.removed` row, then
/// the record. None, and nothing to write, when the session does not carry it.
fn removed(
    mut rec: SessionRecord,
    session_id: &str,
    flag: Flag,
    who: &Answerer,
    how: &str,
    correlation_id: Option<&str>,
    (pack_of, item_of): (&dyn Fn(&str) -> String, &ItemOf<'_>),
) -> Result<Option<(UnlabelResult, Vec<NewRecord>)>> {
    let Some(mut flags) = rec.data.take() else {
        return Ok(None);
    };
    let at_ms = theseus_protocol::now_unix_ms();
    let Some(label) = flags.remove(flag, &who.label, &who.via(), at_ms) else {
        return Ok(None);
    };
    rec.data = Some(flags);
    let r = UnlabelResult {
        session_id: session_id.into(),
        flag: flag.as_str().into(),
        by: who.label.clone(),
        who: who.who(),
        via: who.via(),
        how: how.into(),
        correlation_id: correlation_id.map(str::to_string),
        at_ms,
        label: label_view(&label),
    };
    let mut frame = vec![fact::row(
        &ContentRemoved { result: &r },
        Some(session_id),
        None,
    )?];
    // A removal is the clearest "you got this wrong" the classifier will
    // get (theseus-gyjyo.3): the raising judgment's answer to the flag's
    // question was `yes` and should have been `no`. It is written in this
    // frame, so a removal that survives a restart is never one whose label
    // was lost. A label that names no judgment (raised before store format
    // 53) teaches nothing.
    if !label.judgment.is_empty() {
        frame.push(wrong_row(
            &label,
            session_id,
            who,
            &pack_of(&label.judgment),
        )?);
    }
    // The removal is kept on the item that raised the label too (`item_labels`), in this frame, as THIS
    // session's: a fork or a child holds the same node's label, and its removal speaks for it alone. A label
    // from before items kept their labels starts its item's record with itself. An item record that cannot
    // be read is not replaced: the session's removal is written, the item's is not.
    if let (Some(node), Some(gone)) = (label.node.as_deref(), flags_removed(&rec, flag)) {
        match item_of(node) {
            Ok(found) => {
                let mut item = found.unwrap_or_else(|| {
                    let mut i =
                        crate::item_labels::ItemLabels::new(node, label.kind, &label.source, 0);
                    i.raised.push(label.clone());
                    i
                });
                crate::item_labels::removed(&mut item, session_id, &gone);
                frame.push(item.record()?);
            }
            Err(e) => tracing::warn!(node, error = %format!("{e:#}"),
                "content: an item's labels were not read, so its removal is the session's alone"),
        }
    }
    frame.push(NewRecord::json(kinds::SESSION, Some(session_id), &rec)?);
    Ok(Some((r, frame)))
}

/// The removal of `flag` the session just recorded: its last.
fn flags_removed(rec: &SessionRecord, flag: Flag) -> Option<crate::content::Removed> {
    let flags = rec.data.as_deref()?;
    flags.removed.iter().rev().find(|r| r.flag == flag).cloned()
}

/// The `judge.label` row that says the judgment that raised `label` answered
/// its question wrongly: `yes` where the truth was `no`. Source `operator`,
/// weight 1.0, keyed `lbl_<id>` and scoped as its judgment, as `judge.label`
/// writes one, so the learning ledger reads it with the rest.
fn wrong_row(label: &Label, session_id: &str, who: &Answerer, pack: &str) -> Result<NewRecord> {
    let id = crate::new_id("lbl");
    let (who_s, via) = (who.who(), who.via());
    let note = format!("removed by the owner via {via}");
    let f = fact::judge::JudgeLabel {
        id: &id,
        judgment: &label.judgment,
        pack,
        question: Some(label.flag.as_str()),
        about: None,
        label: json!("no"),
        source: "operator",
        who: &who_s,
        via: &via,
        weight: 1.0,
        note: &note,
        correlation_id: None,
        rule: None,
    };
    let mut rec = fact::row(&f, Some(session_id), None)?;
    rec.key = Some(id);
    Ok(rec.scoped(&super::judge::scope_of(pack)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_judge::band::Band;

    fn label(flag: Flag, kind: ItemKind) -> Label {
        Label {
            flag,
            node: Some("n1".into()),
            kind,
            source: "http.fetch https://example.com/x".into(),
            confidence: 0.9,
            band: Band::Act,
            at_ms: 1,
            judgment: String::new(),
        }
    }

    /// The message the owner approved the shape of (design §6.3): the flag in
    /// bold, what it does, what raised it and from where.
    #[test]
    fn the_notice_says_what_the_flag_does_and_what_raised_it() {
        let inj = label_notice(
            &label(Flag::Injection, ItemKind::ToolResult),
            InjectionMode::Refuse,
        );
        assert_eq!(
            inj,
            "⚑ **injection** — this session is holding: a tool result from `http.fetch \
             https://example.com/x` tried to steer me, so writes, programs and sends are refused \
             until you clear it."
        );
        let sec = label_notice(
            &label(Flag::Secret, ItemKind::OperatorMessage),
            InjectionMode::Refuse,
        );
        assert!(sec.starts_with("⚑ **secret** — "), "{sec}");
        assert!(!sec.contains("China-origin"), "no model is kept off: {sec}");
        assert!(sec.contains("your message from `http.fetch"), "{sec}");
        for f in [Flag::Confidential, Flag::Personal] {
            let t = label_notice(&label(f, ItemKind::FetchedPage), InjectionMode::Refuse);
            assert!(t.contains(&format!("**{}**", f.as_str())), "{t}");
            assert!(!t.contains("refused"), "nothing is blocked: {t}");
        }
        let n = label_notice(
            &label(Flag::Injection, ItemKind::SearchResult),
            InjectionMode::Notify,
        );
        assert!(n.contains("Nothing is refused here"), "{n}");
    }
}
