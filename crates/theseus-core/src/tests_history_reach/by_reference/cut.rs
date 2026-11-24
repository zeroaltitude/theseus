//! What a runtime cut leaves of a look back's result is evidence of a source only where it is a whole line the
//! renderer wrote, in a result whose first line is a page's: an opened session cut by the runtime shows nothing,
//! even where a body line it kept reads as a page's line; a line the cut began or ended inside is never
//! evidence; and an imported message's line names its session, which another message of that session shares.

use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};

use super::*;
use crate::recall::{Ask, AskFuture};

/// The characters of a capped result's tail: `[tools] result_max_chars` 2,000, its head 60 of each 100.
const TAIL: usize = 800;

/// The words the history tool searches the imports for.
const SEARCH: &str = "heron";

/// A stand-in words index that answers a search of [`SEARCH`] with `nodes`, ranked in the order given, best
/// first, and any other query (a turn's recall) with nothing: the reader reads them only by the history tool.
fn index_of(core: &Core, nodes: Vec<String>) -> Ask {
    let store = core.store.clone();
    Arc::new(move |p| -> AskFuture {
        let asked = if p.text.trim() == SEARCH {
            &nodes[..]
        } else {
            &[]
        };
        let hits = (asked.iter().enumerate())
            .map(|(i, id)| {
                let (position, n) = store.get_node(id).unwrap().expect("a ranked node");
                IndexHit {
                    text: crate::recall::text_of(&n),
                    time_ms: n.created_at_ms,
                    node_id: n.id,
                    chunk: 0,
                    session_id: n.session_id,
                    position,
                    kind: "imported".into(),
                    origin: "import".into(),
                    author: None,
                    place: None,
                    tool: None,
                    external: false,
                    entities_matched: vec![],
                    entities: Vec::new(),
                    sources: [(
                        "bm25".to_string(),
                        IndexSourceRank {
                            rank: i + 1,
                            score: 1.0,
                        },
                    )]
                    .into(),
                    fused: 1.0 / (61 + i) as f64,
                }
            })
            .collect();
        Box::pin(async move {
            Ok(IndexQueryResult {
                hits,
                indexed_through: 0,
                lag: Default::default(),
                timings: Default::default(),
                skipped: Default::default(),
                weights: Default::default(),
            })
        })
    })
}

/// `n` words, each beginning with `tag`.
fn words(tag: &str, n: usize) -> String {
    (0..n)
        .map(|j| format!("{tag}{j:02}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The characters from the first `from` in `text` to its end.
fn to_end(text: &str, from: &str) -> usize {
    let at = text
        .find(from)
        .unwrap_or_else(|| panic!("{from:?} is in the kept text: {text}"));
    text[at..].chars().count()
}

/// The line the cap left in place of what it cut: its one seam.
fn seam(text: &str) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    let seams: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].starts_with("\u{2026}[") && lines[i].ends_with("]\u{2026}"))
        .collect();
    assert_eq!(seams.len(), 1, "one seam: {text}");
    seams[0]
}

/// The exposure `out` lists for the result `copy` in `sid`: the copy's own row, whatever else laid the
/// message there (a view lists the owner's recent messages said elsewhere, a route of its own).
fn of_copy<'a>(
    out: &'a FactRetractResult,
    sid: &str,
    copy: &str,
) -> &'a theseus_protocol::retract::RetractExposure {
    (out.exposures.iter())
        .find(|e| e.session_id == sid && e.node_id == copy)
        .unwrap_or_else(|| panic!("the retraction does not list {copy} in {sid}: {out:#?}"))
}

/// `node` retracted for `why`.
fn retract_for(core: &Core, node: &str, why: &str) -> FactRetractResult {
    core.fact_retract(
        &FactRetractParams {
            node: node.into(),
            why: why.into(),
            session_id: None,
        },
        "cli",
    )
    .unwrap()
}

