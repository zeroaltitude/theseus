//! The cut's retired settings keys (issue 44): a settings file that still
//! names one loads, says so once per key, and the start keeps their names for
//! health; a key nobody ever had still fails the load.

use super::{strip, RETIRED};
use crate::Config;

/// The least a settings file holds to load: a model key's reference.
const BASE: &str = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n";

/// A file naming every retired key, each with a value its old reader took.
const EVERY: &str = "[model]\n\
                     keep_warm_hours = 12\n\
                     [context]\n\
                     assembly = \"transcript\"\n\
                     kept_mb = 0\n\
                     ledger = false\n\
                     [memory]\n\
                     summary_profile = \"session\"\n\
                     session_recall_cap_tokens = 64000\n\
                     experiment = \"m6-1\"\n\
                     [cache]\n\
                     keep_warm_hours = 24\n\
                     threads_keep_warm_hours = 4\n\
                     stub_min_tokens = 20000\n\
                     stub_after_turns = 3\n\
                     keep_warm_every_minutes = 55\n\
                     keep_warm_minutes = 50\n\
                     [threads]\n\
                     enabled = true\n\
                     keep_warm_hours = 2\n\
                     working_tokens = 60000\n\
                     working_percent = 40\n\
                     cross_recall_penalty = 0.35\n\
                     cross_recall_bar = 0.75\n\
                     cross_recall_beats_own = true\n\
                     cross_recall_needs_entity = true\n\
                     base_distance = true\n\
                     own_top_admissible = true\n\
                     cross_recall_judge = true\n\
                     cross_recall_judge_k = 3\n\
                     working_set_grows = true\n\
                     bench_clock = false\n\
                     resume_keep_turns = 20\n\
                     home_warn_percent = 50\n\
                     seal_recommend = false\n\
                     max_live = 50\n\
                     link_proposals = true\n\
                     discord_channel = \"123456789012345678\"\n\
                     sticky_half_life_secs = 600\n\
                     place_digest = true\n\
                     place_digest_line_chars = 200\n\
                     route_wait_ms = 400\n\
                     [prefix]\n\
                     enabled = true\n\
                     max_bytes = 10485760\n\
                     act = 0.9\n\
                     confirm = 0.6\n\
                     candidates = 16\n\
                     [server]\n\
                     store_engine = \"redb\"\n\
                     [voice]\n\
                     judge_ms = 400\n\
                     pause_ms = 1400\n\
                     lead_in_ms = 0\n\
                     gap_lead_in_ms = 0\n\
                     merge_ms = 0\n\
                     eot_threshold = 0.8\n\
                     eager_eot_threshold = 0.5\n\
                     eot_timeout_ms = 6000\n\
                     [discord]\n\
                     edit_interval_ms = 250\n\
                     notice_embeds = true\n\
                     status_line = false\n\
                     app_emojis = false\n\
                     [kernel]\n\
                     steer_queued = false\n\
                     [policy.tools]\n\
                     \"thread.read\" = \"open\"\n\
                     \"channel.read\" = \"notify\"\n\
                     \"memory.lookup\" = \"open\"\n\
                     [profiles.quick]\n\
                     provider = \"anthropic\"\n\
                     model = \"claude-haiku-5-5\"\n\
                     effort_fixed = true\n\
                     keep_warm_hours = 2\n";

fn parse(text: &str) -> anyhow::Result<(Config, Vec<String>)> {
    Config::parse(&format!("{BASE}{text}"))
}

fn names(cfg: &Config) -> Vec<&str> {
    cfg.ignored_keys.iter().map(|i| i.name.as_str()).collect()
}

/// Every key of the table, in a file that names them all, loads; each is in
/// the start's ignored list, and one warning line names it and why it went.
#[test]
fn a_settings_file_naming_every_retired_key_loads_and_names_each() {
    let (cfg, warnings) = Config::parse(&format!("narrative = true\n{BASE}{EVERY}"))
        .expect("an old settings file loads");
    let ignored = names(&cfg);
    for k in RETIRED {
        let name = match k.section {
            "" => k.key.to_string(),
            "profiles.*" => format!("profiles.quick.{}", k.key),
            s => format!("{s}.{}", k.key),
        };
        assert!(ignored.contains(&name.as_str()), "{name}: {ignored:?}");
        let said: Vec<_> = warnings
            .iter()
            .filter(|w| w.starts_with(&format!("{name} is retired: ")))
            .collect();
        assert_eq!(said.len(), 1, "{name}: {warnings:?}");
        assert!(said[0].contains(k.why), "{}", said[0]);
    }
    assert_eq!(ignored.len(), RETIRED.len());
    // Nothing of what the file said reached a reader: the context section is as if unset (the ledger has
    // no switch left to read).
    assert!(cfg.context.is_empty(), "{:?}", cfg.context);
}

