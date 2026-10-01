//! The reader rule's registry test (P0's rule 3, theseus-wjy): nothing is
//! declared without its reader. Each check enumerates what is declared, from
//! the table that declares it, and fails an item nothing reads, unless a
//! reserved marker names the roadmap re-cut's row and milestone that bring its
//! reader (`row <n> (<step>), <milestone>: <its reader>`).
//!
//! - **Crates.** Every workspace member is reached by normal dependencies from
//!   `theseusd`, `theseus`, or a tool binary (an installed binary of its own,
//!   `[package.metadata.theseus] tool = "<what runs it>"`), or its manifest
//!   says `reserved_for`. It reads the `Cargo.toml` files: `cargo metadata`
//!   takes most of a second.
//! - **Methods.** Every `method::*` name has its dispatch arm: a core answers
//!   it with anything but "method not found".
//! - **Notifications.** Every `notify::*` name has its `Event`, and every
//!   `Event` a sender: this crate's code, its tests aside, builds it.
//! - **Edge kinds and labels** (`graph`). Every variant has a reader: a
//!   `match` arm, a pattern, or an `==` that names it by its type, in the code
//!   of a crate the binaries run, its tests aside.
//!
//! Every failure names the item and its fix. A marker on an item that has its
//! reader now fails too, so Part III's list of reserved items stays true:
//! `print_the_reserved_list` prints it. The gate runs this module before the
//! rest of the suite. It lives in theseus-core because the dispatch check
//! needs a core, and the senders are this crate's.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};
use serde_json::Value;
use theseus_protocol::{error_code, method, notify, Event, Id, Message, Request};
use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::graph::{EdgeKind, Label};
use crate::provider::FakeProvider;
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::Config;

/// The binaries that ship. What they reach is read, and so is what a tool
/// binary reaches.
const ROOTS: [&str; 2] = ["theseusd", "theseus"];

/// What is declared ahead of its reader, besides crates (whose markers are in
/// their own manifests): `("<kind> <name>", "row <n> (<step>), <milestone>:
/// <its reader>")`. The kind is `method`, `notify`, `edge`, or `label`, and
/// the name is the wire's (`node.reach`, `derived_from`). Empty: everything
/// else has its reader.
const RESERVED: &[(&str, &str)] = &[];

/// How a failure for an item with no reader ends: what to add, or the marker
/// to add instead, in `RESERVED`.
fn fix(add: &str, item: &str, reader: &str) -> String {
    format!(
        "Add {add} on the same commit, or reserve it: in tests_registry.rs's `RESERVED`, \
         `(\"{item}\", \"row <n> (<step>), <milestone>: <{reader}>\")`"
    )
}

/// Fails with every problem found, one to a line.
fn fail_on(problems: Vec<String>) {
    assert!(
        problems.is_empty(),
        "the reader rule (theseus-wjy):\n- {}\n",
        problems.join("\n- ")
    );
}

/// The `RESERVED` marker of `<kind> <name>`.
fn reserved(kind: &str, name: &str) -> Option<&'static str> {
    RESERVED
        .iter()
        .find(|(item, _)| item.split_once(' ') == Some((kind, name)))
        .map(|(_, marker)| *marker)
}

/// A `RESERVED` entry of `kind` whose name is not declared: a typo, or an item
/// since removed.
fn orphans(kind: &str, declared: &[&str]) -> Vec<String> {
    RESERVED
        .iter()
        .filter_map(|(item, _)| {
            let (k, name) = item.split_once(' ')?;
            (k == kind && !declared.contains(&name)).then(|| {
                format!("`RESERVED` names {kind} `{name}`, which is not declared: remove the entry")
            })
        })
        .collect()
}

/// Why a reserved marker is malformed, if it is. Its form is `row <n>
/// (<step>), <milestone>: <its reader>`: a row of the roadmap re-cut's §2, with
/// its step, and a milestone, `M<n>` (`M3.6`), or `AWS`, block D, which floats
/// on Eddie's go and belongs to no milestone.
fn malformed(marker: &str) -> Option<String> {
    const FORM: &str = "its form is `row <n> (<step>), <milestone>: <its reader>`";
    let Some((head, reader)) = marker.split_once(": ") else {
        return Some(format!("it names no reader; {FORM}"));
    };
    let Some((row, milestone)) = head.rsplit_once(", ") else {
        return Some(format!("it names no milestone; {FORM}"));
    };
    let digit = |s: &str| s.starts_with(|c: char| c.is_ascii_digit());
    if !row.strip_prefix("row ").is_some_and(digit) {
        return Some(format!("`{row}` is no row; {FORM}"));
    }
    let numbered = milestone
        .strip_prefix('M')
        .is_some_and(|m| digit(m) && m.chars().all(|c| c.is_ascii_digit() || c == '.'));
    if !numbered && milestone != "AWS" {
        return Some(format!(
            "`{milestone}` is no milestone (`M4`, `M3.6`, or `AWS`); {FORM}"
        ));
    }
    if reader.trim().is_empty() {
        return Some(format!("it names no reader; {FORM}"));
    }
    None
}

