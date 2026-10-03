//! The two modes' layouts (M4 §2.9, theseus-7hh): what `--user` and
//! `--separate` make, and what each one's `--remove` takes away.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::host::{Host, User};
use super::layout::*;
use super::migrate::{Migrate, NAMES};
use super::{Env, Globals, InstallArgs};

/// What a `--user` unit carries of this shell's environment: `PATH`, so the
/// daemon finds `op` and a job finds its tools (`[tools] proc_env` passes the
/// daemon's own), and whichever of the rest of `proc_env` this shell sets.
/// `HOME` and `USER` come from the user manager, and `TERM` is a terminal's.
const USER_ENV: [&str; 6] = ["PATH", "LANG", "LC_ALL", "TZ", "CARGO_HOME", "RUSTUP_HOME"];

/// systemd's own `PATH` for a system service, where the separated daemon
/// looks for `op`.
const SYSTEM_PATH: [&str; 6] = [
    "/usr/local/sbin",
    "/usr/local/bin",
    "/usr/sbin",
    "/usr/bin",
    "/sbin",
    "/bin",
];

/// The config's source as a unit names it: the vault's note as it is, a
/// file by an absolute path.
enum Source {
    Vault(String),
    File(PathBuf),
}

impl Source {
    fn of(config: &str, env: &Env) -> Self {
        if config.starts_with("op://") {
            Source::Vault(config.into())
        } else {
            Source::File(env.absolute(Path::new(config)))
        }
    }

    fn arg(&self) -> String {
        match self {
            Source::Vault(r) => r.clone(),
            Source::File(p) => p.display().to_string(),
        }
    }
}

/// The operator: `--operator`, else whoever ran `sudo`, else whoever runs
/// this. Never root.
fn operator(env: &Env, host: &dyn Host, named: Option<&str>) -> Result<User> {
    let user = match named {
        Some(name) => host
            .user(name)?
            .with_context(|| format!("--operator {name}: no such user in /etc/passwd"))?,
        None => {
            let uid = if env.euid == 0 {
                env.var("SUDO_UID").and_then(|u| u.parse().ok()).context(
                    "who is the operator? Run it with sudo from the operator's account, or \
                         name them with --operator <user>",
                )?
            } else {
                env.ruid
            };
            host.user_by_uid(uid)?.with_context(|| {
                format!("uid {uid} is not in /etc/passwd: name the operator with --operator <user>")
            })?
        }
    };
    if user.uid == 0 {
        bail!("the operator cannot be root: run it with sudo from the operator's account, or name them with --operator <user>");
    }
    Ok(user)
}

/// A user's primary group, by name.
fn own_group(host: &dyn Host, user: &User) -> Result<String> {
    Ok(host
        .group_by_gid(user.gid)?
        .map_or_else(|| user.gid.to_string(), |g| g.name))
}

