//! `[personas.<name>]` (theseus-c48, theseus-slvl.2): a persona as data. Its
//! context files (the owner's words, compiled after the system level's), and
//! the short fields that name and shape it: `name`, `emoji`, `card`,
//! `tts_voice`, `avatar` and `initiative`. A persona the config does not
//! name is the built-in default (`persona/default.md`), which an owner's
//! persona file replaces whole and is never merged with.

use serde::{Deserialize, Serialize};

use crate::context_files::ContextEntry;

pub use theseus_protocol::Initiative;

fn normal(i: &Initiative) -> bool {
    *i == Initiative::Normal
}

/// `[personas.<name>]`: a persona's context files, compiled after the system
/// level's while the persona is in play, and its short fields. Every key is
/// optional.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonaConfig {
    #[serde(default)]
    pub files: Vec<ContextEntry>,
    /// Shown on replies, the bot's nickname, the voice greeting. Absent: the
    /// table's key, capitalised.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    /// About 60 tokens of specialization and background: what the persona
    /// question of route.v5 reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
    /// Overrides `[voice] tts_voice` while this persona is in play.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tts_voice: Option<String>,
    /// A path or a URL, for Discord.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    /// How readily it speaks first (theseus-slvl.9): `quiet`, `normal` (the
    /// default) or `chatty`. `notices::policy` reads it through
    /// `Rules::initiative`, and the daemon's own proactive posts through
    /// `Initiative::lets`.
    #[serde(default, skip_serializing_if = "normal")]
    pub initiative: Initiative,
    /// Whether a shared place may be answered by this persona beyond its own
    /// (route.v5's persona question and an explicit address, theseus-c1uz).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shareable: bool,
}

/// The built-in default persona (`persona/default.md`): who Theseus is, a
/// line of voice per surface, and values with candour first. The header's
/// persona text for an install that names no persona, and for a persona whose
/// files cannot be read. Static, so it sits in the cached prefix.
pub const DEFAULT_TEXT: &str = include_str!("../../persona/default.md");

/// The name the built-in persona shows.
pub const DEFAULT_NAME: &str = "Theseus";

/// A persona as the rest of the daemon reads it (`Config::persona_for`).
#[derive(Debug, Clone, PartialEq)]
pub struct PersonaView {
    /// The `[personas]` key; none for the built-in default.
    pub key: Option<String>,
    pub name: String,
    pub emoji: Option<String>,
    pub card: Option<String>,
    pub tts_voice: Option<String>,
    pub avatar: Option<String>,
    pub initiative: Initiative,
    pub files: Vec<ContextEntry>,
    pub shareable: bool,
}

impl PersonaView {
    pub fn built_in() -> Self {
        Self {
            key: None,
            name: DEFAULT_NAME.into(),
            emoji: None,
            card: None,
            tts_voice: None,
            avatar: None,
            initiative: Initiative::Normal,
            files: Vec::new(),
            shareable: false,
        }
    }

    pub fn of(key: &str, p: &PersonaConfig) -> Self {
        Self {
            key: Some(key.to_string()),
            name: p.name.clone().unwrap_or_else(|| capitalised(key)),
            emoji: p.emoji.clone(),
            card: p.card.clone(),
            tts_voice: p.tts_voice.clone(),
            avatar: p.avatar.clone(),
            initiative: p.initiative,
            files: p.files.clone(),
            shareable: p.shareable,
        }
    }
}

/// `quiet` as `Quiet`.
pub fn capitalised(key: &str) -> String {
    let mut c = key.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

impl PersonaConfig {
    /// The short fields' spelling: a field present must say something, and a
    /// name, an emoji and a voice are one line. Files are checked by the
    /// config (paths only).
    pub fn validate(&self, key: &str) -> anyhow::Result<()> {
        for (field, v) in [
            ("name", &self.name),
            ("emoji", &self.emoji),
            ("tts_voice", &self.tts_voice),
            ("avatar", &self.avatar),
            ("card", &self.card),
        ] {
            let Some(v) = v else { continue };
            if v.trim().is_empty() {
                anyhow::bail!("personas.{key}.{field} is empty: leave the key out instead");
            }
            if field != "card" && v.contains('\n') {
                anyhow::bail!("personas.{key}.{field} must be one line");
            }
        }
        if self
            .card
            .as_deref()
            .is_some_and(|c| c.chars().count() > CARD_MAX)
        {
            anyhow::bail!("personas.{key}.card is at most {CARD_MAX} characters (about 60 tokens)");
        }
        if self.name.as_deref().is_some_and(|n| n.chars().count() > 80) {
            anyhow::bail!("personas.{key}.name is at most 80 characters");
        }
        Ok(())
    }
}

/// The card's cap, in characters: about 60 tokens with room.
pub const CARD_MAX: usize = 600;

impl crate::config::Config {
    /// What the config says of personas that no longer does anything
    /// (theseus-utfnl): `[personas]` and `[context] default_persona` are read
    /// once, by the migration at the first start of the version that moved
    /// personas into the ontology, and ignored after it. One warning at each
    /// start while they stay in the file, naming any file a persona listed
    /// beyond its first, which is not carried (a persona is one small file).
    pub fn persona_warnings(&self) -> Vec<String> {
        if self.personas.is_empty() && self.context.default_persona.is_none() {
            return Vec::new();
        }
        let mut text = String::from(
            "[personas] and [context] default_persona are read once, at the first start of this \
             version, and the personas are the ontology's now (`theseus persona list`, `add`, \
             `soul`): they do nothing here, and the file may drop them",
        );
        for (key, p) in &self.personas {
            if p.files.len() > 1 {
                let rest: Vec<&str> = p.files[1..].iter().map(ContextEntry::path).collect();
                text.push_str(&format!(
                    "; persona {key}'s soul is {}, and its other files are no longer carried: {}",
                    p.files[0].path(),
                    rest.join(", ")
                ));
            }
        }
        vec![text]
    }
}
