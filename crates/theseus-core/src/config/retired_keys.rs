//! The settings keys the cut retired (issue 44): the old way of building a
//! request is gone, and with it every key that tuned it. A settings file that
//! still names one must LOAD, because the sections that held them reject an
//! unknown key and a live daemon would otherwise not start. So each key in
//! [`RETIRED`] is taken out of the document before the strict parse, one
//! warning line names it and says why it is gone, and the start keeps the
//! list (`Config::ignored_keys`) for `theseus health` to show.
//!
//! This is the one table of them, kept whole whether or not the code that
//! read a key is deleted yet: while it is not, that code sees its default.
//! An unknown key that is NOT here still fails the load, as it always did.
//! (`renamed.rs`'s `RETIRED` is the other kind: names that fail the load on
//! purpose, with the new name.)

/// Every section's `*` (only `profiles.*`): the key in each table under it.
const EACH: &str = "*";

/// One retired key: its section, dotted when it is nested (`policy.tools`;
/// `profiles.*` for each profile; empty for the top level), its name, and one
/// sentence of what replaced it or why it is gone.
pub struct Retired {
    pub section: &'static str,
    pub key: &'static str,
    pub why: &'static str,
}

const VIEW: &str = "every request is built by the view now, so there is nothing to choose";
const KEPT: &str = "the kept copy of each session's request is gone: the view builds every turn's \
                    request afresh";
const LEDGER: &str = "the exchange ledger is what the view reads, so it can no longer be switched \
                      off";
const SUMMARY: &str = "compaction and its summary call are gone: a turn that outgrows the window \
                       makes room itself, or fails plainly";
const WARM: &str = "nothing is kept warm between turns any more";
const WAIT: &str = "nothing is kept warm during a wait any more (2026-11-22): no saved request is \
                    sent again";
const STUB: &str = "attachments are no longer replaced by stubs: there is no cold rewrite of a \
                    session any more";
const EXPERIMENT: &str =
    "the arm-assignment rows that named the experiment are no longer written: every \
                          session runs the one arm";
const CAP: &str =
    "recall no longer pauses at a cap on a session's notes: each request lays recall \
                   afresh, within the view's own budget for it";
const COLD: &str = "a thread is no longer resumed cold from a cut transcript: the view reads its \
                    conversation's exchanges";
const PREFIX: &str = "the prefix library is gone (2026-11-22): each request's system sections are \
                      rendered afresh by the view, with the same content, and no saved header is reused";
const THREADS: &str = "threads are no longer containers (2026-11-22): a thread's own recall, its \
                       growth, its home's size and its seals are gone";
const MAX_LIVE: &str =
    "threads are no longer containers (2026-11-22): nothing counts live threads, so \
                        there is no bound to set";
const EFFORT_FIXED: &str =
    "nothing reads it since the effort bounds and their clamp went: a profile's \
                            effort is what it sets, or what the route pack answers";
const STORE_ENGINE: &str =
    "redb is the only index engine behind the WAL, so there is nothing to choose";
const INERT: &str = "nothing read it any more: the link offer, the Discord thread mirror and the \
                     place tail's half-life went with threads as containers";
const LOOK_BACK: &str =
    "the history tool, context.history, is the one way to look back (issue 46): it \
                         reads every place, by words, a thing or a place, so this tool is gone";
const NARRATIVE: &str = "the narrative always runs and the web UI always shows its tab (issue 46)";
const VOICE: &str = "a call's timings are the code's, and Flux ends a turn by its own defaults (issue 46)";
const DIGEST: &str =
    "the place digest is gone (2026-11-22): no turn waits on Jev's scope answer, and \
                      no request carries a digest of the other threads";

