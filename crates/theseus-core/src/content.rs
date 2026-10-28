//! Content flags (theseus-9ymz0): every item that enters a session is
//! classified once, by itself, and what the classifier finds becomes a durable
//! label on the session. This module is the whole rule, and it is pure: no
//! store, no judge, no I/O. The ask is `judge/content.rs`; the effects are read
//! at the provider chokepoint (`data_gate.rs`), at the turn's floor
//! (`turn/data_step.rs`) and in the tool gate (`policy/modes.rs`'s
//! `refusal_at`).
//!
//! It replaces the open view, which proved per request that every node a
//! request carried belonged to an unbroken run of content judged open. That
//! proof could not be discharged: the route pack's `data` question reached the
//! act band on 2 to 15% of answers, with no labels ever accrued, so a model of
//! restricted origin was refused nearly everything.
//!
//! - **A flag is raised, never proved.** A session may use a model of
//!   restricted origin unless something found in it says otherwise.
//! - **Two bars, on purpose.** [`Flag::Secret`], [`Flag::Confidential`] and
//!   [`Flag::Personal`] raise at the confirm band: raising one wrongly costs a
//!   session its cheap model, which is the safe direction, and the direction
//!   the owner's standing rule points. [`Flag::Injection`] raises only at the
//!   act band: raising it wrongly freezes a session until the operator clears
//!   it, and a false freeze teaches the operator to ignore the control.
//! - **The names tripwire is untouched.** `[privacy] sensitive_names` at the
//!   chokepoint is deterministic and independent of every flag here. It is why
//!   the low bar above is safe.
//! - **Labels are durable.** They live on `SessionRecord.data`, survive
//!   compaction and restart, and only the operator removes one
//!   (`content.unlabel`, `theseus content remove`, the one Discord message).

use serde::{Deserialize, Serialize};
use theseus_judge::band::Band;
use theseus_tools::ToolClass;

use crate::node::{Body, Node, Origin};

/// What a classification can find in one item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flag {
    /// The item tries to steer the assistant: instructions it was not given by
    /// the operator. The session freezes until the operator clears it.
    Injection,
    /// The item shows the value of a credential.
    Secret,
    /// The item shows an organisation's non-public matter.
    Confidential,
    /// The item shows a person's private information.
    Personal,
}

impl Flag {
    /// Every flag, in the order `disqualifies_cn` reports them.
    pub const ALL: [Flag; 4] = [
        Flag::Injection,
        Flag::Secret,
        Flag::Confidential,
        Flag::Personal,
    ];

    /// Its word in the pack, the rows, the CLI and the config.
    pub fn as_str(self) -> &'static str {
        match self {
            Flag::Injection => "injection",
            Flag::Secret => "secret",
            Flag::Confidential => "confidential",
            Flag::Personal => "personal",
        }
    }

    /// The flag by its word, as the CLI and the protocol take it.
    pub fn parse(s: &str) -> Option<Flag> {
        Flag::ALL.into_iter().find(|f| f.as_str() == s)
    }

    /// The band at or above which an answer of `yes` raises this flag. The two
    /// bars are the asymmetry this design turns on; see the module note.
    pub fn bar(self) -> Band {
        match self {
            Flag::Injection => Band::Act,
            Flag::Secret | Flag::Confidential | Flag::Personal => Band::Confirm,
        }
    }

    /// Whether an answer of `yes` in `band` raises it. `Band` has no ordering
    /// of its own (its variants read `Act`, `Confirm`, `Escalate`, so a derived
    /// one would run backwards), so the two cases are written out.
    pub fn raised_by(self, band: Band) -> bool {
        match self {
            Flag::Injection => band == Band::Act,
            Flag::Secret | Flag::Confidential | Flag::Personal => {
                matches!(band, Band::Act | Band::Confirm)
            }
        }
    }

    /// Whether this flag keeps the session off a model of restricted origin.
    pub fn disqualifies_cn(self) -> bool {
        !matches!(self, Flag::Injection)
    }

    /// What the operator is told when it is raised, in one line.
    pub fn means(self) -> &'static str {
        match self {
            Flag::Injection => "something in this session tried to steer me",
            Flag::Secret => "this session has seen the value of a credential",
            Flag::Confidential => "this session has seen an organisation's non-public matter",
            Flag::Personal => "this session has seen a person's private information",
        }
    }
}

/// What an item was, for the label that came of it and for the pack's state.
/// A `const`, not a free string, so every hook site names one of these and a
/// reader of a row knows the whole set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// A message **the owner** sent. Never asked the injection question.
    OperatorMessage,
    /// A message anyone else sent, in a shared place. Asked like any other
    /// text that did not come from the owner (see [`ItemKind::asks_injection`]).
    PersonMessage,
    /// Any tool's result: a file read, a program's output, an MCP call.
    ToolResult,
    /// A page `http.fetch` brought in.
    FetchedPage,
    /// `web.search`'s results.
    SearchResult,
    /// A task's brief, entering the task's own session.
    TaskBrief,
    /// A task's report, entering the parent.
    TaskReport,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::OperatorMessage => "operator_message",
            ItemKind::PersonMessage => "person_message",
            ItemKind::ToolResult => "tool_result",
            ItemKind::FetchedPage => "fetched_page",
            ItemKind::SearchResult => "search_result",
            ItemKind::TaskBrief => "task_brief",
            ItemKind::TaskReport => "task_report",
        }
    }

    /// Whether the injection question is asked of an item of this kind.
    ///
    /// **The list lives in `theseus_judge::builders::content::asks_injection`,
    /// which is what the builder itself calls**, and this delegates to it. It
    /// was stated in both places until the review of 2026-10-26 found this one
    /// was dead code and the two had drifted. The exemptions live in code
    /// rather than in the pack's wording either way: a wording rule can be
    /// talked around by the very text it judges, a question that is never asked
    /// cannot be answered.
    ///
    /// - [`ItemKind::OperatorMessage`] — the owner's own words (the owner,
    ///   2026-10-17: "my own DM or channel text should never cause a flag").
    ///   **This is the owner's message alone.** A message from anyone else in a
    ///   shared place is [`ItemKind::PersonMessage`] and *is* asked: the rule
    ///   he stated is about his own text, and a guest in a shared channel is
    ///   exactly who an injection would come from.
    /// - [`ItemKind::TaskBrief`] — a brief is the instructions a parent session
    ///   gave its child. It is steering by nature and by design, so the
    ///   question is a guaranteed false positive, and at the act band it would
    ///   freeze every task session on its first item. Nothing is lost: a parent
    ///   carrying `injection` is frozen and **cannot call `task.create` at all**
    ///   (it is not a local read), so an injection cannot launder itself into a
    ///   child; and a brief that quotes foreign text is still asked `secret`,
    ///   `confidential` and `personal`.
    ///
    /// [`ItemKind::TaskReport`] is **not** exempt. A report reports rather than
    /// instructs, so the false-positive argument does not hold, and a child that
    /// read a poisoned page can write its parent a report shaped by it — which
    /// is a real way in, and the one the freeze does not already close.
    ///
    /// The other three questions are asked of every kind, the owner's own
    /// messages included: he can paste a key or a confidential document, and he
    /// wants that to disqualify a model of restricted origin.
    pub fn asks_injection(self) -> bool {
        theseus_judge::builders::asks_injection(self.as_str())
    }
}

