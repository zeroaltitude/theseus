//! The owner's writes to a persona (theseus-utfnl): add, soul, set, retire,
//! default. Each is a write to the persona's category, as the owner's, and
//! the ontology refuses what is not allowed (a machine origin, a membership,
//! guidance, a second default). `rpc/persona_write.rs` judges the caller.

use anyhow::{bail, Result};
use theseus_ontology::{Category, CategoryId, Initiative, Origin, PersonaCard, Record};
use theseus_protocol::{
    method, PersonaAddParams, PersonaKeyParams, PersonaSetParams, PersonaSoulParams,
    PersonaWriteResult,
};

use crate::approval::Answerer;
use crate::config::persona::capitalised;
use crate::fact;
use crate::rpc::Core;

fn id_of(key: &str) -> Result<CategoryId> {
    Ok(CategoryId::new(super::KIND, key.trim())?)
}

fn set(field: &mut Option<String>, v: &Option<String>) {
    match v.as_deref() {
        Some("") => *field = None,
        Some(v) => *field = Some(v.to_string()),
        None => {}
    }
}

impl Core {
    fn persona_category(&self, key: &str) -> Result<Category> {
        let o = self.runner.ontology.snapshot(&self.store)?;
        match o.category(&id_of(key)?) {
            Some(c) => Ok(c.clone()),
            None => bail!(
                "no persona {key:?}: `theseus persona` lists them, `theseus persona add` makes one"
            ),
        }
    }

    /// Write `records` as the owner's, each with its category row, and
    /// `extra` rows in the same frame.
    fn write_persona_records(
        &self,
        records: Vec<Category>,
        who: &Answerer,
        extra: impl FnOnce() -> Result<Vec<theseus_store::NewRecord>>,
    ) -> Result<()> {
        let (who_s, via) = (who.who(), who.via());
        let sets: Vec<_> = records
            .iter()
            .map(|c| fact::ontology::CategorySet {
                category: c,
                origin: "operator",
                who: &who_s,
                via: &via,
                undo: None,
            })
            .collect();
        self.runner.ontology.write(
            &self.store,
            records.iter().cloned().map(Record::Category).collect(),
            Origin::Operator,
            |_| {
                let mut rows = vec![];
                for s in &sets {
                    rows.push(fact::row(s, None, None)?);
                }
                rows.extend(extra()?);
                Ok(rows)
            },
        )?;
        for s in &sets {
            self.rec(None).announce(s);
        }
        Ok(())
    }

    fn written(&self, c: &Category) -> PersonaWriteResult {
        let card = c.persona.as_ref();
        let soul = card.and_then(|p| p.soul.clone());
        let tokens = soul
            .as_deref()
            .and_then(|s| std::fs::read_to_string(crate::config::expand(s)).ok())
            .map(|t| super::soul::tokens(&t));
        PersonaWriteResult {
            key: c.id.local().into(),
            state: match (c.retired_ms, soul.is_some()) {
                (Some(_), _) => "retired",
                (None, true) => "live",
                (None, false) => "awaiting",
            }
            .into(),
            soul,
            tokens,
            cap: self.cfg.context.persona_cap(),
        }
    }

    /// `persona.add`: a persona, with its soul when it comes with one, else
    /// awaiting it (the owner is asked once, by `persona_soul_note`).
    pub fn persona_add(&self, p: &PersonaAddParams, who: &Answerer) -> Result<PersonaWriteResult> {
        self.judge_persona(method::PERSONA_ADD, &format!("persona {}", p.key), who)?;
        let key = p.key.trim();
        let id = id_of(key)?;
        if self.personas().exists(key) {
            bail!("persona {key:?} exists: `theseus persona soul {key}` writes its soul, `persona set {key}` its fields");
        }
        let name = p.name.clone().unwrap_or_else(|| capitalised(key));
        let mut card = PersonaCard {
            emoji: p.emoji.clone(),
            card: p.card.clone(),
            ..PersonaCard::default()
        };
        let written = match p.soul.as_deref() {
            Some(text) => Some(self.write_soul(key, text, None)?),
            None => None,
        };
        if let Some(w) = &written {
            card.soul = Some(w.path.clone());
            card.soul_digest = Some(w.digest.clone());
        }
        let mut c = Category::new(id, name, super::soul_added_by(who));
        c.persona = Some(card);
        let fact = written.as_ref().map(|w| super::soul_fact(key, w, who));
        self.write_persona_records(vec![c.clone()], who, || match &fact {
            Some(f) => Ok(vec![fact::row(f, None, None)?]),
            None => Ok(vec![]),
        })?;
        if written.is_none() {
            self.persona_soul_note(key, &c.name, p.from.as_deref().unwrap_or("the CLI"));
        }
        Ok(self.written(&c))
    }

    /// `persona.soul`: write the soul, and set the category's pointer and
    /// digest in the same frame as its `persona.soul` row.
    pub fn persona_soul(
        &self,
        p: &PersonaSoulParams,
        who: &Answerer,
    ) -> Result<PersonaWriteResult> {
        self.judge_persona(
            method::PERSONA_SOUL,
            &format!("the soul of persona {}", p.key),
            who,
        )?;
        let key = p.key.trim();
        let mut c = self.persona_category(key)?;
        if c.is_gone() {
            bail!("persona {key:?} is retired");
        }
        let card = c.persona.get_or_insert_with(PersonaCard::default);
        let w = self.write_soul(key, &p.text, card.soul.as_deref())?;
        card.soul = Some(w.path.clone());
        card.soul_digest = Some(w.digest.clone());
        let fact = super::soul_fact(key, &w, who);
        self.write_persona_records(vec![c.clone()], who, || {
            Ok(vec![fact::row(&fact, None, None)?])
        })?;
        self.rec(None).announce(&fact);
        Ok(self.written(&c))
    }