// ---------------------------------------------------------------- crates

/// A workspace member, as its manifest declares it.
struct Member {
    name: String,
    /// Its directory, from the workspace's root (`crates/theseus-core`).
    dir: String,
    /// What it depends on: normal dependencies, target-specific ones too, but
    /// not optional ones, which a binary has only with their feature.
    deps: Vec<String>,
    reserved_for: Option<String>,
    tool: Option<String>,
    has_bin: bool,
    /// What is wrong with its `[package.metadata.theseus]`.
    problems: Vec<String>,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace's root")
}

fn manifest(path: &Path) -> toml::Table {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every workspace member, in the `members` list's order.
fn members() -> Vec<Member> {
    let root = workspace_root();
    let top = manifest(&root.join("Cargo.toml"));
    let workspace = top
        .get("workspace")
        .and_then(toml::Value::as_table)
        .expect("the root manifest's [workspace]");
    // What each `workspace = true` dependency builds (`package = …` renames).
    let shared: BTreeMap<&str, &str> = workspace
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .map(|deps| {
            deps.iter()
                .map(|(key, spec)| {
                    let built = spec.get("package").and_then(toml::Value::as_str);
                    (key.as_str(), built.unwrap_or(key.as_str()))
                })
                .collect()
        })
        .unwrap_or_default();
    workspace
        .get("members")
        .and_then(toml::Value::as_array)
        .expect("[workspace] members")
        .iter()
        .map(|dir| member(&root, dir.as_str().expect("a member is a path"), &shared))
        .collect()
}

fn member(root: &Path, dir: &str, shared: &BTreeMap<&str, &str>) -> Member {
    assert!(
        !dir.contains(['*', '?', '[']),
        "workspace member `{dir}` is a glob: list each crate, so the reader rule's registry test reads it"
    );
    let path = format!("{dir}/Cargo.toml");
    let m = manifest(&root.join(&path));
    let package = m
        .get("package")
        .and_then(toml::Value::as_table)
        .unwrap_or_else(|| panic!("{path}: no [package]"));
    let name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("{path}: no package name"))
        .to_string();
    // The dependency tables cargo builds into the crate's own code.
    let mut tables: Vec<&toml::Table> = m
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .into_iter()
        .collect();
    if let Some(targets) = m.get("target").and_then(toml::Value::as_table) {
        tables.extend(
            targets
                .values()
                .filter_map(|t| t.get("dependencies"))
                .filter_map(toml::Value::as_table),
        );
    }
    let mut deps = Vec::new();
    for (key, spec) in tables.into_iter().flatten() {
        if spec.get("optional").and_then(toml::Value::as_bool) == Some(true) {
            continue;
        }
        let built = if spec.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
            shared.get(key.as_str()).copied().unwrap_or(key.as_str())
        } else {
            let renamed = spec.get("package").and_then(toml::Value::as_str);
            renamed.unwrap_or(key.as_str())
        };
        deps.push(built.to_string());
    }

    let (mut reserved_for, mut tool, mut problems) = (None, None, Vec::new());
    if let Some(marks) = package.get("metadata").and_then(|m| m.get("theseus")) {
        let Some(marks) = marks.as_table() else {
            panic!("{path}: [package.metadata.theseus] is not a table");
        };
        for (key, value) in marks {
            let Some(text) = value.as_str() else {
                problems.push(format!(
                    "{path}: `{key}` under [package.metadata.theseus] is not a string"
                ));
                continue;
            };
            match key.as_str() {
                "reserved_for" => reserved_for = Some(text.to_string()),
                "tool" => tool = Some(text.to_string()),
                _ => problems.push(format!(
                    "{path}: `{key}` under [package.metadata.theseus] is no marker: `reserved_for` or `tool`"
                )),
            }
        }
    }
    let src = root.join(dir).join("src");
    let has_bin = m
        .get("bin")
        .and_then(toml::Value::as_array)
        .is_some_and(|bins| !bins.is_empty())
        || src.join("main.rs").is_file()
        || src.join("bin").is_dir();
    Member {
        name,
        dir: dir.to_string(),
        deps,
        reserved_for,
        tool,
        has_bin,
        problems,
    }
}

