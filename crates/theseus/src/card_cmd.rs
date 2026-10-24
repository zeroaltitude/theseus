//! `theseus card …` (theseus-slvl.6): the owner card, proposed from the
//! imported history. `card` lists what waits as questions (Keep, Edit, Not
//! this), what was kept and declined, and the spend; `propose` runs the job
//! now (`--dry-run` says what it would send and at most cost); `keep`,
//! `edit` and `decline` answer one (`--scope` says where a kept one holds);
//! `scope` moves a kept one's record (theseus-ly75u); `done` stops the
//! nightly run. The writes
//! are the owner's acts (refused inside a job, `client::OPERATORS`).

use anyhow::Result;
use clap::Subcommand;
use theseus_client::{render, Conn};
use theseus_protocol::{
    method, CardAnswerParams, CardAnswerResult, CardDoneParams, CardDoneResult, CardListParams,
    CardListResult, CardObservedParams, CardObservedResult, CardProposal, CardProposeParams,
    CardProposeResult, ObservationState,
};

use crate::cmd::output;

#[derive(Subcommand, Debug, Default)]
pub enum CardCmd {
    /// What waits for your answer, what you kept and declined, and what the job has spent
    /// (default).
    #[default]
    List,
    /// Run the job now: read the imported feedback, SOP notes, USER files and decisions, and
    /// propose what they say about you, each as a question. Under `[card] day_usd`.
    Propose {
        /// Say what a run would send and at most cost; no model call, nothing written.
        #[arg(long)]
        dry_run: bool,
    },
    /// Keep a proposal: it becomes a standing record every prompt of its scope carries.
    Keep {
        id: String,
        /// Where it holds, instead of the proposal's scope: everywhere, surface:NAME
        /// (terminal, discord, voice, cockpit, mcp), place:TARGET, persona:NAME, or
        /// session:ID.
        #[arg(long)]
        scope: Option<String>,
    },
    /// Keep a proposal with your own words.
    Edit {
        id: String,
        /// Where it holds, instead of the proposal's scope (as for keep).
        #[arg(long)]
        scope: Option<String>,
        #[arg(required = true)]
        text: Vec<String>,
    },
    /// Move a kept proposal's record to another scope: the same words and source, the old
    /// record ended. SCOPE as for keep's --scope.
    Scope { id: String, scope: String },
    /// "Not this": the proposal is never made again for those sources.
    Decline { id: String },
    /// End it: the record a Stale question asks about no longer holds, so it is revoked.
    End { id: String },
    /// What was noticed about you in conversation, kept privately and not yet asked about
    /// (`--used` and `--dropped` list the others).
    Observed {
        /// Every state's, not only the open ones.
        #[arg(long, conflicts_with_all = ["used", "dropped"])]
        all: bool,
        /// Those a question rests on.
        #[arg(long, conflicts_with = "dropped")]
        used: bool,
        /// Those set aside.
        #[arg(long)]
        dropped: bool,
    },
    /// Stop the nightly run: the card is done. `--undo` takes it back.
    Done {
        #[arg(long)]
        undo: bool,
    },
}

