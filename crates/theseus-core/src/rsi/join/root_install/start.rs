//! The start command in force: which file of the user manager's unit path holds the `ExecStart=` that systemd will
//! run, so the install changes that line and no other (issue 20, "the first install by the helper did not take").
//!
//! systemd composes a service's settings from its unit file and every drop-in (`<unit>.d/*.conf`, and the type-level
//! `service.d/*.conf`) in the directories of its unit path. The unit file is the first one found along the path,
//! highest priority first. Drop-ins of every directory are sorted together by file name, and a name found in a
//! higher-priority directory hides the same name in a lower one. Read in that order, an `ExecStart=` with nothing after
//! it clears every command set before it, and a later one adds to what is left: the commands left at the end are what
//! runs. A service with one command has its winner in the last file that set it.
//!
//! The directories read, for a user service (`systemd.unit(5)`, "Unit File Load Path"), highest priority first:
//! `$XDG_CONFIG_HOME/systemd/user.control`, the runtime `transient` and `generator.early`, `$XDG_CONFIG_HOME/systemd/user`
//! (`~/.config/systemd/user`), each `$XDG_CONFIG_DIRS/systemd/user`, `/etc/systemd/user`, the runtime `user`,
//! `/run/systemd/user`, the runtime `generator`, `$XDG_DATA_HOME/systemd/user`, each `$XDG_DATA_DIRS/systemd/user`,
//! `/usr/local/lib/systemd/user`, `/usr/lib/systemd/user`, and the runtime `generator.late`.
//!
//! The service's user may change only its own: the two under `$XDG_CONFIG_HOME` and the one under `$XDG_DATA_HOME`.
//! The administrator's (`/etc`, `/usr`) are not the user's to write, and the runtime ones are gone at the next boot or
//! belong to systemd: a start command that wins from any of those is **refused**, in words, before anything changes,
//! because a line written anywhere else would not be the one that wins. This module only reads the files to find the
//! winner. What systemd will really run is asked of systemd afterwards (`Units::will_run`), so a layout this reading
//! misjudged still cannot be restarted onto the wrong program.

use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// Whose a directory of the unit path is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// The service's user's own: written by that user.
    Own,
    /// Runtime directories: systemd's own, or gone at the next boot.
    Volatile,
    /// The administrator's: `/etc`, `/usr`.
    Admin,
}

/// A directory of the unit path.
#[derive(Debug, Clone)]
pub struct Dir {
    pub path: PathBuf,
    pub place: Place,
}

/// A file that takes part in the service's settings: the unit file, or one drop-in.
#[derive(Debug, Clone)]
pub struct Piece {
    pub path: PathBuf,
    pub place: Place,
    pub text: String,
}

fn dir(path: impl Into<PathBuf>, place: Place) -> Dir {
    Dir {
        path: path.into(),
        place,
    }
}

/// An absolute directory list from an XDG variable's value, or its default.
fn xdg_list(var: &str, default: &[&str]) -> Vec<PathBuf> {
    let set = std::env::var(var).ok().filter(|v| !v.is_empty());
    let all: Vec<PathBuf> = match set {
        Some(v) => v.split(':').map(PathBuf::from).collect(),
        None => default.iter().map(PathBuf::from).collect(),
    };
    all.into_iter().filter(|p| p.is_absolute()).collect()
}

fn xdg_home(var: &str, under_home: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(under_home)))
}

