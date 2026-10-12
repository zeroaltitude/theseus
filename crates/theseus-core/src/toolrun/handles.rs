//! A job's short id (theseus-n8gk): `j1`, `j2`, … in the order a session's
//! jobs went on in the background, each mapped to its job's correlation id.
//! The record is the job's placeholder result, whose meta names it (`job`);
//! the map is that record read once a session per daemon life, then kept up
//! as jobs go on. No record of its own.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::node::{Body, ResultStatus};
use crate::store::Store;

/// The meta key a job's placeholder result names its short id by.
pub const KEY: &str = "job";

/// Each session's jobs, by short id: `ids[n - 1]` is `jn`'s.
#[derive(Default)]
pub struct Handles {
    sessions: Mutex<HashMap<String, Vec<Entry>>>,
}

/// A job a short id names: its correlation id, and its program's argv.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    pub corr: String,
    pub argv: Vec<String>,
}

/// `jn`'s number, from its name.
fn number(name: &str) -> Option<usize> {
    let n: usize = name.strip_prefix('j')?.parse().ok()?;
    (n > 0).then_some(n)
}

impl Handles {
    /// Under the session's map, read from its transcript the first time.
    fn with<T>(&self, store: &Store, session: &str, f: impl FnOnce(&mut Vec<Entry>) -> T) -> T {
        let mut sessions = self.sessions.lock().unwrap();
        if !sessions.contains_key(session) {
            let ids = read(store, session);
            sessions.insert(session.to_string(), ids);
        }
        f(sessions.get_mut(session).expect("inserted"))
    }

    /// The short id of `corr`, a job of `session`'s going on in the
    /// background: the one it has, else the next.
    pub fn assign(&self, store: &Store, session: &str, corr: &str, argv: &[String]) -> String {
        self.with(store, session, |ids| {
            let i = match ids.iter().position(|e| e.corr == corr) {
                Some(i) => i,
                None => {
                    ids.push(Entry {
                        corr: corr.to_string(),
                        argv: argv.to_vec(),
                    });
                    ids.len() - 1
                }
            };
            format!("j{}", i + 1)
        })
    }

    /// The job a short id of `session`'s names.
    pub fn resolve(&self, store: &Store, session: &str, name: &str) -> Option<Entry> {
        let n = number(name)?;
        self.with(store, session, |ids| {
            ids.get(n - 1).filter(|e| !e.corr.is_empty()).cloned()
        })
    }

    /// The short id of `corr`, when `session` gave it one, and its entry.
    pub fn name_of(&self, store: &Store, session: &str, corr: &str) -> Option<(String, Entry)> {
        self.with(store, session, |ids| {
            ids.iter()
                .position(|e| e.corr == corr)
                .map(|i| (format!("j{}", i + 1), ids[i].clone()))
        })
    }

    /// Every job `session` gave a short id: its name and entry.
    pub fn of_session(&self, store: &Store, session: &str) -> Vec<(String, Entry)> {
        self.with(store, session, |ids| {
            ids.iter()
                .enumerate()
                .filter(|(_, e)| !e.corr.is_empty())
                .map(|(i, e)| (format!("j{}", i + 1), e.clone()))
                .collect()
        })
    }
}

/// A session's short ids, from its jobs' placeholder results: its tool
/// results alone are read, once a session per daemon life, at its first job
/// that goes on in the background or its first job tool's call. Their argv
/// is left for the job tools to read when they need it. A transcript that
/// cannot be read gives none: the long ids still work.
fn read(store: &Store, session: &str) -> Vec<Entry> {
    let Ok(nodes) = store.transcript(session) else {
        return Vec::new();
    };
    let mut ids: Vec<Entry> = Vec::new();
    for (_, n) in nodes
        .iter()
        .filter(|(_, n)| n.kind == crate::stub::Kind::ToolResult)
    {
        let Body::ToolResult {
            status: ResultStatus::Background,
            correlation_id: Some(c),
            late: false,
            meta,
            ..
        } = &n.body
        else {
            continue;
        };
        let Some(k) = meta.get(KEY).and_then(|v| v.as_str()).and_then(number) else {
            continue;
        };
        if ids.len() < k {
            ids.resize(k, Entry::default());
        }
        ids[k - 1] = Entry {
            corr: c.clone(),
            argv: Vec::new(),
        };
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_id_is_j_and_a_positive_number() {
        assert_eq!(number("j7"), Some(7));
        assert_eq!(number("j0"), None);
        assert_eq!(number("7"), None);
        assert_eq!(number("act_01"), None);
        assert_eq!(number("j-1"), None);
    }
}
