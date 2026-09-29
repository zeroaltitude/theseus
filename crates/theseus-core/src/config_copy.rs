//! The last-known-good copy of the vault's config note (theseus-2fo, spec
//! §3.19). A start whose config is an `op://` reference serves from this copy
//! at once and reads the vault behind the socket; nothing acts until the
//! vault confirms the copy (`config_gate`). The note holds only `op://`
//! references, so the copy holds no secret. The vault stays the one
//! authority: the copy is a file the operator's user can write, and at L0 a
//! job runs as that user (theseus-6qy), while an agent cannot write the vault.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::Config;

/// The copy's name in the state dir.
pub const FILE: &str = "config.last-good.toml";

/// The copy's first line names the reference it came from, and the note's
/// exact text follows it.
const HEADER: &str = "# theseusd: the last-known-good copy of ";

/// Where the copy lives: `--state-dir` when given, else `~/.theseus`, since it
/// must be found before any config is read.
pub fn path(state_dir: Option<&Path>) -> PathBuf {
    state_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| crate::config::expand(crate::config::DEFAULT_STATE_DIR))
        .join(FILE)
}

/// A copy that can serve a start.
pub struct Copy {
    /// The note's exact text, as the vault last gave it.
    pub text: String,
    pub config: Config,
    /// The retired keys it still uses, for the start to log.
    pub warnings: Vec<String>,
}

/// The copy of `source` at `path`: `Ok(None)` if there is none, `Err(why)` if
/// it cannot serve this start (unreadable, another reference's, or it does
/// not load). Either way that start reads the vault first, as a first start.
pub fn read(path: &Path, source: &str) -> Result<Option<Copy>, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("it cannot be read: {e}")),
    };
    let (header, text) = raw.split_once('\n').unwrap_or((raw.as_str(), ""));
    match header.strip_prefix(HEADER) {
        Some(r) if r == source => {}
        Some(r) => return Err(format!("it is the copy of {r}, not of {source}")),
        None => return Err("its first line does not name the reference it came from".into()),
    }
    let (config, warnings) = Config::parse(text).map_err(|e| format!("it does not load: {e:#}"))?;
    Ok(Some(Copy {
        text: text.to_string(),
        config,
        warnings,
    }))
}

/// Keep `text`, the note `source` names, as its copy: mode 0600, written to
/// a temporary file and renamed into place, so a reader finds the old copy
/// or the new one and never part of either. No fsync: a copy lost in a crash
/// costs one start that reads the vault first.
pub fn write(path: &Path, source: &str, text: &str) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let dir = path.parent().context("the copy's path has no directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let tmp = dir.join(format!(".{FILE}.{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("creating {}", tmp.display()))?;
    f.write_all(format!("{HEADER}{source}\n").as_bytes())?;
    f.write_all(text.as_bytes())?;
    drop(f);
    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming the copy into {}", path.display()))
}

/// The digest `config.changed` names each version by.
pub fn sha256(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// How the vault's note stands against the copy a start served from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Compared {
    /// The same text.
    Same,
    /// Only comments or formatting differ: the parsed documents are equal.
    Comments,
    /// The documents differ, in these tables, by dotted name (never a value).
    Changed(Vec<String>),
}

/// Compare the copy's text with the vault's. Both have loaded.
pub fn compare(copy: &str, vault: &str) -> Compared {
    if copy == vault {
        return Compared::Same;
    }
    let doc = |t: &str| t.parse::<toml::Table>().unwrap_or_default();
    let (a, b) = (doc(copy), doc(vault));
    if a == b {
        return Compared::Comments;
    }
    let mut out = BTreeSet::new();
    tables(&a, &b, "", &mut out);
    Compared::Changed(out.into_iter().collect())
}

