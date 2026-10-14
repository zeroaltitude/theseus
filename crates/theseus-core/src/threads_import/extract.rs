//! The working set a stream's text names (theseus-71ib): repos, pull-request
//! and issue and Beads ids, branches, files and hosts, found by rules, never
//! by a model. A token is read whole between whitespace, with the punctuation
//! of prose trimmed off its ends; each kind has one shape (below), so the
//! same text always gives the same set. Each list keeps its [`KEEP`] most
//! frequent entries (a tie by name), because a working set rides prompts
//! later.
//!
//! - **repos**: `owner/name` from a github.com URL, an ssh remote
//!   (`git@github.com:owner/name.git`), or the word after `-R`, `--repo`,
//!   `repo` or `clone`. A plain `a/b` is a path, not a repo.
//! - **refs**: `#123`, a github issue or pull URL (`owner/name#123`), and a
//!   Beads id: a lower-case prefix of 3 to 10 letters, a hyphen and 3 to 6
//!   letters or digits, with at least one digit, or any suffix after the
//!   prefix `theseus`.
//! - **branches**: `feat/…`, `fix/…`, `feature/…`, `lane/…`, `cloud/…`,
//!   `release/…`, `hotfix/…`, `chore/…`, `self/…`, and the word after `branch`
//!   or `checkout -b`/`switch -c`.
//! - **files**: a token with a `/` whose last part has an extension, or a bare
//!   name with a code extension (`rs`, `py`, `ts`, `tsx`, `js`, `toml`, `md`,
//!   `json`, `yaml`, `yml`, `sh`).
//! - **hosts**: the host of any other URL.

use std::collections::BTreeMap;

use theseus_protocol::threads::WorkingSet;

/// The most entries of one kind a working set keeps.
pub const KEEP: usize = 8;

const BRANCH_KINDS: &[&str] = &[
    "feat", "fix", "feature", "lane", "cloud", "release", "hotfix", "chore", "self",
];
const CODE_EXT: &[&str] = &[
    "rs", "py", "ts", "tsx", "js", "toml", "md", "json", "yaml", "yml", "sh",
];
const PREFIXES: &[&str] = &["theseus"];

#[derive(Default)]
struct Counts {
    repos: BTreeMap<String, u32>,
    refs: BTreeMap<String, u32>,
    branches: BTreeMap<String, u32>,
    files: BTreeMap<String, u32>,
    hosts: BTreeMap<String, u32>,
}

fn bump(m: &mut BTreeMap<String, u32>, k: String) {
    *m.entry(k).or_default() += 1;
}

/// The working set the `texts` name.
pub fn working_set<'a>(texts: impl IntoIterator<Item = &'a str>) -> WorkingSet {
    let mut c = Counts::default();
    for text in texts {
        scan(text, &mut c);
    }
    WorkingSet {
        roots: Vec::new(),
        repos: top(c.repos),
        branches: top(c.branches),
        refs: top(c.refs),
        files: top(c.files),
        hosts: top(c.hosts),
    }
}

fn top(m: BTreeMap<String, u32>) -> Vec<String> {
    let mut v: Vec<(String, u32)> = m.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.into_iter().take(KEEP).map(|(k, _)| k).collect()
}

fn trim(tok: &str) -> &str {
    tok.trim_matches(|c: char| "()[]<>{}\"'`,;:!?*".contains(c))
        .trim_end_matches('.')
}

fn name_ok(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        && s.chars().any(|c| c.is_ascii_alphanumeric())
}

