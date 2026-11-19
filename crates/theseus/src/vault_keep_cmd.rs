//! `theseus vault keep` (issue 26, line 2): the owner keeps a secret in the
//! vault from their own terminal, typing its value at a hidden prompt. The
//! value goes from here to the daemon in one request (`vault.keep.answer` for a
//! card a turn put, `vault.keep` for a keep with no card) and nowhere else: not
//! an argument, not the environment, not standard input from a pipe, not
//! printed, not logged. Yours alone: refused inside a Theseus job's shell, with
//! no terminal on both ends, and by the daemon from every surface but your
//! own terminal.

use std::io::{BufRead, IsTerminal, Write};

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use theseus_client::Conn;
use theseus_protocol::owner_proof::ProofCode;
use theseus_protocol::vault_keep::{SecretText, VaultKeepAnswerParams, VaultKeepParams, VaultKept};
use theseus_protocol::{method, ConfirmListResult, ConfirmRequest};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum VaultCmd {
    /// Keep a new secret in the vault: the value is typed here with nothing shown, and goes to the
    /// daemon alone. With no arguments it answers the card Theseus put to you (the only one
    /// waiting, or the one named); with `--vault`, `--item` and `--field` it keeps one with no
    /// card: the way to replace an item Theseus's own settings read. Never paste a secret into
    /// chat.
    Keep(KeepArgs),
}

#[derive(Args, Debug)]
pub struct KeepArgs {
    /// The card to answer, when more than one is waiting (`theseus answer` lists them).
    pub question: Option<String>,
    /// Keep with no card: the vault's name.
    #[arg(long, requires_all = ["item", "field"], conflicts_with = "question")]
    pub vault: Option<String>,
    /// Keep with no card: the item's title.
    #[arg(long, requires_all = ["vault", "field"])]
    pub item: Option<String>,
    /// Keep with no card: the field's label.
    #[arg(long, requires_all = ["vault", "item"])]
    pub field: Option<String>,
    /// The six digits your authenticator app shows (`-` reads them from standard input). Asked for
    /// when one is needed and this is not given.
    #[arg(long, value_name = "DIGITS")]
    pub code: Option<String>,
}

pub async fn run(conn: &mut Conn, json: bool, cmd: VaultCmd) -> Result<()> {
    let VaultCmd::Keep(a) = cmd;
    if let Some(job) = theseus_client::client::job_session() {
        bail!(
            "this shell is a Theseus job's (THESEUS_SESSION={job} is set), so it would give a \
             secret in your name: run it from your own terminal"
        );
    }
    if !std::io::stdin().is_terminal() {
        bail!(
            "the value is typed at a hidden prompt of your own terminal: a pipe or a capture is \
             refused, so it cannot be read by anything else"
        );
    }
    match (&a.vault, &a.item, &a.field) {
        (Some(vault), Some(title), Some(field)) => {
            let code = code(a.code.as_deref(), true)?;
            let value = hidden(&format!(
                "Value for {title}, field {field} (nothing is shown): "
            ))?;
            let params = VaultKeepParams {
                vault: vault.clone(),
                title: title.clone(),
                field: field.clone(),
                value,
                code,
                from_job: None,
            };
            let v = conn
                .call(method::VAULT_KEEP, serde_json::to_value(params)?, |_, _| {})
                .await?;
            output(json, v, told)
        }
        _ => answer(conn, json, a).await,
    }
}

/// Answer the card Theseus put to the owner.
async fn answer(conn: &mut Conn, json: bool, a: KeepArgs) -> Result<()> {
    let waiting: ConfirmListResult = serde_json::from_value(
        conn.request(method::CONFIRM_LIST, serde_json::Value::Null)
            .await?,
    )?;
    let cards: Vec<&ConfirmRequest> = waiting
        .confirms
        .iter()
        .filter(|c| c.tool == "config.ack" && c.input["keep"].is_object())
        .filter(|c| {
            a.question
                .as_deref()
                .is_none_or(|q| c.correlation_id.ends_with(q))
        })
        .collect();
    let card = match cards.as_slice() {
        [] => bail!("no secret is waiting to be kept: ask Theseus to keep one, and it puts a card"),
        [one] => *one,
        many => {
            let ids: Vec<&str> = many.iter().map(|c| c.correlation_id.as_str()).collect();
            bail!(
                "{} secrets are waiting: name one, `theseus vault keep <id>`: {}",
                many.len(),
                ids.join(", ")
            )
        }
    };
    let keep = &card.input["keep"];
    let said = |k: &str| keep[k].as_str().unwrap_or("?").to_string();
    println!(
        "{} the vault \"{}\", item \"{}\", field \"{}\" ({}).",
        match said("state").as_str() {
            "replaces" => "This REPLACES the value in",
            _ => "Keeping a secret in",
        },
        said("vault"),
        said("title"),
        said("field"),
        said("state")
    );
    let needs = card.needs_fresh_proof
        || (card.needs_proof && card.proof.as_ref().is_none_or(|p| p.code_needed));
    let code = code(a.code.as_deref(), needs)?;
    let value = hidden(&format!("Value for {} (nothing is shown): ", said("title")))?;
    let params = VaultKeepAnswerParams {
        question_id: card.correlation_id.clone(),
        value,
        code,
        from_job: None,
    };
    let v = conn
        .call(
            method::VAULT_KEEP_ANSWER,
            serde_json::to_value(params)?,
            |_, _| {},
        )
        .await?;
    output(json, v, told)
}

fn told(k: VaultKept) -> Result<()> {
    println!("{}", k.words);
    Ok(())
}

/// The code: `given` (`-` reads one line), else asked for on the terminal when
/// `needed`, else none.
fn code(given: Option<&str>, needed: bool) -> Result<Option<ProofCode>> {
    let line = match given {
        Some("-") => read_line()?,
        Some(c) => c.to_string(),
        None if needed => {
            eprint!("Code from your authenticator app: ");
            std::io::stderr().flush()?;
            read_line()?
        }
        None => return Ok(None),
    };
    let line = line.trim();
    Ok((!line.is_empty()).then(|| ProofCode::new(line)))
}

fn read_line() -> Result<String> {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line)
}

/// A line typed with nothing shown: echo off for the read and put back after,
/// whatever happens. The one place the value is in this process.
fn hidden(prompt: &str) -> Result<SecretText> {
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    let _quiet = NoEcho::on()?;
    let line = read_line();
    eprintln!();
    let line = line?;
    let value = SecretText::new(line.trim_end_matches(['\n', '\r']));
    if value.is_blank() {
        bail!("no value was given");
    }
    Ok(value)
}

/// The terminal with echo off, put back on drop.
struct NoEcho(libc::termios);

impl NoEcho {
    fn on() -> Result<Self> {
        // SAFETY: termios is plain data, and fd 0 is a terminal (checked by the caller).
        let orig = unsafe {
            let mut t = std::mem::zeroed::<libc::termios>();
            if libc::tcgetattr(0, &mut t) != 0 {
                bail!("the terminal's modes could not be read, so the value would be shown");
            }
            t
        };
        let mut quiet = orig;
        quiet.c_lflag &= !libc::ECHO;
        // SAFETY: as above.
        if unsafe { libc::tcsetattr(0, libc::TCSANOW, &quiet) } != 0 {
            bail!("the terminal's echo could not be turned off, so the value would be shown");
        }
        Ok(Self(orig))
    }
}

impl Drop for NoEcho {
    fn drop(&mut self) {
        // SAFETY: a saved termios, put back on the terminal it came from.
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, &self.0);
        }
    }
}