/// Every member the shipped binaries run, with the path that reaches it
/// (`theseusd → theseus-core → theseus-kernel`): from `ROOTS`, and from each
/// tool.
fn reached(members: &[Member]) -> BTreeMap<String, Vec<String>> {
    let by_name: BTreeMap<&str, &Member> = members.iter().map(|m| (m.name.as_str(), m)).collect();
    for root in ROOTS {
        assert!(
            by_name.contains_key(root),
            "`{root}` is no workspace member"
        );
    }
    let tools = members
        .iter()
        .filter(|m| m.tool.is_some())
        .map(|m| m.name.as_str());
    let mut queue: VecDeque<Vec<String>> = ROOTS
        .into_iter()
        .chain(tools)
        .map(|name| vec![name.to_string()])
        .collect();
    let mut reached = BTreeMap::new();
    while let Some(path) = queue.pop_front() {
        let name = path.last().expect("a path names its member").clone();
        if reached.contains_key(&name) {
            continue;
        }
        for dep in &by_name[name.as_str()].deps {
            if by_name.contains_key(dep.as_str()) && !reached.contains_key(dep) {
                let mut next = path.clone();
                next.push(dep.clone());
                queue.push_back(next);
            }
        }
        reached.insert(name, path);
    }
    reached
}

/// Every crate is read: reached from a shipped binary or a tool, a tool
/// itself, or reserved for the row that wires it in.
#[test]
fn every_crate_is_read_by_a_binary_or_reserved() {
    let members = members();
    let reached = reached(&members);
    let mut problems = Vec::new();
    for m in &members {
        let (name, path) = (&m.name, format!("{}/Cargo.toml", m.dir));
        problems.extend(m.problems.iter().cloned());
        if let Some(why) = m.reserved_for.as_deref().and_then(malformed) {
            problems.push(format!(
                "crate `{name}`: the `reserved_for` in {path} is malformed: {why}"
            ));
        }
        if m.tool.is_some() && !m.has_bin {
            problems.push(format!(
                "crate `{name}` says `tool` in {path}, but builds no binary: a tool is an installed \
                 binary of its own, so mark a library `reserved_for`"
            ));
        }
        if m.tool.is_some() && m.reserved_for.is_some() {
            problems.push(format!(
                "crate `{name}` says both `tool` and `reserved_for` in {path}: a tool is its own \
                 reader, so keep one"
            ));
            continue;
        }
        match (reached.get(name), &m.reserved_for) {
            (Some(by), Some(_)) => problems.push(format!(
                "crate `{name}` is read now ({}), so its `reserved_for` is stale: remove it from \
                 {path}, and its line from Part III's reserved list",
                by.join(" → ")
            )),
            (None, None) => problems.push(format!(
                "crate `{name}` has no reader: nothing theseusd or theseus reaches depends on it, and \
                 it is no tool. Add its reader on the same commit (a normal dependency on the path from \
                 theseusd or theseus), or reserve it: in {path}, `[package.metadata.theseus] \
                 reserved_for = \"row <n> (<step>), <milestone>: <its reader>\"` (or `tool = \"<what runs \
                 it>\"`, for an installed binary of its own)"
            )),
            _ => {}
        }
    }
    fail_on(problems);
}

// ---------------------------------------------------------------- the protocol

/// A core on a fresh store in `dir`: the example config, a fake provider.
fn core_in(dir: &Path) -> Arc<Core> {
    let store = Store::open(&dir.join("store")).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        store,
    ))
    .unwrap()
}

