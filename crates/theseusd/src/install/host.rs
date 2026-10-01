//! What the installer asks of the machine beyond files: its accounts, and who
//! owns a path (theseus-7hh). The real host reads `/etc/passwd` and
//! `/etc/group`, and changes accounts with the system's own tools, as root.
//! Tests use `Fake`, which keeps accounts and owners in memory, so
//! `--separate` runs end to end, in a temp root, without root.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// A user, as `/etc/passwd` holds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct User {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub home: PathBuf,
    pub shell: PathBuf,
}

/// A group, as `/etc/group` holds one: its supplementary members only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Group {
    pub name: String,
    pub gid: u32,
    pub members: Vec<String>,
}

/// Accounts, and ownership. Paths are real paths: the root already joined.
pub(crate) trait Host {
    fn user(&self, name: &str) -> Result<Option<User>>;
    fn user_by_uid(&self, uid: u32) -> Result<Option<User>>;
    fn group(&self, name: &str) -> Result<Option<Group>>;
    fn group_by_gid(&self, gid: u32) -> Result<Option<Group>>;
    /// A system group.
    fn add_group(&mut self, name: &str) -> Result<()>;
    /// A system user with a group of its own name, no login shell, and a home
    /// it does not create.
    fn add_user(&mut self, name: &str, home: &Path, shell: &Path) -> Result<()>;
    fn add_member(&mut self, group: &str, user: &str) -> Result<()>;
    fn remove_member(&mut self, group: &str, user: &str) -> Result<()>;
    fn remove_user(&mut self, name: &str) -> Result<()>;
    fn remove_group(&mut self, name: &str) -> Result<()>;
    /// Never through a symlink.
    fn chown(&mut self, path: &Path, uid: u32, gid: u32) -> Result<()>;
    fn owner(&self, path: &Path) -> Result<(u32, u32)>;
    /// A rename, whose owners go with it.
    fn rename(&mut self, from: &Path, to: &Path) -> Result<()> {
        std::fs::rename(from, to)
            .with_context(|| format!("moving {} to {}", from.display(), to.display()))
    }
}

/// The effective uid, from `/proc/self/status`. Root is who this process
/// acts as, not who logged in: `$USER` stays the operator's under `sudo -E`.
pub(crate) fn euid() -> Result<u32> {
    uid_field(&read_status()?, 1)
}

/// The real uid: who ran this, before any `sudo`.
pub(crate) fn ruid() -> Result<u32> {
    uid_field(&read_status()?, 0)
}

fn read_status() -> Result<String> {
    std::fs::read_to_string("/proc/self/status").context("reading /proc/self/status")
}

/// The `Uid:` line's fields are the real, effective, saved, and filesystem uids.
pub(crate) fn uid_field(status: &str, at: usize) -> Result<u32> {
    let line = status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .context("/proc/self/status has no Uid: line")?;
    line.split_whitespace()
        .nth(at)
        .and_then(|f| f.parse().ok())
        .with_context(|| format!("/proc/self/status: a malformed Uid: line ({})", line.trim()))
}

/// The machine itself: `/` only.
pub(crate) struct System;

impl System {
    fn passwd() -> Result<Vec<User>> {
        Ok(parse_passwd(
            &std::fs::read_to_string("/etc/passwd").context("reading /etc/passwd")?,
        ))
    }

    fn groups() -> Result<Vec<Group>> {
        Ok(parse_group(
            &std::fs::read_to_string("/etc/group").context("reading /etc/group")?,
        ))
    }

