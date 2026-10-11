//! The finder (theseus-uttu): a pure function over ledger rows. It reads
//! the questions the gate asked (`tool.confirm_requested`), the answers
//! (`action.confirmed`, `action.declined`) and every "should have asked"
//! (`policy.tightened` with a call), and proposes the groups the owner
//! approved every time:
//!
//! - **`proc.run`**, by the shortest argv prefix that holds across a group's
//!   approvals and is at least the program and one argument. The approvals
//!   go into a trie of their argv's words (the program by its base name); a
//!   node is a candidate when the approvals that reach it and no narrower
//!   candidate holds number at least `min_approvals`. So `cargo nextest run
//!   -p a` approved 30 times and `-p b` 27 are two candidates, and 57 `git
//!   commit -m <each a different message>` are one, `git commit -m`, said
//!   to take anything after it. A shell, or a program that runs what it is
//!   given, is never a prefix (`prefix_ok`).
//! - **A file tool**, by tool and workspace root.
//!
//! A group is a candidate only if no call under its prefix (or tool and
//! root) in the window was declined, pressed "should have asked" on, or
//! reached the floor, and each of its approvals was asked by the tool's
//! posture alone: an approval the approve list, the roots, the floor or the
//! external-text hold asked for says nothing about the posture a grant
//! changes. A call with an `env` or steps is no evidence either.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use theseus_protocol::earned::{EarnedGroup, EarnedRecord};

use crate::ledger::LedgerRow;

/// What the finder is asked.
pub struct FindParams<'a> {
    pub now_ms: u64,
    /// Approvals and refusals count from `now_ms - window_ms`.
    pub window_ms: u64,
    pub min_approvals: u64,
    /// The workspace roots, canonical: a file tool's approvals group by the
    /// one that holds their path.
    pub roots: &'a [PathBuf],
    /// Whether a group is not to be proposed: a grant already, passed on, or
    /// on an open card.
    pub skip: &'a dyn Fn(&str) -> bool,
}

/// A question the gate asked, as its row says it.
#[derive(Debug, Clone)]
pub(crate) struct Ask {
    pub corr: String,
    pub tool: String,
    pub session: String,
    pub at_ms: u64,
    pub floor: bool,
    /// Asked for the external-text hold.
    pub external: bool,
    /// The tool's posture alone asked (`by_posture`).
    pub posture: bool,
    /// `proc.run`'s argv, its program by base name; none for a batch or a
    /// call with an `env`.
    pub argv: Option<Vec<String>>,
    /// A file tool's path.
    pub path: Option<PathBuf>,
}

/// Whether a reason says the tool's posture asked, and nothing listed: the
/// gate's reasons end `— approve (<why>)`, and a posture's why is a setting.
/// A tightening's is not a posture the owner chose to loosen.
pub(crate) fn by_posture(reason: &str) -> bool {
    let Some((_, why)) = reason.rsplit_once("— approve (") else {
        return false;
    };
    [
        "enforcement =",
        "[policy.tools]",
        "[policy.aws]",
        "[policy.mcp]",
    ]
    .iter()
    .any(|s| why.starts_with(s))
}

/// The program's base name first, as the gate matches argv.
pub(crate) fn normalized(argv: &[String]) -> Vec<String> {
    crate::policy::normalized_argv(argv)
}

