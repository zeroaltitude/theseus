//! The router's question in a terminal (theseus-rha2): "Is this about the pr
//! round or the security fix?" with `[1] [2] [n]ew [h]ome`. A turn that asks
//! waits in the daemon for the answer, so an `ask` that sees the question
//! says it on stderr and, on a terminal, reads the answer from stdin and
//! sends `thread.answer` on a connection of its own (the turn's is busy
//! until the turn ends). Anywhere else (a pipe, `--spawn`) it names the
//! command that answers.

use std::io::IsTerminal;

use serde_json::Value;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::thread_route::{ThreadAnswerParams, ThreadAnswerResult, ThreadAsk};

/// The question as a terminal shows it.
pub fn lines(a: &ThreadAsk) -> Vec<String> {
    let mut out = vec![format!("\u{2753} {}", a.question)];
    if !a.message.is_empty() {
        out.push(format!("   > {}", a.message));
    }
    let opts: Vec<String> = a
        .options
        .iter()
        .map(|o| match o.key.as_str() {
            "new" => format!("[n]ew {}", o.label),
            "home" => "[h]ome".to_string(),
            k => format!("[{k}] {}", o.label),
        })
        .collect();
    out.push(format!("   {}", opts.join("   ")));
    out
}

/// The command that answers, for a terminal that cannot ask.
pub fn command(a: &ThreadAsk) -> String {
    let keys: Vec<&str> = a.options.iter().map(|o| o.key.as_str()).collect();
    format!("theseus thread answer {} {}", a.ask_id, keys.join("|"))
}

/// An option's key from what was typed: its key, `n` or `h`, or a label.
pub fn choice_of(a: &ThreadAsk, typed: &str) -> Option<String> {
    let t = typed.trim().to_lowercase();
    let t = match t.as_str() {
        "n" => "new",
        "h" => "home",
        other => other,
    };
    a.options
        .iter()
        .find(|o| o.key == t || o.label.to_lowercase() == t)
        .map(|o| o.key.clone())
}

/// A `thread.asked` notification, seen while a turn runs. Said on stderr;
/// on a terminal the answer is read and sent beside the turn.
pub(crate) fn on_asked(p: &Value) {
    let Ok(a) = serde_json::from_value::<ThreadAsk>(p.clone()) else {
        return;
    };
    for l in lines(&a) {
        eprintln!("{l}");
    }
    let socket =
        std::env::var("THESEUS_SOCKET").unwrap_or_else(|_| "~/.theseus/theseus.sock".into());
    if !std::io::stdin().is_terminal() || tokio::runtime::Handle::try_current().is_err() {
        eprintln!("   answer: {}", command(&a));
        return;
    }
    tokio::spawn(async move {
        eprint!("   answer: ");
        let typed = tokio::task::spawn_blocking(|| {
            let mut s = String::new();
            std::io::stdin().read_line(&mut s).map(|_| s)
        })
        .await;
        let Ok(Ok(typed)) = typed else { return };
        let Some(choice) = choice_of(&a, &typed) else {
            eprintln!("   not an option: `{}`; {}", typed.trim(), command(&a));
            return;
        };
        let sent = async {
            let mut c = Conn::socket(&socket).await?;
            let p = ThreadAnswerParams {
                ask_id: a.ask_id.clone(),
                choice,
                author: None,
                discord: None,
            };
            let v = c
                .request(method::THREAD_ANSWER, serde_json::to_value(&p)?)
                .await?;
            anyhow::Ok(serde_json::from_value::<ThreadAnswerResult>(v)?)
        }
        .await;
        if let Err(e) = sent {
            eprintln!("   the answer was not sent: {e:#}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::thread_route::AskOption;

    fn ask() -> ThreadAsk {
        ThreadAsk {
            ask_id: "ask_7".into(),
            session_id: "ses_a".into(),
            place: None,
            question: "Is this about the pr round or the security fix?".into(),
            options: vec![
                AskOption {
                    key: "1".into(),
                    label: "pr round".into(),
                },
                AskOption {
                    key: "2".into(),
                    label: "security fix".into(),
                },
                AskOption {
                    key: "home".into(),
                    label: "home".into(),
                },
            ],
            message: "why is the build red".into(),
            expires_ms: 1,
        }
    }

    /// The question is one line, the waiting message, and the keys.
    #[test]
    fn the_question_shows_its_keys_and_names_the_command() {
        let l = lines(&ask());
        assert_eq!(
            l[0],
            "\u{2753} Is this about the pr round or the security fix?"
        );
        assert_eq!(l[1], "   > why is the build red");
        assert_eq!(l[2], "   [1] pr round   [2] security fix   [h]ome");
        assert_eq!(command(&ask()), "theseus thread answer ask_7 1|2|home");
    }

    #[test]
    fn what_is_typed_names_an_option() {
        let a = ask();
        for (typed, want) in [
            ("1", Some("1")),
            (" 2\n", Some("2")),
            ("h", Some("home")),
            ("home", Some("home")),
            ("Security Fix", Some("2")),
            ("3", None),
            ("n", None),
            ("", None),
        ] {
            assert_eq!(choice_of(&a, typed).as_deref(), want, "{typed:?}");
        }
    }
}
