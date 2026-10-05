//! The layouts `theseusd install` makes or takes away, as lists of items
//! (theseus-7hh). Each item looks at the machine and says what it would do,
//! its act: the plan prints the acts, `--check` prints the ones that change
//! something, and `--apply` looks again just before each one and does it.

use std::fmt;
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::host::Host;
use super::migrate::Migrate;

/// The first words of every file the installer writes. `--remove` takes
/// away only a file that starts with them.
pub(crate) const HEADER: &str = "# Written by `theseusd install";

pub(crate) const DAEMON_USER: &str = "theseus";
pub(crate) const OPS_GROUP: &str = "theseus-ops";
pub(crate) const STATE_DIR: &str = "/var/lib/theseus";
pub(crate) const ETC_DIR: &str = "/etc/theseus";
pub(crate) const TOKEN_FILE: &str = "/etc/theseus/op-token";
pub(crate) const CONFIG_FILE: &str = "/etc/theseus/theseus.toml";
pub(crate) const LIB_DIR: &str = "/usr/local/lib/theseus";
pub(crate) const BINARY: &str = "/usr/local/lib/theseus/theseusd";
pub(crate) const RUN_DIR: &str = "/run/theseus";
pub(crate) const SOCKET: &str = "/run/theseus/theseus.sock";
pub(crate) const SYSTEM_UNIT: &str = "/etc/systemd/system/theseusd.service";
pub(crate) const JOB_HOST_UNIT: &str = "/etc/systemd/user/theseus-job-host.service";
pub(crate) const TMPFILES: &str = "/etc/tmpfiles.d/theseus.conf";

/// Who owns a path, by name: the names resolve when the item acts, after
/// the items before it made the accounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Owner {
    pub user: String,
    pub group: String,
}

impl Owner {
    pub fn new(user: &str, group: &str) -> Self {
        Self {
            user: user.into(),
            group: group.into(),
        }
    }

    pub fn root() -> Self {
        Self::new("root", "root")
    }

    /// Its uid and gid, once both accounts exist.
    fn ids(&self, host: &dyn Host) -> Result<Option<(u32, u32)>> {
        Ok(match (host.user(&self.user)?, host.group(&self.group)?) {
            (Some(u), Some(g)) => Some((u.uid, g.gid)),
            _ => None,
        })
    }

    fn resolve(&self, host: &dyn Host) -> Result<(u32, u32)> {
        self.ids(host)?
            .with_context(|| format!("no account for {self}: the user or the group does not exist"))
    }
}

impl fmt::Display for Owner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.user, self.group)
    }
}

/// What a file holds.
#[derive(Debug, Clone)]
pub(crate) enum Body {
    /// Generated text, rewritten whenever it differs.
    Text(String),
    /// A copy of another file, after a header line if there is one.
    Copy {
        from: PathBuf,
        header: Option<String>,
    },
    /// Made empty, and never written again: the token's place.
    Placeholder,
}

/// When `--remove` takes a file away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileRule {
    /// Only if it starts with the installer's header.
    Generated,
    /// Always: the installed binary.
    Binary,
    /// Only while it is empty. A token in it is kept, made root's.
    Token,
}

/// When `--remove` takes a directory away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirRule {
    /// Only once it is empty.
    IfEmpty,
    /// Once it holds only sockets, which go with it: `/run/theseus`.
    Sockets,
    /// The state dir: only when empty. One that holds anything is kept, or
    /// with `purge` moved aside to `<path>.removed-<stamp>`. Never deleted.
    State { purge: bool, stamp: String },
}

#[derive(Debug, Clone)]
pub(crate) enum Item {
    Group(String),
    User {
        name: String,
        home: PathBuf,
        shell: PathBuf,
    },
    Member {
        group: String,
        user: String,
    },
    Dir {
        path: PathBuf,
        owner: Owner,
        mode: u32,
    },
    File {
        path: PathBuf,
        owner: Owner,
        mode: u32,
        body: Body,
    },
    State(Migrate),
    /// The token file a `--user` unit names: looked at, never opened, and
    /// refused until it is the operator's `uid` alone (`token.rs`).
    Token {
        path: PathBuf,
        uid: u32,
        name: String,
    },
    /// A socket no daemon may be answering on.
    Stopped(PathBuf),
    NoFile {
        path: PathBuf,
        rule: FileRule,
    },
    /// `seal`: a directory that stays is made `root:root 0700`, so no
    /// account that later gets the removed one's ids can read it.
    NoDir {
        path: PathBuf,
        rule: DirRule,
        seal: bool,
    },
    NoMember {
        group: String,
        user: String,
    },
    NoUser {
        name: String,
        home: PathBuf,
    },
    NoGroup(String),
}

