//! `theseusd install` (M4 §2.9, step 22a, theseus-7hh): the daemon as a
//! systemd service, either the operator's own (`--user`), or as its own
//! `theseus` user with a group for the operator (`--separate`, as root).
//!
//! Each mode prints a plan and changes nothing; `--apply` performs it, and
//! `--check` compares the machine with it. Every mode is idempotent, and
//! `--apply` logs each thing it does. `--remove` is each mode's inverse.
//! Nothing here enables or starts a unit, and nothing runs on the daemon's
//! start path: `main` dispatches it before any runtime exists. A `--user`
//! unit's token file is checked, never read (`token.rs`), and `--apply`
//! refuses until it is right.

mod host;
mod layout;
mod migrate;
mod modes;
#[cfg(test)]
mod tests;
mod token;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use host::Host;
use layout::{Act, Body, Going, Item, Layout};

const OVERVIEW: &str = "\
theseusd install: run the daemon as a systemd service. Pick a mode:

  --user       Your own daemon as a systemd user service. Needs nothing but you: run it as
               yourself, not root (L1 runs no job of a root daemon).
               Writes ~/.config/systemd/user/theseusd.service; the state dir stays where it is.
  --separate   The daemon as its own `theseus` user, so a job, run as you, cannot read or write
               the store, the config's copy, or the vault token. Needs root: run it with sudo.
               Makes the theseus user and the theseus-ops group (you join it), /var/lib/theseus,
               /etc/theseus, /usr/local/lib/theseus/theseusd, /run/theseus, and both units.
               --migrate-state <dir> copies a stopped daemon's state into it; --remove takes it
               all away again (--purge-state moves a state dir that holds a store aside).

Each prints a plan and changes nothing. --apply performs it, and --check compares the machine
with it (exit 1 when anything differs). Every mode is idempotent. Neither enables nor starts a
unit: the plan says what to run.
";

/// `theseusd install`'s own flags. The daemon's `--config`, `--op-token-file`,
/// `--state-dir`, and `--socket` (or their variables) say how the daemon runs
/// today, which `--user` keeps.
#[derive(clap::Args, Debug, Default)]
pub(crate) struct InstallArgs {
    /// Your own daemon as a systemd user service. Needs nothing but you.
    #[arg(long, conflicts_with = "separate")]
    pub user: bool,
    /// The daemon as its own `theseus` user, the layout, and both units. Needs root (sudo).
    #[arg(long)]
    pub separate: bool,
    /// Perform the plan, logging each action (without it, only the plan is printed).
    #[arg(long, conflicts_with = "check")]
    pub apply: bool,
    /// Compare the machine with the layout: print what differs, and exit 1 if anything does.
    #[arg(long)]
    pub check: bool,
    /// The inverse: take the mode's layout away (a plan, unless --apply).
    #[arg(long)]
    pub remove: bool,
    /// With --separate --remove: move a state dir that holds a store aside. Nothing is deleted.
    #[arg(long, requires = "remove")]
    pub purge_state: bool,
    /// With --separate: copy a stopped daemon's state (store, spool, config copy, bindings) from DIR.
    #[arg(long, value_name = "DIR", conflicts_with = "remove")]
    pub migrate_state: Option<PathBuf>,
    /// With --separate: the operator, who joins theseus-ops (default: whoever ran sudo).
    #[arg(long, value_name = "USER")]
    pub operator: Option<String>,
    /// With --user --apply and no --op-token-file: the token comes from a drop-in (an
    /// `EnvironmentFile`) you write yourself, so the unit may name no token file.
    #[arg(long, requires = "user")]
    pub token_from_drop_in: bool,
}

/// The daemon's own flags, as this invocation has them.
#[derive(Debug, Clone, Default)]
pub(crate) struct Globals {
    pub config: String,
    pub op_token_file: Option<String>,
    pub state_dir: Option<PathBuf>,
    pub socket: Option<PathBuf>,
}