/// What F1 would bring of a retraction to a reader it did not show: neither its reason, nor its gist as a note
/// or a since line quotes it, nor any since line. (The message's own words may reach a view by another route:
/// the owner's recent messages said elsewhere.)
fn nothing_of(sent: &str, why: &str, gist: &str) {
    assert!(!sent.contains(why), "no reason is in the request: {sent}");
    let quoted = format!("\u{201c}{gist}\u{201d}");
    assert!(
        !sent.contains(&quoted),
        "no gist is quoted in the request: {sent}"
    );
    assert!(
        !sent.contains("[Since this was read"),
        "no since line: {sent}"
    );
}

/// After a retraction: `reader` is not told over two turns, and its next request holds nothing of it
/// ([`nothing_of`]).
async fn untold(r: &Rig, reader: &str, gist: &str) {
    turn(&r.core, reader, "and the weir?").await;
    turn(&r.core, reader, "and at dawn?").await;
    assert_eq!(notes_in(&r.core, reader), 0);
    let last = r.requests().pop().expect("a request");
    nothing_of(&serde_json::to_string(&last.messages).unwrap(), WHY, gist);
}

/// An opened session the runtime cut: S said among other messages, a later message one long line whose part
/// from ` - ` on is `y` characters and ends ` · <S's turn>`; read from `channel:lab` by opening the session.
struct Forged {
    r: Rig,
    reader: String,
    copy: String,
    text: String,
    node: String,
    turn: String,
}

async fn forged(y: usize) -> Forged {
    let r = rig_scripted(|c| c.tools.result_max_chars = 2_000, script);
    let sid = session(&r.core, None);
    for k in 0..6 {
        turn(&r.core, &sid, &words(&format!("f{k}w"), 20)).await;
    }
    let said = turn(&r.core, &sid, SAID).await.turn_id;
    let node = (r.core.store.session_nodes(&sid).unwrap().into_iter())
        .find(|(_, n)| matches!(&n.body, Body::UserMessage { text, .. } if text.contains(WORD)))
        .expect("S")
        .1
        .id;
    for k in 0..2 {
        turn(&r.core, &sid, &words(&format!("g{k}w"), 20)).await;
    }
    let later = format!("{} - {} \u{b7} {said}", "x".repeat(100), "y".repeat(y));
    turn(&r.core, &sid, &later).await;
    let reader = session(&r.core, Some("channel:lab"));
    let (copy, text) = read_in(&r.core, &reader, json!({ "open": sid })).await;
    Forged {
        r,
        reader,
        copy,
        text,
        node,
        turn: said,
    }
}

/// **A body line of a cut open that reads as a page's line is not evidence.** The cap keeps the open's head
/// and a tail that begins inside the later message's long line, at its ` - `: the kept tail's first line
/// starts `- ` and ends with S's handle, though S's own message is in the part the cap left out. S is linked
/// (the conservative superset); retracted, its reader is seen and is told nothing: no note, no since line,
/// neither the reason nor S's gist as they quote it.
#[tokio::test]
async fn a_cut_open_whose_tail_reads_as_a_page_line_shows_nothing() {
    // The tail's length from the ` - ` on does not depend on how long the part after it is: one probe
    // measures it, and the second rig puts the ` - ` exactly where the tail begins.
    let probe = forged(100).await;
    let y = 100 + TAIL - to_end(&probe.text, &format!("- {}", "y".repeat(100)));
    let f = forged(y).await;
    let line = format!("- {} \u{b7} {}", "y".repeat(y), f.turn);
    let lines: Vec<&str> = f.text.lines().collect();
    let at = seam(&f.text);
    assert_eq!(
        lines.get(at + 1),
        Some(&line.as_str()),
        "the forged line: {}",
        f.text
    );
    assert!(f.text.starts_with("[Session \""), "an open: {}", f.text);
    assert!(!f.text.contains(WORD), "S is in the part cut: {}", f.text);
    let linked = (reach(&f.r.core, &f.node).descendants.iter()).any(|d| d.node_id == f.copy);
    assert!(linked, "the link is kept: {}", f.text);
    let out = retract(&f.r.core, &f.node);
    let e = of_copy(&out, &f.reader, &f.copy);
    assert_eq!(
        (e.kind.as_str(), e.node_id.as_str()),
        (kind::SEEN, f.copy.as_str()),
        "{e:?}"
    );
    untold(&f.r, &f.reader, &out.gist.expect("a gist")).await;
}