    /// Run one of the system's account tools; its error output is the error.
    fn run(tool: &str, args: &[&str]) -> Result<()> {
        let bin = ["/usr/sbin", "/usr/bin", "/sbin", "/bin"]
            .iter()
            .map(|d| Path::new(d).join(tool))
            .find(|p| p.is_file())
            .with_context(|| {
                format!("`{tool}` is not installed (looked in /usr/sbin, /usr/bin, /sbin, /bin)")
            })?;
        let out = std::process::Command::new(&bin)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .with_context(|| format!("running {}", bin.display()))?;
        if !out.status.success() {
            bail!(
                "{} {} failed ({}): {}",
                bin.display(),
                args.join(" "),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }
}

impl Host for System {
    fn user(&self, name: &str) -> Result<Option<User>> {
        Ok(Self::passwd()?.into_iter().find(|u| u.name == name))
    }
    fn user_by_uid(&self, uid: u32) -> Result<Option<User>> {
        Ok(Self::passwd()?.into_iter().find(|u| u.uid == uid))
    }
    fn group(&self, name: &str) -> Result<Option<Group>> {
        Ok(Self::groups()?.into_iter().find(|g| g.name == name))
    }
    fn group_by_gid(&self, gid: u32) -> Result<Option<Group>> {
        Ok(Self::groups()?.into_iter().find(|g| g.gid == gid))
    }
    fn add_group(&mut self, name: &str) -> Result<()> {
        Self::run("groupadd", &["--system", name])
    }
    fn add_user(&mut self, name: &str, home: &Path, shell: &Path) -> Result<()> {
        let (home, shell) = (home.to_string_lossy(), shell.to_string_lossy());
        Self::run(
            "useradd",
            &[
                "--system",
                "--user-group",
                "--no-create-home",
                "--home-dir",
                &home,
                "--shell",
                &shell,
                "--comment",
                "Theseus daemon",
                name,
            ],
        )
    }
    fn add_member(&mut self, group: &str, user: &str) -> Result<()> {
        Self::run("gpasswd", &["--add", user, group])
    }
    fn remove_member(&mut self, group: &str, user: &str) -> Result<()> {
        Self::run("gpasswd", &["--delete", user, group])
    }
    fn remove_user(&mut self, name: &str) -> Result<()> {
        Self::run("userdel", &[name])
    }
    fn remove_group(&mut self, name: &str) -> Result<()> {
        Self::run("groupdel", &[name])
    }
    fn chown(&mut self, path: &Path, uid: u32, gid: u32) -> Result<()> {
        std::os::unix::fs::lchown(path, Some(uid), Some(gid))
            .with_context(|| format!("chown {uid}:{gid} {}", path.display()))
    }
    fn owner(&self, path: &Path) -> Result<(u32, u32)> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(path)
            .with_context(|| format!("reading {}", path.display()))?;
        Ok((m.uid(), m.gid()))
    }
}

pub(crate) fn parse_passwd(text: &str) -> Vec<User> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            (f.len() >= 7).then_some(())?;
            Some(User {
                name: f[0].to_string(),
                uid: f[2].parse().ok()?,
                gid: f[3].parse().ok()?,
                home: PathBuf::from(f[5]),
                shell: PathBuf::from(f[6]),
            })
        })
        .collect()
}

pub(crate) fn parse_group(text: &str) -> Vec<Group> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            (f.len() >= 4).then_some(())?;
            Some(Group {
                name: f[0].to_string(),
                gid: f[2].parse().ok()?,
                members: f[3]
                    .split(',')
                    .filter(|m| !m.is_empty())
                    .map(str::to_string)
                    .collect(),
            })
        })
        .collect()
}

/// Accounts and owners in memory, for tests: system ids are taken from 999
/// down, as `useradd --system` takes them. A path never chowned is owned as
/// the filesystem says.
#[cfg(test)]
pub(crate) struct Fake {
    pub users: Vec<User>,
    pub groups: Vec<Group>,
    pub owners: std::collections::HashMap<PathBuf, (u32, u32)>,
    /// Every account change, in order.
    pub calls: Vec<String>,
}

#[cfg(test)]
impl Fake {
    /// `root`, and the operator with a group of their own name.
    pub fn new(operator: &str, uid: u32) -> Self {
        let user = |name: &str, uid: u32, home: &str, shell: &str| User {
            name: name.into(),
            uid,
            gid: uid,
            home: home.into(),
            shell: shell.into(),
        };
        let group = |name: &str, gid: u32| Group {
            name: name.into(),
            gid,
            members: vec![],
        };
        Self {
            users: vec![
                user("root", 0, "/root", "/bin/bash"),
                user(operator, uid, &format!("/home/{operator}"), "/bin/bash"),
            ],
            groups: vec![group("root", 0), group(operator, uid)],
            owners: Default::default(),
            calls: vec![],
        }
    }

    fn free_id(&self) -> u32 {
        (100..1000)
            .rev()
            .find(|id| {
                !self.users.iter().any(|u| u.uid == *id)
                    && !self.groups.iter().any(|g| g.gid == *id)
            })
            .expect("a free system id")
    }
}

