//! What the three chat acts do, and the one gate each takes.
//!
//! Every one of them goes through the verb the buttons and the CLI already
//! take — [`Core::confirm_action_with`], [`Core::undo_allow_taken`],
//! [`Core::sync_request`] — so **nothing about what any of these acts MEANS is
//! widened here: only which inputs reach them.** `judge_act` judges a call
//! made from a tool exactly as it judges one typed at a terminal, and it is
//! asked about the OWNER, from the message that started the turn, never about
//! the model.

use anyhow::Result;
use serde_json::{json, Value};
use theseus_protocol::work::undo::{allowed_words, HeldCall, SHORTEST_ID};
use theseus_protocol::ConfirmRequest;

use super::tools::{allow_args, answer_args, named_hold, named_question};
use super::{Board, ALLOW};
use crate::toolrun::holds::Holds;
use crate::toolrun::TurnCtx;
use crate::Core;

/// What a chat act gives back: the words the model reads, and the meta the
/// ledger and the web UI read.
type Done = Result<(String, Value), String>;

/// Every question waiting that the owner is being asked here: this session's,
/// and those of a session that answers at this one's place (a task's own asks
/// where its parent does). A place of its own and nothing else: an answer
/// given in one conversation never answers another's.
///
/// Moved here whole from `rpc/answer_in_words.rs`, which is gone: what it
/// decided about SCOPE was right and is unchanged. Only its reading of the
/// owner's words went.
pub fn waiting_here(core: &Core, session: &str) -> Result<Vec<ConfirmRequest>> {
    let here = core.outbox.target(session);
    Ok(core
        .confirm_list()?
        .into_iter()
        .filter(|q| {
            q.session_id == session || (here.is_some() && core.outbox.target(&q.session_id) == here)
        })
        .collect())
}

/// Every call held that the owner is being offered here, by the same rule.
/// Waiting for an allowance only: a hold he has already allowed is not
/// offered again. Moved here whole from `rpc/allow_in_words.rs`.
pub fn held_here(core: &Core, session: &str) -> Result<Vec<HeldCall>> {
    let here = core.outbox.target(session);
    Ok(Holds::read(&core.store)?
        .list(None, false)
        .into_iter()
        .filter(|h| h.allowed.is_none())
        .filter(|h| {
            h.session_id == session || (here.is_some() && core.outbox.target(&h.session_id) == here)
        })
        .collect())
}

impl Board {
    /// The core and the message that started this turn, or the refusal the
    /// model reads. **The structural rule lives here**: a call made in a turn
    /// no message of the owner's started — a wake, a task's report, a model
    /// naming a tool it was never offered — gets nothing.
    fn standing(&self, tc: &TurnCtx<'_>) -> Result<(std::sync::Arc<Core>, super::Offered), String> {
        let core = self
            .core()
            .ok_or("this tool is not ready: the core is not built")?;
        let offered = self.offered(tc.session_id, tc.turn_id).ok_or(
            "this tool acts only in a turn the operator's own message started, and no message of \
             theirs started this one. Nothing was done.",
        )?;
        Ok((core, offered))
    }

    /// Answer the question waiting here with the answer the operator gave.
    pub(crate) fn answer(&self, tc: &TurnCtx<'_>, input: &Value) -> Done {
        let (core, offered) = self.standing(tc)?;
        let a = answer_args(input)?;
        // What this turn was OFFERED, not what is listed now: a question is
        // not listed while a turn runs in its session (`chat_acts::Offered`).
        let waiting = &offered.waiting;
        let q = one_of(waiting, a.question.as_deref(), |q| q.correlation_id.clone())
            .map_err(|e| e.said("question", "waiting", waiting, named_question))?;
        let note = a.note.as_deref().map(str::trim).filter(|t| !t.is_empty());
        let answer = a.answer.trim();
        let name = named_question(q);
        let out = match crate::rpc::answer::kind_of(q) {
            theseus_protocol::work::QuestionKind::Ask => {
                core.answer_ask_act(q, answer, note, offered.said.who.clone())
            }
            _ => core.answer_plain_act(q, answer, note, offered.said.who.clone()),
        }?;
        // What the harness made of it, for the rest of this turn
        // (`yes_fact.rs`): the block says what was answered and what was not,
        // so a turn cannot read a silence as either.
        core.stage_answer_fact(tc.session_id, &name, waiting, q);
        Ok((
            format!("{out} ({name}). Nothing else waiting here was answered."),
            json!({"chat_act": {"answered": q.correlation_id, "answer": answer}}),
        ))
    }

