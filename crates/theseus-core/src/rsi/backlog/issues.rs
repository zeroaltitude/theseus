//! The repository's own open GitHub issues as a second backlog source
//! (theseus-dl4pu; the owner, 2026-11-12: *"add a 1.0.6 task to have theseus
//! look to theseus github issues for open theseus ideas"*, under his rule
//! that after 1.0.6 planning moves to GitHub as **one issue per bucket**,
//! each tagged with the version it is planned for).
//!
//! The night's other sources read Theseus's own record: the ledger, health,
//! the pattern miner. This one reads work the owner and his collaborators
//! wrote down somewhere else, so three things hold, and each has a test:
//!
//! 1. **An item names the issue it came from** — its number and its title —
//!    so a person reading `theseus self backlog` can go and look. The number
//!    is the item's stable key (`github:issue:<N>`), so the same issue is one
//!    cause night after night, its new text joining the item it already has.
//! 2. **The read is READ-ONLY.** [`Verb`] has exactly one variant, `Get`, and
//!    [`Request`] is the only thing in this crate that describes a call to
//!    GitHub. Nothing here opens, closes, comments on or labels an issue, and
//!    `no_write_verb_can_be_constructed` walks the type to say so.
//! 3. **A source that FAILS must not look like a source that is EMPTY.**
//!    [`read`] answers `Result`, and the night keeps the observations it
//!    already had whatever it answers ([`crate::Core::issue_obs`]). Silent
//!    emptiness is how a backlog quietly stops containing half the work.
//!
//! **Issue text is text other people write**, so it is an injection surface
//! like any fetched page. Nothing read here is treated as an instruction: the
//! title and the body become an item's words, which reach a model only as the
//! objective of a task the owner can see and drop, and Jev classifies that
//! content exactly as it classifies any other outside text. The source is
//! **off by default** (`[self_issues] enabled`).
//!
//! **What this row does NOT build: the live read.** A read of the real
//! repository needs a token and the owner's own machine, and the card put it
//! outside its bar in as many words. So [`Request`] is built and validated
//! here and **nothing sends it**; the bytes come from the file
//! `[self_issues] file` names, which is what `gh issue list --json` or any
//! other fetch writes. Wiring the request to the web tools is one step, and
//! it is the step that needs the owner's token.

use serde::Deserialize;

use super::read::Obs;

/// The one verb this source has. There is no `Post`, `Patch` or `Delete`
/// variant, and adding one is the change a reviewer should refuse: the card's
/// rule is that nothing here writes to an issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Get,
}

impl Verb {
    /// The HTTP method, for a sender that does not exist yet.
    pub fn method(self) -> &'static str {
        match self {
            Verb::Get => "GET",
        }
    }
}

/// A read of one repository's open issues, validated before it could be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub verb: Verb,
    pub owner: String,
    pub repo: String,
    pub per_page: usize,
}

/// `owner/name` split, or `None` when the string is not that shape. One
/// path component each, no spaces, no slashes beyond the one.
pub fn repo_parts(repo: &str) -> Option<(&str, &str)> {
    let (owner, name) = repo.trim().split_once('/')?;
    let ok = |s: &str| {
        !s.is_empty()
            && s.len() <= 100
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    };
    (ok(owner) && ok(name)).then_some((owner, name))
}

impl Request {
    /// The read for `repo`, or why it is not a repository. Plan-time
    /// validation, as the AWS surface does it: a misshapen name is said
    /// before anything is built, never sent and found out later.
    pub fn open_issues(repo: &str, per_page: usize) -> Result<Self, String> {
        let (owner, name) = repo_parts(repo)
            .ok_or_else(|| format!("a repository is owner/name, not {:?}", repo.trim()))?;
        Ok(Self {
            verb: Verb::Get,
            owner: owner.to_string(),
            repo: name.to_string(),
            per_page: per_page.clamp(1, 100),
        })
    }

    /// The path and query a sender would use. Read-only by construction:
    /// `state=open` and nothing that changes anything.
    pub fn path(&self) -> String {
        format!(
            "/repos/{}/{}/issues?state=open&per_page={}",
            self.owner, self.repo, self.per_page
        )
    }
}