#[cfg(test)]
impl Host for Fake {
    fn user(&self, name: &str) -> Result<Option<User>> {
        Ok(self.users.iter().find(|u| u.name == name).cloned())
    }
    fn user_by_uid(&self, uid: u32) -> Result<Option<User>> {
        Ok(self.users.iter().find(|u| u.uid == uid).cloned())
    }
    fn group(&self, name: &str) -> Result<Option<Group>> {
        Ok(self.groups.iter().find(|g| g.name == name).cloned())
    }
    fn group_by_gid(&self, gid: u32) -> Result<Option<Group>> {
        Ok(self.groups.iter().find(|g| g.gid == gid).cloned())
    }
    fn add_group(&mut self, name: &str) -> Result<()> {
        if self.groups.iter().any(|g| g.name == name) {
            bail!("groupadd: group '{name}' already exists");
        }
        let gid = self.free_id();
        self.groups.push(Group {
            name: name.into(),
            gid,
            members: vec![],
        });
        self.calls.push(format!("groupadd --system {name}"));
        Ok(())
    }
    fn add_user(&mut self, name: &str, home: &Path, shell: &Path) -> Result<()> {
        if self.users.iter().any(|u| u.name == name) {
            bail!("useradd: user '{name}' already exists");
        }
        let id = self.free_id();
        self.groups.push(Group {
            name: name.into(),
            gid: id,
            members: vec![],
        });
        self.users.push(User {
            name: name.into(),
            uid: id,
            gid: id,
            home: home.into(),
            shell: shell.into(),
        });
        self.calls.push(format!(
            "useradd --system --user-group {name} (home {}, shell {})",
            home.display(),
            shell.display()
        ));
        Ok(())
    }
    fn add_member(&mut self, group: &str, user: &str) -> Result<()> {
        let g = self
            .groups
            .iter_mut()
            .find(|g| g.name == group)
            .with_context(|| format!("gpasswd: group '{group}' does not exist"))?;
        if !g.members.iter().any(|m| m == user) {
            g.members.push(user.into());
        }
        self.calls.push(format!("gpasswd --add {user} {group}"));
        Ok(())
    }
    fn remove_member(&mut self, group: &str, user: &str) -> Result<()> {
        let g = self
            .groups
            .iter_mut()
            .find(|g| g.name == group)
            .with_context(|| format!("gpasswd: group '{group}' does not exist"))?;
        g.members.retain(|m| m != user);
        self.calls.push(format!("gpasswd --delete {user} {group}"));
        Ok(())
    }
    fn remove_user(&mut self, name: &str) -> Result<()> {
        let at = self
            .users
            .iter()
            .position(|u| u.name == name)
            .with_context(|| format!("userdel: user '{name}' does not exist"))?;
        let u = self.users.remove(at);
        // `userdel` removes the user's own group when no one else is in it
        // (USERGROUPS_ENAB, Debian's and Ubuntu's default).
        self.groups
            .retain(|g| !(g.name == name && g.gid == u.gid && g.members.is_empty()));
        for g in &mut self.groups {
            g.members.retain(|m| m != name);
        }
        self.calls.push(format!("userdel {name}"));
        Ok(())
    }
    fn remove_group(&mut self, name: &str) -> Result<()> {
        let at = self
            .groups
            .iter()
            .position(|g| g.name == name)
            .with_context(|| format!("groupdel: group '{name}' does not exist"))?;
        self.groups.remove(at);
        self.calls.push(format!("groupdel {name}"));
        Ok(())
    }
    fn chown(&mut self, path: &Path, uid: u32, gid: u32) -> Result<()> {
        std::fs::symlink_metadata(path).with_context(|| format!("chown {}", path.display()))?;
        self.owners.insert(path.to_path_buf(), (uid, gid));
        Ok(())
    }
    fn owner(&self, path: &Path) -> Result<(u32, u32)> {
        if let Some(o) = self.owners.get(path) {
            return Ok(*o);
        }
        System.owner(path)
    }
    fn rename(&mut self, from: &Path, to: &Path) -> Result<()> {
        std::fs::rename(from, to)?;
        let moved: Vec<PathBuf> = self
            .owners
            .keys()
            .filter(|k| k.starts_with(from))
            .cloned()
            .collect();
        for k in moved {
            let o = self.owners.remove(&k).expect("a key just listed");
            let rel = k.strip_prefix(from).expect("under from");
            let at = if rel.as_os_str().is_empty() {
                to.to_path_buf()
            } else {
                to.join(rel)
            };
            self.owners.insert(at, o);
        }
        Ok(())
    }
}
