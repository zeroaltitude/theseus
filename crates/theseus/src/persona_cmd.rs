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
    method, HealthResult, LedgerTailParams, LedgerTailResult, PersonaListParams, PersonaListResult,
};

use crate::cmd::output;

#[derive(Subcommand, Debug, Default)]
pub enum PersonaCmd {
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
        /// Overrides `[voice] tts_voice` while this persona is in play.
        #[arg(long)]
        tts_voice: Option<String>,
        /// A path or a URL, for Discord.
        #[arg(long)]
        avatar: Option<String>,
        /// `quiet`, `normal` or `chatty`.
        #[arg(long)]
        initiative: Option<String>,
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
    match &cmd {
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
        PersonaCmd::Edit { persona, .. } => edit(conn, persona.as_deref(), fields_of(&cmd)).await,
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
    let health: HealthResult = serde_json::from_value(
        conn.request(method::HEALTH, serde_json::Value::Null)
            .await?,
    )?;
    let doc = set_fields_doc(&fields)?;
    if health.config.source != "file" {
        bail!(
            "the config is the vault note {}: edit it there. Under [personas.{key}] set:\n{doc}",
            health.config.reference
        );
    }
    let path = std::path::PathBuf::from(&health.config.reference);
    let text = std::fs::read_to_string(&path)?;
    let edited = set_fields(&text, &key, &fields)?;
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
            out.push(format!("  card: {c}"));
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
}
