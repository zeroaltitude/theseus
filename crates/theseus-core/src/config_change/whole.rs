//! The whole text of a change waiting for the owner's yes (issue 23): what
//! `theseus config show` and the cockpit print, so that nothing is said yes
//! to that was only summarised.
//!
//! Built by the daemon from the texts it kept when it made the card (the
//! prompt file as the card was built from it, the text it would become; for
//! a setting too long for its card, the settings file as it was and the
//! candidate), every line of each, with the characters that cannot be seen
//! written out and counted. Never from what the model said.

use serde_json::Value;
use theseus_kernel::Action;

use super::write::{self, Kept};
use super::{diff, shown, ACK};
use crate::config_copy::sha256;
use crate::rpc::Core;

fn args_of(a: &Action) -> Value {
    a.proposal
        .as_ref()
        .map(|p| p.args.clone())
        .unwrap_or_default()
}

/// The questions waiting whose card has a text too long to show on it.
fn waiting(core: &Core) -> Vec<Action> {
    core.kernel
        .pending_confirms()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.tool == ACK && args_of(a)["whole"].as_bool() == Some(true))
        .collect()
}

/// The question named by `id` (exactly, or by a prefix that names one), or the
/// only one waiting.
fn pick(mut all: Vec<Action>, id: Option<&str>) -> Result<Action, String> {
    match (id.map(str::trim).filter(|i| !i.is_empty()), all.len()) {
        (Some(i), _) => {
            all.retain(|a| a.correlation_id == i || a.correlation_id.starts_with(i));
            match all.len() {
                1 => Ok(all.remove(0)),
                0 => Err(format!("no change waiting for your yes has the id {i}")),
                _ => Err(format!(
                    "{i} names more than one waiting change: give more of its id"
                )),
            }
        }
        (None, 0) => Err("no change waiting for your yes has a long text to read".into()),
        (None, 1) => Ok(all.remove(0)),
        (None, _) => {
            let ids: Vec<String> = all.iter().map(|a| a.correlation_id.clone()).collect();
            Err(format!(
                "more than one change is waiting: name one by its id: {}",
                ids.join(", ")
            ))
        }
    }
}

/// The whole of a prompt file's change.
fn file_words(core: &Core, args: &Value) -> Result<String, String> {
    let state = &core.tools.config_change.state;
    let sha = |k: &str| args[k].as_str().unwrap_or_default();
    let before = write::read_kept(state, Kept::FileBase, sha("base_sha256"))?;
    let after = write::read_kept(state, Kept::FileNew, sha("candidate_sha256"))?;
    let path = args["file"]["path"].as_str().unwrap_or_default();
    let now = match super::prompt_file::read_text(std::path::Path::new(path)) {
        Ok(t) if sha256(&t) == sha("base_sha256") => "It has not changed since the card was made.",
        Ok(_) => "THE FILE HAS CHANGED since the card was made: a yes would write nothing.",
        Err(_) => "The file cannot be read now: a yes would write nothing.",
    };
    Ok(format!(
        "The prompt file {}\n{now}\n\n{}\n{}",
        shown::escape_line(path).0,
        shown::whole("before (the file as it is now)", &before),
        shown::whole("after (what the file would become)", &after),
    ))
}

/// The whole of each value too long for its card, in a settings change.
fn settings_words(core: &Core, args: &Value) -> Result<String, String> {
    let state = &core.tools.config_change.state;
    let sha = |k: &str| args[k].as_str().unwrap_or_default();
    let base = write::read_kept(state, Kept::SettingsBase, sha("base_sha256"))?;
    let candidate = write::read_candidate(state, sha("candidate_sha256"))?;
    Ok(super::long_value::whole(&diff::between(&base, &candidate)?))
}

/// The question shown and its words, for `id` or the only change waiting.
pub fn words(core: &Core, id: Option<&str>) -> Result<(String, String), String> {
    let a = pick(waiting(core), id)?;
    let args = args_of(&a);
    let words = match super::prompt_file::write::is_card(&args) {
        true => file_words(core, &args)?,
        false => settings_words(core, &args)?,
    };
    Ok((a.correlation_id, words))
}