/// What the installer reads of its process: tests make their own.
#[derive(Debug, Clone)]
pub(crate) struct Env {
    /// Where the layout's paths are: `/`, or a temp dir in tests.
    pub root: PathBuf,
    /// This binary: what `--user`'s unit runs, and what `--separate` installs.
    pub exe: PathBuf,
    /// This command line, less the program: what a re-run repeats.
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub euid: u32,
    pub ruid: u32,
    /// The variables the installer reads, never a secret's value.
    pub vars: BTreeMap<String, String>,
    /// UTC, for a state dir moved aside.
    pub stamp: String,
}

/// The variables `Env` keeps.
const VARS: [&str; 10] = [
    "HOME",
    "XDG_CONFIG_HOME",
    "PATH",
    "LANG",
    "LC_ALL",
    "TZ",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "SUDO_UID",
    "SUDO_USER",
];

impl Env {
    fn from_process() -> Result<Self> {
        let exe = std::env::current_exe().context("finding this binary")?;
        if exe.to_string_lossy().ends_with(" (deleted)") {
            bail!("this binary was replaced or deleted since it started: run the one on disk");
        }
        let vars = VARS
            .iter()
            .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
            .collect();
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        Ok(Self {
            root: PathBuf::from("/"),
            exe,
            argv: std::env::args_os()
                .skip(1)
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
            cwd: std::env::current_dir().context("reading the working directory")?,
            euid: host::euid()?,
            ruid: host::ruid()?,
            vars,
            stamp: utc_stamp(secs),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The command that ran this, run again with `more` after it: a hint that
    /// keeps every flag the operator gave. `more` is written as given, so a
    /// placeholder such as `<file>` is not quoted.
    pub fn rerun_with(&self, more: &str) -> String {
        std::iter::once(shell_word(&self.exe.to_string_lossy()))
            .chain(self.argv.iter().map(|a| shell_word(a)))
            .chain(std::iter::once(more.to_string()))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn var(&self, k: &str) -> Option<&str> {
        self.vars
            .get(k)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// A path as a unit needs it: `~` expanded, and absolute.
    pub fn absolute(&self, p: &Path) -> PathBuf {
        let s = p.to_string_lossy();
        let p = match (s.strip_prefix("~/"), self.var("HOME")) {
            (Some(rest), Some(home)) => Path::new(home).join(rest),
            _ if s == "~" => self
                .var("HOME")
                .map_or_else(|| p.to_path_buf(), PathBuf::from),
            _ => p.to_path_buf(),
        };
        if p.is_absolute() {
            p
        } else {
            self.cwd.join(p)
        }
    }

    /// An executable on this environment's `PATH`.
    pub fn which(&self, name: &str) -> Option<PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        self.var("PATH")?
            .split(':')
            .filter(|d| !d.is_empty())
            .map(|d| Path::new(d).join(name))
            .find(|p| {
                std::fs::metadata(p)
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
    }
}

/// A word as a shell reads it back: plain characters as they are, anything
/// else in single quotes.
pub(crate) fn shell_word(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-:@+=,%".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// `YYYYMMDDTHHMMSSZ`.
pub(crate) fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Plan,
    Check,
    Apply,
}

/// `theseusd install`: its exit status.
pub(crate) fn run(args: &InstallArgs, g: &Globals) -> Result<i32> {
    let env = Env::from_process()?;
    let mut out = Out(std::io::stdout());
    run_with(args, g, &env, &mut host::System, &mut out)
}

/// `run`, on any host, root, and output.
pub(crate) fn run_with(
    args: &InstallArgs,
    g: &Globals,
    env: &Env,
    host: &mut dyn Host,
    out: &mut dyn Write,
) -> Result<i32> {
    if !args.user && !args.separate {
        if args.apply
            || args.check
            || args.remove
            || args.purge_state
            || args.migrate_state.is_some()
            || args.operator.is_some()
        {
            bail!("name a mode first: --user or --separate (`theseusd install` lists both)");
        }
        out.write_all(OVERVIEW.as_bytes())?;
        return Ok(0);
    }
    let mode = match (args.apply, args.check) {
        (true, _) => Mode::Apply,
        (_, true) => Mode::Check,
        _ => Mode::Plan,
    };
    let layout = if args.user {
        if args.purge_state || args.migrate_state.is_some() || args.operator.is_some() {
            bail!("--purge-state, --migrate-state, and --operator go with --separate");
        }
        if env.euid == 0 {
            bail!(
                "`--user` installs your own daemon's unit: run it as yourself, not as root (no \
                 sudo)"
            );
        }
        // The unnamed token stays a note in a plan, so a drop-in's token isn't blocked; but
        // --apply writes a unit whose daemon cannot start, unless the operator says why not
        // (theseus-4xyj).
        if mode == Mode::Apply
            && !args.remove
            && g.op_token_file.is_none()
            && !args.token_from_drop_in
        {
            bail!(
                "nothing was changed: the unit would name no token file, and the daemon will not \
                 start without its 1Password token. Name one with --op-token-file <file> \
                 (THESEUS_OP_TOKEN_FILE), or, if a drop-in supplies the token \
                 (`systemctl --user edit theseusd`, an `EnvironmentFile`), add --token-from-drop-in"
            );
        }
        modes::user(env, g, args.remove, host)?
    } else {
        if mode != Mode::Plan && env.euid != 0 {
            bail!(
                "`--separate` needs root: run it with sudo:\n  sudo {} install {}",
                env.exe.display(),
                separate_flags(args)
            );
        }
        modes::separate(env, g, args, host)?
    };
    execute(&layout, mode, env.root(), host, out)
}

/// The flags, as typed, for the line that says how to run it with sudo.
fn separate_flags(a: &InstallArgs) -> String {
    let mut f = vec!["--separate".to_string()];
    if a.remove {
        f.push("--remove".into());
    }
    if a.purge_state {
        f.push("--purge-state".into());
    }
    if let Some(d) = &a.migrate_state {
        f.push(format!("--migrate-state {}", d.display()));
    }
    if let Some(o) = &a.operator {
        f.push(format!("--operator {o}"));
    }
    f.push(if a.check { "--check" } else { "--apply" }.into());
    f.join(" ")
}

/// Print the plan, check the machine, or apply: the exit status.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
pub(crate) fn execute(
    l: &Layout,
    mode: Mode,
    root: &Path,
    host: &mut dyn Host,
    out: &mut dyn Write,
) -> Result<i32> {
    let mut going = Going::default();
    let mut acts = vec![];
    for e in &l.entries {
        let act = match e.item.inspect(root, &*host, &going) {
            Ok(a) => a,
            // A plan needs no root, so the layout's private places may be
            // closed to it: it says so rather than failing.
            Err(err) if mode == Mode::Plan && denied(&err) => Act::Unseen(format!(
                "not visible without root ({err:#}): run the plan with sudo to see it"
            )),
            Err(err) => return Err(err),
        };
        e.item.goes(&act, &mut going);
        acts.push(act);
    }
    let refused = acts.iter().filter(|a| matches!(a, Act::Refuse(_))).count();
    let unseen = acts.iter().filter(|a| matches!(a, Act::Unseen(_))).count();
    match mode {
        Mode::Plan => {
            let sudo = if l.command.contains("--separate") {
                ", as root"
            } else {
                ""
            };
            writeln!(
                out,
                "{}: the plan. Nothing is changed: --apply performs it{sudo}.",
                l.command
            )?;
            for c in &l.context {
                writeln!(out, "  {c}")?;
            }
            writeln!(out)?;
            for (e, a) in l.entries.iter().zip(&acts) {
                render(out, &e.item, a, Some(&e.why))?;
            }
            writeln!(out)?;
            let todo = acts.iter().filter(|a| a.changes()).count();
            if refused > 0 {
                writeln!(
                    out,
                    "{refused} refused: --apply changes nothing until each is resolved."
                )?;
            } else if todo == 0 && unseen == 0 {
                writeln!(out, "Nothing to do: the machine matches the layout.")?;
            } else {
                writeln!(
                    out,
                    "{todo} to do, {} already done.",
                    acts.len() - todo - unseen
                )?;
            }
            if unseen > 0 {
                writeln!(
                    out,
                    "{unseen} not visible without root: run the plan with sudo to see them."
                )?;
            }
            notes(out, l)?;
            Ok(i32::from(refused > 0))
        }
        Mode::Check => {
            let differ: Vec<_> = l
                .entries
                .iter()
                .zip(&acts)
                .filter(|(_, a)| a.changes() || matches!(a, Act::Refuse(_)))
                .collect();
            if differ.is_empty() {
                writeln!(out, "{}: the machine matches the layout.", l.command)?;
                return Ok(0);
            }
            writeln!(
                out,
                "{}: {} difference(s) from the layout:",
                l.command,
                differ.len()
            )?;
            for (e, a) in differ {
                render(out, &e.item, a, None)?;
            }
            Ok(1)
        }
        Mode::Apply => {
            if refused > 0 {
                for (e, a) in l.entries.iter().zip(&acts) {
                    if matches!(a, Act::Refuse(_)) {
                        render(out, &e.item, a, None)?;
                    }
                }
                bail!("nothing was changed: {refused} refused (above)");
            }
            writeln!(out, "{}: applying.", l.command)?;
            let mut done = 0;
            for e in &l.entries {
                // Looked at again: the items before it have done their part.
                let act = e.item.inspect(root, &*host, &Going::default())?;
                if let Act::Refuse(why) = &act {
                    bail!("stopped after {done} change(s): {why}; the changes before it stay, and a re-run finishes");
                }
                if !act.changes() {
                    continue;
                }
                let mut log = |line: String| {
                    let _ = writeln!(out, "  {line}");
                };
                e.item.apply(&act, root, host, &l.parents, &mut log)?;
                done += 1;
            }
            if done == 0 {
                writeln!(out, "Nothing to do: the machine matches the layout.")?;
            } else {
                writeln!(out, "Done: {done} change(s).")?;
            }
            notes(out, l)?;
            Ok(0)
        }
    }
}

/// One item in a plan or a check: its verb, kind, and what; then what
/// differs, why it stays, or why it is refused; then, in a plan, why the
/// layout has it, and any text it would write.
fn render(out: &mut dyn Write, item: &Item, act: &Act, why: Option<&str>) -> Result<()> {
    let (kind, what) = item.describe();
    writeln!(out, "  {:<7} {kind:<6} {what}", item.verb(act))?;
    let pad = "                 ";
    match act {
        Act::Fix(fix) => {
            for f in fix {
                writeln!(out, "{pad}{f}")?;
            }
        }
        Act::MoveAside(to) => writeln!(out, "{pad}to {}", to.display())?,
        Act::Keep { why, fix } => {
            writeln!(out, "{pad}stays: {why}")?;
            for f in fix {
                writeln!(out, "{pad}{f}")?;
            }
        }
        Act::Refuse(r) => writeln!(out, "{pad}refused: {r}")?,
        Act::Unseen(why) => writeln!(out, "{pad}{why}")?,
        _ => {}
    }
    if let Some(why) = why {
        writeln!(out, "{pad}{why}")?;
        if let (
            Item::File {
                body: Body::Text(t),
                ..
            },
            Act::Create | Act::Fix(_),
        ) = (item, act)
        {
            for line in t.lines() {
                if line.is_empty() {
                    writeln!(out, "{pad}|")?;
                } else {
                    writeln!(out, "{pad}| {line}")?;
                }
            }
        }
    }
    Ok(())
}

/// Whether an error is the filesystem saying no.
fn denied(err: &anyhow::Error) -> bool {
    err.chain().any(|c| {
        c.downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied)
    })
}

fn notes(out: &mut dyn Write, l: &Layout) -> Result<()> {
    if l.notes.is_empty() {
        return Ok(());
    }
    writeln!(out, "\nThen, by hand:")?;
    for n in &l.notes {
        writeln!(out, "  - {n}")?;
    }
    Ok(())
}

/// Standard output that a reader who went away does not fail: `--apply` goes
/// on to its end, and its log is best effort.
struct Out(std::io::Stdout);

impl Write for Out {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.0.write(buf) {
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(buf.len()),
            r => r,
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self.0.flush() {
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
            r => r,
        }
    }
}
