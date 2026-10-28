//! The grouping every surface shares (theseus-5i8i).

use super::*;
use crate::threads::ThreadState;
use crate::work::{Question, QuestionKind, WorkRef, WorkState};
use crate::Attention;

const NOW: u64 = 10_000_000;

fn level(s: WorkState) -> Level {
    match s {
        WorkState::NeedsYou | WorkState::Failed => Level::NeedsYou,
        WorkState::Queued | WorkState::Running | WorkState::Waiting => Level::Working,
        WorkState::Ready => Level::Ready,
        _ => Level::Idle,
    }
}

fn view(id: &str, kind: WorkKind, state: WorkState, thread: Option<&str>) -> WorkView {
    WorkView {
        moved_to: None,
        id: id.into(),
        kind,
        position: 1,
        at_ms: NOW - 60_000,
        parent: None,
        root: id.into(),
        session_id: None,
        title: id.into(),
        place: None,
        state,
        previous: None,
        attention: Attention {
            level: level(state),
            label: state.as_str().into(),
            since_ms: 1,
        },
        started_at_ms: 1,
        state_since_ms: 1,
        ended_at_ms: None,
        doing: None,
        progress: None,
        question: None,
        cost: None,
        eta: None,
        below: None,
        why: None,
        report: None,
        woke: None,
        expect: None,
        flagged: false,
        dismissed: false,
        retry_of: None,
        forecast: None,
        thread: thread.map(str::to_string),
        purpose: None,
        next_at_ms: None,
        series: None,
        paused: false,
        routine: None,
        watch: None,
        project: None,
        data: None,
    }
}

fn under(mut v: WorkView, parent: &str) -> WorkView {
    v.parent = Some(parent.into());
    v.root = parent.into();
    v
}

fn asking(mut v: WorkView) -> WorkView {
    v.question = Some(Question {
        id: v.id.clone(),
        kind: QuestionKind::Approval,
        asked_by: WorkRef {
            id: "exe_a".into(),
            kind: WorkKind::Conversation,
            session_id: None,
            title: String::new(),
        },
        prompt: "proc.run cargo test".into(),
        detail: None,
        input: None,
        options: vec![],
        on_expiry: None,
        expires_at_ms: None,
        floor: false,
        may_answer: true,
        tool: Some("proc.run".into()),
        asked_at_ms: 1,
        choices: vec![],
    });
    v
}

fn thread(slug: &str, title: &str, state: ThreadState) -> ThreadGroup {
    ThreadGroup {
        slug: slug.into(),
        title: title.into(),
        state,
        persona: None,
        sealed: false,
        seal_pending: None,
        session_id: Some(format!("ses_{slug}")),
        last_active_ms: NOW - 120_000,
        context_tokens: Some(41_000),
        today_usd: Some(1.2),
    }
}

/// Two threads with a task and a question each, and one loose task.
fn board() -> Vec<WorkView> {
    vec![
        view(
            "exe_pr",
            WorkKind::Conversation,
            WorkState::Ready,
            Some("pr-round"),
        ),
        under(
            view(
                "exe_pr_t",
                WorkKind::Task,
                WorkState::Running,
                Some("pr-round"),
            ),
            "exe_pr",
        ),
        under(
            asking(view(
                "act_pr_q",
                WorkKind::Question,
                WorkState::NeedsYou,
                Some("pr-round"),
            )),
            "exe_pr",
        ),
        view(
            "exe_tides",
            WorkKind::Conversation,
            WorkState::Ready,
            Some("tides"),
        ),
        under(
            view(
                "exe_tides_t",
                WorkKind::Task,
                WorkState::Running,
                Some("tides"),
            ),
            "exe_tides",
        ),
        view("exe_dm", WorkKind::Conversation, WorkState::Running, None),
        under(
            view("exe_dm_t", WorkKind::Task, WorkState::Running, None),
            "exe_dm",
        ),
    ]
}