/// An item, and why the layout has it.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub item: Item,
    pub why: String,
}

/// What an item would do, looking at the machine as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Act {
    InPlace,
    Create,
    /// It is there, and these differ.
    Fix(Vec<String>),
    Remove,
    MoveAside(PathBuf),
    /// It stays, for `why`; `fix` is what is made safe on the way.
    Keep {
        why: String,
        fix: Vec<String>,
    },
    Absent,
    Refuse(String),
    /// A plan run without root cannot look: the layout's private places.
    Unseen(String),
}

/// What the items before this one in a plan will take away: a plan reads
/// the machine once, and `--apply` acts in order, so a directory whose last
/// file an earlier item removes is planned as removed, not kept.
#[derive(Debug, Default)]
pub(crate) struct Going {
    paths: std::collections::BTreeSet<PathBuf>,
    users: std::collections::BTreeSet<String>,
}

impl Act {
    /// Whether doing it changes the machine.
    pub fn changes(&self) -> bool {
        match self {
            Act::Create | Act::Fix(_) | Act::Remove | Act::MoveAside(_) => true,
            Act::Keep { fix, .. } => !fix.is_empty(),
            Act::InPlace | Act::Absent | Act::Refuse(_) | Act::Unseen(_) => false,
        }
    }
}

/// A layout: its items in the order they act, and what the operator does
/// by hand around it.
#[derive(Debug, Clone)]
pub(crate) struct Layout {
    /// The command, as the operator would type it.
    pub command: String,
    /// What the plan says first: who, from where.
    pub context: Vec<String>,
    pub entries: Vec<Entry>,
    /// Who owns a parent directory the installer makes.
    pub parents: Owner,
    /// What to run by hand, in order.
    pub notes: Vec<String>,
}

/// A layout path on the machine under `root`.
pub(crate) fn real(root: &Path, path: &Path) -> PathBuf {
    root.join(path.strip_prefix("/").unwrap_or(path))
}

/// Whether a socket answers, within two seconds: a connect that a listener
/// holds counts as an answer.
pub(crate) fn answers(sock: &Path) -> bool {
    let p = sock.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(std::os::unix::net::UnixStream::connect(&p).is_ok());
    });
    rx.recv_timeout(std::time::Duration::from_secs(2))
        .unwrap_or(true)
}

fn is_nologin(shell: &Path) -> bool {
    shell
        .file_name()
        .is_some_and(|n| n == "nologin" || n == "false")
}

/// `user:group` for ids, by name where the accounts have one.
fn names(host: &dyn Host, (uid, gid): (u32, u32)) -> Result<String> {
    let u = host
        .user_by_uid(uid)?
        .map_or_else(|| uid.to_string(), |u| u.name);
    let g = host
        .group_by_gid(gid)?
        .map_or_else(|| gid.to_string(), |g| g.name);
    Ok(format!("{u}:{g}"))
}

fn mode_of(m: &std::fs::Metadata) -> u32 {
    m.permissions().mode() & 0o7777
}

/// What differs in a path's owner and mode.
fn owner_and_mode(
    host: &dyn Host,
    at: &Path,
    m: &std::fs::Metadata,
    owner: &Owner,
    mode: u32,
) -> Result<Vec<String>> {
    let mut fix = vec![];
    let have = host.owner(at)?;
    if owner.ids(host)? != Some(have) {
        fix.push(format!("owner {} -> {owner}", names(host, have)?));
    }
    if mode_of(m) != mode {
        fix.push(format!("mode {:04o} -> {mode:04o}", mode_of(m)));
    }
    Ok(fix)
}

