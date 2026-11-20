//! The "should have asked" controls under a tool message: one small grey
//! button for each tool that can be tightened, `🔔 <tool>`, and the pull-down
//! (`asked_menu`) only past five, which is the compact way to offer many. A
//! button's id is the notice card's, `tighten:<tool>|<call id>`, so a press goes
//! the one way a pick and a card's button always did (`should_have_asked`, which
//! sends `policy.tighten` as the presser); a pull-down an older build posted is
//! still answered. A message with a tool name too long for a button's id keeps
//! the pull-down, which carries a tool up to 100 characters. A child of
//! `runtime`, apart from it for the file ceiling.

use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};

use super::{asked_menu, asked_value, ASKED_MENU, ID_LIMIT};
use crate::render::Asked;

/// Discord's most buttons in a row, and so the most tools offered as buttons.
const MAX_BUTTONS: usize = 5;
/// Discord's limit on a button's label, in characters.
const LABEL_LIMIT: usize = 80;

/// A button's custom id: the notice card's, `tighten:<tool>|<call id>`.
fn button_id(a: &Asked) -> String {
    format!("{ASKED_MENU}:{}", asked_value(a))
}

/// One tool's button: a mark and the tool's name, clipped to the label's limit.
/// A name too long for its room keeps its end (the tool of `mcp:<server>/<tool>`),
/// with the `…` in front, so two tools of one long-named server read apart.
fn button(a: &Asked) -> Component {
    let room = LABEL_LIMIT - "🔔 ".chars().count();
    let len = a.tool.chars().count();
    let tool = match len > room {
        true => format!(
            "…{}",
            a.tool.chars().skip(len - (room - 1)).collect::<String>()
        ),
        false => a.tool.clone(),
    };
    Component::Button(Button {
        id: None,
        custom_id: Some(button_id(a)),
        disabled: false,
        emoji: None,
        label: Some(format!("🔔 {tool}")),
        style: ButtonStyle::Secondary,
        url: None,
        sku_id: None,
    })
}

/// A notice card's button (with `[discord] notice_embeds`), or none once its
/// tool asks first.
pub(crate) fn asked_button(ask: Option<&Asked>) -> Vec<Component> {
    let Some(a) = ask else {
        return vec![];
    };
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: vec![button(a)],
    })]
}

