//! The self card (theseus-slvl.8): one short paragraph at the end of the
//! tools note that says what Theseus can do in a place beyond its tools: join
//! the voice channel, set wakes, run tasks, look up past conversations, and
//! where the person sees its work. Facts only, one sentence each, and each
//! only when it holds here.
//!
//! It is built from the config, the catalog and the place's class alone, never
//! from a request, so it is the same bytes for every session of a place class
//! and the cached header does not churn. A change to the config or the
//! bindings is a recompile, as for the tools note.

use crate::ceiling::PlaceView;
use crate::toolrun::ToolRuntime;

/// The card for a place, empty when the place has nothing to say.
pub fn card(cfg: &crate::Config, tools: &ToolRuntime, place: PlaceView) -> String {
    let offers =
        |name: &str| tools.registry.get(name).is_some() && place.offered(name, &tools.public_roots);
    let mut lines: Vec<String> = Vec::new();
    if place.voice && cfg.voice.enabled {
        lines.push(
            "You can join a voice channel the bindings name: the person types /join in a private place to invite you."
                .into(),
        );
    }
    if offers(crate::wake::AT) || offers("task.create") {
        lines.push("You can set a wake to follow up later and run background tasks.".into());
    }
    if offers(crate::memory_lookup::LOOKUP) {
        lines.push("You can look up past conversations.".into());
    }
    if place.class == crate::places::PlaceClass::Private && cfg.web.enabled {
        let host = match cfg.web.bind.contains(':') {
            true => format!("[{}]", cfg.web.bind),
            false => cfg.web.bind.clone(),
        };
        lines.push(format!(
            "The person can see everything you do in the cockpit at http://{host}:{}.",
            cfg.web.port
        ));
    }
    lines.join(" ")
}
