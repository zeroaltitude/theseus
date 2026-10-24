//! `[personas]` read once (theseus-utfnl). At the first start that has them
//! and no mark that they were migrated, each becomes a `persona:<key>`
//! category (origin operator, added_by `config-migration`) in one frame,
//! with the mark (META `personas.migrated`) and one `persona.migrated` row.
//! From then on `[personas]` and `[context] default_persona` are ignored:
//! `Config::persona_warnings` says so at each start, and one note to the
//! owner says how to delete them.
//!
//! A persona is one small file: the first of a persona's files is its soul,
//! and any other is named in the warning and the row as no longer carried
//! (a runbook is not a persona). No file moves: the soul's pointer is the
//! path the config spelled.

use anyhow::Result;
use serde_json::{json, Value};
use theseus_ontology::{Category, CategoryId, Origin, PersonaCard, Record};

use crate::config::{Config, PersonaConfig, PersonaView};
use crate::context_files::ContextReaders;
use crate::fact;
use crate::owner_note::{note, Kind};
use crate::rpc::Core;

/// The META key that says `[personas]` was read.
pub const MARKER: &str = "personas.migrated";

/// The `added_by` of what the migration wrote.
pub const ADDED_BY: &str = "config-migration";

/// A persona config as its category: the soul is its first file.
pub fn card_of(key: &str, p: &PersonaConfig, default: bool, digest: Option<String>) -> PersonaCard {
    let first = p.files.first();
    PersonaCard {
        soul: first.map(|f| f.path().to_string()),
        soul_public: first.is_some_and(|f| f.readers() == ContextReaders::Public),
        soul_digest: first.and(digest),
        emoji: p.emoji.clone(),
        card: p.card.clone(),
        tts_voice: p.tts_voice.clone(),
        avatar: p.avatar.clone(),
        initiative: match p.initiative {
            crate::config::Initiative::Quiet => theseus_ontology::Initiative::Quiet,
            crate::config::Initiative::Normal => theseus_ontology::Initiative::Normal,
            crate::config::Initiative::Chatty => theseus_ontology::Initiative::Chatty,
        },
        shareable: p.shareable,
        default: default && !key.is_empty(),
    }
}

/// What the migration writes: a category per persona, the digests it read,
/// and the files not carried (`<key>: <path>`).
pub struct Plan {
    pub categories: Vec<Category>,
    pub moved: Vec<(String, String, Option<String>)>,
    pub dropped: Vec<String>,
}

/// The plan for `cfg`, reading each soul once through `read`.
pub fn plan(cfg: &Config, read: impl Fn(&str) -> Option<String>) -> Result<Plan> {
    let mut out = Plan {
        categories: Vec::new(),
        moved: Vec::new(),
        dropped: Vec::new(),
    };
    for (key, p) in &cfg.personas {
        let view = PersonaView::of(key, p);
        let digest = p
            .files
            .first()
            .and_then(|f| read(f.path()))
            .map(|t| crate::persona::digest(&t));
        let default = cfg.context.default_persona.as_deref() == Some(key.as_str());
        let mut c = Category::new(
            CategoryId::new(crate::personas::KIND, key)?,
            view.name,
            ADDED_BY,
        );
        c.persona = Some(card_of(key, p, default, digest.clone()));
        if let Some(first) = p.files.first() {
            out.moved
                .push((key.clone(), first.path().to_string(), digest));
        }
        out.dropped.extend(
            p.files
                .iter()
                .skip(1)
                .map(|f| format!("{key}: {}", f.path())),
        );
        out.categories.push(c);
    }
    Ok(out)
}

/// The one note the migration sends: what moved, and how to delete the tables.
pub fn note_text(keys: &[&str], dropped: &[String]) -> String {
    let not_carried = match dropped.is_empty() {
        true => String::new(),
        false => format!(
            " Not carried (a persona is one small file): {}.",
            dropped.join(", ")
        ),
    };
    note(
        Kind::Fyi,
        "Personas moved into the ontology",
        "your config's [personas] was read once, at this start",
        &format!(
            "{} persona(s) ({}) are the ontology's now. `theseus persona` shows them, \
             `theseus persona add KEY` makes another, and `theseus persona soul KEY` writes a \
             soul. The [personas] tables and [context] default_persona in your config do \
             nothing from here: delete them.{not_carried}",
            keys.len(),
            keys.join(", "),
        ),
    )
}

impl Core {
    /// Read `[personas]` once, before serving. Nothing is read when the
    /// config has none, and a start after the first writes nothing: one META
    /// key says it was done. One frame: a persona the ontology refuses fails
    /// the whole migration, which is logged and tried again at the next start.
    pub(crate) fn migrate_personas(&self) -> Result<()> {
        if self.cfg.personas.is_empty() || self.store.get_meta::<Value>(MARKER)?.is_some() {
            return Ok(());
        }
        let read = |p: &str| std::fs::read_to_string(crate::config::expand(p)).ok();
        let plan = plan(&self.cfg, read)?;
        let now = theseus_protocol::now_unix_ms();
        let keys: Vec<&str> = plan.categories.iter().map(|c| c.id.local()).collect();
        let mark = json!({"at_ms": now, "keys": keys});
        let fact = fact::persona::PersonaMigrated {
            personas: &plan.moved,
            dropped: &plan.dropped,
        };
        let records: Vec<Record> = plan
            .categories
            .iter()
            .cloned()
            .map(Record::Category)
            .collect();
        self.runner
            .ontology
            .write(&self.store, records, Origin::Operator, |_| {
                Ok(vec![
                    theseus_store::NewRecord::json(
                        theseus_store::kinds::META,
                        Some(MARKER),
                        &mark,
                    )?,
                    fact::row(&fact, None, None)?,
                ])
            })?;
        self.rec(None).announce(&fact);
        let text = note_text(&keys, &plan.dropped);
        if let Err(e) = self
            .outbox
            .to_operator(None, json!({"kind": "notice", "text": text}))
        {
            tracing::warn!(error = %format!("{e:#}"), "the personas note was not posted");
        }
        Ok(())
    }
}