/// `[context] ledger = true` is ignored with the same warning as `false`: the
/// key is gone either way, and the line asks for it to be removed.
#[test]
fn the_ledger_key_is_ignored_whatever_it_says() {
    for v in ["true", "false"] {
        let (cfg, warnings) = parse(&format!("[context]\nledger = {v}\n")).unwrap();
        assert_eq!(names(&cfg), ["context.ledger"], "{v}");
        assert_eq!(
            warnings
                .iter()
                .filter(|w| w.starts_with("context.ledger is retired"))
                .count(),
            1,
            "{v}: {warnings:?}"
        );
    }
}

/// The template names no retired key, commented or not, and loads with none
/// ignored; an empty file the same.
#[test]
fn the_template_names_no_retired_key() {
    let (cfg, warnings) = Config::parse(Config::EXAMPLE_TOML).unwrap();
    assert!(cfg.ignored_keys.is_empty(), "{:?}", names(&cfg));
    assert!(
        !warnings.iter().any(|w| w.contains(" is retired: ")),
        "{warnings:?}"
    );
    let out = super::super::tests::uncommented(Config::EXAMPLE_TOML);
    let cfg = Config::from_toml(&out).unwrap();
    assert!(cfg.ignored_keys.is_empty(), "{:?}", names(&cfg));
    let (cfg, _) = parse("").unwrap();
    assert!(cfg.ignored_keys.is_empty());
}

/// The key something still reads is live, not retired: a file that sets it is read. (`[threads]
/// resume_keep_turns` was another until the home-size warning went with the threads, and `[cache]
/// keep_warm_every_minutes` until the read during a wait went, both 2026-11-22: they are retired now, and a
/// file that sets the interval, or its older name, still loads with the warning.)
#[test]
fn the_keys_something_still_reads_are_live() {
    let (cfg, _) = parse("[cache]\nkeep_warm_min_tokens = 5000\n").unwrap();
    assert!(cfg.ignored_keys.is_empty(), "{:?}", names(&cfg));
    assert_eq!(cfg.cache.keep_warm_min_tokens, 5000);
    let (cfg, warnings) =
        parse("[cache]\nkeep_warm_every_minutes = 60\nkeep_warm_minutes = 50\n").unwrap();
    assert_eq!(
        names(&cfg),
        ["cache.keep_warm_every_minutes", "cache.keep_warm_minutes"]
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("cache.keep_warm_every_minutes is retired: ")),
        "{warnings:?}"
    );
}

/// An older name of a retired key loads with the retired key's warning; an older name of a live key still
/// fails the load, asking for its rename.
#[test]
fn an_old_name_of_a_retired_key_is_retired_with_it() {
    let (cfg, warnings) =
        parse("[model]\nkeep_warm_hours = 12\n[threads]\nkeep_warm_hours = 2\n").unwrap();
    assert_eq!(
        names(&cfg),
        ["model.keep_warm_hours", "threads.keep_warm_hours"]
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("model.keep_warm_hours is retired: ")),
        "{warnings:?}"
    );
    let e = parse("[model]\ncache_ttl = \"1h\"\n").unwrap_err();
    assert!(format!("{e:#}").contains("[cache] ttl"), "{e:#}");
}

/// A key no build ever had still fails the load, in the sections that held
/// retired keys and beside one that is retired.
#[test]
fn an_unknown_key_still_fails_the_load() {
    for text in [
        "[context]\nnot_a_key = 1\n",
        "[cache]\nkeep_warm_hours = 24\nnot_a_key = 1\n",
        "[memory]\nnot_a_key = 1\n",
        "[threads]\nnot_a_key = 1\n",
    ] {
        let e = parse(text).expect_err(text);
        assert!(format!("{e:#}").contains("not_a_key"), "{text}: {e:#}");
    }
}

/// Only the retired key goes: its section's other keys, and a table of the
/// same name elsewhere, are untouched.
#[test]
fn strip_takes_only_the_retired_keys() {
    let mut doc: toml::Table = "[cache]\nttl = \"1h\"\nkeep_warm_hours = 1\n\
                                [tui]\nkeep_warm_hours = 3\n"
        .parse()
        .unwrap();
    let out = strip(&mut doc);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].name, "cache.keep_warm_hours");
    assert_eq!(doc["cache"].as_table().unwrap().len(), 1);
    assert!(doc["tui"]
        .as_table()
        .unwrap()
        .contains_key("keep_warm_hours"));
}

/// The template's `[cache]` is the defaults; a retired key's value, even one
/// its old reader refused, loads and is ignored (the table replaces the old
/// range checks and the profile's `keep_warm_hours`).
#[test]
fn a_retired_cache_key_loads_whatever_its_value() {
    use crate::config::cache::CacheConfig;
    assert_eq!(Config::example().cache, CacheConfig::default());
    for text in [
        "[cache]\nkeep_warm_hours = -2\n",
        "[cache]\nstub_min_tokens = -1\n",
        "[profiles.quick]\nprovider = \"anthropic\"\nmodel = \"claude-haiku-5-5\"\n\
         keep_warm_hours = 0\n",
    ] {
        let (cfg, _) = parse(text).expect(text);
        assert_eq!(cfg.ignored_keys.len(), 1, "{text}");
        assert_eq!(cfg.cache, CacheConfig::default(), "{text}");
    }
}