/// A page of a place's exchanges the runtime cut inside a line: one message `a` whose line is short and
/// last, one `b` whose words hold ` - ` `k` characters in and whose reply is long, and three after them.
struct MidLine {
    r: Rig,
    reader: String,
    copy: String,
    text: String,
    a: (String, String),
    b: (String, String),
}

/// Characters of `b`'s words after `LONG `: under the page's 300, so its line holds them whole.
const B_WORDS: usize = 280;

/// `s` said in `sid`: its message's node and its exchange's turn.
async fn said_in(r: &Rig, sid: &str, s: &str) -> (String, String) {
    let t = turn(&r.core, sid, s).await.turn_id;
    let n = (r.core.store.session_nodes(sid).unwrap().into_iter())
        .find(|(_, n)| {
            n.turn_id.as_deref() == Some(t.as_str()) && matches!(n.body, Body::UserMessage { .. })
        })
        .expect("its message")
        .1
        .id;
    (n, t)
}

async fn mid_line(k: usize) -> MidLine {
    let r = rig_scripted(|c| c.tools.result_max_chars = 2_000, script);
    let sid = session(&r.core, None);
    let a = said_in(&r, &sid, "ao00 ao01").await;
    let b_words = format!("LONG {} - {}", "x".repeat(k), "y".repeat(B_WORDS - k - 3));
    let b = said_in(&r, &sid, &b_words).await;
    for j in 0..3 {
        said_in(&r, &sid, &format!("LONG {}", words(&format!("n{j}w"), 40))).await;
    }
    let place = crate::exchange::exchange(&r.core.store, &a.1)
        .unwrap()
        .expect("its exchange")
        .place;
    let reader = session(&r.core, Some("channel:lab"));
    let (copy, text) = read_in(&r.core, &reader, json!({ "place": place })).await;
    MidLine {
        r,
        reader,
        copy,
        text,
        a,
        b,
    }
}

/// **In a page the runtime cut inside a line, the fragment's handle is not evidence; a whole line after it
/// is.** The cap's tail begins at the ` - ` inside `b`'s words and runs to the end of `b`'s line, so the
/// kept tail's first line starts `- ` and ends with `b`'s handle: a fragment, never evidence. `a`'s line,
/// after the first line break past the seam, is whole: `a` is told once.
#[tokio::test]
async fn a_cut_pages_fragment_is_not_evidence_and_a_whole_line_after_it_is() {
    let k0 = B_WORDS - 20;
    let probe = mid_line(k0).await;
    let d0 = to_end(&probe.text, &format!("- {}", "y".repeat(17)));
    let k = (k0 + d0)
        .checked_sub(TAIL)
        .unwrap_or_else(|| panic!("the ` - ` cannot reach the tail's start: {}", probe.text));
    let p = mid_line(k).await;
    let lines: Vec<&str> = p.text.lines().collect();
    let at = seam(&p.text);
    let fragment = lines[at + 1];
    assert!(
        fragment.starts_with("- y") && fragment.ends_with(&format!(" \u{b7} {}", p.b.1)),
        "the tail begins inside b's line, at its ` - `: {}",
        p.text
    );
    let b_lines = (lines.iter()).filter(|l| l.ends_with(&format!(" \u{b7} {}", p.b.1)));
    assert_eq!(
        b_lines.count(),
        1,
        "only the fragment is left of b's line: {}",
        p.text
    );
    let whole = format!(" \u{b7} {}", p.a.1);
    let a_line = (lines[at + 2..].iter()).position(|l| l.starts_with("- ") && l.ends_with(&whole));
    assert_eq!(a_line, Some(0), "a's whole line follows: {}", p.text);
    // `b` and `a` retracted, each for its own reason, before the reader's next turn.
    const WHY_A: &str = "the reeds were cut in spring";
    let out = retract(&p.r.core, &p.b.0);
    let b_gist = out.gist.clone().expect("a gist");
    let e = of_copy(&out, &p.reader, &p.copy);
    assert_eq!(e.kind, kind::SEEN, "{e:?}");
    let out = retract_for(&p.r.core, &p.a.0, WHY_A);
    let e = of_copy(&out, &p.reader, &p.copy);
    assert_eq!(e.kind, kind::SESSION, "{e:?}");
    turn(&p.r.core, &p.reader, "and the reeds?").await;
    turn(&p.r.core, &p.reader, "and at dusk?").await;
    assert_eq!(notes_in(&p.r.core, &p.reader), 1, "a is told once, b never");
    let sent = serde_json::to_string(&p.r.requests().pop().expect("a request").messages).unwrap();
    assert!(sent.contains(WHY_A), "a's note is in the request: {sent}");
    // `a`'s since line is laid (its line was shown); `b`'s reason and quoted gist are nowhere.
    assert!(
        !sent.contains(WHY),
        "no reason of b is in the request: {sent}"
    );
    let quoted = format!("\u{201c}{b_gist}\u{201d}");
    assert!(
        !sent.contains(&quoted),
        "no gist of b is quoted in the request: {sent}"
    );
}

