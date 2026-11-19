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
use theseus_protocol::method;
use theseus_protocol::owner_proof::ProofCode;
use theseus_protocol::vault_keep::{
    ClientEnd, SecretText, VaultKeepAnswerParams, VaultKeepParams, VaultKeepWaiting,
    VaultKeepWaitingParams, VaultKept,
};

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
    // The first thing asked is the daemon's: which cards wait, and why none, with every refusal
    // (a job's shell, no terminal) said by it and written to its record. This program filters
    // nothing and decides nothing about what is waiting.
    let terminal = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let target = match (&a.vault, &a.item, &a.field) {
        (Some(vault), Some(title), Some(field)) => Some((vault, title, field)),
        _ => None,
    };
    let mut ask = VaultKeepWaitingParams {
        question: a.question.clone(),
        vault: target.map(|t| t.0.clone()),
        title: target.map(|t| t.1.clone()),
        field: target.map(|t| t.2.clone()),
        from_job: theseus_client::client::job_session(),
        terminal,
        ended: None,
    };
    let waiting: VaultKeepWaiting =
        serde_json::from_value(conn.request(method::VAULT_KEEP_WAITING, &ask).await?)?;
    let card = match waiting.cards.as_slice() {
        [] => bail!("{}", waiting.none),
        [one] => one,
        many => {
            let each: Vec<String> = many
                .iter()
                .map(|c| {
                    format!(
                        "  {}  {} / {} / {} ({})",
                        c.question_id.as_deref().unwrap_or("?"),
                        c.vault,
                        c.title,
                        c.field,
                        c.state
                    )
                })
                .collect();
            bail!(
                "{} secrets are waiting to be kept: name one, `theseus vault keep <id>` (the end \
                 of its id is enough):\n{}",
                many.len(),
                each.join("\n")
            )
        }
    };
    if let Some(why) = &waiting.blocked {
        bail!("{why}");
    }
    eprintln!("{}", card.heading);
    ask.question = card.question_id.clone();
    let code = match card.code_needed || a.code.is_some() {
        true => match given_code(a.code.as_deref(), card.code_needed) {
            Ok(code) => code,
            Err(gap) => return ended(conn, &ask, gap).await,
        },
        false => None,
    };
    let value = match typed_value(&card.title) {
        Ok(v) => v,
        Err(gap) => return ended(conn, &ask, gap).await,
    };
    let v = match &card.question_id {
        Some(question_id) => {
            let params = VaultKeepAnswerParams {
                question_id: question_id.clone(),
                value,
                code,
                from_job: None,
            };
            conn.call(
                method::VAULT_KEEP_ANSWER,
                serde_json::to_value(params)?,
                |_, _| {},
            )
            .await?
        }
        None => {
            let params = VaultKeepParams {
                vault: card.vault.clone(),
                title: card.title.clone(),
                field: card.field.clone(),
                value,
                code,
                from_job: None,
            };
            conn.call(method::VAULT_KEEP, serde_json::to_value(params)?, |_, _| {})
                .await?
        }
    };
    output(json, v, told)
}

/// The run ends before a value is sent: the daemon is told which way (so its
/// record says what the terminal did), and the person is told in plain words.
async fn ended(conn: &mut Conn, ask: &VaultKeepWaitingParams, gap: Gap) -> Result<()> {
    let end = gap.class();
    let report = VaultKeepWaitingParams {
        ended: Some(end),
        ..ask.clone()
    };
    let _ = conn.request(method::VAULT_KEEP_WAITING, &report).await;
    bail!("{}", gap.words())
}

/// Why a prompt got no line.
enum Gap {
    /// Nothing was typed, twice.
    Blank,
    /// The terminal gave end of input before a line.
    End,
    /// The terminal could not be read.
    Unreadable,
    /// The terminal's echo could not be turned off: nothing is asked.
    NoEcho(&'static str),
}

impl Gap {
    fn class(&self) -> ClientEnd {
        match self {
            Gap::Blank => ClientEnd::NoValue,
            Gap::End => ClientEnd::EndOfInput,
            Gap::Unreadable => ClientEnd::Unreadable,
            Gap::NoEcho(_) => ClientEnd::NoEcho,
        }
    }

