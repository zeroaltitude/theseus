
/// The six interpreted kinds of the design, each assigned by the origins
/// that may file it.
fn interpreted_seeds() -> Vec<Kind> {
    use Origin::{Dream, Import, Jev, Operator, Sweep};
    let interpreted =
        |name, assigned_by, per, shape| seed(name, Basis::Interpreted, assigned_by, per, shape);
    let mined = |mut k: Kind, mined_from: &[MinedFrom]| {
        k.mined_from = mined_from.to_vec();
        k
    };
    let by_three = [Operator, Jev, Sweep];
    vec![
        mined(
            interpreted(
                "topic",
                &[Operator, Import, Jev, Sweep, Dream],
                PerSession::AtMost(3),
                (
                    Some("topic"),
                    40,
                    Rule::Chain,
                    "What a session is about, declared by the operator, or by the import from an \
                     imported session's labels. Topics nest.",
                ),
            ),
            &[MinedFrom::Titles, MinedFrom::Capitalised],
        ),
        interpreted(
            "event",
            &by_three,
            PerSession::AtMost(3),
            (
                Some("project"),
                50,
                Rule::Chain,
                "A dated happening (a release, an outage, a meeting, a deadline); the date is mined \
                 in code, never Jev's.",
            ),
        ),
        mined(
            interpreted(
                "resource",
                &by_three,
                PerSession::Many,
                (
                    None,
                    60,
                    Rule::IntentLine,
                    "A repository, service, host, document or dataset the session works with.",
                ),
            ),
            &[MinedFrom::Paths, MinedFrom::Hosts],
        ),
        interpreted(
            "capability",
            &by_three,
            PerSession::Many,
            (
                None,
                70,
                Rule::IntentLine,
                "A tool, skill or extension the session needed.",
            ),
        ),
        interpreted(
            "expertise",
            &by_three,
            PerSession::AtMost(3),
            (
                Some("expertise"),
                80,
                Rule::Chain,
                "The domain the session draws on.",
            ),
        ),
        interpreted(
            "culture",
            &[Operator, Dream],
            PerSession::Many,
            (
                None,
                90,
                Rule::Chain,
                "How things are done here, learned from corrections over time.",
            ),
        ),
        interpreted(
            crate::persona::KIND,
            &[Operator, Import],
            PerSession::AtMost(1),
            (
                None,
                100,
                Rule::IntentLine,
                "A persona: who answers a message. Its soul is a file; Jev picks one per message. \
                 Only the owner writes one.",
            ),
        ),
    ]
}