/// Asks `core` each of `methods` on one connection, with no params, and
/// returns each one's error code (`None` for a result).
async fn ask(core: Arc<Core>, methods: &[&str]) -> BTreeMap<String, Option<i64>> {
    let (client, server) = duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let serving = tokio::spawn(core.serve_connection(sr, sw, "registry".into()));
    let (cr, mut cw) = tokio::io::split(client);
    for (i, m) in methods.iter().enumerate() {
        let request = Request::new(Id::Num(i as u64), m, Value::Null);
        let mut line = serde_json::to_string(&request).unwrap();
        line.push('\n');
        cw.write_all(line.as_bytes()).await.unwrap();
    }
    let mut lines = BufReader::new(cr).lines();
    let mut codes = BTreeMap::new();
    let answers = async {
        while codes.len() < methods.len() {
            let line = lines.next_line().await.unwrap();
            let line = line.expect("the connection closed before every method answered");
            if let Message::Response(r) = serde_json::from_str(&line).unwrap() {
                let Id::Num(i) = r.id else {
                    panic!("an answer to a request never sent: {:?}", r.id)
                };
                codes.insert(methods[i as usize].to_string(), r.error.map(|e| e.code));
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(20), answers)
        .await
        .expect("every method answers within 20 s");
    let _ = cw.shutdown().await;
    drop(lines);
    let _ = serving.await;
    codes
}

/// Every method has its dispatch arm: the core answers it, most with an error
/// of their own (no params), never with "method not found".
#[tokio::test]
async fn every_method_has_its_dispatch_arm() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_in(dir.path());
    // `shutdown` last, on a connection of its own: it ends the serving loops.
    let (last, first): (Vec<&str>, Vec<&str>) = method::ALL
        .iter()
        .copied()
        .partition(|m| *m == method::SHUTDOWN);
    let mut codes = ask(core.clone(), &first).await;
    codes.extend(ask(core, &last).await);
    let mut problems = Vec::new();
    for name in method::ALL {
        let dispatched = codes[*name] != Some(error_code::METHOD_NOT_FOUND);
        match (dispatched, reserved("method", name)) {
            (false, None) => problems.push(format!(
                "method `{name}` has no dispatch arm: the core answers it \"method not found\". {}",
                fix(
                    "its arm in `dispatch` (theseus-core's rpc/server.rs)",
                    &format!("method {name}"),
                    "its arm"
                )
            )),
            (true, Some(_)) => problems.push(format!(
                "method `{name}` is dispatched now, so its `RESERVED` entry is stale: remove it, and \
                 its line from Part III's reserved list"
            )),
            _ => {}
        }
    }
    problems.extend(orphans("method", method::ALL));
    fail_on(problems);
}

/// Every notification has its `Event`, and every `Event` its sender.
#[test]
fn every_notification_has_its_event_and_a_sender() {
    let events: BTreeMap<&str, &str> = Event::VARIANTS.iter().map(|(v, m)| (*m, *v)).collect();
    let uses = uses_of(&[PathBuf::from(env!("CARGO_MANIFEST_DIR"))], &["Event"]);
    let mut problems = Vec::new();
    for name in notify::ALL {
        let variant = events.get(name);
        let sender = variant.and_then(|v| uses.built.get(&format!("Event::{v}")));
        match (variant, sender, reserved("notify", name)) {
            (None, _, None) => problems.push(format!(
                "notification `{name}` has no `Event`, so nothing can send it. {}",
                fix(
                    "its variant in `events!`'s table (theseus-protocol's events.rs), and its sender,",
                    &format!("notify {name}"),
                    "its sender"
                )
            )),
            (Some(v), None, None) => problems.push(format!(
                "notification `{name}` (`Event::{v}`) has no sender: no code of theseus-core, its tests \
                 aside, builds `Event::{v}(…)`. {}",
                fix("its sender", &format!("notify {name}"), "its sender")
            )),
            (Some(v), Some(at), Some(_)) => problems.push(format!(
                "notification `{name}` is sent now (`Event::{v}`, in {at}), so its `RESERVED` entry is \
                 stale: remove it, and its line from Part III's reserved list"
            )),
            _ => {}
        }
    }
    problems.extend(orphans("notify", notify::ALL));
    fail_on(problems);
}

// ---------------------------------------------------------------- edge kinds and labels

/// Every edge kind and every label has its reader. Both are empty until rows 12
/// (12a) and 21 (19a).
#[test]
fn every_edge_kind_and_label_has_its_reader() {
    let vocabularies = [
        ("edge", "edge kind", "EdgeKind", EdgeKind::VARIANTS),
        ("label", "label", "Label", Label::VARIANTS),
    ];
    // The scan reads every crate the binaries run, so only when there is
    // something to look for.
    let uses = if vocabularies
        .iter()
        .any(|(.., variants)| !variants.is_empty())
    {
        let members = members();
        let reached = reached(&members);
        let root = workspace_root();
        let dirs: Vec<PathBuf> = members
            .iter()
            .filter(|m| reached.contains_key(&m.name))
            .map(|m| root.join(&m.dir))
            .collect();
        uses_of(&dirs, &["EdgeKind", "Label"])
    } else {
        Uses::default()
    };
    let mut problems = Vec::new();
    for (kind, what, ty, variants) in vocabularies {
        for (variant, name) in variants {
            match (uses.read.get(&format!("{ty}::{variant}")), reserved(kind, name)) {
                (None, None) => problems.push(format!(
                    "{what} `{name}` (`{ty}::{variant}`) has no reader: no code the binaries run, its \
                     tests aside, matches it, binds it in a pattern, or compares it with `==`. {}",
                    fix(
                        &format!("a reader of `{ty}::{variant}`"),
                        &format!("{kind} {name}"),
                        "its reader"
                    )
                )),
                (Some(at), Some(_)) => problems.push(format!(
                    "{what} `{name}` is read now (`{ty}::{variant}`, in {at}), so its `RESERVED` entry \
                     is stale: remove it, and its line from Part III's reserved list"
                )),
                _ => {}
            }
        }
        let names: Vec<&str> = variants.iter().map(|(_, name)| *name).collect();
        problems.extend(orphans(kind, &names));
    }
    fail_on(problems);
}

/// Every entry of `RESERVED` names its kind, and has a marker's form.
#[test]
fn every_reserved_entry_has_its_form() {
    let mut problems = Vec::new();
    for (item, marker) in RESERVED {
        if !matches!(
            item.split_once(' '),
            Some(("method" | "notify" | "edge" | "label", _))
        ) {
            problems.push(format!(
                "`RESERVED` entry `{item}`: name it `<kind> <name>`, the kind one of method, notify, \
                 edge, or label"
            ));
        }
        if let Some(why) = malformed(marker) {
            problems.push(format!("`RESERVED` entry `{item}`: {why}"));
        }
    }
    fail_on(problems);
}

/// Part III's list of what is declared ahead of its reader, from the markers.
#[test]
#[ignore = "prints Part III's reserved list; run it with --run-ignored only --no-capture"]
fn print_the_reserved_list() {
    let members = members();
    let mut rows: Vec<(String, &str)> = members
        .iter()
        .filter_map(|m| Some((format!("crate `{}`", m.name), m.reserved_for.as_deref()?)))
        .collect();
    rows.extend(RESERVED.iter().map(|(item, marker)| {
        let (kind, name) = item.split_once(' ').unwrap_or((item, ""));
        (format!("{kind} `{name}`"), *marker)
    }));
    let row_number = |marker: &str| -> u32 {
        let digits = marker.trim_start_matches("row ");
        let end = digits
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(digits.len());
        digits[..end].parse().unwrap_or(u32::MAX)
    };
    rows.sort_by_key(|(item, marker)| (row_number(marker), item.clone()));
    println!("| Item | Row | Milestone | Its reader |");
    println!("|---|---|---|---|");
    for (item, marker) in &rows {
        let (head, reader) = marker.split_once(": ").unwrap_or((marker, ""));
        let (row, milestone) = head.rsplit_once(", ").unwrap_or((head, ""));
        println!("| {item} | {row} | {milestone} | {reader} |");
    }
    println!("\nTools, readers of their own:");
    for m in members.iter().filter(|m| m.tool.is_some()) {
        println!("- `{}`: {}", m.name, m.tool.as_deref().unwrap_or_default());
    }
}

// ---------------------------------------------------------------- reading the code

/// What the code does with each `<Type>::<Variant>` it names, each with the
/// first file that does it: builds it (a value), or reads it (a `match` arm, a
/// pattern of `if let`, `let … else`, or `matches!`, or an `==` or `!=`).
#[derive(Debug, Default)]
struct Uses {
    built: BTreeMap<String, String>,
    read: BTreeMap<String, String>,
}

/// What the code of each crate directory in `dirs`, its tests aside, does with
/// the variants of `types`. Only a file that names one of them is read as
/// tokens, so a string or a comment never counts.
fn uses_of(dirs: &[PathBuf], types: &[&str]) -> Uses {
    let root = workspace_root();
    let needles: Vec<String> = types.iter().map(|t| format!("{t}::")).collect();
    let mut uses = Uses::default();
    for dir in dirs {
        for file in code_files(dir) {
            let text = std::fs::read_to_string(&file).unwrap();
            if !needles.iter().any(|n| text.contains(n.as_str())) {
                continue;
            }
            let stream: TokenStream = text
                .parse()
                .unwrap_or_else(|e| panic!("{}: {e:?}", file.display()));
            let mut toks = Vec::new();
            flatten(stream, &mut toks);
            let at = file
                .strip_prefix(&root)
                .unwrap_or(&file)
                .display()
                .to_string();
            uses_in(&without_tests(toks), types, &at, &mut uses);
        }
    }
    uses
}

/// The `.rs` files under a crate directory's `src`, but the out-of-line modules
/// a `#[cfg(test)]` declares (`#[cfg(test)] mod tests_m3;`) and theirs.
fn code_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![dir.join("src")];
    while let Some(d) = dirs.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|x| x == "rs") {
                files.push(path);
            }
        }
    }
    let mut tests = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        for (k, line) in lines.iter().enumerate() {
            if *line != "#[cfg(test)]" {
                continue;
            }
            let Some(item) = lines[k + 1..].iter().find(|l| !l.starts_with("#[")) else {
                continue;
            };
            let declared = ["mod ", "pub mod ", "pub(crate) mod "]
                .iter()
                .find_map(|p| item.strip_prefix(p)?.strip_suffix(';'));
            let Some(name) = declared else {
                continue;
            };
            // A crate root or a `mod.rs` declares its modules beside it; `x.rs`, in `x/`.
            let stem = file.file_stem().unwrap_or_default();
            let beside = ["lib", "main", "mod"].iter().any(|s| stem == *s)
                || file.parent().is_some_and(|p| p.ends_with("bin"));
            let home = if beside {
                file.parent().unwrap_or(dir).to_path_buf()
            } else {
                file.with_extension("")
            };
            tests.push(home.join(format!("{name}.rs")));
            tests.push(home.join(name));
        }
    }
    files.retain(|f| !tests.iter().any(|t| f.starts_with(t)));
    files.sort();
    files
}