/// One flag on a session, and what raised it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub flag: Flag,
    /// The node whose content raised it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub kind: ItemKind,
    /// The tool and the resource or address it named, or the place a message
    /// arrived in. Shown to the operator, so never a secret and never a path
    /// outside `home_shown`.
    pub source: String,
    pub confidence: f64,
    pub band: Band,
    pub at_ms: u64,
}

/// A label the operator removed, kept as history: a label that was raised and
/// cleared is a different thing from one that was never raised.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Removed {
    pub flag: Flag,
    /// The operator, as an answer's `Answerer` labels them.
    pub by: String,
    /// The surface the removal came through.
    pub via: String,
    pub at_ms: u64,
}

/// A session's content labels (`SessionRecord.data`).
///
/// A record written before this (store format 51 and earlier) held the open
/// view's `DataState` here. Every field below is `#[serde(default)]` and
/// nothing is denied, so such a record reads as **no labels and nothing
/// judged** — which is the right reading: a stale open-run proof means nothing
/// under this rule, and the first item to enter the session judges it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(from = "Stored")]
pub struct ContentFlags {
    /// At most one label per flag: a second raise of a flag already present
    /// keeps the first, so the label names the item that first brought it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<Label>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<Removed>,
    /// The last node whose content was judged, in transcript order. A request
    /// to a model of restricted origin whose session has nodes past this one is
    /// not yet judged, and waits (`turn/data_step.rs`) or goes to Claude.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judged_through: Option<String>,
    /// Nodes that could not be judged (Jev failed, the reservation was
    /// refused, the pack was in shadow): a record for the operator and a
    /// worklist for a re-ask. It does **not** gate the mark — the transcript
    /// does, through [`ContentFlags::judged_run`] — so a stale or lost entry
    /// here can never make a session look more judged than it is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unjudged: Vec<String>,
    /// `judged_through` as this copy was read from the store, which a write
    /// back compares against (`merge`): compare and set. Never stored.
    #[serde(skip)]
    read: Option<String>,
}

/// `ContentFlags` as stored: reading it remembers what it read, as the open
/// view's `DataState` did for its run.
#[derive(Deserialize)]
struct Stored {
    #[serde(default)]
    labels: Vec<Label>,
    #[serde(default)]
    removed: Vec<Removed>,
    #[serde(default)]
    judged_through: Option<String>,
    #[serde(default)]
    unjudged: Vec<String>,
}

impl From<Stored> for ContentFlags {
    fn from(s: Stored) -> Self {
        Self {
            read: s.judged_through.clone(),
            labels: s.labels,
            removed: s.removed,
            judged_through: s.judged_through,
            unjudged: s.unjudged,
        }
    }
}

/// Equal when what is stored is.
impl PartialEq for ContentFlags {
    fn eq(&self, o: &Self) -> bool {
        (
            &self.labels,
            &self.removed,
            &self.judged_through,
            &self.unjudged,
        ) == (&o.labels, &o.removed, &o.judged_through, &o.unjudged)
    }
}

impl Eq for ContentFlags {}

impl ContentFlags {
    pub fn has(&self, f: Flag) -> bool {
        self.labels.iter().any(|l| l.flag == f)
    }

    pub fn label(&self, f: Flag) -> Option<&Label> {
        self.labels.iter().find(|l| l.flag == f)
    }

    /// The flag that keeps this session off a model of restricted origin, if
    /// one does: in `Flag::ALL`'s order, so the word is stable.
    pub fn disqualifies_cn(&self) -> Option<Flag> {
        Flag::ALL
            .into_iter()
            .find(|f| f.disqualifies_cn() && self.has(*f))
    }

    /// The session is frozen: its calls that act are refused until the operator
    /// clears the label.
    pub fn frozen(&self) -> bool {
        self.has(Flag::Injection)
    }

    /// Raise `l`, if its flag is not already on the session. True when it was
    /// raised, so the caller writes its row only then.
    pub fn raise(&mut self, l: Label) -> bool {
        if self.has(l.flag) {
            return false;
        }
        self.labels.push(l);
        self.labels.sort_by_key(|l| l.flag);
        true
    }

    /// Remove `f`, keeping it as history. The label removed, when there was
    /// one; `None` means there was nothing to remove and nothing is written.
    pub fn remove(&mut self, f: Flag, by: &str, via: &str, at_ms: u64) -> Option<Label> {
        let at = self.labels.iter().position(|l| l.flag == f)?;
        let l = self.labels.remove(at);
        self.removed.push(Removed {
            flag: f,
            by: by.into(),
            via: via.into(),
            at_ms,
        });
        Some(l)
    }

