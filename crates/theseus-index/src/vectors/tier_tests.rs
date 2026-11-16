//! The hot set and the tier file over the vector table's mapped files (issue 21 of 1.0.7): what a
//! layout places, that a search over it answers as the exact scan does, and that a forgotten
//! record is in no region of any file.

use std::fs;

use super::files_tests::{mapped, nodes, stamp, vector, write_file, CUT, FULL};
use super::layout::{build_layout, choose_hot, probe_count, Reach};
use super::tier::{Layout, TierFile};
use super::*;
use crate::weights::SplitMix;

const NOW: u32 = 1_800_000_000;

/// A table over its files, settled as a tender's is once it has read the whole index.
fn settled(dir: &Path, all: &[NodeChunks], seed: u64) -> Table {
    write_file(dir, all, seed);
    let mut t = mapped(dir, all);
    t.reconciled = true;
    t.caught_up = true;
    t
}

/// Lay `t` out under a hot region of at most `cap` records, as the tender's pass does; the hot
/// and the cold records it made.
fn lay_out(t: &mut Table, cap: usize) -> (usize, usize) {
    let plan = t.plan_layout(NOW).expect("a plan");
    let (tier, hot, cold) = build_layout(&plan, cap).unwrap();
    if tier.is_some() {
        // Checked, as the tender's pass does after its check: the candidate gets its name.
        fs::rename(plan.cand_path(), &plan.tier_path).unwrap();
    }
    t.install_tier(tier.map(Arc::new));
    (hot, cold)
}

fn queries(n: u64) -> Vec<Vec<f32>> {
    let mut rng = SplitMix(11);
    (0..n)
        .map(|_| {
            let raw: Vec<f32> = (0..FULL).map(|_| rng.unit()).collect();
            embedder::l2_normalize(&raw)
        })
        .collect()
}

fn filters() -> Vec<(Option<u64>, Vec<String>, Filters)> {
    vec![
        (None, vec![], Filters::default()),
        (Some(500), vec![], Filters::default()),
        (None, vec!["ses_3".into()], Filters::default()),
        (
            None,
            vec![],
            Filters {
                sessions: vec!["ses_1".into(), "ses_4".into()],
                kinds: vec!["tool_result".into(), "user_message".into()],
                external: Some(false),
                ..Filters::default()
            },
        ),
    ]
}

/// The rows and score bits `t` answers `q` with, at this reach.
fn ask(
    t: &Table,
    q: &[f32],
    filter: &(Option<u64>, Vec<String>, Filters),
    reach: Reach,
) -> Vec<(u32, u64)> {
    let rf = t.filter(filter.0, &filter.1, &filter.2);
    t.search_reach(q, CUT, &rf, 25, reach)
        .unwrap()
        .into_iter()
        .map(|(i, s)| (i as u32, s.to_bits()))
        .collect()
}

fn node_of(t: &Table, rows: &[(u32, u64)]) -> Vec<String> {
    rows.iter()
        .map(|(i, _)| t.row_node(*i).to_string())
        .collect()
}

fn useful(node: &str) -> heat::Signal {
    heat::Signal::Label {
        node: node.into(),
        label: "useful".into(),
    }
}

/// Everything fits the budget: nothing is laid out, and a recall's search is the exact one.
#[test]
fn what_fits_the_budget_is_not_laid_out() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(300);
    let mut t = settled(tmp.path(), &all, 21);
    let (hot, cold) = lay_out(&mut t, 1_000_000);
    assert!(t.tier.is_none() && cold == 0 && hot > 300);
    assert!(!tmp
        .path()
        .join(format!("{}.tier", stamp_key(&stamp())))
        .exists());
    for q in queries(8) {
        for f in filters() {
            assert_eq!(ask(&t, &q, &f, Reach::Fast), ask(&t, &q, &f, Reach::Exact));
        }
    }
}

