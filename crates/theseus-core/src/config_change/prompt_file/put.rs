//! Put a prompt file's change to the owner as one card (issue 23). Both the
//! proposal (`tools.rs`) and an undo (`undo.rs`) come here and nowhere else, so
//! an undo is held to everything a proposal is.

use serde_json::{json, Value};
use theseus_kernel::Proposal;

use super::{card, Named, KIND, MAX_FILE_BYTES};
use crate::config_change::proof_seam::Needs;
use crate::config_change::shown;
use crate::config_change::undo::Undoes;
use crate::config_change::write::{self, Kept};
use crate::rpc::Core;
use crate::toolrun::{ToolRuntime, TurnCtx};

/// What is asked.
pub struct Ask<'a> {
    pub named: &'a Named,
    /// The file as it is now.
    pub before: &'a str,
    /// The text it would become.
    pub after: &'a str,
    pub reason: &'a str,
    /// An undo: what it undoes, and the card's lines about it.
    pub undoes: Option<(&'a Undoes, String)>,
}

/// Why `after` may not be the text of `named`, or `None`: nothing changes, it
/// is larger than a prompt file is, or a persona's soul over its cap grows.
pub fn refusal(core: &Core, named: &Named, before: &str, after: &str) -> Option<String> {
    if before == after {
        return Some("that would change nothing: the file already says that".into());
    }
    if after.len() > MAX_FILE_BYTES {
        return Some(format!(
            "the file would be {} KiB, more than the {} KiB a prompt file may be: the owner edits \
             it by hand",
            after.len().div_ceil(1024),
            MAX_FILE_BYTES / 1024
        ));
    }
    let cap = core.cfg.context.persona_cap();
    let tokens = crate::personas::soul::tokens(after);
    // A soul already over its cap is carried whole; it may shrink, never grow.
    (named.is_persona() && tokens > cap && after.len() > before.len()).then(|| {
        format!(
            "that makes the persona's file {tokens} tokens, over the cap of {cap} ([context] \
             persona_max_tokens): a persona is one small file, so trim the text"
        )
    })
}

/// Why a change may not be asked, or written, on `before`, the text of
/// `named`: it is a context file and `before` is not its accepted version
/// (`held::base_refusal`). Asked at the proposal (and an undo's) and again at
/// the yes.
pub fn held_base(core: &Core, named: &Named, before: &str) -> Option<String> {
    crate::project_context::held::base_refusal(
        &core.cfg,
        &core.store,
        &core.runner.context_files,
        &named.path,
        before,
    )
}

/// Build the card, keep the two texts for it, and ask the owner.
pub fn put_file(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    core: &Core,
    correlation_id: &str,
    a: Ask<'_>,
) -> Result<(String, Value), String> {
    if let Some(why) = refusal(core, a.named, a.before, a.after) {
        return Err(why);
    }
    if let Some(why) = held_base(core, a.named, a.before) {
        return Err(why);
    }
    let w = &rt.config_change;
    let (before, after) = (shown::facts(a.before), shown::facts(a.after));
    let diff = shown::diff(a.before, a.after);
    let (outside, steered) = crate::config_change::outside_text(tc);
    let path = a.named.path.display().to_string();
    let who = a.named.words();
    let (question, whole) = card::question(&card::Facts {
        path: &path,
        who: &who,
        public: a.named.public,
        before: &before,
        after: &after,
        diff: &diff,
        outside_text: outside || steered,
        steered,
        reason: a.reason,
        preface: a.undoes.as_ref().map(|(_, p)| p.as_str()),
        undoing: a.undoes.as_ref().map(|(u, _)| u.number),
    });
    let kept = |kind, text: &str| {
        write::keep_text(&w.state, kind, text)
            .map_err(|e| format!("the text could not be kept for the card: {e}"))
    };
    let base_sha = kept(Kept::FileBase, a.before)?;
    let new_sha = kept(Kept::FileNew, a.after)?;
    let facts = |f: &shown::Facts| json!({"bytes": f.bytes, "lines": f.lines, "fingerprint": f.fingerprint});
    let proposal = Proposal {
        tool: crate::config_change::ACK.into(),
        args: json!({
            "question": question,
            "settled": crate::config_change::words::file(
                &path,
                diff.added,
                diff.removed,
                a.undoes.as_ref().map(|(u, _)| u.number),
            ),
            "kind": KIND,
            "whole": true,
            "file": {
                "path": path, "who": who, "before": facts(&before), "after": facts(&after),
                "added": diff.added, "removed": diff.removed, "shown_whole": whole,
            },
            "base_sha256": base_sha,
            "candidate_sha256": new_sha,
            "paths": Vec::<String>::new(),
            "restart": false,
            "undoes": a.undoes.as_ref().map(|(u, _)| u.args()),
            "needs_fresh_proof": false,
            "reason": a.reason,
            "outside_text": outside || steered,
            "asked_by": {
                "session_id": tc.session_id,
                "execution_id": tc.execution_id,
                "turn_id": tc.turn_id,
                "place": tc.outbox.target(tc.session_id),
            },
        }),
        resource: Some("prompt-file".into()),
        policy_context: json!({}),
    };
    let said = format!(
        "Asked the owner:\n{question}Nothing changes until they say yes, from a private place; \
         the daemon then writes the file itself. If they decline, or do not answer in time, \
         nothing is written.\n{}\n{}",
        crate::config_change::waiting::FILE_CHECKED,
        crate::config_change::waiting::waiting(tc)
    );
    crate::config_change::ask(rt, tc, proposal, correlation_id, &said, Needs::Card)?;
    Ok((
        said,
        json!({"file": path, "candidate": new_sha, "shown_whole": whole}),
    ))
}