    /// `persona.set`: the short fields.
    pub fn persona_set(&self, p: &PersonaSetParams, who: &Answerer) -> Result<PersonaWriteResult> {
        self.judge_persona(
            method::PERSONA_SET,
            &format!("the fields of persona {}", p.key),
            who,
        )?;
        let mut c = self.persona_category(&p.key)?;
        if let Some(n) = p.name.as_deref() {
            c.name = match n {
                "" => capitalised(c.id.local()),
                n => n.to_string(),
            };
        }
        let card = c.persona.get_or_insert_with(PersonaCard::default);
        set(&mut card.emoji, &p.emoji);
        set(&mut card.card, &p.card);
        set(&mut card.tts_voice, &p.tts_voice);
        set(&mut card.avatar, &p.avatar);
        if let Some(i) = p.initiative.as_deref() {
            card.initiative = match i {
                "quiet" => Initiative::Quiet,
                "normal" => Initiative::Normal,
                "chatty" => Initiative::Chatty,
                _ => bail!("initiative is quiet, normal or chatty, not {i:?}"),
            };
        }
        if let Some(s) = p.shareable {
            card.shareable = s;
        }
        self.write_persona_records(vec![c.clone()], who, || Ok(vec![]))?;
        Ok(self.written(&c))
    }

    /// `persona.retire`: taken away (a persona has no membership and no
    /// guidance, so nothing holds it); the ledger and the undo keep it.
    pub fn persona_retire(
        &self,
        p: &PersonaKeyParams,
        who: &Answerer,
    ) -> Result<PersonaWriteResult> {
        self.judge_persona(
            method::PERSONA_RETIRE,
            &format!(
                "the retirement of persona {}",
                p.key.as_deref().unwrap_or("?")
            ),
            who,
        )?;
        let Some(key) = p.key.as_deref() else {
            bail!("name the persona to retire");
        };
        let c = self
            .persona_category(key)?
            .retired(theseus_protocol::now_unix_ms());
        self.write_persona_records(vec![c.clone()], who, || Ok(vec![]))?;
        Ok(self.written(&c))
    }

    /// `persona.default`: the persona Jev's `none` engages. No key clears
    /// the mark. The old holder is cleared in the same frame.
    pub fn persona_default(
        &self,
        p: &PersonaKeyParams,
        who: &Answerer,
    ) -> Result<PersonaWriteResult> {
        self.judge_persona(method::PERSONA_DEFAULT, "the default persona", who)?;
        let book = self.personas();
        let mut records = Vec::new();
        if let Some(old) = book.default_key().filter(|k| Some(*k) != p.key.as_deref()) {
            let mut c = self.persona_category(old)?;
            if let Some(card) = c.persona.as_mut() {
                card.default = false;
            }
            records.push(c);
        }
        let mut shown = None;
        if let Some(key) = p.key.as_deref() {
            let mut c = self.persona_category(key)?;
            c.persona.get_or_insert_with(PersonaCard::default).default = true;
            shown = Some(c.clone());
            records.push(c);
        }
        let Some(shown) = shown.or_else(|| records.first().cloned()) else {
            bail!("no persona is the default");
        };
        self.write_persona_records(records, who, || Ok(vec![]))?;
        Ok(self.written(&shown))
    }
}

impl Core {
    /// An import's persona is a category (origin import): its soul the first
    /// file carried, its card the draft when it has none, both kept if the
    /// owner has set them. A soul over the cap is not made a persona: the
    /// files are carried byte for byte and the error says what to trim.
    pub(crate) fn persona_from_import(
        &self,
        key: &str,
        r: &theseus_protocol::PersonaImportResult,
    ) -> Result<()> {
        let Some(soul) = r.context_paths.first() else {
            return Ok(());
        };
        let text = std::fs::read_to_string(crate::config::expand(soul))?;
        let cap = self.cfg.context.persona_cap();
        if let Err(e) = super::soul::within(key, &text, cap) {
            bail!(
                "{e}\nThe files are carried to {} as they were, and persona {key} is not made: \
                 trim {soul}, then `theseus persona add {key} --file {soul}`",
                r.dir
            );
        }
        let id = id_of(key)?;
        let o = self.runner.ontology.snapshot(&self.store)?;
        let mut c = match o.category(&id) {
            Some(c) => c.clone(),
            None => Category::new(id, capitalised(key), "import"),
        };
        let card = c.persona.get_or_insert_with(PersonaCard::default);
        card.soul = Some(soul.clone());
        card.soul_digest = Some(crate::persona::digest(&text));
        if card.card.is_none() {
            card.card = r.card.clone();
        }
        let (who, via) = ("persona import".to_string(), "the CLI".to_string());
        let set = fact::ontology::CategorySet {
            category: &c,
            origin: "import",
            who: &who,
            via: &via,
            undo: None,
        };
        self.runner.ontology.write(
            &self.store,
            vec![Record::Category(c.clone())],
            Origin::Import,
            |_| Ok(vec![fact::row(&set, None, None)?]),
        )?;
        self.rec(None).announce(&set);
        Ok(())
    }
}