/// A layout that places only the hot records (the cold ones left to the scan of what no layout
/// places) answers every query, under every filter, to the last bit, as the exact scan does.
#[test]
fn a_layout_of_the_hot_records_alone_answers_as_the_exact_scan_does() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(900);
    let mut t = settled(tmp.path(), &all, 22);
    let plan = t.plan_layout(NOW).expect("a plan");
    let hot = choose_hot(&plan.scores, 400).expect("it does not fit");
    let layout = Layout {
        cut: plan.cut,
        hot: &hot,
        groups: &[],
        made_from: 0,
    };
    let file = TierFile::write(
        &plan.tier_path,
        &layout,
        &|e| (plan.scan.scale(e), plan.scan.q(e).to_vec()),
        &|e| plan.scan.hash(e),
        plan.n,
    )
    .unwrap();
    t.tier = Some(Arc::new(file));
    let tier = t.tier.clone().expect("a layout");
    assert_eq!(
        (tier.hot_n(), tier.assigned(), tier.groups()),
        (400, 400, 0)
    );
    for (n, q) in queries(24).iter().enumerate() {
        let f = &filters()[n % 4];
        let want = ask(&t, q, f, Reach::Exact);
        assert_eq!(want.len(), 25);
        assert_eq!(ask(&t, q, f, Reach::Fast), want, "query {n}");
    }
}

/// The hot set is chosen by what the signals say: a vouched node's records are hot whatever
/// their age, a doubted one's never are, and a laid-out file places each record once.
#[test]
fn the_hot_set_is_what_the_signals_say() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(900);
    let mut t = settled(tmp.path(), &all, 23);
    let vouched = ["nd_00000011", "nd_00000012", "nd_00000013"];
    for n in vouched {
        t.signals.apply(&useful(n));
    }
    t.signals.apply(&heat::Signal::Label {
        node: "nd_00000021".into(),
        label: "wrong".into(),
    });
    let (hot, _) = lay_out(&mut t, 60);
    assert_eq!(hot, 60);
    let tier = t.tier.clone().unwrap();
    let in_hot = |e: u32| (0..tier.hot_n()).any(|p| tier.entry(p) == e);
    for n in vouched {
        for i in t.rows_of(n) {
            let (_, e) = t.row_vec(i).expect("a vector");
            assert!(in_hot(e), "{n} is vouched for and not hot");
        }
    }
    // A doubted node is not hot even when the budget has room for nearly everything.
    let plan = t.plan_layout(NOW).expect("a plan");
    let records = plan
        .scores
        .iter()
        .filter(|s| **s > f32::NEG_INFINITY)
        .count();
    let (hot, _) = lay_out(&mut t, records - 5);
    let tier = t.tier.clone().expect("a layout");
    assert_eq!(tier.hot_n(), hot);
    for i in t.rows_of("nd_00000021") {
        if let Some((_, e)) = t.row_vec(i) {
            assert!(!(0..tier.hot_n()).any(|p| tier.entry(p) == e));
        }
    }
    let mut seen = HashSet::new();
    for p in 0..tier.assigned() {
        assert!(seen.insert(tier.entry(p)), "a record placed twice");
    }
}

/// A record embedded after the layout is searched like the hot ones: new text is hot.
#[test]
fn a_record_embedded_since_the_layout_is_searched() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(900);
    let mut t = settled(tmp.path(), &all, 24);
    lay_out(&mut t, 400);
    let mut rng = SplitMix(99);
    let v = vector(&mut rng);
    let hash = 0xFEED_u128 << 64 | 7;
    t.caches[0].append(&[(hash, &v)]).unwrap();
    t.add_node(&NodeChunks {
        node_id: "nd_new".into(),
        position: 5,
        time_ms: 0,
        session: "ses_1".into(),
        kind: "user_message".into(),
        external: false,
        chunks: vec![ChunkKey {
            chunk: 0,
            hash,
            tokens: 10,
        }],
    });
    let f = &filters()[0];
    let hits = ask(&t, &v.full, f, Reach::Fast);
    assert_eq!(
        node_of(&t, &hits).first().map(String::as_str),
        Some("nd_new")
    );
}