/// The user manager's unit path as this process's environment gives it, highest priority first.
pub fn search_path() -> Result<Vec<Dir>> {
    let config = xdg_home("XDG_CONFIG_HOME", ".config").context("HOME is not set")?;
    let data = xdg_home("XDG_DATA_HOME", ".local/share").context("HOME is not set")?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    let rt = |name: &str| {
        runtime
            .as_ref()
            .map(|r| dir(r.join("systemd").join(name), Place::Volatile))
    };
    let mut out = vec![dir(config.join("systemd/user.control"), Place::Own)];
    out.extend(rt("transient"));
    out.extend(rt("generator.early"));
    out.push(dir(config.join("systemd/user"), Place::Own));
    for d in xdg_list("XDG_CONFIG_DIRS", &["/etc/xdg"]) {
        out.push(dir(d.join("systemd/user"), Place::Admin));
    }
    out.push(dir("/etc/systemd/user", Place::Admin));
    out.extend(rt("user"));
    out.push(dir("/run/systemd/user", Place::Volatile));
    out.extend(rt("generator"));
    out.push(dir(data.join("systemd/user"), Place::Own));
    for d in xdg_list("XDG_DATA_DIRS", &["/usr/local/share", "/usr/share"]) {
        out.push(dir(d.join("systemd/user"), Place::Admin));
    }
    out.push(dir("/usr/local/lib/systemd/user", Place::Admin));
    out.push(dir("/usr/lib/systemd/user", Place::Admin));
    out.extend(rt("generator.late"));
    let mut seen = Vec::new();
    out.retain(|d| {
        let new = !seen.contains(&d.path);
        seen.push(d.path.clone());
        new
    });
    Ok(out)
}

/// Whether `path` is a link to `/dev/null`: how a unit or a drop-in is masked.
fn masked(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
        && std::fs::canonicalize(path).is_ok_and(|c| c == Path::new("/dev/null"))
}

/// The unit file and the drop-ins in force for `unit`, in the order systemd applies them.
pub fn pieces(dirs: &[Dir], unit: &str) -> Result<Vec<Piece>> {
    let read = |path: &Path, place: Place| -> Result<Piece> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Ok(Piece {
            path: path.to_path_buf(),
            place,
            text,
        })
    };
    let main = dirs
        .iter()
        .map(|d| (d.path.join(unit), d.place))
        .find(|(p, _)| std::fs::symlink_metadata(p).is_ok())
        .with_context(|| format!("no unit file {unit} in the user manager's unit path"))?;
    if masked(&main.0) {
        bail!(
            "{unit} is masked ({} is a link to /dev/null)",
            main.0.display()
        );
    }
    let mut out = vec![read(&main.0, main.1)?];
    // Drop-ins: a name in a higher-priority directory hides the same name below it; the unit's own directory hides the
    // type-level one of the same name; all of them are applied sorted by name.
    let mut found: Vec<(String, PathBuf, Place, bool)> = Vec::new();
    for d in dirs {
        for sub in [format!("{unit}.d"), "service.d".to_string()] {
            let Ok(entries) = std::fs::read_dir(d.path.join(&sub)) else {
                continue;
            };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !name.ends_with(".conf") {
                    continue;
                }
                if found.iter().any(|(n, ..)| *n == name) {
                    continue;
                }
                found.push((name, e.path(), d.place, masked(&e.path())));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, path, place, hidden) in found {
        if !hidden && std::fs::metadata(&path).is_ok_and(|m| m.is_file()) {
            out.push(read(&path, place)?);
        }
    }
    Ok(out)
}

/// One `ExecStart=` line: clears what was set before it, or sets a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Clear,
    Set {
        /// The byte range of the command's first word in the text (what a change replaces), when it is an absolute path
        /// on the line itself; none for a prefix (`-`, `@`, `!`), a quote, a specifier, or a command on the next line.
        program: Option<Range<usize>>,
    },
}

