//! What the daemon remembers about tool use (theseus-7m7w.2), in memory: the
//! counts per place kind the selector reads, and what each running
//! conversation has surfaced, asked for and used.
//!
//! The counts are read from the ledger's `tools.used`, `tools.searched` and
//! `tools.missed` rows once after serving (`Board::apply`, from `warm`) and
//! kept by each turn's row, so nothing here is a record: a restart reads the
//! rows again. A conversation's own notes (`Convo`) are in memory only, and
//! lose at most a surfaced tool's name, which a search finds again.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use serde_json::Value;

use super::family::{self, PlaceKind};
use super::fit::Fit;
use super::select::{self, Day, Selection, Stats};
use super::Mechanism;
use crate::config::defer::DeferConfig;

/// A hidden tool: what the model can be told of it.
#[derive(Debug, Clone, PartialEq)]
pub struct HiddenTool {
    pub wire: String,
    pub family: String,
    /// The first sentence of its description.
    pub line: String,
}

/// What one conversation did with its tools, in this process.
#[derive(Debug, Default)]
struct Convo {
    kind: Option<PlaceKind>,
    mechanism: Option<Mechanism>,
    loaded: BTreeSet<String>,
    hidden: Vec<HiddenTool>,
    /// Tools a search surfaced: a call to one is not a miss.
    surfaced: BTreeSet<String>,
    /// Families a search asked for (the fallback's): they stay loaded from
    /// the next compilation on.
    wanted: BTreeSet<String>,
    /// Families a `tools_find` asked for since the turn's last request, until
    /// the turn's next loop takes them (`take_found`, theseus-342qy).
    found: BTreeSet<String>,
    /// Families the conversation has called, or searched for (a search is no use to the
    /// counts, but a family it loaded is not an unused one).
    used: BTreeSet<String>,
    /// Families this turn called, until the turn's row.
    turn_called: BTreeSet<String>,
    /// This turn's searches, until the turn's row.
    searches: Vec<Search>,
    /// This turn's misses, until the turn's row.
    misses: Vec<Miss>,
    /// What this turn's request admitted past the place's budget, until the
    /// turn's row (theseus-xlri5).
    raised: Option<theseus_protocol::route::ToolsRaised>,
    /// Jev's last families judgment of the conversation's prompt: the prompt's
    /// digest and the families it found relevant (theseus-xlri5), memory only.
    judged: Option<(u64, Vec<(String, f64)>)>,
}

/// One search of a turn.
#[derive(Debug, Clone, PartialEq)]
pub struct Search {
    pub query: String,
    pub surfaced: Vec<String>,
    pub families: Vec<String>,
    pub called: bool,
}

/// One miss of a turn: a call to a tool that was not loaded, with no search.
#[derive(Debug, Clone, PartialEq)]
pub struct Miss {
    pub tool: String,
    pub family: String,
}

/// A turn's use, as its row says it.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnUse {
    pub kind: PlaceKind,
    pub mechanism: Option<Mechanism>,
    pub loaded: Vec<String>,
    pub deferred: Vec<String>,
    pub used: Vec<String>,
    pub searched: Vec<String>,
    pub unused: Vec<String>,
    pub searches: Vec<Search>,
    pub misses: Vec<Miss>,
    /// The families that raised the request past the place's budget
    /// (theseus-xlri5); none when nothing did.
    pub raised: Option<theseus_protocol::route::ToolsRaised>,
}

#[derive(Default)]
struct State {
    kinds: BTreeMap<PlaceKind, Stats>,
    convos: BTreeMap<String, Convo>,
    /// The conversations' names, oldest note first: the notes of a long-idle
    /// one go (`MAX_CONVOS`), and a search finds a surfaced tool again.
    order: std::collections::VecDeque<String>,
    /// Whether the ledger's rows have been read (health says so).
    warmed: bool,
    /// When this process counted its first turn: the rows from then on are
    /// counted live, so the read of the ledger skips them.
    live_since: Option<u64>,
}

/// What a request offers a selection: the families it has, the ones a message
/// adds, and the budget the learned ones must fit (theseus-buz3h).
pub struct Offer<'a> {
    pub known: &'a BTreeSet<String>,
    pub extra: &'a BTreeSet<String>,
    pub fit: Option<&'a Fit>,
}