/// The first line where two texts differ, in words.
fn first_difference(have: &str, want: &str) -> String {
    let (mut h, mut w) = (have.lines(), want.lines());
    for n in 1.. {
        match (h.next(), w.next()) {
            (Some(a), Some(b)) if a == b => continue,
            (Some(a), Some(b)) => return format!("content: line {n} is `{a}`, want `{b}`"),
            (Some(a), None) => return format!("content: line {n} is `{a}`, want the end"),
            (None, Some(b)) => return format!("content: line {n} is missing, want `{b}`"),
            (None, None) => break,
        }
    }
    "content: differs in its line endings".into()
}

/// Whether two files hold the same bytes, read in chunks.
pub(crate) fn same_bytes(a: &Path, b: &Path) -> Result<bool> {
    use std::io::Read;
    let (ma, mb) = (std::fs::metadata(a)?, std::fs::metadata(b)?);
    if ma.len() != mb.len() {
        return Ok(false);
    }
    let mut fa = std::fs::File::open(a).with_context(|| format!("reading {}", a.display()))?;
    let mut fb = std::fs::File::open(b).with_context(|| format!("reading {}", b.display()))?;
    let (mut ba, mut bb) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
    loop {
        let n = fa.read(&mut ba)?;
        if n == 0 {
            return Ok(true);
        }
        fb.read_exact(&mut bb[..n])?;
        if ba[..n] != bb[..n] {
            return Ok(false);
        }
    }
}

impl Body {
    /// What differs between the file at `at` and this body, if anything.
    fn differs(&self, at: &Path) -> Result<Option<String>> {
        Ok(match self {
            Body::Placeholder => None,
            Body::Text(want) => {
                let have =
                    std::fs::read(at).with_context(|| format!("reading {}", at.display()))?;
                (have != want.as_bytes())
                    .then(|| first_difference(&String::from_utf8_lossy(&have), want))
            }
            Body::Copy { from, header: None } => (!same_bytes(at, from)?)
                .then(|| format!("content: differs from {}", from.display())),
            Body::Copy {
                from,
                header: Some(h),
            } => {
                let want = [h.as_bytes(), &std::fs::read(from)?].concat();
                let have = std::fs::read(at)?;
                (have != want).then(|| format!("content: differs from {}", from.display()))
            }
        })
    }

    fn fill(&self, f: &mut std::fs::File) -> Result<()> {
        match self {
            Body::Placeholder => {}
            Body::Text(t) => f.write_all(t.as_bytes())?,
            Body::Copy { from, header } => {
                if let Some(h) = header {
                    f.write_all(h.as_bytes())?;
                }
                let mut src = std::fs::File::open(from)
                    .with_context(|| format!("reading {}", from.display()))?;
                std::io::copy(&mut src, f)?;
            }
        }
        Ok(())
    }
}

impl Item {
    /// Note in `going` what `act` takes away.
    pub fn goes(&self, act: &Act, going: &mut Going) {
        match (self, act) {
            (
                Item::NoFile { path, .. } | Item::NoDir { path, .. },
                Act::Remove | Act::MoveAside(_),
            ) => {
                going.paths.insert(path.clone());
            }
            (Item::NoUser { name, .. }, Act::Remove) => {
                going.users.insert(name.clone());
            }
            _ => {}
        }
    }

