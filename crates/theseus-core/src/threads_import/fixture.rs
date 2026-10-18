//! The invented export the thread import's tests read (theseus-71ib): episodes
//! of a make-believe tide survey, as the pipeline writes them, with their
//! times a number of days before `base_ms`. Nothing here is the owner's: the
//! names, channels, repositories and ids are made up. The same builder gives
//! the committed fixture file (`tests/fixtures/threads-import/openclaw.jsonl`,
//! at [`BASE_MS`]) and the tests' own, at the real clock.
//!
//! What it holds, by stream (the tests name each):
//! - `#tide-gauges`, a Discord channel: three episodes of the agent `wren`
//!   (a persona the tests configure), owner lines naming a repository, a pull
//!   request, issues, a Beads id, a branch, files and a host.
//! - `#boat-ops`, a Slack channel: one episode of the agent `gull`, who is no
//!   persona.
//! - `#archive`, a Discord channel whose last owner line is a year old.
//! - `#kestrel-deal`, a Discord channel of partner-confidential episodes,
//!   whose lines carry [`SECRET`] and a repository the working set must not
//!   learn.
//! - the DMs: nine owner lines about moorings (`harbor/moorings`, enough for
//!   a cluster), three about irrigation (too few), and one episode with no
//!   topic.
//! - a CLI session and a heartbeat, which are no stream.

use serde_json::{json, Value};

use crate::import::episode::hash_of;

/// The committed fixture's "now": 2026-10-10T12:00:00Z.
pub const BASE_MS: u64 = 1_791_633_600_000;
const DAY: u64 = 86_400_000;
pub const TAG: &str = "tern-export";
/// A line of partner text no title, topic, working set, card or model
/// request may hold.
pub const SECRET: &str = "KESTREL-TERM-SHEET-LINE";
/// A repository only a partner episode names.
pub const SECRET_REPO: &str = "kestrel-co/term-sheet";

/// `ms` as an RFC 3339 UTC time.
pub fn rfc3339(ms: u64) -> String {
    crate::aws::hands::start::utc(ms)
}

struct Ep<'a> {
    n: u64,
    agent: Option<&'a str>,
    place: (&'a str, Option<&'a str>, Option<&'a str>),
    sensitivity: &'a str,
    partner: Option<&'a str>,
    topics: &'a [&'a str],
    days_ago: u64,
    summary: Option<&'a str>,
    owner: &'a [&'a str],
}

fn episode(base_ms: u64, e: &Ep<'_>) -> Value {
    let start = base_ms - e.days_ago * DAY;
    let mut messages = Vec::new();
    for (i, t) in e.owner.iter().enumerate() {
        let at = start + (i as u64) * 120_000;
        messages.push(json!({"idx": messages.len(), "time": rfc3339(at), "author": "sable",
            "integrity": "operator", "text": t,
            "unit": format!("{TAG}-{}-{i}", e.n), "sha256": format!("{:064x}", e.n * 1000 + i as u64)}));
        let reply = start + (i as u64) * 120_000 + 30_000;
        messages.push(json!({"idx": messages.len(), "time": rfc3339(reply), "author": "agent:main",
            "integrity": "agent", "text": format!("Noted, line {i} of episode {}.", e.n),
            "unit": format!("{TAG}-{}-{i}r", e.n), "sha256": format!("{:064x}", e.n * 1000 + 500 + i as u64)}));
    }
    let end = start + (messages.len() as u64) * 120_000;
    let cites: Vec<usize> = (0..messages.len().min(2)).collect();
    let mut v = json!({
        "format": 1, "import_tag": TAG, "episode_id": format!("ep_{:064x}", 0x7e47 + e.n),
        "source": "openclaw-store", "agent": e.agent,
        "place": {"kind": e.place.0, "name": e.place.1, "id": e.place.2},
        "as_of": {"start": rfc3339(start), "end": rfc3339(end)},
        "labels": {"sensitivity": e.sensitivity, "partner": e.partner,
                   "topic": e.topics, "book_hint": null, "credential_redacted": false},
        "summary": e.summary.map(|s| json!({"text": s, "cites": cites, "model": "claude-opus-5-5"})),
        "messages": messages,
    });
    v["hash"] = json!(hash_of(&v));
    v
}