    /// Let the held call run as it was first proposed.
    ///
    /// Two layers, in this order, and the order is the owner's ruling:
    /// **the model decides**, guided by the tool's description, and then the
    /// `allow_said.v1` judge is asked as defence in depth. A `no` at its bar
    /// refuses with a reason the model can read; a judge that cannot be
    /// reached leaves the call STANDING and the ledger row records that it was
    /// not judged (see `chat_acts::judged`).
    pub(crate) async fn allow(&self, tc: &TurnCtx<'_>, input: &Value) -> Done {
        let (core, offered) = self.standing(tc)?;
        let said = &offered.said;
        if !said.owner_input {
            return Err(
                "this tool acts only on the operator's own message, from a private \
                        place, and this turn's message is not one. Nothing was allowed."
                    .into(),
            );
        }
        let a = allow_args(input)?;
        let held = &offered.held;
        let h = one_of(held, a.hold.as_deref(), |h| h.id.clone())
            .map_err(|e| e.said("hold", "held", held, named_hold))?
            .clone();
        let judged = match super::judged::says_run(&core, &said.text, &h, held).await {
            super::judged::Verdict::No(why) => {
                return Err(format!(
                    "The operator's message does not read as an instruction to run the held call \
                     ({}). Nothing was allowed and the hold stands. {why} Ask them plainly \
                     whether to run it as it was first proposed.",
                    named_hold(&h)
                ))
            }
            super::judged::Verdict::Yes => Some(true),
            // NOT `None`. `None` is the TYPED routes, which ask no second
            // reading at all; `Some(false)` is this route saying that it asked
            // and no judge answered. The fail-open rule owes the ledger that
            // difference, and a row that simply omitted the field would read
            // as `undo.allow` over the wire.
            super::judged::Verdict::Unjudged => Some(false),
        };
        let allowed = core
            .undo_allow_taken(
                &h.id,
                said.who.clone(),
                crate::rpc::Taken {
                    route: Some(ALLOW),
                    judged,
                },
            )
            .map_err(|e| format!("the hold was not allowed: {e:#}"))?;
        Ok((
            allowed_words(&allowed),
            json!({"chat_act": {"allowed": allowed.id, "judged": judged}}),
        ))
    }

    /// Ask for an OpenClaw import sync now.
    ///
    /// **The owner check is `import.sync.request`'s own and no other**:
    /// `judge_act(Act::Import)`, which falls to `places::owner_in_private` —
    /// the CLI, the web UI, a DM with an owner, a thread's own virtual place,
    /// or a guild channel the bindings file bound private. No new place rule
    /// is invented here, and the actor judged is the operator who spoke, never
    /// the model.
    pub(crate) fn sync(&self, tc: &TurnCtx<'_>, input: &Value) -> Done {
        let (core, offered) = self.standing(tc)?;
        let said = &offered.said;
        let _: super::tools::SyncArgs = theseus_tools::parse(input).map_err(|e: String| e)?;
        core.import_act(&said.who)?;
        let started = core.sync_request_for(Some(tc.session_id), &said.who.label);
        Ok((
            started.say.clone(),
            json!({"chat_act": {"sync_started": started.started, "at_ms": started.at_ms}}),
        ))
    }
}

/// Why exactly one of something could not be picked.
pub(crate) enum NotOne {
    /// Nothing of the kind is here at all.
    None,
    /// A name that matches nothing here.
    Unknown(String),
    /// More than one, and the words name none or several.
    Which(Vec<usize>),
}

