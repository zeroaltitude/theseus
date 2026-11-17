//! `config.show` (issue 23): the whole text, before and after, of a change
//! waiting for the owner's yes: a prompt file's, or a setting too long for its
//! card. A read. The daemon builds the words from the texts it kept for the
//! card; a surface only shows them.

use serde::{Deserialize, Serialize};

/// `config.show`'s params: the waiting question's id, or none when only one
/// such change is waiting.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigShowParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub question: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `config.show`'s answer: the question shown, and its words, every line of
/// both texts, with the characters that cannot be seen written out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigShowResult {
    pub question: String,
    pub words: String,
}

/// The tool of a question about a change to Theseus's own settings or files.
pub const CONFIG_ACK: &str = "config.ack";

/// Whether `req` is a change whose card the daemon built to be read as it is,
/// and which has a whole text to read before the yes (`config.show`): a prompt
/// file's change, or a setting too long for the ordinary card. A surface draws
/// such a card with its lines and its blank space as they are, never cut and
/// never run together.
pub fn has_whole_text(req: &crate::ConfirmRequest) -> bool {
    req.tool == CONFIG_ACK && req.input["whole"].as_bool() == Some(true)
}
