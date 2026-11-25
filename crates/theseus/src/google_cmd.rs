//! `theseus google auth [--manual] | status | revoke` (the Google design,
//! §2.2): the one-time consent, the status, and the revoke. The consent URL
//! holds no secret, so it is printed; the paste (the address bar of the page
//! that failed to load, which carries the authorization code) is read from the
//! terminal and sent over the socket, never taken as an argument, so it is in
//! no shell history and no transcript.

use std::io::{self, IsTerminal, Write};

use anyhow::Result;
use clap::Subcommand;
use theseus_client::Conn;
use theseus_protocol::google::{
    GoogleAuthBeginParams, GoogleAuthBeginResult, GoogleAuthFinishParams, GoogleAuthFinishResult,
    GoogleRevokeParams, GoogleRevokeResult, GoogleStatus, GoogleStatusParams,
};
use theseus_protocol::method;

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum GoogleCmd {
    /// The one-time consent: opens Google's sign-in (a page on this machine takes the
    /// redirect; if none arrives in 90 s, paste the address bar of the page that failed to
    /// load). Open the URL on the desktop that runs the daemon, not on a phone. The refresh
    /// token is stored where `[secrets] google_refresh_token` points, and held at once.
    Auth {
        /// Skip the wait for the redirect and go straight to the paste.
        #[arg(long)]
        manual: bool,
    },
    /// The account, the services, the scopes Google granted, the tools on, and the state.
    Status,
    /// Revoke the grant at Google, clear the stored token, and turn the tools off until
    /// `theseus google auth` is run again.
    Revoke,
}

pub async fn run(conn: &mut Conn, json: bool, cmd: GoogleCmd) -> Result<()> {
    match cmd {
        GoogleCmd::Auth { manual } => auth(conn, json, manual).await,
        GoogleCmd::Status => {
            let v = conn
                .request(method::GOOGLE_STATUS, GoogleStatusParams::default())
                .await?;
            output(json, v, |s: GoogleStatus| {
                for l in theseus_client::render::google_status_lines(&s) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        GoogleCmd::Revoke => {
            let v = conn
                .request(method::GOOGLE_REVOKE, GoogleRevokeParams::default())
                .await?;
            output(json, v, |r: GoogleRevokeResult| {
                for l in revoke_lines(&r) {
                    println!("{l}");
                }
                Ok(())
            })
        }
    }
}

async fn auth(conn: &mut Conn, json: bool, manual: bool) -> Result<()> {
    let v = conn
        .request(method::GOOGLE_AUTH_BEGIN, GoogleAuthBeginParams { manual })
        .await?;
    let begun: GoogleAuthBeginResult = serde_json::from_value(v)?;
    for l in begin_lines(&begun) {
        eprintln!("{l}");
    }
    let mut pasted: Option<String> = None;
    let mut wait = begun.paste_after_secs;
    let r: GoogleAuthFinishResult = loop {
        let params = GoogleAuthFinishParams {
            auth_id: begun.auth_id.clone(),
            pasted: pasted.take(),
            wait_secs: wait,
        };
        let r: GoogleAuthFinishResult =
            serde_json::from_value(conn.request(method::GOOGLE_AUTH_FINISH, params).await?)?;
        if r.state != "waiting" {
            break r;
        }
        match ask_paste()? {
            Some(p) => {
                pasted = Some(p);
                wait = 0;
            }
            // Nothing pasted: keep waiting for the redirect.
            None => wait = begun.paste_after_secs.max(30),
        }
    };
    if json {
        println!("{}", serde_json::to_string(&r)?);
        return Ok(());
    }
    for l in finish_lines(&r) {
        println!("{l}");
    }
    Ok(())
}

/// The address bar, from the terminal; none when the line is empty.
fn ask_paste() -> Result<Option<String>> {
    if io::stdin().is_terminal() {
        eprint!("No redirect yet. Paste the address bar of the page that failed to load (Enter to keep waiting): ");
        io::stderr().flush()?;
    }
    let mut line = String::new();
    let n = io::stdin().read_line(&mut line)?;
    anyhow::ensure!(
        n > 0,
        "no address was given and the terminal closed: run `theseus google auth` again"
    );
    let line = line.trim().to_string();
    Ok((!line.is_empty()).then_some(line))
}

pub fn begin_lines(b: &GoogleAuthBeginResult) -> Vec<String> {
    let mut v = vec![
        format!(
            "Sign in as {} to give Theseus access to: {}.",
            b.account,
            b.asks.join(", ")
        ),
        "Open this address in a browser on the desktop that runs the daemon (not on a phone):"
            .to_string(),
        String::new(),
        b.url.clone(),
        String::new(),
        "Google may say it has not verified this app: Advanced, then Go to the app, once."
            .to_string(),
    ];
    if b.redirect.is_empty() {
        v.push("After you approve, the page will fail to load: copy its whole address bar.".into());
    } else {
        v.push(format!(
            "Waiting for the redirect on {} (up to {} minutes)…",
            b.redirect,
            b.expires_secs / 60
        ));
    }
    v
}

pub fn finish_lines(r: &GoogleAuthFinishResult) -> Vec<String> {
    let mut v = vec![format!(
        "Google access for {} is on: {}.",
        r.account,
        r.granted.join(", ")
    )];
    v.extend(r.not_granted.iter().cloned());
    v.push(match r.store.as_str() {
        "memory" => "The refresh token is held in memory only: it works until the daemon restarts."
            .to_string(),
        s => format!("The refresh token is stored ({s}). No restart is needed."),
    });
    if !r.note.is_empty() {
        v.push(r.note.clone());
    }
    v.push(format!("Tools on: {}.", r.tools_on.join(", ")));
    v
}

pub fn revoke_lines(r: &GoogleRevokeResult) -> Vec<String> {
    let mut v = vec![
        if r.revoked_at_google {
            "Revoked at Google.".to_string()
        } else {
            "Not revoked at Google.".to_string()
        },
        if r.cleared {
            "The stored token is cleared.".to_string()
        } else {
            "The stored token was not cleared.".to_string()
        },
        "The Google tools are off until `theseus google auth` is run again.".to_string(),
    ];
    if !r.note.is_empty() {
        v.push(r.note.clone());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lines_say_what_happened_and_never_show_a_token() {
        let r = GoogleAuthFinishResult {
            state: "done".into(),
            account: "owner@example.test".into(),
            granted: vec!["Gmail".into(), "Calendar".into()],
            not_granted: vec!["Drive was not granted: the drive tools stay off".into()],
            tools_on: vec!["gmail.search".into()],
            store: "memory".into(),
            note: "the refresh token could not be stored (no write access)".into(),
        };
        let text = finish_lines(&r).join("\n");
        assert!(text.contains("Drive was not granted"));
        assert!(text.contains("held in memory only"));
        let b = GoogleAuthBeginResult {
            url: "https://accounts.example/auth?x=1".into(),
            redirect: String::new(),
            account: "owner@example.test".into(),
            asks: vec!["Gmail".into()],
            ..Default::default()
        };
        assert!(begin_lines(&b)
            .join("\n")
            .contains("copy its whole address bar"));
    }
}
