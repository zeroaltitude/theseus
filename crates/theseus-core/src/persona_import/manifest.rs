//! What an import leaves beside a persona's files: `import.json`, the
//! record of where they came from, which version this is, and the drafted
//! card the owner has not yet kept.

use serde::{Deserialize, Serialize};

/// `<state>/personas/<name>/import.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub agent: String,
    /// The workspace it was read from.
    pub from: String,
    /// 1, then one more each import whose bytes differ.
    pub version: u32,
    pub imported_ms: u64,
    /// Each carried file: name under the folder, SHA-256 (hex, 16), bytes.
    pub files: Vec<(String, String, u64)>,
    /// The card the import drafted. While the config's card is still this
    /// text and `card_confirmed` is false, it is a draft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_draft: Option<String>,
    #[serde(default)]
    pub card_confirmed: bool,
}

impl Manifest {
    /// `<dir>/import.json`.
    pub fn path(dir: &std::path::Path) -> std::path::PathBuf {
        dir.join("import.json")
    }

    pub fn read(dir: &std::path::Path) -> Option<Self> {
        let text = std::fs::read_to_string(Self::path(dir)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn write(&self, dir: &std::path::Path) -> std::io::Result<()> {
        let tmp = dir.join("import.json.new");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(&tmp, Self::path(dir))
    }

    /// Whether the persona's card, as the config has it, is still the draft.
    pub fn card_is_draft(&self, card: Option<&str>) -> bool {
        !self.card_confirmed && self.card_draft.is_some() && self.card_draft.as_deref() == card
    }
}