/// A source file's tokens, flat: a group's delimiters are tokens of their own.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    /// A punctuation character, and whether the next one joins it (`=` in `==`).
    Punct(char, bool),
    Open(Delimiter),
    Close(Delimiter),
    Literal,
}

fn flatten(stream: TokenStream, out: &mut Vec<Tok>) {
    for tree in stream {
        match tree {
            TokenTree::Group(g) => {
                out.push(Tok::Open(g.delimiter()));
                flatten(g.stream(), out);
                out.push(Tok::Close(g.delimiter()));
            }
            TokenTree::Ident(i) => out.push(Tok::Ident(i.to_string())),
            TokenTree::Punct(p) => out.push(Tok::Punct(p.as_char(), p.spacing() == Spacing::Joint)),
            TokenTree::Literal(_) => out.push(Tok::Literal),
        }
    }
}

/// The token that closes the group opened at `open`.
fn close_of(toks: &[Tok], open: usize) -> usize {
    let mut depth = 0;
    for (k, tok) in toks.iter().enumerate().skip(open) {
        match tok {
            Tok::Open(_) => depth += 1,
            Tok::Close(_) => {
                depth -= 1;
                if depth == 0 {
                    return k;
                }
            }
            _ => {}
        }
    }
    toks.len().saturating_sub(1)
}