/// The asks in `rows`, by correlation id.
pub(crate) fn asks(rows: &[LedgerRow]) -> BTreeMap<String, Ask> {
    let mut out = BTreeMap::new();
    for r in rows.iter().filter(|r| r.is_kind("tool.confirm_requested")) {
        let d = &r.data;
        let (Some(corr), Some(tool)) = (d["correlation_id"].as_str(), d["tool"].as_str()) else {
            continue;
        };
        let input = &d["input"];
        let argv = (tool == "proc.run")
            .then(|| {
                let plain = input.get("steps").is_none_or(Value::is_null)
                    && input
                        .get("env")
                        .is_none_or(|e| e.is_null() || e.as_object().is_some_and(|o| o.is_empty()));
                plain
                    .then(|| input["argv"].as_array())
                    .flatten()
                    .map(|a| {
                        a.iter()
                            .filter_map(|w| w.as_str().map(str::to_string))
                            .collect()
                    })
                    .map(|v: Vec<String>| normalized(&v))
            })
            .flatten();
        out.insert(
            corr.to_string(),
            Ask {
                corr: corr.into(),
                tool: tool.into(),
                session: d["session_id"].as_str().unwrap_or("").into(),
                at_ms: r.at_unix_ms,
                floor: d["floor"].as_bool().unwrap_or(false),
                external: !d["external_text"].is_null(),
                posture: by_posture(d["reason"].as_str().unwrap_or("")),
                argv,
                path: input["path"].as_str().map(PathBuf::from),
            },
        );
    }
    out
}

/// Words a program is never a grant's prefix by: a shell, or a program that
/// runs what it is given next.
const RUNS_WHAT_IT_IS_GIVEN: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ash", "ksh", "csh", "tcsh", "fish", "busybox", "env", "xargs",
    "sudo", "doas", "su", "eval", "exec", "nohup", "timeout", "nice", "ionice", "time", "setsid",
    "chroot", "ssh", "watch", "parallel", "script", "expect", "nsenter", "unshare", "strace",
    "gdb", "awk", "gawk", "mawk", "sed", "find", "command", "builtin", "stdbuf", "flock",
];

/// Interpreters: a prefix runs a script file or it is not a prefix.
const INTERPRETERS: &[&str] = &[
    "python",
    "python2",
    "python3",
    "perl",
    "ruby",
    "node",
    "nodejs",
    "deno",
    "bun",
    "php",
    "lua",
    "luajit",
    "Rscript",
    "osascript",
    "pwsh",
    "powershell",
];

/// Why `prefix` is never a grant's: the program and one argument at least,
/// and not a shell or a program whose argument is code. The gate asks this
/// again of every grant it reads, so a hand-edited store gains nothing.
pub fn prefix_ok(prefix: &[String]) -> Result<(), String> {
    let Some(program) = prefix.first() else {
        return Err("it names no program".into());
    };
    if prefix.len() < 2 {
        return Err(format!(
            "`{program}` alone is too short: a prefix holds the program and one argument at least"
        ));
    }
    let base = Path::new(program)
        .file_name()
        .map_or(program.clone(), |b| b.to_string_lossy().into_owned());
    if RUNS_WHAT_IT_IS_GIVEN.contains(&base.as_str()) {
        return Err(format!(
            "`{base}` runs what its arguments give it (a shell or a wrapper), so its argument is \
             code, not a stable command"
        ));
    }
    if prefix.iter().skip(1).any(|w| {
        matches!(
            w.as_str(),
            "-c" | "-e"
                | "-E"
                | "-x"
                | "--eval"
                | "--exec"
                | "-exec"
                | "-execdir"
                | "-lc"
                | "-ec"
                | "--command"
                | "--cmd"
        )
    }) {
        return Err("a flag takes code or a command (`-c`, `-e`, `--exec`)".into());
    }
    let interpreter = INTERPRETERS.iter().any(|i| {
        base == *i
            || base.strip_prefix(i).is_some_and(|v| {
                v.starts_with('.') || v.chars().all(|c| c.is_ascii_digit() || c == '.')
            })
    });
    if interpreter && prefix[1].starts_with('-') {
        return Err(format!(
            "`{base}` with an option first may take code inline; a script file's path comes first"
        ));
    }
    Ok(())
}

/// A group's id from its key.
pub(crate) fn id_of(key: &str) -> String {
    let h = Sha256::digest(key.as_bytes());
    format!("ern_{}", hex::encode(&h[..4]))
}

/// The key of a `proc.run` prefix, and of a file tool in a root.
pub(crate) fn key_of_prefix(prefix: &[String]) -> String {
    format!("proc.run\u{1}{}", prefix.join("\u{1}"))
}

