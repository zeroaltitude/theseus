//! The live task board (theseus-2j2ka): one Components V2 message a place,
//! pinned, edited when a task changes state and never posted anew, so it
//! never pings. Pure: the work views of a place in, validated components out.
//!
//! ```text
//! 📋 Tasks · 🔴 1 needs you · ⚙️ 3 running · ✅ 2 done since you looked
//! ─────
//! 🔴 Needs you
//! Beta summary …a1b2c3                                  [ Answer ↗ ]
//! fs.write create projects/beta-summary.txt (40 B) · asked 4 minutes ago
//! ─────
//! ⚙️ Now
//! ⚙️ Rename the modules …d4e5f6 · fs.edit · turn 4 · $0.42 of $2 · since 9 minutes ago
//! ─────
//! ✅ Done, not yet seen
//! ✅ Count the fixtures …c7d8e9 · done 2 hours ago
//! ─────
//! [ ✓ Clear done ] [ 🔕 Quiet 1 h ]
//! [ Open a task… ▾ ]
//! ```
//!
//! Times are Discord's relative timestamps (`<t:…:R>`), which tick by
//! themselves, so a quiet board is never edited for the clock. A board that
//! does not fit Discord's 40 components or 4,000 characters folds its
//! sections' tails into `+N more · /tasks`: the needs-you list keeps the
//! longest waiting first, the working and done lists their newest.

use serde_json::Value;
use theseus_protocol::usd;
use theseus_protocol::work::{WorkState, WorkView};

use super::clip;
use super::v2::{button, container, row, section, select, separator, text, validate, Look};

/// A button's `custom_id` prefix: every control of the board starts with it.
pub const PREFIX: &str = "tb:";
/// Clear the done tasks.
pub const CLEAR: &str = "tb:clear";
/// Quiet the place's task pings for an hour, or end that.
pub const QUIET: &str = "tb:quiet";
/// The select that opens a task; the button of a needs-you item is `tb:o:<id>`.
pub const PICK: &str = "tb:pick";
/// `tb:o:` + a task's id.
pub const OPEN: &str = "tb:o:";

/// The accent of the board's box.
const RED: u32 = 0xE5_48_4D;
const BLUE: u32 = 0x3E_63_DD;
const GREEN: u32 = 0x30_A4_6C;

/// How many of each part to try first.
const NEEDS: usize = 7;
const WORKING: usize = 12;
const DONE: usize = 8;

/// A short id: the last six characters, as `/cancel` takes it.
pub fn short(id: &str) -> &str {
    let n = id.chars().count();
    let skip = n.saturating_sub(6);
    id.char_indices().nth(skip).map_or(id, |(i, _)| &id[i..])
}

/// Discord's relative timestamp of `ms`.
pub fn ago(ms: u64) -> String {
    format!("<t:{}:R>", ms / 1000)
}

/// What a board is drawn from.
pub struct Parts<'a> {
    pub needs: &'a [WorkView],
    pub working: &'a [WorkView],
    pub done: &'a [WorkView],
    /// The place's task pings are quiet until then, unix ms.
    pub quiet_until_ms: Option<u64>,
    /// Where each task's thread or forum post is, by task id (theseus-i3do6):
    /// a row that has one ends with a link to it.
    pub links: &'a std::collections::HashMap<String, String>,
}

/// The header line: the counts, `0`s left out.
pub fn headline(needs: usize, running: usize, done: usize) -> String {
    let mut parts = Vec::new();
    if needs > 0 {
        parts.push(format!("🔴 {needs} needs you"));
    }
    if running > 0 {
        parts.push(format!("⚙️ {running} running"));
    }
    if done > 0 {
        parts.push(format!("✅ {done} done since you looked"));
    }
    if parts.is_empty() {
        "all clear".into()
    } else {
        parts.join(" · ")
    }
}

fn title_of(v: &WorkView) -> String {
    let t = v.title.trim();
    if t.is_empty() {
        "(untitled task)".into()
    } else {
        clip(&t.replace('\n', " "), 60)
    }
}