/// The board: shared by every turn, behind one lock held for a few map
/// operations at a time.
#[derive(Default)]
pub struct Board {
    state: Mutex<State>,
    /// Where `theseus.tools.events` goes, once telemetry is built.
    telemetry: std::sync::OnceLock<crate::telemetry::Telemetry>,
    /// A test's way to end the ledger read at the one moment a listing is
    /// open to it: after the listing took its counts (theseus-nyczg).
    #[cfg(test)]
    after_counts: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

/// Conversations whose notes are kept; the oldest go past it.
const MAX_CONVOS: usize = 256;

/// Ledger days kept in memory: twice the longest window anyone is likely to
/// set, trimmed as rows arrive.
const KEEP_DAYS: u64 = 60;

impl State {
    /// A conversation's notes, made if new; a new one past `MAX_CONVOS`
    /// takes the place of the oldest.
    fn convo(&mut self, session: &str) -> &mut Convo {
        if !self.convos.contains_key(session) {
            if self.order.len() >= MAX_CONVOS {
                if let Some(old) = self.order.pop_front() {
                    self.convos.remove(&old);
                }
            }
            self.order.push_back(session.to_string());
        }
        self.convos.entry(session.to_string()).or_default()
    }
}

impl Board {
    /// Count the board's events in telemetry from now on.
    pub fn export_to(&self, t: crate::telemetry::Telemetry) {
        let _ = self.telemetry.set(t);
    }

    /// `theseus.tools.events`, once for each of `families`.
    pub fn emit<'a>(
        &self,
        event: &str,
        place: PlaceKind,
        families: impl IntoIterator<Item = &'a String>,
    ) {
        if let Some(t) = self.telemetry.get() {
            for f in families {
                t.record_tools(event, place.as_str(), f);
            }
        }
    }

    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The selection for a place kind now: the committed defaults, the
    /// counts, and what the conversation asked for. `known` is the families
    /// the request offers. With a `fit`, the learned families that would pass
    /// the place's budget are deferred (theseus-buz3h).
    pub fn select(
        &self,
        session: &str,
        kind: PlaceKind,
        cfg: &DeferConfig,
        now_ms: u64,
        offer: &Offer<'_>,
    ) -> Selection {
        let (known, fit) = (offer.known, offer.fit);
        let st = self.st();
        let mut extra = offer.extra.clone();
        if let Some(c) = st.convos.get(session) {
            extra.extend(c.wanted.iter().cloned());
        }
        let wanted = st
            .convos
            .get(session)
            .map(|c| c.wanted.clone())
            .unwrap_or_default();
        let mut sel = select::select(st.kinds.get(&kind), kind, cfg, now_ms, known, &extra);
        for f in wanted {
            if let Some(w) = sel.families.get_mut(&f) {
                if matches!(w, select::Why::Extra) {
                    *w = select::Why::Wanted;
                }
            }
        }
        if let Some(fit) = fit {
            fit.demote(&mut sel);
        }
        sel
    }

    /// The request a conversation's turn compiled: its loaded families and
    /// the tools it hides.
    pub fn note_request(
        &self,
        session: &str,
        kind: PlaceKind,
        mechanism: Option<Mechanism>,
        loaded: BTreeSet<String>,
        hidden: Vec<HiddenTool>,
    ) {
        let mut st = self.st();
        let c = st.convo(session);
        c.kind = Some(kind);
        c.mechanism = mechanism;
        c.loaded = loaded;
        c.hidden = hidden;
    }

    /// What the turn's request admitted past the place's budget (theseus-xlri5),
    /// for its row: the last loop's request says.
    pub fn note_raised(&self, session: &str, raised: Option<theseus_protocol::route::ToolsRaised>) {
        self.st().convo(session).raised = raised;
    }

    /// Jev's last families judgment of the conversation's prompt (theseus-xlri5):
    /// its digest and the relevant set, which a turn with no new message, or the
    /// same prompt again, reuses.
    pub fn judged(&self, session: &str) -> Option<(u64, Vec<(String, f64)>)> {
        self.st().convos.get(session).and_then(|c| c.judged.clone())
    }

