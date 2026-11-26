//! The owner's yes before the self loop publishes a change to its repository's remote (issue 53). A self branch and
//! a fix made at review are not pushed; the join is the one publication of a self change, and it waits for the owner's
//! answer to one card. The card names the review file (`review_file`): everything the join of the head onto the
//! remote's main would send, made from the objects; a yes is bound to the head, that main, the file's SHA-256, the
//! destination and the configuration digest (`git::Dest`), and is checked again under the join lock before the push
//! (`joining::still_approved`). The revert's push asks no card: it publishes main with an already approved merge undone
//! and no session's text (the conductor's ruling, 2026-11-26); its push shares the join's pin (`git::push_main`).
//!
//! The card is a question planned on its own session, as the install's is (`root_install::offer`): the kernel keeps
//! it across a restart, `confirm.list` and the DM show it, and the owner answers it with `action.confirm`. It never
//! expires (`rpc::confirms::expire_questions` passes it over). A yes is read back from the question itself (its
//! binding) when the join comes to that head again; a no writes the held stop row the join already skips
//! (`join::seen`), in the answer's frame. Which question asks for a branch's head is kept in the store's meta, by
//! branch, so a pass after a restart finds it and asks nothing again.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_kernel::{Action, ActionState, RetryClass, TurnEnd, Wake};
use theseus_protocol::{ConfirmRequest, PlainWords, Proposal, SessionOpenParams};
use theseus_store::{kinds, NewRecord};

use crate::fact::answer::CallAnswered;
use crate::fact::rsi_join::SelfJoinStopped;
use crate::outbox::Closed;
use crate::rpc::Core;
use crate::rsi::tree::Place;
use crate::session::SessionRecord;
use crate::turn::OPERATOR;

/// The question the owner's yes answers: a planned action of this name.
pub const ACK: &str = "self.publish.ack";
/// What an agent's process is refused with when it answers the card.
pub const AGENT_REFUSAL: &str =
    "an agent's process is not the owner, and this sends a change of Theseus to the remote, so only the owner \
     answers it: press it in the DM, or answer from your own terminal";
/// The stop row's phase while a change waits for the owner's answer, and when he said no.
pub const PHASE: &str = "publish";
/// The meta key of the session the questions are planned on.
const SESSION: &str = "self.publish.session";
/// The meta key prefix of the question asked for a branch (`Asked`).
const ASKED: &str = "self.publish.asked.";
/// Who settles a question that was asked for a head the branch no longer has.
const SUPERSEDED: &str = "superseded";

/// What a join would publish, as the card names it.
#[derive(Debug, Clone)]
pub struct Leaving {
    pub branch: String,
    pub item: String,
    pub title: String,
    /// The branch's head the card asks about.
    pub head: String,
    /// The remote's main the review file was made against.
    pub main: String,
    /// The remote's name.
    pub remote: String,
    /// Where the push would go (`git::dest`).
    pub dest: super::git::Dest,
    /// The review file (`review_file`).
    pub review: super::review_file::Review,
}

/// The longest title, item or subject a card shows, in characters.
const UNTRUSTED_CHARS: usize = 120;
/// The most commit subjects a card lists; the review file holds them all.
const SUBJECTS: usize = 20;

