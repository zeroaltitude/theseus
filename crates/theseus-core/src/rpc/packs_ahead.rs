//! What a promotion's answer says when a learned version stands ahead of
//! the version moved (theseus-nwa5). Each point asks
//! `JudgeService::placed(root, session)` for the version in its root's
//! place: the newest learned one the ladder placed, a canary's only in its
//! canary arm. So while a learned version stands in shadow, or live, its
//! root's own canary or live acts in no session; while one stands as a
//! canary, only in that canary's control arm. A promotion is never refused
//! for it: its `said` names the versions ahead and the way out
//! (`theseus packs rollback <version>`).
//!
//! The check reads as `placed` does (`names_of_root`, each version's rows
//! and `ladder().standing`) without calling it, so it reads for no
//! session and never changes what a point is given.

use super::Core;
use crate::fact::ladder::mode_words;
use crate::judge::ladder::Rung;
use crate::judge::lineage::is_placed;

/// One learned version standing ahead of the one moved.
struct Ahead {
    name: String,
    rung: Rung,
    share: Option<f64>,
}

impl Ahead {
    /// "in shadow", "live", "as canary 0.3".
    fn how(&self) -> String {
        match self.rung {
            Rung::Shadow => "in shadow".into(),
            Rung::Canary => format!("as {}", mode_words("canary", self.share)),
            r => r.as_str().into(),
        }
    }
}

impl Core {
    /// The learned versions of `pack`'s lineage a point reads before it, as
    /// `placed` walks them: newest first, a canary's control arm going on to
    /// the next, a shadow or live version ending the walk. A root's are all
    /// its placed learned versions; a learned version's, the newer ones.
    fn ahead_of(&self, pack: &str) -> Vec<Ahead> {
        let j = &self.runner.judge;
        if !j.config().enabled {
            return Vec::new();
        }
        let root = j.root_of(pack);
        let l = j.ladder();
        let mut out = Vec::new();
        for name in j.lineage().names_of_root(&self.store, &root) {
            if name == pack {
                break;
            }
            let has_row = !l.rows_of(&name).iter().all(|r| r.declined);
            let s = l.standing(&name);
            if !is_placed(s.rung, has_row) {
                continue;
            }
            let ends = s.rung != Rung::Canary;
            out.push(Ahead {
                name,
                rung: s.rung,
                share: s.share,
            });
            if ends {
                break;
            }
        }
        out
    }

    /// The sentence a promotion's `said` gains when a learned version
    /// stands ahead of `pack` (with a leading space), else nothing. `card`:
    /// the move is a question not yet answered, so it "would" judge.
    pub(super) fn ahead_words(
        &self,
        pack: &str,
        mode: &str,
        share: Option<f64>,
        card: bool,
    ) -> String {
        let ahead = self.ahead_of(pack);
        let Some(last) = ahead.last() else {
            return String::new();
        };
        let root = self.runner.judge.root_of(pack);
        let stand = match ahead.as_slice() {
            [one] => format!("{} stands in {root}'s place {}", one.name, one.how()),
            many => {
                let each: Vec<String> = many
                    .iter()
                    .map(|a| format!("{} ({})", a.name, a.how()))
                    .collect();
                format!("{} stand in {root}'s place", and_list(&each))
            }
        };
        let moved = match mode {
            "canary" => format!("{pack}'s {}", mode_words(mode, share)),
            "shadow" => format!("{pack} in shadow"),
            m => format!("{pack} {}", mode_words(m, share)),
        };
        let verb = if card { "would judge" } else { "judges" };
        let names: Vec<String> = ahead.iter().map(|a| a.name.clone()).collect();
        let arms_left = if last.rung == Rung::Canary {
            let arms: Vec<String> = names.iter().map(|n| format!("{n}'s")).collect();
            match arms.as_slice() {
                [one] => format!("only in {one} control arm"),
                _ => format!("only in sessions in {} control arms", and_list(&arms)),
            }
        } else {
            "in no session".into()
        };
        let until = match names.as_slice() {
            [one] => format!("{one} moves"),
            _ => "they move".into(),
        };
        let ways: Vec<String> = names
            .iter()
            .map(|n| format!("`theseus packs rollback {n}`"))
            .collect();
        format!(
            " {stand}, so {moved} {verb} {arms_left} until {until} ({}).",
            ways.join(", ")
        )
    }
}

/// "a", "a and b", "a, b and c".
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}