/// `[context] assembly` in its table form, which named places, is retired whole: inline or as a section of
/// its own, it loads, is named once, and nothing of it is read.
#[test]
fn the_assembly_table_naming_places_is_retired_whole() {
    for text in [
        "[context]\nassembly = { default = \"transcript\", places = { \"discord:dm:42\" = \"view\" } }\n",
        "[context]\nfiles = []\n[context.assembly]\ndefault = \"view\"\nplaces = { \"local\" = \"transcript\" }\n",
    ] {
        let (cfg, warnings) = parse(text).expect(text);
        assert_eq!(names(&cfg), ["context.assembly"], "{text}");
        assert_eq!(
            warnings
                .iter()
                .filter(|w| w.starts_with("context.assembly is retired: "))
                .count(),
            1,
            "{text}: {warnings:?}"
        );
    }
}

/// The prefix library's section, as an old settings file has it, loads with one warning a key (above); a key
/// it never had still fails the load, as in any section.
#[test]
fn an_old_prefix_section_loads_and_a_key_it_never_had_still_fails() {
    let (_, warnings) = parse("[prefix]\nenabled = false\nact = 0.5\n").expect("loads");
    assert_eq!(
        warnings.iter().filter(|w| w.starts_with("prefix.")).count(),
        2,
        "{warnings:?}"
    );
    let e = parse("[prefix]\nenabled = false\nwarmth = 3\n").unwrap_err();
    assert!(format!("{e:#}").contains("warmth"), "{e:#}");
}

/// The Narrative tab is always shown (issue 46): a settings file that still says `narrative`, either way, loads
/// and names it once as retired.
#[test]
fn the_narrative_key_is_retired_whatever_it_says() {
    for v in ["true", "false"] {
        let (cfg, warnings) = Config::parse(&format!("narrative = {v}\n\n{BASE}")).unwrap();
        assert_eq!(names(&cfg), ["narrative"], "{v}");
        assert_eq!(
            warnings
                .iter()
                .filter(|w| w.starts_with("narrative is retired: "))
                .count(),
            1,
            "{v}: {warnings:?}"
        );
    }
}

/// The eight voice timings are the code's (issue 46): a `[voice]` that still sets them, to anything, loads, names
/// each once, and reads as if they were unset.
#[test]
fn the_voice_timings_are_retired_and_read_as_unset() {
    let text = "[voice]\njudge_ms = 2000\npause_ms = 3000\nlead_in_ms = 0\ngap_lead_in_ms = 0\n\
                merge_ms = 0\neot_threshold = 0.9\neager_eot_threshold = 0.5\neot_timeout_ms = 6000\n";
    let (cfg, warnings) = parse(text).unwrap();
    assert_eq!(
        names(&cfg),
        [
            "voice.judge_ms",
            "voice.pause_ms",
            "voice.lead_in_ms",
            "voice.gap_lead_in_ms",
            "voice.merge_ms",
            "voice.eot_threshold",
            "voice.eager_eot_threshold",
            "voice.eot_timeout_ms"
        ]
    );
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.starts_with("voice.") && w.contains(" is retired: "))
            .count(),
        8,
        "{warnings:?}"
    );
    assert_eq!(cfg.voice, crate::voice::VoiceConfig::default());
}

/// The four cosmetic Discord switches are fixed (issue 46): a `[discord]` that still sets them loads, names each
/// once, and reads as today's defaults.
#[test]
fn the_cosmetic_discord_switches_are_retired_and_read_as_their_defaults() {
    let text = "[discord]\nedit_interval_ms = 250\nnotice_embeds = true\nstatus_line = false\n\
                app_emojis = false\n";
    let (cfg, warnings) = parse(text).unwrap();
    assert_eq!(
        names(&cfg),
        [
            "discord.edit_interval_ms",
            "discord.notice_embeds",
            "discord.status_line",
            "discord.app_emojis"
        ]
    );
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.starts_with("discord.") && w.contains(" is retired: "))
            .count(),
        4,
        "{warnings:?}"
    );
    assert_eq!(
        (cfg.discord.edit_interval_ms, cfg.discord.app_emojis),
        (1200, true)
    );
}

/// A message sent while a turn runs always joins it (issue 46): a settings file that still says
/// `[kernel] steer_queued`, either way, loads and names it once as retired.
#[test]
fn the_steer_switch_is_retired_whatever_it_says() {
    for v in ["true", "false"] {
        let (cfg, warnings) = parse(&format!("[kernel]\nsteer_queued = {v}\n")).unwrap();
        assert_eq!(names(&cfg), ["kernel.steer_queued"], "{v}");
        assert_eq!(
            warnings
                .iter()
                .filter(|w| w.starts_with("kernel.steer_queued is retired: "))
                .count(),
            1,
            "{v}: {warnings:?}"
        );
    }
}
