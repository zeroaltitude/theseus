//! The import itself: read the workspace, plan the copy, and (unless it is a
//! dry run) write it. The only code here that touches the disk.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use theseus_protocol::{
    ImportedFile, OverLimit, PersonaImportParams, PersonaImportResult, ProposedStanding,
};

use super::manifest::Manifest;
use super::{card, feedback, split};

/// The persona's folder under the state dir.
pub fn dir_of(state_dir: &Path, name: &str) -> PathBuf {
    state_dir.join("personas").join(name)
}

/// A persona's key: lower case letters, digits, `_` and `-`.
pub fn check_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if !ok {
        bail!("{name:?} is not a persona name: use lower case letters, digits, `_` and `-` (at most 64)");
    }
    Ok(())
}

/// One thing to carry.
struct Item {
    rel: String,
    bytes: Vec<u8>,
    part_of: Option<String>,
    note: bool,
}

/// What the workspace gave, and what it did not.
struct Plan {
    items: Vec<Item>,
    skipped: Vec<String>,
    over: Vec<OverLimit>,
    identity: Option<String>,
    agents: Option<String>,
    /// (rel, text) of every note that may hold feedback.
    feedback_from: Vec<(String, String)>,
}

pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))[..16].to_string()
}

/// Import `p.from` as persona `p.name`. `names` are the config's
/// `[persona] import_files`; `limit` the context-file limit in bytes.
pub fn import(
    state_dir: &Path,
    p: &PersonaImportParams,
    names: &[String],
    limit: usize,
    now_ms: u64,
) -> Result<(PersonaImportResult, Vec<ProposedStanding>)> {
    check_name(&p.name)?;
    let from = Path::new(&p.from);
    if !from.is_dir() {
        bail!("{} is not a folder", p.from);
    }
    let plan = plan(from, names, limit)?;
    if plan.items.iter().all(|i| i.note) {
        bail!(
            "{} holds none of the persona files ({}): is it an agent's workspace?",
            p.from,
            names.join(", ")
        );
    }
    let dir = dir_of(state_dir, &p.name);
    let before = Manifest::read(&dir);
    let agent = p.agent.clone().unwrap_or_else(|| p.name.clone());

    let existing: BTreeMap<&str, &str> = before
        .iter()
        .flat_map(|m| m.files.iter().map(|(r, d, _)| (r.as_str(), d.as_str())))
        .collect();
    let mut files: Vec<ImportedFile> = plan
        .items
        .iter()
        .map(|i| {
            let d = digest(&i.bytes);
            ImportedFile {
                rel: i.rel.clone(),
                path: dir.join(&i.rel).display().to_string(),
                bytes: i.bytes.len() as u64,
                changed: existing.get(i.rel.as_str()) != Some(&d.as_str())
                    || !dir.join(&i.rel).is_file(),
                digest: d,
                part_of: i.part_of.clone(),
                note: i.note,
            }
        })
        .collect();
    let removed = existing
        .keys()
        .any(|r| !plan.items.iter().any(|i| i.rel == *r));
    let unchanged = before.is_some() && !removed && files.iter().all(|f| !f.changed);
    let version = match (&before, unchanged) {
        (None, _) => 1,
        (Some(m), true) => m.version,
        (Some(m), false) => m.version + 1,
    };
    let draft = card::draft(plan.identity.as_deref(), plan.agents.as_deref());
    let mut proposals = Vec::new();
    for (rel, text) in &plan.feedback_from {
        for (kind, t) in feedback::of_note(text).items {
            let pr = feedback::proposal(&p.name, rel, kind, t);
            if !proposals.iter().any(|q: &ProposedStanding| q.id == pr.id) {
                proposals.push(pr);
            }
        }
    }
    let context_paths = files
        .iter()
        .filter(|f| !f.note)
        .map(|f| f.path.clone())
        .collect();
    let result = PersonaImportResult {
        name: p.name.clone(),
        agent: agent.clone(),
        dry_run: p.dry_run,
        dir: dir.display().to_string(),
        version,
        files: files.clone(),
        context_paths,
        over_limit: plan.over,
        skipped: plan.skipped,
        card: draft.clone(),
        unchanged,
        proposals: proposals.clone(),
        proposals_known: 0,
    };
    if p.dry_run || unchanged {
        return Ok((result, proposals));
    }
    write(&dir, &plan.items, before.as_ref(), version)?;
    let confirmed = before
        .as_ref()
        .is_some_and(|m| m.card_confirmed && m.card_draft == draft);
    let manifest = Manifest {
        name: p.name.clone(),
        agent,
        from: p.from.clone(),
        version,
        imported_ms: now_ms,
        files: files
            .drain(..)
            .map(|f| (f.rel, f.digest, f.bytes))
            .collect(),
        card_draft: draft,
        card_confirmed: confirmed,
    };
    manifest
        .write(&dir)
        .context("the import's record cannot be written")?;
    Ok((result, proposals))
}

