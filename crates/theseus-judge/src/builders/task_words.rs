//! `task_words.v1` (issue 44, decision 6): one message of the operator's,
//! whole, and the one task it addresses as it stands now. One choice: does
//! the message, read alone, restrict the task, pause it, resume it, or ask
//! something else of it?
//!
//! The state is deliberately one-sided. It carries his words and the task,
//! and **nothing of the acting model's proposal**: not the kind it declared,
//! not the objective it wrote. The question is what his message asks, so the
//! model that proposed the change must not be able to argue the judge into it.

use super::*;

/// The message, clipped: past this it is not one instruction about one task.
const MESSAGE_CHARS: usize = 2_000;
/// The task's objective, and one line of its acceptance.
const OBJECTIVE_CHARS: usize = 1_500;
const LINE_CHARS: usize = 300;
/// The most acceptance lines the state names.
pub const LINES: usize = 12;

/// `task_words.v1`'s input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskWordsInput {
    /// The operator's message, whole.
    pub message: String,
    /// The task it addresses: its title, objective, acceptance and state now.
    pub title: String,
    pub objective: String,
    #[serde(default)]
    pub acceptance: Vec<String>,
    /// `accepted`, `running`, `suspended`, ….
    pub state: String,
}

impl TaskWordsInput {
    /// Whether the state would hold less than this input: the message, the
    /// title, the objective or an acceptance line past its length, or more
    /// acceptance lines than it names. A judgment of a clipped input cannot
    /// authorise the whole message (issue 44).
    pub fn clipped(&self) -> bool {
        let over = |s: &str, n: usize| s.trim().chars().count() > n;
        over(&self.message, MESSAGE_CHARS)
            || over(&self.title, LINE_CHARS)
            || over(&self.objective, OBJECTIVE_CHARS)
            || over(&self.state, 32)
            || self.acceptance.len() > LINES
            || self.acceptance.iter().any(|l| over(l, LINE_CHARS))
    }
}

pub fn task_words(i: &TaskWordsInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let mut b = StateBuilder::new("task_words", TASK_WORDS_VERSION, cap, scrub);
    b.scalar("message", c.clip(i.message.trim(), MESSAGE_CHARS))
        .cut_if("message", c.cut());
    b.scalar("task_title", c.clip(i.title.trim(), LINE_CHARS));
    b.scalar(
        "task_objective",
        c.clip(i.objective.trim(), OBJECTIVE_CHARS),
    )
    .cut_if("task_objective", c.cut());
    b.scalar("task_state", c.clip(i.state.trim(), 32));
    let kept: Vec<&String> = i.acceptance.iter().take(LINES).collect();
    let lines: Vec<Value> = kept
        .iter()
        .map(|s| Value::String(c.clip(s.trim(), LINE_CHARS)))
        .collect();
    b.list_after(
        "task_acceptance",
        10,
        share(cap, 30),
        lines,
        i.acceptance.len().saturating_sub(kept.len()),
    )
    .cut_if("task_acceptance", c.cut());
    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::by_name;
    use crate::state::NoScrub;

    fn input() -> TaskWordsInput {
        TaskWordsInput {
            message: "make it staging only".into(),
            title: "Deploy".into(),
            objective: "Deploy the service to staging and production.".into(),
            acceptance: vec!["Both answer on /health.".into()],
            state: "running".into(),
        }
    }

    /// The state holds his words and the task, and the pack asks its one
    /// choice with the four options.
    #[test]
    fn the_state_holds_the_message_and_the_task_and_one_choice_is_asked() {
        let p = by_name("task_words.v1").unwrap();
        let prepared = prepare(&p, &Input::TaskWords(input()), &NoScrub).unwrap();
        let v = prepared.state.value();
        assert_eq!(v["message"], "make it staging only");
        assert_eq!(v["task_title"], "Deploy");
        assert_eq!(v["task_state"], "running");
        assert!(v["task_acceptance"].to_string().contains("/health"), "{v}");
        let asked = p.ask(&prepared.dynamic);
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].id, "change");
        let crate::client::Question::Choice { options, .. } = &asked[0].question else {
            panic!("a choice");
        };
        let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["restricts", "pauses", "resumes", "other"]);
    }

    /// A long message that narrows at its start and adds work past the cut,
    /// and a task whose acceptance the state cannot name whole, are clipped;
    /// the plain input is not.
    #[test]
    fn a_long_message_or_an_omitted_acceptance_is_clipped() {
        assert!(!input().clipped());
        let mut long = input();
        long.message = format!(
            "make it staging only. {} Also deploy the new billing service to production.",
            "Context. ".repeat(300)
        );
        assert!(long.clipped());
        let p = by_name("task_words.v1").unwrap();
        let prepared = prepare(&p, &Input::TaskWords(long), &NoScrub).unwrap();
        assert!(
            !prepared.state.value()["message"]
                .to_string()
                .contains("billing"),
            "the widening is past what the judge reads"
        );
        let mut many = input();
        many.acceptance = (0..LINES + 1).map(|n| format!("check {n}")).collect();
        many.acceptance.push("Production is never touched.".into());
        assert!(many.clipped());
    }

    /// The input has no room for a proposal (a declared kind or a new
    /// objective is refused), and it round-trips through JSON.
    #[test]
    fn nothing_of_a_proposal_fits_the_input_and_the_input_round_trips() {
        let i = input();
        let mut with_kind = serde_json::to_value(&i).unwrap();
        with_kind["kind"] = "narrows".into();
        assert!(serde_json::from_value::<TaskWordsInput>(with_kind).is_err());
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(
            Input::parse(Builder::TaskWords, &json).unwrap(),
            Input::TaskWords(i)
        );
    }
}