/// The `ExecStart=` lines of a file's `[Service]` section, in order, with where each one's program is.
pub fn lines(text: &str) -> Vec<Line> {
    let mut out = Vec::new();
    let mut in_service = false;
    let mut continued = false;
    let mut at = 0;
    for raw in text.split_inclusive('\n') {
        let start = at;
        at += raw.len();
        let body = raw.trim_end_matches(['\n', '\r']);
        let was_continued = continued;
        continued = body.ends_with('\\');
        if was_continued {
            continue;
        }
        let t = body.trim_start();
        if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
            continued = false;
            continue;
        }
        if t.starts_with('[') {
            in_service = t.trim_end() == "[Service]";
            continue;
        }
        if !in_service {
            continue;
        }
        let Some((key, value)) = t.split_once('=') else {
            continue;
        };
        if key.trim_end() != "ExecStart" {
            continue;
        }
        let value_at = start + (body.len() - value.len());
        let lead = value.len() - value.trim_start().len();
        let word = value.trim_start();
        if word.trim_end().is_empty() {
            out.push(Line::Clear);
            continue;
        }
        let end = word.find(char::is_whitespace).unwrap_or(word.len());
        let first = &word[..end];
        let program = (first.starts_with('/') && !first.ends_with('\\'))
            .then(|| (value_at + lead)..(value_at + lead + end));
        out.push(Line::Set { program });
    }
    out
}

/// What is left of the `ExecStart=` lines of one file after its last clear.
fn left(text: &str) -> Vec<Line> {
    let all = lines(text);
    let from = all
        .iter()
        .rposition(|l| *l == Line::Clear)
        .map_or(0, |i| i + 1);
    all[from..].to_vec()
}

/// The one program a file's own `ExecStart=` lines leave, and where its first word is in the text.
pub fn one_in(text: &str) -> Option<(String, Range<usize>)> {
    match left(text).as_slice() {
        [Line::Set { program: Some(r) }] => Some((text[r.clone()].to_string(), r.clone())),
        _ => None,
    }
}

/// The piece of `pieces` whose line wins, and the program it names. Refused, in words, when no file sets a start
/// command, when more than one is left, or when the one left is not a plain absolute path.
pub fn winner(pieces: &[Piece]) -> Result<(usize, String)> {
    let mut left: Vec<(usize, Line)> = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        for l in lines(&p.text) {
            match l {
                Line::Clear => left.clear(),
                set => left.push((i, set)),
            }
        }
    }
    match left.as_slice() {
        [] => bail!("no file of the service sets a start command (no ExecStart=)"),
        [(i, Line::Set { program: Some(r) })] => Ok((*i, pieces[*i].text[r.clone()].to_string())),
        [(i, _)] => bail!(
            "the start command in {} does not begin with a plain absolute path to a program",
            pieces[*i].path.display()
        ),
        more => bail!(
            "the service has {} start commands left after its drop-ins ({}), and Theseus changes only one",
            more.len(),
            more.iter()
                .map(|(i, _)| pieces[*i].path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Why Theseus will not write `piece`, in words for the owner; none when it may.
pub fn refusal(piece: &Piece, own: impl Fn(&Path) -> bool) -> Option<String> {
    let at = piece.path.display();
    match piece.place {
        Place::Admin => Some(format!(
            "this machine's service takes its start command from {at}, which belongs to the administrator: Theseus \
             does not write there, and a line written anywhere else would not be the one that wins"
        )),
        Place::Volatile => Some(format!(
            "this machine's service takes its start command from {at}, a place systemd clears at every restart of \
             the machine, so Theseus does not change it"
        )),
        Place::Own => {
            let link = std::fs::symlink_metadata(&piece.path).is_ok_and(|m| m.file_type().is_symlink());
            if link {
                Some(format!(
                    "this machine's service takes its start command from {at}, which is a link to another file: \
                     Theseus does not write over or through a link"
                ))
            } else if !own(&piece.path) {
                Some(format!(
                    "this machine's service takes its start command from {at}, which this user cannot write"
                ))
            } else {
                None
            }
        }
    }
}

/// The programs in `systemctl --user show -p ExecStart`'s text: one `{ path=/a/b ; argv[]=... ; ... }` per command.
/// Split at the braces' own `{ path=`, so an argument that happens to say `path=` is not a program.
pub fn programs_of(shown: &str) -> Vec<String> {
    shown
        .split("{ path=")
        .skip(1)
        .filter_map(|rest| rest.split(" ;").next())
        .map(|p| p.trim().to_string())
        .collect()
}