/// Write the plan's files, moving the earlier version's aside first.
fn write(dir: &Path, items: &[Item], before: Option<&Manifest>, version: u32) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    if let Some(m) = before {
        let aside = dir.join(".versions").join(m.version.to_string());
        for (rel, _, _) in &m.files {
            let from = dir.join(rel);
            if !from.is_file() {
                continue;
            }
            let to = aside.join(rel);
            std::fs::create_dir_all(to.parent().unwrap_or(&aside))?;
            std::fs::rename(&from, &to)
                .with_context(|| format!("{} cannot be moved aside", from.display()))?;
        }
    }
    for i in items {
        let to = dir.join(&i.rel);
        std::fs::create_dir_all(to.parent().unwrap_or(dir))?;
        let tmp = to.with_extension("new");
        std::fs::write(&tmp, &i.bytes)?;
        std::fs::rename(&tmp, &to)?;
    }
    let _ = version;
    Ok(())
}

fn plan(from: &Path, names: &[String], limit: usize) -> Result<Plan> {
    let mut plan = Plan {
        items: Vec::new(),
        skipped: Vec::new(),
        over: Vec::new(),
        identity: None,
        agents: None,
        feedback_from: Vec::new(),
    };
    for name in names {
        if name.ends_with(".md") {
            top_file(from, name, limit, &mut plan)?;
        } else {
            notes(from, name, &mut plan)?;
        }
    }
    Ok(plan)
}

fn read_text(path: &Path) -> Result<Result<String, String>> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(Err("a link: not followed".into()));
    }
    let bytes = std::fs::read(path)?;
    Ok(String::from_utf8(bytes).map_err(|_| "not UTF-8 text".to_string()))
}

fn top_file(from: &Path, name: &str, limit: usize, plan: &mut Plan) -> Result<()> {
    let path = from.join(name);
    if std::fs::symlink_metadata(&path).is_err() {
        return Ok(());
    }
    let text = match read_text(&path)? {
        Ok(t) => t,
        Err(why) => {
            plan.skipped.push(format!("{name}: {why}"));
            return Ok(());
        }
    };
    if text.trim().is_empty() {
        plan.skipped.push(format!("{name}: empty"));
        return Ok(());
    }
    if name.eq_ignore_ascii_case("IDENTITY.md") {
        plan.identity = Some(text.clone());
    }
    if name.eq_ignore_ascii_case("AGENTS.md") {
        plan.agents = Some(text.clone());
    }
    if name.eq_ignore_ascii_case("MEMORY.md") {
        plan.feedback_from.push((name.to_string(), text.clone()));
    }
    if text.len() <= limit {
        plan.items.push(Item {
            rel: name.to_string(),
            bytes: text.into_bytes(),
            part_of: None,
            note: false,
        });
        return Ok(());
    }
    let parts = split::parts(&text, limit);
    plan.over.push(OverLimit {
        rel: name.to_string(),
        bytes: text.len() as u64,
        parts: parts.len() as u32,
    });
    for (n, part) in parts.iter().enumerate() {
        plan.items.push(Item {
            rel: split::part_name(name, n + 1),
            bytes: part.as_bytes().to_vec(),
            part_of: Some(name.to_string()),
            note: false,
        });
    }
    Ok(())
}

/// The `.md` files of a folder (and of its folders, one level down), sorted.
fn notes(from: &Path, folder: &str, plan: &mut Plan) -> Result<()> {
    let root = from.join(folder);
    if !root.is_dir() {
        return Ok(());
    }
    let mut found = Vec::new();
    collect(&root, folder, 0, &mut found)?;
    found.sort();
    for (rel, path) in found {
        match read_text(&path)? {
            Ok(t) if t.trim().is_empty() => plan.skipped.push(format!("{rel}: empty")),
            Ok(t) => {
                plan.feedback_from.push((rel.clone(), t.clone()));
                plan.items.push(Item {
                    rel,
                    bytes: t.into_bytes(),
                    part_of: None,
                    note: true,
                });
            }
            Err(why) => plan.skipped.push(format!("{rel}: {why}")),
        }
    }
    Ok(())
}

fn collect(dir: &Path, rel: &str, depth: usize, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = e.path();
        let kind = e.file_type()?;
        if kind.is_symlink() {
            out.push((format!("{rel}/{name}"), path));
        } else if kind.is_dir() && depth < 1 {
            collect(&path, &format!("{rel}/{name}"), depth + 1, out)?;
        } else if kind.is_file() && name.ends_with(".md") {
            out.push((format!("{rel}/{name}"), path));
        }
    }
    Ok(())
}