/// A needs-you item's second line: the question, or what blocks it.
fn ask_line(v: &WorkView) -> String {
    let what = match v.question.as_ref() {
        Some(q) => clip(&q.prompt.replace('\n', " "), 160),
        None => clip(&v.attention.label, 160),
    };
    let since = v.question.as_ref().map_or(v.state_since_ms, |q| {
        if q.asked_at_ms > 0 {
            q.asked_at_ms
        } else {
            v.state_since_ms
        }
    });
    format!("{what} · asked {}", ago(since))
}

/// What a working task is doing: its doing's label, its turn, its spend.
fn working_line(v: &WorkView) -> String {
    let mut bits = vec![format!("⚙️ **{}** `…{}`", title_of(v), short(&v.id))];
    let label = v
        .doing
        .as_ref()
        .map(|d| d.label.clone())
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| v.attention.label.clone());
    if !label.is_empty() {
        bits.push(clip(&label, 60));
    }
    if let Some(d) = v
        .doing
        .as_ref()
        .filter(|d| d.turn > 0 && !label.starts_with("turn "))
    {
        bits.push(format!("turn {}", d.turn));
    }
    if let Some(c) = v.cost.filter(|c| c.spent_usd > 0.0 || c.limit_usd > 0.0) {
        bits.push(if c.limit_usd > 0.0 {
            format!("{} of {}", usd(c.spent_usd), usd(c.limit_usd))
        } else {
            usd(c.spent_usd)
        });
    }
    if v.state == WorkState::Queued {
        bits.push("queued".into());
    }
    if v.started_at_ms > 0 {
        bits.push(format!("since {}", ago(v.started_at_ms)));
    }
    bits.join(" · ")
}

fn done_line(v: &WorkView) -> String {
    let at = v.ended_at_ms.unwrap_or(v.state_since_ms);
    let (mark, word) = match v.state {
        WorkState::Failed => ("❌", "failed"),
        WorkState::Cancelled => ("⛔", "cancelled"),
        WorkState::Expired => ("⌛", "expired"),
        _ => ("✅", "done"),
    };
    let mut s = format!(
        "{mark} **{}** `…{}` · {word} {}",
        title_of(v),
        short(&v.id),
        ago(at)
    );
    if v.state == WorkState::Failed {
        if let Some(r) = v
            .report
            .as_ref()
            .and_then(|r| r.reason.as_deref())
            .filter(|r| !r.is_empty())
        {
            s.push_str(&format!(" · {}", clip(&r.replace('\n', " "), 80)));
        }
    }
    s
}

/// No task has a thread or a post: the links of a place that makes none.
#[cfg(test)]
pub fn no_links() -> &'static std::collections::HashMap<String, String> {
    static NONE: std::sync::LazyLock<std::collections::HashMap<String, String>> =
        std::sync::LazyLock::new(std::collections::HashMap::new);
    &NONE
}

/// ` · ↗` linking the task's thread or post, if it has one. The angle
/// brackets keep Discord from drawing an embed for it.
fn link_of(p: &Parts<'_>, v: &WorkView) -> String {
    p.links
        .get(&v.id)
        .map(|url| format!(" · [↗ thread](<{url}>)"))
        .unwrap_or_default()
}

fn more(n: usize) -> String {
    format!("-# +{n} more · /tasks")
}