    fn words(&self) -> String {
        match self {
            Gap::Blank => "nothing was typed, twice, so nothing was kept".into(),
            Gap::End => "the terminal ended (end of input) before anything was typed, so nothing \
                 was kept"
                .into(),
            Gap::Unreadable => "the terminal could not be read, so nothing was kept".into(),
            Gap::NoEcho(why) => format!("{why}, so nothing was asked and nothing was kept"),
        }
    }
}

fn told(k: VaultKept) -> Result<()> {
    println!("{}", k.words);
    Ok(())
}

/// The code: `given` (`-` reads one line), else asked for on the terminal when
/// `needed`, else none.
fn given_code(given: Option<&str>, needed: bool) -> Result<Option<ProofCode>, Gap> {
    let line = match given {
        Some("-") => line_of("", false)?,
        Some(c) => c.to_string(),
        None if needed => ask_twice("Code from your authenticator app: ", false)?,
        None => return Ok(None),
    };
    let line = line.trim();
    Ok((!line.is_empty()).then(|| ProofCode::new(line)))
}

/// The value, typed with nothing shown.
fn typed_value(title: &str) -> Result<SecretText, Gap> {
    let line = ask_twice(&format!("Value for {title} (nothing is shown): "), true)?;
    Ok(SecretText::new(line.trim_end_matches(['\n', '\r'])))
}

/// A prompt that is asked once more when nothing was typed: an empty line is
/// a slip, not an answer.
fn ask_twice(prompt: &str, hide: bool) -> Result<String, Gap> {
    for again in [false, true] {
        let line = line_of(prompt, hide)?;
        if !line.trim().is_empty() {
            return Ok(line);
        }
        if !again {
            eprintln!("Nothing was typed: once more.");
        }
    }
    Err(Gap::Blank)
}

/// What is already waiting on the terminal's input is thrown away: a newline
/// left over from the command's own line, or typed ahead, is not a code or a value.
fn drain() {
    // SAFETY: fd 0 is a terminal (the daemon was told so, and a prompt on one that is not fails
    // below); `tcflush` takes no pointer.
    unsafe {
        libc::tcflush(0, libc::TCIFLUSH);
    }
}

/// One line from the terminal: what was pending is drained first, the echo is
/// off for a `hide` and put back after, and the prompt is on standard error
/// (which is a terminal: the daemon refused anything else).
fn line_of(prompt: &str, hide: bool) -> Result<String, Gap> {
    drain();
    let _quiet = match hide {
        true => Some(NoEcho::on()?),
        false => None,
    };
    if !prompt.is_empty() {
        eprint!("{prompt}");
        let _ = std::io::stderr().flush();
    }
    let mut line = String::new();
    let got = std::io::stdin().lock().read_line(&mut line);
    if hide {
        eprintln!();
    }
    match got {
        Ok(0) => Err(Gap::End),
        Err(_) => Err(Gap::Unreadable),
        Ok(_) => Ok(line),
    }
}

/// The terminal with echo off, put back on drop.
struct NoEcho(libc::termios);

impl NoEcho {
    fn on() -> Result<Self, Gap> {
        // SAFETY: termios is plain data, and fd 0 is a terminal (checked by the caller).
        let orig = unsafe {
            let mut t = std::mem::zeroed::<libc::termios>();
            if libc::tcgetattr(0, &mut t) != 0 {
                return Err(Gap::NoEcho(
                    "the terminal's modes could not be read, so the value would be shown",
                ));
            }
            t
        };
        let mut quiet = orig;
        quiet.c_lflag &= !libc::ECHO;
        // SAFETY: as above.
        if unsafe { libc::tcsetattr(0, libc::TCSANOW, &quiet) } != 0 {
            return Err(Gap::NoEcho(
                "the terminal's echo could not be turned off, so the value would be shown",
            ));
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