/// A string a model or a commit wrote, as one line of a card: control characters removed, white space made single
/// spaces, cut to `max` characters, and every character Markdown or a link could use escaped.
pub fn untrusted(s: &str, max: usize) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c.is_control() || c.is_whitespace() { ' ' } else { c })
        .filter(|c| !matches!(*c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = flat.chars().take(max).collect();
    let mut out = String::with_capacity(cut.len());
    for c in cut.chars() {
        if "\\`*_[]()<>#|~!:@".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    if flat.chars().count() > max {
        out.push_str(" [cut]");
    }
    out
}

/// `s` in single quotes, as a shell reads it.
fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

impl Leaving {
    /// The fixed command that prints the review file.
    pub fn command(&self) -> String {
        format!("cat -- {}", quoted(&self.review.path.display().to_string()))
    }

    /// The card's words: the untrusted strings each on a labelled line of their own, escaped; the shas, the
    /// destination, the review file and the decision on fixed lines.
    pub fn question(&self) -> String {
        let r = &self.review;
        let mut subjects: String = (r.commits.iter().take(SUBJECTS))
            .map(|(c, s)| format!("Commit {}: {}\n", short(c), untrusted(s, UNTRUSTED_CHARS)))
            .collect();
        if r.commits.len() > SUBJECTS {
            subjects.push_str(&format!(
                "({} more commits are in the review file)\n",
                r.commits.len() - SUBJECTS
            ));
        }
        let size = match r.size.is_empty() {
            true => "no change to any file".to_string(),
            false => r.size.clone(),
        };
        format!(
            "Publish a self change to the remote's main?\n\n\
             Title: {title}\nItem: {item}\nBranch: {branch}\n{subjects}\n\
             HEAD: {head}\nREMOTE MAIN: {main}\nDESTINATION: {dest}\nCOMMITS: {n}\nFILES: {files} ({size})\n\
             REVIEW FILE: {path}\nREVIEW FILE SIZE: {bytes} bytes\nREVIEW FILE SHA-256: {sha}\nPRINT IT: {cmd}\n\n\
             The review file holds every commit the join of HEAD onto REMOTE MAIN would send: each commit's raw object \
             and its change against each of its parents, made from git's objects with text conversion, external diff \
             and renames off. The join adds its merge commit and main's count to them.\n\n\
             YES: the join merges HEAD into main, runs main's gate on the merge, signs it and pushes main, one ref, to \
             DESTINATION. Your yes counts only for this HEAD, this review file and this DESTINATION, while the remote's \
             main is REMOTE MAIN or has only moved forward with none of HEAD's commits sent; anything else asks a new \
             card.\nNO: nothing is published, the branch's HEAD is held, and you are not asked about it again.",
            title = untrusted(&self.title, UNTRUSTED_CHARS),
            item = untrusted(&self.item, UNTRUSTED_CHARS),
            branch = untrusted(&self.branch, UNTRUSTED_CHARS),
            head = self.head,
            main = self.main,
            dest = self.dest.shown(),
            n = r.commits.len(),
            files = r.files,
            path = r.path.display(),
            bytes = r.bytes,
            sha = r.sha256,
            cmd = self.command(),
        )
    }
}

/// The question asked for a branch, and what a yes to it is bound to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Asked {
    pub question: String,
    pub head: String,
    /// The remote's main the review file was made against.
    pub main: String,
    /// The review file's SHA-256.
    pub file_sha256: String,
    /// The destination and the configuration digest (`git::Dest`).
    pub dest: super::git::Dest,
}

/// The owner's answer to a question, as the question stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Not answered yet.
    Waiting,
    Yes,
    /// The owner said no.
    No,
    /// It ended with no answer of the owner's (its session retired, or it was superseded): ask again.
    Ended,
}

/// Whether a yes given against `card_main` still covers a join of `head` onto `now`: `now` is `card_main`, or a
/// fast-forward of it (`card_main` an ancestor of `now`) after which every commit the push would send of `head`'s,
/// `now..head`, is one of `card_main..head`, the review file's.
pub fn covered(
    p: &crate::rsi::tree::Place,
    card_main: &str,
    now: &str,
    head: &str,
) -> anyhow::Result<bool> {
    use super::git;
    if now == card_main {
        return Ok(true);
    }
    if !git::is_ancestor(p, card_main, now)? {
        return Ok(false);
    }
    let shown = git::commits(p, &format!("{card_main}..{head}"))?;
    Ok(git::commits(p, &format!("{now}..{head}"))?
        .iter()
        .all(|c| shown.contains(c)))
}

/// A commit's first twelve characters.
pub fn short(c: &str) -> &str {
    &c[..c.len().min(12)]
}

fn asked_key(branch: &str) -> String {
    format!("{ASKED}{branch}")
}

impl Core {
    /// The question asked for `branch`, if one was.
    pub(crate) fn publish_asked(&self, branch: &str) -> Result<Option<Asked>> {
        self.store.get_meta::<Asked>(&asked_key(branch))
    }

    /// How the question `id` stands.
    pub(crate) fn publish_answer(&self, id: &str) -> Result<Answer> {
        let Some(a) = self.kernel.action(id)? else {
            return Ok(Answer::Ended);
        };
        Ok(match (a.state, a.confirm.is_some()) {
            (ActionState::Planned, false) => Answer::Waiting,
            (ActionState::Cancelled, _) => match self.publish_declined(id)? {
                true => Answer::No,
                false => Answer::Ended,
            },
            (_, true) => Answer::Yes,
            _ => Answer::Ended,
        })
    }

    /// Whether the owner declined the question `id`: its no wrote the held stop row with its id.
    fn publish_declined(&self, id: &str) -> Result<bool> {
        let rows =
            crate::rsi::join::rows_of(&self.store, &["self.join.stopped"])?.unwrap_or_default();
        Ok(rows.iter().any(|r| {
            let d = &r.data;
            d["phase"] == PHASE && d["numbers"]["held"] == true && d["numbers"]["question"] == id
        }))
    }

