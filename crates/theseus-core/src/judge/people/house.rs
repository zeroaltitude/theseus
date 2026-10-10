//! The owner's agents as the store knows them (theseus-0p1r), so they are
//! never proposed as people with no list made by hand: the backfill's first
//! run proposed his agents' display names and the judge's own name, and
//! `people.v1`'s `real` Noul passed them.
//!
//! - **Every imported episode's agent**: the source's agent id (`main`, a
//!   named agent's id), as `import.sessions` lists it.
//! - **Each agent's display name**, from its imported identity file: an
//!   episode of place kind `file` whose file is `IDENTITY.md` holds the
//!   agent's `- **Name:** …` line, the field OpenClaw itself reads the
//!   agent's name from.
//!
//! Read from the import catalog (`import/catalog.rs`, `import.sessions`'
//! projection) and the identity files' nodes, and kept until the import
//! changes (the catalog's version), so a live point reads it once. The
//! house's own names and the config's profiles are [`super::NotPeople::of`]'s.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use anyhow::Result;

use super::{fold, Line, NotPeople};
use crate::node::Body;
use crate::rpc::Core;

/// The names, at the catalog's version.
#[derive(Default)]
pub struct Kept(Mutex<Option<(String, Arc<HashSet<String>>)>>);

/// Whether a file's path names an identity file.
fn is_identity(path: &str) -> bool {
    path.rsplit(['/', '\\'])
        .next()
        .is_some_and(|f| f.eq_ignore_ascii_case("IDENTITY.md"))
}

/// The `Name:` fields of an identity file's text (`- **Name:** Wren`), each
/// trimmed of its markdown; never a placeholder in parentheses, nor one of
/// more than four words.
pub fn identity_names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let t = l.trim_start_matches(['-', '*', '_', ' ', '\t']);
            let rest = t
                .strip_prefix("Name:")
                .or_else(|| t.strip_prefix("name:"))?;
            let v = rest.trim_matches(['*', '_', '`', ' ', '\t']);
            let ok = !v.is_empty() && !v.starts_with('(') && v.split_whitespace().count() <= 4;
            ok.then(|| v.to_string())
        })
        .collect()
}

impl Core {
    /// The agents' names the store knows, folded; none when the import was
    /// not read (with a warning).
    pub(crate) fn people_house(&self) -> Arc<HashSet<String>> {
        self.people_house_read().unwrap_or_else(|e| {
            tracing::warn!(error = %format!("{e:#}"), "people: the agents' names were not read");
            Arc::default()
        })
    }

    fn people_house_read(&self) -> Result<Arc<HashSet<String>>> {
        let list = crate::import::write::list(&self.store)?;
        let version = crate::import::catalog::version_of(&list);
        let kept = &self.runner.judge.people.house;
        if let Some((v, names)) = kept.0.lock().unwrap().as_ref() {
            if *v == version {
                return Ok(names.clone());
            }
        }
        let (cat, _) = self.episodes.at(&self.store, &list)?;
        let mut names = HashSet::new();
        for i in 0..cat.rows.len() {
            let e = cat.episode(i);
            if e.erased {
                continue;
            }
            if let Some(a) = e.agent.as_deref() {
                names.insert(fold(a));
            }
            if e.place_kind == "file" && e.place_name.as_deref().is_some_and(is_identity) {
                for (_, n) in self.store.session_nodes(&e.session_id)? {
                    if let Body::Imported { text, .. } = &n.body {
                        names.extend(identity_names(text).iter().map(|n| fold(n)));
                    }
                }
            }
        }
        names.remove("");
        let names = Arc::new(names);
        *kept.0.lock().unwrap() = Some((version, names.clone()));
        Ok(names)
    }

    /// Who is never proposed (theseus-0p1r): the config's and the lines'
    /// ([`NotPeople::of`]), the owner's handles as the place rule knows
    /// them, the agents the store knows, and the held people those handles
    /// hold.
    pub(crate) fn not_people(&self, lines: &[Line], o: &theseus_ontology::Ontology) -> NotPeople {
        let cfg = &self.runner.cfg;
        NotPeople::of(cfg, lines)
            .with_owners(self.runner.place_rule.owners(cfg))
            .with_names(self.people_house().iter())
            .with_held(o)
    }
}
