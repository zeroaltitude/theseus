//! `theseus persona …` (theseus-slvl.2): the persona as data. `persona` shows
//! every persona the config defines and the built-in default; `persona edit`
//! sets a persona's short fields in the config file, or opens its file in
//! `$EDITOR`; `persona history` lists the `persona.changed` rows.

use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::Value;
use theseus_client::{render, Conn};
use theseus_protocol::initiative::{InitiativeWeekParams, InitiativeWeekResult};
use theseus_protocol::{
    method, HealthResult, LedgerTailParams, LedgerTailResult, PersonaCardConfirmParams,
    PersonaCardConfirmResult, PersonaImportParams, PersonaImportResult, PersonaListParams,
    PersonaListResult,
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
    /// Set a persona's short fields in the config file (a restart applies them), or, with none of
    /// those flags, open its first file in $EDITOR. Yours alone (refused inside a job).
    Edit {
        /// The persona's key in `[personas]`: omitted, the default persona.
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
    /// Carry an agent's workspace over as a persona, unchanged (theseus-a8n7): its persona files are copied
    /// byte for byte into the state dir, `[personas.<name>]` is pointed at them, a specialization card is
    /// drafted for you to read, and the agent's feedback is proposed as standing records. Yours alone.
    Import {
        /// The agent's workspace folder (SOUL.md, IDENTITY.md, USER.md, AGENTS.md, MEMORY.md, memory/).
        #[arg(long, value_name = "DIR")]
        from: String,
        /// The persona's key in `[personas]`.
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

/// The fields a flag set names, as (config key, value).
pub fn fields_of(cmd: &PersonaCmd) -> Vec<(&'static str, String)> {
    let PersonaCmd::Edit {
        display_name,
        emoji,
        card,
        tts_voice,
        avatar,
        initiative,
        ..
    } = cmd
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
        PersonaCmd::Edit {
            persona,
            confirm_card,
            ..
        } => {
            if *confirm_card {
                return confirm(conn, json, persona.as_deref()).await;
            }
            edit(conn, persona.as_deref(), fields_of(&cmd)).await
        }
    }
}

async fn edit(
    conn: &mut Conn,
    persona: Option<&str>,
    fields: Vec<(&'static str, String)>,
) -> Result<()> {
    if let Some(s) = std::env::var("THESEUS_SESSION")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        bail!(
            "theseus persona edit refused: it is the operator's to run, and this shell is a \
             Theseus job's (THESEUS_SESSION={s} is set), so it would answer for the operator. \
             Run it from your own shell."
        );
    }
    let listed: PersonaListResult = serde_json::from_value(
        conn.request(
            method::PERSONA_LIST,
            serde_json::to_value(PersonaListParams::default())?,
        )
        .await?,
    )?;
    let key = match persona.map(str::to_string).or(listed.in_play.clone()) {
        Some(k) => k,
        None => bail!(
            "no persona is in play: name one (`theseus persona edit <name>`); the built-in \
             default is not editable, and an install names its own under [personas.<name>]"
        ),
    };
    let info = listed
        .personas
        .iter()
        .find(|p| p.key.as_deref() == Some(&key));
    if fields.is_empty() {
        let Some(info) = info else {
            bail!("{key:?} is not a persona: {}", names(&listed));
        };
        let Some(file) = info.files.first() else {
            bail!(
                "{key} names no files: add `files = [\"~/path/persona.md\"]` under \
                 [personas.{key}] and run this again"
            );
        };
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".into());
        let status = std::process::Command::new(&editor)
            .arg(&file.path)
            .status()?;
        if !status.success() {
            bail!("{editor} exited with {status}");
        }
        println!(
            "Edited {}. Every session that speaks as {key} recompiles with it at its next turn.",
            file.path
        );
        return Ok(());
    }
    write_fields(conn, &key, &fields).await
}

/// Set `fields` under `[personas.<key>]` in the config file (a vault note is
/// told what to hold instead).
async fn write_fields(conn: &mut Conn, key: &str, fields: &[(&'static str, String)]) -> Result<()> {
    let health: HealthResult = serde_json::from_value(
        conn.request(method::HEALTH, serde_json::Value::Null)
            .await?,
    )?;
    let doc = set_fields_doc(fields)?;
    if health.config.source != "file" {
        bail!(
            "the config is the vault note {}: edit it there. Under [personas.{key}] set:\n{doc}",
            health.config.reference
        );
    }
    let path = std::path::PathBuf::from(&health.config.reference);
    let text = std::fs::read_to_string(&path)?;
    let edited = set_fields(&text, key, fields)?;
    if edited == text {
        println!("{key} already has those values.");
        return Ok(());
    }
    // Keep what it was, then replace it whole.
    let bak = path.with_extension(format!("toml.bak-{}", theseus_protocol::now_unix_ms()));
    std::fs::copy(&path, &bak)?;
    let tmp = path.with_extension("toml.new");
    std::fs::write(&tmp, &edited)?;
    std::fs::set_permissions(&tmp, std::fs::metadata(&path)?.permissions())?;
    std::fs::rename(&tmp, &path)?;
    println!(
        "Set {} under [personas.{key}] in {} (the old file is {}). The daemon serves the config it \
         started with: restart it to apply.",
        fields.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", "),
        path.display(),
        bak.display()
    );
    Ok(())
}

/// `persona import`: the daemon carries the files; this writes the config.
async fn import(
    conn: &mut Conn,
    json: bool,
    from: &str,
    name: &str,
    agent: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    if let Some(s) = std::env::var("THESEUS_SESSION")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        bail!(
            "theseus persona import refused: it is the operator's to run, and this shell is a \
             Theseus job's (THESEUS_SESSION={s} is set). Run it from your own shell."
        );
    }
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
    if dry_run || r.unchanged {
        return Ok(());
    }
    let listed: PersonaListResult = serde_json::from_value(
        conn.request(
            method::PERSONA_LIST,
            serde_json::to_value(PersonaListParams::default())?,
        )
        .await?,
    )?;
    let has_card = listed
        .personas
        .iter()
        .find(|p| p.key.as_deref() == Some(name))
        .is_some_and(|p| p.card.is_some());
    let mut fields = vec![("files", r.context_paths.join("\n"))];
    // The owner's own card is never replaced by a draft.
    if let (Some(card), false) = (&r.card, has_card) {
        fields.push(("card", card.clone()));
    }
    write_fields(conn, name, &fields).await
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
        true => "no [personas.<name>] table is defined".into(),
        false => format!("the personas are {}", known.join(", ")),
    }
}

/// The field lines a vault note would get.
fn set_fields_doc(fields: &[(&'static str, String)]) -> Result<String> {
    fields
        .iter()
        .map(|(k, v)| Ok(format!("{k} = {}", value_of(k, v)?)))
        .collect::<Result<Vec<_>>>()
        .map(|l| l.join("\n"))
}

/// A field's value as TOML, checked.
fn value_of(field: &str, v: &str) -> Result<String> {
    // `files` is a list: its paths, one a line.
    if field == "files" {
        let paths: Vec<&str> = v.lines().filter(|l| !l.trim().is_empty()).collect();
        if paths.is_empty() {
            bail!("files names no path");
        }
        return Ok(serde_json::to_string(&paths)?);
    }
    if v.trim().is_empty() {
        bail!(
            "--{} is empty: leave the flag out instead",
            field.replace('_', "-")
        );
    }
    if v.contains('\n') && field != "card" {
        bail!("--{} must be one line", field.replace('_', "-"));
    }
    if field == "initiative" && !["quiet", "normal", "chatty"].contains(&v) {
        bail!("--initiative {v:?} is not one of quiet, normal, chatty");
    }
    if field == "card" && v.chars().count() > 600 {
        bail!("--card is at most 600 characters (about 60 tokens)");
    }
    Ok(serde_json::to_string(v)?)
}

/// `text` with `fields` set under `[personas.<key>]`: a line that sets one is
/// replaced, a missing one is added after the table's last line, and a
/// missing table is added at the end. Everything else is as it was, comments
/// included.
pub fn set_fields(text: &str, key: &str, fields: &[(&'static str, String)]) -> Result<String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let header = |l: &str| {
        let t = l.trim();
        t == format!("[personas.{key}]") || t == format!("[personas.\"{key}\"]")
    };
    let Some(start) = lines.iter().position(|l| header(l)) else {
        let mut out = text.trim_end().to_string();
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("[personas.{key}]\n"));
        for (k, v) in fields {
            out.push_str(&format!("{k} = {}\n", value_of(k, v)?));
        }
        return Ok(out);
    };
    for (k, v) in fields {
        let value = value_of(k, v)?;
        let end = lines[start + 1..]
            .iter()
            .position(|l| l.trim_start().starts_with('['))
            .map_or(lines.len(), |i| start + 1 + i);
        let at = lines[start + 1..end].iter().position(|l| {
            l.trim_start()
                .strip_prefix(k)
                .is_some_and(|rest| rest.trim_start().starts_with('='))
        });
        match at {
            Some(i) => lines[start + 1 + i] = format!("{k} = {value}"),
            None => {
                let mut at = end;
                while at > start + 1 && lines[at - 1].trim().is_empty() {
                    at -= 1;
                }
                lines.insert(at, format!("{k} = {value}"));
            }
        }
    }
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
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
        out.push(format!("{shown}{emoji}  initiative {}{play}", p.initiative));
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
            "warning: {} names persona {:?}, which the config does not define: it speaks as the default",
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

    fn f(k: &'static str, v: &str) -> (&'static str, String) {
        (k, v.to_string())
    }

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

    /// A field present is replaced in place; one missing is added after the table's last line,
    /// before the next table; comments and the rest are untouched.
    #[test]
    fn a_field_is_set_in_its_table_and_nothing_else_moves() {
        let text = "[context]\nfiles = []\n\n[personas.pier]\n# the owner's words\nfiles = [\"~/p.md\"]\nemoji = \"x\"\n\n[kernel]\nspend = 1\n";
        let out = set_fields(text, "pier", &[f("emoji", "🌊"), f("initiative", "quiet")]).unwrap();
        assert_eq!(
            out,
            "[context]\nfiles = []\n\n[personas.pier]\n# the owner's words\nfiles = [\"~/p.md\"]\nemoji = \"🌊\"\ninitiative = \"quiet\"\n\n[kernel]\nspend = 1\n"
        );
        let again = set_fields(&out, "pier", &[f("emoji", "🌊")]).unwrap();
        assert_eq!(again, out, "setting the same value changes nothing");
    }

    /// A missing table is added at the end.
    #[test]
    fn a_missing_table_is_added_at_the_end() {
        let out = set_fields("[kernel]\nspend = 1\n", "pier", &[f("name", "Pier")]).unwrap();
        assert_eq!(
            out,
            "[kernel]\nspend = 1\n\n[personas.pier]\nname = \"Pier\"\n"
        );
    }

    /// A value is checked before it is written.
    #[test]
    fn a_bad_value_is_refused_with_the_allowed_ones() {
        let e = set_fields("", "pier", &[f("initiative", "loud")])
            .unwrap_err()
            .to_string();
        assert!(e.contains("quiet, normal, chatty"), "{e}");
        assert!(set_fields("", "pier", &[f("emoji", " ")]).is_err());
        assert!(set_fields("", "pier", &[f("name", "a\nb")]).is_err());
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

    /// `files` is written as a TOML list of the paths, one a line in, and the line is a valid
    /// array of strings.
    #[test]
    fn the_files_field_is_a_list_of_paths() {
        let paths = "/s/personas/wren/SOUL.md\n/s/personas/wren/IDENTITY.md";
        let out = set_fields(
            "",
            "wren",
            &[f("files", paths), f("card", "Tides. Areas: a; b.")],
        )
        .unwrap();
        assert_eq!(
            out,
            "[personas.wren]\nfiles = [\"/s/personas/wren/SOUL.md\",\"/s/personas/wren/IDENTITY.md\"]\ncard = \"Tides. Areas: a; b.\"\n"
        );
        // A JSON array of strings is a TOML array of strings.
        let line = out
            .lines()
            .nth(1)
            .unwrap()
            .strip_prefix("files = ")
            .unwrap();
        let listed: Vec<String> = serde_json::from_str(line).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(value_of("files", "\n \n").is_err());
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