/// A forgotten node is in no region and no file: the layout goes with the compaction that drops
/// its records, and neither reach returns it.
#[test]
fn a_forgotten_record_is_in_no_region() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(900);
    let mut t = settled(tmp.path(), &all, 25);
    lay_out(&mut t, 400);
    let path = t.caches[0].tier_path();
    assert!(path.exists());
    let row = t.rows_of("nd_00000031")[0];
    let probe = t.full_of(row as usize).unwrap().expect("a vector");
    t.remove_node("nd_00000031");
    let done = Vectors::compact_files(&mut t, tmp.path(), &|t, h| !t.holds(h)).unwrap();
    assert!(done.dropped > 0);
    assert!(t.tier.is_none(), "the layout of the old numbers is dropped");
    assert!(!path.exists(), "and so is its file");
    for reach in [Reach::Fast, Reach::Exact] {
        let hits = ask(&t, &probe, &filters()[0], reach);
        assert!(!node_of(&t, &hits).contains(&"nd_00000031".to_string()));
    }
}

/// A tier file made before a compaction renumbered the records is not trusted, nor is a torn one
/// or one whose order column changed.
#[test]
fn a_tier_file_that_is_not_the_scan_files_is_left_out() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(900);
    let mut t = settled(tmp.path(), &all, 26);
    lay_out(&mut t, 400);
    let path = t.caches[0].tier_path();
    let whole = fs::read(&path).unwrap();
    assert!(layout::open_tier(&t.caches[0]).is_some());
    // Torn.
    fs::write(&path, &whole[..whole.len() - 1]).unwrap();
    assert!(layout::open_tier(&t.caches[0]).is_none());
    assert!(!path.exists(), "a file left out is removed");
    // An order column that does not check.
    let mut bad = whole.clone();
    bad[200] ^= 0xFF;
    fs::write(&path, &bad).unwrap();
    assert!(layout::open_tier(&t.caches[0]).is_none());
    // Made before a record was dropped.
    t.remove_node("nd_00000031");
    Vectors::compact_files(&mut t, tmp.path(), &|t, h| !t.holds(h)).unwrap();
    fs::write(&path, &whole).unwrap();
    assert!(layout::open_tier(&t.caches[0]).is_none());
}

/// A restart reads the layout back: the file is opened, and answers as it did.
#[test]
fn a_restart_finds_the_layout_again() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(900);
    let mut t = settled(tmp.path(), &all, 27);
    lay_out(&mut t, 400);
    let q = &queries(1)[0];
    let want = ask(&t, q, &filters()[0], Reach::Fast);
    let (hot, cold) = (400, t.tier.clone().unwrap().assigned() - 400);
    let again = layout::open_tier(&t.caches[0]).expect("the file opens");
    let (opened_hot, assigned) = (again.hot_n(), again.assigned());
    t.tier = Some(Arc::new(again));
    assert_eq!((opened_hot, assigned), (hot, hot + cold));
    assert_eq!(ask(&t, q, &filters()[0], Reach::Fast), want);
}

/// Records drawn round a few anchor points, as real embeddings gather round their subjects and
/// random vectors do not: the structure the groups are for.
fn planted_file(dir: &Path, nodes: &[NodeChunks], seed: u64) {
    const ANCHORS: usize = 24;
    let mut rng = SplitMix(seed);
    let anchors: Vec<Vec<f32>> = (0..ANCHORS)
        .map(|_| (0..FULL).map(|_| rng.unit()).collect())
        .collect();
    let mut cache = Cache::open(dir, &stamp()).unwrap();
    let mut seen = HashSet::new();
    let mut items: Vec<(u128, Vector)> = Vec::new();
    for n in nodes {
        for c in &n.chunks {
            if seen.insert(c.hash) {
                let a = &anchors[c.hash as usize % ANCHORS];
                let raw: Vec<f32> = a.iter().map(|x| x + 0.15 * rng.unit()).collect();
                let (full, cut) = embedder::finish(&raw, CUT);
                items.push((
                    c.hash,
                    Vector {
                        full,
                        cut,
                        tokens: 1,
                        windowed: false,
                        truncated: false,
                    },
                ));
            }
        }
    }
    for part in items.chunks(500) {
        let refs: Vec<(u128, &Vector)> = part.iter().map(|(h, v)| (*h, v)).collect();
        cache.append(&refs).unwrap();
    }
}