/// The board's components, within Discord's limits. `caps` are the most of
/// each part to show; `render` lowers them until the message fits.
fn build(p: &Parts<'_>, caps: [usize; 3]) -> Vec<Value> {
    let [cn, cw, cd] = caps;
    let mut inner = vec![text(format!(
        "📋 **Tasks** · {}{}",
        headline(p.needs.len(), p.working.len(), p.done.len()),
        match p.quiet_until_ms {
            Some(t) => format!(" · 🔕 quiet until {}", format_args!("<t:{}:t>", t / 1000)),
            None => String::new(),
        }
    ))];
    if !p.needs.is_empty() {
        inner.push(separator());
        inner.push(text("**🔴 Needs you**"));
        for v in p.needs.iter().take(cn) {
            inner.push(section(
                vec![text(format!(
                    "**{}** `…{}`\n{}{}",
                    title_of(v),
                    short(&v.id),
                    ask_line(v),
                    link_of(p, v)
                ))],
                button(format!("{OPEN}{}", v.id), "Answer ↗", Look::Primary),
            ));
        }
        if p.needs.len() > cn {
            inner.push(text(more(p.needs.len() - cn)));
        }
    }
    if !p.working.is_empty() {
        inner.push(separator());
        // The newest first; the oldest fold.
        let mut lines = vec!["**⚙️ Now**".to_string()];
        lines.extend(
            p.working
                .iter()
                .rev()
                .take(cw)
                .map(|v| format!("{}{}", working_line(v), link_of(p, v))),
        );
        if p.working.len() > cw {
            lines.push(more(p.working.len() - cw));
        }
        inner.push(text(lines.join("\n")));
    }
    if !p.done.is_empty() {
        inner.push(separator());
        let mut lines = vec!["**✅ Done, not yet seen**".to_string()];
        lines.extend(
            p.done
                .iter()
                .take(cd)
                .map(|v| format!("{}{}", done_line(v), link_of(p, v))),
        );
        if p.done.len() > cd {
            lines.push(more(p.done.len() - cd));
        }
        inner.push(text(lines.join("\n")));
    }
    inner.push(separator());
    let quiet = if p.quiet_until_ms.is_some() {
        button(QUIET, "🔔 Unquiet", Look::Secondary)
    } else {
        button(QUIET, "🔕 Quiet 1 h", Look::Secondary)
    };
    inner.push(row(vec![
        button(CLEAR, "✓ Clear done", Look::Secondary),
        quiet,
    ]));
    let options: Vec<(String, String)> = p
        .needs
        .iter()
        .take(cn)
        .map(|v| (format!("🔴 {}", title_of(v)), v.id.clone()))
        .chain(
            p.working
                .iter()
                .rev()
                .take(cw)
                .map(|v| (format!("⚙️ {}", title_of(v)), v.id.clone())),
        )
        .chain(
            p.done
                .iter()
                .take(cd)
                .map(|v| (format!("✅ {}", title_of(v)), v.id.clone())),
        )
        .take(25)
        .collect();
    if !options.is_empty() {
        inner.push(select(PICK, "Open a task…", &options));
    }
    let accent = if !p.needs.is_empty() {
        RED
    } else if !p.working.is_empty() {
        BLUE
    } else {
        GREEN
    };
    vec![container(Some(accent), inner)]
}

