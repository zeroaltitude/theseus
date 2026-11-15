//! A profile named for one of Jev's `model` options that runs another model. The router reaches an option's
//! model through the profile that runs it (`routing::model::profile_for`), so when no profile runs that model the
//! pick is a `fallback` and the turn stays where it was. Fable 5.2 became `fable`'s model on 2026-11-14: a config
//! written before then, `[profiles.fable]` at Fable 5.1, would route nothing to Fable and say nothing. This is the
//! load's warning for that (at each start and in `theseusd check`), in words that name the line to change.

use super::Config;
use crate::routing::model::OPTIONS;

impl Config {
    /// One warning per option whose name is a profile here, no profile runs the option's model, and the named one
    /// runs another model. An option with no profile of its name is the operator's choice (`deepseek_flash` is
    /// commented out until its key is added), so it says nothing.
    pub fn option_profile_warnings(&self) -> Vec<String> {
        let profiles = self.all_profiles();
        OPTIONS
            .iter()
            .filter(|(_, model)| !profiles.values().any(|p| p.model == *model))
            .filter_map(|(option, model)| {
                let had = &profiles.get(*option)?.model;
                Some(format!(
                    "[profiles.{option}] runs {had}, but Theseus's `{option}` choice is now {model} and no \
                     profile runs it, so a turn Jev sends to {option} stays on the model it was on: change \
                     that profile's model to \"{model}\" (keep {had} as a pin under another profile name if \
                     you still want it)"
                ))
            })
            .collect()
    }
}