/// The channels' episodes.
fn channel_episodes() -> Vec<Ep<'static>> {
    vec![
        Ep {
            n: 1,
            agent: Some("wren"),
            place: ("discord-channel", Some("#tide-gauges"), Some("4101")),
            sensitivity: "company-confidential",
            partner: None,
            topics: &["projects/tide-gauges"],
            days_ago: 3,
            summary: Some("The gauge sync moved to lane/gauge-sync; the drift fix is pull request 12."),
            owner: &[
                "Gauge 4 drifts after dusk, see https://github.com/reef-lab/gauge-log/pull/12 and #12",
                "Tracking it as theseus-ab12; I pushed lane/gauge-sync.",
            ],
        },
        Ep {
            n: 2,
            agent: Some("wren"),
            place: ("discord-channel", Some("#tide-gauges"), Some("4101")),
            sensitivity: "personal",
            partner: None,
            topics: &["projects/tide-gauges", "ops/calibration"],
            days_ago: 2,
            summary: None,
            owner: &[
                "Edit src/gauge.rs and notes/drift.md, then check https://tides.example.org/api/readings",
                "Issue #14 is the calibration table.",
            ],
        },
        Ep {
            n: 3,
            agent: Some("wren"),
            place: ("discord-channel", Some("#tide-gauges"), Some("4101")),
            sensitivity: "company-confidential",
            partner: None,
            topics: &["projects/tide-gauges"],
            days_ago: 1,
            summary: None,
            owner: &["Ship the gauge fix tonight."],
        },
        Ep {
            n: 4,
            agent: Some("gull"),
            place: ("slack-channel", Some("#boat-ops"), Some("C0BOAT")),
            sensitivity: "personal",
            partner: None,
            topics: &["ops/boats"],
            days_ago: 5,
            summary: None,
            owner: &["The skiff needs its mooring line replaced."],
        },
        Ep {
            n: 5,
            agent: Some("wren"),
            place: ("discord-channel", Some("#archive"), Some("4999")),
            sensitivity: "personal",
            partner: None,
            topics: &["archive/misc"],
            days_ago: 400,
            summary: None,
            owner: &["Old business, closed long ago."],
        },
        Ep {
            n: 6,
            agent: Some("wren"),
            place: ("discord-channel", Some("#kestrel-deal"), Some("4777")),
            sensitivity: "partner-confidential",
            partner: Some("partner-candidate:kestrel"),
            topics: &["deals/kestrel"],
            days_ago: 2,
            summary: Some("KESTREL-TERM-SHEET-LINE: the partner's terms."),
            owner: &[
                "KESTREL-TERM-SHEET-LINE: they want the second tranche early, see https://github.com/kestrel-co/term-sheet/pull/9",
                "Branch fix/kestrel-terms holds the redline; ask theseus-kk77.",
            ],
        },
    ]
}

/// The DMs' episodes (two clusters and one with no topic), and a CLI session, which is no stream.
fn dm_episodes() -> Vec<Ep<'static>> {
    vec![
        Ep {
            n: 7,
            agent: Some("wren"),
            place: ("dm", Some("sable"), Some("dm-1")),
            sensitivity: "personal",
            partner: None,
            topics: &["harbor/moorings/winter"],
            days_ago: 4,
            summary: None,
            owner: &[
                "Which moorings need replacing before winter?",
                "Check the harbour master's list.",
                "The north buoy is the worst.",
            ],
        },
        Ep {
            n: 8,
            agent: Some("wren"),
            place: ("dm", Some("sable"), Some("dm-1")),
            sensitivity: "personal",
            partner: None,
            topics: &["harbor/moorings"],
            days_ago: 3,
            summary: None,
            owner: &[
                "Price the chain for the north buoy.",
                "And the shackles.",
                "Order two spares.",
            ],
        },
        Ep {
            n: 9,
            agent: Some("wren"),
            place: ("dm", Some("sable"), Some("dm-1")),
            sensitivity: "company-confidential",
            partner: None,
            topics: &["harbor/moorings"],
            days_ago: 1,
            summary: None,
            owner: &[
                "Book the diver for the 14th.",
                "Confirm the tide table for that day.",
                "Send the mooring plan to the crew.",
            ],
        },
        Ep {
            n: 10,
            agent: Some("wren"),
            place: ("dm", Some("sable"), Some("dm-1")),
            sensitivity: "personal",
            partner: None,
            topics: &["garden/irrigation"],
            days_ago: 2,
            summary: None,
            owner: &[
                "Set the drip timer.",
                "Check the hose.",
                "Water the beds at dawn.",
            ],
        },
        Ep {
            n: 11,
            agent: Some("wren"),
            place: ("dm", Some("sable"), Some("dm-1")),
            sensitivity: "personal",
            partner: None,
            topics: &[],
            days_ago: 1,
            summary: None,
            owner: &["Remind me about the dentist."],
        },
        Ep {
            n: 12,
            agent: Some("main"),
            place: ("cli", Some("/work/reef"), None),
            sensitivity: "personal",
            partner: None,
            topics: &["projects/reef"],
            days_ago: 1,
            summary: None,
            owner: &["Run the survey build."],
        },
    ]
}

/// The export, as episode values, in file order.
pub fn export(base_ms: u64) -> Vec<Value> {
    let mut eps = channel_episodes();
    eps.extend(dm_episodes());
    eps.iter().map(|e| episode(base_ms, e)).collect()
}

/// The export as the file's text: a line each.
pub fn export_text(base_ms: u64) -> String {
    export(base_ms)
        .iter()
        .map(|v| serde_json::to_string(v).expect("an episode"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}