    pub fn set_judged(&self, session: &str, digest: u64, relevant: Vec<(String, f64)>) {
        self.st().convo(session).judged = Some((digest, relevant));
    }

    /// The tools this conversation's request hides, for `tools_find`.
    pub fn hidden(&self, session: &str) -> Vec<HiddenTool> {
        self.st()
            .convos
            .get(session)
            .map(|c| c.hidden.clone())
            .unwrap_or_default()
    }

    /// The mechanism and the hidden set of the conversation's last request.
    pub fn mechanism(&self, session: &str) -> Option<Mechanism> {
        self.st().convos.get(session).and_then(|c| c.mechanism)
    }

    /// Whether `wire` is a tool the conversation's request hides, and a
    /// search of it has not surfaced it.
    pub fn is_unsurfaced(&self, session: &str, wire: &str) -> Option<HiddenTool> {
        let st = self.st();
        let c = st.convos.get(session)?;
        if c.surfaced.contains(wire) {
            return None;
        }
        c.hidden.iter().find(|h| h.wire == wire).cloned()
    }

    /// A search ran: the tools it surfaced now count as loaded for the
    /// conversation, and their families as asked for.
    pub fn searched(&self, session: &str, query: &str, surfaced: &[String]) {
        let mut st = self.st();
        let c = st.convo(session);
        let mut families = BTreeSet::new();
        for w in surfaced {
            c.surfaced.insert(w.clone());
            let f = family::of_wire(w);
            if family::hideable(&f) {
                c.wanted.insert(f.clone());
                c.used.insert(f.clone());
                families.insert(f);
            }
        }
        c.searches.push(Search {
            query: query.chars().take(200).collect(),
            surfaced: surfaced.to_vec(),
            families: families.into_iter().collect(),
            called: false,
        });
    }

    /// The fallback's search: families the model asked for. The turn's next
    /// loop adds them to its tools (`take_found`), and they stay loaded for
    /// the conversation's later turns.
    pub fn wants(&self, session: &str, query: &str, families: &[String], tools: &[String]) {
        let mut st = self.st();
        let c = st.convo(session);
        for f in families {
            c.wanted.insert(f.clone());
            c.found.insert(f.clone());
            c.used.insert(f.clone());
        }
        c.searches.push(Search {
            query: query.chars().take(200).collect(),
            surfaced: tools.to_vec(),
            families: families.to_vec(),
            called: false,
        });
    }

    /// The families a `tools_find` asked for since the last take: the turn's
    /// next loop loads them at once (theseus-342qy).
    pub fn take_found(&self, session: &str) -> BTreeSet<String> {
        self.st()
            .convos
            .get_mut(session)
            .map(|c| std::mem::take(&mut c.found))
            .unwrap_or_default()
    }

    /// A call was made to `wire`: its family is used, and a search of the
    /// turn that surfaced it was followed by the call.
    pub fn called(&self, session: &str, wire: &str) {
        let mut st = self.st();
        let c = st.convo(session);
        let f = family::of_wire(wire);
        c.used.insert(f.clone());
        c.turn_called.insert(f);
        for s in &mut c.searches {
            if s.surfaced.iter().any(|w| w == wire) {
                s.called = true;
            }
        }
    }

    /// A call to a tool the request hid, with no search: a miss.
    pub fn missed(&self, session: &str, miss: Miss) {
        let mut st = self.st();
        let c = st.convo(session);
        c.used.insert(miss.family.clone());
        c.misses.push(miss);
    }