    /// Whether `node`, and everything before it, has been judged.
    pub fn judged(&self, node: &str) -> bool {
        self.judged_through.as_deref() == Some(node)
    }

    /// Record that `node` could not be judged. Idempotent. This is a record
    /// for the operator and for a re-ask to work from; it is **not** what
    /// stops the mark — see [`ContentFlags::judged_run`], which is gated by
    /// the transcript itself.
    pub fn could_not_judge(&mut self, node: &str) {
        if !self.unjudged.iter().any(|n| n == node) {
            self.unjudged.push(node.into());
        }
    }

    /// Advance the mark to `node`, where `span` is **every transcript node
    /// after the current mark up to and including `node`**, in order, and
    /// `judged_now` is the set this landing actually judged. The mark moves
    /// only when every node of the span was judged. Returns whether it moved.
    ///
    /// The span comes from the transcript, which is why this holds **across a
    /// restart with no in-memory state at all**. If an item was skipped, the
    /// span from the mark will always contain it again, and a later landing
    /// that judged only the newest item cannot carry the mark over it. An
    /// earlier draft of this gated on a stored list of skipped nodes instead;
    /// that was a worse primitive, because it needed the caller to pass a run
    /// it had not judged in order to clear it, and it got the re-ask case
    /// wrong. The transcript is already the durable record; nothing else has
    /// to be.
    pub fn judged_run(&mut self, span: &[String], judged_now: &[String], node: &str) -> bool {
        if span.is_empty()
            || span.last().map(String::as_str) != Some(node)
            || !span.iter().all(|n| judged_now.contains(n))
        {
            return false;
        }
        self.unjudged.retain(|n| !span.contains(n));
        self.judged_through = Some(node.into());
        true
    }

    /// A write back merged into the stored labels, never over them: a stored
    /// label always wins (a turn's copy cannot clear one, only the operator
    /// can), a stored removal is kept, and `judged_through` is taken only while
    /// the stored one is still the one this copy read, or already this copy's:
    /// compare and set, as the open view's run was.
    pub fn merge(
        stored: Option<Box<ContentFlags>>,
        turn: Option<&ContentFlags>,
    ) -> Option<Box<ContentFlags>> {
        let Some(turn) = turn else {
            return stored;
        };
        let mut s = stored.map(|b| *b).unwrap_or_default();
        for l in &turn.labels {
            s.raise(l.clone());
        }
        for n in &turn.unjudged {
            s.could_not_judge(n);
        }
        if s.judged_through == turn.read || s.judged_through == turn.judged_through {
            s.judged_through.clone_from(&turn.judged_through);
        }
        (s != ContentFlags::default()).then(|| Box::new(s))
    }

    /// Its line for a person (`inspect`, the CLI).
    pub fn line(&self) -> String {
        let labels = match self.labels.is_empty() {
            true => "no labels".to_string(),
            false => {
                let each: Vec<&str> = self.labels.iter().map(|l| l.flag.as_str()).collect();
                each.join(", ")
            }
        };
        let judged = self
            .judged_through
            .as_deref()
            .map_or("nothing judged".to_string(), |n| format!("judged to {n}"));
        format!("{labels}; {judged}")
    }
}

// ------------------------------------------------------------ the freeze

/// `[content_based_security] injection`: what an `injection` label does to a call that acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectionMode {
    /// The call is refused until the operator removes the label.
    #[default]
    Refuse,
    /// The call runs and the operator is told. **The bench daemon's setting**,
    /// so a trial never freezes waiting for a tap.
    Notify,
}

impl InjectionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            InjectionMode::Refuse => "refuse",
            InjectionMode::Notify => "notify",
        }
    }
}

/// The tools a frozen session may still call: reads that do not leave this
/// machine. **An allowlist, deliberately**, as `open_public` was: a tool added
/// later is refused in a frozen session until someone has judged it here.
///
/// This is the correction of a hole in the first draft of the design, which
/// said the owner's three words (writes, programs, outbound sends) needed no
/// list of their own because "each is already a class that is not `Read`".
/// That is false: `http.fetch` and `web.search` are both [`ToolClass::Read`],
/// and a fetch of `https://elsewhere.example/?x=<whatever the model knows>` is
/// an outbound send with the data in the query string. A session that an
/// injection has taken over must not keep that channel.
///
/// So: local reads of files and code (`fs.`, `git.`, `text.`), the tool
/// catalogue, and a task's reads of its own record. Nothing that crosses the
/// machine boundary — no fetch, no search, no MCP tool, no Workspace or cloud
/// call, no post — and nothing that writes or runs.
///
/// **`lsp.` is deliberately NOT here**, though its reads are `Read` class. The
/// review of 2026-10-26 found the hole: starting a language server *is* a run
/// (`lsp/mod.rs`: "a language server runs build scripts and proc macros, as
/// `cargo build` would"), and `lsp::gate` handles that by raising the call's
/// posture rather than its class — which is not what this reads. Where that
/// posture is open, a frozen session could have started rust-analyzer and run
/// the project's `build.rs`. `fs.`, `git.` and `text.` are enough to
/// investigate a frozen session, which is what this allowlist is for.
pub fn local_read(tool: &str) -> bool {
    ["fs.", "git.", "text.", "task.read", "tools.find"]
        .iter()
        .any(|p| tool.starts_with(p))
}

/// Why an `injection` label refuses this call, or `None`. Read in
/// `ToolRuntime::refusal_at`, beside plan mode's refusal, which this follows
/// word for word: after the whole order and the allow list, the call is
/// refused in words the model can act on.
///
/// A **local** read still runs, so the session can be investigated and the
/// operator can see in the transcript what the injection said. Everything else
/// is refused: writes, programs, and every send outward — see [`local_read`]
/// for why that last one cannot be left to the tool's class.
pub fn refusal(
    flags: Option<&ContentFlags>,
    mode: InjectionMode,
    class: ToolClass,
    tool: &str,
) -> Option<String> {
    if mode == InjectionMode::Notify || (class == ToolClass::Read && local_read(tool)) {
        return None;
    }
    let l = flags?.label(Flag::Injection)?;
    Some(format!(
        "this session is holding: {} ({}{}), so everything but a local read of files and code is \
         refused until the operator clears the label — writes, programs, and anything that leaves \
         this machine, a fetch or a search included. {tool} was not run. Tell the operator what \
         you were asked to do and that the session needs clearing; they clear it with `theseus \
         content remove <session> injection`, or by the button on the message in this place. Do \
         not route around this.",
        Flag::Injection.means(),
        l.kind.as_str(),
        match l.node.as_deref() {
            Some(n) => format!(", node {n}"),
            None => String::new(),
        },
    ))
}

