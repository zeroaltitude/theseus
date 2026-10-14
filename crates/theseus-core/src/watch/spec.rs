//! A watch as the record keeps it, and how one is made (theseus-f2r2b).
//!
//! No stored field was added: a watch is a pending wake with the purpose
//! `Watch` and the run `Probe(WakeProbe { kind, subject, until })`.
//!
//! - `kind`: `run`, `fetch` or `file`.
//! - `subject`: a JSON object, the check's own fields (`argv`, `url`, `path`,
//!   `json`) and `then` (`notice`, `turn`, `task` with its `brief`).
//! - `until`: the condition as typed (`equals MERGED`).
//! - The wake's `note` is the title, its `repeat` is `every` with `until_ms`
//!   the expiry, and its `due_at_ms` is the expiry too, so every list shows
//!   when a watch gives up.

use serde::{Deserialize, Serialize};
use theseus_kernel::{Every, NewWake, PendingWake, Purpose, Repeat, WakeProbe, WakeRun};
use theseus_protocol::{WatchCheck, WatchCreateParams};

use super::cond::{self, Condition};
use crate::config::WatchesConfig;

/// The longest title, in characters.
pub const MAX_TITLE: usize = 200;

/// What the subject JSON holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Subject {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    argv: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    json: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    then: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brief: Option<String>,
}

/// What a watch looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Run {
        argv: Vec<String>,
        json: Option<String>,
    },
    Fetch {
        url: String,
        json: Option<String>,
    },
    File {
        path: String,
    },
}

impl Check {
    pub fn kind(&self) -> &'static str {
        match self {
            Check::Run { .. } => "run",
            Check::Fetch { .. } => "fetch",
            Check::File { .. } => "file",
        }
    }

    /// The command, URL or path as people read it.
    pub fn target(&self) -> String {
        match self {
            Check::Run { argv, .. } => argv
                .iter()
                .map(|a| {
                    if a.is_empty() || a.contains(char::is_whitespace) {
                        format!("'{a}'")
                    } else {
                        a.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
            Check::Fetch { url, .. } => url.clone(),
            Check::File { path } => path.clone(),
        }
    }

    pub fn json(&self) -> Option<&str> {
        match self {
            Check::Run { json, .. } | Check::Fetch { json, .. } => json.as_deref(),
            Check::File { .. } => None,
        }
    }
}

/// What happens when it is met.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Then {
    Notice,
    Turn,
    Task(String),
}

impl Then {
    pub fn word(&self) -> &'static str {
        match self {
            Then::Notice => "notice",
            Then::Turn => "turn",
            Then::Task(_) => "task",
        }
    }
}

/// A watch, read from its wake.
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub title: String,
    pub check: Check,
    pub then: Then,
    pub until: String,
    pub cond: Condition,
    pub every_ms: u64,
    pub expires_ms: u64,
}

impl Spec {
    /// The watch a wake is, or why its record does not read.
    pub fn of(w: &PendingWake) -> Result<Spec, String> {
        let p = w.probe().ok_or("not a watch")?;
        let s: Subject = serde_json::from_str(&p.subject)
            .map_err(|e| format!("a watch's subject does not read: {e}"))?;
        let check = check_of(&p.kind, &s)?;
        let then = then_of(&s.then, s.brief.as_deref())?;
        let r = w.repeat.as_ref().ok_or("a watch has no interval")?;
        Ok(Spec {
            title: w.note.clone(),
            cond: cond::parse(&p.until)?,
            until: p.until.clone(),
            check,
            then,
            every_ms: r.every.nominal_ms(),
            expires_ms: r.until_ms.unwrap_or(w.due_at_ms),
        })
    }
}

fn check_of(kind: &str, s: &Subject) -> Result<Check, String> {
    match kind {
        "run" => Ok(Check::Run {
            argv: s
                .argv
                .clone()
                .filter(|a| !a.is_empty())
                .ok_or("no command")?,
            json: s.json.clone(),
        }),
        "fetch" => Ok(Check::Fetch {
            url: s.url.clone().ok_or("no URL")?,
            json: s.json.clone(),
        }),
        "file" => Ok(Check::File {
            path: s.path.clone().ok_or("no path")?,
        }),
        k => Err(format!("`{k}` is not a kind of check")),
    }
}

fn then_of(then: &str, brief: Option<&str>) -> Result<Then, String> {
    match then {
        "" | "notice" => Ok(Then::Notice),
        "turn" => Ok(Then::Turn),
        "task" => match brief.map(str::trim).filter(|b| !b.is_empty()) {
            Some(b) => Ok(Then::Task(b.to_string())),
            None => Err("`then` task needs a `brief`: what the task is to do".into()),
        },
        t => Err(format!("`then` is `notice`, `turn` or `task`, not `{t}`")),
    }
}

