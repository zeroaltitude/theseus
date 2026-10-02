//! `theseus-sim discord`: Discord without a person (theseus-9kjv).
//!
//! - `proof` runs the kl8m proof against a real `theseusd`, with its own
//!   stand-ins (`theseus_sim::discord_proof`), and exits 1 when a step fails.
//! - `say`, `press`, and `read` drive a running `theseus-sim fake-discord
//!   --gateway …` from a live check: a message typed as a user, a button on a
//!   posted message pressed as one, and what the stand-in holds, read back
//!   (its control routes, under `/_fake/` on its REST port).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use serde_json::{json, Value};
use theseus_sim::discord_proof;

#[derive(Subcommand)]
pub enum Cmd {
    /// The kl8m proof: a typed message and its reply, a write on the approve
    /// list and its card, a refused press and an Approve press, the call
    /// run and the card updated, against a real daemon and stand-ins for
    /// Discord and the model. Exits 1 when a step fails.
    Proof {
        /// The daemon to prove (default: the `theseusd` beside this binary).
        #[arg(long)]
        theseusd: Option<PathBuf>,
        /// Work in this directory and keep it (default: a temporary one).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Type a message as a user, through a running fake-discord's gateway.
    Say {
        /// The fake's REST address (its `--addr`).
        #[arg(long, default_value = "127.0.0.1:9447")]
        fake: String,
        /// A guild channel's id; without it, the user's DM with the bot.
        #[arg(long)]
        channel: Option<u64>,
        #[arg(long)]
        user: u64,
        #[arg(long, default_value = "tester")]
        name: String,
        text: String,
    },
    /// Press a button on a message the bot posted, as a user.
    Press {
        #[arg(long, default_value = "127.0.0.1:9447")]
        fake: String,
        /// The message's id (`read` lists them).
        #[arg(long)]
        message: String,
        /// The button's label (`Approve`) or custom id.
        #[arg(long, default_value = "Approve")]
        button: String,
        #[arg(long)]
        user: u64,
        #[arg(long, default_value = "tester")]
        name: String,
    },
    /// What the fake holds, as JSON: `messages` (each with every version and
    /// its buttons), `replies` (the answers to interactions), or `gateway`.
    Read {
        #[arg(long, default_value = "127.0.0.1:9447")]
        fake: String,
        #[arg(default_value = "messages")]
        what: String,
    },
    /// The proof's stand-in for the Messages API, alone, for a daemon's
    /// `[model] api_base`: a prompt holding PROOF-WRITE asks for an
    /// `fs.write` of `<dir>/outside/proof.txt`, a tool's result ends the
    /// turn, and anything else is answered "ready". Serves until killed.
    Model {
        #[arg(long, default_value = "127.0.0.1:9448")]
        addr: String,
        /// The rig's directory (`discord rig --dir`).
        #[arg(long)]
        dir: PathBuf,
    },
    /// Lay out a directory for a scratch daemon on the stand-ins, as the
    /// proof does, and print how to start each process: `config.toml`, a
    /// fake `op` in `bin/` (every secret an invented value), the bindings,
    /// and `guild.json` for `fake-discord --guild`.
    Rig {
        #[arg(long)]
        dir: PathBuf,
        /// The daemon (default: the `theseusd` beside this binary).
        #[arg(long)]
        theseusd: Option<PathBuf>,
        /// The fake's REST address.
        #[arg(long, default_value = "127.0.0.1:9447")]
        fake: String,
        /// The fake's gateway address.
        #[arg(long, default_value = "127.0.0.1:9449")]
        gateway: String,
        /// The model stand-in's address.
        #[arg(long, default_value = "127.0.0.1:9448")]
        model: String,
    },
}

/// `theseusd` beside this binary, unless named.
fn beside(theseusd: Option<PathBuf>) -> Result<PathBuf> {
    match theseusd {
        Some(p) => Ok(p),
        None => Ok(std::env::current_exe()?
            .parent()
            .context("this binary's directory")?
            .join("theseusd")),
    }
}

pub fn run(cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Proof { theseusd, dir } => {
            let theseusd = beside(theseusd)?;
            println!("discord proof: {}", theseusd.display());
            let r = discord_proof::run(&discord_proof::Opts {
                theseusd,
                dir,
                verbose: true,
            })?;
            // Each step printed as it ended; then the count and the transcript.
            print!("{}", r.summary());
            if !r.passed() {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Say {
            fake,
            channel,
            user,
            name,
            text,
        } => {
            let body = json!({"channel": channel, "user": user, "name": name, "content": text});
            print_sent(control(&fake, "POST", "say", &body)?)
        }
        Cmd::Press {
            fake,
            message,
            button,
            user,
            name,
        } => {
            let body = json!({"message": message, "button": button, "user": user, "name": name});
            print_sent(control(&fake, "POST", "press", &body)?)
        }
        Cmd::Read { fake, what } => {
            let (_, v) = control(&fake, "GET", &what, &Value::Null)?;
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(())
        }
        Cmd::Model { addr, dir } => {
            let m = discord_proof::Model::start_on(&addr, discord_proof::write_input(&dir))?;
            println!("the proof's model stand-in on {}", m.addr);
            loop {
                std::thread::park();
            }
        }
        Cmd::Rig {
            dir,
            theseusd,
            fake,
            gateway,
            model,
        } => rig(&dir, &beside(theseusd)?, &fake, &gateway, &model),
    }
}

/// Lay out the rig, and say how to run each process and drive them.
fn rig(dir: &Path, theseusd: &Path, fake: &str, gateway: &str, model: &str) -> Result<()> {
    let ends = discord_proof::Ends {
        rest: fake,
        gateway,
        model,
    };
    discord_proof::lay_out(dir, theseusd, &ends)?;
    let d = dir.display();
    let (ana, lab) = (discord_proof::ANA, discord_proof::LAB);
    println!(
        "laid out {d}: config.toml, bin/op, state/bindings.toml, guild.json, outside/\n\
         start, each in its own shell:\n\
         \x20 theseus-sim fake-discord --addr {fake} --gateway {gateway} --guild {d}/guild.json --log {d}/fake.log\n\
         \x20 theseus-sim discord model --addr {model} --dir {d}\n\
         \x20 PATH={d}/bin:$PATH OP_SERVICE_ACCOUNT_TOKEN=proof-not-a-token {} --config {d}/config.toml \
         --socket {d}/sock --state-dir {d}/state\n\
         then, as ana in #lab (ids {ana} and {lab}):\n\
         \x20 theseus-sim discord say --fake {fake} --channel {lab} --user {ana} --name ana \"PROOF-WRITE: write it\"\n\
         \x20 theseus-sim discord read --fake {fake}            # the card's id\n\
         \x20 theseus-sim discord press --fake {fake} --message <id> --button Approve --user {ana} --name ana",
        theseusd.display()
    );
    Ok(())
}

fn print_sent((status, v): (u16, Value)) -> Result<()> {
    if status != 200 {
        bail!(
            "the fake said {status}: {}",
            v["error"].as_str().unwrap_or("")
        );
    }
    println!("{}", v["id"].as_str().unwrap_or(""));
    Ok(())
}

/// One request to the fake's control routes, bounded: a connect to a port
/// nothing listens on hangs on this machine.
fn control(fake: &str, method: &str, route: &str, body: &Value) -> Result<(u16, Value)> {
    let addr = fake
        .parse()
        .with_context(|| format!("{fake} is not host:port"))?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .with_context(|| format!("connecting to the fake at {fake}"))?;
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    let b = if body.is_null() {
        String::new()
    } else {
        body.to_string()
    };
    write!(
        s,
        "{method} /_fake/{route} HTTP/1.1\r\nhost: fake\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{b}",
        b.len()
    )?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    let (head, body) = out.split_once("\r\n\r\n").context("no answer")?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .context("no status")?;
    Ok((status, serde_json::from_str(body).unwrap_or(Value::Null)))
}