    /// What it would do, the machine as it is, less what `going` says the
    /// items before it take away.
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    pub fn inspect(&self, root: &Path, host: &dyn Host, going: &Going) -> Result<Act> {
        match self {
            Item::Group(name) => Ok(match host.group(name)? {
                Some(_) => Act::InPlace,
                None => Act::Create,
            }),
            Item::User { name, home, .. } => Ok(match host.user(name)? {
                None => Act::Create,
                Some(u) if &u.home == home => Act::InPlace,
                Some(u) => Act::Refuse(format!(
                    "a user {name} exists with home {}: not one this installer made; rename or \
                     remove it first",
                    u.home.display()
                )),
            }),
            Item::Member { group, user } => Ok(match host.group(group)? {
                Some(g) if g.members.iter().any(|m| m == user) => Act::InPlace,
                _ => Act::Create,
            }),
            Item::Dir { path, owner, mode } => {
                let at = real(root, path);
                let Some(m) = present(&at)? else {
                    return Ok(Act::Create);
                };
                if m.file_type().is_symlink() || !m.is_dir() {
                    return Ok(Act::Refuse(format!(
                        "{} is not a directory (a symlink, or a file): the layout wants one there",
                        path.display()
                    )));
                }
                let fix = owner_and_mode(host, &at, &m, owner, *mode)?;
                Ok(if fix.is_empty() {
                    Act::InPlace
                } else {
                    Act::Fix(fix)
                })
            }
            Item::File {
                path,
                owner,
                mode,
                body,
            } => {
                let at = real(root, path);
                let Some(m) = present(&at)? else {
                    return Ok(Act::Create);
                };
                if m.file_type().is_symlink() || !m.is_file() {
                    return Ok(Act::Refuse(format!(
                        "{} is not a regular file (a symlink, or a directory): the layout wants one \
                         there",
                        path.display()
                    )));
                }
                let mut fix = owner_and_mode(host, &at, &m, owner, *mode)?;
                fix.extend(body.differs(&at)?);
                Ok(if fix.is_empty() {
                    Act::InPlace
                } else {
                    Act::Fix(fix)
                })
            }
            Item::State(mig) => mig.inspect(root),
            Item::Token { path, uid, name } => {
                super::token::inspect(root, host, path, (*uid, name))
            }
            Item::Stopped(sock) => {
                let at = real(root, sock);
                Ok(match present(&at)? {
                    Some(m) if m.file_type().is_socket() && answers(&at) => Act::Refuse(format!(
                        "a daemon answers on {}: stop it first (`sudo systemctl stop theseusd`, or \
                         `theseus shutdown`)",
                        sock.display()
                    )),
                    _ => Act::InPlace,
                })
            }
            Item::NoFile { path, rule } => {
                let at = real(root, path);
                let Some(m) = present(&at)? else {
                    return Ok(Act::Absent);
                };
                if m.file_type().is_symlink() || !m.is_file() {
                    return Ok(Act::Keep {
                        why: "not a regular file, so not one the installer wrote".into(),
                        fix: vec![],
                    });
                }
                Ok(match rule {
                    FileRule::Binary => Act::Remove,
                    FileRule::Generated => {
                        let head = std::fs::read(&at)?;
                        if head.starts_with(HEADER.as_bytes()) {
                            Act::Remove
                        } else {
                            Act::Keep {
                                why: "it does not start with the installer's header, so the \
                                      installer did not write it"
                                    .into(),
                                fix: vec![],
                            }
                        }
                    }
                    FileRule::Token if m.len() == 0 => Act::Remove,
                    FileRule::Token => Act::Keep {
                        why: "it holds a token: delete it yourself once no daemon needs it".into(),
                        fix: owner_and_mode(host, &at, &m, &Owner::root(), 0o600)?,
                    },
                })
            }
            Item::NoDir { path, rule, seal } => {
                let at = real(root, path);
                let Some(m) = present(&at)? else {
                    return Ok(Act::Absent);
                };
                if m.file_type().is_symlink() || !m.is_dir() {
                    return Ok(Act::Keep {
                        why: "not a directory, so not one the installer made".into(),
                        fix: vec![],
                    });
                }
                let mut held: Vec<(String, bool)> = vec![];
                for e in
                    std::fs::read_dir(&at).with_context(|| format!("reading {}", at.display()))?
                {
                    let e = e?;
                    if !going.paths.contains(&path.join(e.file_name())) {
                        held.push((
                            e.file_name().to_string_lossy().into_owned(),
                            e.file_type()?.is_socket(),
                        ));
                    }
                }
                held.sort();
                let sealed = |why: String| -> Result<Act> {
                    let fix = if *seal {
                        owner_and_mode(host, &at, &m, &Owner::root(), 0o700)?
                    } else {
                        vec![]
                    };
                    Ok(Act::Keep { why, fix })
                };
                let holds = || {
                    let mut n: Vec<&str> = held.iter().take(4).map(|(n, _)| n.as_str()).collect();
                    if held.len() > 4 {
                        n.push("…");
                    }
                    format!("it holds {}", n.join(", "))
                };
                match rule {
                    _ if held.is_empty() => Ok(Act::Remove),
                    DirRule::Sockets if held.iter().all(|(_, sock)| *sock) => Ok(Act::Remove),
                    DirRule::IfEmpty | DirRule::Sockets => sealed(holds()),
                    DirRule::State { purge: true, stamp } => {
                        let name = path
                            .file_name()
                            .map_or_else(|| "state".into(), |n| n.to_string_lossy().into_owned());
                        Ok(Act::MoveAside(
                            path.with_file_name(format!("{name}.removed-{stamp}")),
                        ))
                    }
                    DirRule::State { purge: false, .. } => sealed(format!(
                        "{}: the installer never deletes a store (--purge-state moves it aside)",
                        holds()
                    )),
                }
            }
            Item::NoMember { group, user } => Ok(match host.group(group)? {
                Some(g) if g.members.iter().any(|m| m == user) => Act::Remove,
                _ => Act::Absent,
            }),
            Item::NoUser { name, home } => Ok(match host.user(name)? {
                None => Act::Absent,
                Some(u) if &u.home == home && is_nologin(&u.shell) => Act::Remove,
                Some(u) => Act::Keep {
                    why: format!(
                        "home {}, shell {}: not the system user this installer makes",
                        u.home.display(),
                        u.shell.display()
                    ),
                    fix: vec![],
                },
            }),
            Item::NoGroup(name) => Ok(match host.group(name)? {
                None => Act::Absent,
                Some(g) => match host.user(name)? {
                    Some(u) if u.gid == g.gid && !going.users.contains(name) => Act::Keep {
                        why: format!("it is user {name}'s own group, which stays"),
                        fix: vec![],
                    },
                    _ => Act::Remove,
                },
            }),
        }
    }