/// Whether the attribute whose `[` is at `open` is `cfg(test)` or
/// `cfg(all(test, …))`.
fn is_test_cfg(toks: &[Tok], open: usize) -> bool {
    let ident = |k: usize, s: &str| matches!(toks.get(k), Some(Tok::Ident(i)) if i == s);
    let paren = |k: usize| toks.get(k) == Some(&Tok::Open(Delimiter::Parenthesis));
    ident(open + 1, "cfg")
        && paren(open + 2)
        && ((ident(open + 3, "test")
            && toks.get(open + 4) == Some(&Tok::Close(Delimiter::Parenthesis)))
            || (ident(open + 3, "all") && paren(open + 4) && ident(open + 5, "test")))
}

/// The tokens outside what `#[cfg(test)]` marks: a test module, function, or
/// impl, and the rest of a file or module marked `#![cfg(test)]`.
fn without_tests(toks: Vec<Tok>) -> Vec<Tok> {
    let mut out = Vec::with_capacity(toks.len());
    let mut i = 0;
    while i < toks.len() {
        if !matches!(toks[i], Tok::Punct('#', _)) {
            out.push(toks[i].clone());
            i += 1;
            continue;
        }
        let inner = matches!(toks.get(i + 1), Some(Tok::Punct('!', _)));
        let open = if inner { i + 2 } else { i + 1 };
        if toks.get(open) != Some(&Tok::Open(Delimiter::Bracket)) || !is_test_cfg(&toks, open) {
            out.push(toks[i].clone());
            i += 1;
            continue;
        }
        let mut k = close_of(&toks, open) + 1;
        if inner {
            // To the end of the module (or the file) it is inside.
            let mut depth = 0;
            while k < toks.len() {
                match toks[k] {
                    Tok::Open(_) => depth += 1,
                    Tok::Close(_) if depth == 0 => break,
                    Tok::Close(_) => depth -= 1,
                    _ => {}
                }
                k += 1;
            }
            i = k;
            continue;
        }
        // The item's other attributes, then the item: to its `;`, or through
        // its first `{ … }`, at its own depth.
        while matches!(toks.get(k), Some(Tok::Punct('#', _)))
            && toks.get(k + 1) == Some(&Tok::Open(Delimiter::Bracket))
        {
            k = close_of(&toks, k + 1) + 1;
        }
        let mut depth = 0;
        while k < toks.len() {
            match toks[k] {
                Tok::Open(Delimiter::Brace) if depth == 0 => {
                    k = close_of(&toks, k) + 1;
                    break;
                }
                Tok::Open(_) => depth += 1,
                Tok::Close(_) if depth == 0 => break,
                Tok::Close(_) => depth -= 1,
                Tok::Punct(';', _) if depth == 0 => {
                    k += 1;
                    break;
                }
                _ => {}
            }
            k += 1;
        }
        i = k;
    }
    out
}