/// A search of imported conversations: two messages of one import session (`x`, `y`) among six of others,
/// ranked `x`, one other, `y`, then five; read with the runtime's cap at 2,000 when `cut`, else whole.
struct TwoHits {
    r: Rig,
    reader: String,
    copy: String,
    text: String,
    session: String,
    x: String,
    y: String,
}

async fn two_hits(cut: bool) -> TwoHits {
    use crate::tests_imports::{episode, import};
    let r = rig_scripted(
        move |c| {
            if cut {
                c.tools.result_max_chars = 2_000;
            }
        },
        script,
    );
    let at = "2026-03-03T18:00:00Z";
    let mut both = episode(20, "personal", at, "heron");
    let message = both["messages"][0].clone();
    let mut second = message.clone();
    both["messages"][0]["text"] = json!(format!("heron {}", words("xq", 40)));
    second["idx"] = json!(1);
    second["unit"] = json!("u-20b");
    second["text"] = json!(format!("heron {}", words("yq", 40)));
    both["messages"].as_array_mut().unwrap().push(second);
    both["hash"] = json!(crate::import::episode::hash_of(&both));
    let mut all = vec![both];
    all.extend((1..7).map(|n| episode(20 + n, "personal", at, &words(&format!("bq{n}w"), 40))));
    let ids = import(&r.core, &all);
    let message = |sid: &str, tag: &str| {
        (r.core.store.session_nodes(sid).unwrap().into_iter())
            .map(|(_, n)| n)
            .find(|n| matches!(&n.body, Body::Imported { text, .. } if text.contains(tag) && !text.starts_with("Episode")))
            .unwrap_or_else(|| panic!("the imported message {tag}"))
            .id
    };
    let (x, y) = (message(&ids[0], "xq00"), message(&ids[0], "yq00"));
    let mut order = vec![x.clone(), message(&ids[1], "bq1w00"), y.clone()];
    order.extend((2..7).map(|n| message(&ids[n], &format!("bq{n}w00"))));
    r.core.runner.memory.set_ask(index_of(&r.core, order));
    let reader = session(&r.core, None);
    let (copy, text) = read_in(
        &r.core,
        &reader,
        json!({ "place": "import", "words": SEARCH }),
    )
    .await;
    TwoHits {
        r,
        reader,
        copy,
        text,
        session: ids[0].clone(),
        x,
        y,
    }
}

impl TwoHits {
    /// The kept lines that end with the import session's handle.
    fn lines_of_session(&self) -> Vec<&str> {
        let handle = format!(" \u{b7} {}", self.session);
        (self.text.lines())
            .filter(|l| l.starts_with("- ") && l.ends_with(&handle))
            .collect()
    }