/// A thread's tasks, questions and wakes are under it; work in no thread stays
/// where the board shows it.
#[test]
fn items_are_grouped_by_thread_and_unthreaded_work_is_left_alone() {
    let views = board();
    let groups = vec![
        thread("pr-round", "PR round", ThreadState::Active),
        thread("tides", "Tides", ThreadState::Active),
    ];
    let g = group(&views, &groups);
    fn ids<'a>(t: &GroupedThread<'a>) -> Vec<&'a str> {
        t.items.iter().map(|v| v.id.as_str()).collect()
    }
    let pr = g
        .threads
        .iter()
        .find(|t| t.group.slug == "pr-round")
        .unwrap();
    assert_eq!(ids(pr), ["act_pr_q", "exe_pr_t"], "needs you first");
    assert_eq!(pr.head.map(|h| h.id.as_str()), Some("exe_pr"));
    let tides = g.threads.iter().find(|t| t.group.slug == "tides").unwrap();
    assert_eq!(ids(tides), ["exe_tides_t"]);
    let loose: Vec<&str> = g.loose.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(loose, ["exe_dm", "exe_dm_t"]);
}

/// A view stamped with a thread the board has no group for is not lost: it is
/// shown where today's board shows it.
#[test]
fn a_view_of_an_unknown_thread_is_loose() {
    let views = board();
    let g = group(&views, &[]);
    assert!(g.threads.is_empty());
    assert_eq!(g.loose.len(), views.len());
}

/// A question counts once, a seal recommendation counts, and the threads
/// that need you come first.
#[test]
fn needs_you_counts_a_question_once_and_a_pending_seal() {
    let views = board();
    let mut tides = thread("tides", "Tides", ThreadState::Active);
    tides.seal_pending = Some("the harbour notes are private".into());
    let groups = vec![tides, thread("pr-round", "PR round", ThreadState::Active)];
    let g = group(&views, &groups);
    let n = |slug: &str| {
        g.threads
            .iter()
            .find(|t| t.group.slug == slug)
            .unwrap()
            .needs_you
    };
    assert_eq!(n("pr-round"), 1);
    assert_eq!(n("tides"), 1);
    assert_eq!(g.needs_you(), 2);
}

/// A waiting conversation with its question view in the list counts through
/// the question, not twice.
#[test]
fn a_conversation_that_asks_counts_through_its_question() {
    let mut head = view(
        "exe_pr",
        WorkKind::Conversation,
        WorkState::NeedsYou,
        Some("pr-round"),
    );
    head = asking(head);
    let q = under(
        asking(view(
            "act_q",
            WorkKind::Question,
            WorkState::NeedsYou,
            Some("pr-round"),
        )),
        "exe_pr",
    );
    let views = vec![head, q];
    let groups = vec![thread("pr-round", "PR round", ThreadState::Active)];
    let g = group(&views, &groups);
    assert_eq!(g.threads[0].needs_you, 1);
}

/// A paused thread shows paused, keeps its own count, and leaves the board's.
#[test]
fn a_paused_thread_leaves_the_boards_count() {
    let views = board();
    let groups = vec![
        thread("pr-round", "PR round", ThreadState::Paused),
        thread("tides", "Tides", ThreadState::Active),
    ];
    let g = group(&views, &groups);
    let pr = g
        .threads
        .iter()
        .find(|t| t.group.slug == "pr-round")
        .unwrap();
    assert_eq!(pr.needs_you, 1);
    assert_eq!(pr.board_needs(), 0);
    assert_eq!(g.needs_you(), 0);
    assert!(pr.header(NOW, 80).contains("paused"));
}

/// A done thread is listed only while something under it is open.
#[test]
fn a_done_thread_is_shown_only_while_something_is_open() {
    let groups = vec![thread("tides", "Tides", ThreadState::Done)];
    let none = vec![view(
        "exe_tides",
        WorkKind::Conversation,
        WorkState::Done,
        Some("tides"),
    )];
    assert!(group(&none, &groups).threads.is_empty());
    let open = board();
    assert_eq!(group(&open, &groups).threads.len(), 1);
}