/// One open issue, as much of GitHub's JSON as a backlog item needs.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Issue {
    pub number: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub labels: Vec<Label>,
    /// Present on a pull request and absent on an issue: GitHub's issues
    /// endpoint returns both, and a pull request is not a bucket of work.
    #[serde(default)]
    pub pull_request: Option<serde_json::Value>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// A label, as an object with a name or as a bare string (the search
/// endpoint and `gh --json labels` differ).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Label {
    Name(String),
    Object {
        #[serde(default)]
        name: String,
    },
}

impl Label {
    pub fn name(&self) -> &str {
        match self {
            Label::Name(s) => s,
            Label::Object { name } => name,
        }
    }
}

impl Issue {
    /// Whether this is an open issue (not a pull request, not closed).
    pub fn is_open_issue(&self) -> bool {
        self.pull_request.is_none()
            && self.number > 0
            && self.state.as_deref().unwrap_or("open") == "open"
    }

    fn has_label(&self, want: &str) -> bool {
        self.labels
            .iter()
            .any(|l| l.name().eq_ignore_ascii_case(want))
    }
}

/// The issues of `bytes`, which is either a JSON array of issues or an object
/// holding one under `items` (the search endpoint's shape).
///
/// An error here is a **failed source**, never an empty one: the caller keeps
/// the night's other observations and says this one did not read.
pub fn parse(bytes: &[u8]) -> Result<Vec<Issue>, String> {
    let v: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("the issue JSON did not parse: {e}"))?;
    let arr = match &v {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(o) => match o.get("items") {
            Some(serde_json::Value::Array(a)) => a.clone(),
            _ => {
                return Err(
                    "the issue JSON is an object with no `items` array: it is not a listing".into(),
                )
            }
        },
        _ => return Err("the issue JSON is neither a listing nor an object".into()),
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, item) in arr.into_iter().enumerate() {
        let issue: Issue = serde_json::from_value(item)
            .map_err(|e| format!("issue {} of the listing did not parse: {e}", i + 1))?;
        out.push(issue);
    }
    Ok(out)
}

/// The cause key of an issue. The **number alone**, never the title: the
/// keel mark is read from a key by word (`super::keel::class_of`), so a title
/// holding "policy" or "secret" must not decide an item's class, and the same
/// issue must be one cause however its title is edited.
pub fn key_of(number: u64) -> String {
    format!("github:issue:{number}")
}

/// The source name every one of these observations carries, so a reader of
/// the backlog can tell where an item came from at a glance.
pub const SOURCE: &str = "github";

/// How much work an issue is taken to be. Every issue is the same: nothing
/// here guesses from its words, which is the rule the keel mark follows too.
const WORK: u8 = 3;

/// The issues of one read as the night's observations, at most `max`, the most
/// recently updated first.
///
/// `version_label`, when it is not empty, keeps only the issues carrying it:
/// the owner's rule is one issue per bucket, each tagged with the version it
/// is planned for, so a 1.0.7 issue is not this release's work.
pub fn obs(issues: &[Issue], repo: &str, version_label: &str, max: usize, now_ms: u64) -> Vec<Obs> {
    let mut kept: Vec<&Issue> = issues
        .iter()
        .filter(|i| i.is_open_issue())
        .filter(|i| version_label.is_empty() || i.has_label(version_label))
        .collect();
    // Newest first by GitHub's own `updated_at`, which sorts as a string
    // because it is RFC 3339 in UTC; an issue without one sorts last.
    kept.sort_by(|a, b| {
        b.updated_at
            .as_deref()
            .unwrap_or("")
            .cmp(a.updated_at.as_deref().unwrap_or(""))
            .then(a.number.cmp(&b.number))
    });
    kept.truncate(max);
    kept.into_iter()
        .map(|i| {
            let title = i.title.trim();
            let title = match title.is_empty() {
                true => format!("issue #{} of {repo}", i.number),
                false => format!("#{}: {title}", i.number),
            };
            Obs {
                key: key_of(i.number),
                title,
                source: SOURCE.into(),
                // The origin, recorded: a person reading the backlog has the
                // repository and the number, which is the whole of what it
                // takes to go and look. The issue's own `updated_at` is on
                // the end because evidence is deduplicated by this identity
                // (`backlog::Work::add`): an unchanged issue read a second
                // night adds nothing, and an EDITED one is new evidence on
                // the item it already has.
                id: match i.updated_at.as_deref() {
                    Some(at) => format!("github:{repo}#{}@{at}", i.number),
                    None => format!("github:{repo}#{}", i.number),
                },
                note: note_of(i, repo),
                at_ms: now_ms,
                usd: 0.0,
                person: true,
                task_failed: false,
                work: WORK,
                graduate: None,
                pattern: None,
            }
        })
        .collect()
}