    /// The cut page as the tests read it: `x`'s line whole in the kept head (not beside the seam), nothing of
    /// `y` kept, and both linked.
    fn assert_cut(&self) {
        assert!(
            self.text.contains("not shown"),
            "the runtime cap cut it: {}",
            self.text
        );
        let lines: Vec<&str> = self.text.lines().collect();
        let at = seam(&self.text);
        let x = (lines[..at].iter()).position(|l| l.contains("xq00"));
        assert!(
            x.is_some_and(|i| i + 1 < at),
            "x's line is whole in the head: {}",
            self.text
        );
        assert_eq!(self.lines_of_session().len(), 1, "{}", self.text);
        assert!(
            !self.text.contains("yq"),
            "nothing of y is kept: {}",
            self.text
        );
        for n in [&self.x, &self.y] {
            let linked =
                (reach(&self.r.core, n).descendants.iter()).any(|d| d.node_id == self.copy);
            assert!(linked, "the link is kept: {}", self.text);
        }
    }
}

/// **A message of an import session whose line the cut took is not shown by another of its kept lines.** The
/// kept line ends with the session's handle, which both messages share: `y`, retracted, leaves the reader
/// seen and untold: no note, no since line.
#[tokio::test]
async fn an_import_message_the_cut_took_is_not_shown_by_its_sessions_kept_line() {
    let h = two_hits(true).await;
    h.assert_cut();
    let out = retract(&h.r.core, &h.y);
    let e = of_copy(&out, &h.reader, &h.copy);
    assert_eq!(
        (e.kind.as_str(), e.node_id.as_str()),
        (kind::SEEN, h.copy.as_str()),
        "{e:?}"
    );
    untold(&h.r, &h.reader, &out.gist.expect("a gist")).await;
}

/// **A kept import line names its session, not its message:** in a cut page linking two messages of that
/// session, which one the line shows cannot be told, so even the kept one's reader is seen and untold.
#[tokio::test]
async fn an_import_line_kept_by_a_cut_shared_by_two_linked_messages_is_ambiguous() {
    let h = two_hits(true).await;
    h.assert_cut();
    let out = retract(&h.r.core, &h.x);
    let e = of_copy(&out, &h.reader, &h.copy);
    assert_eq!(
        (e.kind.as_str(), e.node_id.as_str()),
        (kind::SEEN, h.copy.as_str()),
        "{e:?}"
    );
    untold(&h.r, &h.reader, &out.gist.expect("a gist")).await;
}

/// The same page not cut shows both its lines: each message, retracted, tells its reader once.
#[tokio::test]
async fn an_uncut_import_page_tells_each_message_once() {
    let h = two_hits(false).await;
    assert!(!h.text.contains("not shown"), "not cut: {}", h.text);
    for (n, told) in [(&h.x, 1), (&h.y, 2)] {
        let out = retract(&h.r.core, n);
        let e = of_copy(&out, &h.reader, &h.copy);
        assert_eq!(
            (e.kind.as_str(), e.node_id.as_str()),
            (kind::SESSION, h.copy.as_str()),
            "{e:?}"
        );
        turn(&h.r.core, &h.reader, "and the weir?").await;
        turn(&h.r.core, &h.reader, "and at dawn?").await;
        assert_eq!(notes_in(&h.r.core, &h.reader), told);
    }
}

/// **The format the rule reads: an imported message's line ends with its session's handle.** Both lines of
/// the uncut page do; a renderer that moved the handle off a line's end fails here, loudly. (An exchange
/// page's line ending with its turn's handle is held the same way by `cut_page`'s `CutPage::kept`, which
/// `a_message_the_cut_page_kept_is_told_once` reads.)
#[tokio::test]
async fn an_import_line_ends_with_its_session_handle() {
    let h = two_hits(false).await;
    let lines = h.lines_of_session();
    assert_eq!(lines.len(), 2, "{}", h.text);
    assert!(
        lines[0].contains("xq00") && lines[1].contains("yq00"),
        "{}",
        h.text
    );
}