/// `5m`, `2h`, `1d`: a span as `every` writes it.
pub fn every_word(ms: u64) -> String {
    let (n, u) = match ms {
        m if m % 86_400_000 == 0 => (m / 86_400_000, "d"),
        m if m % 3_600_000 == 0 => (m / 3_600_000, "h"),
        m => (m.div_ceil(60_000), "m"),
    };
    format!("{n}{u}")
}

/// The kind and the subject a check is stored as, or why it is not one.
fn subject_of(check: &WatchCheck) -> Result<(&'static str, Subject), String> {
    Ok(match check {
        WatchCheck::Run { argv, json } => {
            if argv.first().is_none_or(|a| a.trim().is_empty()) {
                return Err("a `run` check needs a command: its argv, such as [\"gh\", \"pr\", \"view\", \"123\"]".into());
            }
            (
                "run",
                Subject {
                    argv: Some(argv.clone()),
                    json: json.clone(),
                    ..Default::default()
                },
            )
        }
        WatchCheck::Fetch { url, json } => {
            let u = crate::web::fetch::web_url(url)?;
            (
                "fetch",
                Subject {
                    url: Some(u.to_string()),
                    json: json.clone(),
                    ..Default::default()
                },
            )
        }
        WatchCheck::File { path } => {
            if !path.starts_with('/') {
                return Err(format!(
                    "a `file` check needs an absolute path, not `{path}`"
                ));
            }
            (
                "file",
                Subject {
                    path: Some(path.clone()),
                    ..Default::default()
                },
            )
        }
    })
}

/// A watch made: the wake to add, and its spec.
#[derive(Debug, Clone)]
pub struct Built {
    pub new: NewWake,
    pub spec: Spec,
}