/// Records each `<Type>::<Variant>` in `toks`, of `types`, as built or read.
fn uses_in(toks: &[Tok], types: &[&str], at: &str, uses: &mut Uses) {
    for (i, tok) in toks.iter().enumerate() {
        let Tok::Ident(ty) = tok else { continue };
        if !types.contains(&ty.as_str()) {
            continue;
        }
        let (Some(Tok::Punct(':', true)), Some(Tok::Punct(':', false)), Some(Tok::Ident(variant))) =
            (toks.get(i + 1), toks.get(i + 2), toks.get(i + 3))
        else {
            continue;
        };
        // `Type::Assoc::…` names no variant.
        if matches!(toks.get(i + 4), Some(Tok::Punct(':', true))) {
            continue;
        }
        let into = if is_read(toks, i, i + 3) {
            &mut uses.read
        } else {
            &mut uses.built
        };
        into.entry(format!("{ty}::{variant}"))
            .or_insert_with(|| at.to_string());
    }
}

/// Whether the `Type::Variant` whose type is at `ty` and variant at `variant`
/// is read: a pattern, a comparison, or the pattern of a `matches!`.
fn is_read(toks: &[Tok], ty: usize, variant: usize) -> bool {
    let punct = |k: usize, c: char| matches!(toks.get(k), Some(Tok::Punct(p, _)) if *p == c);
    let joint = |k: usize, c: char| matches!(toks.get(k), Some(Tok::Punct(p, true)) if *p == c);
    // Past its payload, `(…)` or `{…}`.
    let mut after = variant + 1;
    if matches!(
        toks.get(after),
        Some(Tok::Open(Delimiter::Parenthesis | Delimiter::Brace))
    ) {
        after = close_of(toks, after) + 1;
    }
    // Back over its path's start (`theseus_protocol::Event`).
    let mut start = ty;
    while start >= 3
        && toks[start - 1] == Tok::Punct(':', false)
        && toks[start - 2] == Tok::Punct(':', true)
        && matches!(toks[start - 3], Tok::Ident(_))
    {
        start -= 3;
    }
    // `== Type::Variant`, `Type::Variant != …`.
    let compared_before =
        start >= 2 && (joint(start - 2, '=') || joint(start - 2, '!')) && punct(start - 1, '=');
    let compared_after = (joint(after, '=') || joint(after, '!')) && punct(after + 1, '=');
    if compared_before || compared_after {
        return true;
    }
    // Past the patterns that enclose it (`Some(…)`, `[…]`): a match arm's `=>`,
    // an or-pattern's `|`, a guard's `if`, or a `let`'s `=`.
    let mut k = after;
    while matches!(
        toks.get(k),
        Some(Tok::Close(Delimiter::Parenthesis | Delimiter::Bracket))
    ) {
        k += 1;
    }
    if (joint(k, '=') && punct(k + 1, '>'))
        || toks.get(k) == Some(&Tok::Punct('|', false))
        || toks.get(k) == Some(&Tok::Punct('=', false))
        || matches!(toks.get(k), Some(Tok::Ident(i)) if i == "if")
    {
        return true;
    }
    // Inside a `matches!(…)`, up to the block it is in.
    let mut depth = 0;
    for k in (0..start).rev() {
        match &toks[k] {
            Tok::Close(_) => depth += 1,
            Tok::Open(_) if depth > 0 => depth -= 1,
            Tok::Open(Delimiter::Brace) => return false,
            Tok::Open(_)
                if k >= 2
                    && matches!(toks[k - 1], Tok::Punct('!', _))
                    && matches!(&toks[k - 2], Tok::Ident(i) if i == "matches") =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

/// The scan tells a build from a read, and skips what `#[cfg(test)]` marks.
#[test]
fn the_scan_tells_a_build_from_a_read_and_skips_tests() {
    let code = r#"
        fn send(sink: &Sink, q: Q) {
            sink.send(Event::Built(Built { x: 1 }));
            let m = Message::from(theseus_protocol::Event::Qualified(q));
            let s = "Event::InAString(x)"; // Event::InAComment(x)
            let names = Event::VARIANTS;
        }
        fn read(e: &Event, k: EdgeKind) {
            match e {
                Event::Arm(_) => {}
                Event::OrA(_) | Event::OrB(_) => {}
                Event::Guarded(g) if g.ok => {}
                _ => {}
            }
            if let Some(Event::IfLet(x)) = maybe {}
            let Event::LetElse(y) = e else { return };
            assert!(matches!(e, Some(Event::Matches(_))));
            if k == EdgeKind::Compared {}
            if EdgeKind::ComparedFirst != k {}
        }
        #[cfg(test)]
        mod tests {
            fn t() { sink.send(Event::InATest(1)); }
        }
        #[cfg(test)]
        #[allow(dead_code)]
        fn helper() -> Event { Event::InATestFn(2) }
        #[cfg(all(test, unix))]
        impl X { fn y() { Event::InATestImpl(3) } }
        fn after_the_tests() { sink.send(Event::AfterTheTests(4)); }
    "#;
    let mut toks = Vec::new();
    flatten(code.parse().unwrap(), &mut toks);
    let mut uses = Uses::default();
    uses_in(
        &without_tests(toks),
        &["Event", "EdgeKind"],
        "x.rs",
        &mut uses,
    );
    let keys = |m: &BTreeMap<String, String>| m.keys().cloned().collect::<Vec<_>>();
    assert_eq!(
        keys(&uses.built),
        [
            "Event::AfterTheTests",
            "Event::Built",
            "Event::Qualified",
            "Event::VARIANTS"
        ]
    );
    assert_eq!(
        keys(&uses.read),
        [
            "EdgeKind::Compared",
            "EdgeKind::ComparedFirst",
            "Event::Arm",
            "Event::Guarded",
            "Event::IfLet",
            "Event::LetElse",
            "Event::Matches",
            "Event::OrA",
            "Event::OrB",
        ]
    );
    let whole_file = "#![cfg(test)]\nfn f() { sink.send(Event::InATestFile(1)); }";
    let mut toks = Vec::new();
    flatten(whole_file.parse().unwrap(), &mut toks);
    let mut uses = Uses::default();
    uses_in(&without_tests(toks), &["Event"], "y.rs", &mut uses);
    assert!(uses.built.is_empty(), "{uses:?}");
}

/// A marker's form: a row, a milestone, and a reader.
#[test]
fn a_marker_names_its_row_its_milestone_and_its_reader() {
    assert_eq!(malformed("row 17 (17b), M4: proc.run's L1 path"), None);
    assert_eq!(malformed("row 29 (C1, 14a), AWS: aws.call"), None);
    assert_eq!(malformed("row 5, M3.6: the CLI"), None);
    for bad in [
        "M4: the wrapper",
        "row 17 (17b): the wrapper",
        "17b, M4: the wrapper",
        "row 17, M: the wrapper",
        "row 17, someday: the wrapper",
        "row 17, M4:  ",
    ] {
        assert!(malformed(bad).is_some(), "{bad:?} passed");
    }
}

/// The checks see what they must: every crate of the workspace, the 31
/// methods, the 21 notifications, and every notification's sender.
#[test]
fn the_registry_sees_the_whole_tree() {
    let members = members();
    assert!(members.len() >= 21, "{} members", members.len());
    assert!(members.iter().any(|m| m.name == "theseus-core"));
    let reached = reached(&members);
    assert_eq!(
        reached["theseus-store"].first().map(String::as_str),
        Some("theseusd"),
        "{:?}",
        reached["theseus-store"]
    );
    assert!(method::ALL.len() >= 31 && method::ALL.contains(&method::WAKE_CANCEL));
    assert!(notify::ALL.len() >= 21 && notify::ALL.contains(&notify::NARRATIVE_LINE));
    let uses = uses_of(&[PathBuf::from(env!("CARGO_MANIFEST_DIR"))], &["Event"]);
    // `turn.rs`'s, not a test's.
    assert!(
        uses.built["Event::TurnStarted"].ends_with("turn.rs"),
        "{:?}",
        uses.built
    );
}