/// `--user`: the operator's own daemon as a systemd user service.
pub(crate) fn user(env: &Env, g: &Globals, remove: bool, host: &dyn Host) -> Result<Layout> {
    let op = operator(env, host, None)?;
    let owner = Owner::new(&op.name, &own_group(host, &op)?);
    let home = env
        .var("HOME")
        .filter(|h| Path::new(h).is_absolute())
        .map(PathBuf::from)
        .context("HOME is not set to an absolute path: the unit's place is under it")?;
    let config_home = env
        .var("XDG_CONFIG_HOME")
        .filter(|h| Path::new(h).is_absolute())
        .map_or_else(|| home.join(".config"), PathBuf::from);
    let unit = config_home.join("systemd/user/theseusd.service");
    let who = format!("operator:  {} (uid {})", op.name, op.uid);
    if remove {
        return Ok(Layout {
            command: "theseusd install --user --remove".into(),
            context: vec![who],
            entries: vec![Entry {
                item: Item::NoFile {
                    path: unit,
                    rule: FileRule::Generated,
                },
                why: "the daemon's user unit".into(),
            }],
            parents: owner,
            notes: vec![
                "before --apply: systemctl --user disable --now theseusd.service".into(),
                "after it: systemctl --user daemon-reload".into(),
                "your state dir is not touched: the daemon you start by hand serves it as before"
                    .into(),
            ],
        });
    }
    let source = Source::of(&g.config, env);
    let mut exec = vec![
        env.exe.display().to_string(),
        "--config".into(),
        source.arg(),
    ];
    let mut context = vec![
        who,
        format!("binary:    {}", env.exe.display()),
        format!("config:    {}", source.arg()),
    ];
    match &g.state_dir {
        Some(d) => {
            let d = env.absolute(d);
            context.push(format!("state dir: {}", d.display()));
            exec.extend(["--state-dir".into(), d.display().to_string()]);
        }
        None => context.push(
            "state dir: the config's [server] state_dir (default ~/.theseus), as today".into(),
        ),
    }
    if let Some(s) = &g.socket {
        exec.extend(["--socket".into(), env.absolute(s).display().to_string()]);
    }
    let mut notes = vec![];
    let mut entries: Vec<Entry> = token_file(env, g, &op, &mut exec, &mut notes)
        .into_iter()
        .collect();
    let vars: Vec<(String, String)> = USER_ENV
        .iter()
        .filter_map(|k| env.var(k).map(|v| (k.to_string(), v.to_string())))
        .collect();
    if vars.iter().any(|(k, _)| k == "PATH") {
        context.push(
            "PATH:      this shell's, so the daemon finds `op` and your jobs their tools".into(),
        );
    }
    notes.extend([
        "systemctl --user daemon-reload".into(),
        "stop the daemon you started by hand (`theseus shutdown`), so the unit's can take its \
         socket"
            .into(),
        "systemctl --user enable --now theseusd.service".into(),
        format!(
            "loginctl enable-linger {}: your user manager, and the daemon, keep running when you \
             log out",
            op.name
        ),
        "journalctl --user -u theseusd: its log".into(),
    ]);
    entries.push(Entry {
        item: Item::File {
            path: unit,
            owner: owner.clone(),
            mode: 0o644,
            body: Body::Text(user_unit(&exec, &vars)?),
        },
        why: "the daemon as your systemd user service".into(),
    });
    Ok(Layout {
        command: "theseusd install --user".into(),
        context,
        entries,
        parents: owner,
        notes,
    })
}

/// The token file the unit names, as an item the plan checks and `--apply`
/// refuses on until it is right (theseus-w1nf); or, when none is named, the
/// note that gives the exact command to run again with one.
fn token_file(
    env: &Env,
    g: &Globals,
    op: &User,
    exec: &mut Vec<String>,
    notes: &mut Vec<String>,
) -> Option<Entry> {
    let Some(named) = &g.op_token_file else {
        notes.push(format!(
            "the unit names no token file, and the daemon will not start without its 1Password \
             token. Put the token in a file only you can read (mode 0600), then re-run: {}. A \
             unit never holds the token itself.",
            env.rerun_with("--op-token-file <file>")
        ));
        return None;
    };
    let path = env.absolute(Path::new(named));
    exec.extend(["--op-token-file".into(), path.display().to_string()]);
    Some(Entry {
        item: Item::Token {
            path,
            uid: op.uid,
            name: op.name.clone(),
        },
        why: "the 1Password token the daemon reads when it starts: its owner, mode, and size are \
              checked, and it is never opened"
            .into(),
    })
}