    /// Do `act`, which `inspect` said a moment ago, and say what was done.
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    pub fn apply(
        &self,
        act: &Act,
        root: &Path,
        host: &mut dyn Host,
        parents: &Owner,
        log: &mut dyn FnMut(String),
    ) -> Result<()> {
        match (self, act) {
            (Item::Group(name), Act::Create) => {
                host.add_group(name)?;
                log(format!("created group {name}"));
            }
            (Item::User { name, home, shell }, Act::Create) => {
                host.add_user(name, home, shell)?;
                log(format!(
                    "created user {name} (system, its own group, home {}, shell {})",
                    home.display(),
                    shell.display()
                ));
            }
            (Item::Member { group, user }, Act::Create) => {
                host.add_member(group, user)?;
                log(format!("added {user} to group {group}"));
            }
            (Item::Dir { path, owner, mode }, Act::Create) => {
                let at = real(root, path);
                make_parents(root, path, parents, host, log)?;
                std::fs::DirBuilder::new()
                    .mode(*mode)
                    .create(&at)
                    .with_context(|| format!("creating {}", at.display()))?;
                set(host, &at, owner, *mode)?;
                log(format!(
                    "created dir {} ({owner} {mode:04o})",
                    path.display()
                ));
            }
            (Item::Dir { path, owner, mode }, Act::Fix(fix)) => {
                set(host, &real(root, path), owner, *mode)?;
                log(format!("fixed {}: {}", path.display(), fix.join("; ")));
            }
            (
                Item::File {
                    path,
                    owner,
                    mode,
                    body,
                },
                Act::Create | Act::Fix(_),
            ) => {
                let at = real(root, path);
                let fresh = present(&at)?.is_none();
                // A placeholder is never written once it is there.
                let rewrite = fresh || body.differs(&at)?.is_some();
                if fresh {
                    make_parents(root, path, parents, host, log)?;
                }
                if rewrite {
                    match body {
                        Body::Placeholder => {
                            std::fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .mode(*mode)
                                .open(&at)
                                .with_context(|| format!("creating {}", at.display()))?;
                        }
                        _ => write_whole(&at, body, *mode)?,
                    }
                }
                set(host, &at, owner, *mode)?;
                match (act, body, fresh) {
                    (Act::Fix(fix), _, _) if !rewrite => {
                        log(format!("fixed {}: {}", path.display(), fix.join("; ")));
                    }
                    (_, Body::Placeholder, _) => {
                        log(format!(
                            "created {} empty ({owner} {mode:04o})",
                            path.display()
                        ));
                    }
                    (_, Body::Copy { from, .. }, f) => log(format!(
                        "{} {} from {} ({owner} {mode:04o})",
                        if f { "installed" } else { "replaced" },
                        path.display(),
                        from.display()
                    )),
                    (_, Body::Text(_), f) => log(format!(
                        "{} {} ({owner} {mode:04o})",
                        if f { "wrote" } else { "rewrote" },
                        path.display()
                    )),
                }
            }
            (Item::State(mig), Act::Create) => mig.apply(root, host, log)?,
            (Item::NoFile { path, .. }, Act::Remove) => {
                std::fs::remove_file(real(root, path))
                    .with_context(|| format!("removing {}", path.display()))?;
                log(format!("removed {}", path.display()));
            }
            (Item::NoFile { path, .. }, Act::Keep { why, fix }) => {
                set(host, &real(root, path), &Owner::root(), 0o600)?;
                log(format!(
                    "kept {} ({why}); {}",
                    path.display(),
                    fix.join("; ")
                ));
            }
            (Item::NoDir { path, rule, .. }, Act::Remove) => {
                let at = real(root, path);
                if *rule == DirRule::Sockets {
                    for e in std::fs::read_dir(&at)? {
                        let e = e?;
                        if e.file_type()?.is_socket() {
                            std::fs::remove_file(e.path())?;
                            log(format!("removed {}", path.join(e.file_name()).display()));
                        }
                    }
                }
                std::fs::remove_dir(&at).with_context(|| format!("removing {}", path.display()))?;
                log(format!("removed dir {}", path.display()));
            }
            (Item::NoDir { path, .. }, Act::MoveAside(to)) => {
                let (from, dest) = (real(root, path), real(root, to));
                if present(&dest)?.is_some() {
                    bail!(
                        "{} is already there: not moving {} onto it",
                        to.display(),
                        path.display()
                    );
                }
                host.rename(&from, &dest)?;
                set(host, &dest, &Owner::root(), 0o700)?;
                log(format!(
                    "moved {} aside to {} (root:root 0700); nothing in it was deleted",
                    path.display(),
                    to.display()
                ));
            }
            (Item::NoDir { path, .. }, Act::Keep { why, fix }) => {
                set(host, &real(root, path), &Owner::root(), 0o700)?;
                log(format!(
                    "kept {} ({why}); {}",
                    path.display(),
                    fix.join("; ")
                ));
            }
            (Item::NoMember { group, user }, Act::Remove) => {
                host.remove_member(group, user)?;
                log(format!("removed {user} from group {group}"));
            }
            (Item::NoUser { name, .. }, Act::Remove) => {
                host.remove_user(name)?;
                log(format!("removed user {name}"));
            }
            (Item::NoGroup(name), Act::Remove) => {
                host.remove_group(name)?;
                log(format!("removed group {name}"));
            }
            (item, act) => bail!("internal: no way to do {act:?} for {item:?}"),
        }
        Ok(())
    }