/// The whole list (the map of the cut, §6 step 8), less the one key something that stays still reads:
/// `[cache] keep_warm_min_tokens` (the sticky effort rule), a live setting. The thread keys whose readers
/// went on 2026-11-22 are here too, and `[cache] keep_warm_every_minutes`, whose one reader (the read
/// during a wait) went that day. So are the prefix library's (`[prefix]`), gone the same day.
pub const RETIRED: &[Retired] = &[
    r("context", "assembly", VIEW),
    r("memory", "session_recall_cap_tokens", CAP),
    r("memory", "experiment", EXPERIMENT),
    r("context", "kept_mb", KEPT),
    r("context", "ledger", LEDGER),
    r("memory", "summary_profile", SUMMARY),
    r("cache", "keep_warm_hours", WARM),
    r("cache", "keep_warm_every_minutes", WAIT),
    r("cache", "threads_keep_warm_hours", WARM),
    r("cache", "stub_min_tokens", STUB),
    r("cache", "stub_after_turns", STUB),
    r("profiles.*", "keep_warm_hours", WARM),
    // Their older names (they were `config/renamed.rs` rows): retired with them, not asked to be renamed.
    r("model", "keep_warm_hours", WARM),
    r("cache", "keep_warm_minutes", WAIT),
    r("threads", "enabled", THREADS),
    r("threads", "keep_warm_hours", WARM),
    r("threads", "working_tokens", COLD),
    r("threads", "working_percent", COLD),
    r("threads", "cross_recall_penalty", THREADS),
    r("threads", "cross_recall_bar", THREADS),
    r("threads", "cross_recall_beats_own", THREADS),
    r("threads", "cross_recall_needs_entity", THREADS),
    r("threads", "base_distance", THREADS),
    r("threads", "own_top_admissible", THREADS),
    r("threads", "cross_recall_judge", THREADS),
    r("threads", "cross_recall_judge_k", THREADS),
    r("threads", "working_set_grows", THREADS),
    r("threads", "bench_clock", THREADS),
    r("threads", "resume_keep_turns", THREADS),
    r("threads", "home_warn_percent", THREADS),
    r("threads", "seal_recommend", THREADS),
    // XD3: inert keys, with the readers that went (the link offer, the Discord thread mirror, the place tail).
    r("threads", "link_proposals", INERT),
    r("threads", "discord_channel", INERT),
    r("threads", "sticky_half_life_secs", INERT),
    r("prefix", "enabled", PREFIX),
    r("prefix", "max_bytes", PREFIX),
    r("prefix", "act", PREFIX),
    r("prefix", "confirm", PREFIX),
    r("prefix", "candidates", PREFIX),
    // Settings nothing read any more (2026-11-22): a thread bound that went with D1, a clamp that went with
    // the effort bounds, an engine with one choice.
    r("threads", "max_live", MAX_LIVE),
    r("profiles.*", "effort_fixed", EFFORT_FIXED),
    r("server", "store_engine", STORE_ENGINE),
    // A posture set for a tool that is gone (issue 46).
    r("policy.tools", "thread.read", LOOK_BACK),
    r("policy.tools", "channel.read", LOOK_BACK),
    r("policy.tools", "memory.lookup", LOOK_BACK),
    r("threads", "place_digest", DIGEST),
    r("threads", "place_digest_line_chars", DIGEST),
    r("threads", "route_wait_ms", DIGEST),
    // Settings nobody set, fixed at what they do today (issue 46).
    r("", "narrative", NARRATIVE),
    r("voice", "judge_ms", VOICE),
    r("voice", "pause_ms", VOICE),
    r("voice", "lead_in_ms", VOICE),
    r("voice", "gap_lead_in_ms", VOICE),
    r("voice", "merge_ms", VOICE),
    r("voice", "eot_threshold", VOICE),
    r("voice", "eager_eot_threshold", VOICE),
    r("voice", "eot_timeout_ms", VOICE),
];

const fn r(section: &'static str, key: &'static str, why: &'static str) -> Retired {
    Retired { section, key, why }
}

/// A key this start found and ignored: its dotted name and why it is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ignored {
    pub name: String,
    pub why: &'static str,
}

/// Take every retired key out of `doc`, and say which were there, in the
/// table's order (each profile's by its name).
pub fn strip(doc: &mut toml::Table) -> Vec<Ignored> {
    let mut out = Vec::new();
    for k in RETIRED {
        let (head, each) = match k.section.rsplit_once('.') {
            Some((head, EACH)) => (head, true),
            _ => (k.section, false),
        };
        let Some(section) = table_at(doc, head) else {
            continue;
        };
        if !each {
            if section.remove(k.key).is_some() {
                out.push(ignored(k.section, k.key, k.why));
            }
            continue;
        }
        for (name, t) in section.iter_mut() {
            let Some(t) = t.as_table_mut() else { continue };
            if t.remove(k.key).is_some() {
                out.push(ignored(&format!("{head}.{name}"), k.key, k.why));
            }
        }
    }
    out
}

/// The table at the dotted `path` of `doc` (`policy.tools`), when each step is one; `doc` itself for an empty
/// path.
fn table_at<'a>(doc: &'a mut toml::Table, path: &str) -> Option<&'a mut toml::Table> {
    path.split('.')
        .filter(|step| !step.is_empty())
        .try_fold(doc, |t, step| t.get_mut(step)?.as_table_mut())
}

fn ignored(section: &str, key: &str, why: &'static str) -> Ignored {
    let name = match section {
        "" => key.to_string(),
        s => format!("{s}.{key}"),
    };
    Ignored { name, why }
}

/// The start's warning for one: what it was, that it does nothing now, why.
pub fn warning(i: &Ignored) -> String {
    format!(
        "{} is retired: {}. It is read and ignored; remove it",
        i.name, i.why
    )
}

/// The start's warnings for every key it ignored, in order.
pub fn warnings(ignored: &[Ignored]) -> Vec<String> {
    ignored.iter().map(warning).collect()
}

#[cfg(test)]
#[path = "tests_retired_keys.rs"]
mod tests;