    /// End the question asked for `branch`, if any: a card still waiting is settled as superseded, and what was
    /// asked is forgotten, so a yes to it counts for nothing and the next pass asks one new card for what is there
    /// then. `why` is the settled card's note.
    pub(crate) fn publish_end(&self, branch: &str, why: &str) -> Result<()> {
        let Some(old) = self.publish_asked(branch)? else {
            return Ok(());
        };
        if self.publish_answer(&old.question)? == Answer::Waiting {
            if let Some(a) = self.kernel.action(&old.question)? {
                self.publish_settle(&a, false, Some(why), (SUPERSEDED, SUPERSEDED))?;
            }
        }
        self.store.delete_meta(&asked_key(branch))
    }

    /// End the question asked for `branch` when it was asked for `head`: that head is superseded.
    pub(crate) fn publish_end_for_head(&self, branch: &str, head: &str) -> Result<()> {
        match self.publish_asked(branch)? {
            Some(a) if a.head == head => self.publish_end(
                branch,
                "the branch has a newer head; a new card asks for it",
            ),
            _ => Ok(()),
        }
    }

    /// Ask the owner one card for `l`. A question still waiting for an earlier head of the branch is settled as
    /// superseded first: an answer to it would be about content that no longer leaves. Returns the question's id.
    pub(crate) fn ask_publish(&self, l: &Leaving) -> Result<String> {
        if let Some(old) = self.publish_asked(&l.branch)? {
            if (old.head, &old.main) != (l.head.clone(), &l.main)
                && self.publish_answer(&old.question)? == Answer::Waiting
            {
                if let Some(a) = self.kernel.action(&old.question)? {
                    let note = "the branch moved after this was asked; a new card asks for its new content";
                    self.publish_settle(&a, false, Some(note), (SUPERSEDED, SUPERSEDED))?;
                }
            }
        }
        let rec = self.publish_session()?;
        let exec = rec
            .execution_id
            .clone()
            .context("the publish session has no execution")?;
        let question = l.question();
        let proposal = Proposal {
            tool: ACK.into(),
            args: json!({
                "question": question, "branch": l.branch, "item": l.item, "title": l.title,
                "head": l.head, "main": l.main, "remote": l.remote, "destination": l.dest.shown(),
                "review_file": l.review.path, "review_sha256": l.review.sha256, "command": l.command(),
            }),
            resource: Some(format!("self-publish:{}", short(&l.head))),
            policy_context: json!({}),
        };
        let guard = self.kernel.admit_input(&exec)?;
        let key = asked_key(&l.branch);
        let branch = l.branch.clone();
        let bound = Asked {
            question: String::new(),
            head: l.head.clone(),
            main: l.main.clone(),
            file_sha256: l.review.sha256.clone(),
            dest: l.dest.clone(),
        };
        let planned = self.kernel.plan_confirm_with(
            &guard,
            &proposal,
            RetryClass::NonRepeatable,
            None,
            |a| {
                let asked = Asked {
                    question: a.correlation_id.clone(),
                    ..bound.clone()
                };
                Ok(vec![NewRecord::json(kinds::META, Some(&key), &asked)?])
            },
        );
        let ended = self
            .kernel
            .end_turn(guard, TurnEnd::Wait { wake: Wake::Input });
        let a = planned?;
        ended?;
        let ttl = self.kernel.config().confirm_ttl_ms;
        self.session_rec(&rec.session_id)
            .record(&crate::fact::tool::CallAsked {
                request: &confirm_request(&a, &rec, ttl),
            });
        tracing::info!(%branch, head = %short(&l.head), question = %a.correlation_id, "self-publish: the owner is asked");
        Ok(a.correlation_id)
    }

    /// The questions' own session, opened at the first card and kept while it is open.
    fn publish_session(&self) -> Result<SessionRecord> {
        if let Some(id) = self.store.get_meta::<String>(SESSION)? {
            if let Some(rec) = self.store.get_session::<SessionRecord>(&id)? {
                let open = (rec.execution_id.as_deref())
                    .and_then(|x| self.kernel.execution(x).ok().flatten())
                    .is_some_and(|e| !e.state.is_terminal());
                if open {
                    return Ok(rec);
                }
            }
        }
        let rec = self.open_session(SessionOpenParams {
            dir: None,
            kind: None,
            label: Some("self-publish: changes waiting on the owner's yes".into()),
            opened_from: None,
            persona: None,
        })?;
        self.store.put_meta(SESSION, &rec.session_id)?;
        Ok(rec)
    }