// ------------------------------------------------------------ the items

/// The results no item is made of: their text is Theseus's own fixed
/// acknowledgement, so there is nothing in them to classify and a call for each
/// would be the judge's budget spent on our own words. Everything else is
/// classified, whatever tool it came from.
pub const UNJUDGED_TOOLS: [&str; 2] = ["task.update", "task.close"];

/// Whether a result of `tool` is classified.
pub fn judges(tool: &str) -> bool {
    !UNJUDGED_TOOLS.contains(&tool)
}

/// The authors whose user-role nodes are the harness talking to the model in
/// **its own words** (theseus-sh1n3): a line Theseus composes out of its own
/// state, with nothing in it that entered the session from outside. They are
/// not items — neither judged nor counted against a model of restricted
/// origin.
///
/// The list is written out rather than inferred from a `harness:` prefix,
/// because not every node the harness writes is in its own words:
/// `harness:retraction` quotes a gist of the node it retracts, a `task:…`
/// report carries a child session's text, a `wake:…` node carries the note the
/// wake was set with, `harness:deferred` carries a task's header, and
/// `harness:end_note` names **the files a command left on disk** — paths out
/// of an archive, a clone, a download. Those are items like any other, and the
/// one that looks least like one is the one that nearly got in: the end note
/// reads as fixed asks and is not (the review of 2026-10-27, B2).
///
/// **The bar for adding an author here:** read its write site, and satisfy
/// yourself that every branch of the text it writes is a constant or is
/// composed from the harness's own numbers alone. A wrong entry is not a
/// wedge, it is a hole — unjudged outside text reaching a China-origin model.
///
/// Each entry is the write site's own constant rather than a copy of it, so a
/// rename fails to compile instead of silently ending the exemption.
pub const HARNESS_AUTHORS: [&str; 5] = [
    crate::turn::clock_step::CLOCK_AUTHOR, // "[The harness: 9 minutes left …]"
    crate::turn::progress_step::PROGRESS_AUTHOR, // the time and budget share
    crate::turn::ceiling_step::NOTE_AUTHOR, // the day's ceiling and its turnover
    crate::turn::helper_step::NOTE_AUTHOR, // its one NOTE constant
    crate::turn::nudge_step::NUDGE_AUTHOR, // its one NUDGE_TEXT constant
];

/// Whether a node is the harness's own words ([`HARNESS_AUTHORS`]). Both the
/// origin and the author must say so: an author is a string any relay carries.
pub fn harness_own(origin: Origin, author: Option<&str>) -> bool {
    origin == Origin::Harness && author.is_some_and(|a| HARNESS_AUTHORS.contains(&a))
}

/// **The one definition of an item** (theseus-sh1n3): the text the content
/// pack classifies, or `None` for a node that is no item.
///
/// Everything reads this and nothing re-states it: the blocker the chokepoint
/// and the floor share (`content_wait::is_item`, which builds the mark's span
/// too) and the submitter (`ContentItem::of_message`, `of_result`). They were
/// two statements of one rule until 2026-10-27, and they had drifted in the
/// one way that matters: a node the blocker counted and the submitter never
/// built stayed unjudged for ever, so **every** request of that session to a
/// model of restricted origin was refused for the session's whole life, in
/// silence. The turn clock's node was one such node and a `task.create` result
/// another. Keep them one function.
///
/// An item is a user-role message that is not the harness's own words
/// ([`harness_own`]), or a tool result whose text the harness did not write
/// itself ([`judges`]). A node with no text is no item: there would be nothing
/// to ask about, and counted it would stand in the mark's way for good.
pub fn item_text(node: &Node) -> Option<&str> {
    let text = match &node.body {
        Body::UserMessage { text, .. } => match harness_own(node.origin, node.author.as_deref()) {
            true => return None,
            false => text.as_str(),
        },
        Body::ToolResult { tool, content, .. } => match judges(tool) {
            true => content.as_str(),
            false => return None,
        },
        _ => return None,
    };
    (!text.trim().is_empty()).then_some(text)
}