impl NotOne {
    /// The refusal the model reads, naming each candidate so the next turn can
    /// ask the operator which they mean. **Nothing is ever guessed**: the rule
    /// `undo.allow` takes by id is the rule here.
    fn said<T>(self, field: &str, kind: &str, all: &[T], name: impl Fn(&T) -> String) -> String {
        let list = |ix: &[usize]| {
            ix.iter()
                .filter_map(|i| all.get(*i))
                .map(|t| format!("\n- {}", name(t)))
                .collect::<String>()
        };
        match self {
            NotOne::None => format!("Nothing is {kind} in this conversation. Nothing was done."),
            NotOne::Unknown(w) => format!(
                "`{w}` names nothing {kind} here. Nothing was done. What is {kind} here:{}",
                list(&(0..all.len()).collect::<Vec<_>>())
            ),
            NotOne::Which(ix) => format!(
                "`{field}` names {} of the {} {kind} here, so which one is meant cannot be read. \
                 Nothing was done — ask the operator which they mean:{}",
                match ix.len() {
                    n if n == all.len() => "all".to_string(),
                    n => n.to_string(),
                },
                all.len(),
                list(&ix)
            ),
        }
    }
}

/// Whether `id` is what `want` names: the whole id, or an end of
/// [`SHORTEST_ID`] characters or more, lower case — **exactly**
/// `work::undo::pick`'s rule, applied to a question's correlation id as well
/// as to a hold's, so the two vocabularies cannot drift. `pick` itself is
/// typed to `HeldCall`, which a `ConfirmRequest` is not; the rule is one line
/// and is stated once here rather than fabricated holds being passed to it.
fn names(id: &str, want: &str) -> bool {
    let id = id.to_lowercase();
    id == want || id.ends_with(want)
}