fn planted_table(dir: &Path, nodes: usize, seed: u64) -> Table {
    let all = super::files_tests::nodes(nodes as u64);
    planted_file(dir, &all, seed);
    let mut t = mapped(dir, &all);
    t.reconciled = true;
    t.caught_up = true;
    t
}

/// The cold part in groups, a search reading the nearest eighth of them, keeps nearly all of the
/// exact search's best ten on records that are near each other.
#[test]
fn the_nearest_groups_keep_what_a_query_is_near() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = planted_table(tmp.path(), 1500, 31);
    let (hot, cold) = lay_out(&mut t, 300);
    assert_eq!(hot, 300);
    let tier = t.tier.clone().expect("a layout");
    // About the square root of the cold count, each record in one group.
    assert!(
        tier.groups() > 30 && tier.groups() < 80,
        "{}",
        tier.groups()
    );
    assert_eq!(tier.assigned(), hot + cold);
    assert_eq!(probe_count(540), 68);
    assert_eq!(probe_count(1), 1);
    let mut rng = SplitMix(77);
    let f = &filters()[0];
    let mut share = 0.0;
    let tries = 80;
    for k in 0..tries {
        let row = (k * 37 % t.row_count()) as u32;
        let own = t.full_of(row as usize).unwrap().expect("a vector");
        let noisy: Vec<f32> = own.iter().map(|x| x + 0.05 * rng.unit()).collect();
        let q = embedder::l2_normalize(&noisy);
        let exact: HashSet<u32> = ask(&t, &q, f, Reach::Exact)[..10]
            .iter()
            .map(|x| x.0)
            .collect();
        let fast: HashSet<u32> = ask(&t, &q, f, Reach::Fast)[..10]
            .iter()
            .map(|x| x.0)
            .collect();
        share += exact.intersection(&fast).count() as f64 / 10.0;
    }
    let share = share / f64::from(tries as u32);
    assert!(share >= 0.9, "the groups kept {share:.3} of the best ten");
}

/// A node forgotten from the table is in no group's answer, even before the files are compacted:
/// a record names rows, and only alive rows answer.
#[test]
fn a_forgotten_node_is_not_returned_from_a_cold_group() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = planted_table(tmp.path(), 1500, 32);
    lay_out(&mut t, 300);
    let tier = t.tier.clone().expect("a layout");
    let hot: HashSet<u32> = (0..tier.hot_n()).map(|p| tier.entry(p)).collect();
    // A node whose first chunk's record is cold and in a group.
    let (node, row) = (0..t.row_count() as u32)
        .find_map(|i| {
            let (_, e) = t.row_vec(i)?;
            (tier.places(e) && !hot.contains(&e)).then(|| (t.row_node(i).to_string(), i))
        })
        .expect("a cold record");
    let own = t.full_of(row as usize).unwrap().expect("a vector");
    let q = embedder::l2_normalize(&own);
    let before = ask(&t, &q, &filters()[0], Reach::Fast);
    assert!(
        node_of(&t, &before).contains(&node),
        "found while it is held"
    );
    t.remove_node(&node);
    for reach in [Reach::Fast, Reach::Exact] {
        let after = ask(&t, &q, &filters()[0], reach);
        assert!(!node_of(&t, &after).contains(&node), "{reach:?}");
    }
}

/// A node of the table whose first record is cold and in a group, and that row.
fn a_cold_node(t: &Table) -> (String, u32) {
    let tier = t.tier.clone().expect("a layout");
    (0..t.row_count() as u32)
        .find_map(|i| {
            let (_, e) = t.row_vec(i)?;
            (tier.places(e) && !tier.is_hot(e)).then(|| (t.row_node(i).to_string(), i))
        })
        .expect("a cold record")
}