    /// A turn ended: its row's data, and the counts it adds. Takes the
    /// turn's searches and misses.
    pub fn end_turn(&self, session: &str, fallback: PlaceKind, now_ms: u64) -> TurnUse {
        let mut st = self.st();
        st.live_since.get_or_insert(now_ms);
        let (kind, mechanism, loaded, deferred, searches, misses, unused, called, raised) = {
            let c = st.convo(session);
            let kind = c.kind.unwrap_or(fallback);
            let deferred: BTreeSet<String> = c.hidden.iter().map(|h| h.family.clone()).collect();
            let unused: Vec<String> = c
                .loaded
                .iter()
                .filter(|f| family::hideable(f) && !c.used.contains(*f))
                .cloned()
                .collect();
            (
                kind,
                c.mechanism,
                c.loaded.iter().cloned().collect::<Vec<_>>(),
                deferred.into_iter().collect::<Vec<_>>(),
                std::mem::take(&mut c.searches),
                std::mem::take(&mut c.misses),
                unused,
                std::mem::take(&mut c.turn_called),
                c.raised.take(),
            )
        };
        let searched: BTreeSet<String> = searches.iter().flat_map(|s| s.families.clone()).collect();
        let used = called.clone();
        let turn = TurnUse {
            kind,
            mechanism,
            loaded,
            deferred,
            used: called.into_iter().collect(),
            searched: searched.into_iter().collect(),
            unused,
            searches,
            misses,
            raised,
        };
        Self::count(st.kinds.entry(kind).or_default(), &turn, &used, now_ms);
        turn
    }

    fn count(stats: &mut Stats, t: &TurnUse, used: &BTreeSet<String>, at_ms: u64) {
        let day = stats.day_mut(at_ms);
        day.turns += 1;
        for f in used {
            *day.used.entry(f.clone()).or_insert(0) += 1;
        }
        for f in &t.loaded {
            *day.loaded.entry(f.clone()).or_insert(0) += 1;
        }
        for f in &t.unused {
            *day.unused.entry(f.clone()).or_insert(0) += 1;
        }
        for s in &t.searches {
            for f in &s.families {
                *day.searched.entry(f.clone()).or_insert(0) += 1;
            }
        }
        for m in &t.misses {
            *day.missed.entry(m.family.clone()).or_insert(0) += 1;
        }
        for f in used {
            let last = stats.last_used.entry(f.clone()).or_insert(0);
            *last = (*last).max(at_ms);
        }
        stats.trim(at_ms, KEEP_DAYS);
    }