/// The longest first line of an issue body an observation carries. An issue
/// is a bucket of work, not a brief: the item's words point at the issue, and
/// a reader opens it.
const NOTE_BODY_CHARS: usize = 240;

fn note_of(i: &Issue, repo: &str) -> String {
    let mut note = format!("{repo} issue #{}", i.number);
    let labels: Vec<&str> = i
        .labels
        .iter()
        .map(Label::name)
        .filter(|s| !s.is_empty())
        .collect();
    if !labels.is_empty() {
        note.push_str(&format!(" [{}]", labels.join(", ")));
    }
    let first = i
        .body
        .as_deref()
        .unwrap_or("")
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if !first.is_empty() {
        note.push_str(": ");
        match first.chars().count() > NOTE_BODY_CHARS {
            true => {
                note.extend(first.chars().take(NOTE_BODY_CHARS));
                note.push('…');
            }
            false => note.push_str(first),
        }
    }
    note
}

/// The bytes `[self_issues] file` names, read as one file.
///
/// Separate from [`parse`] so the failure of each is its own: a file that is
/// not there and a file that is not a listing are different things to say.
pub fn read(path: &std::path::Path) -> Result<Vec<Issue>, String> {
    let bytes = std::fs::read(path).map_err(|e| {
        format!(
            "the issue JSON at {} could not be read: {e}",
            path.display()
        )
    })?;
    parse(&bytes)
}

impl crate::Core {
    /// The night's observations from the repository's open issues, and **what
    /// to say when there are none**. `Vec::new()` with a `Some(why)` is a
    /// source that FAILED; `Vec::new()` with `None` is a source that read and
    /// found nothing. The night keeps every other observation either way
    /// (`Core::backlog_night`): that distinction is the card's second
    /// non-negotiable, and collapsing it is how a backlog quietly stops
    /// holding half the work.
    pub(crate) fn issue_obs(&self, now_ms: u64) -> (Vec<Obs>, Option<String>) {
        let cfg = &self.cfg.self_issues;
        if let Some(why) = cfg.refusal() {
            // Off, or named nothing to read: not a failure, and not news.
            // `enabled = false` is the default, so this is the quiet path.
            return (Vec::new(), (cfg.enabled).then_some(why));
        }
        // Validated before anything is read, and never sent: the read-only
        // request is the record of what a live read would ask for.
        let request = match Request::open_issues(&cfg.repo, cfg.max) {
            Ok(r) => r,
            Err(why) => return (Vec::new(), Some(why)),
        };
        debug_assert_eq!(request.verb, Verb::Get);
        // `~` and `$VAR` as every other path key in the config reads them.
        let path = crate::config::expand(cfg.file.trim());
        let issues = match theseus_store::blocking(|| read(&path)) {
            Ok(issues) => issues,
            Err(why) => {
                tracing::warn!(
                    repo = %cfg.repo,
                    %why,
                    "the self backlog's GitHub issue source did not read; the rest of the backlog is unchanged"
                );
                return (Vec::new(), Some(why));
            }
        };
        let out = obs(&issues, &cfg.repo, &cfg.version_label, cfg.max, now_ms);
        tracing::info!(
            repo = %cfg.repo,
            read = issues.len(),
            kept = out.len(),
            "the self backlog read the repository's open issues"
        );
        (out, None)
    }
}

#[cfg(test)]
mod tests;