/// The one of `all` that `name` means. With no name and exactly one
/// candidate, that one; with no name and several, a refusal. **Nothing is
/// ever guessed** — the rule that produced the wrong task being abandoned on
/// a bare yes (S17) is the rule here.
fn one_of<'a, T>(
    all: &'a [T],
    name: Option<&str>,
    id: impl Fn(&T) -> String,
) -> Result<&'a T, NotOne> {
    if all.is_empty() {
        return Err(NotOne::None);
    }
    let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) else {
        return match all.len() {
            1 => Ok(&all[0]),
            _ => Err(NotOne::Which((0..all.len()).collect())),
        };
    };
    let want = name.to_lowercase();
    if want.len() < SHORTEST_ID {
        return Err(NotOne::Unknown(name.to_string()));
    }
    let whole: Vec<usize> = (0..all.len())
        .filter(|i| id(&all[*i]).to_lowercase() == want)
        .collect();
    let ix = match whole.is_empty() {
        false => whole,
        true => (0..all.len())
            .filter(|i| names(&id(&all[*i]), &want))
            .collect(),
    };
    match ix.as_slice() {
        [] => Err(NotOne::Unknown(name.to_string())),
        [i] => Ok(&all[*i]),
        many => Err(NotOne::Which(many.to_vec())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A held call, as [`held_here`] hands them to [`one_of`].
    fn a_hold(id: &str, tool: &str) -> HeldCall {
        HeldCall {
            id: id.into(),
            session_id: "ses_1".into(),
            turn_id: None,
            tool: tool.into(),
            input: json!({"path": "/w/notes.txt"}),
            summary: format!("{tool} notes.txt"),
            judgment: "jdg_1".into(),
            shape: None,
            held_ms: 1,
            allowed: None,
            ran_ms: None,
        }
    }

    /// A question waiting, as [`waiting_here`] hands them to [`one_of`].
    fn a_question(id: &str, tool: &str) -> ConfirmRequest {
        ConfirmRequest {
            correlation_id: id.into(),
            session_id: "ses_1".into(),
            tool: tool.into(),
            ..Default::default()
        }
    }

    /// The hold `name` picks, or `<refused>`: [`NotOne`] carries no `Debug`,
    /// and a case that cares which refusal it was matches on it below.
    fn picks(all: &[HeldCall], name: Option<&str>) -> String {
        one_of(all, name, |h| h.id.clone())
            .map(|h| h.id.clone())
            .unwrap_or_else(|_| "<refused>".into())
    }

    /// Two held calls, nothing to tell them apart.
    fn two() -> [HeldCall; 2] {
        [
            a_hold("hld_1a2b3c4d", "fs.write"),
            a_hold("hld_5e6f7a8b", "fs.patch"),
        ]
    }

    /// **Nothing is ever guessed.** With two held and no name the tool
    /// refuses and names both, so the next turn can ask which is meant; with
    /// one held there is nothing to guess between and it is taken.
    ///
    /// This is S17's rule — the one that abandoned the wrong task on a bare
    /// yes — kept for the tool that replaced the matcher.
    #[test]
    fn with_two_held_and_no_name_nothing_is_guessed_and_both_are_named() {
        let two = two();
        let said = match one_of(&two, None, |h| h.id.clone()) {
            Ok(h) => panic!("it guessed {}", h.id),
            Err(NotOne::Which(ix)) => {
                assert_eq!(ix, vec![0, 1], "both are candidates");
                NotOne::Which(ix).said("hold", "held", &two, named_hold)
            }
            Err(NotOne::None) => panic!("two are held"),
            Err(NotOne::Unknown(w)) => panic!("no name was given, not `{w}`"),
        };
        assert!(said.contains("names all of the 2 held here"), "{said}");
        assert!(said.contains("Nothing was done"), "{said}");
        assert!(said.contains("ask the operator which they mean"), "{said}");
        assert!(said.contains("hld_1a2b3c4d"), "the first is named: {said}");
        assert!(said.contains("hld_5e6f7a8b"), "the second is named: {said}");

        // One held and no name: that one. A blank name is no name.
        let one = [a_hold("hld_1a2b3c4d", "fs.write")];
        assert_eq!(picks(&one, None), "hld_1a2b3c4d");
        assert_eq!(picks(&one, Some("   ")), "hld_1a2b3c4d");
        assert_eq!(picks(&two, Some("  ")), "<refused>", "and still no guess");
    }

    /// A hold is named by its WHOLE id, or by an end of at least
    /// [`SHORTEST_ID`] characters — `work::undo::pick`'s rule, so the tool's
    /// vocabulary and `undo.allow`'s cannot drift. Case does not matter.
    #[test]
    fn a_hold_is_named_by_its_whole_id_or_by_an_end_of_four() {
        let two = two();
        assert_eq!(picks(&two, Some("hld_5e6f7a8b")), "hld_5e6f7a8b");
        assert_eq!(picks(&two, Some("HLD_5E6F7A8B")), "hld_5e6f7a8b");
        assert_eq!(
            picks(&two, Some("3c4d")),
            "hld_1a2b3c4d",
            "an end of exactly SHORTEST_ID"
        );
        assert_eq!(
            picks(&two, Some("1a2b3c4d")),
            "hld_1a2b3c4d",
            "a longer end"
        );
        assert_eq!(SHORTEST_ID, 4, "the floor the case above sits on");

        // A whole id wins outright over an end that would match more, so the
        // exact thing the operator named is never lost to a near miss.
        let shadowed = [
            a_hold("hld_aaaa1111", "fs.write"),
            a_hold("aaaa1111", "fs.patch"),
        ];
        assert_eq!(picks(&shadowed, Some("aaaa1111")), "aaaa1111");
    }

    /// A name under the floor picks nothing, and **a TOOL no longer names a
    /// hold.** The matcher read the operator's whole sentence and could take
    /// `fs.patch` out of it; the tool is given one name by the model and
    /// compares it against ids alone. Narrower on purpose: it refuses rather
    /// than resolving a word that is not an id.
    #[test]
    fn a_name_under_the_floor_or_naming_a_tool_picks_nothing() {
        let two = two();
        for under in ["c4d", "4d", "d", ""] {
            assert_eq!(picks(&two, Some(under)), "<refused>", "`{under}`");
        }
        match one_of(&two, Some("c4d"), |h| h.id.clone()) {
            Err(NotOne::Unknown(w)) => assert_eq!(w, "c4d", "it is named back"),
            Ok(h) => panic!("`c4d` picked {}", h.id),
            Err(_) => panic!("a name under the floor is Unknown, not None or Which"),
        }
        let said = match one_of(&two, Some("fs.patch"), |h| h.id.clone()) {
            Err(NotOne::Unknown(w)) => {
                assert_eq!(w, "fs.patch");
                NotOne::Unknown(w).said("hold", "held", &two, named_hold)
            }
            Ok(h) => panic!("a tool named a hold: {}", h.id),
            Err(_) => panic!("a tool name is Unknown"),
        };
        assert!(
            said.contains("`fs.patch` names nothing held here"),
            "{said}"
        );
        assert!(
            said.contains("hld_1a2b3c4d"),
            "what is held is listed: {said}"
        );
        assert!(said.contains("hld_5e6f7a8b"), "{said}");
    }

    /// **An end that matches two is still ambiguous**, and naming them BOTH
    /// in one string refuses too. Either way nothing is done and the model is
    /// told to ask: the removed matcher's `yes to aaaa1111 and bbbb2222` is
    /// this case, and it still refuses.
    #[test]
    fn a_name_that_fits_two_refuses_and_so_does_naming_both() {
        let twins = [
            a_hold("hld_aaaa1111", "fs.write"),
            a_hold("hld_bbbb1111", "fs.write"),
        ];
        match one_of(&twins, Some("1111"), |h| h.id.clone()) {
            Err(NotOne::Which(ix)) => assert_eq!(ix, vec![0, 1], "both ends match"),
            Ok(h) => panic!("`1111` picked {}", h.id),
            Err(_) => panic!("an end that fits two is Which"),
        }
        let said = NotOne::Which(vec![0, 1]).said("hold", "held", &twins, named_hold);
        assert!(said.contains("names all of the 2 held here"), "{said}");

        // Both named at once: not an id, so nothing is picked out of it.
        assert_eq!(
            picks(&twins, Some("hld_aaaa1111 and hld_bbbb1111")),
            "<refused>"
        );
    }

    /// **Nothing waiting and a name that matches nothing are told apart**,
    /// over the question vocabulary — the same rule, the same code, a
    /// correlation id instead of a hold id.
    #[test]
    fn nothing_waiting_and_a_name_that_matches_nothing_are_told_apart() {
        let none: [ConfirmRequest; 0] = [];
        let said = match one_of(&none, Some("cor_aaaa1111"), |q| q.correlation_id.clone()) {
            Err(NotOne::None) => NotOne::None.said("question", "waiting", &none, named_question),
            Ok(q) => panic!("nothing waits, yet {} was picked", q.correlation_id),
            Err(_) => panic!("an empty list is None, whatever was named"),
        };
        assert!(
            said.contains("Nothing is waiting in this conversation"),
            "{said}"
        );

        let two = [
            a_question("cor_aaaa1111", "task.close"),
            a_question("cor_bbbb2222", "proc.run"),
        ];
        let pick = |n: &str| {
            one_of(&two, Some(n), |q| q.correlation_id.clone())
                .map(|q| q.correlation_id.clone())
                .unwrap_or_else(|_| "<refused>".into())
        };
        assert_eq!(pick("cor_bbbb2222"), "cor_bbbb2222", "the whole id");
        assert_eq!(pick("2222"), "cor_bbbb2222", "an end of four");
        assert_eq!(pick("task.close"), "<refused>", "a tool is not an id");
        let said = match one_of(&two, Some("cor_cccc3333"), |q| q.correlation_id.clone()) {
            Err(NotOne::Unknown(w)) => {
                NotOne::Unknown(w).said("question", "waiting", &two, named_question)
            }
            Ok(q) => panic!("{} was picked by a name nothing has", q.correlation_id),
            Err(_) => panic!("a name nothing has is Unknown, not None"),
        };
        assert!(
            said.contains("`cor_cccc3333` names nothing waiting here"),
            "{said}"
        );
        assert!(said.contains("`task.close`"), "both are listed: {said}");
        assert!(said.contains("`proc.run`"), "{said}");
    }
}