    /// A `tools.used` row read back from the ledger (the warm read).
    pub fn apply_used(&self, at_ms: u64, data: &Value) {
        let Some(kind) = data["place"].as_str().and_then(PlaceKind::parse) else {
            return;
        };
        let list = |k: &str| -> BTreeSet<String> {
            data[k]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        let (called, searched) = (list("used"), list("searched"));
        let used = called.clone();
        let t = TurnUse {
            kind,
            mechanism: None,
            loaded: list("loaded").into_iter().collect(),
            deferred: vec![],
            used: called.into_iter().collect(),
            searched: searched.into_iter().collect(),
            unused: list("unused").into_iter().collect(),
            // The searches and misses are their own rows.
            searches: vec![],
            misses: vec![],
            raised: None,
        };
        let mut st = self.st();
        if st.live_since.is_some_and(|l| at_ms >= l) {
            return;
        }
        Self::count(st.kinds.entry(kind).or_default(), &t, &used, at_ms);
    }

    /// A `tools.searched` row read back: its families count as searched.
    pub fn apply_searched(&self, at_ms: u64, data: &Value) {
        let Some(kind) = data["place"].as_str().and_then(PlaceKind::parse) else {
            return;
        };
        let mut st = self.st();
        if st.live_since.is_some_and(|l| at_ms >= l) {
            return;
        }
        let day = st.kinds.entry(kind).or_default().day_mut(at_ms);
        for f in data["families"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            *day.searched.entry(f.to_string()).or_insert(0) += 1;
        }
    }

    /// A `tools.missed` row read back.
    pub fn apply_missed(&self, at_ms: u64, data: &Value) {
        let Some(kind) = data["place"].as_str().and_then(PlaceKind::parse) else {
            return;
        };
        let Some(f) = data["family"].as_str() else {
            return;
        };
        let mut st = self.st();
        if st.live_since.is_some_and(|l| at_ms >= l) {
            return;
        }
        let day = st.kinds.entry(kind).or_default().day_mut(at_ms);
        *day.missed.entry(f.to_string()).or_insert(0) += 1;
    }

    /// Forget everything: a test starts the read of the ledger again on the
    /// same board (theseus-nyczg).
    #[cfg(test)]
    pub(crate) fn reset(&self) {
        *self.st() = State::default();
    }

    /// Run `f` once, when the next listing (or health's line) has taken its
    /// counts: a read that ends there is the one the listing must not call done
    /// with none of its rows (theseus-nyczg).
    #[cfg(test)]
    pub(crate) fn after_counts(&self, f: impl FnOnce() + Send + 'static) {
        *self.after_counts.lock().unwrap() = Some(Box::new(f));
    }

    /// A listing has taken its counts: the test's hook runs, if it set one.
    #[cfg(test)]
    pub(crate) fn counted(&self) {
        let hook = self.after_counts.lock().unwrap().take();
        if let Some(f) = hook {
            f();
        }
    }

    pub fn set_warmed(&self) {
        self.st().warmed = true;
    }

    pub fn warmed(&self) -> bool {
        self.st().warmed
    }

    /// A copy of one kind's counts, for the listing.
    pub fn stats(&self, kind: PlaceKind) -> Stats {
        self.st().kinds.get(&kind).cloned().unwrap_or_default()
    }

    /// The day counts of every kind: the listing and health read them.
    pub fn all_stats(&self) -> BTreeMap<PlaceKind, Stats> {
        self.st().kinds.clone()
    }

    /// Forget a conversation's notes (its session ended).
    pub fn forget(&self, session: &str) {
        let mut st = self.st();
        st.convos.remove(session);
        st.order.retain(|s| s != session);
    }
}

/// The sum of a kind's per-family counts over the days since `since_ms`.
pub fn sum(
    stats: &Stats,
    since_ms: u64,
    pick: impl Fn(&Day) -> &BTreeMap<String, u32>,
) -> BTreeMap<String, u32> {
    stats.sum_since(since_ms, pick)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 50 * select::DAY_MS;

    fn hidden(wire: &str) -> HiddenTool {
        HiddenTool {
            wire: wire.into(),
            family: family::of_wire(wire),
            line: "x".into(),
        }
    }

    #[test]
    fn the_oldest_conversations_notes_go_past_the_cap() {
        let b = Board::default();
        for i in 0..MAX_CONVOS + 5 {
            b.note_request(
                &format!("s{i}"),
                PlaceKind::Cli,
                None,
                Default::default(),
                vec![hidden("aws_call")],
            );
        }
        assert!(b.st().convos.len() <= MAX_CONVOS);
        assert!(
            b.is_unsurfaced("s0", "aws_call").is_none(),
            "the oldest was forgotten"
        );
        assert!(b
            .is_unsurfaced(&format!("s{}", MAX_CONVOS + 4), "aws_call")
            .is_some());
        b.forget(&format!("s{}", MAX_CONVOS + 4));
        assert!(b
            .is_unsurfaced(&format!("s{}", MAX_CONVOS + 4), "aws_call")
            .is_none());
    }

    #[test]
    fn a_call_to_a_hidden_tool_is_unsurfaced_until_a_search_surfaces_it() {
        let b = Board::default();
        b.note_request(
            "s",
            PlaceKind::Cli,
            Some(Mechanism::Search),
            ["files".to_string()].into(),
            vec![hidden("aws_call"), hidden("aws_whoami")],
        );
        assert!(b.is_unsurfaced("s", "aws_call").is_some());
        assert!(
            b.is_unsurfaced("s", "fs_read").is_none(),
            "a loaded tool is not hidden"
        );
        b.searched("s", "aws account", &["aws_call".to_string()]);
        assert!(b.is_unsurfaced("s", "aws_call").is_none());
        assert!(
            b.is_unsurfaced("s", "aws_whoami").is_some(),
            "only the surfaced one"
        );
        // Another conversation's search surfaces nothing here.
        assert!(
            b.is_unsurfaced("t", "aws_call").is_none(),
            "t hides nothing yet"
        );
    }

    #[test]
    fn a_turn_counts_its_use_and_its_row_says_what_it_did() {
        let b = Board::default();
        b.note_request(
            "s",
            PlaceKind::Dm,
            Some(Mechanism::Search),
            ["files".to_string(), "memory".to_string()].into(),
            vec![hidden("aws_call")],
        );
        b.searched("s", "aws", &["aws_call".to_string()]);
        b.called("s", "aws_call");
        b.called("s", "fs_read");
        b.missed(
            "s",
            Miss {
                tool: "aws_whoami".into(),
                family: "aws".into(),
            },
        );
        let t = b.end_turn("s", PlaceKind::Cli, NOW);
        assert_eq!(t.kind, PlaceKind::Dm);
        assert_eq!(t.searched, ["aws"]);
        assert_eq!(t.deferred, ["aws"]);
        assert!(t.searches[0].called, "the search was followed by the call");
        assert_eq!(t.misses.len(), 1);
        assert_eq!(t.unused, ["memory"], "memory was loaded and not used");
        let s = b.stats(PlaceKind::Dm);
        assert_eq!(s.turns_since(0), 1);
        assert_eq!(s.sum_since(0, |d| &d.missed)["aws"], 1);
        assert_eq!(s.sum_since(0, |d| &d.unused)["memory"], 1);
        // The next turn's row starts clean.
        let t2 = b.end_turn("s", PlaceKind::Cli, NOW + 1000);
        assert!(t2.searches.is_empty() && t2.misses.is_empty());
    }

    #[test]
    fn rows_read_back_make_the_same_counts() {
        let live = Board::default();
        live.note_request(
            "s",
            PlaceKind::Cli,
            None,
            ["files".to_string()].into(),
            vec![],
        );
        live.called("s", "web_search");
        let t = live.end_turn("s", PlaceKind::Cli, NOW);
        let row = serde_json::json!({
            "place": t.kind.as_str(), "loaded": t.loaded, "used": t.used,
            "searched": t.searched, "unused": t.unused,
        });
        let back = Board::default();
        back.apply_used(NOW, &row);
        assert_eq!(live.stats(PlaceKind::Cli), back.stats(PlaceKind::Cli));
    }

    /// A search surfaces a family; only a call counts as its use (theseus-buz3h): the ledger's
    /// `tools.used` rows read back the same way.
    #[test]
    fn a_search_alone_is_not_use() {
        let b = Board::default();
        b.note_request(
            "s",
            PlaceKind::Cli,
            Some(Mechanism::Search),
            ["files".to_string()].into(),
            vec![hidden("aws_call")],
        );
        b.searched("s", "aws account", &["aws_call".to_string()]);
        let t = b.end_turn("s", PlaceKind::Cli, NOW);
        assert_eq!((t.used.len(), t.searched), (0, vec!["aws".to_string()]));
        let s = b.stats(PlaceKind::Cli);
        assert!(s.sum_since(0, |d| &d.used).is_empty());
        assert_eq!(s.sum_since(0, |d| &d.searched)["aws"], 1);
        assert!(s.last_used.is_empty());
        // The same from a row of the ledger.
        let back = Board::default();
        back.apply_used(
            NOW,
            &serde_json::json!({"place": "cli", "loaded": ["files"], "used": [],
            "searched": ["aws"], "unused": []}),
        );
        assert!(back
            .stats(PlaceKind::Cli)
            .sum_since(0, |d| &d.used)
            .is_empty());
        // And a call is.
        b.called("s", "aws_call");
        b.end_turn("s", PlaceKind::Cli, NOW + 1);
        assert_eq!(b.stats(PlaceKind::Cli).sum_since(0, |d| &d.used)["aws"], 1);
    }

    #[test]
    fn a_fallback_search_asks_for_a_family_and_the_selection_loads_it() {
        let b = Board::default();
        b.wants(
            "s",
            "aws bucket",
            &["aws".to_string()],
            &["aws_call".to_string()],
        );
        let known: BTreeSet<String> = ["files", "aws"].iter().map(|s| s.to_string()).collect();
        let sel = b.select(
            "s",
            PlaceKind::Cli,
            &DeferConfig::default(),
            NOW,
            &Offer {
                known: &known,
                extra: &BTreeSet::new(),
                fit: None,
            },
        );
        assert!(sel.loaded().contains("aws"));
        assert_eq!(sel.families["aws"], select::Why::Wanted);
        let other = b.select(
            "t",
            PlaceKind::Cli,
            &DeferConfig::default(),
            NOW,
            &Offer {
                known: &known,
                extra: &BTreeSet::new(),
                fit: None,
            },
        );
        assert!(
            !other.loaded().contains("aws"),
            "another conversation did not ask"
        );
    }
}
