//! The owner's yes before the self loop publishes a change (issue 53). The join is the one publication the owner is
//! asked about: it waits for his answer to one card that names the change and the command that shows it. The loop
//! pushes no self branch and no fix made at review. The revert's push is not asked: it publishes main with a merge
//! the owner already said yes to undone, and carries no session's text.
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

/// What would be sent: a join of a branch (the card's and its rows' `kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Join,
}

/// What the card names: the change, and how much of it would leave.
#[derive(Debug, Clone)]
pub struct Leaving {
    pub kind: Kind,
    /// The branch.
    pub branch: String,
    pub item: String,
    pub title: String,
    /// The commit that would be sent (the branch's head).
    pub head: String,
    /// The remote's main the content is read against.
    pub main: String,
    /// The remote.
    pub remote: String,
    /// `(commit, subject)` of every commit `main..head`, oldest first.
    pub commits: Vec<(String, String)>,
    pub files: usize,
    /// `git diff --shortstat main...head`.
    pub size: String,
    /// The self repository.
    pub repo: std::path::PathBuf,
}

impl Leaving {
    /// What `head` would send against `main`, read from the self repository.
    pub fn read(
        p: &Place,
        kind: Kind,
        (branch, item, title): (&str, &str, &str),
        remote: &str,
        main: &str,
        head: &str,
    ) -> Result<Self> {
        let range = format!("{main}..{head}");
        let log = p.git_ok(&p.repo, &["log", "--reverse", "--format=%H%x09%s", &range])?;
        let commits = log
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(c, s)| (c.to_string(), s.to_string()))
            .collect();
        let three = format!("{main}...{head}");
        let names = p.git_ok(&p.repo, &["diff", "--name-only", "--no-renames", &three])?;
        let size = p.git_ok(&p.repo, &["diff", "--shortstat", "--no-renames", &three])?;
        Ok(Self {
            kind,
            branch: branch.into(),
            item: item.into(),
            title: title.into(),
            head: head.into(),
            main: main.into(),
            remote: remote.into(),
            commits,
            files: names.lines().filter(|l| !l.trim().is_empty()).count(),
            size: size.trim().to_string(),
            repo: p.repo.clone(),
        })
    }

    /// The command that prints the change: every commit message, whole, and each commit's text change, against the
    /// remote's main as it was when the card was asked. A binary file is named, not shown, and a merge commit's own
    /// changes are not shown.
    pub fn command(&self) -> String {
        format!(
            "git -C {} log --reverse --format=fuller --patch --stat {}..{}",
            self.repo.display(),
            self.main,
            self.head
        )
    }

    /// The card's words.
    pub fn question(&self) -> String {
        let first = format!(
            "Merge this self change into main and publish it: {}?",
            self.title
        );
        let subjects: String = self
            .commits
            .iter()
            .map(|(c, s)| format!("- {} {s}\n", short(c)))
            .collect();
        let size = if self.size.is_empty() {
            "no change to any file".to_string()
        } else {
            self.size.clone()
        };
        format!(
            "{first}\n\nItem: {item}\nBranch: {branch}\nCommit: {head}\n{n} commit(s), {files} file(s): {size}\n\n\
             {subjects}\nIt has not been pushed. To read it first, ask Theseus here to show you this change (commit \
             {head8} against main {main8}), or run:\n\n{cmd}\n\n\
             It shows every commit message and every text change; a binary file is named, not shown, and a merge \
             commit's own changes are not.\n\n\
             Yes: Theseus merges it into main, runs main's gate on the merge, signs it and pushes main. No: nothing \
             is sent, the branch is held and you are not asked again. This answer is for this commit only.",
            item = self.item,
            branch = self.branch,
            head = self.head,
            n = self.commits.len(),
            files = self.files,
            head8 = eight(&self.head),
            main8 = eight(&self.main),
            cmd = self.command(),
        )
    }
}

/// The question asked for a branch: its id and the head it asks about.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Asked {
    pub question: String,
    pub head: String,
    pub kind: Kind,
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

/// A commit's first eight characters, as the card names it.
fn eight(c: &str) -> &str {
    &c[..c.len().min(8)]
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

    /// Ask the owner one card for `l`. A question still waiting for an earlier head of the branch is settled as
    /// superseded first: an answer to it would be about content that no longer leaves. Returns the question's id.
    pub(crate) fn ask_publish(&self, l: &Leaving) -> Result<String> {
        if let Some(old) = self.publish_asked(&l.branch)? {
            if old.head != l.head && self.publish_answer(&old.question)? == Answer::Waiting {
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
                "question": question, "kind": l.kind, "branch": l.branch, "item": l.item, "title": l.title,
                "head": l.head, "main": l.main, "remote": l.remote, "command": l.command(),
            }),
            resource: Some(format!("self-publish:{}", short(&l.head))),
            policy_context: json!({}),
        };
        let guard = self.kernel.admit_input(&exec)?;
        let key = asked_key(&l.branch);
        let (branch, head, kind) = (l.branch.clone(), l.head.clone(), l.kind);
        let planned = self.kernel.plan_confirm_with(
            &guard,
            &proposal,
            RetryClass::NonRepeatable,
            None,
            |a| {
                let asked = Asked {
                    question: a.correlation_id.clone(),
                    head: head.clone(),
                    kind,
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
                numbers: json!({"held": true, "question": corr, "kind": args["kind"]}),
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
        what: format!("publish {}", text("title")),
        why: String::new(),
        held: "it sends a change of Theseus to the remote, so only you may allow it".into(),
        risk: "network".into(),
        yes: format!("merges it into main and pushes main to {}.", text("remote")),
        no: "sends nothing; the change is held and you are not asked again.".into(),
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
    json!({"parked": true, "question": question, "main": l.main, "commits": l.commits.len(), "files": l.files})
}
