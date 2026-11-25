//! The yes to a changed prompt file (issue 23): the daemon's own write.
//!
//! Nothing is written unless everything is as the card showed it: the second
//! proof was asked, the settings still name the file, the file is still a
//! regular file with no link at its end, and it is byte for byte the file the
//! card was built from (one edited since is refused, and the owner asks
//! again). Then the text it replaces is kept as a numbered copy and the new
//! text is written in one step: a temporary file in the same directory, synced,
//! given the old file's permissions, renamed over it, the directory synced.
//!
//! The owner's yes to a context file's change is also the accept of the text
//! written (`Core::held_written`): what Theseus wrote at his asking is the
//! version carried from the next request, with no second accept.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde_json::Value;
use theseus_kernel::Action;

use super::{put, read_text, resolve, KIND};
use crate::config_change::write::{self, Kept};
use crate::config_copy::sha256;
use crate::rpc::Core;

/// Whether `args` are a prompt file's card.
pub fn is_card(args: &Value) -> bool {
    args["kind"].as_str() == Some(KIND)
}

impl Core {
    /// Write the text the card named over the file it was built from, or say
    /// why not, and keep the text written as the accepted version of a
    /// context file, by `by` through `via`. The copy number it is kept as,
    /// the digest of the file before and the digest after.
    pub(crate) fn write_prompt_file(
        &self,
        a: &Action,
        args: &Value,
        by: &str,
        via: &str,
    ) -> Result<(u32, String, String), String> {
        let state = &self.tools.config_change.state;
        let path = PathBuf::from(args["file"]["path"].as_str().unwrap_or_default());
        let shown = path.display().to_string();
        let named = resolve(self, &shown).map_err(|why| {
            format!(
                "the file can no longer be changed as prompt text, so nothing was written: {why}"
            )
        })?;
        let now = read_text(&path).map_err(|e| format!("{shown}: {e}, so nothing was written"))?;
        let want = args["base_sha256"].as_str().unwrap_or_default();
        if sha256(&now) != want {
            return Err(
                "the file changed after this card was made, so nothing was written: ask \
                        for the change again"
                    .into(),
            );
        }
        let new = write::read_kept(
            state,
            Kept::FileNew,
            args["candidate_sha256"].as_str().unwrap_or_default(),
        )?;
        if let Some(why) =
            put::refusal(self, &named, &now, &new).or_else(|| put::held_base(self, &named, &now))
        {
            return Err(format!("{why}, so nothing was written"));
        }
        let (previous, _) = write::keep_previous_file(state, &now).map_err(|e| {
            format!("the previous text could not be kept, so nothing was written: {e}")
        })?;
        let mode = std::fs::metadata(&path)
            .map(|m| m.permissions().mode() & 0o7777)
            .map_err(|e| format!("{shown} cannot be read: {e}"))?;
        write::atomic(&path, &new, Some(mode))
            .map_err(|e| format!("the file could not be written: {e}"))?;
        self.held_written(&named.path, &new, by, via);
        Ok((previous, sha256(&now), sha256(&new)))
    }
}

/// Forget the texts a settled question kept.
pub fn drop_kept(state: &std::path::Path, args: &Value) {
    let sha = |k: &str| args[k].as_str().unwrap_or_default().to_string();
    if is_card(args) {
        write::drop_kept(state, Kept::FileBase, &sha("base_sha256"));
        write::drop_kept(state, Kept::FileNew, &sha("candidate_sha256"));
    } else if args["base_kept"].as_bool() == Some(true) {
        write::drop_kept(state, Kept::SettingsBase, &sha("base_sha256"));
    }
}