/// `--separate`: the daemon as `theseus`, or with `--remove`, all of it
/// taken away again.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
pub(crate) fn separate(env: &Env, g: &Globals, a: &InstallArgs, host: &dyn Host) -> Result<Layout> {
    let op = operator(env, host, a.operator.as_deref())?;
    let theseus = Owner::new(DAEMON_USER, DAEMON_USER);
    let root_theseus = Owner::new("root", DAEMON_USER);
    let who = format!("operator:  {} (uid {}), in {OPS_GROUP}", op.name, op.uid);
    let mut context = vec![who];
    if g.state_dir.is_some() || g.socket.is_some() {
        context.push(
            "ignored:   --state-dir and --socket (THESEUS_STATE_DIR, THESEUS_SOCKET) name the \
             daemon you run now; this layout has its own"
                .into(),
        );
    }
    let entry = |item: Item, why: &str| Entry {
        item,
        why: why.into(),
    };
    if a.remove {
        let stamp = env.stamp.clone();
        let entries =
            vec![
            entry(Item::Stopped(SOCKET.into()), "the daemon must be stopped first"),
            entry(
                Item::NoFile {
                    path: JOB_HOST_UNIT.into(),
                    rule: FileRule::Generated,
                },
                "the job host's user unit",
            ),
            entry(
                Item::NoFile {
                    path: SYSTEM_UNIT.into(),
                    rule: FileRule::Generated,
                },
                "the daemon's unit",
            ),
            entry(
                Item::NoFile {
                    path: TMPFILES.into(),
                    rule: FileRule::Generated,
                },
                "what made /run/theseus at boot",
            ),
            entry(
                Item::NoDir {
                    path: RUN_DIR.into(),
                    rule: DirRule::Sockets,
                    seal: false,
                },
                "the sockets' directory, with any socket a stopped daemon left",
            ),
            entry(
                Item::NoFile {
                    path: BINARY.into(),
                    rule: FileRule::Binary,
                },
                "the installed binary",
            ),
            entry(
                Item::NoDir {
                    path: LIB_DIR.into(),
                    rule: DirRule::IfEmpty,
                    seal: false,
                },
                "the binary's directory, once empty",
            ),
            entry(
                Item::NoFile {
                    path: CONFIG_FILE.into(),
                    rule: FileRule::Generated,
                },
                "a copied config file (the original is yours, and stays)",
            ),
            entry(
                Item::NoFile {
                    path: TOKEN_FILE.into(),
                    rule: FileRule::Token,
                },
                "the token's place: removed while empty; a token in it is kept, made root's",
            ),
            entry(
                Item::NoDir {
                    path: ETC_DIR.into(),
                    rule: DirRule::IfEmpty,
                    seal: true,
                },
                "the token's directory, once empty; otherwise made root:root 0700",
            ),
            entry(
                Item::NoDir {
                    path: STATE_DIR.into(),
                    rule: DirRule::State {
                        purge: a.purge_state,
                        stamp,
                    },
                    seal: true,
                },
                "the state dir: removed only while empty; one that holds anything stays (moved \
                 aside with --purge-state), made root:root 0700",
            ),
            entry(
                Item::NoMember {
                    group: OPS_GROUP.into(),
                    user: op.name,
                },
                "your membership",
            ),
            entry(
                Item::NoUser {
                    name: DAEMON_USER.into(),
                    home: STATE_DIR.into(),
                },
                "the daemon's user, if it is the one this installer makes",
            ),
            entry(Item::NoGroup(DAEMON_USER.into()), "the daemon user's own group"),
            entry(Item::NoGroup(OPS_GROUP.into()), "the operator's group"),
        ];
        return Ok(Layout {
            command: format!(
                "theseusd install --separate --remove{}",
                if a.purge_state { " --purge-state" } else { "" }
            ),
            context,
            entries,
            parents: Owner::root(),
            notes: vec![
                "before --apply: sudo systemctl disable --now theseusd.service; and, as yourself, \
                 systemctl --user disable --now theseus-job-host.service if you enabled it"
                    .into(),
                "after it: sudo systemctl daemon-reload".into(),
            ],
        });
    }

    let source = Source::of(&g.config, env);
    context.push(format!("binary:    {} -> {BINARY}", env.exe.display()));
    context.push(match &source {
        Source::Vault(r) => format!(
            "config:    {r}, the vault's note, stays the source; the unit's flags give the \
             layout's paths"
        ),
        Source::File(p) => format!(
            "config:    {}, copied to {CONFIG_FILE} for the daemon to read; the unit's flags give \
             the layout's paths",
            p.display()
        ),
    });
    let config_arg = match &source {
        Source::Vault(r) => r.clone(),
        Source::File(_) => CONFIG_FILE.into(),
    };
    let exec: Vec<String> = [
        BINARY,
        "--config",
        &config_arg,
        "--state-dir",
        STATE_DIR,
        "--socket",
        SOCKET,
        "--op-token-file",
        TOKEN_FILE,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let nologin = ["/usr/sbin/nologin", "/sbin/nologin"]
        .into_iter()
        .find(|s| real(env.root(), Path::new(s)).is_file())
        .unwrap_or("/usr/sbin/nologin");

    let mut entries = vec![
        entry(
            Item::Group(OPS_GROUP.into()),
            "the operator's group: its members reach the daemon's sockets",
        ),
        entry(
            Item::User {
                name: DAEMON_USER.into(),
                home: STATE_DIR.into(),
                shell: nologin.into(),
            },
            "the runtime and its storage run as their own OS identity, so a job, run as you, \
             cannot read or write them",
        ),
        entry(
            Item::Member {
                group: OPS_GROUP.into(),
                user: op.name,
            },
            "so you reach the daemon's sockets",
        ),
        entry(
            Item::Dir {
                path: STATE_DIR.into(),
                owner: theseus.clone(),
                mode: 0o700,
            },
            "the store (WAL, index, blobs), the config's copy, and the bindings file",
        ),
        entry(
            Item::Dir {
                path: ETC_DIR.into(),
                owner: root_theseus.clone(),
                mode: 0o750,
            },
            "the token's directory: the daemon reads it, your jobs cannot",
        ),
        entry(
            Item::File {
                path: TOKEN_FILE.into(),
                owner: theseus.clone(),
                mode: 0o600,
                body: Body::Placeholder,
            },
            "the 1Password service-account token's place; the installer never writes a token",
        ),
    ];
    if let Source::File(p) = &source {
        entries.push(entry(
            Item::File {
                path: CONFIG_FILE.into(),
                owner: root_theseus,
                mode: 0o640,
                body: Body::Copy {
                    from: p.clone(),
                    header: Some(format!(
                        "{HEADER} --separate`: a copy of {}.\n# Edit that file, then re-run \
                         --apply.\n",
                        p.display()
                    )),
                },
            },
            "your config, where the daemon can read it",
        ));
    }
    entries.extend([
        entry(
            Item::Dir {
                path: LIB_DIR.into(),
                owner: Owner::root(),
                mode: 0o755,
            },
            "the binary's directory",
        ),
        entry(
            Item::File {
                path: BINARY.into(),
                owner: Owner::root(),
                mode: 0o755,
                body: Body::Copy {
                    from: env.exe.clone(),
                    header: None,
                },
            },
            "the binary is part of the floor: the operator cannot replace it",
        ),
        entry(
            Item::File {
                path: SYSTEM_UNIT.into(),
                owner: Owner::root(),
                mode: 0o644,
                body: Body::Text(system_unit(&exec)?),
            },
            "the daemon as theseus",
        ),
        entry(
            Item::File {
                path: JOB_HOST_UNIT.into(),
                owner: Owner::root(),
                mode: 0o644,
                body: Body::Text(job_host_unit()),
            },
            "the job host's user unit, disabled until step 22b; here rather than in your home, \
             so root never writes where your jobs can",
        ),
        entry(
            Item::File {
                path: TMPFILES.into(),
                owner: Owner::root(),
                mode: 0o644,
                body: Body::Text(tmpfiles()),
            },
            "makes /run/theseus at each boot: /run is a tmpfs",
        ),
        entry(
            Item::Dir {
                path: RUN_DIR.into(),
                owner: Owner::new(DAEMON_USER, OPS_GROUP),
                mode: 0o750,
            },
            "the sockets' directory: theseus.sock, and the job host's jobs.sock (22b)",
        ),
    ]);
    if let Some(from) = &a.migrate_state {
        let from = env.absolute(from);
        context.push(format!(
            "migrate:   {} (its {}), only read",
            from.display(),
            NAMES.join(", ")
        ));
        entries.push(entry(
            Item::State(Migrate {
                from,
                to: STATE_DIR.into(),
                owner: theseus,
            }),
            "your stopped daemon's state; the source is only read, and never deleted",
        ));
    }

    let token = g.op_token_file.as_deref().map_or_else(
        || "<your token file>".to_string(),
        |t| env.absolute(Path::new(t)).display().to_string(),
    );
    let mut notes = vec![format!(
        "copy the token in: sudo install -o {DAEMON_USER} -g {DAEMON_USER} -m 0600 {token} {TOKEN_FILE}"
    )];
    let op_cli = SYSTEM_PATH
        .iter()
        .map(|d| Path::new(d).join("op"))
        .find(|p| real(env.root(), p).is_file());
    if op_cli.is_none() {
        let mut note = format!(
            "the daemon runs `op` from systemd's PATH ({}), and none is there: install the \
             1Password CLI system-wide, or add its directory in a drop-in (`sudo systemctl edit \
             theseusd`: [Service] Environment=PATH=<its dir>:/usr/local/bin:/usr/bin:/bin)",
            SYSTEM_PATH.join(":")
        );
        if let Some(found) = env.which("op") {
            note.push_str(&format!("; yours is {}", found.display()));
        }
        notes.push(note);
    }
    notes.extend([
        "sudo systemctl daemon-reload".into(),
        "stop the daemon you run now (`theseus shutdown`): both would serve the web UI's port and \
         Discord; then sudo systemctl enable --now theseusd.service"
            .into(),
        format!(
            "log in again (or `newgrp {OPS_GROUP}`) for your membership in {OPS_GROUP} to take \
             effect"
        ),
        "leave theseus-job-host.service disabled until step 22b".into(),
    ]);
    Ok(Layout {
        command: "theseusd install --separate".into(),
        context,
        entries,
        parents: Owner::root(),
        notes,
    })
}