/// The tables where `a` and `b` differ. A key outside any table is named by
/// itself; a table added or removed is named whole, with its sub-tables.
fn tables(a: &toml::Table, b: &toml::Table, at: &str, out: &mut BTreeSet<String>) {
    let name = |k: &str| match at {
        "" => k.to_string(),
        _ => format!("{at}.{k}"),
    };
    let none = toml::Table::new();
    let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    for k in keys {
        let (x, y) = (a.get(k.as_str()), b.get(k.as_str()));
        if x == y {
            continue;
        }
        match (x, y) {
            (Some(toml::Value::Table(x)), Some(toml::Value::Table(y))) => {
                tables(x, y, &name(k), out)
            }
            (Some(toml::Value::Table(t)), None) | (None, Some(toml::Value::Table(t))) => {
                if t.is_empty() || t.values().any(|v| !v.is_table()) {
                    out.insert(name(k));
                }
                tables(t, &none, &name(k), out);
            }
            _ if at.is_empty() => {
                out.insert(k.clone());
            }
            _ => {
                out.insert(at.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REF: &str = "op://V/theseus-config/notesPlain";
    const NOTE: &str = "[secrets]\nanthropic_api_key = \"op://V/i/f\"\n";

    /// A copy is the note's exact text under a line naming its reference,
    /// mode 0600, read back as it was written.
    #[test]
    fn a_copy_is_written_0600_and_reads_back_for_its_own_reference() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = path(Some(d.path()));
        assert_eq!(p, d.path().join("config.last-good.toml"));
        assert!(read(&p, REF).unwrap().is_none(), "none yet");
        let text = format!("# Eddie's note\n{NOTE}");
        write(&p, REF, &text).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let raw = std::fs::read_to_string(&p).unwrap();
        assert_eq!(raw, format!("{HEADER}{REF}\n{text}"));
        let c = read(&p, REF).unwrap().unwrap();
        assert_eq!(c.text, text);
        assert!(c.config.secrets.contains_key("anthropic_api_key"));
        // Another reference's copy serves no start.
        let e = read(&p, "op://V/other/notesPlain").err().unwrap();
        assert!(
            e.contains("the copy of op://V/theseus-config/notesPlain"),
            "{e}"
        );
        // Rewritten in place, and no temporary file is left.
        write(&p, REF, NOTE).unwrap();
        assert_eq!(read(&p, REF).unwrap().unwrap().text, NOTE);
        let names: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["config.last-good.toml"]);
    }

    /// A copy that does not load, or whose first line names no reference, is
    /// ignored with the reason: that start reads the vault first.
    #[test]
    fn a_copy_that_does_not_load_is_ignored_with_the_reason() {
        let d = tempfile::tempdir().unwrap();
        let p = path(Some(d.path()));
        write(&p, REF, "[secrets\nbroken").unwrap();
        let e = read(&p, REF).err().unwrap();
        assert!(e.starts_with("it does not load"), "{e}");
        write(&p, REF, "[kernel]\nspend_limit_usd = -1.0\n").unwrap();
        assert!(read(&p, REF).err().unwrap().starts_with("it does not load"));
        std::fs::write(&p, NOTE).unwrap();
        let e = read(&p, REF).err().unwrap();
        assert!(e.contains("does not name the reference"), "{e}");
    }

    #[test]
    fn the_vault_is_the_same_only_comments_differ_or_named_tables_changed() {
        assert_eq!(compare(NOTE, NOTE), Compared::Same);
        let commented = format!("# a comment\n{}\n\n", NOTE.replace(" = ", "="));
        assert_eq!(compare(NOTE, &commented), Compared::Comments);
        let changed = format!(
            "narrative = true\n{NOTE}\n[kernel]\nspend_limit_usd = 5.0\n\n\
             [policy.tools]\n\"proc.run\" = \"approve\"\n\n\
             [profiles.glm]\nmodel = \"glm-5.3\"\n"
        );
        assert_eq!(
            compare(NOTE, &changed),
            Compared::Changed(vec![
                "kernel".into(),
                "narrative".into(),
                "policy.tools".into(),
                "profiles.glm".into(),
            ])
        );
        // One value in one table; a table dropped; never a value named.
        let a = format!("{NOTE}[kernel]\nspend_limit_usd = 5.0\nheartbeat_secs = 60\n");
        let b = format!("{NOTE}[kernel]\nspend_limit_usd = 7.5\nheartbeat_secs = 60\n");
        assert_eq!(compare(&a, &b), Compared::Changed(vec!["kernel".into()]));
        assert_eq!(compare(&a, NOTE), Compared::Changed(vec!["kernel".into()]));
        let Compared::Changed(t) = compare(&a, &b) else {
            unreachable!()
        };
        assert!(!t.iter().any(|s| s.contains('5') || s.contains('7')));
        assert_eq!(sha256("x").len(), 64);
        assert_ne!(sha256(&a), sha256(&b));
    }
}