    /// The item, in a plan's words: a kind, then what.
    pub fn describe(&self) -> (&'static str, String) {
        let at = |p: &Path, o: &Owner, mode: u32| format!("{} {o} {mode:04o}", p.display());
        match self {
            Item::Group(n) | Item::NoGroup(n) => ("group", n.clone()),
            Item::User { name, home, shell } => (
                "user",
                format!(
                    "{name} (system; its own group; home {}; shell {})",
                    home.display(),
                    shell.display()
                ),
            ),
            Item::NoUser { name, .. } => ("user", name.clone()),
            Item::Member { group, user } | Item::NoMember { group, user } => {
                ("member", format!("{user} of {group}"))
            }
            Item::Dir { path, owner, mode } => ("dir", at(path, owner, *mode)),
            Item::File {
                path,
                owner,
                mode,
                body,
            } => (
                "file",
                match body {
                    Body::Placeholder => format!("{} (empty)", at(path, owner, *mode)),
                    Body::Copy { from, .. } => {
                        format!("{} (from {})", at(path, owner, *mode), from.display())
                    }
                    Body::Text(_) => at(path, owner, *mode),
                },
            ),
            Item::State(mig) => ("state", mig.describe()),
            Item::Token { path, .. } => (
                "token",
                format!(
                    "{} (yours alone: mode 0600 or stricter, not empty)",
                    path.display()
                ),
            ),
            Item::Stopped(sock) => (
                "socket",
                format!("{} (no daemon may answer)", sock.display()),
            ),
            Item::NoFile { path, .. } => ("file", path.display().to_string()),
            Item::NoDir { path, .. } => ("dir", path.display().to_string()),
        }
    }