/// A tool message's controls: a row of buttons for one to five tools, the
/// pull-down for more, nothing for none. A button's id counts in characters, as
/// Discord's validator does: one past `ID_LIMIT` is a pull-down for all.
pub(crate) fn asked_controls(options: &[Asked]) -> Vec<Component> {
    let id_too_long = options
        .iter()
        .any(|a| button_id(a).chars().count() > ID_LIMIT);
    match options.len() {
        0 => vec![],
        n if n <= MAX_BUTTONS && !id_too_long => vec![Component::ActionRow(ActionRow {
            id: None,
            components: options.iter().map(button).collect(),
        })],
        _ => asked_menu(options),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::parse_asked_pick;

    fn asked(tool: &str, call: Option<&str>) -> Asked {
        Asked {
            tool: tool.into(),
            correlation_id: call.map(String::from),
        }
    }

    /// `n` tools, each with its newest call.
    fn tools(n: usize) -> Vec<Asked> {
        [
            "proc.run",
            "fs.write",
            "git.commit",
            "web.fetch",
            "aws.call",
            "mcp.tool",
        ]
        .iter()
        .take(n)
        .enumerate()
        .map(|(i, t)| asked(t, Some(&format!("act_{i:03}"))))
        .collect()
    }

    /// The controls of a tool message with one, three, five and six tools, as
    /// Discord is sent them: the first three and the fifth are one row of
    /// small grey buttons, the sixth the pull-down. A change to what a person
    /// sees is made with `THESEUS_GOLDEN=write`.
    #[test]
    fn a_tool_messages_controls_golden() {
        let mut out = String::new();
        for n in [1, 3, 5, 6] {
            out.push_str(&format!(
                "=== {n} tool{}\n{}\n\n",
                if n == 1 { "" } else { "s" },
                serde_json::to_string_pretty(&asked_controls(&tools(n))).unwrap()
            ));
        }
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/runtime/golden/asked_controls.txt"
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

    /// One to five tools are one row of grey buttons, `🔔 <tool>`, on the
    /// notice card's id; six are the pull-down; none is nothing (planted:
    /// buttons for six, or an id that is not `tighten:`).
    #[test]
    fn one_to_five_tools_are_small_grey_buttons_and_six_are_the_pull_down() {
        for n in 1..=5 {
            let options = tools(n);
            let rows = asked_controls(&options);
            assert_eq!(rows.len(), 1, "{n}: one row");
            let Component::ActionRow(row) = &rows[0] else {
                panic!()
            };
            assert_eq!(row.components.len(), n);
            for (c, a) in row.components.iter().zip(&options) {
                let Component::Button(b) = c else {
                    panic!("{n}: {c:?}")
                };
                assert_eq!(b.style, ButtonStyle::Secondary);
                assert_eq!(b.label.as_deref(), Some(format!("🔔 {}", a.tool).as_str()));
                assert_eq!(
                    b.custom_id.as_deref(),
                    Some(
                        format!("tighten:{}|{}", a.tool, a.correlation_id.as_ref().unwrap())
                            .as_str()
                    )
                );
                // The press is a pick's: the same tool and the same call.
                assert_eq!(
                    parse_asked_pick(b.custom_id.as_deref().unwrap(), &[]).as_ref(),
                    Some(a)
                );
            }
        }
        let rows = asked_controls(&tools(6));
        let Component::ActionRow(row) = &rows[0] else {
            panic!()
        };
        assert!(
            matches!(&row.components[..], [Component::SelectMenu(m)] if m.custom_id == "tighten"),
            "six tools: the pull-down, as before: {rows:?}"
        );
        assert!(asked_controls(&[]).is_empty());
        // One tool and the notice card's twin say the same thing.
        let one = tools(1);
        assert_eq!(asked_controls(&one), asked_button(Some(&one[0])));
        assert!(asked_button(None).is_empty());
    }

    /// A long tool name is clipped inside Discord's 80 characters, keeping its
    /// end behind a `…`, and its id stays inside 100 (the tool alone past that,
    /// as for a pick).
    #[test]
    fn a_long_tool_name_is_clipped_to_the_label_limit() {
        let long = asked(&format!("mcp:{}/tool", "s".repeat(82)), Some("act_001"));
        let rows = asked_controls(std::slice::from_ref(&long));
        let Component::ActionRow(row) = &rows[0] else {
            panic!()
        };
        let Component::Button(b) = &row.components[0] else {
            panic!()
        };
        let label = b.label.as_deref().unwrap();
        assert_eq!(label.chars().count(), LABEL_LIMIT);
        assert!(
            label.starts_with("🔔 …") && label.ends_with("sss/tool"),
            "{label}"
        );
        assert!(b.custom_id.as_deref().unwrap().len() <= 100);
        assert_eq!(
            parse_asked_pick(b.custom_id.as_deref().unwrap(), &[])
                .unwrap()
                .tool,
            long.tool
        );
    }

    fn label_of(c: &Component) -> String {
        let Component::Button(b) = c else { panic!() };
        b.label.clone().unwrap()
    }

    /// Two tools of one long-named server do not read the same: the label
    /// keeps the end of the name (planted: keep the start again).
    #[test]
    fn two_long_names_of_one_server_have_different_labels() {
        let server = "s".repeat(70);
        let options = [
            asked(&format!("mcp:{server}/read"), Some("act_001")),
            asked(&format!("mcp:{server}/write"), Some("act_002")),
        ];
        let rows = asked_controls(&options);
        let Component::ActionRow(row) = &rows[0] else {
            panic!("{rows:?}")
        };
        let labels: Vec<String> = row.components.iter().map(label_of).collect();
        assert_ne!(labels[0], labels[1]);
        assert!(labels[0].ends_with("/read") && labels[1].ends_with("/write"));
        assert!(labels.iter().all(|l| l.chars().count() <= LABEL_LIMIT));
    }

    /// A button's id is `tighten:` and the tool, 8 characters more than the
    /// tool, once the call id no longer fits: a tool of 92 characters is an id
    /// of exactly 100, a button; 93 is 101, so the message keeps the pull-down.
    /// And one long name among short ones makes the pull-down for all
    /// (planted: take the condition out).
    #[test]
    fn a_tool_name_too_long_for_a_buttons_id_keeps_the_pull_down() {
        let is_menu = |rows: &[Component]| {
            matches!(rows, [Component::ActionRow(r)]
                if matches!(&r.components[..], [Component::SelectMenu(_)]))
        };
        let edge = ID_LIMIT - format!("{ASKED_MENU}:").chars().count();
        let fits = asked(&"t".repeat(edge), Some("act_001"));
        let rows = asked_controls(std::slice::from_ref(&fits));
        let Component::ActionRow(row) = &rows[0] else {
            panic!("{rows:?}")
        };
        let Component::Button(b) = &row.components[0] else {
            panic!("the edge is a button: {rows:?}")
        };
        assert_eq!(b.custom_id.as_deref().unwrap().chars().count(), ID_LIMIT);

        let over = asked(&"t".repeat(edge + 1), Some("act_001"));
        assert!(is_menu(&asked_controls(std::slice::from_ref(&over))));
        let long = asked(&"t".repeat(95), Some("act_001"));
        assert!(is_menu(&asked_controls(std::slice::from_ref(&long))));

        let mut mixed = tools(2);
        mixed.insert(1, over);
        let rows = asked_controls(&mixed);
        assert!(is_menu(&rows), "a long name among short ones: {rows:?}");
        let Component::ActionRow(row) = &rows[0] else {
            panic!()
        };
        let Component::SelectMenu(m) = &row.components[0] else {
            panic!()
        };
        assert_eq!(m.options.as_ref().map(Vec::len), Some(3));
    }
}
