//! Why a held shell command is held, in words that say which program and where
//! the look is (issue 26).
//!
//! A turn asked whether a model's key was in the vault ran the vault's program
//! in a shell, and the card said only that the call "touches Theseus's own
//! state or a secret". The owner could not tell a turn that was lost from one
//! that needed him. The floor's reason names what it found, and this reads it:
//! the program, or the path, and what Theseus has in its place. The floor's
//! decision is not touched (`policy.rs` says what is held; this says it).
//!
//! The reason is the floor's own sentence (`policy::floor`), so a test plays
//! each real one through the gate (`config_change::tests_look`) and a change to
//! the sentence fails it.

/// The vault's program, as the floor names it.
const VAULT_PROGRAM: &str = "op";
/// Theseus's own program.
const OWN_PROGRAM: &str = "theseusd";
const LOOKS: &str = "To look, Theseus has tools, and a look is not a question for you";

/// The clause of a floor hold: what is between `(floor: ` and the `)` that
/// closes the reason, or `None` for a reason that is not the floor's.
fn clause(reason: &str) -> Option<&str> {
    let at = reason.rfind("(floor: ")?;
    reason[at + "(floor: ".len()..].trim_end().strip_suffix(')')
}

/// The words for a floor hold: which program or path, and what to use to look.
/// The general words when the clause is none of the three the floor says.
pub(super) fn words(reason: &str) -> String {
    let general = "it touches Theseus's own state or a secret, which always asks".to_string();
    let Some(c) = clause(reason) else {
        return general;
    };
    if let Some(program) = c
        .strip_prefix('`')
        .and_then(|r| r.split_once("` is Theseus's own binary"))
        .map(|(p, _)| p)
    {
        return match program {
            VAULT_PROGRAM => format!(
                "it names the vault's program (`{program}`), which always asks, and a shell has no \
                 vault session by design. {LOOKS}: `look.vault` lists what the vault holds by name"
            ),
            OWN_PROGRAM => format!(
                "it names Theseus's own program (`{program}`), which always asks. {LOOKS}: \
                 `look.theseus` shows its models, keys, health and build, and `config.show` its settings"
            ),
            other => format!(
                "it names a program Theseus protects (`{other}`), which always asks"
            ),
        };
    }
    if c.starts_with("argument `") {
        return format!(
            "it names Theseus's own state or its settings file, which always asks. {LOOKS}: \
             `config.show` reads a setting and `look.theseus` shows Theseus itself"
        );
    }
    general
}

#[cfg(test)]
mod tests {
    use super::*;

    const OP: &str = "run `bash -c op whoami` in /w: proc.run — approve (floor: `op` is Theseus's own binary or the 1Password CLI)";
    const OWN: &str = "run `theseusd` in /w: proc.run — approve (floor: `theseusd` is Theseus's own binary or the 1Password CLI)";
    const ARG: &str = "run `ls /etc/x` in /w: proc.run — approve (floor: argument `/etc/x` is Theseus's own state or the 1Password token)";

    /// Each floor sentence names its program, or says it is the state, and says
    /// what to use instead; no path or command text is repeated.
    #[test]
    fn each_floor_hold_says_which_and_where_the_look_is() {
        let op = words(OP);
        assert!(
            op.contains("the vault's program (`op`)") && op.contains("`look.vault`"),
            "{op}"
        );
        assert!(op.contains("no vault session by design"), "{op}");
        let own = words(OWN);
        assert!(
            own.contains("(`theseusd`)") && own.contains("`look.theseus`"),
            "{own}"
        );
        let arg = words(ARG);
        assert!(
            arg.contains("`config.show`") && arg.contains("`look.theseus`"),
            "{arg}"
        );
        assert!(!arg.contains("/etc/x"), "{arg}");
    }

    /// A hold the floor made for another reason, or a reason that is not a
    /// floor's, keeps the general words.
    #[test]
    fn any_other_reason_keeps_the_general_words() {
        let general = "it touches Theseus's own state or a secret, which always asks";
        assert_eq!(
            words("x: aws.stack.apply — approve (floor: the budget: replaces a thing)"),
            general
        );
        assert_eq!(words("x: fs.grep — approve (floor: /s/store is Theseus's own state or the 1Password token)"), general);
        assert_eq!(words("anything else"), general);
        assert!(
            words("x (floor: `other` is Theseus's own binary or the 1Password CLI)")
                .contains("`other`")
        );
    }
}