/// The board for these parts: at most 40 components and 4,000 characters,
/// the tails of its parts folded into `+N more · /tasks` as much as it takes.
pub fn render(p: &Parts<'_>) -> Vec<Value> {
    let mut caps = [NEEDS, WORKING, DONE];
    loop {
        let c = build(p, caps);
        if validate(&c).is_ok() {
            return c;
        }
        // Lower the part with the most lines shown; a board that cannot fit
        // with nothing shown is its header alone.
        let shown = [
            p.needs.len().min(caps[0]),
            p.working.len().min(caps[1]),
            p.done.len().min(caps[2]),
        ];
        match (0..3)
            .max_by_key(|&i| (shown[i], 3 - i))
            .filter(|&i| shown[i] > 0)
        {
            Some(i) => caps[i] -= 1,
            None => return c,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::v2::{count, text_len, texts};
    use theseus_protocol::work::{
        Cost, Doing, DoingKind, Question, QuestionKind, WorkKind, WorkRef,
    };
    use theseus_protocol::{Attention, Level};

    const NOW: u64 = 1_790_000_000_000;

    fn view(n: usize, state: WorkState) -> WorkView {
        let id = format!("exe_01J0000000000000000000{n:04}");
        WorkView {
            id,
            kind: WorkKind::Task,
            position: n as u64,
            at_ms: NOW,
            parent: None,
            root: "exe_chat".into(),
            session_id: Some(format!("ses_{n}")),
            title: format!("Task number {n}"),
            place: None,
            state,
            previous: None,
            attention: Attention {
                level: Level::Working,
                label: "turn 1".into(),
                since_ms: NOW,
            },
            started_at_ms: NOW - 540_000,
            state_since_ms: NOW - 540_000,
            ended_at_ms: state.ended().then_some(NOW - 7_200_000),
            doing: None,
            progress: None,
            question: None,
            cost: None,
            eta: None,
            below: None,
            why: None,
            report: None,
            woke: None,
            flagged: false,
            dismissed: false,
            retry_of: None,
            forecast: None,
            thread: None,
            purpose: None,
            next_at_ms: None,
            series: None,
            paused: false,
            routine: None,
            watch: None,
            project: None,
        }
    }

    fn working(n: usize) -> WorkView {
        let mut v = view(n, WorkState::Running);
        v.doing = Some(Doing {
            what: DoingKind::Tool,
            label: "fs.edit".into(),
            since_ms: NOW,
            turn: 4,
            loop_index: 1,
            last_line: None,
        });
        v.cost = Some(Cost {
            spent_usd: 0.42,
            limit_usd: 2.0,
            turn_usd: None,
        });
        v
    }

    fn asking(n: usize) -> WorkView {
        let mut v = view(n, WorkState::NeedsYou);
        v.attention.level = Level::NeedsYou;
        v.question = Some(Question {
            id: format!("act_01J000000000000000000{n:05}"),
            kind: QuestionKind::Approval,
            asked_by: WorkRef {
                id: v.id.clone(),
                kind: WorkKind::Task,
                session_id: None,
                title: String::new(),
            },
            prompt: "fs.write create projects/beta-summary.txt (40 B)".into(),
            detail: None,
            input: None,
            options: vec![],
            on_expiry: None,
            expires_at_ms: None,
            floor: false,
            may_answer: false,
            tool: Some("fs.write".into()),
            asked_at_ms: NOW - 240_000,
            external_text: false,
            choices: vec![],
        });
        v
    }

    fn parts<'a>(
        needs: &'a [WorkView],
        working: &'a [WorkView],
        done: &'a [WorkView],
    ) -> Parts<'a> {
        Parts {
            needs,
            working,
            done,
            quiet_until_ms: None,
            links: no_links(),
        }
    }

    /// A task with a thread or a post has a link to it on its row, in every
    /// part, and one without has none (theseus-i3do6).
    #[test]
    fn a_row_links_the_tasks_thread_or_post() {
        let (n, w, d) = (
            vec![view(1, WorkState::NeedsYou)],
            vec![view(2, WorkState::Running)],
            vec![view(3, WorkState::Done)],
        );
        let mut links = std::collections::HashMap::new();
        for v in n.iter().chain(&w).chain(&d).take(2) {
            links.insert(
                v.id.clone(),
                format!("https://discord.com/channels/1/{}", v.position),
            );
        }
        let mut p = parts(&n, &w, &d);
        p.links = &links;
        let t = texts(&render(&p)).join("\n");
        assert!(
            t.contains("[↗ thread](<https://discord.com/channels/1/1>)"),
            "{t}"
        );
        assert!(
            t.contains("[↗ thread](<https://discord.com/channels/1/2>)"),
            "{t}"
        );
        assert_eq!(t.matches("↗ thread").count(), 2, "the third has none: {t}");
    }

    fn shown(p: &Parts<'_>) -> String {
        let c = render(p);
        format!(
            "{}\ncomponents={} chars={}",
            texts(&c).join("\n---\n"),
            count(&c),
            text_len(&c)
        )
    }

    #[test]
    fn a_board_with_no_tasks_says_all_clear_and_offers_no_select() {
        let c = render(&parts(&[], &[], &[]));
        assert_eq!(texts(&c), ["📋 **Tasks** · all clear"],);
        assert!(!serde_json::to_string(&c).unwrap().contains(PICK));
    }

    #[test]
    fn a_board_with_one_working_task_names_what_it_does_and_what_it_costs() {
        let w = [working(1)];
        let c = render(&parts(&[], &w, &[]));
        let t = texts(&c);
        assert_eq!(t[0], "📋 **Tasks** · ⚙️ 1 running");
        assert_eq!(
            t[1],
            "**⚙️ Now**\n⚙️ **Task number 1** `…000001` · fs.edit · turn 4 · $0.42 of $2 · since <t:1789999460:R>"
        );
    }

    #[test]
    fn a_needs_you_item_carries_its_question_and_an_answer_button() {
        let n = [asking(2)];
        let c = render(&parts(&n, &[], &[]));
        let t = texts(&c);
        assert_eq!(t[0], "📋 **Tasks** · 🔴 1 needs you");
        assert_eq!(t[1], "**🔴 Needs you**");
        assert_eq!(
            t[2],
            "**Task number 2** `…000002`\nfs.write create projects/beta-summary.txt (40 B) · asked <t:1789999760:R>"
        );
        let json = serde_json::to_string(&c).unwrap();
        assert!(
            json.contains("\"custom_id\":\"tb:o:exe_01J00000000000000000000002\""),
            "{json}"
        );
        assert!(json.contains("Answer ↗"));
    }

    #[test]
    fn eight_tasks_in_every_part_fit_and_say_nothing_is_missing() {
        let n: Vec<_> = (0..2).map(asking).collect();
        let w: Vec<_> = (10..15).map(working).collect();
        let d: Vec<_> = (20..22).map(|i| view(i, WorkState::Done)).collect();
        let s = shown(&parts(&n, &w, &d));
        assert!(!s.contains("more"), "{s}");
        assert!(
            s.starts_with(
                "📋 **Tasks** · 🔴 2 needs you · ⚙️ 5 running · ✅ 2 done since you looked"
            ),
            "{s}"
        );
    }

    #[test]
    fn thirty_tasks_fold_their_tails_and_stay_inside_discords_limits() {
        let n: Vec<_> = (0..10).map(asking).collect();
        let w: Vec<_> = (100..114).map(working).collect();
        let d: Vec<_> = (200..206).map(|i| view(i, WorkState::Done)).collect();
        let c = render(&parts(&n, &w, &d));
        assert!(
            count(&c) <= 40 && text_len(&c) <= 4000,
            "{} {}",
            count(&c),
            text_len(&c)
        );
        let t = texts(&c).join("\n");
        assert!(t.contains("more · /tasks"), "{t}");
        // The longest waiting stays first; the newest working task is shown, the oldest folded.
        assert!(t.contains("Task number 113"), "{t}");
        assert!(!t.contains("Task number 100"), "{t}");
        assert!(t.starts_with(
            "📋 **Tasks** · 🔴 10 needs you · ⚙️ 14 running · ✅ 6 done since you looked"
        ));
    }

    #[test]
    fn a_failed_task_says_why_and_a_quiet_board_says_until_when() {
        let mut f = view(3, WorkState::Failed);
        f.report = Some(theseus_protocol::work::WorkReport {
            outcome: "failed".into(),
            first_line: None,
            reason: Some("the provider refused the request (400)".into()),
        });
        let d = [f];
        let mut p = parts(&[], &[], &d);
        p.quiet_until_ms = Some(NOW + 3_600_000);
        let c = render(&p);
        let t = texts(&c);
        assert!(
            t[0].ends_with("🔕 quiet until <t:1790003600:t>"),
            "{}",
            t[0]
        );
        assert!(
            t[1].contains(
                "❌ **Task number 3** `…000003` · failed <t:1789992800:R> · the provider refused"
            ),
            "{}",
            t[1]
        );
        assert!(serde_json::to_string(&c).unwrap().contains("🔔 Unquiet"));
    }

    #[test]
    fn the_same_tasks_make_the_same_board_so_a_quiet_minute_edits_nothing() {
        let w = [working(1), working(2)];
        assert_eq!(render(&parts(&[], &w, &[])), render(&parts(&[], &w, &[])));
    }

    /// The board for each case the design names, as a person reads it, with the
    /// counts Discord limits and the ids a press sends: 0, 1, 8 and 30 tasks, a
    /// needs-you board, and all done. `THESEUS_GOLDEN=write` rewrites the file.
    #[test]
    fn the_boards_for_0_1_8_and_30_tasks_needs_you_and_all_done_are_as_they_were() {
        type Case = (&'static str, Vec<WorkView>, Vec<WorkView>, Vec<WorkView>);
        let cases: Vec<Case> = vec![
            ("0 tasks", vec![], vec![], vec![]),
            ("1 task", vec![], vec![working(1)], vec![]),
            (
                "8 tasks",
                (0..2).map(asking).collect(),
                (10..15).map(working).collect(),
                vec![view(20, WorkState::Done)],
            ),
            (
                "30 tasks",
                (0..10).map(asking).collect(),
                (100..114).map(working).collect(),
                (200..206).map(|i| view(i, WorkState::Done)).collect(),
            ),
            ("needs you", vec![asking(1)], vec![], vec![]),
            (
                "all done",
                vec![],
                vec![],
                vec![view(1, WorkState::Done), view(2, WorkState::Cancelled), {
                    let mut f = view(3, WorkState::Failed);
                    f.report = Some(theseus_protocol::work::WorkReport {
                        outcome: "failed".into(),
                        first_line: None,
                        reason: Some("the provider refused the request (400)".into()),
                    });
                    f
                }],
            ),
        ];
        let mut out = String::new();
        for (name, n, w, d) in &cases {
            let c = render(&parts(n, w, d));
            out.push_str(&format!(
                "=== {name}: components={} chars={}\n{}\nids: {}\n\n",
                count(&c),
                text_len(&c),
                texts(&c).join("\n---\n"),
                crate::render::v2::custom_ids(&c).join(" ")
            ));
        }
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/render/golden/board_v2.txt"
        );
        if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
            std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
            std::fs::write(path, &out).unwrap();
        }
        let golden =
            std::fs::read_to_string(path).expect("the golden: THESEUS_GOLDEN=write writes it");
        assert_eq!(
            out, golden,
            "THESEUS_GOLDEN=write rewrites it, for a change you mean"
        );
    }

    #[test]
    fn a_short_id_is_the_last_six_characters() {
        assert_eq!(short("exe_01J0000000000000000000abcdef"), "abcdef");
        assert_eq!(short("tiny"), "tiny");
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// However many tasks of each part, and however long their words, the board is
            /// within 40 components and 4,000 characters, and its ids are unique.
                #[test]
            fn any_board_fits_and_has_unique_ids(a in 0usize..40, b in 0usize..60, c in 0usize..40, long in 1usize..400) {
                let n: Vec<_> = (0..a).map(|i| { let mut v = asking(i); v.title = "t".repeat(long); v.question.as_mut().unwrap().prompt = "p".repeat(long); v }).collect();
                let w: Vec<_> = (0..b).map(|i| { let mut v = working(1000 + i); v.title = "w".repeat(long); v }).collect();
                let d: Vec<_> = (0..c).map(|i| view(2000 + i, WorkState::Done)).collect();
                let out = render(&parts(&n, &w, &d));
                prop_assert!(count(&out) <= 40, "components {}", count(&out));
                prop_assert!(text_len(&out) <= 4000, "chars {}", text_len(&out));
                prop_assert!(crate::render::v2::validate(&out).is_ok());
                let ids = crate::render::v2::custom_ids(&out);
                let set: std::collections::HashSet<_> = ids.iter().collect();
                prop_assert_eq!(ids.len(), set.len());
            }
        }
    }
}