    /// The owner's answer to a publish card: the bind or the decline, the answer's row and, for a no, the held stop
    /// row of the head it asked about, in one frame. Nothing is sent from here: the join reads a yes when it comes to
    /// that head.
    pub(crate) fn answer_publish(
        &self,
        a: &Action,
        approve: bool,
        note: Option<&str>,
        by: &str,
        via: &str,
    ) -> Result<theseus_protocol::ActionConfirmResult> {
        self.publish_settle(a, approve, note, (by, via))?;
        Ok(theseus_protocol::ActionConfirmResult {
            correlation_id: a.correlation_id.clone(),
            approved: approve,
            session_id: a.session_id.clone(),
            execution_id: a.execution_id.clone(),
            resumes: false,
        })
    }

    fn publish_settle(
        &self,
        a: &Action,
        approve: bool,
        note: Option<&str>,
        (by, via): (&str, &str),
    ) -> Result<()> {
        let args = a
            .proposal
            .as_ref()
            .map(|p| p.args.clone())
            .unwrap_or_default();
        let corr = a.correlation_id.as_str();
        let answered = CallAnswered {
            action: a,
            approve,
            note,
            by,
            via,
            option: Some(if approve { "approve" } else { "decline" }),
        };
        let session = Some(a.session_id.as_str());
        let mut rows = vec![crate::fact::row(&answered, session, None)?];
        // The owner's no holds the head; a question ended for him (superseded) holds nothing.
        if !approve && by != SUPERSEDED {
            let stop = SelfJoinStopped {
                branch: args["branch"].as_str().unwrap_or_default().to_string(),
                head: args["head"].as_str().unwrap_or_default().to_string(),
                phase: PHASE.into(),
                why: format!(
                    "the owner said no to publishing it{}",
                    note.map(|n| format!(": {n}")).unwrap_or_default()
                ),
                numbers: json!({"held": true, "question": corr}),
            };
            rows.push(crate::fact::row(&stop, None, None)?);
        }
        let proposal = a.proposal.clone().unwrap_or_default();
        self.kernel.frame(&[&a.execution_id], |k| {
            match approve {
                true => k.bind_confirm(corr, OPERATOR, &proposal).map(drop)?,
                false => {
                    k.decline_action(corr, by, note.unwrap_or("the owner declined"))
                        .map(drop)?;
                }
            }
            k.stage(&rows)
        })?;
        self.session_rec(&a.session_id).announce(&answered);
        let closed = match (approve, by) {
            (true, _) => Closed::new("approved", Some(by)),
            (false, SUPERSEDED) => Closed {
                note: note.map(str::to_string),
                ..Closed::new("expired", None)
            },
            (false, _) => Closed {
                note: note.map(str::to_string),
                ..Closed::new("declined", Some(by))
            },
        };
        self.card_closed(corr, closed);
        Ok(())
    }
}

/// The question as the owner sees it: its card, `confirm.list`.
pub fn confirm_request(a: &Action, session: &SessionRecord, ttl_ms: u64) -> ConfirmRequest {
    let args = a
        .proposal
        .as_ref()
        .map(|p| p.args.clone())
        .unwrap_or_default();
    let text = |k: &str| args[k].as_str().unwrap_or_default().to_string();
    let plain = PlainWords {
        who: "Theseus".into(),
        what: format!("publish {}", untrusted(&text("title"), UNTRUSTED_CHARS)),
        why: String::new(),
        held:
            "it publishes a change of Theseus to its repository's remote, so only you may allow it"
                .into(),
        risk: "network".into(),
        yes: format!(
            "merges it into main and pushes main to {}.",
            text("destination")
        ),
        no: "publishes nothing; the head is held and you are not asked about it again.".into(),
        detail: text("question"),
    };
    crate::rpc::offer(ConfirmRequest {
        correlation_id: a.correlation_id.clone(),
        session_id: session.session_id.clone(),
        execution_id: a.execution_id.clone(),
        turn_id: None,
        tool: ACK.into(),
        reason: text("question"),
        input: args,
        resource: a.resource.clone(),
        by: OPERATOR.into(),
        requested_at_ms: a.planned_at_ms,
        expires_at_ms: a.planned_at_ms + ttl_ms,
        floor: false,
        budget: None,
        task: crate::task::task_ref(session),
        change: None,
        options: Vec::new(),
        may_answer: None,
        preview: None,
        grant_offer: None,
        plain: Some(plain),
        needs_proof: false,
        needs_fresh_proof: false,
        proof: None,
    })
}

/// The stop row's numbers while `l` waits for the answer to `question`.
pub fn parked(question: &str, l: &Leaving) -> Value {
    json!({"parked": true, "question": question, "main": l.main, "commits": l.review.commits.len(),
           "files": l.review.files, "review_sha256": l.review.sha256})
}