pub async fn run(conn: &mut Conn, json: bool, cmd: CardCmd) -> Result<()> {
    match cmd {
        CardCmd::List => {
            let v = conn
                .request(
                    method::CARD_LIST,
                    serde_json::to_value(CardListParams::default())?,
                )
                .await?;
            output(json, v, |r: CardListResult| {
                for l in list_lines(&r) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        CardCmd::Observed { all, used, dropped } => {
            let state = match (all, used, dropped) {
                (true, ..) => None,
                (_, true, _) => Some(ObservationState::Used),
                (_, _, true) => Some(ObservationState::Dropped),
                _ => Some(ObservationState::Open),
            };
            // The wire lists one state; `--all` asks each and joins them.
            let mut r = CardObservedResult::default();
            for s in [
                ObservationState::Open,
                ObservationState::Used,
                ObservationState::Dropped,
            ] {
                if state.is_some_and(|w| w != s) {
                    continue;
                }
                let p = CardObservedParams {
                    state: Some(s),
                    ..Default::default()
                };
                let v = conn
                    .request(method::CARD_OBSERVED, serde_json::to_value(&p)?)
                    .await?;
                let one: CardObservedResult = serde_json::from_value(v)?;
                r.observations.extend(one.observations);
                (r.open, r.used, r.dropped, r.observe) =
                    (one.open, one.used, one.dropped, one.observe);
            }
            r.observations
                .sort_by(|a, b| (b.at_ms, &b.id).cmp(&(a.at_ms, &a.id)));
            output(json, serde_json::to_value(&r)?, |r: CardObservedResult| {
                for l in observed_lines(&r, theseus_protocol::now_unix_ms()) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        CardCmd::Propose { dry_run } => {
            let p = CardProposeParams {
                dry_run,
                ..Default::default()
            };
            let v = conn
                .request(method::CARD_PROPOSE, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: CardProposeResult| {
                for l in propose_lines(&r, dry_run) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        CardCmd::Keep { id, scope } => answer(conn, json, id, "keep", None, scope).await,
        CardCmd::Edit { id, scope, text } => {
            answer(conn, json, id, "edit", Some(text.join(" ")), scope).await
        }
        CardCmd::Scope { id, scope } => answer(conn, json, id, "scope", None, Some(scope)).await,
        CardCmd::Decline { id } => answer(conn, json, id, "decline", None, None).await,
        CardCmd::End { id } => answer(conn, json, id, "end", None, None).await,
        CardCmd::Done { undo } => {
            let p = CardDoneParams {
                done: !undo,
                ..Default::default()
            };
            let v = conn
                .request(method::CARD_DONE, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: CardDoneResult| {
                println!(
                    "{}",
                    if r.done {
                        "The card is done: the nightly run stops. `theseus card propose` still runs it."
                    } else {
                        "The nightly run is back on."
                    }
                );
                Ok(())
            })
        }
    }
}

async fn answer(
    conn: &mut Conn,
    json: bool,
    id: String,
    option: &str,
    text: Option<String>,
    scope: Option<String>,
) -> Result<()> {
    let p = CardAnswerParams {
        id,
        option: option.into(),
        text,
        scope,
        ..Default::default()
    };
    let v = conn
        .request(method::CARD_ANSWER, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: CardAnswerResult| {
        println!(
            "{}",
            if option == "end" {
                ended_line(&r)
            } else {
                answered_line(&r)
            }
        );
        Ok(())
    })
}

/// What an End it says: the record a Stale question named is revoked.
pub fn ended_line(r: &CardAnswerResult) -> String {
    format!(
        "Ended: {} is off your card; every prompt of its scope stops carrying it from its next turn.",
        r.proposal.record.as_deref().unwrap_or("the record")
    )
}

/// What an answer says.
pub fn answered_line(r: &CardAnswerResult) -> String {
    let p = &r.proposal;
    match &r.record {
        Some(rec) if p.ask == theseus_protocol::CardAsk::Stale && p.kept_text.is_none() => {
            format!(
                "Kept: {} {} still holds for {}.",
                rec.kind.name(),
                rec.id,
                rec.scope.words()
            )
        }
        Some(rec) => format!(
            "Kept as {} {} for {}: every prompt of that scope carries it from its next turn.",
            rec.kind.name(),
            rec.id,
            rec.scope.words()
        ),
        None => format!(
            "Not this: {} will not be proposed again for its {} sources.",
            short(&p.id),
            p.sources.len()
        ),
    }
}

fn short(id: &str) -> &str {
    &id[id.len().saturating_sub(8)..]
}

/// `theseus card propose`'s lines.
pub fn propose_lines(r: &CardProposeResult, dry_run: bool) -> Vec<String> {
    let mut out = vec![format!(
        "Read {} evidence episodes ({} partner-confidential left unread): {} clusters, {} new.",
        r.evidence, r.withheld, r.clusters, r.fresh
    )];
    if dry_run {
        out.push(format!(
            "A run would make up to {} calls, at most ${:.2} (the day's line is ${:.2}, ${:.2} spent).",
            r.fresh.div_ceil(8).min(8),
            r.at_most_usd,
            r.day_usd,
            r.spent_today_usd
        ));
    } else {
        out.push(format!(
            "{} calls, ${:.2} spent (${:.2} of ${:.2} today): {} proposed, {} statements dropped{}.",
            r.calls,
            r.spent_usd,
            r.spent_today_usd,
            r.day_usd,
            r.proposed.len(),
            r.dropped,
            if r.expired > 0 {
                format!(", {} unanswered expired to Not this", r.expired)
            } else {
                String::new()
            }
        ));
    }
    if r.setup + r.one_task > 0 {
        out.push(format!(
            "Left out: {} lines of an assistant's setup (its instructions to itself), {} of one task, \
             repository or run.",
            r.setup, r.one_task
        ));
    }
    for c in &r.conflicts {
        out.push(format!("Conflict: {c}."));
    }
    if let Some(why) = &r.stopped {
        out.push(format!("Stopped: {why}."));
    }
    for p in &r.proposed {
        out.push(format!("  {}  {}", short(&p.id), p.question));
    }
    out
}

/// `theseus card`'s lines: the open questions, then the counts and the spend.
pub fn list_lines(r: &CardListResult) -> Vec<String> {
    let mut out = Vec::new();
    if r.open.is_empty() {
        out.push("No proposals wait. `theseus card propose` reads your history for some.".into());
    }
    for p in &r.open {
        // A live question (theseus-6f8t0) says what it asks and where from.
        let kind = if p.observations.is_empty() && p.ask.is_add() {
            String::new()
        } else {
            format!("{} from your conversations, ", ask_name(p.ask))
        };
        out.push(format!(
            "{}  {}  ({kind}asks until {})",
            short(&p.id),
            p.question,
            render::fmt_date(p.expires_ms)
        ));
    }
    out.push(String::new());
    out.push(format!(
        "{} waiting (at most {}), {} kept, {} declined. Spent ${:.2} today of ${:.2}, ${:.2} in all.{}",
        r.open.len(),
        r.max_open,
        r.kept_total,
        r.declined_total,
        r.spent_today_usd,
        r.day_usd,
        r.spent_total_usd,
        if r.done {
            " The card is done: no nightly run."
        } else if r.every_days == 0 {
            " No nightly run ([card] every_days = 0)."
        } else {
            ""
        }
    ));
    for (label, list) in [("Kept", &r.kept), ("Declined", &r.declined)] {
        for p in list.iter().take(5) {
            out.push(format!("  {label}: {}", line_of(p)));
        }
    }
    out
}

/// `theseus card observed`'s lines: one for each observation (the handles of
/// its messages, its kind, its words, its age), or one saying there are none.
pub fn observed_lines(r: &CardObservedResult, now_ms: u64) -> Vec<String> {
    if r.observations.is_empty() {
        return vec![format!(
            "Nothing observed here ({} open, {} used, {} dropped). [card] observe is {}.",
            r.open,
            r.used,
            r.dropped,
            if r.observe { "on" } else { "off" }
        )];
    }
    r.observations
        .iter()
        .map(|o| {
            let ids: Vec<&str> = o.nodes.iter().map(String::as_str).collect();
            let handles = theseus_protocol::cite::handles_for(&ids).join(" ");
            let days = now_ms.saturating_sub(o.at_ms) / 86_400_000;
            let age = if days == 0 {
                "today".to_string()
            } else {
                format!("{days}d ago")
            };
            format!(
                "{}  {handles}  {}  {}  ({age})",
                short(&o.id),
                serde_json::to_value(o.kind)
                    .ok()
                    .and_then(|k| k.as_str().map(str::to_string))
                    .unwrap_or_default(),
                o.text
            )
        })
        .collect()
}

/// What a question asks, in a word.
fn ask_name(a: theseus_protocol::CardAsk) -> &'static str {
    match a {
        theseus_protocol::CardAsk::Add => "add",
        theseus_protocol::CardAsk::Pattern => "pattern",
        theseus_protocol::CardAsk::Stale => "stale",
    }
}

fn line_of(p: &CardProposal) -> String {
    format!(
        "{}  {}  ({} sources{})",
        short(&p.id),
        p.kept_text.as_deref().unwrap_or(&p.text),
        p.support,
        p.answered_by
            .as_deref()
            .filter(|b| *b == "expiry")
            .map_or(String::new(), |_| ", expired".into())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::{CardSource, ProposalState, StandingKind, StandingScope};

    fn prop(id: &str, text: &str, state: ProposalState) -> CardProposal {
        CardProposal {
            id: id.into(),
            kind: StandingKind::Preference,
            scope: StandingScope::Everywhere,
            text: text.into(),
            question: theseus_protocol::card_question(text, &StandingScope::Everywhere, 14),
            options: theseus_protocol::card_options(),
            sources: vec![CardSource {
                episode: "ep_a".into(),
                node: "n".into(),
                kind: "decision".into(),
                at_ms: 1,
            }],
            support: 14,
            created_ms: 0,
            expires_ms: 1_790_000_000_000,
            state,
            run: "crd_1".into(),
            answered_ms: None,
            answered_by: None,
            kept_text: None,
            standing_id: None,
            ask: Default::default(),
            record: None,
            observations: vec![],
            posted_ms: None,
        }
    }

    #[test]
    fn observed_lines_name_the_messages_the_kind_the_words_and_the_age() {
        use theseus_protocol::{CardObservation, ObservationKind, ObservationState};
        let none = CardObservedResult {
            observe: true,
            ..Default::default()
        };
        assert_eq!(
            observed_lines(&none, 0),
            ["Nothing observed here (0 open, 0 used, 0 dropped). [card] observe is on."]
        );
        let one = CardObservedResult {
            observations: vec![CardObservation {
                id: "cob_00aa11bb".into(),
                session: "ses_1".into(),
                nodes: vec!["nod_000000abcdef".into()],
                kind: ObservationKind::Fact,
                text: "moors at the north jetty".into(),
                at_ms: 0,
                contradicts: None,
                state: ObservationState::Open,
                proposal: None,
            }],
            open: 1,
            ..Default::default()
        };
        let lines = observed_lines(&one, 3 * 86_400_000);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0],
            "00aa11bb  m:abcdef  fact  moors at the north jetty  (3d ago)"
        );
    }

    fn listed(open: Vec<CardProposal>) -> CardListResult {
        CardListResult {
            open,
            max_open: 30,
            day_usd: 2.0,
            every_days: 7,
            ..CardListResult::default()
        }
    }

    #[test]
    fn the_list_shows_each_question_with_its_short_id_and_the_counts() {
        let r = listed(vec![prop(
            "cdp_0000000000000000aaaaaaaa11112222",
            "wants your opinion when you have one",
            ProposalState::Open,
        )]);
        let lines = list_lines(&r);
        assert!(lines[0].starts_with("11112222  Add to your card: 'wants your opinion when you have one' (from 14 sources)? [Keep] [Edit] [Not this]"), "{lines:?}");
        assert!(
            lines
                .last()
                .unwrap()
                .contains("1 waiting (at most 30), 0 kept, 0 declined. Spent $0.00 today of $2.00"),
            "{lines:?}"
        );
    }

    #[test]
    fn an_empty_list_says_how_to_fill_it_and_a_done_card_says_so() {
        let mut r = listed(vec![]);
        assert!(list_lines(&r)[0].contains("theseus card propose"));
        r.done = true;
        assert!(list_lines(&r)
            .last()
            .unwrap()
            .ends_with("The card is done: no nightly run."));
        r.done = false;
        r.every_days = 0;
        assert!(list_lines(&r).last().unwrap().contains("No nightly run"));
    }

    #[test]
    fn a_run_says_what_it_read_spent_and_why_it_stopped() {
        let r = CardProposeResult {
            evidence: 6,
            withheld: 1,
            clusters: 5,
            fresh: 4,
            calls: 1,
            proposed: vec![prop(
                "cdp_x_aaaabbbb",
                "wants your opinion",
                ProposalState::Open,
            )],
            dropped: 2,
            expired: 1,
            spent_usd: 0.01,
            spent_today_usd: 0.03,
            day_usd: 2.0,
            stopped: Some("budget: $0.00 of [card] day_usd $2.00 is left".into()),
            ..CardProposeResult::default()
        };
        let lines = propose_lines(&r, false);
        assert_eq!(
            lines[0],
            "Read 6 evidence episodes (1 partner-confidential left unread): 5 clusters, 4 new."
        );
        assert!(lines[1].contains("1 calls, $0.01 spent ($0.03 of $2.00 today): 1 proposed, 2 statements dropped, 1 unanswered expired to Not this."), "{lines:?}");
        assert!(lines[2].starts_with("Stopped: budget"), "{lines:?}");
        assert!(
            lines[3].starts_with("  aaaabbbb  Add to your card"),
            "{lines:?}"
        );
        let dry = propose_lines(
            &CardProposeResult {
                fresh: 20,
                at_most_usd: 0.05,
                day_usd: 2.0,
                ..CardProposeResult::default()
            },
            true,
        );
        assert!(dry[1].contains("up to 3 calls, at most $0.05"), "{dry:?}");
    }

    #[test]
    fn an_answer_says_what_became_of_the_proposal() {
        let kept = CardAnswerResult {
            proposal: prop("cdp_x_aaaabbbb", "t", ProposalState::Kept),
            record: Some(theseus_protocol::StandingRecord {
                id: "std_1".into(),
                kind: StandingKind::Preference,
                text: "t".into(),
                scope: StandingScope::Everywhere,
                since_ms: 1,
                until_ms: None,
                source: "card:cdp_x_aaaabbbb".into(),
                ends: None,
            }),
        };
        assert!(answered_line(&kept).starts_with("Kept as preference std_1 for everywhere"));
        let no = CardAnswerResult {
            proposal: prop("cdp_x_aaaabbbb", "t", ProposalState::Declined),
            record: None,
        };
        assert_eq!(
            answered_line(&no),
            "Not this: aaaabbbb will not be proposed again for its 1 sources."
        );
    }

    #[test]
    fn end_parses_and_a_live_question_says_what_it_asks_and_where_from() {
        #[derive(clap::Parser)]
        struct T {
            #[command(subcommand)]
            c: CardCmd,
        }
        use clap::Parser as _;
        let t = T::try_parse_from(["t", "end", "cdp_x_aaaabbbb"]).unwrap();
        assert!(matches!(t.c, CardCmd::End { id } if id == "cdp_x_aaaabbbb"));
        assert!(T::try_parse_from(["t", "end"]).is_err());
        let mut live = prop("cdp_x_11112222", "t", ProposalState::Open);
        live.ask = theseus_protocol::CardAsk::Stale;
        live.observations = vec!["cob_1".into()];
        let plain = prop("cdp_x_33334444", "t", ProposalState::Open);
        let lines = list_lines(&listed(vec![live, plain]));
        assert!(
            lines[0].contains("(stale from your conversations, asks until "),
            "{lines:?}"
        );
        assert!(!lines[1].contains("conversations"), "{lines:?}");
        let ended = CardAnswerResult {
            proposal: CardProposal {
                record: Some("std_1".into()),
                ..prop("cdp_x_aaaabbbb", "t", ProposalState::Declined)
            },
            record: None,
        };
        assert!(ended_line(&ended).starts_with("Ended: std_1 is off your card"));
    }
}