/// The title of a sealed thread begins with a lock.
#[test]
fn a_sealed_threads_title_has_a_lock() {
    let views = board();
    let mut sealed = thread("tides", "Tides", ThreadState::Active);
    sealed.sealed = true;
    let groups = vec![sealed];
    let g = group(&views, &groups);
    assert_eq!(g.threads[0].title(), "🔒 Tides");
    assert!(g.threads[0].header(NOW, 80).starts_with("🔒 Tides"));
    let plain = vec![thread("tides", "Tides", ThreadState::Active)];
    assert_eq!(group(&views, &plain).threads[0].title(), "Tides");
}

/// The header at a wide and a narrow width: the title first, details as they fit.
#[test]
fn the_header_gives_what_fits() {
    let views = board();
    let mut g0 = thread("pr-round", "PR round", ThreadState::Active);
    g0.persona = Some("maren".into());
    let groups = vec![g0];
    let g = group(&views, &groups);
    let t = &g.threads[0];
    assert_eq!(
        t.header(NOW, 100),
        "PR round · active · 2m ago · warm · 41k ctx · $1.20 today · maren"
    );
    assert_eq!(t.header(NOW, 30), "PR round · active · 2m ago");
    assert_eq!(t.header(NOW, 6), "PR ro…");
}

/// theseus-pmqhb: a thread's last activity is its own last message. The head conversation's view
/// is stamped with the board's start (a daemon restart), which is no activity of the thread's.
#[test]
fn a_restart_stamped_head_is_not_the_threads_last_activity() {
    let mut views = board();
    for v in views
        .iter_mut()
        .filter(|v| v.kind == WorkKind::Conversation && v.parent.is_none())
    {
        v.at_ms = NOW;
    }
    let groups = vec![thread("pr-round", "PR round", ThreadState::Active)];
    let g = group(&views, &groups);
    let t = &g.threads[0];
    assert!(t.head.is_some(), "the thread has its head conversation");
    assert_eq!(t.last_active_ms, NOW - 120_000);
    assert!(
        t.header(NOW, 80).contains("2m ago"),
        "{}",
        t.header(NOW, 80)
    );
    // With no message of its own, the head's time stands.
    let mut fresh = thread("new", "New", ThreadState::Active);
    fresh.last_active_ms = 0;
    let groups = vec![fresh];
    let g = group(&views, &groups);
    assert_eq!(
        g.threads[0].last_active_ms,
        g.threads[0].head.map_or(0, |h| h.at_ms)
    );
}

/// Warm is within the provider cache's lifetime; a thread with no messages
/// is neither.
#[test]
fn warm_is_within_the_caches_lifetime() {
    let views = board();
    let groups = vec![thread("tides", "Tides", ThreadState::Active)];
    let g = group(&views, &groups);
    let t = &g.threads[0];
    assert!(t.warm(t.last_active_ms + WARM_MS));
    assert!(!t.warm(t.last_active_ms + WARM_MS + 1));
    let mut fresh = thread("new", "New", ThreadState::Active);
    fresh.last_active_ms = 0;
    fresh.session_id = None;
    let groups = vec![fresh];
    let g = group(&[], &groups);
    assert!(!g.threads[0].warm(NOW));
    assert!(g.threads[0].header(NOW, 80).contains("no messages yet"));
}

/// An update's unread stats leave the ones held.
#[test]
fn a_merge_keeps_the_stats_an_update_did_not_read() {
    let mut held = thread("tides", "Tides", ThreadState::Active);
    let mut newer = thread("tides", "Tides, again", ThreadState::Paused);
    newer.context_tokens = None;
    newer.today_usd = Some(2.0);
    held.merge(newer);
    assert_eq!(held.title, "Tides, again");
    assert_eq!(held.context_tokens, Some(41_000));
    assert_eq!(held.today_usd, Some(2.0));
}