/// `p` as a watch to add at `now_ms`, or why it cannot be. `target` is where
/// the conversation posts, for the wake's record.
pub fn build(
    p: &WatchCreateParams,
    now_ms: u64,
    cfg: &WatchesConfig,
    target: Option<String>,
) -> Result<Built, String> {
    let title = p.title.trim();
    if title.is_empty() {
        return Err(
            "a watch needs a title: what it waits for, as a question (`PR 123 merged?`)".into(),
        );
    }
    let n = title.chars().count();
    if n > MAX_TITLE {
        return Err(format!(
            "the title is {n} characters, over the {MAX_TITLE} a watch takes"
        ));
    }
    let (_, min_ms) = cfg.spans_ms().map_err(|e| e.to_string())?;
    let every = match p.every.as_deref() {
        Some(e) => Every::parse(e)?,
        None => Every::parse(&cfg.default_every)?,
    };
    if every.nominal_ms() < min_ms {
        return Err(format!(
            "a watch checks every {} at the most often, not every {every}",
            cfg.min_every
        ));
    }
    let max_ms = cfg.max_days.saturating_mul(86_400_000);
    let for_ms = match p.for_at_most.as_deref() {
        Some(f) => {
            crate::wake::parse_after(f).map_err(|e| e.replace("`after`", "`for_at_most`"))?
        }
        None => max_ms,
    };
    if for_ms > max_ms {
        return Err(format!(
            "a watch waits {} days at the most ([watches] max_days), not {}",
            cfg.max_days,
            crate::wake::span(for_ms)
        ));
    }
    if for_ms < every.nominal_ms() {
        return Err(format!(
            "`for_at_most` is shorter than the interval ({every}): it would give up before its first look"
        ));
    }
    let cond = cond::parse(&p.until)?;
    let (kind, mut subject) = subject_of(&p.check)?;
    if cond.reads_exit() && kind != "run" {
        return Err("`exit` is a command's status: it belongs to a `run` check".into());
    }
    if let Some(j) = subject.json.as_deref() {
        if j.trim().is_empty()
            || !(j.trim_start().starts_with('.') || j.trim_start().starts_with('$'))
        {
            return Err(format!("`json` is a path such as `.state`, not `{j}`"));
        }
    }
    let (then_kind, brief) = match &p.then {
        Some(t) => (t.kind.as_str(), t.brief.as_deref()),
        None => ("", None),
    };
    let then = then_of(then_kind, brief)?;
    subject.then = then.word().to_string();
    if let Then::Task(b) = &then {
        subject.brief = Some(b.clone());
    }
    let expires = now_ms.saturating_add(for_ms);
    let new = NewWake {
        due_at_ms: expires,
        note: title.to_string(),
        target,
        repeat: Some(Repeat {
            every,
            first_ms: now_ms,
            days: vec![],
            until_ms: Some(expires),
        }),
        purpose: Purpose::Watch,
        run: WakeRun::Probe(WakeProbe {
            kind: kind.to_string(),
            subject: serde_json::to_string(&subject).map_err(|e| e.to_string())?,
            until: p.until.trim().to_string(),
        }),
    };
    let spec = Spec {
        title: title.to_string(),
        check: check_of(kind, &subject)?,
        then,
        until: p.until.trim().to_string(),
        cond,
        every_ms: every.nominal_ms(),
        expires_ms: expires,
    };
    Ok(Built { new, spec })
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::WatchThen;

    fn params(check: WatchCheck, until: &str) -> WatchCreateParams {
        WatchCreateParams {
            title: "PR 123 merged?".into(),
            check,
            until: until.into(),
            every: None,
            for_at_most: None,
            then: None,
            session_id: None,
            author: None,
            discord: None,
        }
    }

    fn run() -> WatchCheck {
        WatchCheck::Run {
            argv: vec!["gh".into(), "pr".into(), "view".into(), "123".into()],
            json: None,
        }
    }

    /// A made watch reads back as itself from its wake's fields.
    #[test]
    fn a_watch_survives_its_record() {
        let cfg = WatchesConfig::default();
        let mut p = params(run(), "equals MERGED");
        p.every = Some("10m".into());
        p.for_at_most = Some("3d".into());
        p.then = Some(WatchThen {
            kind: "task".into(),
            brief: Some("deploy it".into()),
        });
        let b = build(&p, 1_000, &cfg, None).unwrap();
        let w = PendingWake {
            id: "wak_x".into(),
            due_at_ms: b.new.due_at_ms,
            note: b.new.note.clone(),
            set_at_ms: 1_000,
            by: "t".into(),
            target: None,
            repeat: b.new.repeat.clone(),
            occurrence: 1,
            purpose: b.new.purpose,
            run: b.new.run.clone(),
            paused_at_ms: None,
        };
        let s = Spec::of(&w).unwrap();
        assert_eq!(s, b.spec);
        assert_eq!(s.every_ms, 600_000);
        assert_eq!(s.expires_ms, 1_000 + 3 * 86_400_000);
        assert_eq!(s.then, Then::Task("deploy it".into()));
        assert_eq!(s.check.target(), "gh pr view 123");
    }

    #[test]
    fn the_bounds_hold_with_words() {
        let cfg = WatchesConfig::default();
        let at = |f: &dyn Fn(&mut WatchCreateParams)| {
            let mut p = params(run(), "equals MERGED");
            f(&mut p);
            build(&p, 0, &cfg, None).map(|_| ()).unwrap_err()
        };
        let mut p = params(run(), "equals X");
        p.every = Some("1m".into());
        assert!(build(&p, 0, &cfg, None).is_ok(), "a minute is the floor");
        assert!(at(&|p| p.for_at_most = Some("31d".into())).contains("30 days"));
        assert!(at(&|p| p.title = "  ".into()).contains("needs a title"));
        assert!(at(&|p| p.until = "soon".into()).contains("not a condition"));
        assert!(at(&|p| p.then = Some(WatchThen {
            kind: "task".into(),
            brief: None
        }))
        .contains("brief"));
        assert!(at(&|p| p.then = Some(WatchThen {
            kind: "page".into(),
            brief: None
        }))
        .contains("`notice`"));
        assert!(at(&|p| p.check = WatchCheck::File { path: "rel".into() }).contains("absolute"));
        assert!(at(&|p| p.check = WatchCheck::Fetch {
            url: "ftp://x".into(),
            json: None
        })
        .contains("http"));
        assert!(at(&|p| {
            p.check = WatchCheck::File { path: "/x".into() };
            p.until = "exit 0".into()
        })
        .contains("`exit`"));
        assert!(at(&|p| p.check = WatchCheck::Run {
            argv: vec![],
            json: None
        })
        .contains("needs a command"));
        assert!(at(&|p| p.check = WatchCheck::Run {
            argv: vec!["a".into()],
            json: Some("state".into())
        })
        .contains("path"));
        let fast = WatchesConfig {
            min_every: "5m".into(),
            default_every: "10m".into(),
            ..Default::default()
        };
        let mut p = params(run(), "equals X");
        p.every = Some("2m".into());
        assert!(build(&p, 0, &fast, None)
            .unwrap_err()
            .contains("at the most often"));
    }
}