    /// The verb for `act` on this item.
    pub fn verb(&self, act: &Act) -> &'static str {
        match act {
            Act::InPlace => "ok",
            Act::Create => match self {
                Item::Member { .. } => "add",
                Item::File {
                    body: Body::Text(_),
                    ..
                } => "write",
                Item::File {
                    body: Body::Copy { .. },
                    ..
                } => "install",
                Item::State(_) => "copy",
                _ => "create",
            },
            Act::Fix(_) => "fix",
            Act::Remove => "remove",
            Act::MoveAside(_) => "move",
            Act::Keep { .. } => "keep",
            Act::Absent => "gone",
            Act::Refuse(_) => "REFUSE",
            Act::Unseen(_) => "unseen",
        }
    }
}

/// Its metadata, or `None` when nothing is there. Never through a symlink.
pub(crate) fn present(at: &Path) -> Result<Option<std::fs::Metadata>> {
    match std::fs::symlink_metadata(at) {
        Ok(m) => Ok(Some(m)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow::Error::new(e).context(format!("reading {}", at.display()))),
    }
}

/// The owner, then the mode: a chown can clear mode bits.
pub(crate) fn set(host: &mut dyn Host, at: &Path, owner: &Owner, mode: u32) -> Result<()> {
    let (uid, gid) = owner.resolve(host)?;
    host.chown(at, uid, gid)?;
    std::fs::set_permissions(at, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("chmod {mode:04o} {}", at.display()))
}

/// The directories above `path` that are missing, made `0755` and owned by
/// `owner`: system directories a temp root lacks, or `~/.config/systemd/user`.
fn make_parents(
    root: &Path,
    path: &Path,
    owner: &Owner,
    host: &mut dyn Host,
    log: &mut dyn FnMut(String),
) -> Result<()> {
    let mut missing = vec![];
    let mut p = path.parent();
    while let Some(dir) = p {
        if dir.parent().is_none() || present(&real(root, dir))?.is_some() {
            break;
        }
        missing.push(dir);
        p = dir.parent();
    }
    for dir in missing.into_iter().rev() {
        let at = real(root, dir);
        std::fs::DirBuilder::new()
            .mode(0o755)
            .create(&at)
            .with_context(|| format!("creating {}", at.display()))?;
        set(host, &at, owner, 0o755)?;
        log(format!(
            "created dir {} ({owner} 0755), a parent",
            dir.display()
        ));
    }
    Ok(())
}

/// Replace a file whole: a temporary file beside it, filled, synced, and
/// renamed over it, so a reader (or a running binary) sees the old or the new.
fn write_whole(at: &Path, body: &Body, mode: u32) -> Result<()> {
    let dir = at.parent().context("a file with no directory")?;
    let name = at
        .file_name()
        .context("a file with no name")?
        .to_string_lossy();
    let tmp = dir.join(format!(".{name}.theseus-install"));
    if present(&tmp)?.is_some() {
        // An interrupted run's: the installer's own.
        std::fs::remove_file(&tmp)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&tmp)
        .with_context(|| format!("creating {}", tmp.display()))?;
    body.fill(&mut f)?;
    f.sync_all()?;
    drop(f);
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(&tmp, at).with_context(|| format!("renaming onto {}", at.display()))
}

/// A word or path on a unit's command line: `%` and `$` escaped, and quoted
/// when it holds anything but the plain characters.
pub(crate) fn unit_arg(s: &str) -> Result<String> {
    if s.chars().any(char::is_control) {
        bail!("{s:?}: a unit's command line cannot hold a control character");
    }
    let e = s.replace('%', "%%").replace('$', "$$");
    if !e.is_empty()
        && e.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-:@+=,%$".contains(c))
    {
        Ok(e)
    } else {
        Ok(format!(
            "\"{}\"",
            e.replace('\\', "\\\\").replace('"', "\\\"")
        ))
    }
}

/// An `Environment=` line: `%` escaped, the whole assignment quoted.
pub(crate) fn unit_env(key: &str, value: &str) -> Result<String> {
    if value.chars().any(char::is_control) {
        bail!("{key}: a unit's environment cannot hold a control character");
    }
    let kv = format!("{key}={value}")
        .replace('%', "%%")
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    Ok(format!("Environment=\"{kv}\""))
}