/// A cold record a turn used is searched with the hot set from then on (and no row answers
/// twice for it), and the next layout makes it hot, which ends the promotion; a restart finds
/// the promotions again from the signals.
#[test]
fn a_used_cold_record_is_promoted_until_a_layout_makes_it_hot() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = planted_table(tmp.path(), 1500, 33);
    lay_out(&mut t, 300);
    let (node, _) = a_cold_node(&t);
    t.signals.apply(&heat::Signal::Used {
        node: node.clone(),
        at_ms: 1_700_000_000_000,
    });
    t.promote_node(&node);
    let held = t.rows_of(&node).len();
    assert!(!t.promoted.is_empty() && t.promoted.len() <= held);
    for k in 0..20 {
        let row = (k * 53 % t.row_count()) as u32;
        let q = embedder::l2_normalize(&t.full_of(row as usize).unwrap().expect("a vector"));
        let hits = ask(&t, &q, &filters()[0], Reach::Fast);
        let rows: HashSet<u32> = hits.iter().map(|h| h.0).collect();
        assert_eq!(rows.len(), hits.len(), "a row came back twice");
    }
    // A restart: the layout is read back and the promotions follow from the signals.
    let before = t.promoted.clone();
    let again = layout::open_tier(&t.caches[0]).expect("the file opens");
    t.install_tier(Some(Arc::new(again)));
    assert_eq!(t.promoted, before);
    // The next layout: the node is hot, and there is nothing left to promote.
    lay_out(&mut t, 300);
    let tier = t.tier.clone().unwrap();
    for i in t.rows_of(&node) {
        if let Some((_, e)) = t.row_vec(i) {
            assert!(tier.is_hot(e), "a used node is hot after the layout");
        }
    }
    assert!(t.promoted.is_empty());
}

/// A tender's table with a hot region of `hot` records, for the passes that live in `Vectors`.
fn in_a_tender(dir: &Path, t: Table, hot: usize) -> Vectors {
    let mut cfg = VectorConfig::new(Some(dir.join("weights")));
    cfg.hot_records = Some(hot);
    let mut v = Vectors::new(cfg, dir);
    v.table = RwLock::new(t);
    v
}

/// A layout of records with no structure to group by keeps little of the exact best ten: it is
/// not put in place, no file is left, the search stays exact, the status and the log say so, and
/// nothing asks again until the store has grown.
#[test]
fn a_layout_that_keeps_too_little_is_not_put_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let t = settled(tmp.path(), &nodes(1500), 41);
    let v = in_a_tender(tmp.path(), t, 200);
    assert_eq!(v.layout_due(), Some("over the budget"));
    v.layout_now("test");
    assert!(v.table.read().unwrap().tier.is_none());
    let failed = v
        .layouts
        .lock()
        .unwrap()
        .failed
        .expect("it failed its check");
    assert!(failed.share < failed.bar, "{}", failed.share);
    assert!(!tmp
        .path()
        .join(format!("{}.tier", stamp_key(&stamp())))
        .exists());
    let s = v.tier_status().expect("a status");
    assert_eq!(s.mode, "exact");
    assert!(s.why_exact.unwrap().contains("must keep 97%"));
    assert_eq!(v.layout_due(), None, "not again until the store has grown");
    // Exact still answers.
    let q = &queries(1)[0];
    let t = v.table.read().unwrap();
    assert_eq!(
        ask(&t, q, &filters()[0], Reach::Fast),
        ask(&t, q, &filters()[0], Reach::Exact)
    );
}

