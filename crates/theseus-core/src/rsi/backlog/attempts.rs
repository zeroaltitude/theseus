//! A cap on attempts per item (issue 6, the loop's failure handling, conditions 4 and 5).
//!
//! In one night two items made eight red builds or rejections, and each was announced. An
//! item now has [`MAX_ATTEMPTS`] tries; the try that reaches the cap
//! moves the item to the failed list with every try's reason, says so in **one** notice, and
//! waits for the owner's `theseus self retry`. A red try inside the cap says nothing on its
//! own account: the night's or the morning's note lists it beside the starts.
//!
//! **A try** is read from the item's own evidence, so no stored record gains a field:
//!
//! - a gate that ran and ended red (`gate:red`);
//! - a review that rejected its branch (`review:<branch>@<head>`);
//! - a build that ended with no green branch and ran no gate at all (`ended:no-branch` with
//!   no `gate:` since its `session:` began), so a build that died before its gate counts too.
//!
//! A build that never started (main red, no room, the kill switch) is not a try, and neither
//! is formatting (`rsi::tree_fmt`: an unformatted change is formatted before the gate).
//!
//! **The count follows the cause, not the item.** A failed item's cause files again the
//! next night as a new item, so the tries of every item of one cause count together, from
//! the owner's last `retried:owner` on any of them. A cause at the cap is left out of the
//! night's filing (as a dropped one is, but only until the owner's retry).

use theseus_protocol::tasks::{TaskEvidence, TaskRecord};

/// How many tries an item gets. A constant and not a `[self]` setting: the `[self]` config is a
/// file the keel guards, so a setting would need the owner's yes for its edit, and the number
/// is one line to change here.
pub const MAX_ATTEMPTS: u32 = 3;

/// The evidence a capped item carries: the one notice was made, and the item waits.
pub const CAPPED: &str = "failed:cap";
/// The evidence the owner's retry leaves; tries before it no longer count.
pub const RETRIED: &str = "retried:owner";
/// The evidence that follows a gate's own when the gate never judged the code (`infra`): the try
/// it would have been is taken back.
pub const UNJUDGED: &str = "gate:unjudged";

/// One try that did not end green.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Try {
    pub at_ms: u64,
    /// Why it failed, in the words the evidence has.
    pub why: String,
}

/// How long a red notice for an item holds the next ones back: one night.
pub const NIGHT_MS: u64 = 12 * 3_600_000;

/// Whether `t` already had a red gate within the night, so its next red is not announced on
/// its own account (issue 6, condition 5): the note lists it, and the cap's notice ends it.
pub fn red_tonight(t: &TaskRecord) -> bool {
    let now = theseus_protocol::now_unix_ms();
    t.evidence
        .iter()
        .any(|e| e.identity == "gate:red" && now.saturating_sub(e.at_ms) < NIGHT_MS)
}

/// When the owner last retried any of `tasks` (0 when never).
pub fn last_retry<'a>(tasks: impl IntoIterator<Item = &'a TaskRecord>) -> u64 {
    tasks
        .into_iter()
        .flat_map(|t| &t.evidence)
        .filter(|e| e.identity == RETRIED)
        .map(|e| e.at_ms)
        .max()
        .unwrap_or(0)
}

/// The tries one item's evidence holds after `since_ms`, oldest first.
pub fn tries(t: &TaskRecord, since_ms: u64) -> Vec<Try> {
    let mut out = Vec::new();
    // Whether a gate ran since the build's session began.
    let mut gated = false;
    // Whether the evidence just before this was a `gate:red`, which an `UNJUDGED` takes back.
    let mut after_red = false;
    for e in t.evidence.iter().filter(|e| e.at_ms > since_ms) {
        let id = e.identity.as_str();
        if id.starts_with("session:") {
            gated = false;
        } else if id.starts_with("gate:") {
            gated = true;
        }
        // A gate that never judged the code: the try just before it was no try.
        if id == UNJUDGED {
            if after_red {
                out.pop();
            }
            after_red = false;
            continue;
        }
        after_red = id == "gate:red";
        let why = match id {
            "gate:red" => Some(or(e, "its gate ended red")),
            "ended:no-branch" if !gated => Some(or(e, "its build ended with no green branch")),
            _ if id.starts_with("review:") => Some(or(e, "a review rejected its branch")),
            _ => None,
        };
        if let Some(why) = why {
            out.push(Try {
                at_ms: e.at_ms,
                why,
            });
        }
    }
    out
}

fn or(e: &TaskEvidence, fallback: &str) -> String {
    e.note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

/// The tries of every item of one cause, from the owner's last retry on any of them.
pub fn of_cause(items: &[&TaskRecord]) -> Vec<Try> {
    let since = last_retry(items.iter().copied());
    let mut all: Vec<Try> = items.iter().flat_map(|t| tries(t, since)).collect();
    all.sort_by_key(|t| t.at_ms);
    all
}

/// Whether the item already says it was capped, since the owner's last retry.
pub fn capped(t: &TaskRecord) -> bool {
    let since = last_retry([t]);
    t.evidence
        .iter()
        .any(|e| e.identity == CAPPED && e.at_ms > since)
}

/// The one notice for a capped item: what it was, how many tries, each try's reason, and the
/// way back. Plain words; nothing waits for an answer.
pub fn notice(item: &str, title: &str, tries: &[Try], cap: u32) -> String {
    use crate::owner_note::{note, Kind};
    let mut body = format!(
        "\"{title}\" did not work out after {} tries, so Theseus stopped building it. \
         Nothing waits for you.\nWhat went wrong each time:\n",
        tries.len().max(cap as usize)
    );
    let cuts = crate::scrub::Cuts::live();
    for (i, t) in tries.iter().enumerate() {
        let (why, _) = crate::scrub::safe::head_with(&cuts, &t.why, 300);
        body.push_str(&format!("- try {}: {why}\n", i + 1));
    }
    body.push_str(&format!(
        "To let it try again: `{}`",
        theseus_protocol::rsi::retry_command(item)
    ));
    note(
        Kind::Fyi,
        "Theseus stopped trying an item",
        "its tries ran out",
        &body,
    )
}

/// The evidence that marks the cap, with every try's reason in its note.
pub fn marker(tries: &[Try], now: u64) -> TaskEvidence {
    TaskEvidence {
        node: None,
        identity: CAPPED.into(),
        note: Some(format!(
            "failed after {} tries: {}",
            tries.len(),
            tries
                .iter()
                .map(|t| t.why.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        )),
        by: "self-pipeline".into(),
        at_ms: now,
    }
}
