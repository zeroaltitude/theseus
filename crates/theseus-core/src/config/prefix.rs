//! `[prefix]`, retired (2026-11-22): the prefix library is gone, and every key it had is in
//! `retired_keys::RETIRED`, so a settings file that still names one loads with its warning. The
//! section itself still parses (empty once those keys are taken out), so an unknown key in it still
//! fails the load, as in every section.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefixConfig {}

impl PrefixConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}
