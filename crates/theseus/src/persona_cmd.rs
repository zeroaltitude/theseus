//! `theseus persona …` (theseus-slvl.2, theseus-utfnl): the persona as data.
//! `persona` shows every persona (the ontology's `persona:` categories) and
//! the built-in default; `persona add|soul|set|retire|default` write them,
//! each a method of the daemon and the owner's alone; `persona edit` is `set`
//! with field flags and `soul` without; `persona history` lists the
//! `persona.changed` rows. The CLI writes no config file.

use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::Value;
use theseus_client::{render, Conn};
use theseus_protocol::initiative::{InitiativeWeekParams, InitiativeWeekResult};
use theseus_protocol::{
    method, LedgerTailParams, LedgerTailResult, PersonaAddParams, PersonaCardConfirmParams,
    PersonaCardConfirmResult, PersonaImportParams, PersonaImportResult, PersonaKeyParams,
    PersonaListParams, PersonaListResult, PersonaSetParams, PersonaSoulParams, PersonaWriteResult,
};

use crate::cmd::output;

#[derive(Subcommand, Debug, Default)]
pub enum PersonaCmd {
    /// The speaking voice: `persona voice` shows the voice a call speaks in and where it comes from;
    /// `persona voice audition [--voices a,b,c] [--text WORDS] [--out DIR]` makes the greeting in
    /// several voices as WAV files and prints their paths and the cost (a few cents; yours alone).
    /// Pick with `persona edit --tts-voice ID`.
    Voice {
        #[command(subcommand)]
        cmd: Option<crate::voice_cmd::VoiceCmd>,
    },
    /// Your 👍 (`up`) or 👎 (`down`) on a message Theseus sent first: the latest, or the one `ID`
    /// names (the turn id of a wake's or a task's reply, or the id a note it wrote itself has).
    /// Yours alone (refused inside a job); the week's totals are in `persona`.
    Rate {
        /// `up` or `down`.
        #[arg(value_parser = ["up", "down"])]
        verdict: String,
        id: Option<String>,
        /// The session it was said in.
        #[arg(long, short, value_name = "SESSION")]
        session: Option<String>,
    },
    /// Every persona: name, emoji, voice, initiative, its files (size, digest, when edited), the
    /// compiles that carried it, and the places that speak as it (default).
    #[default]
    Show,
    /// Add a persona: its key, and optionally its name, emoji, card and soul (`--soul TEXT` or
    /// `--file PATH`). Without a soul it awaits one: you are asked once, and it is never picked
    /// until `persona soul KEY` writes it. Yours alone (refused inside a job).
    Add {
        /// The persona's key.
        key: String,
        #[arg(long = "name", value_name = "NAME")]
        display_name: Option<String>,
        #[arg(long)]
        emoji: Option<String>,
        /// About 60 tokens of specialization and background.
        #[arg(long)]
        card: Option<String>,
        /// The soul's text.
        #[arg(long, conflicts_with = "file")]
        soul: Option<String>,
        /// A file holding the soul.
        #[arg(long, value_name = "PATH")]
        file: Option<std::path::PathBuf>,
    },
    /// Write a persona's soul: from `--file PATH`, from stdin when it is piped, else in $EDITOR.
    /// A soul over `[context] persona_max_tokens` (2,000) is refused with its count. Yours alone.
    Soul {
        key: String,
        #[arg(long, value_name = "PATH")]
        file: Option<std::path::PathBuf>,
    },
    /// Set a persona's short fields (an empty value clears one). Yours alone.
    Set {
        key: String,
        #[arg(long = "name", value_name = "NAME")]
        display_name: Option<String>,
        #[arg(long)]
        emoji: Option<String>,
        #[arg(long)]
        card: Option<String>,
        #[arg(long, alias = "voice")]
        tts_voice: Option<String>,
        #[arg(long)]
        avatar: Option<String>,
        /// `quiet`, `normal` or `chatty`.
        #[arg(long)]
        initiative: Option<String>,
        /// A shared place may be answered by this persona too.
        #[arg(long)]
        shareable: Option<bool>,
    },
    /// Retire a persona (the ledger and `ontology undo` keep it). Yours alone.
    Retire { key: String },
    /// Make a persona the default, the one Jev's `none` engages; `--clear` takes the mark off.
    #[command(name = "default")]
    MakeDefault {
        key: Option<String>,
        #[arg(long)]
        clear: bool,
    },
    /// Set a persona's short fields (as `set`), or, with none of those flags, write its soul (as
    /// `soul`). Kept as the older spelling. Yours alone (refused inside a job).
    Edit {
        /// The persona's key: omitted, the default persona.
        persona: Option<String>,
        /// Shown on replies, the bot's nickname, the voice greeting.
        #[arg(long = "name", value_name = "NAME")]
        display_name: Option<String>,
        #[arg(long)]
        emoji: Option<String>,
        /// About 60 tokens of specialization and background.
        #[arg(long)]
        card: Option<String>,
        /// Overrides `[voice] tts_voice` while this persona is in play (`--voice` is the same flag;
        /// `theseus persona voice audition` lets you hear several first).
        #[arg(long, alias = "voice")]
        tts_voice: Option<String>,
        /// A path or a URL, for Discord.
        #[arg(long)]
        avatar: Option<String>,
        /// `quiet`, `normal` or `chatty`.
        #[arg(long)]
        initiative: Option<String>,
        /// Keep the card an import drafted as it is: Jev is offered it from then on.
        #[arg(long)]
        confirm_card: bool,
    },
    /// Carry an agent's workspace over as a persona, unchanged: its persona files are copied
    /// byte for byte into the state dir, the persona's soul is pointed at the first, a specialization card is
    /// drafted for you to read, and the agent's feedback is proposed as standing records. Yours alone.
    Import {
        /// The agent's workspace folder (SOUL.md, IDENTITY.md, USER.md, AGENTS.md, MEMORY.md, memory/).
        #[arg(long, value_name = "DIR")]
        from: String,
        /// The persona's key.
        #[arg(long, value_name = "NAME")]
        name: String,
        /// The name its imported episodes carry, when it is not the persona's key.
        #[arg(long)]
        agent: Option<String>,
        /// Say what would be carried and written, and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// The `persona.changed` rows, newest first: which persona, which file or field, and the
    /// digests either side.
    History {
        /// How many rows.
        #[arg(short = 'n', long, default_value_t = 20)]
        n: usize,
    },
}

/// The fields a flag set names, as (field, value).
pub fn fields_of(cmd: &PersonaCmd) -> Vec<(&'static str, String)> {
    let (PersonaCmd::Edit {
        display_name,
        emoji,
        card,
        tts_voice,
        avatar,
        initiative,
        ..
    }
    | PersonaCmd::Set {
        display_name,
        emoji,
        card,
        tts_voice,
        avatar,
        initiative,
        ..
    }) = cmd
    else {
        return Vec::new();
    };
    [
        ("name", display_name),
        ("emoji", emoji),
        ("card", card),
        ("tts_voice", tts_voice),
        ("avatar", avatar),
        ("initiative", initiative),
    ]
    .into_iter()
    .filter_map(|(k, v)| Some((k, v.clone()?)))
    .collect()
}

pub async fn run(conn: &mut Conn, json: bool, cmd: PersonaCmd) -> Result<()> {
    match cmd {
        PersonaCmd::Voice { cmd } => {
            crate::voice_cmd::run(conn, json, cmd.unwrap_or_default()).await
        }
        PersonaCmd::Rate {
            verdict,
            id,
            session,
        } => crate::notices_cmd::rate(conn, json, verdict, id, session).await,
        cmd => shown(conn, json, cmd).await,
    }
}

async fn shown(conn: &mut Conn, json: bool, cmd: PersonaCmd) -> Result<()> {
    match &cmd {
        PersonaCmd::Voice { .. } | PersonaCmd::Rate { .. } => unreachable!("`run` sends these on"),
        PersonaCmd::Show => {
            let v = conn
                .request(
                    method::PERSONA_LIST,
                    serde_json::to_value(PersonaListParams::default())?,
                )
                .await?;
            if json {
                return output(true, v, |_: Value| Ok(()));
            }
            output(false, v, |r: PersonaListResult| {
                for l in show_lines(&r) {
                    println!("{l}");
                }
                Ok(())
            })?;
            let week = conn
                .request(
                    method::INITIATIVE_WEEK,
                    serde_json::to_value(InitiativeWeekParams::default())?,
                )
                .await?;
            output(false, week, |w: InitiativeWeekResult| {
                for l in week_lines(&w) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        PersonaCmd::History { n } => {
            let v = conn
                .request(
                    method::LEDGER_TAIL,
                    serde_json::to_value(LedgerTailParams {
                        n: Some(*n),
                        kind: Some("persona.changed".into()),
                        ..Default::default()
                    })?,
                )
                .await?;
            output(json, v, |r: LedgerTailResult| {
                for l in history_lines(&r) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        PersonaCmd::Import {
            from,
            name,
            agent,
            dry_run,
        } => import(conn, json, from, name, agent.as_deref(), *dry_run).await,
        _ => write_cmd(conn, json, cmd).await,
    }
}

/// The owner's writes: edit, add, soul, set, retire, default.
async fn write_cmd(conn: &mut Conn, json: bool, cmd: PersonaCmd) -> Result<()> {
    match &cmd {
        PersonaCmd::Edit {
            persona,
            confirm_card,
            ..
        } => {
            if *confirm_card {
                return confirm(conn, json, persona.as_deref()).await;
            }
            let fields = fields_of(&cmd);
            match fields.is_empty() {
                true => {
                    let key = in_play_or(conn, persona.as_deref()).await?;
                    soul(conn, json, &key, None).await
                }
                false => {
                    let key = in_play_or(conn, persona.as_deref()).await?;
                    set(conn, json, &key, &fields, None).await
                }
            }
        }
        PersonaCmd::Add {
            key,
            display_name,
            emoji,
            card,
            soul: text,
            file,
        } => {
            let soul = match (text, file) {
                (Some(t), _) => Some(t.clone()),
                (None, Some(f)) => Some(std::fs::read_to_string(f)?),
                (None, None) => None,
            };
            let v = conn
                .request(
                    method::PERSONA_ADD,
                    serde_json::to_value(PersonaAddParams {
                        key: key.clone(),
                        name: display_name.clone(),
                        emoji: emoji.clone(),
                        card: card.clone(),
                        soul,
                        from: Some("the CLI".into()),
                        ..Default::default()
                    })?,
                )
                .await?;
            written(json, v)
        }
        PersonaCmd::Soul { key, file } => soul(conn, json, key, file.as_deref()).await,
        PersonaCmd::Set { key, shareable, .. } => {
            set(conn, json, key, &fields_of(&cmd), *shareable).await
        }
        PersonaCmd::Retire { key } => {
            let v = conn
                .request(
                    method::PERSONA_RETIRE,
                    serde_json::to_value(PersonaKeyParams {
                        key: Some(key.clone()),
                        ..Default::default()
                    })?,
                )
                .await?;
            written(json, v)
        }
        PersonaCmd::MakeDefault { key, clear } => {
            let key = match (key, clear) {
                (Some(k), false) => Some(k.clone()),
                (None, true) => None,
                _ => bail!("name the persona to make the default, or --clear"),
            };
            let v = conn
                .request(
                    method::PERSONA_DEFAULT,
                    serde_json::to_value(PersonaKeyParams {
                        key,
                        ..Default::default()
                    })?,
                )
                .await?;
            written(json, v)
        }
        PersonaCmd::Voice { .. }
        | PersonaCmd::Rate { .. }
        | PersonaCmd::Show
        | PersonaCmd::History { .. }
        | PersonaCmd::Import { .. } => unreachable!("`shown` keeps these"),
    }
}

/// The persona a command names, else the one in play.
async fn in_play_or(conn: &mut Conn, persona: Option<&str>) -> Result<String> {
    if let Some(p) = persona {
        return Ok(p.to_string());
    }
    let listed: PersonaListResult = serde_json::from_value(
        conn.request(
            method::PERSONA_LIST,
            serde_json::to_value(PersonaListParams::default())?,
        )
        .await?,
    )?;
    match listed.in_play {
        Some(k) => Ok(k),
        None => bail!(
            "no persona is in play: name one (`theseus persona soul <key>`); the built-in \
             default is not editable (`theseus persona add <key>` makes one)"
        ),
    }
}

/// `persona set`: the short fields.
async fn set(
    conn: &mut Conn,
    json: bool,
    key: &str,
    fields: &[(&'static str, String)],
    shareable: Option<bool>,
) -> Result<()> {
    let get = |k: &str| fields.iter().find(|(f, _)| *f == k).map(|(_, v)| v.clone());
    if fields.is_empty() && shareable.is_none() {
        bail!("name a field to set: --name, --emoji, --card, --tts-voice, --avatar, --initiative, --shareable");
    }
    let v = conn
        .request(
            method::PERSONA_SET,
            serde_json::to_value(PersonaSetParams {
                key: key.to_string(),
                name: get("name"),
                emoji: get("emoji"),
                card: get("card"),
                tts_voice: get("tts_voice"),
                avatar: get("avatar"),
                initiative: get("initiative"),
                shareable,
                ..Default::default()
            })?,
        )
        .await?;
    written(json, v)
}

/// `persona soul`: the text from a file, a pipe, or $EDITOR.
async fn soul(
    conn: &mut Conn,
    json: bool,
    key: &str,
    file: Option<&std::path::Path>,
) -> Result<()> {
    let text = soul_text(conn, key, file).await?;
    let v = conn
        .request(
            method::PERSONA_SOUL,
            serde_json::to_value(PersonaSoulParams {
                key: key.to_string(),
                text,
                ..Default::default()
            })?,
        )
        .await?;
    written(json, v)
}

async fn soul_text(conn: &mut Conn, key: &str, file: Option<&std::path::Path>) -> Result<String> {
    use std::io::{IsTerminal, Read};
    if let Some(f) = file {
        return Ok(std::fs::read_to_string(f)?);
    }
    if !std::io::stdin().is_terminal() {
        let mut t = String::new();
        std::io::stdin().read_to_string(&mut t)?;
        return Ok(t);
    }
    // The editor opens the soul as it stands (a copy), and what it saves is sent.
    let listed: PersonaListResult = serde_json::from_value(
        conn.request(
            method::PERSONA_LIST,
            serde_json::to_value(PersonaListParams::default())?,
        )
        .await?,
    )?;
    let info = listed
        .personas
        .iter()
        .find(|p| p.key.as_deref() == Some(key));
    if info.is_none() {
        bail!("{key:?} is not a persona: {}", names(&listed));
    }
    let current = info
        .and_then(|i| i.files.first())
        .and_then(|f| std::fs::read_to_string(&f.path).ok())
        .unwrap_or_default();
    let tmp = std::env::temp_dir().join(format!("theseus-soul-{key}-{}.md", std::process::id()));
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(current.as_bytes())?;
    }
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    let status = std::process::Command::new(&editor).arg(&tmp).status();
    let text = std::fs::read_to_string(&tmp);
    let _ = std::fs::remove_file(&tmp);
    if !status?.success() {
        bail!("{editor} exited without success: nothing was written");
    }
    Ok(text?)
}

/// What a persona write did, in a line.
pub fn written_lines(r: &PersonaWriteResult) -> Vec<String> {
    let soul = match (&r.soul, r.tokens) {
        (Some(p), Some(t)) => format!("soul {p} ({t} of {} tokens)", r.cap),
        (Some(p), None) => format!("soul {p}"),
        (None, _) => "no soul yet: `theseus persona soul KEY` writes it".into(),
    };
    vec![format!("persona {}: {}; {soul}", r.key, r.state)]
}

fn written(json: bool, v: Value) -> Result<()> {
    output(json, v, |r: PersonaWriteResult| {
        for l in written_lines(&r) {
            println!("{l}");
        }
        Ok(())
    })
}

/// `persona import`: the daemon carries the files and makes the persona (its
/// soul the first file, its card the draft); this prints what it did.
async fn import(
    conn: &mut Conn,
    json: bool,
    from: &str,
    name: &str,
    agent: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    // The daemon reads the folder on its own machine: a relative path is made whole here.
    let from = std::fs::canonicalize(from)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| from.to_string());
    let v = conn
        .request(
            method::PERSONA_IMPORT,
            serde_json::to_value(PersonaImportParams {
                from,
                name: name.to_string(),
                agent: agent.map(str::to_string),
                dry_run,
            })?,
        )
        .await?;
    let r: PersonaImportResult = serde_json::from_value(v.clone())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        for l in import_lines(&r) {
            println!("{l}");
        }
    }
    Ok(())
}

/// What an import (or a dry run of one) says, line by line.
pub fn import_lines(r: &PersonaImportResult) -> Vec<String> {
    let mut out = Vec::new();
    let head = if r.dry_run {
        format!(
            "Dry run: persona {} from agent {}, nothing written.",
            r.name, r.agent
        )
    } else if r.unchanged {
        format!(
            "Persona {} is as it was (version {}): every file's bytes equal the copy.",
            r.name, r.version
        )
    } else {
        format!(
            "Persona {} imported as version {} into {}.",
            r.name, r.version, r.dir
        )
    };
    out.push(head);
    for f in &r.files {
        let kind = match (&f.part_of, f.note) {
            (Some(of), _) => format!("part of {of}"),
            (None, true) => "note, carried not compiled".into(),
            (None, false) => "persona file".into(),
        };
        out.push(format!(
            "  {}  {}  digest {}  ({kind}{})",
            f.rel,
            render::fmt_bytes(f.bytes),
            f.digest,
            if f.changed { ", new or changed" } else { "" }
        ));
    }
    for o in &r.over_limit {
        out.push(format!(
            "  {} is {}, over the 64 KB file limit: carried verbatim as {} parts cut at its headings",
            o.rel,
            render::fmt_bytes(o.bytes),
            o.parts
        ));
    }
    for s in &r.skipped {
        out.push(format!("  not carried: {s}"));
    }
    match &r.card {
        Some(c) => out.push(format!(
            "  card (a DRAFT; no model wrote it): {c}\n  Read it, edit it with `theseus persona edit {} --card \"…\"`, \
             or keep it as it is with `theseus persona edit {} --confirm-card`; Jev is offered it only then.",
            r.name, r.name
        )),
        None => out.push("  no card could be drafted: write one with `theseus persona edit --card`".into()),
    }
    if r.proposals_known > 0 {
        out.push(format!(
            "  {} already proposed earlier (`theseus standing proposals --all`)",
            r.proposals_known
        ));
    }
    if !r.proposals.is_empty() {
        out.push(format!(
            "  {} standing record(s) proposed from the agent's feedback; none is live until you keep it:",
            r.proposals.len()
        ));
        for p in &r.proposals {
            out.push(format!(
                "    {} [{}] {}  (from {})",
                p.id,
                p.kind.name(),
                p.text,
                p.from
            ));
        }
    }
    out
}

/// `persona edit --confirm-card`: the owner has read the drafted card.
async fn confirm(conn: &mut Conn, json: bool, persona: Option<&str>) -> Result<()> {
    let Some(name) = persona else {
        bail!("name the persona: `theseus persona edit <name> --confirm-card`");
    };
    let v = conn
        .request(
            method::PERSONA_CARD_CONFIRM,
            serde_json::to_value(PersonaCardConfirmParams {
                name: name.to_string(),
            })?,
        )
        .await?;
    output(json, v, |r: PersonaCardConfirmResult| {
        println!(
            "Kept {}'s card; Jev is offered it from now on: {}",
            r.name, r.card
        );
        Ok(())
    })
}

fn names(r: &PersonaListResult) -> String {
    let known: Vec<_> = r.personas.iter().filter_map(|p| p.key.as_deref()).collect();
    match known.is_empty() {
        true => "none is defined (`theseus persona add`)".into(),
        false => format!("the personas are {}", known.join(", ")),
    }
}

/// `theseus persona`'s lines.
/// The last week of what Theseus said first: promises kept, and the owner's
/// verdicts (theseus-slvl.9). The maintainer's measure over a week is promises
/// kept at 95% or more and the 👍 share.
pub fn week_lines(w: &InitiativeWeekResult) -> Vec<String> {
    let pct =
        |s: Option<f64>| s.map_or_else(|| "none yet".to_string(), |s| format!("{:.0}%", s * 100.0));
    let mut out = vec![format!(
        "initiative this week ({}): promises kept {} ({} kept, {} unkept)",
        w.level,
        pct(w.kept_share),
        w.kept,
        w.unkept
    )];
    out.push(format!(
        "  said first {}, 👍 {} 👎 {}: {} liked",
        w.spoke,
        w.up,
        w.down,
        pct(w.up_share)
    ));
    if w.voiced > 0 {
        out.push(format!(
            "  {} written by a model for ${:.4}",
            w.voiced, w.voiced_usd
        ));
    }
    out
}

/// A persona's soul: its size against the cap, or what it waits for.
fn soul_lines(p: &theseus_protocol::PersonaInfo, cap: u64) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(t) = p.soul_tokens {
        let over = match p.over_cap {
            true => format!(", OVER ITS CAP of {cap}"),
            false => String::new(),
        };
        out.push(format!("  soul: {t} tokens{over}"));
    }
    if p.awaiting {
        let key = p.key.as_deref().unwrap_or("KEY");
        out.push(format!(
            "  waiting for its soul: `theseus persona soul {key}`"
        ));
    }
    out
}

pub fn show_lines(r: &PersonaListResult) -> Vec<String> {
    let mut out = Vec::new();
    for p in &r.personas {
        let shown = match &p.key {
            Some(k) => format!("{k}: {}", p.name),
            None => format!("built-in default: {}", p.name),
        };
        let emoji = p
            .emoji
            .as_deref()
            .map(|e| format!(" {e}"))
            .unwrap_or_default();
        let play = if p.in_play { "  [in play]" } else { "" };
        let mark = match (p.is_default, p.awaiting) {
            (true, true) => "  [default] [awaiting its soul]",
            (true, false) => "  [default]",
            (false, true) => "  [awaiting its soul]",
            (false, false) => "",
        };
        out.push(format!(
            "{shown}{emoji}  initiative {}{play}{mark}",
            p.initiative
        ));
        if let Some(c) = &p.card {
            let state = if p.card_draft {
                " (DRAFT: not offered to Jev until you keep it)"
            } else {
                ""
            };
            out.push(format!("  card{state}: {c}"));
        }
        if let Some(v) = &p.tts_voice {
            out.push(format!("  voice: {v}"));
        }
        if let Some(a) = &p.avatar {
            out.push(format!("  avatar: {a}"));
        }
        if p.key.is_none() {
            out.push(format!(
                "  {} bytes, digest {}: used by a persona with no file of its own",
                r.built_in_bytes, r.built_in_digest
            ));
            continue;
        }
        out.extend(soul_lines(p, r.cap));
        if p.awaiting {
            continue;
        }
        for f in &p.files {
            let state = match (&f.missing, &f.digest) {
                (Some(why), _) => format!("MISSING ({why})"),
                (None, Some(d)) => format!(
                    "{}, digest {d}{}",
                    render::fmt_bytes(f.bytes),
                    f.edited_ms
                        .map(|t| format!(", edited {}", render::fmt_date(t)))
                        .unwrap_or_default()
                ),
                (None, None) => "not read".into(),
            };
            out.push(format!("  file: {} ({state})", f.path));
        }
        if p.files.is_empty() {
            out.push("  no files: the built-in default's text stands in for it".into());
        } else if p.on_default {
            out.push("  no file is readable: compiles run on the built-in default".into());
        }
        out.push(format!(
            "  digest {}{}",
            p.digest,
            p.changed_ms
                .map(|t| format!(", last changed {}", render::fmt_date(t)))
                .unwrap_or_default()
        ));
        if let Some(t) = &p.tally {
            out.push(format!(
                "  in every compile since {} ({} compiles, {} missing)",
                render::fmt_date(t.since_ms),
                t.compiles,
                t.missing
            ));
        }
        for place in &p.places {
            out.push(format!("  place: {}", place.place));
        }
        for w in &p.warnings {
            out.push(format!("  warning: {w}"));
        }
    }
    for x in r.places.iter().filter(|x| x.unknown) {
        out.push(format!(
            "warning: {} names persona {:?}, which is not a persona: it speaks as the default",
            x.place, x.persona
        ));
    }
    out
}

/// `theseus persona history`'s lines.
pub fn history_lines(r: &LedgerTailResult) -> Vec<String> {
    if r.rows.is_empty() {
        return vec!["No persona has changed since it was first seen.".into()];
    }
    r.rows
        .iter()
        .rev()
        .map(|row| {
            let d = &row.data;
            let s = |k: &str| d[k].as_str().unwrap_or("").to_string();
            let what = match d["path"].as_str() {
                Some(p) => format!("file {p}"),
                None => s("field"),
            };
            let digest = |x: String| if x.is_empty() { "none".to_string() } else { x };
            format!(
                "{}  {}  {what}  {} -> {}",
                render::fmt_date(row.at_unix_ms),
                s("persona"),
                digest(s("old")),
                digest(s("new"))
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `persona edit --voice ID` is `--tts-voice ID` (theseus-slvl.7): it sets the persona's
    /// `tts_voice`.
    #[test]
    fn edit_voice_sets_the_personas_tts_voice() {
        #[derive(clap::Parser)]
        struct T {
            #[command(subcommand)]
            c: PersonaCmd,
        }
        use clap::Parser as _;
        for flag in ["--voice", "--tts-voice"] {
            let t = T::try_parse_from(["t", "edit", "pier", flag, "aura-2-luna-en"]).unwrap();
            assert_eq!(
                fields_of(&t.c),
                vec![("tts_voice", "aura-2-luna-en".to_string())]
            );
        }
    }

    /// `set` takes the same flags as `edit`, and `add`, `soul`, `retire` and `default` parse.
    #[test]
    fn the_writers_parse() {
        #[derive(clap::Parser)]
        struct T {
            #[command(subcommand)]
            c: PersonaCmd,
        }
        use clap::Parser as _;
        let t = T::try_parse_from(["t", "set", "pier", "--emoji", "🌊", "--initiative", "quiet"])
            .unwrap();
        assert_eq!(
            fields_of(&t.c),
            vec![
                ("emoji", "🌊".to_string()),
                ("initiative", "quiet".to_string())
            ]
        );
        assert!(T::try_parse_from(["t", "add", "pier", "--soul", "x", "--file", "y"]).is_err());
        for argv in [
            ["t", "add", "pier", "--card", "Tides."].as_slice(),
            ["t", "soul", "pier", "--file", "/x/s.md"].as_slice(),
            ["t", "retire", "pier"].as_slice(),
            ["t", "default", "pier"].as_slice(),
            ["t", "default", "--clear"].as_slice(),
        ] {
            T::try_parse_from(argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
        }
    }

    /// A write's line says the state, the soul and its size against the cap.
    #[test]
    fn a_write_says_the_state_the_soul_and_its_size() {
        let r = PersonaWriteResult {
            key: "pier".into(),
            state: "live".into(),
            soul: Some("/s/personas/pier/PERSONA.md".into()),
            tokens: Some(312),
            cap: 2000,
        };
        assert_eq!(
            written_lines(&r),
            ["persona pier: live; soul /s/personas/pier/PERSONA.md (312 of 2000 tokens)"]
        );
        let waiting = PersonaWriteResult {
            state: "awaiting".into(),
            soul: None,
            tokens: None,
            ..r
        };
        assert!(written_lines(&waiting)[0].contains("no soul yet"));
    }

    /// The show lines carry each field, a missing file, and the tally.
    #[test]
    fn show_lines_name_the_fields_the_files_and_the_tally() {
        use theseus_protocol::{PersonaFile, PersonaInfo, PersonaTally};
        let r = PersonaListResult {
            personas: vec![
                PersonaInfo {
                    key: Some("pier".into()),
                    name: "Pier".into(),
                    emoji: Some("🌊".into()),
                    initiative: "quiet".into(),
                    in_play: true,
                    files: vec![PersonaFile {
                        path: "/w/p.md".into(),
                        missing: Some("not found".into()),
                        ..Default::default()
                    }],
                    digest: "abc".into(),
                    on_default: true,
                    tally: Some(PersonaTally {
                        since_ms: 0,
                        compiles: 3,
                        missing: 0,
                    }),
                    ..Default::default()
                },
                PersonaInfo {
                    name: "Theseus".into(),
                    initiative: "normal".into(),
                    ..Default::default()
                },
            ],
            in_play: Some("pier".into()),
            built_in_bytes: 665,
            built_in_digest: "d1".into(),
            cap: 2000,
            places: vec![],
        };
        let lines = show_lines(&r).join("\n");
        for want in [
            "pier: Pier 🌊  initiative quiet  [in play]",
            "file: /w/p.md (MISSING (not found))",
            "no file is readable: compiles run on the built-in default",
            "(3 compiles, 0 missing)",
            "built-in default: Theseus  initiative normal",
            "665 bytes, digest d1",
        ] {
            assert!(lines.contains(want), "{want:?} not in\n{lines}");
        }
    }

    /// The import's report says what is carried, what is a draft, and that nothing is live.
    #[test]
    fn the_import_report_names_the_draft_and_the_proposals() {
        use theseus_protocol::{ImportedFile, OverLimit, ProposedStanding, StandingKind};
        let r = PersonaImportResult {
            name: "wren".into(),
            agent: "wren".into(),
            dry_run: true,
            dir: "/s/personas/wren".into(),
            version: 1,
            files: vec![ImportedFile {
                rel: "SOUL.md".into(),
                path: "/s/personas/wren/SOUL.md".into(),
                bytes: 120,
                digest: "0123456789abcdef".into(),
                changed: true,
                ..Default::default()
            }],
            over_limit: vec![OverLimit {
                rel: "AGENTS.md".into(),
                bytes: 90_000,
                parts: 2,
            }],
            skipped: vec!["USER.md: empty".into()],
            card: Some("Tides.".into()),
            proposals: vec![ProposedStanding {
                id: "stp_aaaaaaaaaaaa".into(),
                kind: StandingKind::Order,
                text: "Never pad.".into(),
                from: "memory/b.md".into(),
            }],
            ..Default::default()
        };
        let text = import_lines(&r).join("\n");
        for want in [
            "Dry run: persona wren from agent wren, nothing written.",
            "SOUL.md",
            "digest 0123456789abcdef",
            "new or changed",
            "AGENTS.md is",
            "2 parts cut at its headings",
            "not carried: USER.md: empty",
            "card (a DRAFT; no model wrote it): Tides.",
            "--confirm-card",
            "none is live until you keep it",
            "stp_aaaaaaaaaaaa [order] Never pad.",
        ] {
            assert!(text.contains(want), "{want:?} not in\n{text}");
        }
    }
}