/// One item split for the pack's state cap: `(piece, of, text)`, one-based, cut
/// at line boundaries so no line is split across pieces, and never empty — a
/// text that fits is one piece of one. A single line longer than `cap` is its
/// own piece, over the cap: cutting mid-line would hide the very thing a flag
/// is looking for across the seam.
///
/// `cap` is in characters; the caller derives it from the pack's
/// `state_cap_tokens` at four characters a token, with room for the rest of the
/// state.
pub fn pieces(text: &str, cap: usize) -> Vec<(u32, u32, String)> {
    let cap = cap.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in text.split_inclusive('\n') {
        if !cur.is_empty() && cur.chars().count() + line.chars().count() > cap {
            out.push(std::mem::take(&mut cur));
        }
        cur.push_str(line);
        if cur.chars().count() >= cap {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    let of = out.len() as u32;
    out.into_iter()
        .enumerate()
        .map(|(i, t)| (i as u32 + 1, of, t))
        .collect()
}

/// One item split for the pack, bounded: at most `max` pieces, taken from the
/// start, and whether anything was left unread.
///
/// **Why there is a bound at all, corrected 2026-10-26 by the review.** My
/// first reason for it was wrong: I justified it against `[tools]
/// job_output_max_bytes` (64 MiB), but a job's output never reaches an item —
/// the result node is capped at the tool's `result_max_chars`, and the whole
/// output goes to a file. The incident I wrote the bound to prevent cannot
/// happen. The largest item that can actually exist is an `fs.read` at
/// `FS_READ_MAX_CHARS` (100,000 characters).
///
/// So the bound's job is not to stop a runaway; it is to be large enough that
/// nothing real is ever cut. It is set so the largest possible item fits whole,
/// and Jev's price (about $0.000147 for a 3,000-token piece) makes that cheap:
/// reading one 100,000-character file in full costs about half a thousandth of
/// a dollar.
///
/// **What truncation would cost, if it ever happened:** the text past the bound
/// is not seen and the item still counts as judged. The deterministic
/// `[privacy] sensitive_names` tripwire still runs at the provider on the whole
/// request, and a job's granted secrets are withheld by the wrapper's pipe
/// (`redact.rs`) before a node exists. Marking long items unjudged instead
/// would take a session off China-origin models for good the first time it read
/// a large file, which serves nobody.
///
/// The row records `truncated`, so this is visible rather than silent.
pub fn split(text: &str, cap: usize, max: usize) -> (Vec<(u32, u32, String)>, bool) {
    let all = pieces(text, cap);
    let max = max.max(1);
    if all.len() <= max {
        return (all, false);
    }
    let of = max as u32;
    let kept = all
        .into_iter()
        .take(max)
        .map(|(p, _, t)| (p, of, t))
        .collect();
    (kept, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(flag: Flag) -> Label {
        Label {
            flag,
            node: Some("n1".into()),
            kind: ItemKind::ToolResult,
            source: "fs.read /srv/open/x".into(),
            confidence: 0.8,
            band: flag.bar(),
            at_ms: 1,
        }
    }

    /// The two bars are the design's asymmetry, and they are asserted here so
    /// no later edit can quietly level them: the three cn-disqualifying flags
    /// raise at the confirm band, injection only at act.
    #[test]
    fn the_two_bars_are_what_the_design_says() {
        assert_eq!(Flag::Injection.bar(), Band::Act);
        for f in [Flag::Secret, Flag::Confidential, Flag::Personal] {
            assert_eq!(f.bar(), Band::Confirm, "{}", f.as_str());
            assert!(f.raised_by(Band::Confirm), "{}", f.as_str());
            assert!(f.raised_by(Band::Act), "{}", f.as_str());
            assert!(f.disqualifies_cn(), "{}", f.as_str());
        }
        assert!(!Flag::Injection.raised_by(Band::Confirm), "act alone");
        assert!(Flag::Injection.raised_by(Band::Act));
        for f in Flag::ALL {
            assert!(!f.raised_by(Band::Escalate), "{}", f.as_str());
        }
        assert!(!Flag::Injection.disqualifies_cn(), "it freezes, not cn");
        for f in Flag::ALL {
            assert_eq!(Flag::parse(f.as_str()), Some(f));
        }
        assert_eq!(Flag::parse("open"), None, "no open-view word parses");
    }

    /// The cn word is the first disqualifying flag in `Flag::ALL`'s order, and
    /// injection alone disqualifies nothing; the freeze is injection alone.
    #[test]
    fn what_each_flag_does() {
        let mut f = ContentFlags::default();
        assert_eq!(f.disqualifies_cn(), None);
        assert!(!f.frozen());
        assert!(f.raise(label(Flag::Injection)));
        assert_eq!(f.disqualifies_cn(), None, "injection is not a cn word");
        assert!(f.frozen());
        assert!(f.raise(label(Flag::Personal)));
        assert_eq!(f.disqualifies_cn(), Some(Flag::Personal));
        assert!(f.raise(label(Flag::Secret)));
        assert_eq!(f.disqualifies_cn(), Some(Flag::Secret), "secret first");
        assert!(f.raise(label(Flag::Confidential)));
        assert_eq!(f.disqualifies_cn(), Some(Flag::Secret));
    }

    /// A second raise of a flag already on the session keeps the first label,
    /// so the label names the item that first brought it, and the caller writes
    /// no second row.
    #[test]
    fn a_second_raise_keeps_the_first_label() {
        let mut f = ContentFlags::default();
        assert!(f.raise(label(Flag::Secret)));
        let later = Label {
            node: Some("n9".into()),
            at_ms: 99,
            ..label(Flag::Secret)
        };
        assert!(!f.raise(later), "the second raise writes nothing");
        assert_eq!(f.label(Flag::Secret).unwrap().node.as_deref(), Some("n1"));
        assert_eq!(f.labels.len(), 1);
    }

    /// A removal takes the label and keeps it as history; removing a flag that
    /// is not there changes nothing, so the caller errors and writes nothing.
    #[test]
    fn a_removal_is_kept_as_history() {
        let mut f = ContentFlags::default();
        f.raise(label(Flag::Injection));
        assert!(f.remove(Flag::Secret, "the owner", "cli", 2).is_none());
        assert!(f.removed.is_empty(), "nothing was removed");
        let l = f.remove(Flag::Injection, "the owner", "cli", 2).unwrap();
        assert_eq!(l.flag, Flag::Injection);
        assert!(!f.frozen());
        assert_eq!(f.removed.len(), 1);
        assert_eq!(f.removed[0].by, "the owner");
    }

    /// A turn's write back can add a label and move `judged_through`, but a
    /// stored label always wins and a stale `judged_through` is dropped.
    #[test]
    fn a_write_back_never_clears_a_stored_label() {
        let stored = {
            let mut f = ContentFlags::default();
            f.raise(label(Flag::Confidential));
            f.judged_through = Some("n5".into());
            Box::new(f)
        };
        // A turn that read the session before the label, and judged further.
        let turn = ContentFlags {
            judged_through: Some("n7".into()),
            ..Default::default()
        };
        let merged = ContentFlags::merge(Some(stored.clone()), Some(&turn)).unwrap();
        assert!(merged.has(Flag::Confidential), "the stored label survives");
        assert_eq!(
            merged.judged_through.as_deref(),
            Some("n5"),
            "the turn never saw n5, so its claim to have judged through n7 would skip what \
             came between: the stored mark stands and the turn's is dropped"
        );
        // One that read the stored state: its advance is taken.
        let read: ContentFlags = serde_json::from_value(serde_json::to_value(&*stored).unwrap())
            .expect("a stored copy reads back");
        let mut turn = read.clone();
        turn.judged_through = Some("n7".into());
        let merged = ContentFlags::merge(Some(stored.clone()), Some(&turn)).unwrap();
        assert_eq!(merged.judged_through.as_deref(), Some("n7"));
        // And a turn's own new label is added beside the stored one.
        let mut turn = read;
        turn.raise(label(Flag::Secret));
        let merged = ContentFlags::merge(Some(stored), Some(&turn)).unwrap();
        assert!(merged.has(Flag::Secret) && merged.has(Flag::Confidential));
    }

    /// A record written before this held the open view's `DataState` here. It
    /// reads as no labels and nothing judged, with no migration: the first item
    /// to enter such a session judges it.
    #[test]
    fn an_open_view_data_state_reads_as_no_labels() {
        let old = serde_json::json!({
            "opened": {"how": "task", "at_ms": 1},
            "run": {"from": "n1", "through": "n4"},
            "taint": {"why": "verdict_private", "node": "n5", "at_ms": 2},
        });
        let f: ContentFlags = serde_json::from_value(old).expect("the old layout reads");
        assert_eq!(f, ContentFlags::default());
        assert_eq!(f.disqualifies_cn(), None, "a stale proof allows nothing");
        assert!(!f.frozen());
        assert_eq!(f.judged_through, None, "and nothing counts as judged");
    }

    /// The freeze refuses every class but a read, says the one remedy, and is
    /// off in `notify` — the bench daemon's mode, where a trial never waits for
    /// a tap.
    #[test]
    fn the_freeze_refuses_what_acts_and_notify_refuses_nothing() {
        let mut f = ContentFlags::default();
        f.raise(label(Flag::Injection));
        let why = refusal(
            Some(&f),
            InjectionMode::Refuse,
            ToolClass::Write,
            "fs.write",
        )
        .expect("a write is refused");
        assert!(why.contains("theseus content remove"), "{why}");
        assert!(why.contains("fs.write was not run"), "{why}");
        assert!(
            refusal(Some(&f), InjectionMode::Refuse, ToolClass::Run, "proc.run").is_some(),
            "a program is refused too"
        );
        assert_eq!(
            refusal(Some(&f), InjectionMode::Refuse, ToolClass::Read, "fs.read"),
            None,
            "a LOCAL read still runs, so the session can still be investigated"
        );
        // The hole the first draft left: these are Read-class, and a fetch of
        // `https://elsewhere.example/?x=...` is an outbound send with the data
        // in the query string. A frozen session keeps no channel off this
        // machine.
        for t in [
            "http.fetch",
            "web.search",
            "channel.post",
            "google.call",
            "aws.call",
            "drive.read",
            "mcp:server/tool",
        ] {
            assert!(
                refusal(Some(&f), InjectionMode::Refuse, ToolClass::Read, t).is_some(),
                "{t} reaches off this machine and must be refused though it reads"
            );
        }
        // `lsp.` is NOT here: starting a language server runs build scripts.
        assert!(!local_read("lsp.hover"), "a language server start is a run");
        assert!(
            refusal(
                Some(&f),
                InjectionMode::Refuse,
                ToolClass::Read,
                "lsp.hover"
            )
            .is_some(),
            "a frozen session may not start a language server"
        );
        for t in ["fs.read", "git.log", "text.grep", "tools.find", "task.read"] {
            assert!(local_read(t), "{t}");
            assert_eq!(
                refusal(Some(&f), InjectionMode::Refuse, ToolClass::Read, t),
                None,
                "{t}"
            );
        }
        // A local-read NAME at a class that acts is still refused: the name
        // alone never clears it.
        assert!(
            refusal(
                Some(&f),
                InjectionMode::Refuse,
                ToolClass::Write,
                "fs.write"
            )
            .is_some(),
            "the allowlist is a narrowing of Read, never a widening"
        );
        assert_eq!(
            refusal(
                Some(&f),
                InjectionMode::Notify,
                ToolClass::Write,
                "fs.write"
            ),
            None,
            "notify refuses nothing"
        );
        // Without the label, and with no record at all, nothing is refused.
        let other = {
            let mut o = ContentFlags::default();
            o.raise(label(Flag::Secret));
            o
        };
        for flags in [Some(&other), None] {
            assert_eq!(
                refusal(flags, InjectionMode::Refuse, ToolClass::Write, "fs.write"),
                None
            );
        }
    }

    /// The skip list is exactly the two acknowledgement results, and a tool not
    /// on it is always judged.
    #[test]
    fn only_our_own_acknowledgements_go_unjudged() {
        assert!(!judges("task.update"));
        assert!(!judges("task.close"));
        for t in [
            "fs.read",
            "http.fetch",
            "web.search",
            "proc.run",
            "fs.write",
        ] {
            assert!(judges(t), "{t}");
        }
    }

    /// A user-role node as its write site builds one.
    fn msg(origin: Origin, author: &str, text: &str) -> Node {
        Node::relayed("ses_1", Some("trn_1"), origin, author, text)
    }

    /// A tool result node as a completion's frame builds one.
    fn result(tool: &str, content: &str) -> Node {
        Node::tool_result(
            "ses_1",
            Some("trn_1"),
            None,
            Body::ToolResult {
                tool_use_id: "tu_1".into(),
                tool: tool.into(),
                status: crate::node::ResultStatus::Ok,
                is_error: false,
                content: content.into(),
                correlation_id: None,
                bytes_total: 0,
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta: serde_json::Value::Null,
                image: None,
                external: None,
            },
        )
    }

    /// Judged: it came from outside the session, or it carries what did.
    fn judged_kinds() -> Vec<(&'static str, Node)> {
        vec![
            ("the owner's message", msg(Origin::Operator, "cli", "hello")),
            ("another person's", msg(Origin::Operator, "web#1", "hello")),
            ("an MCP prompt's", msg(Origin::Mcp, "prompt:s/p", "do this")),
            ("a task's brief", msg(Origin::Agent, "task:ab12", "find it")),
            (
                "a task's report, which carries a child session's text",
                msg(Origin::Harness, "task:ab12", "I found it"),
            ),
            (
                "a wake's note, set earlier by whoever set it",
                msg(Origin::Harness, "wake:cd34", "⏰ wake: ring the bell"),
            ),
            (
                "a retraction, which quotes a gist of the node it retracts",
                msg(Origin::Harness, "harness:retraction", "⏮ …«a gist»"),
            ),
            ("a thread's note", msg(Origin::Harness, "thread", "moved")),
            (
                "a task's deferred header",
                msg(Origin::Harness, "harness:deferred", "⏳ deferred: …"),
            ),
            (
                "the end note, which NAMES THE FILES a command left on disk \
                 — paths out of an archive, a clone, a download. It reads as \
                 fixed asks and is not (the review of 2026-10-27, B2)",
                msg(
                    Origin::Harness,
                    "harness:end_note",
                    "[The harness: this turn would end here. Left beside its \
                     edits: ./acme-merger-term-sheet/draft.pdf.]",
                ),
            ),
            (
                "a harness author under any other origin: the author is a \
                 string a relay carries, so it is claimed, not proved",
                msg(Origin::Operator, "harness:clock", "[The harness: …]"),
            ),
            ("a file read", result("fs.read", "the file's bytes")),
            ("a fetched page", result("http.fetch", "<html>…")),
            ("a program's output", result("proc.run", "the output")),
            (
                "a task.create result — a harness tool's, judged like any other",
                result("task.create", "Started task ab12"),
            ),
        ]
    }

    /// Not items: the harness's own words, our own fixed acknowledgements, a
    /// node with no text, and everything that is no message or result.
    fn exempt_kinds() -> Vec<(&'static str, Node)> {
        let sid = "ses_1";
        vec![
            (
                "the turn clock, which wedged every cn session with a deadline",
                msg(Origin::Harness, "harness:clock", "[The harness: 9 left.]"),
            ),
            (
                "the progress line",
                msg(Origin::Harness, "harness:progress", "[The harness: …]"),
            ),
            (
                "the day's ceiling",
                msg(Origin::Harness, "harness:day_ceiling", "⛔ day 3: …"),
            ),
            (
                "a helper's budget note",
                msg(Origin::Harness, "harness:helper", "[The harness: …]"),
            ),
            (
                "the stop nudge",
                msg(Origin::Harness, "harness:stop_nudge", "[The harness: …]"),
            ),
            ("task.update's acknowledgement", result("task.update", "ok")),
            ("task.close's", result("task.close", "ok")),
            ("a message with no text", msg(Origin::Operator, "cli", "  ")),
            ("a result with no text", result("fs.read", "")),
            (
                "a model's own words",
                Node::assistant(
                    sid,
                    "trn_1",
                    0,
                    crate::node::Body::AssistantMessage {
                        blocks: vec![serde_json::json!({"type": "text", "text": "mine"})],
                        model: "m".into(),
                        provider: "p".into(),
                        stop_reason: None,
                        usage: Default::default(),
                        cost_usd: None,
                        catalog_version: None,
                        request_id: None,
                        correlation_id: None,
                        compilation_id: None,
                        request_digest: None,
                        effort: None,
                    },
                ),
            ),
            (
                "a recall's note",
                Node::recall(sid, "trn_1", "rcl_1", "baseline", vec![]),
            ),
        ]
    }

    /// Everything that entered the session from outside is an item: it is
    /// judged, and until it is, it keeps the session off a model of
    /// restricted origin (theseus-sh1n3). One function decides
    /// ([`item_text`]); this is the written record of what it decides and
    /// why, so a kind added later is decided rather than inherited.
    #[test]
    fn everything_from_outside_the_session_is_an_item() {
        for (why, n) in &judged_kinds() {
            assert!(item_text(n).is_some(), "an item: {why}");
        }
    }

    /// The other side of the same decision: the harness's own words are no
    /// item, nor is our own fixed acknowledgement of a tool, a node with no
    /// text, or anything that is neither a message nor a result. A node
    /// wrongly counted here is one the judge is never asked about, so
    /// `judged_through` can never pass it and the session is off every
    /// restricted-origin model for good — the clock node, until this row.
    #[test]
    fn the_harness_own_words_are_no_item() {
        for (why, n) in &exempt_kinds() {
            assert_eq!(item_text(n), None, "no item: {why}");
        }
        // Every author on the exempt list is reachable by it, so none rots.
        for a in HARNESS_AUTHORS {
            assert!(harness_own(Origin::Harness, Some(a)), "{a}");
            assert!(!harness_own(Origin::Agent, Some(a)), "{a} under agent");
        }
    }

    /// A long item splits at line boundaries, each piece knows its place, the
    /// pieces rejoin to the whole, and a single line past the cap is its own
    /// piece rather than being cut through the middle.
    #[test]
    fn a_long_item_splits_at_line_boundaries() {
        let text = "alpha\nbravo\ncharlie\ndelta\n";
        let one = pieces(text, 1000);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0], (1, 1, text.to_string()));
        let many = pieces(text, 12);
        assert!(many.len() > 1, "{many:?}");
        let of = many.len() as u32;
        for (i, (p, o, _)) in many.iter().enumerate() {
            assert_eq!((*p, *o), (i as u32 + 1, of));
        }
        let joined: String = many.iter().map(|(_, _, t)| t.as_str()).collect();
        assert_eq!(joined, text, "the pieces are the whole");
        for (_, _, t) in &many {
            assert!(t.ends_with('\n') || t == "delta\n", "cut at a line: {t:?}");
        }
        let long = format!("{}\nshort\n", "x".repeat(50));
        let split = pieces(&long, 10);
        assert!(
            split.iter().any(|(_, _, t)| t.chars().count() > 10),
            "a line past the cap is its own piece: {split:?}"
        );
        assert_eq!(
            split.iter().map(|(_, _, t)| t.as_str()).collect::<String>(),
            long
        );
        // Empty text is one empty piece, never no pieces at all.
        assert_eq!(pieces("", 10), vec![(1, 1, String::new())]);
    }

    /// The bound on pieces: a runaway result is read from the start, up to the
    /// bound, and says so. Under the bound nothing is dropped and nothing is
    /// marked truncated.
    #[test]
    fn a_runaway_item_is_bounded_and_says_so() {
        let short = "a\nb\nc\n";
        let (p, cut) = split(short, 2, 6);
        assert!(!cut, "under the bound nothing is truncated");
        assert_eq!(p.len(), pieces(short, 2).len());

        let long: String = (0..500).map(|i| format!("line {i}\n")).collect();
        let (p, cut) = split(&long, 20, 6);
        assert!(cut, "past the bound it says so");
        assert_eq!(p.len(), 6, "at most the bound, from the start");
        for (i, (n, of, _)) in p.iter().enumerate() {
            assert_eq!((*n, *of), (i as u32 + 1, 6), "each piece knows its place");
        }
        assert!(p[0].2.starts_with("line 0\n"), "read from the start");
        // A bound of zero is read as one: an item is never judged on nothing.
        let (p, cut) = split(&long, 20, 0);
        assert_eq!((p.len(), cut), (1, true));
    }

    /// Exactly two kinds are exempt from the injection question, for two
    /// different reasons, and nothing else is. In particular another person's
    /// message in a shared place IS asked — the owner's rule is about his own
    /// text, and a guest is exactly who an injection would come from — and a
    /// task's REPORT is asked though its BRIEF is not.
    #[test]
    fn only_the_owners_words_and_a_brief_escape_the_injection_question() {
        assert!(
            !ItemKind::OperatorMessage.asks_injection(),
            "the owner's own"
        );
        assert!(
            !ItemKind::TaskBrief.asks_injection(),
            "a brief is our own parent instructing its child: steering by design, \
             so the question is a guaranteed false positive"
        );
        for k in [
            ItemKind::PersonMessage,
            ItemKind::ToolResult,
            ItemKind::FetchedPage,
            ItemKind::SearchResult,
            ItemKind::TaskReport,
        ] {
            assert!(k.asks_injection(), "{}", k.as_str());
        }
        // Every kind round-trips by its word, so a row is readable.
        for k in [
            ItemKind::OperatorMessage,
            ItemKind::PersonMessage,
            ItemKind::ToolResult,
            ItemKind::FetchedPage,
            ItemKind::SearchResult,
            ItemKind::TaskBrief,
            ItemKind::TaskReport,
        ] {
            let w = serde_json::to_string(&k).unwrap();
            assert_eq!(w, format!("\"{}\"", k.as_str()));
        }
    }

    /// The mark moves only over a span every node of which was judged, and the
    /// span comes from the transcript — so the guarantee survives a restart
    /// with no in-memory state at all. The restart case is the one that
    /// matters: the process forgets which item it gave up on, and the
    /// transcript remembers for it.
    #[test]
    fn the_mark_never_carries_over_an_item_nothing_judged_even_after_a_restart() {
        let n = |s: &str| s.to_string();
        let mut f = ContentFlags::default();
        // n1 judged.
        assert!(f.judged_run(&[n("n1")], &[n("n1")], "n1"));
        assert_eq!(f.judged_through.as_deref(), Some("n1"));

        // n2 could not be judged; n3 was. The span from the mark is [n2, n3],
        // and n2 was not judged, so the mark stays.
        f.could_not_judge("n2");
        f.could_not_judge("n2");
        assert_eq!(f.unjudged, ["n2"], "idempotent");
        assert!(!f.judged_run(&[n("n2"), n("n3")], &[n("n3")], "n3"));
        assert_eq!(f.judged_through.as_deref(), Some("n1"));

        // THE RESTART. Everything the judging process knew is gone; only what
        // was stored comes back. A fresh landing judges only the newest item,
        // n5 — and the span from the mark still holds n2, n3, n4.
        let mut back: ContentFlags =
            serde_json::from_value(serde_json::to_value(&f).unwrap()).expect("round trip");
        assert!(!back.judged_run(&[n("n2"), n("n3"), n("n4"), n("n5")], &[n("n5")], "n5"));
        assert_eq!(
            back.judged_through.as_deref(),
            Some("n1"),
            "the transcript remembers the skipped item for the process that forgot it"
        );

        // A re-ask that covers the whole span moves the mark and clears the record.
        assert!(back.judged_run(
            &[n("n2"), n("n3"), n("n4"), n("n5")],
            &[n("n2"), n("n3"), n("n4"), n("n5")],
            "n5"
        ));
        assert_eq!(back.judged_through.as_deref(), Some("n5"));
        assert!(back.unjudged.is_empty(), "the re-ask cleared the record");

        // Malformed spans move nothing: empty, or not ending at `node`.
        let before = back.judged_through.clone();
        assert!(!back.judged_run(&[], &[], "n6"));
        assert!(!back.judged_run(&[n("n6"), n("n7")], &[n("n6"), n("n7")], "n6"));
        assert_eq!(back.judged_through, before);

        // A turn's write-back carries a new record up, and never drops a stored one.
        let stored = Box::new(back.clone());
        let mut turn = back;
        turn.could_not_judge("n9");
        let merged = ContentFlags::merge(Some(stored), Some(&turn)).unwrap();
        assert_eq!(merged.unjudged, ["n9"]);
    }

    /// Its line for a person, with and without labels.
    #[test]
    fn the_line_says_the_labels_and_how_far_it_is_judged() {
        let f = ContentFlags::default();
        assert_eq!(f.line(), "no labels; nothing judged");
        let mut f = f;
        f.raise(label(Flag::Secret));
        f.raise(label(Flag::Injection));
        f.judged_through = Some("n4".into());
        assert_eq!(f.line(), "injection, secret; judged to n4");
    }
}
