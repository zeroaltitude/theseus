//! The self card (theseus-slvl.8): one short paragraph at the end of the
//! tools note that says what Theseus can do in a place beyond its tools: join
//! the voice channel, set wakes, run tasks, look up past conversations, the
//! chat route for the owner's own acts, and where the person sees its work.
//! Facts only, one sentence each, and each only when it holds here.
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
    // F5 of the live freeze of 2026-11-13: the owner's acts have a CHAT
    // route, and the card never said so. Asked to untrust a repository from
    // his DM, the model searched the CLI's source, found only the terminal
    // route, and sent him to a shell -- against his standing wish that no
    // owner act need one. The product had the route (`/project trust`, from
    // 2026-11-12) and no way to say it existed.
    //
    // The line names the FAMILY and not a list, because the omission is
    // systematic rather than one command missing: the binding registers
    // twenty-nine slash commands (`theseus-discord/src/runtime/slash.rs`)
    // and this card named exactly one of them, `/join`, as a side effect of
    // the voice line above. Enumerating them here would go stale at the next
    // command and would spend the cached header on a list the person can
    // read from Discord's own menu; naming the family and the two acts a
    // model is most likely to send someone to a terminal for does not.
    //
    // It is LAST on purpose: every place with no chat route keeps the
    // card it had, byte for byte, so the three exact-match card tests and
    // the private-request golden are untouched by this commit.
    if place.chat {
        lines.push(
            "The person can act here from chat with slash commands, which Discord lists when they \
             type `/` \u{2014} `/trust` clears a content label this conversation is holding, and \
             `/project trust <path>` grants or revokes a repository's own instruction files; \
             never send them to a terminal for something a command here does."
                .into(),
        );
    }
    lines.join(" ")
}
