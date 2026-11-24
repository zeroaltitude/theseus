//! `theseus own-file`: your own instruction file (`<state dir>/AGENTS.md`).
//! Theseus carries the version you last accepted. `own-file` shows what is
//! accepted, what is on disk, and the change; `own-file accept` shows the
//! same and asks, at your terminal, whether to accept what is on disk. It
//! accepts the digest you were shown, so a file that changes after you
//! looked is refused and nothing is accepted.

use anyhow::{bail, Result};
use clap::Subcommand;
use theseus_protocol::method;
use theseus_protocol::own_file::{
    OwnFileAcceptParams, OwnFileAcceptResult, OwnFileShowParams, OwnFileShowResult, OwnFileState,
};

use theseus_client::Conn;

#[derive(Debug, Clone, Subcommand)]
pub enum OwnFileCmd {
    /// Show the change, then ask "Accept this version? [y/N]" at your terminal. With no terminal it
    /// accepts nothing. Yours alone (refused inside a job).
    Accept,
}

pub async fn run(c: &mut Conn, json: bool, cmd: Option<OwnFileCmd>) -> Result<()> {
    let accept = matches!(cmd, Some(OwnFileCmd::Accept));
    if accept {
        // Before anything is shown or asked: a job's shell gets no question.
        theseus_client::client::refuse_in_a_job(
            method::OWN_FILE_ACCEPT,
            theseus_client::client::job_session().as_deref(),
        )?;
    }
    let v = c
        .request(method::OWN_FILE_SHOW, OwnFileShowParams {})
        .await?;
    if !accept {
        return crate::cmd::output(json, v, |r: OwnFileShowResult| {
            print!("{}", shown(&r, true));
            Ok(())
        });
    }
    let r: OwnFileShowResult = serde_json::from_value(v)?;
    use std::io::{IsTerminal as _, Write as _};
    if json || !std::io::stdin().is_terminal() {
        bail!(
            "{} asks at your terminal, and there is none here (or the output is JSON): nothing was \
             accepted",
            theseus_protocol::own_file::accept_command()
        );
    }
    print!("{}", shown(&r, false));
    if matches!(r.state, OwnFileState::Same | OwnFileState::Unreadable) {
        println!("Nothing to accept.");
        return Ok(());
    }
    eprint!("Accept this version? [y/N] ");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    if !said_yes(&line) {
        println!("Nothing was accepted.");
        return Ok(());
    }
    let v = c
        .request(
            method::OWN_FILE_ACCEPT,
            OwnFileAcceptParams {
                digest: r.digest.clone(),
                ..Default::default()
            },
        )
        .await?;
    let done: OwnFileAcceptResult = serde_json::from_value(v)?;
    println!("{}", accepted(&done));
    Ok(())
}

fn said_yes(line: &str) -> bool {
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// What the show prints: the file, where it stands, and the change from the
/// accepted version to the file on disk. `hint` adds the accept's command
/// when there is something to accept.
pub fn shown(r: &OwnFileShowResult, hint: bool) -> String {
    let mut out = format!("Your own instruction file: {}\n", r.path);
    let on = r
        .accepted
        .as_ref()
        .map(|a| a.on.as_str())
        .unwrap_or_default();
    let seq = r.accepted.as_ref().map_or(0, |a| a.seq);
    let bytes = r.bytes.unwrap_or(0);
    let state = match r.state {
        OwnFileState::NotAccepted if r.bytes.is_some() => {
            format!("No version is accepted yet, so none of it is carried. On disk: {bytes} B.")
        }
        OwnFileState::NotAccepted => "No version is accepted yet, and there is no file.".into(),
        OwnFileState::Unreadable => "Its accepted version cannot be read from the store, so none \
                                     of it is carried."
            .into(),
        OwnFileState::Same if r.bytes.is_some() => format!(
            "Version {seq}, accepted on {on}, is what is carried, and the file on disk is the same."
        ),
        OwnFileState::Same => format!("Accepted as absent on {on}, and there is no file."),
        OwnFileState::Differs => format!(
            "Version {seq}, accepted on {on}, is what is carried; the file on disk ({bytes} B) \
             differs."
        ),
        OwnFileState::Gone => format!(
            "Version {seq}, accepted on {on}, is what is carried; the file is gone from disk."
        ),
        OwnFileState::New => format!(
            "Accepted as absent on {on}; a file is on disk now ({bytes} B), and none of it is \
             carried."
        ),
    };
    out.push_str(&state);
    out.push('\n');
    if let Some(change) = &r.change {
        out.push_str("The change, from what is accepted to what is on disk:\n");
        out.push_str(change);
        if !change.ends_with('\n') {
            out.push('\n');
        }
    }
    let open = !matches!(r.state, OwnFileState::Same | OwnFileState::Unreadable);
    if hint && open {
        out.push_str(&format!(
            "Accept what is on disk with `{}` at your terminal.\n",
            theseus_protocol::own_file::accept_command()
        ));
    }
    out
}

/// What an accept prints.
pub fn accepted(r: &OwnFileAcceptResult) -> String {
    if r.kept {
        format!(
            "Accepted: version {} of {} is what Theseus carries from the next turn.",
            r.seq, r.path
        )
    } else {
        format!(
            "Nothing new to accept: version {} of {} is what is on disk.",
            r.seq, r.path
        )
    }
}

#[cfg(test)]
mod tests {
    use theseus_protocol::own_file::OwnFileAccepted;

    use super::*;

    fn result(state: OwnFileState, change: Option<&str>) -> OwnFileShowResult {
        OwnFileShowResult {
            path: "/srv/state/AGENTS.md".into(),
            state,
            accepted: Some(OwnFileAccepted {
                seq: 3,
                digest: Some("0123456789abcdef".into()),
                at_ms: 1,
                on: "2026-11-20".into(),
            }),
            bytes: Some(120),
            digest: "fedcba9876543210".into(),
            change: change.map(str::to_string),
        }
    }

    /// The show says where the file stands, prints the change whole, and
    /// names the accept only when there is something to accept.
    #[test]
    fn the_show_says_where_the_file_stands_and_names_the_accept() {
        let differs = shown(
            &result(OwnFileState::Differs, Some("```diff\n-old\n+new\n```\n")),
            true,
        );
        assert_eq!(
            differs,
            "Your own instruction file: /srv/state/AGENTS.md\n\
             Version 3, accepted on 2026-11-20, is what is carried; the file on disk (120 B) differs.\n\
             The change, from what is accepted to what is on disk:\n\
             ```diff\n-old\n+new\n```\n\
             Accept what is on disk with `theseus own-file accept` at your terminal.\n"
        );
        let same = shown(&result(OwnFileState::Same, None), true);
        assert_eq!(
            same,
            "Your own instruction file: /srv/state/AGENTS.md\n\
             Version 3, accepted on 2026-11-20, is what is carried, and the file on disk is the same.\n"
        );
    }
}
