//! `--user`'s token file (theseus-w1nf): the file the unit's `--op-token-file`
//! names, judged by what `stat` reports and nothing else. It is never opened,
//! so no byte of the token can reach a plan, an error, or a log.
//!
//! The daemon refuses a token file that group or others can read, and an empty
//! one, and a unit that cannot read its own token never starts. So the plan says
//! what is wrong and how to fix it before the unit is written, and `--apply`
//! refuses until it is right.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::Result;

use super::host::Host;
use super::layout::{present, real, Act};
use super::shell_word;

/// The act on a token file: `InPlace` when it is as the unit needs it (a
/// regular file, the operator's, mode 0600 or stricter, and not empty), else
/// a `Refuse` that says what is wrong and what to run.
pub(crate) fn inspect(
    root: &Path,
    host: &dyn Host,
    path: &Path,
    (uid, name): (u32, &str),
) -> Result<Act> {
    let at = real(root, path);
    let word = shell_word(&path.to_string_lossy());
    let Some(m) = present(&at)? else {
        return Ok(refuse(
            &["it does not exist".into()],
            &[
                "make it with mode 0600, holding the 1Password service-account token \
               (docs/user-service.md shows how)"
                    .into(),
            ],
        ));
    };
    if m.file_type().is_symlink() || !m.is_file() {
        return Ok(refuse(
            &["it is not a regular file (a symlink, a directory, or a device)".into()],
            &["name the file itself".into()],
        ));
    }
    let (mut wrong, mut fix) = (vec![], vec![]);
    let (have, _) = host.owner(&at)?;
    if have != uid {
        let owner = host.user_by_uid(have)?.map_or_else(
            || format!("uid {have}"),
            |u| format!("{} (uid {have})", u.name),
        );
        wrong.push(format!("it is owned by {owner}, not by you ({name})"));
        fix.push(format!("sudo chown {name} {word}"));
    }
    let mode = m.permissions().mode() & 0o7777;
    if mode & !0o600 != 0 {
        wrong.push(format!("mode {mode:04o} is looser than 0600"));
        fix.push(format!("chmod 600 {word}"));
    } else if mode & 0o400 == 0 {
        wrong.push(format!("mode {mode:04o} does not let you read it"));
        fix.push(format!("chmod 600 {word}"));
    }
    if m.len() == 0 {
        wrong.push("it is empty".into());
        fix.push("put the 1Password service-account token in it".into());
    }
    Ok(if wrong.is_empty() {
        Act::InPlace
    } else {
        refuse(&wrong, &fix)
    })
}

fn refuse(wrong: &[String], fix: &[String]) -> Act {
    Act::Refuse(format!("{}. Fix: {}", wrong.join("; "), fix.join("; ")))
}