/// Records that group well pass their own check and are put in place; the check's share is
/// recorded and the status says the vector side is tiered.
#[test]
fn a_layout_that_passes_its_check_is_put_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let t = planted_table(tmp.path(), 1500, 34);
    let v = in_a_tender(tmp.path(), t, 300);
    v.layout_now("test");
    let s = v.tier_status().expect("a status");
    assert_eq!(s.mode, "tiered", "{s:?}");
    assert_eq!(s.hot, 300);
    assert!(
        s.kept.unwrap() >= f64::from(layout::KEEP_BAR_RECORDS),
        "{:?}",
        s.kept
    );
    assert!(s.groups > 30 && s.layouts == 1);
    assert_eq!(v.layout_due(), None);
    // A full promotion list asks for a layout.
    v.table
        .write()
        .unwrap()
        .promoted
        .extend(1_000_000..1_002_048u32);
    assert_eq!(v.layout_due(), Some("promoted"));
}

/// The signals the tender reads are kept beside the vectors, and a forgotten node leaves them.
#[test]
fn signals_are_kept_across_a_restart_and_a_forgotten_node_leaves_them() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("vectors")).unwrap();
    let v = Vectors::new(
        VectorConfig::new(Some(tmp.path().join("weights"))),
        tmp.path(),
    );
    v.on_signals(&[
        heat::Signal::Used {
            node: "nd_a".into(),
            at_ms: 9_000,
        },
        useful("nd_b"),
    ]);
    let file = tmp.path().join("vectors").join("heat.log");
    assert_eq!(heat::Signals::load(&file).unwrap().0.len(), 2);
    v.forget_signals(&["nd_a".to_string()]);
    let (left, _) = heat::Signals::load(&file).unwrap();
    assert_eq!((left.len(), left.get("nd_a").used), (1, 0));
    assert!(v.table.read().unwrap().signals.get("nd_b").wants_hot());
}

/// A layout is written under another name until it has passed its check, so a kill during the
/// check leaves no layout that was never checked; checked, it gets its name.
#[test]
fn a_layout_has_its_name_only_once_it_is_checked() {
    let tmp = tempfile::tempdir().unwrap();
    let t = planted_table(tmp.path(), 1500, 35);
    let plan = t.plan_layout(NOW).expect("a plan");
    let (tier, _, _) = build_layout(&plan, 300).unwrap();
    assert!(tier.is_some());
    assert!(plan.cand_path().exists() && !plan.tier_path.exists());
    let v = in_a_tender(tmp.path(), t, 300);
    fs::remove_file(plan.cand_path()).unwrap();
    v.layout_now("test");
    assert!(plan.tier_path.exists() && !plan.cand_path().exists());
    assert_eq!(v.tier_status().unwrap().mode, "tiered");
}

/// Enough real queries served, and a layout is checked on them (and the status says so).
#[test]
fn a_layout_is_checked_on_real_queries_once_there_are_enough() {
    let tmp = tempfile::tempdir().unwrap();
    let t = planted_table(tmp.path(), 1500, 36);
    let queries: Vec<Vec<f32>> = (0..40)
        .map(|k| {
            let row = (k * 41 % t.row_count()) as u32;
            embedder::l2_normalize(&t.full_of(row as usize).unwrap().expect("a vector"))
        })
        .collect();
    let v = in_a_tender(tmp.path(), t, 300);
    for q in &queries {
        v.remember_query(q);
    }
    v.layout_now("test");
    let s = v.tier_status().expect("a status");
    assert_eq!(s.checked_on.as_deref(), Some("queries"), "{s:?}");
    assert_eq!(s.mode, "tiered");
}

/// A promoted record the layout makes hot is read once: a promotion made against an older layout
/// never doubles a row in a newer one.
#[test]
fn a_promoted_record_that_is_hot_in_this_layout_is_read_once() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = planted_table(tmp.path(), 1500, 37);
    lay_out(&mut t, 300);
    let tier = t.tier.clone().unwrap();
    let q = &queries(1)[0];
    let before = ask(&t, q, &filters()[0], Reach::Fast);
    t.promoted.insert(tier.entry(0));
    t.promoted.insert(tier.entry(1));
    let after = ask(&t, q, &filters()[0], Reach::Fast);
    assert_eq!(before, after);
    let rows: HashSet<u32> = after.iter().map(|x| x.0).collect();
    assert_eq!(rows.len(), after.len(), "a row came back twice");
}