/// The lines every unit the installer writes shares: how it stops, and why,
/// and the cgroup that is the daemon's own (theseus-a5nv): it stays in it,
/// so a restart while a job runs starts as any start does, and no stop hook
/// is needed.
const SERVICE_COMMON: &str = "\
# SIGINT is the daemon's clean stop: it removes its socket and closes the
# store. A stop signals the daemon alone, so running jobs finish, and the
# next daemon reads their results from the spool, as a shell-started
# daemon's stop leaves them.
KillSignal=SIGINT
KillMode=process
Restart=on-failure
RestartSec=1
# The unit's cgroup is the daemon's own: each L0 job gets a cgroup of its
# own inside it, with a process cap and an exact stop.
Delegate=yes
";

/// The start limit both daemon units share (theseus-0v8s): a start takes about 20 ms, so a
/// panic at every start would otherwise restart the daemon every few seconds for as long as it
/// lasts, one more file in `crashes/` each time, and systemd's default (5 starts in 10 s)
/// never trips at that pace. Ten starts in 300 s stops the loop and keeps its files.
const START_LIMIT: &str = "\
StartLimitIntervalSec=300
StartLimitBurst=10
";

/// `--user`'s unit, `name` (`theseusd`, or a second daemon's `--unit`).
pub(crate) fn user_unit(name: &str, exec: &[String], env: &[(String, String)]) -> Result<String> {
    // The operator's own unit says only "Theseus daemon", as it always has, so
    // an installed unit still matches its plan; a second daemon's names itself.
    let description = if name == super::USER_UNIT {
        "Theseus daemon".to_string()
    } else {
        format!("Theseus daemon ({name})")
    };
    let mut s = format!(
        "{HEADER} --user`. A later --apply rewrites it:\n\
         # put your own changes in a drop-in (`systemctl --user edit {name}`).\n\
         [Unit]\n\
         Description={description}\n\
         {START_LIMIT}\n\
         [Service]\n\
         Type=exec\n\
         ExecStart={}\n",
        exec.iter()
            .map(|a| unit_arg(a))
            .collect::<Result<Vec<_>>>()?
            .join(" ")
    );
    for (k, v) in env {
        s.push_str(&unit_env(k, v)?);
        s.push('\n');
    }
    s.push_str(SERVICE_COMMON);
    s.push_str("\n[Install]\nWantedBy=default.target\n");
    Ok(s)
}

/// `--separate`'s system unit: the daemon as `theseus`.
pub(crate) fn system_unit(exec: &[String]) -> Result<String> {
    Ok(format!(
        "{HEADER} --separate`. A later --apply rewrites it:\n\
         # put your own changes in a drop-in (`sudo systemctl edit theseusd`).\n\
         [Unit]\n\
         Description=Theseus daemon, as its own user\n\
         Wants=network-online.target\n\
         After=network-online.target\n\
         {START_LIMIT}\n\
         [Service]\n\
         Type=exec\n\
         User={DAEMON_USER}\n\
         Group={DAEMON_USER}\n\
         # The group whose members reach the daemon's sockets.\n\
         SupplementaryGroups={OPS_GROUP}\n\
         ExecStart={}\n\
         UMask=0077\n\
         {SERVICE_COMMON}\n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        exec.iter()
            .map(|a| unit_arg(a))
            .collect::<Result<Vec<_>>>()?
            .join(" ")
    ))
}

/// The job host's user unit: disabled until step 22b gives `theseusd` its
/// `job-host` role, and its own stop.
pub(crate) fn job_host_unit() -> String {
    format!(
        "{HEADER} --separate`. Leave it disabled until step 22b\n\
         # gives theseusd its job-host role: until then its start fails.\n\
         [Unit]\n\
         Description=Theseus job host: the separated daemon's jobs, run as you\n\
         \n\
         [Service]\n\
         Type=exec\n\
         ExecStart={BINARY} job-host\n\
         # A stop signals the host alone: its jobs finish, and their results\n\
         # wait in the spool for the next host to relay.\n\
         KillMode=process\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

/// `/run` is a tmpfs: this makes the sockets' directory at each boot.
pub(crate) fn tmpfiles() -> String {
    format!(
        "{HEADER} --separate`.\n\
         # The daemon's sockets: theseus.sock, and the job host's jobs.sock.\n\
         d {RUN_DIR} 0750 {DAEMON_USER} {OPS_GROUP} -\n"
    )
}