fn scan(text: &str, c: &mut Counts) {
    let toks: Vec<&str> = text.split_whitespace().collect();
    for (i, raw) in toks.iter().enumerate() {
        let tok = trim(raw);
        if tok.is_empty() {
            continue;
        }
        let prev = i.checked_sub(1).map(|p| trim(toks[p]).to_lowercase());
        let prev2 = i
            .checked_sub(2)
            .map(|p| trim(toks[p]).to_lowercase())
            .unwrap_or_default();
        if tok.starts_with("http://") || tok.starts_with("https://") {
            url(tok, c);
        } else if let Some(r) = tok.strip_prefix("git@github.com:") {
            repo(r.trim_end_matches(".git"), c);
        } else if let Some(n) = tok.strip_prefix('#').filter(|n| number(n)) {
            bump(&mut c.refs, format!("#{n}"));
        } else if bead(tok) {
            bump(&mut c.refs, tok.to_string());
        } else if branch_like(tok) {
            bump(&mut c.branches, tok.to_string());
        } else if matches!(prev.as_deref(), Some("-r" | "--repo" | "repo" | "clone")) {
            repo(tok, c);
        } else if matches!(prev.as_deref(), Some("branch"))
            || (matches!(prev.as_deref(), Some("-b" | "-c"))
                && matches!(prev2.as_str(), "checkout" | "switch"))
        {
            if name_ok(&tok.replace('/', "-")) && tok.len() <= 80 {
                bump(&mut c.branches, tok.to_string());
            }
        } else if file_like(tok) {
            bump(&mut c.files, tok.to_string());
        }
    }
}

fn number(s: &str) -> bool {
    !s.is_empty() && s.len() <= 7 && s.bytes().all(|b| b.is_ascii_digit())
}

fn repo(s: &str, c: &mut Counts) {
    let mut parts = s.split('/');
    if let (Some(o), Some(n), None) = (parts.next(), parts.next(), parts.next()) {
        if name_ok(o) && name_ok(n) {
            bump(&mut c.repos, format!("{o}/{n}"));
        }
    }
}

fn url(tok: &str, c: &mut Counts) {
    let rest = tok.split_once("://").map_or(tok, |(_, r)| r);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host
        .rsplit('@')
        .next()
        .unwrap_or(host)
        .split(':')
        .next()
        .unwrap_or(host)
        .to_lowercase();
    if !host.contains('.') || !name_ok(&host) {
        return;
    }
    if host == "github.com" || host == "www.github.com" {
        let segs: Vec<&str> = path
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if let [o, n, rest @ ..] = segs.as_slice() {
            if name_ok(o) && name_ok(n) {
                let full = format!("{o}/{}", n.trim_end_matches(".git"));
                bump(&mut c.repos, full.clone());
                if let [kind, num, ..] = rest {
                    if matches!(*kind, "pull" | "issues") && number(num) {
                        bump(&mut c.refs, format!("{full}#{num}"));
                    }
                }
            }
        }
    } else {
        bump(&mut c.hosts, host);
    }
}

fn bead(t: &str) -> bool {
    let Some((prefix, rest)) = t.split_once('-') else {
        return false;
    };
    let suffix = rest
        .split_once('.')
        .map_or(rest, |(s, n)| if number(n) { s } else { "" });
    let lower = |s: &str| s.bytes().all(|b| b.is_ascii_lowercase());
    (3..=10).contains(&prefix.len())
        && lower(prefix)
        && (3..=6).contains(&suffix.len())
        && suffix
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && (PREFIXES.contains(&prefix) || suffix.bytes().any(|b| b.is_ascii_digit()))
}

fn branch_like(t: &str) -> bool {
    match t.split_once('/') {
        Some((k, rest)) => {
            BRANCH_KINDS.contains(&k)
                && !rest.is_empty()
                && rest.len() <= 80
                && rest
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c))
                && !file_like(t)
        }
        None => false,
    }
}

fn file_like(t: &str) -> bool {
    if t.starts_with('-') || t.contains("://") || t.len() > 120 {
        return false;
    }
    let Some(last) = t.rsplit('/').next() else {
        return false;
    };
    let Some((stem, ext)) = last.rsplit_once('.') else {
        return false;
    };
    let ext_ok = (1..=5).contains(&ext.len())
        && ext.bytes().all(|b| b.is_ascii_alphanumeric())
        && !ext.bytes().all(|b| b.is_ascii_digit());
    let body_ok = t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c));
    ext_ok
        && body_ok
        && !stem.is_empty()
        && if t.contains('/') {
            true
        } else {
            CODE_EXT.contains(&ext.to_ascii_lowercase().as_str())
        }
}