pub(crate) fn key_of_root(tool: &str, root: &Path) -> String {
    format!("{tool}\u{2}{}", root.display())
}

#[derive(Default)]
struct Node {
    children: BTreeMap<String, Node>,
    /// Every item that reaches this node.
    all: Vec<usize>,
}

/// The approved calls the groups are made of.
struct Item<'a> {
    ask: &'a Ask,
    approved_ms: u64,
}

fn insert(root: &mut Node, argv: &[String], item: usize) {
    let mut n = root;
    for w in argv {
        n = n.children.entry(w.clone()).or_default();
        n.all.push(item);
    }
}

/// The candidate nodes under `n`, narrowest first: a node whose subtree
/// holds a narrower candidate is not one itself.
fn emit(n: &Node, path: &mut Vec<String>, min: usize, out: &mut Vec<(Vec<String>, Vec<usize>)>) {
    let before = out.len();
    for (w, c) in &n.children {
        path.push(w.clone());
        emit(c, path, min, out);
        path.pop();
    }
    if out.len() == before && path.len() >= 2 && n.all.len() >= min {
        out.push((path.clone(), n.all.clone()));
    }
}

fn record(items: &[&Item<'_>], samples: Vec<String>) -> EarnedRecord {
    let mut ids: Vec<&str> = items.iter().map(|i| i.ask.corr.as_str()).collect();
    ids.sort_unstable();
    let digest = hex::encode(Sha256::digest(ids.join("\n").as_bytes()));
    EarnedRecord {
        approvals: items.len() as u64,
        first_ms: items.iter().map(|i| i.approved_ms).min().unwrap_or(0),
        last_ms: items.iter().map(|i| i.approved_ms).max().unwrap_or(0),
        sessions: items
            .iter()
            .map(|i| i.ask.session.as_str())
            .collect::<BTreeSet<_>>()
            .len() as u64,
        samples,
        digest: digest[..32].to_string(),
    }
}

/// The three newest distinct calls, newest first.
fn samples(items: &[&Item<'_>], say: impl Fn(&Ask) -> String) -> Vec<String> {
    let mut by_time: Vec<&&Item<'_>> = items.iter().collect();
    by_time.sort_by_key(|i| std::cmp::Reverse(i.approved_ms));
    let mut out: Vec<String> = Vec::new();
    for i in by_time {
        let s = say(i.ask);
        if !out.contains(&s) {
            out.push(s);
        }
        if out.len() == 3 {
            break;
        }
    }
    out
}

/// The `proc.run` groups: the trie of approved argvs.
fn procs(items: &[Item<'_>], against: &[&Ask], p: &FindParams<'_>, min: usize) -> Vec<EarnedGroup> {
    let mut out = Vec::new();
    let mut root = Node::default();
    for (i, it) in items.iter().enumerate() {
        if let Some(a) = it.ask.argv.as_ref().filter(|a| a.len() >= 2) {
            insert(&mut root, a, i);
        }
    }
    let mut found = Vec::new();
    emit(&root, &mut Vec::new(), min, &mut found);
    for (prefix, idx) in found {
        if prefix_ok(&prefix).is_err() {
            continue;
        }
        let covered = |a: &Ask| {
            a.tool == "proc.run"
                && a.argv
                    .as_ref()
                    .is_some_and(|v| v.len() >= prefix.len() && v[..prefix.len()] == prefix[..])
        };
        if against.iter().any(|a| covered(a)) {
            continue;
        }
        let id = id_of(&key_of_prefix(&prefix));
        if (p.skip)(&id) {
            continue;
        }
        let group: Vec<&Item<'_>> = idx.iter().map(|i| &items[*i]).collect();
        let open_ended = group
            .iter()
            .any(|i| i.ask.argv.as_ref().is_some_and(|v| v.len() > prefix.len()));
        out.push(EarnedGroup {
            id,
            tool: "proc.run".into(),
            label: prefix.join(" "),
            prefix,
            root: None,
            open_ended,
            record: record(
                &group,
                samples(&group, |a| {
                    a.argv.as_ref().map(|v| v.join(" ")).unwrap_or_default()
                }),
            ),
        });
    }

    out
}

/// The file tools' groups: a tool in a workspace root.
fn files(items: &[Item<'_>], against: &[&Ask], p: &FindParams<'_>, min: usize) -> Vec<EarnedGroup> {
    let mut out = Vec::new();
    let mut files: BTreeMap<(String, usize), Vec<usize>> = BTreeMap::new();
    for (i, it) in items.iter().enumerate() {
        let (a, Some(path)) = (it.ask, it.ask.path.as_ref()) else {
            continue;
        };
        if !a.tool.starts_with("fs.") {
            continue;
        }
        if let Some(r) = p
            .roots
            .iter()
            .position(|r| theseus_tools::paths::within(path, r))
        {
            files.entry((a.tool.clone(), r)).or_default().push(i);
        }
    }
    for ((tool, r), idx) in files {
        if idx.len() < min {
            continue;
        }
        let root = &p.roots[r];
        let under = |a: &Ask| {
            a.tool == tool
                && a.path
                    .as_ref()
                    .is_some_and(|x| theseus_tools::paths::within(x, root))
        };
        if against.iter().any(|a| under(a)) {
            continue;
        }
        let id = id_of(&key_of_root(&tool, root));
        if (p.skip)(&id) {
            continue;
        }
        let group: Vec<&Item<'_>> = idx.iter().map(|i| &items[*i]).collect();
        out.push(EarnedGroup {
            id,
            label: format!("{tool} under {}", root.display()),
            tool: tool.clone(),
            prefix: Vec::new(),
            root: Some(root.display().to_string()),
            open_ended: true,
            record: record(
                &group,
                samples(&group, |a| {
                    a.path
                        .as_ref()
                        .map(|x| x.display().to_string())
                        .unwrap_or_default()
                }),
            ),
        });
    }
    out
}

/// The candidates `rows` hold, most approved first. `rows` are the ledger's
/// in time order, from before the window by a day or two so a question
/// whose answer falls inside it is read.
pub fn find(rows: &[LedgerRow], p: &FindParams<'_>) -> Vec<EarnedGroup> {
    let since = p.now_ms.saturating_sub(p.window_ms);
    let asks = asks(rows);
    let in_window = |ms: u64| ms >= since && ms <= p.now_ms;
    let mut approved: BTreeMap<&str, u64> = BTreeMap::new();
    let mut refused: BTreeSet<&str> = BTreeSet::new();
    for r in rows {
        let Some(corr) = r.data["correlation_id"].as_str() else {
            continue;
        };
        match r.kind.as_str() {
            "action.confirmed" if in_window(r.at_unix_ms) => {
                approved.entry(corr).or_insert(r.at_unix_ms);
            }
            "action.declined" | "action.denied" | "policy.tightened" if in_window(r.at_unix_ms) => {
                refused.insert(corr);
            }
            _ => {}
        }
    }
    // A call that reached the floor, asked in the window, counts against.
    let against: Vec<&Ask> = asks
        .values()
        .filter(|a| refused.contains(a.corr.as_str()) || (a.floor && in_window(a.at_ms)))
        .collect();
    let items: Vec<Item<'_>> = approved
        .iter()
        .filter_map(|(c, ms)| {
            let a = asks.get(*c)?;
            (a.posture && !a.floor && !a.external).then_some(Item {
                ask: a,
                approved_ms: *ms,
            })
        })
        .collect();
    let min = p.min_approvals.max(1) as usize;
    let mut out = procs(&items, &against, p, min);
    out.extend(files(&items, &against, p, min));
    out.sort_by(|a, b| {
        b.record
            .approvals
            .cmp(&a.record.approvals)
            .then(a.label.cmp(&b.label))
    });
    out
}
