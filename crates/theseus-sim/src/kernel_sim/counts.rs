//! What sim2 drives (theseus-celu.35): its counts in the report, and what the
//! world keeps between checks for `/stop`, a task's report, and the outbox.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use theseus_kernel::Action;

/// sim2's counts, one field of `SimReport`, said on the seed's line and the
/// TOTAL line in one clause.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Sim2Counts {
    /// `stop_execution` calls that stopped an open conversation, those that
    /// landed while a turn ran (in it, or on the racing thread), the calls
    /// they told to stop, what they declined, and the stops whose next input
    /// ran a turn.
    pub stops: u64,
    pub stops_in_turn: u64,
    pub stopped_calls: u64,
    pub stopped_declined: u64,
    pub stops_resumed: u64,
    /// One running call stopped alone (`stop_call`), and tasks refused a stop.
    pub call_stops: u64,
    pub task_stops_refused: u64,
    /// Tasks opened under a parent, those with `wake_parent`, the calls run
    /// again that found their task, and opens a task's turn was refused.
    pub tasks_opened: u64,
    pub tasks_waking: u64,
    pub tasks_found_again: u64,
    pub depth_refused: u64,
    /// Reports a parent's turn read, those that woke it, and the turns that
    /// read more than one at once.
    pub reports_read: u64,
    pub report_wakes: u64,
    pub reports_together: u64,
    /// Wakes that fell due while their turn ran, queued by its end.
    pub busy_wakes: u64,
    /// Posts staged in a turn's end and planned outside one; sent, sent again
    /// under the same key after a crash, refused by the channel, settled, and
    /// settled a second time; and crashes around a post's transitions.
    pub posts_staged: u64,
    pub posts_planned: u64,
    pub posts_sent: u64,
    pub posts_resent: u64,
    pub posts_refused: u64,
    pub posts_settled: u64,
    pub second_settles: u64,
    pub post_crashes: u64,
}

impl Sim2Counts {
    pub fn add(&mut self, o: &Self) {
        self.stops += o.stops;
        self.stops_in_turn += o.stops_in_turn;
        self.stopped_calls += o.stopped_calls;
        self.stopped_declined += o.stopped_declined;
        self.stops_resumed += o.stops_resumed;
        self.call_stops += o.call_stops;
        self.task_stops_refused += o.task_stops_refused;
        self.tasks_opened += o.tasks_opened;
        self.tasks_waking += o.tasks_waking;
        self.tasks_found_again += o.tasks_found_again;
        self.depth_refused += o.depth_refused;
        self.reports_read += o.reports_read;
        self.report_wakes += o.report_wakes;
        self.reports_together += o.reports_together;
        self.busy_wakes += o.busy_wakes;
        self.posts_staged += o.posts_staged;
        self.posts_planned += o.posts_planned;
        self.posts_sent += o.posts_sent;
        self.posts_resent += o.posts_resent;
        self.posts_refused += o.posts_refused;
        self.posts_settled += o.posts_settled;
        self.second_settles += o.second_settles;
        self.post_crashes += o.post_crashes;
    }
}

impl fmt::Display for Sim2Counts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "sim2: {} stops: {} while a turn ran, {} calls told to stop, {} unsent declined, {} next inputs ran a turn; {} calls stopped alone, {} task stops refused · {} tasks opened: {} waking their parent, {} found again, {} refused at depth one; {} reports read: {} woke their parent, {} read together · {} wakes due in a turn queued by its end · {} posts staged in a turn's end, {} planned outside one; {} sent, {} sent again, {} refused, {} settled, {} second settles, {} crashes around a post",
            self.stops,
            self.stops_in_turn,
            self.stopped_calls,
            self.stopped_declined,
            self.stops_resumed,
            self.call_stops,
            self.task_stops_refused,
            self.tasks_opened,
            self.tasks_waking,
            self.tasks_found_again,
            self.depth_refused,
            self.reports_read,
            self.report_wakes,
            self.reports_together,
            self.busy_wakes,
            self.posts_staged,
            self.posts_planned,
            self.posts_sent,
            self.posts_resent,
            self.posts_refused,
            self.posts_settled,
            self.second_settles,
            self.post_crashes,
        )
    }
}

/// What the world keeps for sim2's checks.
#[derive(Default)]
pub(super) struct Sim2 {
    /// Executions a stop landed on while a turn ran, by that turn, read from
    /// the ledger: until the turn's end, they plan nothing.
    pub stopped_turn: HashMap<String, u64>,
    /// Executions a stop left free since the last due scan: a report's wake
    /// waits for that scan.
    pub stopped_since_scan: HashSet<String>,
    /// Every task opened under a parent, and its parent.
    pub tasks: BTreeMap<String, String>,
    /// Tasks whose report a parent's turn read.
    pub reports_taken: HashSet<String>,
    /// Parents queued for a report's wake, and not run since.
    pub report_queued: HashSet<String>,
    /// Every post the sim wrote, as the binding last left it.
    pub posts: BTreeMap<String, Action>,
    /// Each post record the binding wrote and the WAL scan has not yet
    /// read, in the order written: the scan finds no other.
    pub post_writes: std::collections::VecDeque<Action>,
    /// The fake channel the binding sends to.
    pub channel: Channel,
}

/// The fake channel (a Discord stand-in): one message per downstream key,
/// whatever sends under it again, as Discord's nonce keeps one.
#[derive(Default)]
pub(super) struct Channel {
    /// Each message: its key, the post it carries, and its id.
    pub messages: Vec<(String, String, String)>,
}

impl Channel {
    /// Send `post` under `key`: the message already sent under it, or a new
    /// one. Returns its id and whether it was new.
    pub fn send(&mut self, key: &str, post: &str) -> (String, bool) {
        if let Some((_, _, id)) = self.messages.iter().find(|(k, _, _)| k == key) {
            return (id.clone(), false);
        }
        let id = format!("msg_{}", self.messages.len() + 1);
        self.messages
            .push((key.to_string(), post.to_string(), id.clone()));
        (id, true)
    }

    /// The messages that carry `post`.
    pub fn copies(&self, post: &str) -> Vec<&str> {
        self.messages
            .iter()
            .filter(|(_, p, _)| p == post)
            .map(|(_, _, id)| id.as_str())
            .collect()
    }
}
